//! Bureau de Windows : notifications (toasts), sons, icône de la zone de
//! notification, et le fichier `notify.toml`. L'interface ne parle qu'au
//! trait [`Desktop`] ; ce qui touche au système ou au disque se fait dans le
//! fil « ferrocord-bureau » (bloqué sur sa file : aucun réveil au repos),
//! sauf l'icône, qui doit vivre dans le fil de la boucle d'événements.
//!
//! Aucun `unsafe` ici : les API WinRT du crate `windows` (toasts, lecteur
//! multimédia), `windows-registry` et `tray-icon` offrent des interfaces
//! sûres ; leur `unsafe` reste dans ces bibliothèques.

pub mod icon;
/// Fichier `keys.toml` des raccourcis clavier (lot navigation).
pub mod keys;
#[cfg(windows)]
mod system;
pub mod xml;

use crate::state::notify::{Alert, DeskPrefs, Kind};
use std::sync::{Arc, Mutex};

#[cfg(windows)]
pub use system::Desk;

/// Ce que le bureau renvoie à l'interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeskEvent {
    /// `notify.toml` lu (ou absent : valeurs par défaut).
    Prefs(DeskPrefs),
    /// Écriture de `notify.toml` ratée (raison).
    SaveFailed(String),
    /// Clic sur une notification : ramener la fenêtre et ouvrir ce salon.
    Open { guild: Option<u64>, channel: u64 },
    /// Clic gauche sur l'icône : afficher ou masquer la fenêtre.
    TrayToggle,
    TrayShow,
    TrayHide,
    /// Statut choisi dans le menu de l'icône (`online`, `idle`, `dnd`, `invisible`).
    TrayStatus(&'static str),
    TrayQuit,
}

/// Ce que montre l'icône de la zone de notification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayState {
    pub visible: bool,
    /// Mentions non lues (0 : pas de pastille).
    pub mentions: u32,
    /// Fenêtre affichée (le menu propose alors « Masquer »).
    pub window_shown: bool,
    pub lang: crate::i18n::Lang,
    /// Statut actuel (`online`, `idle`, `dnd`, `invisible`), s'il est connu.
    pub status: Option<String>,
    /// Couleurs de l'icône et de la pastille (jetons du thème), RVB.
    pub accent: [u8; 3],
    pub badge: [u8; 3],
}

/// Statuts proposés par le menu de l'icône, dans l'ordre de Discord.
pub const STATUSES: [&str; 4] = ["online", "idle", "dnd", "invisible"];

/// Le bureau, vu de l'interface.
pub trait Desktop {
    /// Lit `notify.toml` ; la réponse arrive par [`DeskEvent::Prefs`].
    fn load_prefs(&mut self);
    fn save_prefs(&mut self, prefs: DeskPrefs);
    /// Montre une notification (sans son : les sons sont à part).
    fn toast(&mut self, alert: &Alert);
    /// Joue le son de `kind` au volume donné (0 à 100), sans attendre.
    fn sound(&mut self, kind: Kind, volume: u8);
    /// Met l'icône dans cet état (rien n'est fait s'il n'a pas changé).
    fn tray(&mut self, state: &TrayState);
    /// Événements arrivés depuis le dernier appel.
    fn poll(&mut self) -> Vec<DeskEvent>;
}

/// Ce qu'un [`Recorder`] a reçu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    Load,
    Save(DeskPrefs),
    Toast(Alert),
    Sound(Kind, u8),
    Tray(TrayState),
}

/// Bureau sans système (tests, aperçu) : retient ce qu'on lui demande, et
/// rend les événements qu'on lui donne.
#[derive(Clone, Default)]
pub struct Recorder {
    pub calls: Arc<Mutex<Vec<Call>>>,
    pub events: Arc<Mutex<Vec<DeskEvent>>>,
}

impl Recorder {
    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().map(|c| c.clone()).unwrap_or_default()
    }

    pub fn push(&self, e: DeskEvent) {
        if let Ok(mut v) = self.events.lock() {
            v.push(e);
        }
    }

    fn record(&self, c: Call) {
        if let Ok(mut v) = self.calls.lock() {
            v.push(c);
        }
    }
}

impl Desktop for Recorder {
    fn load_prefs(&mut self) {
        self.record(Call::Load);
    }

    fn save_prefs(&mut self, prefs: DeskPrefs) {
        self.record(Call::Save(prefs));
    }

    fn toast(&mut self, alert: &Alert) {
        self.record(Call::Toast(alert.clone()));
    }

    fn sound(&mut self, kind: Kind, volume: u8) {
        self.record(Call::Sound(kind, volume));
    }

    fn tray(&mut self, state: &TrayState) {
        self.record(Call::Tray(state.clone()));
    }

    fn poll(&mut self) -> Vec<DeskEvent> {
        self.events.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default()
    }
}

/// Chemin de `notify.toml`, à côté de `theme.toml`.
pub fn default_path() -> Option<std::path::PathBuf> {
    Some(crate::platform::config_dir()?.join("ferrocord").join("notify.toml"))
}

/// Lit `notify.toml` (absent : valeurs par défaut ; syntaxe fausse : valeurs
/// par défaut aussi, le fichier sera réécrit au prochain changement).
pub fn read_prefs(path: &std::path::Path) -> DeskPrefs {
    std::fs::read_to_string(path).ok().and_then(|t| DeskPrefs::parse(&t).ok()).unwrap_or_default()
}

/// Écrit `notify.toml` dans un fichier voisin puis le renomme : jamais lu à moitié écrit.
pub fn write_prefs(path: &std::path::Path, prefs: &DeskPrefs) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("toml.tmp");
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(prefs.to_toml().as_bytes())?;
        f.sync_all()
    })();
    let result = written.and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefs_file_missing_or_damaged_gives_defaults() {
        let dir = std::env::temp_dir().join(format!("ferrocord-desk-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("notify.toml");
        let _ = std::fs::remove_file(&path);
        assert_eq!(read_prefs(&path), DeskPrefs::default());
        let p = DeskPrefs { volume: 12, ..DeskPrefs::default() };
        write_prefs(&path, &p).unwrap();
        assert_eq!(read_prefs(&path), p);
        std::fs::write(&path, "volume = ").unwrap();
        assert_eq!(read_prefs(&path), DeskPrefs::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recorder_keeps_calls_and_hands_events_once() {
        let mut r = Recorder::default();
        r.sound(Kind::Mention, 40);
        r.push(DeskEvent::TrayQuit);
        assert_eq!(r.calls(), vec![Call::Sound(Kind::Mention, 40)]);
        assert_eq!(r.poll(), vec![DeskEvent::TrayQuit]);
        assert!(r.poll().is_empty());
    }
}
