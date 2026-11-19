//! Le bureau de Windows pour de vrai : toasts et sons (WinRT) dans le fil
//! « ferrocord-bureau », icône de la zone de notification dans le fil de
//! l'interface (Shell_NotifyIcon doit y être appelé : c'est lui qui reçoit
//! ses messages).
//!
//! Notifications d'une application non empaquetée : Windows les affiche sous
//! le nom et l'icône inscrits dans la clé du registre de l'utilisateur
//! `HKCU\Software\Classes\AppUserModelId\Ferrocord` (DisplayName, IconUri),
//! écrite une fois, au premier toast seulement.

use super::{icon, xml, DeskEvent, Desktop, TrayState, STATUSES};
use crate::i18n::table::notify::{p as np, t as nt};
use crate::i18n::table::social::t as so;
use crate::images::{decode, disk::Disk, DiskConfig, Source};
use crate::net::Wake;
use crate::state::model::ImageRef;
use crate::audio::sons::perso::Banque;
use crate::state::notify::{Alert, DeskPrefs, Kind};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use tray_icon::menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::core::HSTRING;
use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::TypedEventHandler;
use windows::Media::Core::MediaSource;
use windows::Media::Playback::{MediaPlaybackState, MediaPlayer};
use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager, ToastNotifier};

/// Identifiant d'application des notifications (AppUserModelID).
const AUMID: &str = "Ferrocord";
/// Toasts gardés en vie (leur clic doit encore arriver), au plus.
const LIVE_TOASTS: usize = 8;
/// Fichiers d'avatar des toasts, réutilisés en rond : le dossier reste borné.
const AVATAR_SLOTS: u32 = 16;
/// Tailles CDN essayées dans le cache d'images pour l'avatar (puissances de deux).
const AVATAR_SIZES: [u32; 5] = [64, 128, 32, 256, 16];

enum Job {
    Load,
    Save(DeskPrefs),
    Toast(Alert, [u8; 3]),
    Sound(Kind, u8),
    Silence(Kind),
}

/// Bureau de Windows.
pub struct Desk {
    jobs: mpsc::Sender<Job>,
    events: mpsc::Receiver<DeskEvent>,
    tray: Option<Tray>,
    accent: [u8; 3],
}

impl Desk {
    /// Démarre le fil du bureau. `source` et `images` : adresses et dossier du
    /// cache d'images (avatars déjà téléchargés ; rien n'est téléchargé ici).
    pub fn start(wake: Wake, source: Arc<dyn Source>, images: Option<PathBuf>) -> Desk {
        let (jobs, rx) = mpsc::channel();
        let (tx, events) = mpsc::channel();
        install_handlers(tx.clone(), wake.clone());
        let worker = Worker { tx, wake, source, images, disk: None, notifier: None, registered: false, live: VecDeque::new(), players: Vec::new(), sons: Banque::new(crate::audio::sons::perso::dossier()), slot: 0 };
        // Sans fil, les demandes sont perdues sans bruit : l'application reste utilisable.
        let _ = std::thread::Builder::new().name("ferrocord-bureau".into()).spawn(move || worker.run(rx));
        Desk { jobs, events, tray: None, accent: [0, 0, 0] }
    }
}

impl Desktop for Desk {
    fn load_prefs(&mut self) {
        let _ = self.jobs.send(Job::Load);
    }

    fn save_prefs(&mut self, prefs: DeskPrefs) {
        let _ = self.jobs.send(Job::Save(prefs));
    }

    fn toast(&mut self, alert: &Alert) {
        let _ = self.jobs.send(Job::Toast(alert.clone(), self.accent));
    }

    fn sound(&mut self, kind: Kind, volume: u8) {
        let _ = self.jobs.send(Job::Sound(kind, volume));
    }

    fn silence(&mut self, kind: Kind) {
        let _ = self.jobs.send(Job::Silence(kind));
    }

    fn custom_sounds(&self) -> bool {
        true
    }

    fn tray(&mut self, state: &TrayState) {
        self.accent = state.accent;
        if !state.visible {
            // Retirer l'objet retire l'icône.
            self.tray = None;
            return;
        }
        match &mut self.tray {
            Some(t) => t.update(state),
            None => self.tray = Tray::new(state),
        }
    }

    fn poll(&mut self) -> Vec<DeskEvent> {
        self.events.try_iter().collect()
    }
}

/// Clics sur l'icône et son menu : envoyés à l'interface, qui est réveillée.
/// Le survol de l'icône (événements Enter/Move/Leave) ne réveille rien.
fn install_handlers(tx: mpsc::Sender<DeskEvent>, wake: Wake) {
    let (tx2, wake2) = (tx.clone(), wake.clone());
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
            let _ = tx2.send(DeskEvent::TrayToggle);
            wake2();
        }
    }));
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let ev = match e.id.as_ref() {
            "show" => DeskEvent::TrayShow,
            "hide" => DeskEvent::TrayHide,
            "quit" => DeskEvent::TrayQuit,
            id => match STATUSES.iter().find(|s| id.strip_prefix("status:") == Some(**s)) {
                Some(s) => DeskEvent::TrayStatus(s),
                None => return,
            },
        };
        let _ = tx.send(ev);
        wake();
    }));
}

struct Tray {
    icon: TrayIcon,
    state: TrayState,
}

fn tray_icon(state: &TrayState) -> Option<tray_icon::Icon> {
    let badge = (state.mentions > 0).then_some(state.badge);
    tray_icon::Icon::from_rgba(icon::rgba(icon::TRAY_SIDE, state.accent, badge), icon::TRAY_SIDE, icon::TRAY_SIDE).ok()
}

fn tooltip(state: &TrayState) -> String {
    match state.mentions {
        0 => crate::state::notify::APP_NAME.to_string(),
        n => format!("{} — {}", crate::state::notify::APP_NAME, np::unread_mentions(state.lang, u64::from(n))),
    }
}

fn menu(state: &TrayState) -> Option<Menu> {
    let lang = state.lang;
    let toggle = if state.window_shown { MenuItem::with_id("hide", nt::TRAY_HIDE.get(lang), true, None) } else { MenuItem::with_id("show", nt::TRAY_SHOW.get(lang), true, None) };
    let names = [so::STATUS_ONLINE, so::STATUS_IDLE, so::STATUS_DND, crate::i18n::table::s::t::STATUS_INVISIBLE];
    let known = state.status.is_some();
    let items: Vec<CheckMenuItem> = STATUSES
        .iter()
        .zip(names)
        .map(|(s, name)| CheckMenuItem::with_id(format!("status:{s}"), name.get(lang), known, state.status.as_deref() == Some(*s), None))
        .collect();
    let refs: Vec<&dyn IsMenuItem> = items.iter().map(|i| i as &dyn IsMenuItem).collect();
    let status = Submenu::with_items(nt::TRAY_STATUS.get(lang), known, &refs).ok()?;
    let quit = MenuItem::with_id("quit", nt::TRAY_QUIT.get(lang), true, None);
    Menu::with_items(&[&toggle, &status, &PredefinedMenuItem::separator(), &quit]).ok()
}

impl Tray {
    fn new(state: &TrayState) -> Option<Tray> {
        let mut b = TrayIconBuilder::new().with_tooltip(tooltip(state)).with_menu_on_left_click(false);
        if let Some(i) = tray_icon(state) {
            b = b.with_icon(i);
        }
        if let Some(m) = menu(state) {
            b = b.with_menu(Box::new(m));
        }
        Some(Tray { icon: b.build().ok()?, state: state.clone() })
    }

    fn update(&mut self, state: &TrayState) {
        let old = &self.state;
        if old == state {
            return;
        }
        if (old.mentions > 0) != (state.mentions > 0) || old.accent != state.accent || old.badge != state.badge {
            let _ = self.icon.set_icon(tray_icon(state));
        }
        if old.mentions != state.mentions || old.lang != state.lang {
            let _ = self.icon.set_tooltip(Some(tooltip(state)));
        }
        if old.window_shown != state.window_shown || old.status != state.status || old.lang != state.lang {
            self.icon.set_menu(menu(state).map(|m| Box::new(m) as Box<dyn tray_icon::menu::ContextMenu>));
        }
        self.state = state.clone();
    }
}

struct Worker {
    tx: mpsc::Sender<DeskEvent>,
    wake: Wake,
    source: Arc<dyn Source>,
    images: Option<PathBuf>,
    disk: Option<Disk>,
    notifier: Option<ToastNotifier>,
    registered: bool,
    live: VecDeque<ToastNotification>,
    /// Lecteurs des sons, du moins récemment servi au plus récent.
    players: Vec<(Kind, MediaPlayer)>,
    /// Sons fabriqués à la demande (rien tant qu'aucun son n'est joué).
    sons: Banque,
    slot: u32,
}

impl Worker {
    /// Attend les demandes (bloqué sur la file : aucun réveil au repos).
    fn run(mut self, rx: mpsc::Receiver<Job>) {
        for job in rx {
            match job {
                Job::Load => {
                    let prefs = super::default_path().map(|p| super::read_prefs(&p)).unwrap_or_default();
                    self.send(DeskEvent::Prefs(prefs));
                }
                Job::Save(p) => {
                    if let Some(path) = super::default_path()
                        && let Err(e) = super::write_prefs(&path, &p)
                    {
                        self.send(DeskEvent::SaveFailed(e.to_string()));
                    }
                }
                Job::Toast(a, accent) => {
                    let _ = self.show(&a, accent);
                }
                Job::Sound(kind, volume) => {
                    let _ = self.play(kind, volume);
                }
                Job::Silence(kind) => self.silence(kind),
            }
        }
    }

    fn send(&self, e: DeskEvent) {
        let _ = self.tx.send(e);
        (self.wake)();
    }

    /// Nom et icône des notifications, inscrits une fois par lancement.
    fn register(&mut self, accent: [u8; 3]) {
        if self.registered {
            return;
        }
        self.registered = true;
        let logo = crate::platform::config_dir().map(|d| d.join("ferrocord").join("ferrocord.png"));
        let logo = logo.filter(|p| write_png(p, icon::LOGO_SIDE, icon::rgba(icon::LOGO_SIDE, accent, None)).is_ok());
        if let Ok(key) = windows_registry::CURRENT_USER.create(format!(r"Software\Classes\AppUserModelId\{AUMID}")) {
            let _ = key.set_string("DisplayName", crate::state::notify::APP_NAME);
            if let Some(p) = logo {
                let _ = key.set_string("IconUri", p.to_string_lossy());
            }
        }
    }

    fn show(&mut self, a: &Alert, accent: [u8; 3]) -> windows::core::Result<()> {
        self.register(accent);
        if self.notifier.is_none() {
            self.notifier = Some(ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?);
        }
        let avatar = a.avatar.and_then(|r| self.avatar_file(&r)).map(|p| xml::file_uri(&p));
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml::toast(a, avatar.as_deref())))?;
        let toast = ToastNotification::CreateToastNotification(&doc)?;
        // Même étiquette par salon : la nouvelle remplace l'ancienne dans le centre de notifications.
        toast.SetTag(&HSTRING::from(a.channel.to_string()))?;
        toast.SetGroup(&HSTRING::from(AUMID))?;
        let (tx, wake, guild, channel) = (self.tx.clone(), self.wake.clone(), a.guild, a.channel);
        toast.Activated(&TypedEventHandler::new(move |_, _| {
            let _ = tx.send(DeskEvent::Open { guild, channel });
            wake();
            Ok(())
        }))?;
        if let Some(n) = &self.notifier {
            n.Show(&toast)?;
        }
        if self.live.len() >= LIVE_TOASTS {
            self.live.pop_front();
        }
        self.live.push_back(toast);
        Ok(())
    }

    /// Avatar déjà dans le cache d'images, converti en PNG (les toasts ne
    /// lisent pas le WebP du CDN). Rien n'est téléchargé.
    fn avatar_file(&mut self, image: &ImageRef) -> Option<PathBuf> {
        let dir = self.images.clone()?;
        if self.disk.is_none() {
            self.disk = Disk::open(DiskConfig::new(dir.clone())).ok();
        }
        let disk = self.disk.as_ref()?;
        let bytes = AVATAR_SIZES.iter().find_map(|s| disk.get(&self.source.url(image, *s)))?;
        let img = decode::decode(&bytes, decode::MIN_ACCEPTED_SIDE).ok()?.thumbnail(64, 64).to_rgba8();
        let (w, h) = img.dimensions();
        self.slot = (self.slot + 1) % AVATAR_SLOTS;
        let path = dir.parent()?.join("toasts").join(format!("avatar-{}.png", self.slot));
        write_png(&path, w.max(h), img.into_raw()).ok().filter(|_| w == h).map(|_| path)
    }

    fn play(&mut self, kind: Kind, volume: u8) -> windows::core::Result<()> {
        // Fabriqué (ou lu dans le dossier de l'utilisateur) ici, hors du fil d'interface.
        let son = self.sons.son(kind);
        let source = media_source(&son.wav)?;
        let player = self.player(kind)?;
        player.SetSource(&source)?;
        player.SetVolume(f64::from(volume.min(100)) / 100.0)?;
        player.Play()
    }

    /// Lecteur pour `kind` : le sien s'il en a déjà un (le son repart du
    /// début), sinon un lecteur au repos, sinon un nouveau ; au-delà de
    /// [`PLAYERS`], le plus ancien est repris. Une arrivée dans le salon ne
    /// coupe donc pas la sonnerie.
    fn player(&mut self, kind: Kind) -> windows::core::Result<MediaPlayer> {
        let at = match self.players.iter().position(|(k, _)| *k == kind).or_else(|| self.players.iter().position(|(_, p)| !playing(p))) {
            Some(i) => i,
            None if self.players.len() < PLAYERS => {
                let player = MediaPlayer::new()?;
                // Pas d'entrée dans les commandes multimédias de Windows (touches
                // lecture/pause, volet du volume) pour un simple son d'interface.
                let _ = player.CommandManager().and_then(|c| c.SetIsEnabled(false));
                self.players.push((kind, player));
                self.players.len() - 1
            }
            None => 0,
        };
        let (_, player) = self.players.remove(at);
        self.players.push((kind, player.clone()));
        Ok(player)
    }

    /// Arrête le son de `kind` s'il joue encore.
    fn silence(&mut self, kind: Kind) {
        for (_, p) in self.players.iter().filter(|(k, _)| *k == kind) {
            let _ = p.Pause();
        }
    }
}

/// Lecteurs gardés, au plus : sonnerie, son du vocal et notification peuvent se chevaucher.
const PLAYERS: usize = 3;

fn playing(p: &MediaPlayer) -> bool {
    let state = p.PlaybackSession().and_then(|s| s.PlaybackState());
    matches!(state, Ok(MediaPlaybackState::Playing | MediaPlaybackState::Opening | MediaPlaybackState::Buffering))
}

/// Source de lecture d'un fichier WAV tenu en mémoire (aucun fichier temporaire).
fn media_source(wav: &[u8]) -> windows::core::Result<MediaSource> {
    let stream = InMemoryRandomAccessStream::new()?;
    let writer = DataWriter::CreateDataWriter(&stream)?;
    writer.WriteBytes(wav)?;
    writer.StoreAsync()?.join()?;
    // Le flux reste ouvert quand l'écrivain est lâché.
    writer.DetachStream()?;
    stream.Seek(0)?;
    MediaSource::CreateFromStream(&stream, &HSTRING::from("audio/wav"))
}

/// Écrit une image carrée RGBA en PNG (fichier voisin puis renommage).
fn write_png(path: &Path, side: u32, rgba: Vec<u8>) -> image::ImageResult<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let img = image::RgbaImage::from_raw(side, side, rgba).ok_or_else(|| image::ImageError::Parameter(image::error::ParameterError::from_kind(image::error::ParameterErrorKind::DimensionMismatch)))?;
    let tmp = path.with_extension("png.tmp");
    img.save_with_format(&tmp, image::ImageFormat::Png)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows sait ouvrir nos WAV depuis la mémoire et y lit la bonne durée.
    /// La source est seulement ouverte : aucun lecteur, aucune sortie audio.
    #[test]
    fn windows_ouvre_les_sons_depuis_la_memoire() {
        for kind in [Kind::VoiceMute, Kind::Ring] {
            let pcm = crate::audio::sons::integre(kind);
            let source = media_source(&crate::audio::sons::synth::wav(&pcm)).unwrap();
            source.OpenAsync().unwrap().join().unwrap();
            let ticks = source.Duration().unwrap().Value().unwrap().Duration;
            let voulu = (pcm.len() / 2) as i64 * 10_000_000 / i64::from(crate::audio::sons::synth::FREQ);
            assert!((ticks - voulu).abs() < 100_000, "{kind:?} : {ticks} pour {voulu}");
        }
    }
}
