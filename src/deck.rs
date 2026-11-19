//! Commande locale pour le Stream Deck (plugin « Stream Deck », 0.1.14).
//!
//! Un tube nommé de Windows (`\\.\pipe\ferrocord-commande`) que le plugin
//! Stream Deck de Ferrocord (`streamdeck/`, installé par [`install`]) ouvre
//! sur ce même ordinateur. Une ligne JSON par message :
//!
//! - reçu : `{"do":"mute"}`, `{"do":"talk","on":true}` ([`Do`]) ; tout le reste est ignoré ;
//! - envoyé à la connexion puis à chaque changement : l'état ([`State`]) ;
//! - tableau de sons : `{"do":"sounds"}` demande la liste (envoyée ensuite à
//!   chaque changement, [`sounds_line`]), `{"do":"sound","id":"…","guild":"…"}`
//!   joue un son, comme un clic dans le tableau de sons.
//!
//! Rien d'autre ne se commande d'ici (ni message, ni salon, ni compte), rien
//! ne se lit que ces sept booléens et le nom des sons jouables, et rien ne part chez Discord : les
//! actions sont celles des boutons du panneau vocal. Le tube est créé avec
//! `first_pipe_instance` (personne ne s'y glisse) et refuse les clients
//! distants (valeur par défaut de tokio) ; la sécurité par défaut de Windows
//! ne laisse écrire que le compte qui l'a créé.
//!
//! Plugin coupé : rien n'existe (ni fil, ni tube).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Nom du tube.
pub const PIPE: &str = r"\\.\pipe\ferrocord-commande";
/// Ligne reçue plus longue : connexion fermée.
const MAX_LINE: usize = 256;
/// Actions en attente : l'interface reprend la file à chaque image.
const MAX_FIRED: usize = 16;
/// Programmes branchés à la fois (un seul plugin Stream Deck suffit).
#[cfg(windows)]
const MAX_CLIENTS: usize = 4;

/// Ce qu'une touche demande.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Do {
    Mute,
    Deafen,
    Leave,
    Camera,
    Share,
    Answer,
    Decline,
    /// Appui-pour-parler : touche enfoncée (`true`) ou relâchée. Un
    /// programme qui se débranche touche enfoncée la relâche.
    Talk(bool),
    /// Demande la liste des sons du tableau de sons.
    Sounds,
    /// Joue un son ; `guild` : son serveur (`None` : son par défaut).
    Sound { id: u64, guild: Option<u64> },
}

/// Un son proposé aux touches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoundRef {
    pub id: u64,
    pub guild: Option<u64>,
    pub name: String,
}

/// La ligne de la liste des sons. Les identifiants sont des chaînes : un
/// nombre de 64 bits ne tient pas dans un nombre de JavaScript.
pub fn sounds_line(list: &[SoundRef]) -> String {
    let sounds: Vec<serde_json::Value> = list.iter().map(|s| serde_json::json!({ "id": s.id.to_string(), "guild": s.guild.map(|g| g.to_string()), "name": s.name })).collect();
    let mut s = serde_json::json!({ "sounds": sounds }).to_string();
    s.push('\n');
    s
}

/// Lit une ligne reçue : `{"do":"mute"}`.
pub fn parse(line: &str) -> Option<Do> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let id = |k: &str| v.get(k).and_then(|x| x.as_str()).and_then(|s| s.parse::<u64>().ok());
    Some(match v.get("do")?.as_str()? {
        "mute" => Do::Mute,
        "deafen" => Do::Deafen,
        "leave" => Do::Leave,
        "camera" => Do::Camera,
        "share" => Do::Share,
        "answer" => Do::Answer,
        "decline" => Do::Decline,
        "talk" => Do::Talk(v.get("on")?.as_bool()?),
        "sounds" => Do::Sounds,
        "sound" => Do::Sound { id: id("id")?, guild: id("guild") },
        _ => return None,
    })
}

/// Ce que les touches montrent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// Dans un salon vocal ou un appel.
    pub voice: bool,
    pub mute: bool,
    pub deaf: bool,
    pub camera: bool,
    pub share: bool,
    /// Un appel sonne.
    pub ringing: bool,
    /// Touche d'appui-pour-parler tenue.
    pub talk: bool,
}

impl State {
    /// La ligne envoyée (terminée par un saut de ligne).
    pub fn line(&self) -> String {
        let mut s = serde_json::json!({ "v": 1, "voice": self.voice, "mute": self.mute, "deaf": self.deaf, "camera": self.camera, "share": self.share, "ringing": self.ringing, "talk": self.talk }).to_string();
        s.push('\n');
        s
    }
}

/// Réveille l'interface quand une action arrive.
pub type Wake = Arc<dyn Fn() + Send + Sync>;
type Fired = Arc<Mutex<Vec<Do>>>;

/// Le serveur, tant que cette valeur vit : la lâcher ferme le tube et
/// débranche les programmes.
pub struct Deck {
    last: State,
    #[cfg(windows)]
    state: tokio::sync::watch::Sender<String>,
    /// Liste des sons (vide tant que personne ne l'a demandée).
    #[cfg(windows)]
    sounds: tokio::sync::watch::Sender<String>,
    fired: Fired,
}

impl Deck {
    /// Ouvre le tube sur un fil à lui. `None` : pas de tubes nommés sur ce
    /// système, ou fil refusé.
    pub fn start(wake: Wake) -> Option<Deck> {
        Self::start_on(PIPE.to_string(), wake)
    }

    #[cfg(windows)]
    fn start_on(name: String, wake: Wake) -> Option<Deck> {
        let last = State::default();
        let (state, rx) = tokio::sync::watch::channel(last.line());
        let (sounds, sounds_rx) = tokio::sync::watch::channel(String::new());
        let fired: Fired = Arc::default();
        let f = fired.clone();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
        std::thread::Builder::new().name("stream-deck".into()).spawn(move || rt.block_on(imp::serve(name, rx, sounds_rx, f, wake))).ok()?;
        Some(Deck { last, state, sounds, fired })
    }

    #[cfg(not(windows))]
    fn start_on(_name: String, _wake: Wake) -> Option<Deck> {
        None
    }

    /// Actions demandées depuis le dernier appel, dans l'ordre.
    pub fn take(&self) -> Vec<Do> {
        self.fired.lock().map(|mut f| std::mem::take(&mut *f)).unwrap_or_default()
    }

    /// Dit l'état du moment : n'écrit aux programmes que s'il a changé.
    pub fn publish(&mut self, now: State) {
        if now != self.last {
            self.last = now;
            #[cfg(windows)]
            let _ = self.state.send(now.line());
        }
    }
}

impl Deck {
    /// Dit la liste des sons aux programmes branchés.
    pub fn publish_sounds(&self, list: &[SoundRef]) {
        #[cfg(windows)]
        let _ = self.sounds.send(sounds_line(list));
        #[cfg(not(windows))]
        let _ = list;
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn fire(fired: &Fired, wake: &Wake, d: Do) {
    if let Ok(mut f) = fired.lock()
        && f.len() < MAX_FIRED
    {
        f.push(d);
    }
    wake();
}

#[cfg(windows)]
mod imp {
    use super::{fire, parse, Do, Fired, Wake, MAX_CLIENTS, MAX_LINE};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
    use tokio::sync::watch;
    use tokio::task::JoinSet;

    /// Attente avant de réessayer (tube pris par un autre Ferrocord, erreur du système).
    const RETRY: Duration = Duration::from_secs(5);

    /// Sert jusqu'à ce que l'émetteur de l'état soit lâché.
    pub(super) async fn serve(name: String, rx: watch::Receiver<String>, sounds: watch::Receiver<String>, fired: Fired, wake: Wake) {
        let mut gone = rx.clone();
        tokio::select! {
            _ = listen(&name, rx, sounds, fired, wake) => {}
            _ = async { while gone.changed().await.is_ok() {} } => {}
        }
    }

    async fn create(name: &str, first: bool) -> NamedPipeServer {
        loop {
            match ServerOptions::new().first_pipe_instance(first).create(name) {
                Ok(s) => return s,
                Err(_) => tokio::time::sleep(RETRY).await,
            }
        }
    }

    async fn listen(name: &str, rx: watch::Receiver<String>, sounds: watch::Receiver<String>, fired: Fired, wake: Wake) {
        let mut server = create(name, true).await;
        // Abandonné avec `listen` : chaque connexion en cours est coupée.
        let mut conns = JoinSet::new();
        loop {
            if server.connect().await.is_err() {
                server = create(name, false).await;
                continue;
            }
            // L'instance suivante existe avant que celle-ci soit servie.
            let stream = std::mem::replace(&mut server, create(name, false).await);
            while conns.try_join_next().is_some() {}
            if conns.len() < MAX_CLIENTS {
                conns.spawn(session(stream, rx.clone(), sounds.clone(), fired.clone(), wake.clone()));
            }
        }
    }

    /// Relâche l'appui-pour-parler d'un programme qui s'en va touche enfoncée
    /// (plugin arrêté, Stream Deck fermé) : le micro ne reste jamais ouvert.
    struct Held {
        fired: Fired,
        wake: Wake,
        on: bool,
    }

    impl Drop for Held {
        fn drop(&mut self) {
            if self.on {
                fire(&self.fired, &self.wake, Do::Talk(false));
            }
        }
    }

    async fn session(stream: NamedPipeServer, mut rx: watch::Receiver<String>, mut sounds: watch::Receiver<String>, fired: Fired, wake: Wake) {
        let (mut r, mut w) = tokio::io::split(stream);
        let mut held = Held { fired: fired.clone(), wake: wake.clone(), on: false };
        let first = rx.borrow_and_update().clone() + sounds.borrow_and_update().as_str();
        if w.write_all(first.as_bytes()).await.is_err() {
            return;
        }
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 256];
        loop {
            tokio::select! {
                c = rx.changed() => {
                    if c.is_err() {
                        return;
                    }
                    let line = rx.borrow_and_update().clone();
                    if w.write_all(line.as_bytes()).await.is_err() {
                        return;
                    }
                }
                c = sounds.changed() => {
                    if c.is_err() {
                        return;
                    }
                    let line = sounds.borrow_and_update().clone();
                    if w.write_all(line.as_bytes()).await.is_err() {
                        return;
                    }
                }
                n = r.read(&mut chunk) => {
                    let Ok(n @ 1..) = n else { return };
                    buf.extend_from_slice(&chunk[..n]);
                    while let Some(end) = buf.iter().position(|b| *b == b'\n') {
                        let line: Vec<u8> = buf.drain(..=end).collect();
                        if let Some(d) = std::str::from_utf8(&line).ok().and_then(parse) {
                            if let Do::Talk(on) = d {
                                held.on = on;
                            }
                            fire(&fired, &wake, d);
                        }
                    }
                    if buf.len() > MAX_LINE {
                        return;
                    }
                }
            }
        }
    }
}

/// Dossier du plugin dans celui de Stream Deck.
const FOLDER: &str = "io.github.maitoxx.ferrocord.sdPlugin";

macro_rules! files {
    ($($path:literal),* $(,)?) => {
        &[$(($path, include_bytes!(concat!("../streamdeck/io.github.maitoxx.ferrocord.sdPlugin/", $path)) as &[u8])),*]
    };
}

/// Fichiers du plugin Stream Deck, embarqués dans Ferrocord.
const FILES: &[(&str, &[u8])] = files![
    "manifest.json",
    "bin/package.json",
    "bin/plugin.js",
    "ui/sound.html",
    "imgs/plugin/category.svg",
    "imgs/plugin/marketplace.png",
    "imgs/plugin/marketplace@2x.png",
    "imgs/actions/mute.svg",
    "imgs/actions/deafen.svg",
    "imgs/actions/leave.svg",
    "imgs/actions/camera.svg",
    "imgs/actions/share.svg",
    "imgs/actions/answer.svg",
    "imgs/actions/decline.svg",
    "imgs/actions/talk.svg",
    "imgs/actions/sound.svg",
    "imgs/keys/mute.svg",
    "imgs/keys/mute-on.svg",
    "imgs/keys/deafen.svg",
    "imgs/keys/deafen-on.svg",
    "imgs/keys/leave.svg",
    "imgs/keys/camera.svg",
    "imgs/keys/camera-on.svg",
    "imgs/keys/share.svg",
    "imgs/keys/share-on.svg",
    "imgs/keys/answer.svg",
    "imgs/keys/decline.svg",
    "imgs/keys/talk.svg",
    "imgs/keys/talk-on.svg",
    "imgs/keys/sound.svg",
];

/// Ce que [`install`] a fait.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Installed {
    /// Fichiers écrits ou mis à jour : Stream Deck doit être relancé.
    Written,
    /// Déjà en place, à l'identique.
    Current,
    /// Stream Deck n'est pas installé pour ce compte.
    NoStreamDeck,
    /// Écriture refusée.
    Failed,
}

/// Dossier des plugins de Stream Deck pour ce compte.
pub fn plugins_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Elgato").join("StreamDeck").join("Plugins"))
}

/// Pose le plugin Stream Deck dans `plugins` (le dossier des plugins de
/// Stream Deck), sans toucher aux fichiers déjà identiques.
pub fn install(plugins: &Path) -> Installed {
    if !plugins.is_dir() {
        return Installed::NoStreamDeck;
    }
    let root = plugins.join(FOLDER);
    let mut written = false;
    for (rel, bytes) in FILES {
        let path = root.join(rel);
        if std::fs::read(&path).is_ok_and(|have| have == *bytes) {
            continue;
        }
        let made = path.parent().is_none_or(|d| std::fs::create_dir_all(d).is_ok());
        if !made || std::fs::write(&path, bytes).is_err() {
            return Installed::Failed;
        }
        written = true;
    }
    if written { Installed::Written } else { Installed::Current }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_read_and_written() {
        assert_eq!(parse(r#"{"do":"mute"}"#), Some(Do::Mute));
        assert_eq!(parse("{\"do\":\"decline\"}\r\n"), Some(Do::Decline));
        assert_eq!(parse(r#"{"do":"talk","on":true}"#), Some(Do::Talk(true)));
        assert_eq!(parse(r#"{"do":"talk","on":false}"#), Some(Do::Talk(false)));
        assert_eq!(parse(r#"{"do":"sounds"}"#), Some(Do::Sounds));
        assert_eq!(parse(r#"{"do":"sound","id":"1234567890123456789","guild":"42"}"#), Some(Do::Sound { id: 1_234_567_890_123_456_789, guild: Some(42) }));
        assert_eq!(parse(r#"{"do":"sound","id":"7","guild":null}"#), Some(Do::Sound { id: 7, guild: None }));
        assert_eq!(parse(r#"{"do":"sound","id":7}"#), None, "identifiant en chaîne seulement");
        let l = sounds_line(&[SoundRef { id: 1_234_567_890_123_456_789, guild: None, name: "Tada".into() }]);
        assert_eq!(l, "{\"sounds\":[{\"guild\":null,\"id\":\"1234567890123456789\",\"name\":\"Tada\"}]}\n");
        for bad in ["", "mute", r#"{"do":"send"}"#, r#"{"do":1}"#, r#"{"cmd":"mute"}"#, r#"{"do":"talk"}"#] {
            assert_eq!(parse(bad), None, "{bad}");
        }
        let s = State { voice: true, mute: true, ..Default::default() }.line();
        assert!(s.ends_with('\n'));
        let v: serde_json::Value = serde_json::from_str(&s).unwrap_or_default();
        assert_eq!((v["v"].as_u64(), v["voice"].as_bool(), v["mute"].as_bool(), v["deaf"].as_bool()), (Some(1), Some(true), Some(true), Some(false)));
    }

    #[test]
    fn plugin_files_match_the_manifest_and_install_once() {
        // Chaque image et le script nommés par le manifeste sont embarqués.
        let manifest: serde_json::Value = serde_json::from_slice(FILES[0].1).unwrap_or_default();
        let has = |p: &str| FILES.iter().any(|(f, _)| *f == p);
        assert!(has(manifest["CodePath"].as_str().unwrap_or("?")));
        let actions = manifest["Actions"].as_array().cloned().unwrap_or_default();
        assert_eq!(actions.len(), 9);
        assert!(has(actions.iter().find_map(|a| a["PropertyInspectorPath"].as_str()).unwrap_or("?")));
        for a in &actions {
            let id = a["UUID"].as_str().unwrap_or_default().rsplit('.').next().unwrap_or_default().to_string();
            assert!(parse(&format!(r#"{{"do":"{id}","on":true,"id":"1"}}"#)).is_some(), "action inconnue de Ferrocord : {id}");
            assert!(has(&format!("{}.svg", a["Icon"].as_str().unwrap_or("?"))), "{id}");
            for s in a["States"].as_array().cloned().unwrap_or_default() {
                assert!(has(&format!("{}.svg", s["Image"].as_str().unwrap_or("?"))), "{id}");
            }
        }

        let dir = std::env::temp_dir().join(format!("ferrocord-deck-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(install(&dir), Installed::NoStreamDeck, "pas de dossier : rien n'est créé");
        assert!(!dir.exists());
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(install(&dir), Installed::Written);
        assert_eq!(install(&dir), Installed::Current);
        assert!(dir.join(FOLDER).join("bin").join("plugin.js").is_file());
        std::fs::write(dir.join(FOLDER).join("manifest.json"), b"vieux").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(install(&dir), Installed::Written, "une version différente est remplacée");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_local_program_gets_the_state_and_asks_for_actions() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::windows::named_pipe::ClientOptions;
        let name = format!(r"\\.\pipe\ferrocord-test-commande-{}", std::process::id());
        let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let w = woken.clone();
        let mut deck = Deck::start_on(name.clone(), Arc::new(move || {
            w.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }))
        .unwrap_or_else(|| panic!("serveur"));
        let client = loop {
            match ClientOptions::new().open(&name) {
                Ok(c) => break c,
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(10)).await,
            }
        };
        let (r, mut wr) = tokio::io::split(client);
        let mut lines = BufReader::new(r).lines();
        let read = |l: Option<String>| serde_json::from_str::<serde_json::Value>(&l.unwrap_or_default()).unwrap_or_default();
        let first = read(lines.next_line().await.unwrap_or_default());
        assert_eq!((first["voice"].as_bool(), first["mute"].as_bool()), (Some(false), Some(false)));

        deck.publish(State { voice: true, mute: true, ..Default::default() });
        let next = read(lines.next_line().await.unwrap_or_default());
        assert_eq!((next["voice"].as_bool(), next["mute"].as_bool()), (Some(true), Some(true)));

        wr.write_all(b"{\"do\":\"mute\"}\nn'importe quoi\n{\"do\":\"leave\"}\n").await.unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..500 {
            if woken.load(std::sync::atomic::Ordering::SeqCst) >= 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(deck.take(), vec![Do::Mute, Do::Leave]);
        assert!(deck.take().is_empty());

        // Débranché touche enfoncée : l'appui-pour-parler est relâché.
        wr.write_all(b"{\"do\":\"talk\",\"on\":true}\n").await.unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..500 {
            if woken.load(std::sync::atomic::Ordering::SeqCst) >= 3 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        drop((lines, wr));
        for _ in 0..500 {
            if woken.load(std::sync::atomic::Ordering::SeqCst) >= 4 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(deck.take(), vec![Do::Talk(true), Do::Talk(false)]);

        // Un programme qui arrive après reçoit l'état puis la liste des sons.
        deck.publish_sounds(&[SoundRef { id: 5, guild: Some(6), name: "Clairon".into() }]);
        let late = loop {
            match ClientOptions::new().open(&name) {
                Ok(c) => break c,
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(10)).await,
            }
        };
        let mut lines = BufReader::new(late).lines();
        let _state = lines.next_line().await;
        let list = read(lines.next_line().await.unwrap_or_default());
        assert_eq!((list["sounds"][0]["id"].as_str(), list["sounds"][0]["guild"].as_str(), list["sounds"][0]["name"].as_str()), (Some("5"), Some("6"), Some("Clairon")));
    }
}
