//! Pipeline d'images : demandes de l'interface, téléchargement (par
//! `net::cdn`), cache disque, décodage hors du fil d'interface, textures.
//!
//! L'interface n'appelle que [`Images::get`], qui ne bloque jamais : il
//! renvoie la texture si elle est prête, sinon lance (une fois) son
//! chargement et renvoie `EnAttente`. Quand des images arrivent,
//! l'interface est réveillée une seule fois ; rien n'est redessiné en
//! boucle pendant un chargement. Au plus [`MemoryConfig::uploads_per_frame`]
//! textures sont envoyées au processeur graphique par image ; le reste
//! attend l'image suivante (un redessin est alors demandé).
//!
//! Politique d'éviction des textures (mémoire) : quand les octets des
//! textures dépassent `MemoryConfig::max_bytes`, les textures affichées le
//! moins récemment sont libérées jusqu'à redescendre à 90 % du plafond (un
//! seul tri par dépassement) ; jamais celles de l'image en cours ni, au début
//! d'une image, celles de la précédente : ce qui est à l'écran reste, même
//! au-dessus du plafond (un profil dont la bannière et l'avatar sont de
//! longues animations le dépasse à lui seul), sinon l'écran se viderait et
//! se rechargerait sans fin. Une texture non affichée
//! depuis `MemoryConfig::idle` est libérée même sous le plafond ; ce
//! parcours-là n'a lieu qu'une fois par quart de `idle` (au plus toutes les
//! 10 s), pas à chaque image. Rien ne tourne entre deux images : une fenêtre
//! au repos ne libère rien, mais ne consomme rien non plus.
//!
//! Une image en échec réseau est redemandée au premier dessin qui suit la fin
//! de son délai : aucun réveil n'est programmé pour cela.
//!
//! Lots suivants (pièces jointes, emojis, GIF) : ajouter une variante à
//! `ImageRef`, son adresse dans `net::cdn`, et demander avec un autre
//! [`Fit`] ; l'ordonnanceur, les caches et les limites sont communs.

pub mod anim;
pub mod decode;
pub mod disk;
mod webm;
mod worker;

pub use decode::Fit;
pub use disk::DiskConfig;
pub use worker::{Failure, WorkerConfig, WorkerStats};

use crate::net::cdn::{self, Cdn, FetchError};
use crate::state::model::ImageRef;
use eframe::egui::{self, TextureHandle, TextureId, TextureOptions};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
use tokio::{runtime::Handle, sync::mpsc as tmpsc};
use worker::{FromWorker, Job, Probe, Reply, Shared, Ticket, ToWorker};

/// Forme qui dessine la texture `tex` dans `rect`, coins arrondis de `radius`.
pub fn textured(rect: egui::Rect, radius: egui::CornerRadius, tex: TextureId) -> egui::Shape {
    textured_part(rect, radius, tex, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)))
}

/// Comme [`textured`], pour la partie `uv` de la texture (0 à 1 sur chaque axe).
pub fn textured_part(rect: egui::Rect, radius: egui::CornerRadius, tex: TextureId, uv: egui::Rect) -> egui::Shape {
    // Blanc = la texture telle quelle, sans teinte.
    egui::Shape::Rect(egui::epaint::RectShape::filled(rect, radius, egui::Color32::WHITE).with_texture(tex, uv))
}

/// Une image à une taille donnée : clé des caches et des demandes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub image: ImageRef,
    pub fit: Fit,
    /// Toutes les images d'une animation (sinon : la première seule).
    pub anim: bool,
}

pub type FetchFuture = Pin<Box<dyn Future<Output = Result<Vec<u8>, FetchError>> + Send>>;

/// D'où viennent les octets des images : le CDN de Discord, ou une source
/// locale (aperçu hors ligne, tests).
pub trait Source: Send + Sync + 'static {
    /// Adresse de `image` à la taille CDN `size` (puissance de deux).
    fn url(&self, image: &ImageRef, size: u32) -> String;
    /// Adresse d'une image de pièce jointe ou d'embed (`url` : reçue avec le
    /// message) redimensionnée à `width` × `height` pixels.
    fn media_url(&self, url: &str, width: u32, height: u32) -> String {
        cdn::media_url(url, width, height)
    }
    /// Comme [`Source::media_url`], pour une image fixe : copie sans perte
    /// de qualité ([`cdn::still_url`]). Une source locale garde `media_url`.
    fn still_url(&self, url: &str, width: u32, height: u32) -> String {
        self.media_url(url, width, height)
    }
    fn fetch(&self, url: String) -> FetchFuture;
    /// Adresse de la version animée de `image` (GIF, WebP animé, WebM d'une
    /// plaque), s'il en existe une ; `None` : l'image est fixe.
    fn anim_url(&self, _image: &ImageRef, _size: u32) -> Option<String> {
        None
    }
}

impl Source for Cdn {
    fn url(&self, image: &ImageRef, size: u32) -> String {
        Cdn::url(self, image, size)
    }

    fn still_url(&self, url: &str, width: u32, height: u32) -> String {
        cdn::still_url(url, width, height)
    }

    fn anim_url(&self, image: &ImageRef, size: u32) -> Option<String> {
        Cdn::anim_url(self, image, size)
    }

    fn fetch(&self, url: String) -> FetchFuture {
        let cdn = self.clone();
        Box::pin(async move { cdn.fetch(&url).await })
    }
}

/// Plafonds du cache de textures.
#[derive(Clone, Copy, Debug)]
pub struct MemoryConfig {
    /// Octets de textures (largeur × hauteur × 4) au plus, hors image en cours.
    pub max_bytes: usize,
    /// Une texture non affichée depuis ce délai est libérée.
    pub idle: Duration,
    /// Textures envoyées au processeur graphique par image, au plus.
    pub uploads_per_frame: usize,
}

/// Octets d'une seule texture de la visionneuse au plus (1 770 × 1 770 pixels) :
/// l'image en cours n'est jamais évincée, même au-dessus de `max_bytes`.
pub const VIEWER_MAX_BYTES: usize = 12 << 20;

/// Grand côté demandé au plus pour une animation recadrée
/// ([`Images::get_fill_moving`]) : celui de la taille que le CDN sert pour une
/// bannière de profil complet. Le décodeur réduit ensuite les images selon
/// leur nombre (`anim::fill_plan`) ; le processeur graphique étire le reste.
pub const ANIM_FILL_SIDE: u32 = 512;

/// Côté accepté au plus pour une image servie à taille fixe ([`Images::get_asset`]).
pub const ASSET_MAX_SIDE: u32 = 2048;

/// Côté accepté au moins pour une image recadrée : une plaque nominative est
/// servie à taille fixe, quelle que soit la taille demandée.
const FILL_MIN_ACCEPTED: u32 = 1024;

impl Default for MemoryConfig {
    fn default() -> Self {
        // 16 Mo et non 48 (lot M) : le pilote OpenGL garde une copie de chaque
        // texture dans la mémoire du processus ; au banc, 32 Mo de textures de
        // moins font 64 Mo de mémoire privée de moins en fin de parcours.
        // Un écran plein d'images en tient environ 6 Mo (1 600 × 900 × 4).
        MemoryConfig { max_bytes: 16 << 20, idle: Duration::from_secs(120), uploads_per_frame: 32 }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Config {
    pub memory: MemoryConfig,
    pub worker: WorkerConfig,
}

/// Dossier du cache disque : `<cache>/ferrocord/images`.
pub fn default_dir() -> Option<PathBuf> {
    Some(crate::platform::cache_dir()?.join("ferrocord").join("images"))
}

/// État d'une image demandée.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Image {
    Ready(TextureId),
    /// En cours de chargement : dessiner le remplaçant.
    Pending,
    /// Indisponible : dessiner le remplaçant.
    Failed,
}

/// Chiffres du cache, pour les tests et les mesures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub bytes: usize,
    pub textures: usize,
    pub pending: usize,
    pub failed: usize,
}

enum Entry {
    Pending,
    Ready { tex: TextureHandle, bytes: usize, last_frame: u64, last_used: Instant },
    /// Animation : `tex` montre l'image `player.shown` ; toutes les images
    /// restent en mémoire (comptées dans `bytes`).
    Anim { tex: TextureHandle, frames: Vec<Arc<egui::ColorImage>>, delays: Vec<Duration>, player: anim::Player, bytes: usize, last_frame: u64, last_used: Instant },
    /// `retry_at` : `None` = définitif pour la session. `seen` : dernière
    /// image où il a été demandé.
    Failed { retry_at: Option<Instant>, seen: u64 },
}

/// Couleurs moyennes d'avatars gardées au plus (au-delà, la table repart de zéro).
const MAX_TINTS: usize = 2048;

/// Couleur moyenne des pixels opaques de `img` (un pixel sur quelques-uns suffit).
fn mean_color(img: &egui::ColorImage) -> Option<[u8; 3]> {
    let step = (img.pixels.len() / 4096).max(1);
    let (mut sum, mut n) = ([0u64; 3], 0u64);
    for p in img.pixels.iter().step_by(step).filter(|p| p.a() > 127) {
        for (s, v) in sum.iter_mut().zip([p.r(), p.g(), p.b()]) {
            *s += u64::from(v);
        }
        n += 1;
    }
    (n > 0).then(|| sum.map(|s| (s / n) as u8))
}

/// Entrées gardées avant qu'on oublie les échecs qui ne sont plus affichés.
const MAX_ENTRIES: usize = 8192;
/// Intervalle le plus long entre deux recherches de textures inutilisées.
const IDLE_SCAN_MAX: Duration = Duration::from_secs(10);

/// Mémoire serrée signalée par le système (Android : `onLowMemory`, via
/// winit) : consommé à la fin de l'image suivante.
#[cfg_attr(test, allow(dead_code))]
static MEMORY_LOW: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
thread_local! {
    /// Tests : un signal par fil, pour ne pas vider les caches des tests voisins.
    static MEMORY_LOW_TEST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Le système manque de mémoire : à la fin de la prochaine image, toutes
/// les textures qui n'y sont pas affichées sont libérées (écrans quittés),
/// sans attendre `MemoryConfig::idle`. Elles reviennent du cache disque si
/// on y retourne.
pub fn memory_warning() {
    #[cfg(test)]
    MEMORY_LOW_TEST.with(|c| c.set(true));
    #[cfg(not(test))]
    MEMORY_LOW.store(true, Ordering::Release);
}

fn take_memory_warning() -> bool {
    #[cfg(test)]
    return MEMORY_LOW_TEST.with(|c| c.replace(false));
    #[cfg(not(test))]
    MEMORY_LOW.swap(false, Ordering::AcqRel)
}

struct Inner {
    entries: HashMap<Key, Entry>,
    bytes: usize,
    frame: u64,
    now: Instant,
    /// Clés en attente demandées pendant l'image en cours.
    visible: HashSet<Key>,
    /// Dernier ensemble transmis à l'ordonnanceur.
    published: HashSet<Key>,
    /// Prochaine recherche de textures inutilisées.
    next_idle_scan: Instant,
    /// Nombre d'entrées au-delà duquel les échecs non affichés sont oubliés.
    purge_at: usize,
    /// Images prêtes au-delà des envois permis par image : envoyées aux
    /// images suivantes (toujours comptées dans la file de l'ordonnanceur).
    deferred: VecDeque<(Key, Arrived, Ticket)>,
    /// Prochaine image due parmi les animations jouées pendant l'image en cours.
    next_wake: Option<Instant>,
    /// Couleur moyenne des avatars déjà décodés ([`Images::avatar_tint`]) :
    /// calculée une fois, à l'envoi de la texture.
    tints: HashMap<ImageRef, [u8; 3]>,
}

/// Demande d'une image recadrée ([`Images::get_fill_moving`]) : clé de
/// l'image fixe et de l'animation (plus petite au besoin), tailles CDN.
struct FillPlan {
    still: Key,
    akey: Key,
    size: u32,
    asize: u32,
}

/// Image prête reçue de l'ordonnanceur.
enum Arrived {
    Still(egui::ColorImage),
    Anim(anim::Frames),
}

pub struct Images {
    ctx: egui::Context,
    /// Où lancer un téléchargement à la demande ([`Images::fetch_raw`]).
    rt: Option<Handle>,
    source: Arc<dyn Source>,
    /// `None` : chargement désactivé (tout est `Failed`, aucune demande).
    tx: Option<tmpsc::UnboundedSender<ToWorker>>,
    rx: mpsc::Receiver<FromWorker>,
    woken: Arc<AtomicBool>,
    shared: Arc<Shared>,
    memory: MemoryConfig,
    inner: RefCell<Inner>,
}

impl Images {
    /// Lance l'ordonnanceur sur `rt`. `wake` est appelé (une fois par lot
    /// d'images arrivées) pour redessiner l'interface.
    pub fn start(ctx: egui::Context, rt: &Handle, source: Arc<dyn Source>, disk: Option<DiskConfig>, cfg: Config, wake: crate::net::Wake) -> Self {
        let (tx, worker_rx) = tmpsc::unbounded_channel();
        let (reply_tx, rx) = mpsc::channel();
        let woken = Arc::new(AtomicBool::new(false));
        let probe = Arc::new(Probe::default());
        let shared = Shared::new(&cfg.worker, probe.clone());
        let reply = Reply { tx: reply_tx, woken: woken.clone(), wake, probe };
        rt.spawn(worker::run(cfg.worker, source.clone(), disk, shared.clone(), worker_rx, reply));
        let mut images = Self::with(ctx, source, Some(tx), rx, woken, shared, cfg.memory);
        images.rt = Some(rt.clone());
        images
    }

    /// Télécharge `url` tel quel (les octets du fichier, sans cache ni
    /// redimensionnement) hors du fil d'interface : copier ou enregistrer
    /// une image, à la demande de l'utilisateur (lot VID). `done` est appelé
    /// sur un fil de l'ordonnanceur. `false` : chargement désactivé.
    pub fn fetch_raw(&self, url: String, done: impl FnOnce(Result<Vec<u8>, cdn::FetchError>) + Send + 'static) -> bool {
        let Some(rt) = &self.rt else { return false };
        let job = self.source.fetch(url);
        rt.spawn(async move { done(job.await) });
        true
    }

    /// Aucune image n'est chargée : les remplaçants sont dessinés.
    pub fn disabled(ctx: egui::Context) -> Self {
        let (_, rx) = mpsc::channel();
        let source: Arc<dyn Source> = Arc::new(Cdn::from_endpoints(&crate::net::Endpoints::default()));
        let shared = Shared::new(&WorkerConfig::default(), Arc::default());
        Self::with(ctx, source, None, rx, Arc::default(), shared, MemoryConfig::default())
    }

    fn with(
        ctx: egui::Context,
        source: Arc<dyn Source>,
        tx: Option<tmpsc::UnboundedSender<ToWorker>>,
        rx: mpsc::Receiver<FromWorker>,
        woken: Arc<AtomicBool>,
        shared: Arc<Shared>,
        memory: MemoryConfig,
    ) -> Self {
        let now = Instant::now();
        let inner = Inner {
            entries: HashMap::new(),
            bytes: 0,
            frame: 0,
            now,
            visible: HashSet::new(),
            published: HashSet::new(),
            next_idle_scan: now,
            purge_at: MAX_ENTRIES,
            deferred: VecDeque::new(),
            next_wake: None,
            tints: HashMap::new(),
        };
        Images { ctx, rt: None, source, tx, rx, woken, shared, memory, inner: RefCell::new(inner) }
    }

    /// Image `image` affichée en carré de `points` points de côté (taille
    /// réelle en pixels déduite de l'échelle de l'écran). À n'appeler que
    /// pour ce qui est réellement visible : c'est ce qui déclenche le
    /// chargement, et ce qui le garde en vie.
    pub fn get(&self, image: ImageRef, points: f32) -> Image {
        self.get_key(self.key(image, points), cdn::cdn_size(points, self.ctx.pixels_per_point()))
    }

    /// Image de pièce jointe ou d'embed (`image` : sa clé, `url` : son
    /// adresse reçue) affichée entière dans `size` points. Le proxy média la
    /// redimensionne à cette taille ; une image servie bien plus grande est
    /// refusée avant décodage. Mêmes règles que [`Images::get`] : seulement
    /// pour ce qui est visible.
    pub fn get_media(&self, image: ImageRef, url: &str, size: egui::Vec2) -> Image {
        let key = self.media_key(image, size);
        let Fit::Contain(w, h) = key.fit else { return Image::Failed };
        // Copie sans perte, demandée par paliers ; ramenée ici à la taille exacte de l'écran.
        self.request(key, || {
            let (sw, sh) = cdn::stepped(w, h, decode::MAX_SIDE);
            (self.source.still_url(url, sw, sh), decode::accepted_side(sw.max(sh)))
        })
    }

    /// Clé utilisée par [`Images::get_media`] pour cette taille en points.
    pub fn media_key(&self, image: ImageRef, size: egui::Vec2) -> Key {
        let px = |v: f32| {
            let v = (v * self.ctx.pixels_per_point()).round();
            if v.is_finite() && v >= 1.0 { (v as u32).min(decode::MAX_SIDE) } else { 1 }
        };
        Key { image, fit: Fit::Contain(px(size.x), px(size.y)), anim: false }
    }

    /// Image fixe servie à taille fixe par le CDN (`/assets/…` : bannière de
    /// quête, aperçu d'un effet de profil ; lot ND), recadrée au centre pour
    /// remplir `size` points. L'adresse est demandée telle quelle ; une
    /// source de plus de [`ASSET_MAX_SIDE`] pixels de côté est refusée avant
    /// décodage. Mêmes règles que [`Images::get`] : seulement pour ce qui est visible.
    pub fn get_asset(&self, image: ImageRef, url: &str, size: egui::Vec2) -> Image {
        let Fit::Contain(w, h) = self.media_key(image, size).fit else { return Image::Failed };
        self.request(Key { image, fit: Fit::Fill(w, h), anim: false }, || (url.to_string(), ASSET_MAX_SIDE))
    }

    /// Image d'une quête, servie par le CDN dans le format et à la taille où
    /// l'éditeur l'a déposée (le paramètre `size` y est ignoré) : `fill`
    /// la recadre au centre pour remplir `size` points (image d'ambiance,
    /// vignette), sinon elle tient entière dans `size` sans agrandissement
    /// (logotype, dont les proportions ne sont connues qu'une fois chargé).
    /// Une source de plus de [`decode::MAX_SIDE`] pixels de côté est refusée
    /// avant décodage. Seulement pour ce qui est visible.
    pub fn get_original(&self, image: ImageRef, url: &str, size: egui::Vec2, fill: bool) -> Image {
        let key = self.media_key(image, size);
        let Fit::Contain(w, h) = key.fit else { return Image::Failed };
        let key = if fill { Key { image, fit: Fit::Fill(w, h), anim: false } } else { key };
        self.request(key, || (url.to_string(), decode::MAX_SIDE))
    }

    /// Octets d'une texture de la visionneuse au plus.
    pub fn max_texture_bytes(&self) -> usize {
        VIEWER_MAX_BYTES
    }

    /// Comme [`Images::get`] pour une image entière dans un rectangle de
    /// `w` × `h` points (bannière de profil), sans recadrage.
    pub fn get_within(&self, image: ImageRef, w: f32, h: f32) -> Image {
        let ppp = self.ctx.pixels_per_point();
        let px = |v: f32| {
            let v = (v * ppp).round();
            if v.is_finite() && v >= 1.0 { (v as u32).min(decode::MAX_SIDE) } else { 1 }
        };
        self.get_key(Key { image, fit: Fit::Contain(px(w), px(h)), anim: false }, cdn::cdn_size(w, ppp))
    }

    fn get_key(&self, key: Key, cdn_size: u32) -> Image {
        self.request(key, || (self.source.url(&key.image, cdn_size), decode::accepted_side(cdn_size)))
    }

    /// Comme [`Images::get`], animée si `play` (emoji, avatar ou autocollant
    /// animé) : l'image affichée suit les délais de l'animation, et un seul
    /// réveil est demandé pour la prochaine image due. Sans `play`, la
    /// première image (l'animation déjà chargée, sinon l'image fixe).
    /// Pendant le chargement de l'animation, l'image fixe si elle est là.
    pub fn get_moving(&self, image: ImageRef, points: f32, play: bool) -> Image {
        let key = self.key(image, points);
        let size = cdn::cdn_size(points, self.ctx.pixels_per_point());
        self.moving(key, play, || Some((self.source.anim_url(&image, size)?, decode::accepted_side(size))), || self.get_key(key, size))
    }

    /// [`Images::get_moving`] pour un profil ouvert : l'image fixe est
    /// demandée d'emblée et tient la place tant que l'animation charge.
    pub fn get_moving_still_first(&self, image: ImageRef, points: f32, play: bool) -> Image {
        let key = self.key(image, points);
        let size = cdn::cdn_size(points, self.ctx.pixels_per_point());
        self.moving_as(key, Key { anim: true, ..key }, play, true, || Some((self.source.anim_url(&image, size)?, decode::accepted_side(size))), || self.get_key(key, size))
    }

    /// [`Images::get_within`], animée si `play`.
    pub fn get_within_moving(&self, image: ImageRef, w: f32, h: f32, play: bool) -> Image {
        let ppp = self.ctx.pixels_per_point();
        let size = cdn::cdn_size(w, ppp);
        let px = |v: f32| {
            let v = (v * ppp).round();
            if v.is_finite() && v >= 1.0 { (v as u32).min(decode::MAX_SIDE) } else { 1 }
        };
        let key = Key { image, fit: Fit::Contain(px(w), px(h)), anim: false };
        self.moving(key, play, || Some((self.source.anim_url(&image, size)?, decode::accepted_side(size))), || self.get_key(key, size))
    }

    /// [`Images::get_media`], animée si `play` (GIF, WebP ou APNG joint).
    /// Une image d'une seule image reste fixe, sans réveil.
    pub fn get_media_moving(&self, image: ImageRef, url: &str, size: egui::Vec2, play: bool) -> Image {
        let key = self.media_key(image, size);
        let Fit::Contain(w, h) = key.fit else { return Image::Failed };
        self.moving(key, play, || Some((self.source.media_url(url, w, h), decode::accepted_side(w.max(h)))), || self.get_media(image, url, size))
    }

    /// Image recadrée au centre pour remplir exactement `w` × `h` points,
    /// sans déformation ni bandes (bannière de profil, plaque) ; animée si
    /// `play`. `aspect` : proportions (largeur / hauteur) de l'image
    /// d'origine, pour demander au CDN une taille qui couvre aussi la hauteur.
    ///
    /// L'animation est demandée à [`ANIM_FILL_SIDE`] pixels de grand côté au
    /// plus, puis décodée à la taille qui la fait tenir entière dans
    /// [`anim::MAX_ANIM_BYTES`] (nette si la boucle est courte, réduite et
    /// étirée par le processeur graphique sinon). L'image fixe, à la taille
    /// exacte, tient la place tant que l'animation charge, si elle échoue, et
    /// si elle n'a en fait qu'une image.
    pub fn get_fill_moving(&self, image: ImageRef, w: f32, h: f32, aspect: f32, play: bool) -> Image {
        let p = self.fill_plan(image, w, h, aspect);
        self.moving_as(p.still, p.akey, play, true, || Some((self.source.anim_url(&image, p.asize)?, decode::accepted_side(p.asize))), || self.fill_still(&p))
    }

    /// [`Images::get_fill_moving`] pour une image qui ne s'anime que sous le
    /// pointeur (plaque nominative d'une ligne) : au repos (`play` faux),
    /// l'image fixe, toujours, même si l'animation est en mémoire (sa
    /// première image n'est pas forcément l'image fixe), et rien n'est
    /// demandé ni réveillé ; l'animation, remise au début, n'est plus tenue
    /// pour affichée et suit les règles d'éviction. Survolée (`play`),
    /// l'animation est chargée (une fois : un échec garde l'image fixe sans
    /// nouvelle tentative) puis jouée en boucle, un réveil par image due.
    /// L'animation est servie à taille fixe, comme l'image fixe.
    pub fn get_fill_hover(&self, image: ImageRef, w: f32, h: f32, aspect: f32, play: bool) -> Image {
        let p = self.fill_plan(image, w, h, aspect);
        if !play {
            self.rest(&p.akey);
            return self.fill_still(&p);
        }
        self.moving_as(p.still, p.akey, true, true, || Some((self.source.anim_url(&image, p.asize)?, decode::accepted_side(p.asize).max(FILL_MIN_ACCEPTED))), || self.fill_still(&p))
    }

    /// Animation `akey` au repos : remise à sa première image, sans compter
    /// comme affichée.
    fn rest(&self, akey: &Key) {
        if let Some(Entry::Anim { tex, frames, player, .. }) = self.inner.borrow_mut().entries.get_mut(akey) {
            if player.shown != 0
                && let Some(f) = frames.first()
            {
                tex.set(f.clone(), TextureOptions::LINEAR);
            }
            player.rewind();
        }
    }

    /// Image fixe d'une image recadrée, à la taille exacte.
    fn fill_still(&self, p: &FillPlan) -> Image {
        self.request(p.still, || (self.source.url(&p.still.image, p.size), decode::accepted_side(p.size).max(FILL_MIN_ACCEPTED)))
    }

    /// Clés et tailles CDN d'une image recadrée pour remplir `w` × `h` points.
    fn fill_plan(&self, image: ImageRef, w: f32, h: f32, aspect: f32) -> FillPlan {
        let ppp = self.ctx.pixels_per_point();
        let px = |v: f32| {
            let v = (v * ppp).round();
            if v.is_finite() && v >= 1.0 { (v as u32).min(decode::MAX_SIDE) } else { 1 }
        };
        let (pw, ph) = (px(w), px(h));
        let still = Key { image, fit: Fit::Fill(pw, ph), anim: false };
        let cover = |w: f32, h: f32| cdn::cdn_size(w.max(h * aspect.max(0.01)), 1.0);
        let size = cover(pw as f32, ph as f32);
        let scale = (ANIM_FILL_SIDE as f32 / pw.max(ph) as f32).min(1.0);
        let (aw, ah) = (((pw as f32 * scale).round() as u32).max(1), ((ph as f32 * scale).round() as u32).max(1));
        let akey = Key { image, fit: Fit::Fill(aw, ah), anim: true };
        let asize = cover(aw as f32, ah as f32);
        FillPlan { still, akey, size, asize }
    }

    fn moving(&self, still: Key, play: bool, job: impl FnOnce() -> Option<(String, u32)>, still_get: impl FnOnce() -> Image) -> Image {
        self.moving_as(still, Key { anim: true, ..still }, play, false, job, still_get)
    }

    /// `akey` : clé de l'animation (même image, éventuellement plus petite).
    /// `still_first` : l'image fixe est demandée en même temps que
    /// l'animation et affichée en l'attendant (une bannière animée pèse
    /// parfois plusieurs Mo : le profil ne reste pas vide entre-temps).
    fn moving_as(&self, still: Key, akey: Key, play: bool, still_first: bool, job: impl FnOnce() -> Option<(String, u32)>, still_get: impl FnOnce() -> Image) -> Image {
        {
            let mut inner = self.inner.borrow_mut();
            let (frame, now) = (inner.frame, inner.now);
            let mut wake = None;
            let mut shown = None;
            if let Some(Entry::Anim { tex, frames, delays, player, last_frame, last_used, .. }) = inner.entries.get_mut(&akey) {
                *last_frame = frame;
                *last_used = now;
                let changed = if play {
                    player.advance(delays, now)
                } else {
                    let moved = player.shown != 0;
                    player.rewind();
                    moved
                };
                if changed && let Some(f) = frames.get(player.shown) {
                    tex.set(f.clone(), TextureOptions::LINEAR);
                }
                if play {
                    wake = player.next_at;
                }
                shown = Some(tex.id());
            }
            if let Some(at) = wake {
                inner.next_wake = Some(inner.next_wake.map_or(at, |n| n.min(at)));
            }
            if let Some(id) = shown {
                return Image::Ready(id);
            }
        }
        // Animation d'une seule image : déjà une image fixe. Elle reste en
        // mémoire (pour ne pas être redemandée), mais l'image fixe demandée à
        // la taille exacte, plus nette, passe devant dès qu'elle est là.
        if self.is_loaded(&akey) {
            let single = self.request(akey, || (String::new(), 1));
            if still_first
                && let Image::Ready(id) = still_get()
            {
                return Image::Ready(id);
            }
            return single;
        }
        if !play {
            return still_get();
        }
        let Some(job) = job() else { return still_get() };
        match self.request(akey, || job) {
            Image::Ready(id) => Image::Ready(id),
            Image::Pending if still_first || self.is_loaded(&still) => still_get(),
            Image::Pending => Image::Pending,
            Image::Failed => still_get(),
        }
    }

    /// `job` : adresse et côté accepté, calculés seulement s'il faut demander.
    fn request(&self, key: Key, job: impl FnOnce() -> (String, u32)) -> Image {
        let Some(tx) = &self.tx else { return Image::Failed };
        let mut inner = self.inner.borrow_mut();
        let (frame, now) = (inner.frame, inner.now);
        match inner.entries.get_mut(&key) {
            Some(Entry::Ready { tex, last_frame, last_used, .. } | Entry::Anim { tex, last_frame, last_used, .. }) => {
                *last_frame = frame;
                *last_used = now;
                return Image::Ready(tex.id());
            }
            Some(Entry::Pending) => {
                inner.visible.insert(key);
                return Image::Pending;
            }
            Some(Entry::Failed { retry_at, seen }) if retry_at.is_none_or(|t| now < t) => {
                *seen = frame;
                return Image::Failed;
            }
            Some(Entry::Failed { .. }) | None => {}
        }
        inner.entries.insert(key, Entry::Pending);
        inner.visible.insert(key);
        let (url, max_side) = job();
        let _ = tx.send(ToWorker::Want(Job { key, url, max_side }));
        Image::Pending
    }

    /// Début d'une image : textures arrivées envoyées au processeur
    /// graphique (au plus `uploads_per_frame`), échecs notés.
    pub fn begin_frame(&self) {
        self.woken.store(false, Ordering::Release);
        let mut inner = self.inner.borrow_mut();
        inner.frame += 1;
        inner.now = Instant::now();
        inner.visible.clear();
        inner.next_wake = None;
        let (frame, now) = (inner.frame, inner.now);
        let mut uploads = 0;
        // Celles qui attendent depuis l'image précédente d'abord.
        while uploads < self.memory.uploads_per_frame
            && let Some((key, img, ticket)) = inner.deferred.pop_front()
        {
            uploads += usize::from(self.upload(&mut inner, key, img, ticket));
        }
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                msg @ (FromWorker::Ready(..) | FromWorker::Anim(..)) => {
                    let (key, img, ticket) = match msg {
                        FromWorker::Ready(k, i, t) => (k, Arrived::Still(i), t),
                        FromWorker::Anim(k, f, t) => (k, Arrived::Anim(f), t),
                        _ => continue,
                    };
                    if uploads < self.memory.uploads_per_frame {
                        uploads += usize::from(self.upload(&mut inner, key, img, ticket));
                    } else if matches!(inner.entries.get(&key), Some(Entry::Pending)) {
                        inner.deferred.push_back((key, img, ticket));
                    }
                }
                FromWorker::Failed(key, f) => {
                    if matches!(inner.entries.get(&key), Some(Entry::Pending)) {
                        let retry_at = match f {
                            Failure::Permanent => None,
                            Failure::Retry(d) => Some(now + d),
                        };
                        inner.entries.insert(key, Entry::Failed { retry_at, seen: frame });
                    }
                }
                FromWorker::Cancelled(key) => {
                    if matches!(inner.entries.get(&key), Some(Entry::Pending)) {
                        inner.entries.remove(&key);
                    }
                }
            }
        }
        if !inner.deferred.is_empty() {
            // Le reste passera à l'image suivante.
            self.ctx.request_repaint();
        }
        // Ce qui était à l'écran à l'image précédente y est sans doute encore.
        self.evict(&mut inner, frame.saturating_sub(1));
    }

    /// Couleur moyenne de l'avatar `image`, connue dès qu'une de ses tailles a été
    /// décodée : la bande d'un profil sans bannière ni couleur d'accent, comme dans
    /// le client. Ne demande rien : `None` tant que l'avatar n'est pas arrivé.
    pub fn avatar_tint(&self, image: ImageRef) -> Option<egui::Color32> {
        self.inner.borrow().tints.get(&image).map(|[r, g, b]| egui::Color32::from_rgb(*r, *g, *b))
    }

    /// Envoie une image prête au processeur graphique, si elle est encore
    /// attendue (arrivée après une annulation : jetée). `true` si envoyée.
    fn upload(&self, inner: &mut Inner, key: Key, img: Arrived, ticket: Ticket) -> bool {
        if !matches!(inner.entries.get(&key), Some(Entry::Pending)) {
            return false;
        }
        let (frame, now) = (inner.frame, inner.now);
        if matches!(key.image, ImageRef::Avatar { .. } | ImageRef::MemberAvatar { .. }) && !inner.tints.contains_key(&key.image) {
            let first = match &img {
                Arrived::Still(i) => Some(i),
                Arrived::Anim(f) => f.first().map(|(i, _)| i),
            };
            if let Some(tint) = first.and_then(mean_color) {
                if inner.tints.len() >= MAX_TINTS {
                    inner.tints.clear();
                }
                inner.tints.insert(key.image, tint);
            }
        }
        let entry = match img {
            Arrived::Anim(f) if f.len() > 1 => {
                let (frames, delays): (Vec<Arc<egui::ColorImage>>, Vec<Duration>) = f.into_iter().map(|(i, d)| (Arc::new(i), d)).unzip();
                // Images gardées en mémoire, plus celle de la texture.
                let one = frames[0].size[0] * frames[0].size[1] * 4;
                let bytes = frames.iter().map(|i| i.size[0] * i.size[1] * 4).sum::<usize>() + one;
                let tex = self.ctx.load_texture("anim", frames[0].clone(), TextureOptions::LINEAR);
                Entry::Anim { tex, frames, delays, player: anim::Player::new(), bytes, last_frame: frame, last_used: now }
            }
            Arrived::Anim(f) => {
                let Some((img, _)) = f.into_iter().next() else { return false };
                let bytes = img.size[0] * img.size[1] * 4;
                Entry::Ready { tex: self.ctx.load_texture("image", img, TextureOptions::LINEAR), bytes, last_frame: frame, last_used: now }
            }
            Arrived::Still(img) => {
                let bytes = img.size[0] * img.size[1] * 4;
                Entry::Ready { tex: self.ctx.load_texture("image", img, TextureOptions::LINEAR), bytes, last_frame: frame, last_used: now }
            }
        };
        // Devenue texture : elle ne compte plus dans la file de l'ordonnanceur.
        drop(ticket);
        inner.bytes += match &entry {
            Entry::Ready { bytes, .. } | Entry::Anim { bytes, .. } => *bytes,
            _ => 0,
        };
        inner.entries.insert(key, entry);
        true
    }

    /// Fin d'une image : libère ce qui dépasse les plafonds et dit à
    /// l'ordonnanceur ce qui est encore attendu.
    pub fn end_frame(&self) {
        let mut inner = self.inner.borrow_mut();
        let (frame, now, idle) = (inner.frame, inner.now, self.memory.idle);
        let squeezed = take_memory_warning();
        let idle = if squeezed { Duration::ZERO } else { idle };
        if squeezed || now >= inner.next_idle_scan {
            let mut freed = 0;
            inner.entries.retain(|_, e| match e {
                Entry::Ready { bytes, last_frame, last_used, .. } | Entry::Anim { bytes, last_frame, last_used, .. } if *last_frame < frame && now.duration_since(*last_used) >= idle => {
                    freed += *bytes;
                    false
                }
                _ => true,
            });
            inner.bytes -= freed.min(inner.bytes);
            inner.next_idle_scan = now + (self.memory.idle / 4).min(IDLE_SCAN_MAX);
        }
        self.evict(&mut inner, frame);
        if inner.entries.len() > inner.purge_at {
            // Échecs qui ne sont plus affichés : oubliés (redemandés, ils
            // reçoivent la même réponse de l'ordonnanceur, sans requête).
            inner.entries.retain(|_, e| !matches!(e, Entry::Failed { seen, .. } if *seen < frame));
            let n = inner.entries.len();
            inner.purge_at = MAX_ENTRIES.max(n + n / 10);
        }
        if inner.visible != inner.published
            && let Some(tx) = &self.tx
        {
            let visible = inner.visible.clone();
            let _ = tx.send(ToWorker::Visible(visible.clone()));
            inner.published = visible;
        }
        // Une animation jouée dans cette image : un seul réveil, à l'échéance
        // de la plus proche. Aucune affichée : aucun réveil.
        if let Some(at) = inner.next_wake {
            self.ctx.request_repaint_after(at.saturating_duration_since(Instant::now()));
        }
    }

    /// Au-delà du plafond, libère les textures affichées le moins récemment
    /// jusqu'à 90 % du plafond ; jamais celles affichées depuis l'image
    /// `shown_from` (l'image en cours ; au début d'une image, la précédente).
    fn evict(&self, inner: &mut Inner, shown_from: u64) {
        if inner.bytes <= self.memory.max_bytes {
            return;
        }
        let target = self.memory.max_bytes / 10 * 9;
        let mut old: Vec<(u64, Instant, Key)> = inner
            .entries
            .iter()
            .filter_map(|(k, e)| match e {
                Entry::Ready { last_frame, last_used, .. } | Entry::Anim { last_frame, last_used, .. } if *last_frame < shown_from => Some((*last_frame, *last_used, *k)),
                _ => None,
            })
            .collect();
        old.sort_by_key(|(f, t, _)| (*f, *t));
        for (_, _, key) in old {
            if inner.bytes <= target {
                break;
            }
            if let Some(Entry::Ready { bytes, .. } | Entry::Anim { bytes, .. }) = inner.entries.remove(&key) {
                inner.bytes -= bytes.min(inner.bytes);
            }
        }
    }

    pub fn stats(&self) -> Stats {
        let inner = self.inner.borrow();
        let mut s = Stats { bytes: inner.bytes, ..Stats::default() };
        for e in inner.entries.values() {
            match e {
                Entry::Pending => s.pending += 1,
                Entry::Ready { .. } | Entry::Anim { .. } => s.textures += 1,
                Entry::Failed { .. } => s.failed += 1,
            }
        }
        s
    }

    /// Chiffres de l'ordonnanceur (décodages, file vers l'interface, pause).
    pub fn worker_stats(&self) -> WorkerStats {
        self.shared.stats()
    }

    /// L'image est-elle prête à cette taille ? Ne demande rien (sans effet
    /// sur le chargement ni sur l'éviction).
    pub fn peek(&self, image: ImageRef, points: f32) -> bool {
        self.is_loaded(&self.key(image, points))
    }

    /// Une texture de cette clé est-elle en mémoire ?
    pub fn is_loaded(&self, key: &Key) -> bool {
        matches!(self.inner.borrow().entries.get(key), Some(Entry::Ready { .. }))
    }

    /// Clé utilisée par [`Images::get`] pour ce côté en points.
    pub fn key(&self, image: ImageRef, points: f32) -> Key {
        let side = (points * self.ctx.pixels_per_point()).round();
        let side = if side.is_finite() && side >= 1.0 { (side as u32).min(decode::MAX_SIDE) } else { 1 };
        Key { image, fit: Fit::Cover(side), anim: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::ImageHash;
    use std::sync::{
        atomic::{AtomicUsize, Ordering::SeqCst},
        Mutex,
    };

    /// PNG synthétique de `side` pixels, différent selon `seed`.
    fn png(side: u32, seed: u8) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(side, side, |x, y| image::Rgba([seed, (x * 3) as u8, (y * 3) as u8, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img).write_to(&mut out, image::ImageFormat::Png).unwrap_or_else(|e| panic!("{e}"));
        out.into_inner()
    }

    #[derive(Default)]
    struct Counters {
        fetches: AtomicUsize,
        running: AtomicUsize,
        max_running: AtomicUsize,
    }

    /// Décrémente le nombre de téléchargements en cours, même si la future est annulée.
    struct Leave(Arc<Counters>);

    impl Drop for Leave {
        fn drop(&mut self) {
            self.0.running.fetch_sub(1, SeqCst);
        }
    }

    /// Source locale : compte les téléchargements, peut échouer, et peut
    /// retenir ses réponses jusqu'à ce que le test les libère (porte).
    struct Local {
        n: Arc<Counters>,
        gate: Option<Arc<tokio::sync::Semaphore>>,
        answer: Mutex<Result<(), FetchError>>,
    }

    impl Local {
        fn fetches(&self) -> usize {
            self.n.fetches.load(SeqCst)
        }
        fn answer(&self, a: Result<(), FetchError>) {
            if let Ok(mut s) = self.answer.lock() {
                *s = a;
            }
        }
        /// Laisse passer `n` réponses retenues.
        fn release(&self, n: usize) {
            if let Some(g) = &self.gate {
                g.add_permits(n);
            }
        }
    }

    impl Source for Local {
        fn url(&self, image: &ImageRef, size: u32) -> String {
            format!("local://{image:?}/{size}")
        }
        fn anim_url(&self, image: &ImageRef, size: u32) -> Option<String> {
            match image {
                ImageRef::CustomEmoji { .. } => Some(format!("local://{image:?}/{size}.gif")),
                // Bannière ou avatar animé d'un abonné : une longue boucle.
                ImageRef::Banner { hash, .. } | ImageRef::Avatar { hash, .. } if hash.animated => Some(format!("local://{image:?}/{size}.long.gif")),
                // Plaque animée : un WebM (ou des octets illisibles, pour la plaque « abimee »).
                ImageRef::Nameplate { asset } if asset.name().contains("abimee") => Some(format!("local://{image:?}/bad.webm")),
                ImageRef::Nameplate { .. } => Some(format!("local://{image:?}/asset.webm")),
                _ => None,
            }
        }
        fn fetch(&self, url: String) -> FetchFuture {
            self.n.fetches.fetch_add(1, SeqCst);
            let now = self.n.running.fetch_add(1, SeqCst) + 1;
            self.n.max_running.fetch_max(now, SeqCst);
            let leave = Leave(self.n.clone());
            let answer = self.answer.lock().map(|a| a.clone()).unwrap_or(Ok(()));
            let gate = self.gate.clone();
            Box::pin(async move {
                let _leave = leave;
                if let Some(g) = gate
                    && let Ok(p) = g.acquire().await
                {
                    p.forget();
                }
                // L'utilisateur HOSTILE a un avatar qui fait paniquer le décodeur.
                answer.map(|()| if url.ends_with("bad.webm") {
                    // Signature WebM, piste VP9, données qui n'en sont pas : aucune image.
                    crate::video::mkv::write::webm(Some(("V_VP9", 320, 64)), None, &[(1, 0, true, vec![0x55; 64])], 40.0, false)
                } else if url.ends_with(".webm") {
                    // Sans le codec VP8 de Windows, un GIF : les règles du survol sont les mêmes.
                    webm::tests::plate().unwrap_or_else(|| anim::tests::gif_sized(webm::tests::FRAMES as usize, webm::tests::W, webm::tests::H))
                } else if url.ends_with(".long.gif") { anim::tests::gif_sized(LONG_LOOP, 256, 96) } else if url.ends_with(".gif") { anim::tests::gif(3, 64) } else if url.contains(&format!("user_id: {HOSTILE},")) { decode::PANIC_TRIGGER.to_vec() } else { png(64, url.len() as u8) })
            })
        }
    }

    struct Bench {
        _rt: tokio::runtime::Runtime,
        images: Images,
        source: Arc<Local>,
        wakes: Arc<AtomicUsize>,
    }

    /// `gated` : les réponses n'arrivent que libérées par [`Local::release`].
    fn bench(gated: bool, cfg: Config) -> Bench {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap_or_else(|e| panic!("{e}"));
        let gate = gated.then(|| Arc::new(tokio::sync::Semaphore::new(0)));
        let source = Arc::new(Local { n: Arc::default(), gate, answer: Mutex::new(Ok(())) });
        let wakes = Arc::new(AtomicUsize::new(0));
        let w = wakes.clone();
        let wake: crate::net::Wake = Arc::new(move || {
            w.fetch_add(1, SeqCst);
        });
        let images = Images::start(egui::Context::default(), rt.handle(), source.clone(), None, cfg, wake);
        Bench { _rt: rt, images, source, wakes }
    }

    /// Images de la longue boucle servie pour une bannière ou un avatar animé.
    const LONG_LOOP: usize = 90;

    /// Utilisateur dont l'avatar fait paniquer le décodeur (hors des plages des autres tests).
    const HOSTILE: u64 = u64::MAX - 1;

    fn avatar(i: u64) -> ImageRef {
        ImageRef::Avatar { user_id: i, hash: ImageHash::synthetic(i) }
    }

    /// Attend (sans rien dessiner) que `cond` soit vrai.
    fn until(what: &str, cond: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while !cond() {
            assert!(Instant::now() < deadline, "délai dépassé : {what}");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Une image où `refs` sont affichées.
    fn frame(images: &Images, refs: &[(ImageRef, f32)]) -> Vec<Image> {
        images.begin_frame();
        let got = refs.iter().map(|(r, side)| images.get(*r, *side)).collect();
        images.end_frame();
        got
    }

    /// Images successives où `refs` sont affichées, jusqu'à ce que `done` soit vrai.
    fn frames(images: &Images, refs: &[(ImageRef, f32)], done: impl Fn(&[Image]) -> bool) -> Vec<Image> {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let got = frame(images, refs);
            if done(&got) {
                return got;
            }
            assert!(Instant::now() < deadline, "délai dépassé : {got:?}");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn all_ready(got: &[Image]) -> bool {
        got.iter().all(|i| matches!(i, Image::Ready(_)))
    }

    /// Partie D : une vignette est demandée sans perte de qualité et par
    /// paliers ; la texture garde la taille exacte de l'écran.
    #[test]
    fn media_is_asked_lossless_and_by_steps() {
        struct Stills(Mutex<Vec<String>>);
        impl Source for Stills {
            fn url(&self, _: &ImageRef, _: u32) -> String {
                String::new()
            }
            fn still_url(&self, url: &str, width: u32, height: u32) -> String {
                let u = cdn::still_url(url, width, height);
                if let Ok(mut v) = self.0.lock() {
                    v.push(u.clone());
                }
                u
            }
            fn fetch(&self, _: String) -> FetchFuture {
                Box::pin(async { Ok(png(64, 1)) })
            }
        }
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap_or_else(|e| panic!("{e}"));
        let source = Arc::new(Stills(Mutex::new(Vec::new())));
        let images = Images::start(egui::Context::default(), rt.handle(), source.clone(), None, Config::default(), Arc::new(|| {}));
        let photo = "https://media.discordapp.net/attachments/1/2/photo.jpg?ex=0a&is=0b&hm=0c";
        let image = |slot| ImageRef::Media { owner: 77, slot, rev: 0 };
        // Proportion 16:10, de 641 à 704 pixels de large : un seul palier (704).
        for (slot, width) in (641..=704u32).enumerate() {
            images.get_media(image(slot as u16), photo, egui::vec2(width as f32, (width as f32 / 1.6).round()));
        }
        let asked = source.0.lock().map(|v| v.clone()).unwrap_or_default();
        assert_eq!(asked.len(), 64);
        let mut distinct = asked.clone();
        distinct.sort();
        distinct.dedup();
        assert!(distinct.len() <= 2, "64 largeurs, un palier (hauteur à un pixel près) : {distinct:?}");
        for u in &distinct {
            assert!(u.starts_with(photo) && u.contains("&width=704&height=44") && u.ends_with("&format=webp&quality=lossless"), "{u}");
        }
        // La clé (taille de la texture) reste celle de l'écran, au pixel près.
        assert_eq!(images.media_key(image(0), egui::vec2(688.0, 430.0)).fit, Fit::Contain(688, 430));
    }

    /// Animation (lot A6) : chargée seulement quand elle joue, image fixe
    /// sinon ; un réveil programmé à l'échéance de l'image suivante pendant
    /// qu'elle est affichée et jouée, aucun sinon.
    #[test]
    fn animations_play_only_when_shown_and_asked() {
        let b = bench(false, Config::default());
        let emoji = ImageRef::CustomEmoji { id: 7 };
        let moving = |play: bool| {
            b.images.begin_frame();
            let got = b.images.get_moving(emoji, 32.0, play);
            let wake = b.images.inner.borrow().next_wake;
            b.images.end_frame();
            (got, wake)
        };
        // Sans jouer : l'image fixe seulement, aucun réveil.
        let deadline = Instant::now() + Duration::from_secs(60);
        while !matches!(moving(false), (Image::Ready(_), None)) {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!((b.source.fetches(), b.images.stats().textures), (1, 1));
        // Jouée : l'animation est chargée (GIF de 3 images de 50 ms), l'image fixe sert d'attente.
        loop {
            let (got, wake) = moving(true);
            assert!(matches!(got, Image::Ready(_)), "l'image fixe en attendant : {got:?}");
            if wake.is_some() {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(b.source.fetches(), 2);
        let akey = Key { anim: true, ..b.images.key(emoji, 32.0) };
        let shown = || match b.images.inner.borrow().entries.get(&akey) {
            Some(Entry::Anim { player, frames, .. }) => (player.shown, frames.len()),
            _ => panic!("animation absente"),
        };
        assert_eq!(shown(), (0, 3));
        std::thread::sleep(Duration::from_millis(60));
        let (_, wake) = moving(true);
        assert!(wake.is_some());
        assert_eq!(shown().0, 1, "image suivante après son délai");
        // Plus jouée : retour à la première image, aucun réveil.
        assert!(matches!(moving(false), (Image::Ready(_), None)));
        assert_eq!(shown().0, 0);
        // Pas affichée du tout : rien non plus.
        b.images.begin_frame();
        b.images.end_frame();
        assert_eq!(b.images.inner.borrow().next_wake, None);
        let bytes_per_frame = 32 * 32 * 4;
        assert!(b.images.stats().bytes >= 4 * bytes_per_frame, "toutes les images comptées : {:?}", b.images.stats());
    }

    /// Profil complet d'un abonné (retour d'utilisation) : sa bannière et son
    /// avatar animés, longs, pèsent à eux deux plus que le plafond du cache.
    /// Tant qu'ils sont à l'écran, rien de ce qui est affiché n'est libéré :
    /// sinon les textures affichées partaient au début de chaque image, puis
    /// étaient redemandées (bannière vide, clignotement, rechargement sans fin).
    #[test]
    fn shown_animations_are_never_evicted() {
        let b = bench(false, Config::default());
        let moving = |seed: u64| {
            let mut h = ImageHash::synthetic(seed);
            h.animated = true;
            h
        };
        let banner = ImageRef::Banner { user_id: 1, hash: moving(1) };
        let face = ImageRef::Avatar { user_id: 1, hash: moving(2) };
        let show = || {
            b.images.begin_frame();
            let mut got = vec![b.images.get_fill_moving(banner, 600.0, 210.0, 17.0 / 6.0, true), b.images.get_moving_still_first(face, 160.0, true)];
            got.extend((10..40).map(|i| b.images.get(avatar(i), 40.0)));
            b.images.end_frame();
            got
        };
        let anims = || b.images.inner.borrow().entries.values().filter(|e| matches!(e, Entry::Anim { .. })).count();
        let deadline = Instant::now() + Duration::from_secs(120);
        // Jusqu'à ce que les deux animations soient arrivées (pas forcément
        // ensemble : c'est justement ce qui manquait).
        let mut arrived = HashSet::new();
        loop {
            let got = show();
            arrived.extend(b.images.inner.borrow().entries.iter().filter(|(_, e)| matches!(e, Entry::Anim { .. })).map(|(k, _)| k.image));
            if arrived.len() == 2 && all_ready(&got) {
                break;
            }
            assert!(Instant::now() < deadline, "animations jamais arrivées : {arrived:?}, {:?}", b.images.stats());
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(anims(), 2, "la première arrivée a été libérée à l'arrivée de la seconde");
        assert!(b.images.stats().bytes > b.images.memory.max_bytes, "le cas visé : l'écran dépasse le plafond ({:?})", b.images.stats());
        let fetched = b.source.fetches();
        for n in 0..30 {
            let got = show();
            assert!(all_ready(&got), "image {n} : une texture affichée a été libérée ({got:?})");
            assert_eq!(anims(), 2, "image {n} : une animation affichée a été libérée");
        }
        assert_eq!(b.source.fetches(), fetched, "rien n'est rechargé");
        // Profil fermé : le plafond reprend ses droits.
        for _ in 0..3 {
            b.images.begin_frame();
            b.images.end_frame();
        }
        assert!(b.images.stats().bytes <= b.images.memory.max_bytes, "{:?}", b.images.stats());
    }

    fn plate(name: &str) -> ImageRef {
        let asset = crate::state::cosmetics::Asset::path(&format!("nameplates/essai/{name}/")).unwrap_or_else(|| panic!("chemin"));
        ImageRef::Nameplate { asset }
    }

    /// Compte les redessins demandés à egui par le cache d'images.
    fn repaints(images: &Images) -> Arc<AtomicUsize> {
        let n = Arc::new(AtomicUsize::new(0));
        let counter = n.clone();
        images.ctx.set_request_repaint_callback(move |_| {
            counter.fetch_add(1, SeqCst);
        });
        n
    }

    /// Plaque animée (lot plaques) : au repos, l'image fixe seule, sans
    /// téléchargement du WebM ni aucun redessin demandé, quel que soit le
    /// nombre d'images dessinées ; sous le pointeur, le WebM est chargé une
    /// fois, puis l'animation avance et demande l'image suivante ; le
    /// pointeur parti, retour à l'image fixe et au silence.
    #[test]
    fn nameplates_move_only_under_the_pointer() {
        // Fabriquée d'avance (encodeur VP8 de Windows), hors des fils de l'ordonnanceur.
        let real = webm::tests::plate().is_some();
        if !real {
            eprintln!("VP8 de Windows absent : plaque servie en GIF (mêmes règles de survol)");
        }
        let b = bench(false, Config::default());
        let asked = repaints(&b.images);
        let plate = plate("plaque");
        let show = |hovered: bool| {
            b.images.begin_frame();
            let got = b.images.get_fill_hover(plate, 224.0, 42.0, 16.0 / 3.0, hovered);
            let wake = b.images.inner.borrow().next_wake;
            b.images.end_frame();
            (got, wake)
        };
        let deadline = Instant::now() + Duration::from_secs(60);
        // Pas de survol : l'image fixe, et rien d'autre.
        let still = loop {
            if let (Image::Ready(id), wake) = show(false) {
                assert_eq!(wake, None);
                break id;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        for _ in 0..30 {
            assert_eq!(show(false), (Image::Ready(still), None));
        }
        assert_eq!((b.source.fetches(), b.images.stats().textures), (1, 1), "le WebM n'est pas chargé au repos");
        assert_eq!(asked.load(SeqCst), 0, "aucun redessin demandé au repos");
        // Survol : l'image fixe tient la place pendant le chargement, puis l'animation joue.
        loop {
            let (got, wake) = show(true);
            assert!(matches!(got, Image::Ready(_)), "jamais de trou : {got:?}");
            if wake.is_some() {
                assert_ne!(got, Image::Ready(still), "la texture de l'animation");
                break;
            }
            assert_eq!(got, Image::Ready(still), "l'image fixe en attendant");
            assert!(Instant::now() < deadline, "animation jamais arrivée : {:?}", b.images.stats());
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(b.source.fetches(), 2);
        assert!(asked.load(SeqCst) >= 1, "l'image suivante est demandée pendant le survol");
        let player = || {
            let inner = b.images.inner.borrow();
            let found = inner.entries.iter().find_map(|(k, e)| match e {
                Entry::Anim { player, frames, bytes, .. } if k.image == plate => Some((player.shown, frames.len(), *bytes, frames[0].size)),
                _ => None,
            });
            found.unwrap_or_else(|| panic!("animation absente"))
        };
        let (shown, count, bytes, size) = player();
        assert_eq!((shown, count), (0, webm::tests::FRAMES as usize));
        assert_eq!(size, [224, 42], "décodée à la taille d'affichage");
        assert!(bytes <= webm::MAX_BYTES && bytes <= b.images.memory.max_bytes, "{bytes}");
        std::thread::sleep(Duration::from_millis(70));
        let (got, wake) = show(true);
        assert!(wake.is_some() && got != Image::Ready(still));
        assert!(player().0 >= 1, "l'animation a avancé");
        // Le pointeur s'en va : image fixe, animation remise au début, plus aucun réveil.
        let before = asked.load(SeqCst);
        for _ in 0..30 {
            assert_eq!(show(false), (Image::Ready(still), None));
        }
        assert_eq!(player().0, 0);
        assert_eq!(asked.load(SeqCst), before, "aucun redessin demandé après le survol");
        // Il revient : l'animation en mémoire repart, sans rien recharger.
        assert!(show(true).1.is_some());
        assert_eq!(b.source.fetches(), 2);
    }

    /// Plaque dont le WebM est illisible (ou décodeur VP9 absent) : l'image
    /// fixe reste, sans erreur visible, et le fichier n'est demandé qu'une fois.
    #[test]
    fn unreadable_nameplate_keeps_the_still() {
        let b = bench(false, Config::default());
        let plate = plate("abimee");
        let show = |hovered: bool| {
            b.images.begin_frame();
            let got = b.images.get_fill_hover(plate, 224.0, 42.0, 16.0 / 3.0, hovered);
            let wake = b.images.inner.borrow().next_wake;
            b.images.end_frame();
            (got, wake)
        };
        let deadline = Instant::now() + Duration::from_secs(60);
        let still = loop {
            if let (Image::Ready(id), _) = show(false) {
                break id;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        while b.images.stats().failed == 0 {
            assert_eq!(show(true), (Image::Ready(still), None));
            assert!(Instant::now() < deadline, "échec jamais rapporté : {:?}", b.images.stats());
            std::thread::sleep(Duration::from_millis(1));
        }
        for _ in 0..50 {
            assert_eq!(show(true), (Image::Ready(still), None), "image fixe, aucun réveil");
        }
        assert_eq!(b.source.fetches(), 2, "une seule tentative");
        assert_eq!(b.images.stats(), Stats { bytes: 224 * 42 * 4, textures: 1, pending: 0, failed: 1 });
    }

    #[test]
    fn same_image_is_fetched_once() {
        let b = bench(true, Config::default());
        // 40 et 48 points : même taille CDN (64), deux tailles de texture.
        let refs = [(avatar(1), 40.0), (avatar(1), 40.0), (avatar(1), 48.0)];
        frame(&b.images, &refs);
        // Réponse retenue : les trois demandes sont arrivées avant elle.
        until("téléchargement lancé", || b.source.fetches() == 1);
        b.source.release(10);
        frames(&b.images, &refs, all_ready);
        assert_eq!(b.source.fetches(), 1, "un seul téléchargement");
        assert_eq!(b.images.stats().textures, 2);
    }

    #[test]
    fn concurrency_is_bounded() {
        let cfg = Config { worker: WorkerConfig { concurrency: 3, ..WorkerConfig::default() }, ..Config::default() };
        let b = bench(true, cfg);
        let refs: Vec<(ImageRef, f32)> = (0..30).map(|i| (avatar(i), 32.0)).collect();
        frame(&b.images, &refs);
        until("trois téléchargements", || b.source.fetches() == 3);
        b.source.release(100);
        frames(&b.images, &refs, all_ready);
        assert_eq!(b.source.fetches(), 30);
        assert_eq!(b.source.n.max_running.load(SeqCst), 3, "jamais plus de trois à la fois");
    }

    #[test]
    fn not_found_is_remembered() {
        let b = bench(false, Config::default());
        b.source.answer(Err(FetchError::NotFound));
        frames(&b.images, &[(avatar(1), 32.0)], |g| g[0] == Image::Failed);
        // Redemandé longtemps : jamais retéléchargé.
        for _ in 0..50 {
            frames(&b.images, &[(avatar(1), 32.0)], |g| g[0] == Image::Failed);
        }
        // Autre taille de texture, même adresse : refus immédiat, sans requête.
        frames(&b.images, &[(avatar(1), 31.0)], |g| g[0] == Image::Failed);
        assert_eq!(b.source.fetches(), 1);
    }

    /// Erreur réseau : aucune requête pendant l'attente, quel que soit le
    /// nombre d'images dessinées ni l'avis de l'interface. Sans délai réel :
    /// une attente d'une heure, que l'interface croit échue.
    #[test]
    fn network_errors_back_off() {
        let cfg = Config { worker: WorkerConfig { retry_base: Duration::from_secs(3600), ..WorkerConfig::default() }, ..Config::default() };
        let b = bench(false, cfg);
        b.source.answer(Err(FetchError::Network("coupure".into())));
        frames(&b.images, &[(avatar(1), 32.0)], |g| g[0] == Image::Failed);
        for _ in 0..50 {
            assert_eq!(frame(&b.images, &[(avatar(1), 32.0)]), vec![Image::Failed]);
        }
        assert_eq!(b.source.fetches(), 1);
        // L'interface redemande (attente échue de son côté) : l'ordonnanceur
        // répond sans requête, son attente à lui court toujours.
        for e in b.images.inner.borrow_mut().entries.values_mut() {
            if let Entry::Failed { retry_at, .. } = e {
                *retry_at = Some(Instant::now());
            }
        }
        assert_eq!(frame(&b.images, &[(avatar(1), 32.0)]), vec![Image::Pending]);
        frames(&b.images, &[(avatar(1), 32.0)], |g| g[0] == Image::Failed);
        assert_eq!(b.source.fetches(), 1);
    }

    /// Attente échue (ici nulle) : l'image est redemandée au dessin suivant,
    /// sans réveil programmé, et se charge.
    #[test]
    fn network_errors_are_retried_when_due() {
        let cfg = Config { worker: WorkerConfig { retry_base: Duration::ZERO, ..WorkerConfig::default() }, ..Config::default() };
        let b = bench(true, cfg);
        b.source.answer(Err(FetchError::Network("coupure".into())));
        frame(&b.images, &[(avatar(1), 32.0)]);
        until("premier téléchargement", || b.source.fetches() == 1);
        b.source.release(1);
        // Échec rapporté, et aucune image dessinée depuis : rien n'est redemandé.
        until("échec rapporté", || b.images.worker_stats().replies == 1);
        assert_eq!(b.source.fetches(), 1);
        b.source.answer(Ok(()));
        b.source.release(10);
        frames(&b.images, &[(avatar(1), 32.0)], all_ready);
        assert_eq!(b.source.fetches(), 2);
    }

    /// L'attente double à chaque erreur, plafonnée, et s'efface à la réussite.
    #[test]
    fn backoff_doubles_up_to_the_cap() {
        let (base, max) = (Duration::from_secs(5), Duration::from_secs(600));
        let mut m = worker::Memo::new(100, max);
        let t0 = tokio::time::Instant::now();
        let waits: Vec<u64> = (0..10).map(|_| m.failed("u".into(), t0, base, max).as_secs()).collect();
        assert_eq!(waits, vec![5, 10, 20, 40, 80, 160, 320, 600, 600, 600]);
        assert_eq!(m.waiting("u", t0), Some(max));
        assert_eq!(m.waiting("u", t0 + max), None, "échue");
        m.succeeded("u");
        assert_eq!(m.failed("u".into(), t0, base, max), base, "remise à zéro");
    }

    /// Mémoire des échecs bornée : au plafond, les attentes échues depuis
    /// longtemps partent d'abord ; au-delà, tout est vidé, comme les 404.
    #[test]
    fn failure_memo_is_bounded() {
        let (base, max) = (Duration::from_secs(1), Duration::from_secs(4));
        let mut m = worker::Memo::new(100, max);
        let t0 = tokio::time::Instant::now();
        for i in 0..100 {
            m.failed(format!("vieux{i}"), t0, base, max);
        }
        // Bien plus tard (attentes échues depuis plus de `max`) : les anciennes sont oubliées.
        let later = t0 + Duration::from_secs(60);
        for i in 0..50 {
            m.failed(format!("neuf{i}"), later, base, max);
        }
        assert_eq!(m.len().1, 50, "seules les récentes restent");
        assert_eq!(m.waiting("neuf0", later), Some(base));
        assert_eq!(m.waiting("vieux0", later), None);
        // Toutes récentes : jamais plus que le plafond.
        for i in 0..1000 {
            m.failed(format!("flot{i}"), later, base, max);
            assert!(m.len().1 <= 100);
        }
        // Lue après son échéance depuis longtemps : retirée.
        let much_later = later + Duration::from_secs(60);
        let before = m.len().1;
        assert_eq!(m.waiting("flot999", much_later), None);
        assert_eq!(m.len().1, before - 1);
        for i in 0..1000 {
            m.mark_gone(format!("absent{i}"));
            assert!(m.len().0 <= 100);
        }
    }

    #[test]
    fn memory_cap_holds_under_load() {
        let max_bytes = 1 << 20;
        let cfg = Config { memory: MemoryConfig { max_bytes, idle: Duration::from_secs(3600), ..MemoryConfig::default() }, ..Config::default() };
        let b = bench(false, cfg);
        let side = 64.0;
        let per = 64 * 64 * 4;
        for chunk in 0..100u64 {
            let refs: Vec<(ImageRef, f32)> = (chunk * 20..chunk * 20 + 20).map(|i| (avatar(i), side)).collect();
            frames(&b.images, &refs, all_ready);
            let s = b.images.stats();
            assert!(s.bytes <= max_bytes, "{} octets après {} images", s.bytes, (chunk + 1) * 20);
            assert_eq!(s.bytes, s.textures * per);
        }
        assert_eq!(b.source.fetches(), 2000);
        let s = b.images.stats();
        assert!(s.textures >= max_bytes / per - 20, "cache utilisé : {s:?}");
        assert!(!b.images.is_loaded(&b.images.key(avatar(0), side)), "la plus ancienne est évincée");
        assert!(b.images.is_loaded(&b.images.key(avatar(1999), side)), "la plus récente reste");
    }

    /// Sans délai réel : un délai d'inactivité nul libère une texture dès la
    /// première image où elle n'est plus affichée, et pas avant.
    #[test]
    fn idle_textures_are_released() {
        let cfg = Config { memory: MemoryConfig { idle: Duration::ZERO, ..MemoryConfig::default() }, ..Config::default() };
        let b = bench(false, cfg);
        frames(&b.images, &[(avatar(1), 32.0)], all_ready);
        for _ in 0..5 {
            assert!(all_ready(&frame(&b.images, &[(avatar(1), 32.0)])), "affichée : gardée");
        }
        frame(&b.images, &[]);
        assert_eq!(b.images.stats(), Stats::default());
    }

    /// Mémoire serrée : les textures des écrans quittés partent tout de
    /// suite, celles affichées restent.
    #[test]
    fn memory_warning_releases_hidden_textures() {
        let b = bench(false, Config::default());
        frames(&b.images, &[(avatar(1), 32.0), (avatar(2), 32.0)], all_ready);
        frame(&b.images, &[(avatar(1), 32.0)]);
        assert_eq!(b.images.stats().textures, 2, "sous le plafond et récente : gardée");
        memory_warning();
        frame(&b.images, &[(avatar(1), 32.0)]);
        assert_eq!(b.images.stats().textures, 1, "seule l'image affichée reste");
        assert!(all_ready(&frame(&b.images, &[(avatar(1), 32.0)])));
    }

    #[test]
    fn off_screen_requests_are_cancelled() {
        let cfg = Config { worker: WorkerConfig { concurrency: 2, ..WorkerConfig::default() }, ..Config::default() };
        let b = bench(true, cfg);
        let refs: Vec<(ImageRef, f32)> = (0..20).map(|i| (avatar(i), 32.0)).collect();
        frame(&b.images, &refs);
        until("deux téléchargements", || b.source.fetches() == 2);
        // Plus rien d'affiché : la file est vidée, les téléchargements en cours arrêtés.
        frames(&b.images, &[], |_| b.images.stats().pending == 0);
        until("téléchargements arrêtés", || b.source.n.running.load(SeqCst) == 0);
        // Les réponses libérées maintenant n'ont plus personne à servir.
        b.source.release(100);
        frame(&b.images, &[]);
        assert_eq!(b.source.fetches(), 2);
        assert_eq!(b.images.stats(), Stats::default());
        assert_eq!(b.images.worker_stats().decodes, 0, "rien de décodé pour personne");
    }

    #[test]
    fn one_wake_per_batch() {
        let b = bench(false, Config::default());
        let refs: Vec<(ImageRef, f32)> = (0..10).map(|i| (avatar(i), 32.0)).collect();
        assert!(frame(&b.images, &refs).iter().all(|i| *i == Image::Pending));
        // Aucune image dessinée pendant ce temps : un seul réveil pour tout le lot.
        until("dix images prêtes", || b.images.worker_stats().replies == 10);
        assert_eq!(b.wakes.load(SeqCst), 1);
        let got = frames(&b.images, &refs, all_ready);
        assert!(all_ready(&got), "{got:?}");
    }

    /// Au plus `uploads_per_frame` textures par image ; le reste à l'image
    /// suivante, avec un redessin demandé.
    #[test]
    fn uploads_are_spread_over_frames() {
        let b = bench(false, Config::default());
        let refs: Vec<(ImageRef, f32)> = (0..100).map(|i| (avatar(i), 32.0)).collect();
        frame(&b.images, &refs);
        until("cent images prêtes", || b.images.worker_stats().replies == 100);
        let per = MemoryConfig::default().uploads_per_frame;
        for n in 1..=3 {
            frame(&b.images, &refs);
            assert_eq!(b.images.stats().textures, n * per);
            assert!(b.images.ctx.has_requested_repaint(), "redessin demandé pour la suite");
        }
        let got = frame(&b.images, &refs);
        assert!(all_ready(&got));
        assert_eq!(b.source.fetches(), 100);
    }

    /// Rafales de demandes annulées pendant que l'interface ne reprend rien :
    /// jamais plus de `decoders` décodages simultanés, octets en attente
    /// plafonnés, et une demande annulée ne lance aucun décodage de plus.
    ///
    /// Déterministe : les réponses ne partent que libérées par le test, et
    /// chaque vérification se fait sur un état stable (compteurs exacts
    /// attendus), jamais après un délai.
    #[test]
    fn cancelled_bursts_keep_memory_bounded() {
        let queued = 64 << 10;
        let cfg = Config {
            // Aucune texture créée : tout ce qui est prêt reste en attente.
            memory: MemoryConfig { uploads_per_frame: 0, ..MemoryConfig::default() },
            worker: WorkerConfig { concurrency: 6, decoders: 2, queued_bytes: queued, ..WorkerConfig::default() },
        };
        let mut b = bench(true, cfg);
        let side = 64.0; // 16 Kio par image ajustée : quatre remplissent la file.
        let refs = |round: u64| -> Vec<(ImageRef, f32)> { (round * 30..round * 30 + 30).map(|i| (avatar(i), side)).collect() };
        let stats = |b: &Bench| b.images.worker_stats();
        let invariants = |b: &Bench| {
            let w = stats(b);
            assert!(w.peak_decoding <= 2, "{w:?}");
            assert!(w.peak_queued_bytes <= queued && w.queued_bytes <= queued, "{w:?}");
        };

        // Rafale 0 : six téléchargements, libérés ensemble. Quatre images
        // remplissent la file ; les deux autres attendent une place.
        frame(&b.images, &refs(0));
        until("six téléchargements", || b.source.fetches() == 6);
        b.source.release(6);
        until("file pleine", || {
            let w = stats(&b);
            w.replies == 4 && w.queued_bytes == queued && w.waiting_for_room == 2 && w.decoding == 0
        });
        // Les quatre chargements finis cèdent leur place à quatre autres (retenus).
        until("quatre suivants", || b.source.fetches() == 10 && b.source.n.running.load(SeqCst) == 4);
        assert_eq!(stats(&b).decodes, 6 + 4, "six images sources, quatre ajustées");
        invariants(&b);

        // Rafales suivantes : la précédente est annulée (attentes de place et
        // téléchargements retenus arrêtés), six nouveaux téléchargements
        // libérés ; leurs images sources sont décodées, puis attendent une
        // place qui ne vient jamais. Aucun ajustement.
        for round in 1..=5u64 {
            frame(&b.images, &refs(round));
            let fetched = 10 + 6 * round as usize;
            until("rafale précédente annulée", || {
                let w = stats(&b);
                b.source.fetches() == fetched && b.source.n.running.load(SeqCst) == 6 && w.waiting_for_room == 0
            });
            b.source.release(6);
            until("images sources décodées", || {
                let w = stats(&b);
                w.waiting_for_room == 6 && w.decoding == 0
            });
            let w = stats(&b);
            assert_eq!(w.decodes, 10 + 6 * round as usize, "un décodage par image source, aucun ajustement : {w:?}");
            assert_eq!(w.queued_bytes, queued);
            invariants(&b);
        }

        // Tout est annulé ; puis l'interface reprend les quatre images de la
        // file. Les places libérées ne servent à personne : les tâches qui
        // les attendaient sont arrêtées.
        frame(&b.images, &[]);
        until("tout annulé", || stats(&b).waiting_for_room == 0 && b.source.n.running.load(SeqCst) == 0);
        b.images.memory.uploads_per_frame = 32;
        frame(&b.images, &[]);
        let w = stats(&b);
        assert_eq!(w.queued_bytes, 0, "{w:?}");
        assert_eq!(b.images.stats().textures, 4);
        assert_eq!((w.decodes, w.discarded, b.source.fetches()), (40, 0, 40), "{w:?}");
        invariants(&b);
    }

    /// Panique du décodeur sur une image hostile : cette image échoue, le
    /// permis de décodage est rendu et les autres images se chargent.
    #[test]
    fn decoder_panic_fails_one_image_only() {
        let cfg = Config { worker: WorkerConfig { decoders: 1, ..WorkerConfig::default() }, ..Config::default() };
        let b = bench(false, cfg);
        let refs = [(avatar(HOSTILE), 32.0), (avatar(1), 32.0)];
        let got = frames(&b.images, &refs, |g| g.iter().all(|i| *i != Image::Pending));
        assert_eq!(got[0], Image::Failed);
        assert!(matches!(got[1], Image::Ready(_)));
        // Un seul permis de décodage : s'il n'avait pas été rendu, plus rien ne se chargerait.
        frames(&b.images, &[(avatar(2), 32.0), (avatar(3), 32.0)], all_ready);
        assert_eq!(b.images.worker_stats().decoding, 0);
        // Échec définitif : pas redemandé.
        let n = b.source.fetches();
        frames(&b.images, &refs, |g| g[0] == Image::Failed);
        assert_eq!(b.source.fetches(), n);
    }

    #[test]
    fn disabled_never_requests() {
        let images = Images::disabled(egui::Context::default());
        images.begin_frame();
        assert_eq!(images.get(avatar(1), 32.0), Image::Failed);
        images.end_frame();
        assert_eq!(images.stats(), Stats::default());
    }

    #[test]
    fn key_follows_screen_scale() {
        let images = Images::disabled(egui::Context::default());
        assert_eq!(images.key(avatar(1), 36.0).fit, Fit::Cover(36));
        images.ctx.set_pixels_per_point(1.5);
        images.ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        assert_eq!(images.key(avatar(1), 36.0).fit, Fit::Cover(54));
        assert_eq!(images.key(avatar(1), f32::NAN).fit, Fit::Cover(1));
    }
}
