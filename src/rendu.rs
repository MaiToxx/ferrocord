//! Choix du rendu de l'interface (lot « d3d », essai mesuré).
//!
//! Ferrocord dessine par OpenGL (`glow`). Avec la fonctionnalité Cargo
//! `rendu-wgpu` (désactivée par défaut), l'exécutable contient aussi le rendu
//! `wgpu` d'eframe, limité à Direct3D 12 ; le choix se fait alors au lancement :
//!
//! - `FERROCORD_RENDU=d3d12` ou `opengl` (variable d'environnement) ;
//! - sinon OpenGL.
//!
//! Direct3D 11 n'existe pas dans wgpu 30 (seuls D3D12, Vulkan, Metal et
//! OpenGL y sont) : `d3d11` est refusé comme une valeur inconnue.
//!
//! Repli : l'instance, la carte et le périphérique Direct3D 12 sont créés
//! ICI, avant la fenêtre. Si une étape échoue (pas de pilote D3D12, machine
//! virtuelle, session distante…), la fenêtre est créée en OpenGL comme si rien
//! n'avait été demandé, et [`repli`] dit pourquoi. Les mesures et la décision
//! sont dans `docs/mesures-d3d.md`.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

/// Variable d'environnement du choix du rendu.
pub const ENV: &str = "FERROCORD_RENDU";

/// Rendu de l'interface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Rendu {
    /// OpenGL par `glow` : le rendu de toujours, présent dans tous les exécutables.
    #[default]
    OpenGl,
    /// Direct3D 12 par `wgpu` : seulement avec la fonctionnalité `rendu-wgpu`, sous Windows.
    D3d12,
}

impl Rendu {
    /// Lit une valeur de [`ENV`] ; `None` si elle est inconnue.
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "opengl" | "gl" => Some(Self::OpenGl),
            "d3d12" | "dx12" => Some(Self::D3d12),
            _ => None,
        }
    }

    /// Nom écrit dans les rapports.
    pub fn name(self) -> &'static str {
        match self {
            Self::OpenGl => "opengl",
            Self::D3d12 => "d3d12",
        }
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
    if ACTIVE.load(Ordering::Relaxed) == 1 { Rendu::D3d12 } else { Rendu::OpenGl }
}

/// Raison du repli sur OpenGL, si Direct3D 12 avait été demandé et n'a pas pu servir.
pub fn repli() -> Option<&'static str> {
    FALLBACK.get().map(String::as_str)
}

/// Carte graphique retenue par Direct3D 12 (nom donné par le pilote), pour les rapports.
pub fn carte() -> Option<&'static str> {
    ADAPTER.get().map(String::as_str)
}

/// Règle `options` pour le rendu `wanted`. Direct3D 12 indisponible (absent
/// de l'exécutable, ou refusé par le système) : OpenGL, sans autre effet.
pub fn apply(options: eframe::NativeOptions, wanted: Rendu) -> eframe::NativeOptions {
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
        // wgpu n'a pas de dorsal Direct3D 11.
        assert_eq!(Rendu::parse("d3d11"), None);
        assert_eq!(Rendu::parse(""), None);
        for r in [Rendu::OpenGl, Rendu::D3d12] {
            assert_eq!(Rendu::parse(r.name()), Some(r));
        }
    }

    #[test]
    fn opengl_par_defaut() {
        assert_eq!(Rendu::default(), Rendu::OpenGl);
        // Sans demande, rien ne change : le rendu reste OpenGL.
        let _ = apply(eframe::NativeOptions::default(), Rendu::OpenGl);
        assert_eq!(active(), Rendu::OpenGl);
        assert!(repli().is_none());
    }
}
