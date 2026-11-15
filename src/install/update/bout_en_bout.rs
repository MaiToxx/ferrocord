//! Mise à jour de bout en bout, en local (lot distrib) : un faux serveur de
//! « Releases » sur 127.0.0.1, un manifeste signé par une clé de test, le
//! vrai fil d'installation, un dossier temporaire à la place de celui de
//! l'exécutable. Détection → téléchargement → vérification → remplacement →
//! relance simulée → confirmation, puis les refus. Rien ne sort de la machine.

use super::manifest::{envelope_json, payload_json, payload_json_on};
use super::{swap, Manifest, Status, Version};
use crate::install::hive::MemoryHive;
use crate::install::memo;
use crate::install::service::{Config, InstallService, MIN_GAP};
use crate::install::{InstallEvent, Installer};
use aws_lc_rs::digest;
use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::signature::{Ed25519KeyPair, KeyPair};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
enum Reply {
    Body(Vec<u8>),
    /// Annonce la taille entière, n'envoie que le début, puis coupe.
    Cut(Vec<u8>, usize),
    Redirect(String),
}

/// Faux serveur de fichiers sur 127.0.0.1 ; retient chaque requête reçue.
#[derive(Clone)]
struct Server {
    base: String,
    routes: Arc<Mutex<HashMap<String, Reply>>>,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Server {
    fn start() -> Server {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let s = Server { base: format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port()), routes: Arc::default(), seen: Arc::default() };
        let (routes, seen) = (s.routes.clone(), s.seen.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut head = Vec::new();
                let mut buf = [0u8; 1024];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf) {
                        Ok(n) if n > 0 => head.extend_from_slice(&buf[..n]),
                        _ => break,
                    }
                }
                let head = String::from_utf8_lossy(&head).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                seen.lock().unwrap().push(head);
                let reply = routes.lock().unwrap().get(&path).cloned();
                let _ = match reply {
                    None => stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
                    Some(Reply::Redirect(to)) => stream.write_all(format!("HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()),
                    Some(Reply::Body(b)) => stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b.len()).as_bytes()).and_then(|()| stream.write_all(&b)),
                    Some(Reply::Cut(b, sent)) => stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b.len()).as_bytes()).and_then(|()| stream.write_all(&b[..sent])),
                };
            }
        });
        s
    }

    fn route(&self, path: &str, reply: Reply) {
        self.routes.lock().unwrap().insert(path.to_string(), reply);
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// Chemins demandés jusqu'ici.
    fn paths(&self) -> Vec<String> {
        self.seen.lock().unwrap().iter().map(|h| h.split_whitespace().nth(1).unwrap_or("").to_string()).collect()
    }
}

fn keypair() -> Ed25519KeyPair {
    Ed25519KeyPair::from_pkcs8(Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap().as_ref()).unwrap()
}

fn public(k: &Ed25519KeyPair) -> [u8; 32] {
    k.public_key().as_ref().try_into().unwrap()
}

fn sign(k: &Ed25519KeyPair, payload: &str) -> Vec<u8> {
    envelope_json(payload, k.sign(payload.as_bytes()).as_ref()).into_bytes()
}

fn sha(bytes: &[u8]) -> [u8; 32] {
    digest::digest(&digest::SHA256, bytes).as_ref().try_into().unwrap()
}

/// Un « exécutable » assez gros pour arriver en plusieurs morceaux.
fn binary(seed: u8) -> Vec<u8> {
    (0..300_000u32).map(|i| (i.wrapping_mul(31).wrapping_add(u32::from(seed)) % 251) as u8).collect()
}

/// Dossier d'installation factice, avec la version 1 en place.
struct Home {
    dir: PathBuf,
}

impl Home {
    fn new(name: &str) -> Home {
        let dir = std::env::temp_dir().join(format!("ferrocord-bout-en-bout-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ferrocord.exe"), b"version 1").unwrap();
        Home { dir }
    }

    fn exe(&self) -> PathBuf {
        self.dir.join("ferrocord.exe")
    }

    fn layout(&self) -> swap::Layout {
        swap::Layout { exe: self.exe() }
    }

    fn memo(&self) -> PathBuf {
        self.dir.join("reglages").join(memo::FILE)
    }

    /// Un « lancement » : le fil d'installation de cette version.
    fn service(&self, key: [u8; 32], default_url: &str, current: Version, min_gap: Duration) -> InstallService {
        InstallService::start_with(Config { hive: Box::new(MemoryHive::default()), exe: Some(self.exe()), key: Some(key), default_url: default_url.to_string(), memo: Some(self.memo()), current, min_gap, wake: Arc::new(|| {}) })
    }

    /// Rien d'une mise à jour à moitié faite ne traîne.
    fn assert_untouched(&self) {
        assert_eq!(std::fs::read(self.exe()).unwrap(), b"version 1");
        for name in [swap::NEW, swap::OLD, swap::STATE, "ferrocord.new.exe.part"] {
            assert!(!self.dir.join(name).exists(), "{name} ne devrait pas exister");
        }
        assert_eq!(swap::startup(&self.layout()).unwrap(), swap::Startup::Continue);
        assert_eq!(std::fs::read(self.exe()).unwrap(), b"version 1");
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Attend l'état final d'une opération (ni « recherche », ni « téléchargement »).
fn settle(s: &mut InstallService) -> Status {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        for e in s.poll() {
            if let InstallEvent::Update(st) = e
                && !matches!(st, Status::Checking | Status::Downloading { .. })
            {
                return st;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("le fil d'installation n'a pas répondu");
}

fn available(s: Status) -> Manifest {
    match s {
        Status::Available(m) => m,
        other => panic!("version attendue, reçu {other:?}"),
    }
}

fn failure(s: Status) -> String {
    match s {
        Status::Failed(why) => why,
        other => panic!("échec attendu, reçu {other:?}"),
    }
}

fn wait_for(what: &str, done: impl Fn() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < Duration::from_secs(10), "{what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

const V1: Version = Version(1, 0, 0);

/// Publie `exe` comme la version `version`, à la façon des Releases de
/// GitHub : l'adresse fixe « latest » redirige vers la version, et le
/// fichier vers le stockage. Rend l'adresse fixe du manifeste.
fn publish(srv: &Server, k: &Ed25519KeyPair, version: &str, exe: &[u8], notes: &str) -> String {
    let file = super::source::binary_name(version);
    let payload = payload_json(version, &srv.url(&format!("/moi/ferrocord/releases/download/v{version}/{file}")), &sha(exe), exe.len() as u64, notes);
    srv.route("/moi/ferrocord/releases/latest/download/ferrocord-manifeste.json", Reply::Redirect(srv.url(&format!("/moi/ferrocord/releases/download/v{version}/ferrocord-manifeste.json"))));
    srv.route(&format!("/moi/ferrocord/releases/download/v{version}/ferrocord-manifeste.json"), Reply::Redirect(format!("/stockage/{version}/manifeste")));
    srv.route(&format!("/stockage/{version}/manifeste"), Reply::Body(sign(k, &payload)));
    srv.route(&format!("/moi/ferrocord/releases/download/v{version}/{file}"), Reply::Redirect(srv.url(&format!("/stockage/{version}/exe"))));
    srv.route(&format!("/stockage/{version}/exe"), Reply::Body(exe.to_vec()));
    srv.url("/moi/ferrocord/releases/latest/download/ferrocord-manifeste.json")
}

#[test]
fn update_from_detection_to_confirmed_relaunch() {
    let (srv, k, home) = (Server::start(), keypair(), Home::new("complet"));
    let v2 = binary(2);
    let latest = publish(&srv, &k, "9.1.0", &v2, "Corrections et nouveautés");

    // Lancement de la version 1 : adresse intégrée (rien dans local.toml).
    let mut s = home.service(public(&k), &latest, V1, MIN_GAP);
    s.configure_updates(String::new());
    let m = available(settle(&mut s));
    assert_eq!((m.version, m.notes.as_str(), m.size), (Version(9, 1, 0), "Corrections et nouveautés", v2.len() as u64));
    assert_eq!(s.auto_check(), Some(true), "activée par défaut dès qu'une adresse existe");
    assert!(!home.dir.join(swap::NEW).exists(), "rien n'est téléchargé avant le clic");

    // Clic « Télécharger et vérifier ».
    s.download(m);
    assert_eq!(settle(&mut s), Status::Ready(Version(9, 1, 0)));
    assert_eq!(std::fs::read(home.dir.join(swap::NEW)).unwrap(), v2);
    assert_eq!(std::fs::read(home.exe()).unwrap(), b"version 1", "rien n'est remplacé avant le redémarrage");
    assert!(!home.dir.join("ferrocord.new.exe.part").exists());

    // Les requêtes : seulement ce serveur, sans jeton, témoin ni identifiant.
    let seen = srv.seen.lock().unwrap().clone();
    assert_eq!(srv.paths(), ["/moi/ferrocord/releases/latest/download/ferrocord-manifeste.json", "/moi/ferrocord/releases/download/v9.1.0/ferrocord-manifeste.json", "/stockage/9.1.0/manifeste", "/moi/ferrocord/releases/download/v9.1.0/Ferrocord-9.1.0-x64.exe", "/stockage/9.1.0/exe"]);
    for head in &seen {
        let lower = head.to_lowercase();
        assert!(head.starts_with("GET "), "{head}");
        assert!(lower.contains(&format!("user-agent: ferrocord/{}\r\n", env!("CARGO_PKG_VERSION"))), "{head}");
        for banned in ["authorization", "cookie", "x-super-properties", "x-discord", "discord", "x-fingerprint", "referer", "?"] {
            assert!(!lower.contains(banned), "« {banned} » dans {head}");
        }
        // Rien que les en-têtes ordinaires d'un client HTTP.
        for line in lower.lines().skip(1).filter(|l| !l.is_empty()) {
            let name = line.split(':').next().unwrap();
            assert!(["host", "user-agent", "accept", "accept-encoding"].contains(&name), "en-tête inattendu : {line}");
        }
    }
    drop(s);

    // « Redémarrer pour mettre à jour » : le lancement suivant remplace, puis relance.
    let l = home.layout();
    assert_eq!(swap::startup(&l).unwrap(), swap::Startup::Relaunch);
    assert_eq!(std::fs::read(home.exe()).unwrap(), v2);
    assert_eq!(std::fs::read(home.dir.join(swap::OLD)).unwrap(), b"version 1", "l'ancienne est gardée pour le retour arrière");
    // La relance : la version 9.1.0 démarre à l'essai, affiche sa première image.
    assert_eq!(swap::startup(&l).unwrap(), swap::Startup::Continue);
    let before = srv.paths().len();
    let mut s = home.service(public(&k), &latest, Version(9, 1, 0), MIN_GAP);
    s.configure_updates(String::new());
    // Vérifiée il y a un instant : pas de nouvelle requête (jamais en rafale).
    assert_eq!(settle(&mut s), Status::Idle);
    assert_eq!(srv.paths().len(), before);
    s.confirm_started();
    wait_for("confirmation de la nouvelle version", || !home.dir.join(swap::OLD).exists() && !home.dir.join(swap::STATE).exists());
    // « Rechercher maintenant » vérifie toujours : à jour.
    s.check_updates();
    assert_eq!(settle(&mut s), Status::UpToDate { refused: None });
    assert_eq!(srv.paths().len(), before + 3);
    assert!(memo::read(&home.memo()).derniere_verification > 0);
    assert_eq!(swap::startup(&l).unwrap(), swap::Startup::Continue);
    assert_eq!(std::fs::read(home.exe()).unwrap(), v2);
}

#[test]
fn version_that_does_not_start_is_rolled_back_and_not_offered_again() {
    let (srv, k, home) = (Server::start(), keypair(), Home::new("retour"));
    let v2 = binary(3);
    let latest = publish(&srv, &k, "9.2.0", &v2, "");
    let mut s = home.service(public(&k), &latest, V1, Duration::ZERO);
    s.configure_updates(String::new());
    let m = available(settle(&mut s));
    s.download(m);
    assert_eq!(settle(&mut s), Status::Ready(Version(9, 2, 0)));
    drop(s);
    let l = home.layout();
    assert_eq!(swap::startup(&l).unwrap(), swap::Startup::Relaunch);
    assert_eq!(swap::startup(&l).unwrap(), swap::Startup::Continue, "à l'essai");
    // Elle plante avant sa première image ; l'utilisateur relance.
    assert_eq!(swap::startup(&l).unwrap(), swap::Startup::Relaunch);
    assert_eq!(std::fs::read(home.exe()).unwrap(), b"version 1");
    assert_eq!(swap::startup(&l).unwrap(), swap::Startup::Continue);
    let mut s = home.service(public(&k), &latest, V1, Duration::ZERO);
    s.configure_updates(String::new());
    assert_eq!(settle(&mut s), Status::UpToDate { refused: Some("9.2.0".into()) });
}

#[test]
fn bad_signature_other_channel_and_older_versions_are_refused() {
    let (srv, k, home) = (Server::start(), keypair(), Home::new("refus"));
    let exe = binary(4);
    let url = srv.url("/f.exe");
    srv.route("/f.exe", Reply::Body(exe.clone()));
    let good = payload_json("9.0.0", &url, &sha(&exe), exe.len() as u64, "");
    // Signé par une autre clé ; altéré après signature ; autre canal.
    srv.route("/autre-cle.json", Reply::Body(sign(&keypair(), &good)));
    srv.route("/altere.json", Reply::Body(String::from_utf8(sign(&k, &good)).unwrap().replace("9.0.0", "9.9.0").into_bytes()));
    srv.route("/beta.json", Reply::Body(sign(&k, &payload_json_on("beta", "9.0.0", &url, &sha(&exe), exe.len() as u64, ""))));
    // Plus ancienne que la version installée, puis la même.
    srv.route("/ancienne.json", Reply::Body(sign(&k, &payload_json("0.9.9", &url, &sha(&exe), exe.len() as u64, ""))));
    srv.route("/meme.json", Reply::Body(sign(&k, &payload_json("1.0.0", &url, &sha(&exe), exe.len() as u64, ""))));
    let mut s = home.service(public(&k), "", V1, Duration::ZERO);
    for (path, why) in [("/autre-cle.json", "signature invalide"), ("/altere.json", "signature invalide"), ("/beta.json", "autre canal"), ("/absent.json", "404")] {
        s.configure_updates(srv.url(path));
        let got = failure(settle(&mut s));
        assert!(got.contains(why), "{path} : {got}");
    }
    for path in ["/ancienne.json", "/meme.json"] {
        s.configure_updates(srv.url(path));
        assert_eq!(settle(&mut s), Status::UpToDate { refused: None }, "{path} : pas de retour en arrière");
    }
    // Même demandée de force, une version plus ancienne n'est pas téléchargée.
    s.download(Manifest { version: Version(0, 9, 9), url: url.clone(), sha256: sha(&exe), size: exe.len() as u64, notes: String::new() });
    assert!(failure(settle(&mut s)).contains("plus ancienne"));
    assert!(!srv.paths().iter().any(|p| p == "/f.exe"), "l'exécutable n'a jamais été demandé");
    drop(s);
    home.assert_untouched();
}

#[test]
fn wrong_hash_and_cut_download_leave_nothing_behind() {
    let (srv, k, home) = (Server::start(), keypair(), Home::new("abime"));
    let exe = binary(5);
    // Le serveur rend un autre fichier que celui du manifeste (même taille).
    srv.route("/autre.exe", Reply::Body(binary(6)));
    srv.route("/faux-sha.json", Reply::Body(sign(&k, &payload_json("9.0.0", &srv.url("/autre.exe"), &sha(&exe), exe.len() as u64, ""))));
    // Le téléchargement est coupé à mi-chemin.
    srv.route("/coupe.exe", Reply::Cut(exe.clone(), exe.len() / 2));
    srv.route("/coupe.json", Reply::Body(sign(&k, &payload_json("9.0.0", &srv.url("/coupe.exe"), &sha(&exe), exe.len() as u64, ""))));
    // Le fichier est plus gros qu'annoncé par le manifeste.
    srv.route("/gros.json", Reply::Body(sign(&k, &payload_json("9.0.0", &srv.url("/autre.exe"), &sha(&exe), 1000, ""))));
    let mut s = home.service(public(&k), "", V1, Duration::ZERO);
    for (path, why) in [("/faux-sha.json", "SHA-256"), ("/coupe.json", "interrompu"), ("/gros.json", "trop grosse")] {
        s.configure_updates(srv.url(path));
        let m = available(settle(&mut s));
        s.download(m);
        let got = failure(settle(&mut s));
        assert!(got.contains(why), "{path} : {got}");
        assert!(!home.dir.join(swap::NEW).exists() && !home.dir.join("ferrocord.new.exe.part").exists(), "{path}");
    }
    drop(s);
    home.assert_untouched();
}

#[test]
fn redirection_to_an_unexpected_host_is_refused_before_any_connection() {
    let (srv, k, home) = (Server::start(), keypair(), Home::new("redirection"));
    let exe = binary(7);
    let port = srv.base.rsplit(':').next().unwrap().to_string();
    // Le manifeste renvoie ailleurs : hôte inconnu en HTTPS, puis un autre nom local.
    srv.route("/ailleurs.json", Reply::Redirect("https://exemple.invalid/manifeste.json".into()));
    srv.route("/autre-nom.json", Reply::Redirect(format!("http://localhost:{port}/bon.json")));
    srv.route("/bon.json", Reply::Body(sign(&k, &payload_json("9.0.0", &srv.url("/f.exe"), &sha(&exe), exe.len() as u64, ""))));
    // Le manifeste est bon, mais l'exécutable renvoie ailleurs.
    srv.route("/f.exe", Reply::Redirect("https://exemple.invalid/f.exe".into()));
    let mut s = home.service(public(&k), "", V1, Duration::ZERO);
    for path in ["/ailleurs.json", "/autre-nom.json"] {
        s.configure_updates(srv.url(path));
        let got = failure(settle(&mut s));
        assert!(got.contains("hôte non autorisé"), "{path} : {got}");
    }
    assert!(!srv.paths().iter().any(|p| p == "/bon.json"), "la redirection n'a pas été suivie");
    s.configure_updates(srv.url("/bon.json"));
    let m = available(settle(&mut s));
    s.download(m);
    let got = failure(settle(&mut s));
    assert!(got.contains("hôte non autorisé"), "{got}");
    drop(s);
    home.assert_untouched();
}

#[test]
fn automatic_check_can_be_turned_off_and_is_remembered() {
    let (srv, k, home) = (Server::start(), keypair(), Home::new("reglage"));
    let latest = publish(&srv, &k, "9.3.0", &binary(8), "");
    let mut s = home.service(public(&k), &latest, V1, MIN_GAP);
    s.set_auto_check(false);
    wait_for("réglage pris en compte", || s.auto_check() == Some(false));
    s.configure_updates(String::new());
    assert_eq!(settle(&mut s), Status::Idle);
    assert!(srv.paths().is_empty(), "aucune requête sans le réglage");
    // Le bouton, lui, vérifie.
    s.check_updates();
    assert_eq!(available(settle(&mut s)).version, Version(9, 3, 0));
    drop(s);
    // Lancement suivant : le réglage est resté, toujours aucune requête.
    let before = srv.paths().len();
    let mut s = home.service(public(&k), &latest, V1, MIN_GAP);
    s.configure_updates(String::new());
    assert_eq!(settle(&mut s), Status::Idle);
    assert_eq!((s.auto_check(), srv.paths().len()), (Some(false), before));
    assert_eq!(memo::read(&home.memo()).verifier_auto, Some(false));
    // Une adresse de local.toml l'emporte sur celle intégrée.
    srv.route("/a-moi.json", Reply::Body(sign(&k, &payload_json("9.4.0", &srv.url("/f.exe"), &[1; 32], 10, ""))));
    s.configure_updates(srv.url("/a-moi.json"));
    assert_eq!(settle(&mut s), Status::Idle);
    s.check_updates();
    assert_eq!(available(settle(&mut s)).version, Version(9, 4, 0));
}

