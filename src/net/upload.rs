//! Envoi d'un message avec pièces jointes : Create Message en
//! `multipart/form-data` (documentation de Discord, « Uploading Files »,
//! https://discord.com/developers/docs/reference#uploading-files) :
//! une partie `payload_json` (corps JSON habituel et `attachments`, qui
//! relie chaque `files[n]` à son nom et à sa description par `id` = n),
//! puis une partie `files[n]` par fichier.
//!
//! Le corps est produit en flux par un fil bloquant (`spawn_blocking`) :
//! un fichier n'est jamais lu en entier en mémoire, et rien ne touche le
//! disque sur le fil d'interface. Les octets remis à la connexion sont
//! comptés pour la progression. Module enfant de `rest` : même limiteur,
//! mêmes erreurs que les autres requêtes.
//!
//! L'envoi en deux temps (`POST /channels/{id}/attachments`, puis
//! téléversement vers l'adresse rendue) n'est pas employé : sa forme n'a pas
//! pu être vérifiée sur une source publique pour ce lot.

use super::{api_req, ApiError, Ctx};
use crate::state::social::SendKind;
use crate::state::upload::{Failure, OutFile, Source, UploadEvent};
use crate::state::{Event, Problem};
use bytes::Bytes;
use serde_json::json;
use std::{
    collections::HashMap,
    io::Read,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, PoisonError,
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::sync::{mpsc, watch};

/// Morceaux lus et envoyés à la fois.
const CHUNK: usize = 64 << 10;
/// Morceaux prêts d'avance, au plus (mémoire bornée : 4 × 64 Kio).
const AHEAD: usize = 4;
/// Écart minimal entre deux annonces de progression.
const PROGRESS_EVERY: Duration = Duration::from_millis(200);
/// Attente maximale dans le limiteur avant de partir.
const MAX_WAIT: Duration = Duration::from_secs(30);

/// Envoi à faire, tel que la file du salon le reçoit.
pub(super) struct Job {
    pub channel_id: u64,
    pub nonce: u64,
    pub content: String,
    pub files: Vec<OutFile>,
    pub kind: Option<SendKind>,
    pub cancel: watch::Receiver<bool>,
}

/// Annulations des envois en file ou en cours, par nonce.
#[derive(Clone, Default)]
pub(super) struct Cancels(Arc<Mutex<HashMap<u64, watch::Sender<bool>>>>);

impl Cancels {
    /// Inscrit un envoi ; un nouvel essai du même nonce remplace l'ancien.
    pub(super) fn register(&self, nonce: u64) -> watch::Receiver<bool> {
        let (tx, rx) = watch::channel(false);
        self.0.lock().unwrap_or_else(PoisonError::into_inner).insert(nonce, tx);
        rx
    }

    pub(super) fn cancel(&self, nonce: u64) {
        if let Some(tx) = self.0.lock().unwrap_or_else(PoisonError::into_inner).get(&nonce) {
            tx.send_replace(true);
        }
    }

    pub(super) fn forget(&self, nonce: u64) {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).remove(&nonce);
    }
}

/// Partie du corps.
#[derive(Clone)]
enum Part {
    Bytes(Bytes),
    File { path: std::path::PathBuf, size: u64, name: String },
}

impl Part {
    fn len(&self) -> u64 {
        match self {
            Part::Bytes(b) => b.len() as u64,
            Part::File { size, .. } => *size,
        }
    }
}

/// Séparateur des parties : assez long pour ne jamais se trouver dans un fichier.
fn boundary(nonce: u64) -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("ferrocord-{nonce:016x}-{t:032x}")
}

/// Nom de fichier entre guillemets dans `Content-Disposition` : guillemets
/// et fins de ligne encodés comme le font les navigateurs (WHATWG).
fn quoted(name: &str) -> String {
    name.replace('"', "%22").replace('\r', "%0D").replace('\n', "%0A")
}

/// Type d'après l'extension : Discord se fie au nom, ceci n'est qu'indicatif.
fn content_type(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "txt" | "log" | "md" => "text/plain",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

/// `payload_json` : le corps de Create Message (nonce imposé, réponse) et
/// la liste `attachments`.
fn payload(content: &str, nonce: u64, kind: Option<&SendKind>, files: &[OutFile]) -> serde_json::Value {
    let mut body = super::super::social::send_body(super::send_body(content, nonce), kind);
    let attachments: Vec<serde_json::Value> = files
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let mut a = json!({ "id": i, "filename": f.sent_name() });
            let d = f.description.trim();
            if !d.is_empty() {
                a["description"] = json!(d.chars().take(crate::state::upload::DESCRIPTION_MAX).collect::<String>());
            }
            if let Some(SendKind::Voice { duration_ms, waveform }) = kind {
                a["duration_secs"] = json!(f64::from(*duration_ms) / 1000.0);
                a["waveform"] = json!(waveform);
            }
            a
        })
        .collect();
    body["attachments"] = json!(attachments);
    // Un message fait seulement de fichiers n'a pas de texte.
    if content.is_empty()
        && let Some(o) = body.as_object_mut()
    {
        o.remove("content");
    }
    body
}

/// Corps `multipart/form-data` entier en mémoire (fichiers déjà lus) : pour
/// une interaction avec des options pièce jointe (`POST /interactions`,
/// docs.discord.food, « Create Interaction ») ; les fichiers d'une commande
/// sont petits et peu nombreux. Renvoie le type (avec sa frontière) et le corps.
pub(in crate::net) fn multipart_in_memory(seed: u64, payload: &serde_json::Value, files: &[(String, Vec<u8>)]) -> (String, Vec<u8>) {
    let b = boundary(seed);
    let mut out = format!("--{b}\r\nContent-Disposition: form-data; name=\"payload_json\"\r\nContent-Type: application/json\r\n\r\n{payload}\r\n").into_bytes();
    for (i, (name, bytes)) in files.iter().enumerate() {
        out.extend_from_slice(format!("--{b}\r\nContent-Disposition: form-data; name=\"files[{i}]\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n", quoted(name), content_type(name)).as_bytes());
        out.extend_from_slice(bytes);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{b}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={b}"), out)
}

/// Parties du corps multipart, dans l'ordre.
fn parts(boundary: &str, payload: &serde_json::Value, files: &[OutFile]) -> Vec<Part> {
    let mut out = vec![Part::Bytes(Bytes::from(format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"payload_json\"\r\nContent-Type: application/json\r\n\r\n{payload}\r\n"
    )))];
    for (i, f) in files.iter().enumerate() {
        let name = f.sent_name();
        out.push(Part::Bytes(Bytes::from(format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"files[{i}]\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",
            quoted(&name),
            content_type(&name)
        ))));
        out.push(match &f.source {
            Source::Memory(b) => Part::Bytes(Bytes::from_owner(b.clone())),
            Source::Path(p) => Part::File { path: p.clone(), size: f.size, name: f.name.clone() },
        });
        out.push(Part::Bytes(Bytes::from_static(b"\r\n")));
    }
    out.push(Part::Bytes(Bytes::from(format!("--{boundary}--\r\n"))));
    out
}

/// Corps en flux : reçoit les morceaux du fil de lecture et compte ce qui
/// est remis à la connexion.
struct Streamed {
    rx: mpsc::Receiver<Result<Bytes, std::io::Error>>,
    sent: Arc<AtomicU64>,
    total: u64,
}

impl http_body::Body for Streamed {
    type Data = Bytes;
    type Error = std::io::Error;

    fn poll_frame(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<http_body::Frame<Bytes>, std::io::Error>>> {
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(Ok(b))) => {
                self.sent.fetch_add(b.len() as u64, Ordering::Relaxed);
                Poll::Ready(Some(Ok(http_body::Frame::data(b))))
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn size_hint(&self) -> http_body::SizeHint {
        http_body::SizeHint::with_exact(self.total)
    }
}

/// Lit les parties dans l'ordre (fil bloquant) ; un fichier illisible ou
/// modifié arrête le corps et laisse sa raison dans `failed`.
fn produce(parts: Vec<Part>, tx: mpsc::Sender<Result<Bytes, std::io::Error>>, failed: Arc<Mutex<Option<Failure>>>) {
    let fail = |f: Failure| {
        *failed.lock().unwrap_or_else(PoisonError::into_inner) = Some(f);
        let _ = tx.blocking_send(Err(std::io::Error::other("pièce jointe illisible")));
    };
    for part in parts {
        match part {
            Part::Bytes(b) => {
                for start in (0..b.len()).step_by(CHUNK) {
                    if tx.blocking_send(Ok(b.slice(start..(start + CHUNK).min(b.len())))).is_err() {
                        return;
                    }
                }
            }
            Part::File { path, size, name } => {
                let Ok(mut file) = std::fs::File::open(&path) else { return fail(Failure::Unreadable(name)) };
                let mut left = size;
                let mut buf = vec![0u8; CHUNK];
                while left > 0 {
                    let want = usize::try_from(left).unwrap_or(CHUNK).min(CHUNK);
                    match file.read(&mut buf[..want]) {
                        Ok(0) => return fail(Failure::Changed(name)),
                        Ok(n) => {
                            left -= n as u64;
                            if tx.blocking_send(Ok(Bytes::copy_from_slice(&buf[..n]))).is_err() {
                                return;
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(_) => return fail(Failure::Unreadable(name)),
                    }
                }
                // Plus long qu'à l'ajout : modifié entre-temps.
                if file.read(&mut buf[..1]).is_ok_and(|n| n > 0) {
                    return fail(Failure::Changed(name));
                }
            }
        }
    }
}

/// Taille de chaque fichier du disque vérifiée juste avant l'envoi.
fn check_files(files: &[OutFile]) -> Result<(), Failure> {
    for f in files {
        if let Source::Path(p) = &f.source {
            match std::fs::metadata(p) {
                Ok(m) if m.is_file() && m.len() == f.size => {}
                Ok(_) => return Err(Failure::Changed(f.name.clone())),
                Err(_) => return Err(Failure::Unreadable(f.name.clone())),
            }
        }
    }
    Ok(())
}

/// Délai total accordé à la requête : 60 s, plus le temps d'envoyer le corps
/// à 64 Kio/s (connexion lente), au plus 6 h. L'utilisateur peut annuler avant.
fn timeout_for(total: u64) -> Duration {
    Duration::from_secs(60 + (total >> 16)).min(Duration::from_secs(6 * 3600))
}

async fn cancelled(mut rx: watch::Receiver<bool>) {
    if rx.wait_for(|c| *c).await.is_err() {
        std::future::pending::<()>().await;
    }
}

/// Envoie le message et annonce progression et issue.
/// L'annulation est oubliée avant que l'issue soit annoncée : un nouvel
/// essai (même nonce), demandé en réponse, inscrit la sienne sans être effacé.
pub(super) async fn run(ctx: &Ctx, job: Job, cancels: &Cancels) {
    let nonce = job.nonce;
    let ev = attempt(ctx, job).await;
    cancels.forget(nonce);
    ctx.out.send(ev);
}

async fn attempt(ctx: &Ctx, job: Job) -> Event {
    let Job { channel_id, nonce, content, files, kind, cancel } = job;
    let out = &ctx.out;
    let failed = |failure: Failure| Event::Upload(UploadEvent::Failed { channel_id, nonce, failure });
    if *cancel.borrow() {
        return Event::Upload(UploadEvent::Cancelled { channel_id, nonce });
    }
    let checked = {
        let files = files.clone();
        tokio::task::spawn_blocking(move || check_files(&files)).await.unwrap_or(Err(Failure::Problem(Problem::Api(String::new()))))
    };
    if let Err(f) = checked {
        return failed(f);
    }
    let boundary = boundary(nonce);
    let parts = parts(&boundary, &payload(&content, nonce, kind.as_ref(), &files), &files);
    let total: u64 = parts.iter().map(Part::len).sum();
    let sent = Arc::new(AtomicU64::new(0));
    let read_failure: Arc<Mutex<Option<Failure>>> = Arc::default();
    let url = format!("{}/channels/{channel_id}/messages", ctx.api_root);
    let route = super::Route::channel_messages_post(channel_id);
    let build = |req: reqwest::RequestBuilder| {
        // Chaque essai (après un 429) repart du début.
        sent.store(0, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel(AHEAD);
        let (parts, failed) = (parts.clone(), read_failure.clone());
        tokio::task::spawn_blocking(move || produce(parts, tx, failed));
        req.timeout(timeout_for(total))
            .header(reqwest::header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
            .header(reqwest::header::CONTENT_LENGTH, total)
            .body(reqwest::Body::wrap(Streamed { rx, sent: sent.clone(), total }))
    };
    let request = api_req(ctx, route, reqwest::Method::POST, &url, build, MAX_WAIT, cancelled(cancel.clone()));
    tokio::pin!(request);
    let stop = cancelled(cancel);
    tokio::pin!(stop);
    let mut tick = tokio::time::interval(PROGRESS_EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut shown = u64::MAX;
    let result = loop {
        tokio::select! {
            r = &mut request => break Some(r),
            _ = &mut stop => break None,
            _ = tick.tick() => {
                let now = sent.load(Ordering::Relaxed).min(total);
                if now != shown {
                    shown = now;
                    out.send(Event::Upload(UploadEvent::Progress { nonce, sent: now, total }));
                }
            }
        }
    };
    let read_failure = read_failure.lock().unwrap_or_else(PoisonError::into_inner).take();
    match (result, read_failure) {
        // Annulé pendant l'envoi : la requête est abandonnée (connexion fermée).
        (None | Some(Err(ApiError::Cancelled)), _) => Event::Upload(UploadEvent::Cancelled { channel_id, nonce }),
        (Some(Ok(_)), _) => Event::Upload(UploadEvent::Sent { channel_id, nonce }),
        // Corps interrompu par la lecture : rien n'a pu être créé.
        (Some(Err(_)), Some(f)) => failed(f),
        (Some(Err(ApiError::Unauthorized)), None) => ApiError::Unauthorized.into_event(Event::Error),
        (Some(Err(ApiError::Unknown(p))), None) => failed(Failure::Unknown(p)),
        (Some(Err(ApiError::Other(Problem::Http(413)))), None) => failed(Failure::TooLarge),
        (Some(Err(e)), None) => e.into_event(|p| failed(Failure::Problem(p))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, bytes: &[u8]) -> OutFile {
        OutFile { source: Source::Memory(Arc::from(bytes)), name: name.into(), size: bytes.len() as u64, description: String::new(), spoiler: false }
    }

    fn collect(parts: Vec<Part>) -> Vec<u8> {
        let (tx, mut rx) = mpsc::channel(1024);
        let failed = Arc::default();
        std::thread::spawn(move || produce(parts, tx, failed));
        let mut out = Vec::new();
        while let Some(Ok(b)) = rx.blocking_recv() {
            out.extend_from_slice(&b);
        }
        out
    }

    #[test]
    fn payload_links_each_file_by_id() {
        let mut a = file("chat.png", b"x");
        a.spoiler = true;
        a.description = "  un chat  ".into();
        let b = file("notes.txt", b"y");
        let p = payload("salut", 9, Some(&SendKind::Reply { message_id: 5, mention: false }), &[a, b]);
        assert_eq!(p["nonce"], "9");
        assert_eq!(p["enforce_nonce"], true);
        assert_eq!(p["content"], "salut");
        assert_eq!(p["message_reference"]["message_id"], "5");
        assert_eq!(p["attachments"], json!([{ "id": 0, "filename": "SPOILER_chat.png", "description": "un chat" }, { "id": 1, "filename": "notes.txt" }]));
        // Fichiers seuls : pas de texte.
        let p = payload("", 9, None, &[file("a", b"z")]);
        assert!(p.get("content").is_none(), "{p}");
    }

    #[test]
    fn voice_message_payload() {
        let kind = SendKind::Voice { duration_ms: 3520, waveform: "AP8=".into() };
        let p = payload("texte d'échec", 9, Some(&kind), &[file("voice-message.ogg", b"OggS")]);
        assert_eq!(p["flags"], 8192);
        assert!(p.get("content").is_none(), "{p}");
        assert_eq!(p["attachments"], json!([{ "id": 0, "filename": "voice-message.ogg", "duration_secs": 3.52, "waveform": "AP8=" }]));
        assert_eq!(content_type("voice-message.ogg"), "audio/ogg");
    }

    #[test]
    fn multipart_body_is_well_formed_and_its_length_announced() {
        let files = [file("a\"b.png", b"\x89PNG\r\n--"), file("c.txt", b"bonjour")];
        let p = payload("", 1, None, &files);
        let parts = parts("B", &p, &files);
        let total: u64 = parts.iter().map(Part::len).sum();
        let body = collect(parts);
        assert_eq!(body.len() as u64, total);
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with("--B\r\nContent-Disposition: form-data; name=\"payload_json\"\r\nContent-Type: application/json\r\n\r\n{"), "{text}");
        assert!(text.contains("name=\"files[0]\"; filename=\"a%22b.png\"\r\nContent-Type: image/png\r\n\r\n\u{FFFD}PNG\r\n--\r\n--B\r\n"), "{text}");
        assert!(text.contains("name=\"files[1]\"; filename=\"c.txt\"\r\nContent-Type: text/plain\r\n\r\nbonjour\r\n--B--\r\n"), "{text}");
        assert!(text.ends_with("--B--\r\n"));
    }

    #[test]
    fn files_on_disk_are_streamed_and_changes_detected() {
        let dir = std::env::temp_dir().join(format!("ferrocord-upload-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("gros.bin");
        let data: Vec<u8> = (0..(3 * CHUNK + 17)).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &data).unwrap();
        let f = OutFile { source: Source::Path(path.clone()), name: "gros.bin".into(), size: data.len() as u64, description: String::new(), spoiler: false };
        assert_eq!(check_files(std::slice::from_ref(&f)), Ok(()));
        let body = collect(vec![Part::File { path: path.clone(), size: f.size, name: f.name.clone() }]);
        assert_eq!(body, data);
        // Fichier raccourci depuis l'ajout.
        std::fs::write(&path, b"court").unwrap();
        assert_eq!(check_files(std::slice::from_ref(&f)), Err(Failure::Changed("gros.bin".into())));
        let (tx, mut rx) = mpsc::channel(1024);
        let failed: Arc<Mutex<Option<Failure>>> = Arc::default();
        let f2 = failed.clone();
        let (p, s) = (path.clone(), f.size);
        std::thread::spawn(move || produce(vec![Part::File { path: p, size: s, name: "gros.bin".into() }], tx, f2)).join().unwrap();
        let mut saw_error = false;
        while let Some(r) = rx.blocking_recv() {
            saw_error |= r.is_err();
        }
        assert!(saw_error);
        assert_eq!(*failed.lock().unwrap(), Some(Failure::Changed("gros.bin".into())));
        std::fs::remove_file(&path).unwrap();
        assert_eq!(check_files(&[f]), Err(Failure::Unreadable("gros.bin".into())));
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn timeouts_grow_with_size() {
        assert_eq!(timeout_for(0), Duration::from_secs(60));
        assert_eq!(timeout_for(10 << 20), Duration::from_secs(60 + 160));
        assert_eq!(timeout_for(u64::MAX), Duration::from_secs(6 * 3600));
    }
}
