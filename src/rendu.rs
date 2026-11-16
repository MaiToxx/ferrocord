//! Choix du rendu de l'interface (lot « d3d », essai mesuré).
//!
//! Ferrocord dessine par OpenGL (`glow`). Sous Windows x64, le contexte vient
//! d'ANGLE (OpenGL ES sur Direct3D 11) quand ses deux bibliothèques sont posées à
//! côté de l'exécutable (lot « gl », [`angle`]) : même rendu `glow`, pilote bien
//! plus léger ; sinon du pilote OpenGL du fabricant, comme toujours. Avec la fonctionnalité Cargo
//! `rendu-wgpu` (désactivée par défaut), l'exécutable contient aussi le rendu
//! `wgpu` d'eframe, limité à Direct3D 12 ; le choix se fait alors au lancement :
//!
//! - `FERROCORD_RENDU=angle`, `opengl` ou `d3d12` (variable d'environnement) ;
//! - sinon ANGLE s'il est posé, OpenGL à défaut.
//!
//! Direct3D 11 n'existe pas dans wgpu 30 (seuls D3D12, Vulkan, Metal et
//! OpenGL y sont) : `d3d11` désigne ANGLE, qui dessine par Direct3D 11.
//!
//! Repli : l'instance, la carte et le périphérique Direct3D 12 sont créés
//! ICI, avant la fenêtre. Si une étape échoue (pas de pilote D3D12, machine
//! virtuelle, session distante…), la fenêtre est créée en OpenGL comme si rien
//! n'avait été demandé, et [`repli`] dit pourquoi. Les mesures et la décision
//! sont dans `docs/mesures-d3d.md`.

pub mod angle;

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

/// Variable d'environnement du choix du rendu.
pub const ENV: &str = "FERROCORD_RENDU";

/// Rendu de l'interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rendu {
    /// OpenGL par `glow` et le pilote OpenGL de la carte : présent dans tous les exécutables.
    OpenGl,
    /// Le même `glow`, sur un contexte d'ANGLE (Direct3D 11) : défaut sous Windows x64.
    Angle,
    /// Direct3D 12 par `wgpu` : seulement avec la fonctionnalité `rendu-wgpu`, sous Windows.
    D3d12,
}

impl Rendu {
    /// Lit une valeur de [`ENV`] ; `None` si elle est inconnue.
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "opengl" | "gl" => Some(Self::OpenGl),
            "angle" | "d3d11" => Some(Self::Angle),
            "d3d12" | "dx12" => Some(Self::D3d12),
            _ => None,
        }
    }

    /// Nom écrit dans les rapports.
    pub fn name(self) -> &'static str {
        match self {
            Self::OpenGl => "opengl",
            Self::Angle => "angle",
            Self::D3d12 => "d3d12",
        }
    }
}

impl Default for Rendu {
    /// ANGLE là où il est embarqué (Windows x64) ; [`apply`] se replie sur OpenGL s'il n'est pas posé.
    fn default() -> Self {
        if cfg!(all(windows, target_arch = "x86_64")) { Self::Angle } else { Self::OpenGl }
    }
}

/// L'exécutable contient-il le rendu Direct3D 12 ?
pub const D3D12_COMPILED: bool = cfg!(all(windows, feature = "rendu-wgpu"));

/// Rendu demandé par [`ENV`], s'il y est et s'il est connu.
pub fn from_env() -> Option<Rendu> {
    std::env::var(ENV).ok().as_deref().and_then(Rendu::parse)
}

static ACTIVE: AtomicU8 = AtomicU8::new(0);
static FALLBACK: OnceLock<String> = OnceLock::new();
static ADAPTER: OnceLock<String> = OnceLock::new();

/// Rendu réellement utilisé par la fenêtre de ce processus.
pub fn active() -> Rendu {
    match ACTIVE.load(Ordering::Relaxed) {
        1 => Rendu::D3d12,
        2 => Rendu::Angle,
        _ => Rendu::OpenGl,
    }
}

/// Raison du repli sur OpenGL, si ANGLE ou Direct3D 12 était demandé et n'a pas pu servir.
pub fn repli() -> Option<&'static str> {
    FALLBACK.get().map(String::as_str)
}

/// Carte graphique retenue par Direct3D 12 (nom donné par le pilote), pour les rapports.
pub fn carte() -> Option<&'static str> {
    ADAPTER.get().map(String::as_str)
}

/// [`apply`] pour l'application : un démarrage sous ANGLE qui n'atteint pas l'affichage
/// (pilote Direct3D défaillant) ne doit pas se répéter à chaque lancement. Un témoin
/// ([`angle::TRIAL`]) est posé ici, retiré par [`confirm`] (image affichée) ou par
/// [`finish`] (fenêtre fermée sans erreur avant) ; trouvé au lancement suivant de la même
/// version, il fait revenir au pilote OpenGL. La raison d'un repli va au journal technique.
pub fn apply_guarded(options: eframe::NativeOptions, wanted: Rendu) -> eframe::NativeOptions {
    let _ = STARTED.set(std::time::Instant::now());
    watch_errors();
    let dir = angle::exe_dir().filter(|d| wanted == Rendu::Angle && angle::present(d));
    let options = match dir {
        None => apply(options, wanted),
        Some(dir) if angle::trial_begin(&dir) == angle::Trial::Failed => {
            let _ = FALLBACK.set("le dernier démarrage de cette version sous ANGLE n'a pas atteint l'affichage".to_string());
            apply(options, Rendu::OpenGl)
        }
        Some(dir) => {
            let options = apply(options, wanted);
            if active() != Rendu::Angle {
                angle::trial_end(&dir);
            }
            options
        }
    };
    if wanted == Rendu::Angle && active() != Rendu::Angle && cfg!(all(windows, target_arch = "x86_64")) {
        crate::crash::note("rendu_repli_opengl");
    }
    options
}

/// Instant de [`apply_guarded`] : une perte du rendu juste après le démarrage n'est pas traitée comme une perte tardive.
static STARTED: OnceLock<std::time::Instant> = OnceLock::new();
/// eframe a quitté la boucle d'événements sur une erreur de rendu.
static LOST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Durée en dessous de laquelle une perte du rendu condamne ANGLE pour cette version (pas de boucle de relances).
const EARLY: std::time::Duration = std::time::Duration::from_secs(20);
/// Durée au-delà de laquelle une perte du rendu hors d'ANGLE (pilote OpenGL, Direct3D 12) vaut une relance.
const LONG: std::time::Duration = std::time::Duration::from_secs(300);

/// eframe ne rend pas l'erreur qui lui fait quitter la boucle d'événements (contexte
/// perdu, échange de tampons refusé…) : il l'écrit au journal `log`, puis ferme la
/// fenêtre comme si l'utilisateur l'avait demandé. Ce journaliseur ne garde rien : il
/// reconnaît ce seul message et lève [`LOST`]. (Ferrocord n'a pas d'autre journaliseur
/// `log` ; s'il y en avait un, celui-ci ne s'installerait pas et rien d'autre ne changerait.)
#[cfg(not(windows))]
fn watch_errors() {}

#[cfg(windows)]
fn watch_errors() {
    struct Watch;
    impl log::Log for Watch {
        fn enabled(&self, m: &log::Metadata<'_>) -> bool {
            m.level() == log::Level::Error && m.target().starts_with("eframe")
        }
        fn log(&self, r: &log::Record<'_>) {
            if self.enabled(r.metadata()) && r.args().to_string().starts_with("Exiting because of error") {
                LOST.store(true, Ordering::SeqCst);
            }
        }
        fn flush(&self) {}
    }
    if log::set_logger(&Watch).is_ok() {
        log::set_max_level(log::LevelFilter::Error);
    }
}

/// Ce qu'il faut faire quand la fenêtre s'est fermée.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// Fermeture demandée : rien.
    Closed,
    /// Le rendu a été perdu (périphérique Direct3D retiré : pilote réinitialisé ou mis à
    /// jour, sortie de veille, bureau à distance, carte changée…) : relancer l'application.
    /// Un contexte perdu ne se répare pas dans le processus : toutes les textures seraient à
    /// refaire, et eframe a déjà fermé la fenêtre.
    Relaunch,
}

/// À appeler quand la boucle d'événements a rendu la main.
///
/// - Fermeture sans erreur avant la première image (l'utilisateur a fermé pendant le
///   démarrage) : le témoin est retiré, ce n'est pas un démarrage raté.
/// - Erreur de rendu sous ANGLE moins de vingt secondes après le lancement (démarrage
///   compris) : le témoin est laissé comme après un démarrage raté ; la relance se fait
///   donc sur le pilote OpenGL, et ANGLE attend la version suivante. Pas de boucle.
/// - Erreur de rendu sous ANGLE plus tard : relance sous ANGLE (un nouveau processus
///   obtient un nouveau périphérique ; s'il n'y arrive pas, le témoin fait le reste).
/// - Erreur de rendu hors d'ANGLE (pilote OpenGL, Direct3D 12), le dernier recours : une
///   relance seulement si la fenêtre a tenu cinq minutes ; sinon elle reste fermée, comme avant.
pub fn finish() -> End {
    let dir = angle::exe_dir();
    if !LOST.load(Ordering::SeqCst) {
        if let Some(dir) = &dir {
            angle::trial_end(dir);
        }
        return End::Closed;
    }
    let up = STARTED.get().map_or(std::time::Duration::ZERO, std::time::Instant::elapsed);
    let early = up < EARLY;
    crate::crash::note_n("rendu_perdu", i64::from(early));
    match active() {
        Rendu::Angle => {
            if early && let Some(dir) = &dir {
                angle::trial_fail(dir);
            }
            End::Relaunch
        }
        // Dernier recours : une relance seulement si la fenêtre a tenu longtemps (pas de boucle).
        _ if up >= LONG => End::Relaunch,
        _ => End::Closed,
    }
}

/// À appeler à la création de l'application lancée par [`apply_guarded`] : retire le témoin
/// d'ANGLE quand une image a été affichée (au début de la troisième passe d'egui, les deux
/// premières étant demandées ici ; ensuite, deux lectures d'un compteur par image).
pub fn confirm(ctx: &eframe::egui::Context) {
    simulate_loss(ctx);
    if active() != Rendu::Angle {
        return;
    }
    let Some(dir) = angle::exe_dir() else { return };
    let ended = std::sync::Arc::new(AtomicU8::new(0));
    let seen = ended.clone();
    ctx.on_end_pass(
        "rendu_angle_fin",
        std::sync::Arc::new(move |ctx| {
            if seen.load(Ordering::Relaxed) < 2 {
                seen.fetch_add(1, Ordering::Relaxed);
                ctx.request_repaint();
            }
        }),
    );
    ctx.on_begin_pass(
        "rendu_angle_debut",
        std::sync::Arc::new(move |_| {
            if ended.load(Ordering::Relaxed) == 2 {
                ended.store(3, Ordering::Relaxed);
                angle::trial_end(&dir);
            }
        }),
    );
}

/// Variable d'environnement d'essai : `FERROCORD_RENDU_PERTE=<n>` fait perdre le rendu à
/// la n-ième passe d'egui (voir [`simulate_loss`]). Sans elle, rien n'est installé.
pub const ENV_LOSS: &str = "FERROCORD_RENDU_PERTE";

/// Essai du chemin « rendu perdu » ([`finish`]) : Direct3D n'offre aucun moyen de retirer
/// un périphérique sans toucher au pilote. À la passe demandée par [`ENV_LOSS`], la
/// surface EGL de la fenêtre est donc détruite dans le dos d'eframe : l'échange de tampons
/// suivant échoue, exactement là où `EGL_CONTEXT_LOST` arriverait, et eframe quitte la
/// boucle d'événements par le même chemin. Sous ANGLE seulement.
fn simulate_loss(ctx: &eframe::egui::Context) {
    let Some(at) = std::env::var(ENV_LOSS).ok().and_then(|v| v.trim().parse::<u64>().ok()) else { return };
    ctx.on_begin_pass(
        "rendu_perte",
        std::sync::Arc::new(move |ctx| match ctx.cumulative_pass_nr().cmp(&at) {
            std::cmp::Ordering::Less => ctx.request_repaint(),
            std::cmp::Ordering::Equal => {
                destroy_surface();
                ctx.request_repaint();
            }
            std::cmp::Ordering::Greater => {}
        }),
    );
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn destroy_surface() {}

/// Détruit la surface EGL courante du fil appelant (essai seulement, voir [`simulate_loss`]).
#[cfg(all(windows, target_arch = "x86_64"))]
fn destroy_surface() {
    use std::ffi::c_void;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows::core::{HSTRING, s};
    /// `EGL_DRAW` : la surface de dessin du contexte courant.
    const EGL_DRAW: i32 = 0x3059;
    if active() != Rendu::Angle {
        return;
    }
    // SAFETY: `libEGL.dll` est la bibliothèque d'ANGLE chargée par `angle::activate` et jamais
    // déchargée ; les trois fonctions sont celles de la norme EGL, avec leurs signatures
    // (convention d'appel unique en x64) ; elles sont appelées sur le fil d'interface, celui du
    // contexte courant, avec les poignées qu'EGL vient de rendre lui-même.
    unsafe {
        let Ok(egl) = GetModuleHandleW(&HSTRING::from("libEGL.dll")) else { return };
        let (Some(display), Some(surface), Some(destroy)) = (GetProcAddress(egl, s!("eglGetCurrentDisplay")), GetProcAddress(egl, s!("eglGetCurrentSurface")), GetProcAddress(egl, s!("eglDestroySurface"))) else {
            return;
        };
        let display: unsafe extern "system" fn() -> *mut c_void = std::mem::transmute(display);
        let surface: unsafe extern "system" fn(i32) -> *mut c_void = std::mem::transmute(surface);
        let destroy: unsafe extern "system" fn(*mut c_void, *mut c_void) -> u32 = std::mem::transmute(destroy);
        destroy(display(), surface(EGL_DRAW));
    }
}

/// Règle `options` pour le rendu `wanted`. Direct3D 12 indisponible (absent
/// de l'exécutable, ou refusé par le système) : OpenGL, sans autre effet.
pub fn apply(options: eframe::NativeOptions, wanted: Rendu) -> eframe::NativeOptions {
    // ANGLE : le rendu reste `glow`, seule la source du contexte change.
    let wanted = if wanted == Rendu::Angle {
        match angle::activate() {
            Ok(()) => ACTIVE.store(2, Ordering::Relaxed),
            Err(why) => {
                let _ = FALLBACK.set(why);
            }
        }
        Rendu::OpenGl
    } else {
        wanted
    };
    #[cfg(all(windows, feature = "rendu-wgpu"))]
    {
        d3d12::apply(options, wanted)
    }
    #[cfg(not(all(windows, feature = "rendu-wgpu")))]
    {
        if wanted == Rendu::D3d12 {
            let _ = FALLBACK.set("rendu Direct3D 12 absent de cet exécutable (fonctionnalité Cargo « rendu-wgpu »)".to_string());
            ACTIVE.store(0, Ordering::Relaxed);
        }
        options
    }
}

#[cfg(all(windows, feature = "rendu-wgpu"))]
mod d3d12 {
    use super::{ACTIVE, FALLBACK, Rendu};
    use eframe::egui_wgpu::{WgpuSetup, WgpuSetupExisting};
    use eframe::wgpu;
    use std::sync::atomic::Ordering;

    pub(super) fn apply(mut options: eframe::NativeOptions, wanted: Rendu) -> eframe::NativeOptions {
        // Les deux rendus compilés, eframe choisirait wgpu par défaut : OpenGL est nommé.
        options.renderer = eframe::Renderer::Glow;
        if wanted != Rendu::D3d12 {
            return options;
        }
        match setup(options.viewport.transparent.unwrap_or(false)) {
            Ok(existing) => {
                options.renderer = eframe::Renderer::Wgpu;
                options.wgpu_options.wgpu_setup = WgpuSetup::Existing(existing);
                ACTIVE.store(1, Ordering::Relaxed);
            }
            Err(why) => {
                let _ = FALLBACK.set(why);
            }
        }
        options
    }

    /// Instance, carte et périphérique Direct3D 12, créés avant la fenêtre pour
    /// pouvoir se replier sur OpenGL sans rien avoir affiché.
    fn setup(transparent: bool) -> Result<WgpuSetupExisting, String> {
        let dx12 = wgpu::Dx12BackendOptions {
            // FXC est dans Windows (d3dcompiler_47.dll) : rien à embarquer.
            shader_compiler: wgpu::Dx12Compiler::Fxc,
            // Fenêtre transparente (Mica) : seule la chaîne d'échange posée sur
            // un visuel DirectComposition garde le canal alpha.
            presentation_system: if transparent { wgpu::Dx12SwapchainKind::DxgiFromVisual } else { wgpu::Dx12SwapchainKind::DxgiFromHwnd },
            ..Default::default()
        };
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            // Ni validation ni noms de débogage : c'est le pilote livré qu'on mesure.
            flags: wgpu::InstanceFlags::empty(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            backend_options: wgpu::BackendOptions { dx12, ..Default::default() },
            display: None,
        });
        // Carte économe d'abord : une interface 2D ne doit pas réveiller la carte dédiée d'un portable
        // (`WGPU_POWER_PREF=high` choisit l'autre, pour les mesures).
        let adapter = now(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::from_env().unwrap_or(wgpu::PowerPreference::LowPower), ..Default::default() }))
            .map_err(|e| format!("aucune carte Direct3D 12 : {e}"))?;
        let _ = super::ADAPTER.set(adapter.get_info().name);
        let (device, queue) = now(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("ferrocord"),
            // Comme eframe : une texture aussi grande qu'un écran 8K.
            required_limits: wgpu::Limits { max_texture_dimension_2d: 8192, ..wgpu::Limits::default() },
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .map_err(|e| format!("périphérique Direct3D 12 refusé : {e}"))?;
        Ok(WgpuSetupExisting { instance, adapter, device, queue })
    }

    /// Attend un futur de wgpu : hors du web, ils sont prêts dès le premier
    /// appel (les attentes se font dans l'appel lui-même), la boucle ne tourne pas.
    fn now<F: Future>(future: F) -> F::Output {
        let mut future = std::pin::pin!(future);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        loop {
            if let std::task::Poll::Ready(out) = future.as_mut().poll(&mut cx) {
                return out;
            }
            std::thread::yield_now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valeurs_connues_et_inconnues() {
        assert_eq!(Rendu::parse("opengl"), Some(Rendu::OpenGl));
        assert_eq!(Rendu::parse(" D3D12 "), Some(Rendu::D3d12));
        assert_eq!(Rendu::parse("dx12"), Some(Rendu::D3d12));
        // wgpu n'a pas de dorsal Direct3D 11 : c'est ANGLE qui dessine par lui.
        assert_eq!(Rendu::parse("d3d11"), Some(Rendu::Angle));
        assert_eq!(Rendu::parse("Angle"), Some(Rendu::Angle));
        assert_eq!(Rendu::parse("vulkan"), None);
        assert_eq!(Rendu::parse(""), None);
        for r in [Rendu::OpenGl, Rendu::Angle, Rendu::D3d12] {
            assert_eq!(Rendu::parse(r.name()), Some(r));
        }
    }

    #[test]
    fn angle_par_defaut_et_opengl_a_defaut() {
        assert_eq!(Rendu::default(), if cfg!(all(windows, target_arch = "x86_64")) { Rendu::Angle } else { Rendu::OpenGl });
        // OpenGL demandé : rien ne change.
        let _ = apply(eframe::NativeOptions::default(), Rendu::OpenGl);
        assert_eq!(active(), Rendu::OpenGl);
        assert!(repli().is_none());
        // ANGLE demandé sans ses bibliothèques à côté de l'exécutable de test :
        // OpenGL, avec la raison, et la préférence de glutin-winit intacte.
        let _ = apply(eframe::NativeOptions::default(), Rendu::Angle);
        assert_eq!(active(), Rendu::OpenGl);
        assert!(repli().is_some());
        #[cfg(windows)]
        assert!(!glutin_winit::PREFER_EGL.load(Ordering::Relaxed));
    }
}
