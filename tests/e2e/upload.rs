//! Envoi de fichiers (lot T8) : Create Message en multipart, rejoué après
//! un 429, progression, annulation, fichier modifié avant l'envoi.

use crate::support::{ids::*, *};
use ferrocord::state::upload::{Failure, OutFile, Source, UploadCmd, UploadEvent};
use ferrocord::state::{Cmd, Event};
use std::sync::Arc;
use std::time::Duration;

fn path() -> String {
    format!("/api/v10/channels/{GENERAL}/messages")
}

fn memory(name: &str, bytes: &[u8]) -> OutFile {
    OutFile { source: Source::Memory(Arc::from(bytes)), name: name.into(), size: bytes.len() as u64, description: String::new(), spoiler: false }
}

fn temp_file(name: &str, bytes: &[u8]) -> (std::path::PathBuf, OutFile) {
    let dir = std::env::temp_dir().join(format!("ferrocord-e2e-upload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    let f = OutFile { source: Source::Path(p.clone()), name: name.into(), size: bytes.len() as u64, description: String::new(), spoiler: false };
    (p, f)
}

fn boundary(r: &Request) -> String {
    let ct = r.headers.iter().find(|(n, _)| n == "content-type").map(|(_, v)| v.clone()).unwrap_or_default();
    ct.strip_prefix("multipart/form-data; boundary=").unwrap_or_else(|| panic!("type : {ct}")).to_string()
}

#[tokio::test]
async fn files_are_posted_as_multipart_and_replayed_after_429() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("POST", &path(), [Response::rate_limited(0.05), Response::ok(fixture("rest_message_upload"))]);
    let mut photo = memory("photo.png", b"PNG-image");
    photo.spoiler = true;
    photo.description = "un chat".into();
    let (disk, notes) = temp_file("notes.txt", b"bonjour");
    let nonce = 123_456_789;
    s.client.send(Cmd::Upload(UploadCmd::Send { channel_id: GENERAL, nonce, content: "regarde".into(), files: vec![photo, notes], kind: None }));
    let first = s.http.next_request().await;
    let r = s.http.next_request().await;
    // Le corps en flux est refait en entier pour le nouvel essai.
    assert_eq!(first.body, r.body);
    assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", path().as_str()));
    assert_eq!(r.authorization.as_deref(), Some(USER_TOKEN));
    let b = boundary(&r);
    let len: usize = r.headers.iter().find(|(n, _)| n == "content-length").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    assert_eq!(len, r.body.len());
    let parts: Vec<&str> = r.body.split(&format!("--{b}")).collect();
    // Vide avant le premier séparateur, trois parties, « --\r\n » final.
    assert_eq!(parts.len(), 5, "{}", r.body);
    assert_eq!(parts[4], "--\r\n");
    let (head, json) = parts[1].split_once("\r\n\r\n").unwrap();
    assert!(head.contains("name=\"payload_json\"") && head.contains("application/json"), "{head}");
    let payload: serde_json::Value = serde_json::from_str(json.trim_end()).unwrap();
    assert_eq!(payload["content"], "regarde");
    assert_eq!(payload["nonce"], nonce.to_string());
    assert_eq!(payload["enforce_nonce"], true);
    assert_eq!(payload["attachments"], serde_json::json!([{ "id": 0, "filename": "SPOILER_photo.png", "description": "un chat" }, { "id": 1, "filename": "notes.txt" }]));
    assert!(parts[2].contains("name=\"files[0]\"; filename=\"SPOILER_photo.png\"") && parts[2].ends_with("\r\n\r\nPNG-image\r\n"), "{:?}", parts[2]);
    assert!(parts[3].contains("name=\"files[1]\"; filename=\"notes.txt\"") && parts[3].ends_with("\r\n\r\nbonjour\r\n"), "{:?}", parts[3]);
    s.client.until("envoi terminé", |e| matches!(e, Event::Upload(UploadEvent::Sent { nonce: n, .. }) if *n == nonce)).await;
    let _ = std::fs::remove_file(disk);
}

#[tokio::test]
async fn upload_can_be_cancelled_while_in_flight() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("POST", &path(), [Response::ok(fixture("rest_message_upload")).delayed(Duration::from_secs(5))]);
    let nonce = 42;
    s.client.send(Cmd::Upload(UploadCmd::Send { channel_id: GENERAL, nonce, content: String::new(), files: vec![memory("a.txt", b"abc")], kind: None }));
    let r = s.http.next_request().await;
    assert!(r.body.contains("name=\"files[0]\"; filename=\"a.txt\""));
    // Fichiers seuls : pas de texte dans le corps.
    assert!(!r.body.contains("\"content\""), "{}", r.body);
    s.client.send(Cmd::Upload(UploadCmd::Cancel { nonce }));
    let seen = s.client.until("annulation", |e| matches!(e, Event::Upload(UploadEvent::Cancelled { .. } | UploadEvent::Sent { .. }))).await;
    assert!(seen.contains("Cancelled"), "{seen}");
}

#[tokio::test]
async fn changed_file_fails_before_anything_is_sent() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let (disk, mut f) = temp_file("modifie.txt", b"court");
    f.size = 999;
    s.client.send(Cmd::Upload(UploadCmd::Send { channel_id: GENERAL, nonce: 7, content: "x".into(), files: vec![f], kind: None }));
    let seen = s.client.until("échec", |e| matches!(e, Event::Upload(UploadEvent::Failed { .. }))).await;
    assert!(seen.contains(&format!("{:?}", Failure::Changed("modifie.txt".into()))), "{seen}");
    assert!(s.http.requests().iter().all(|r| r.method != "POST"), "rien ne doit partir");
    let _ = std::fs::remove_file(disk);
}
