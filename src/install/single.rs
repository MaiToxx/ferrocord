//! Instance unique : la première instance écoute sur un canal nommé local
//! (`\\.\pipe\ferrocord-<utilisateur>`) ; un second lancement lui transmet
//! son lien (ou « montrer la fenêtre ») puis se ferme.
//!
//! Le serveur vient de tokio (`named_pipe`), le client est un simple fichier
//! ouvert en écriture : aucun `unsafe` de notre côté. Ce qui arrive par le
//! canal n'est qu'une demande : le lien est revérifié par `links::parse`
//! avant d'ouvrir quoi que ce soit, et jamais confié au navigateur.

use super::links::MAX_LINK;
use std::io::{self, Write};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// Ce qu'un second lancement demande.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Show,
    Open(String),
}

const SHOW: &str = "montrer";
const OPEN: &str = "ouvrir ";
/// Taille maximale d'une demande (un lien et son mot-clé).
const MAX_REQUEST: usize = MAX_LINK + 16;

pub fn encode(r: &Request) -> String {
    match r {
        Request::Show => format!("{SHOW}\n"),
        Request::Open(link) => format!("{OPEN}{link}\n"),
    }
}

pub fn decode(bytes: &[u8]) -> Option<Request> {
    if bytes.len() > MAX_REQUEST {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?.trim_end_matches(['\n', '\r']);
    if text.chars().any(char::is_control) {
        return None;
    }
    if text == SHOW {
        return Some(Request::Show);
    }
    let link = text.strip_prefix(OPEN)?;
    (!link.is_empty() && link.len() <= MAX_LINK).then(|| Request::Open(link.to_string()))
}

/// Nom du canal de cet utilisateur.
pub fn pipe_name() -> String {
    let user: String = std::env::var("USERNAME").unwrap_or_default().chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')).take(64).collect();
    format!(r"\\.\pipe\ferrocord-{user}")
}

type Wake = Arc<dyn Fn() + Send + Sync>;

/// Demandes reçues par la première instance.
pub struct Listener {
    rx: mpsc::Receiver<Request>,
    wake: Arc<Mutex<Option<Wake>>>,
}

impl Listener {
    /// Appelée à chaque demande reçue (réveil de l'interface).
    pub fn set_wake(&self, wake: Wake) {
        if let Ok(mut w) = self.wake.lock() {
            *w = Some(wake);
        }
    }

    pub fn poll(&self) -> Vec<Request> {
        self.rx.try_iter().collect()
    }
}

/// Issue de [`claim`].
pub enum Claim {
    /// Cette instance est la première.
    First(Listener),
    /// Une autre instance a reçu la demande : celle-ci doit se fermer.
    Forwarded,
    /// Canal impossible à créer (rien n'empêche de continuer seul).
    Unavailable,
}

/// Devient la première instance, ou transmet `request` à celle qui l'est.
/// `takeover` : on attend que l'instance ouverte se ferme (redémarrage après
/// une mise à jour) au lieu de lui transmettre quoi que ce soit.
pub fn claim(name: &str, request: &Request, takeover: bool, wait: Duration) -> Claim {
    let deadline = Instant::now() + wait;
    loop {
        match serve(name) {
            Ok(l) => return Claim::First(l),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                if !takeover && send(name, request).is_ok() {
                    return Claim::Forwarded;
                }
            }
            Err(_) => return Claim::Unavailable,
        }
        if Instant::now() >= deadline {
            // L'autre instance ne répond pas et ne s'en va pas : on continue seul.
            return Claim::Unavailable;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Écrit la demande dans le canal (client).
pub fn send(name: &str, request: &Request) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match std::fs::OpenOptions::new().write(true).open(name) {
            Ok(mut f) => return f.write_all(encode(request).as_bytes()),
            // ERROR_PIPE_BUSY : toutes les instances du serveur sont occupées.
            Err(e) if e.raw_os_error() == Some(231) && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(e),
        }
    }
}

#[cfg(windows)]
fn serve(name: &str) -> io::Result<Listener> {
    use tokio::net::windows::named_pipe::ServerOptions;
    let (tx, rx) = mpsc::channel();
    let wake: Arc<Mutex<Option<Wake>>> = Arc::default();
    let (ready_tx, ready_rx) = mpsc::sync_channel::<io::Result<()>>(1);
    let name = name.to_string();
    let wake2 = wake.clone();
    std::thread::Builder::new().name("ferrocord-instance".into()).spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build() {
            Ok(rt) => rt,
            Err(e) => {
                let _ = ready_tx.send(Err(e));
                return;
            }
        };
        rt.block_on(async move {
            let mut server = match ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(&name) {
                Ok(s) => {
                    let _ = ready_tx.send(Ok(()));
                    s
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            loop {
                if server.connect().await.is_err() {
                    continue;
                }
                // L'instance suivante existe avant qu'on lise celle-ci : le
                // nom reste à nous, aucun client ne trouve le canal absent.
                let Ok(next) = ServerOptions::new().reject_remote_clients(true).create(&name) else { return };
                let connected = std::mem::replace(&mut server, next);
                let tx = tx.clone();
                let wake = wake2.clone();
                tokio::spawn(async move {
                    let read = tokio::time::timeout(Duration::from_secs(2), read_all(&connected)).await;
                    if let Ok(Some(r)) = read.map(|b| b.and_then(|b| decode(&b))) {
                        let _ = tx.send(r);
                        if let Some(w) = wake.lock().ok().and_then(|w| w.clone()) {
                            w();
                        }
                    }
                });
            }
        });
    })?;
    match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) => Ok(Listener { rx, wake }),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(io::Error::other("canal de l'instance : pas de réponse")),
    }
}

/// Lit tout ce que le client écrit, jusqu'à sa fermeture (borné).
#[cfg(windows)]
async fn read_all(pipe: &tokio::net::windows::named_pipe::NamedPipeServer) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf = [0u8; 256];
    loop {
        pipe.readable().await.ok()?;
        match pipe.try_read(&mut buf) {
            Ok(0) => return Some(out),
            Ok(n) => {
                out.extend_from_slice(&buf[..n]);
                if out.len() > MAX_REQUEST {
                    return None;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            // Client parti (ERROR_BROKEN_PIPE) : tout ce qu'il a écrit est là.
            Err(_) => return Some(out),
        }
    }
}

#[cfg(not(windows))]
fn serve(_: &str) -> io::Result<Listener> {
    Err(io::Error::other("instance unique : Windows seulement"))
}

/// Arguments de la ligne de commande qui comptent ici.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Args {
    /// Démarrage réduit dans la zone de notification (`--reduit`).
    pub minimized: bool,
    /// Lancé par Ferrocord lui-même après une mise à jour (`--reprendre`) :
    /// attendre la fermeture de l'ancienne instance.
    pub takeover: bool,
    /// Lien reçu (`discord://…` ou `https://discord.com/…`).
    pub link: Option<String>,
}

pub const TAKEOVER_ARG: &str = "--reprendre";

pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Args {
    let mut a = Args::default();
    for arg in args {
        match arg.as_str() {
            super::system::MINIMIZED_ARG => a.minimized = true,
            TAKEOVER_ARG => a.takeover = true,
            s if a.link.is_none() && super::links::parse(s).is_some() => a.link = Some(s.to_string()),
            _ => {}
        }
    }
    a
}

impl Args {
    /// Demande transmise à l'instance ouverte.
    pub fn request(&self) -> Request {
        match &self.link {
            Some(l) => Request::Open(l.clone()),
            None => Request::Show,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip_and_garbage_is_refused() {
        for r in [Request::Show, Request::Open("discord://-/channels/1/2".into())] {
            assert_eq!(decode(encode(&r).as_bytes()), Some(r));
        }
        assert_eq!(decode(b"ouvrir \n"), None);
        assert_eq!(decode(b"ouvrir a\x07b"), None);
        assert_eq!(decode(b"\xff\xfe"), None);
        assert_eq!(decode(b"effacer tout"), None);
        assert_eq!(decode(format!("ouvrir {}", "a".repeat(MAX_LINK + 1)).as_bytes()), None);
    }

    #[test]
    fn arguments() {
        let a = parse_args(["--reduit".to_string(), "discord://-/channels/1/2".into(), "https://discord.com/users/3".into(), "--inconnu".into()]);
        assert_eq!(a, Args { minimized: true, takeover: false, link: Some("discord://-/channels/1/2".into()) });
        assert_eq!(a.request(), Request::Open("discord://-/channels/1/2".into()));
        let b = parse_args(["--reprendre".to_string(), "https://example.invalid".into()]);
        assert_eq!((b.takeover, b.link.clone(), b.request()), (true, None, Request::Show));
    }

    /// Deux « lancements » dans le même processus, sur un canal propre au test.
    #[cfg(windows)]
    #[test]
    fn second_launch_forwards_its_link_then_leaves() {
        let name = format!(r"\\.\pipe\ferrocord-test-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')'], "");
        let Claim::First(first) = claim(&name, &Request::Show, false, Duration::from_secs(2)) else { panic!("première instance attendue") };
        let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let w = woken.clone();
        first.set_wake(Arc::new(move || {
            w.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }));
        let link = Request::Open("discord://-/channels/1/2/3".into());
        assert!(matches!(claim(&name, &link, false, Duration::from_secs(2)), Claim::Forwarded));
        assert!(matches!(claim(&name, &Request::Show, false, Duration::from_secs(2)), Claim::Forwarded));
        let start = Instant::now();
        let mut got = Vec::new();
        while got.len() < 2 && start.elapsed() < Duration::from_secs(10) {
            got.extend(first.poll());
            std::thread::sleep(Duration::from_millis(10));
        }
        // Lues en parallèle : l'ordre d'arrivée n'est pas garanti.
        assert!(got.len() == 2 && got.contains(&link) && got.contains(&Request::Show), "{got:?}");
        assert!(woken.load(std::sync::atomic::Ordering::SeqCst) >= 2);
        // Reprise : l'instance ouverte ne se ferme pas, on abandonne sans rien lui envoyer.
        assert!(matches!(claim(&name, &Request::Show, true, Duration::from_millis(200)), Claim::Unavailable));
        assert!(first.poll().is_empty());
        // Un client brut qui écrit n'importe quoi n'ouvre rien.
        let mut raw = std::fs::OpenOptions::new().write(true).open(&name).unwrap();
        raw.write_all(b"supprimer C:\\").unwrap();
        drop(raw);
        std::thread::sleep(Duration::from_millis(200));
        assert!(first.poll().is_empty());
    }
}
