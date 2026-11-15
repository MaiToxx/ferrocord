//! Ferrocord — client Discord natif (non officiel) en Rust.
//!
//! La bibliothèque existe pour que les tests d'intégration (`tests/`) et
//! l'aperçu hors ligne (`examples/preview.rs`) puissent utiliser `net`,
//! `state`, `theme` et `ui` ; l'exécutable se contente d'appeler [`run`].

pub mod account;
pub mod attach;
pub mod audio;
pub mod expr_pick;
// Programme ajouté à la main comme jeu (lot C3).
pub mod game_pick;
pub mod avatar;
// Rapport de plantage local (lot E5).
pub mod crash;
pub mod desk;
pub mod guild_icon;
pub mod i18n;
pub mod images;
// Installation, liens, instance unique, mises à jour, licences (lot C6).
pub mod install;
pub mod look;
pub mod machine;
pub mod net;
// Historique hors ligne chiffré sur le disque (lot H).
pub mod offline;
pub mod panics;
// Journal de performance facultatif (FERROCORD_PERF=1, lot mesures).
pub mod perf;
// Fonctions propres à Ferrocord, activables dans Paramètres > Plugins (lot P0).
pub mod plugins;
// Dossiers, bureau et point d'entrée selon le système (phase 7 : Android).
pub mod platform;
// Programmes en cours, pour la détection des jeux (lot C3).
pub mod procs;
// Choix du rendu : OpenGL, ou Direct3D 12 à l'essai (lot « d3d »).
pub mod rendu;
// Rich Presence locale : serveur des tubes discord-ipc (lot R).
pub mod rpc;
pub mod session;
pub mod settings;
pub mod state;
pub mod theme;
pub mod ui;
// Vidéo et partage d'écran (phase 5) : types communs.
pub mod video;
pub mod voice;

use eframe::egui;
use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};
use winit::{application::ApplicationHandler, event_loop::ActiveEventLoop};

/// Options de la fenêtre principale (partagées avec l'aperçu).
pub fn native_options() -> eframe::NativeOptions {
    native_options_for(&settings::Window::default())
}

/// Options de la fenêtre, rouverte à la taille où on l'avait laissée. La
/// position n'est pas donnée ici : [`launch`] ne l'applique que si elle
/// tombe sur un écran présent.
pub fn native_options_for(w: &settings::Window) -> eframe::NativeOptions {
    let [width, height] = w.size.unwrap_or(settings::DEFAULT_SIZE);
    let viewport = egui::ViewportBuilder::default()
        .with_title("Ferrocord")
        .with_inner_size([width, height])
        .with_min_inner_size([settings::WIDTH.0, settings::HEIGHT.0])
        .with_maximized(w.maximized);
    eframe::NativeOptions { viewport, ..Default::default() }
}

/// Barre de titre intégrée (lot V1) : si le thème `first` lu au lancement la
/// demande (ou le thème par défaut, sans fichier), la fenêtre est créée sans
/// décoration ; `ui::titlebar` dessine la barre et Windows garde le
/// déplacement, le redimensionnement et l'ancrage (`platform::caption`).
pub fn with_titlebar(mut options: eframe::NativeOptions, first: Option<&theme::Tokens>) -> eframe::NativeOptions {
    let integrated = first.map_or_else(|| theme::Tokens::default().titlebar_integrated(), theme::Tokens::titlebar_integrated);
    if integrated {
        options.viewport = options.viewport.with_decorations(false);
    }
    options
}

/// Mica et transparence de Windows 11 (lot E5) : avec `on`, la fenêtre est
/// créée transparente, pour que la matière du système se voie derrière les
/// zones que l'interface peint en voile. C'est un réglage de création : le
/// plugin ne vaut donc qu'au lancement suivant. Sans effet hors Windows.
pub fn with_backdrop(mut options: eframe::NativeOptions, on: bool) -> eframe::NativeOptions {
    if on && cfg!(windows) {
        options.viewport = options.viewport.with_transparent(true);
        platform::backdrop::set_transparent(true);
    }
    options
}

/// Rendu de la fenêtre (lot « d3d ») : OpenGL, sauf si l'exécutable contient
/// Direct3D 12 (fonctionnalité `rendu-wgpu`) et que `FERROCORD_RENDU` le
/// demande. À appeler en dernier : la transparence de la fenêtre compte.
/// [`launch`] le fait ; seuls les lancements directs par eframe l'appellent.
pub fn with_rendu(options: eframe::NativeOptions) -> eframe::NativeOptions {
    rendu::apply(options, rendu::from_env().unwrap_or_default())
}

/// Le plugin « Mica » est-il activé dans `local.toml` ? Lu ici, avant la
/// fenêtre (le fil des réglages ne le lit qu'après). Le fichier n'est analysé
/// que s'il nomme le plugin : coupé par défaut, il n'y figure pas.
fn backdrop_at_boot() -> bool {
    let key = plugins::get(plugins::Id::Mica).key;
    cfg!(windows)
        && platform::backdrop::supported()
        && machine::default_dir().and_then(|d| std::fs::read_to_string(d.join("local.toml")).ok()).is_some_and(|t| t.contains(key) && state::local::LocalPrefs::parse(&t).is_ok_and(|p| p.plugins.on(plugins::Id::Mica)))
}

/// Ouvre la fenêtre `options` et fait tourner `app` jusqu'à sa fermeture.
/// La position retenue de la fenêtre (`window`) n'est appliquée que si sa
/// barre de titre recoupe un écran présent ([`settings::visible_position`]) :
/// les écrans sont listés au démarrage de la boucle d'événements, juste
/// avant que la fenêtre soit créée. Sinon, le système choisit la place.
pub fn launch(app_name: &str, options: eframe::NativeOptions, window: settings::Window, app: eframe::AppCreator<'_>) -> eframe::Result {
    let mut options = with_rendu(options);
    let monitors: Rc<RefCell<Option<Vec<settings::Monitor>>>> = Rc::default();
    let seen = monitors.clone();
    let previous = options.window_builder.take();
    options.window_builder = Some(Box::new(move |builder| {
        let builder = match previous {
            Some(hook) => hook(builder),
            None => builder,
        };
        match seen.borrow().as_deref().and_then(|m| settings::visible_position(&window, m)) {
            Some(p) => builder.with_position(p),
            None => builder,
        }
    }));
    #[cfg_attr(not(target_os = "android"), allow(unused_mut))]
    let mut builder = winit::event_loop::EventLoop::<eframe::UserEvent>::with_user_event();
    #[cfg(target_os = "android")]
    if let Some(app) = platform::android::app() {
        use winit::platform::android::EventLoopBuilderExtAndroid;
        builder.with_android_app(app);
    }
    let event_loop = builder.build()?;
    let inner = eframe::create_native(app_name, options, app, &event_loop);
    event_loop.run_app(&mut WithMonitors { inner, monitors })?;
    Ok(())
}

/// Liste les écrans avant que eframe crée la fenêtre, puis lui laisse tout.
struct WithMonitors<'a> {
    inner: eframe::EframeWinitApplication<'a>,
    monitors: Rc<RefCell<Option<Vec<settings::Monitor>>>>,
}

impl ApplicationHandler<eframe::UserEvent> for WithMonitors<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.monitors.borrow().is_none() {
            let list = event_loop
                .available_monitors()
                .map(|m| {
                    let scale = m.scale_factor();
                    let p = m.position().to_logical::<f32>(scale);
                    let s = m.size().to_logical::<f32>(scale);
                    settings::Monitor { position: [p.x, p.y], size: [s.width, s.height] }
                })
                .collect();
            *self.monitors.borrow_mut() = Some(list);
        }
        self.inner.resumed(event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: winit::window::WindowId, event: winit::event::WindowEvent) {
        self.inner.window_event(event_loop, id, event);
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: winit::event::StartCause) {
        self.inner.new_events(event_loop, cause);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: eframe::UserEvent) {
        self.inner.user_event(event_loop, event);
    }

    fn device_event(&mut self, event_loop: &ActiveEventLoop, id: winit::event::DeviceId, event: winit::event::DeviceEvent) {
        self.inner.device_event(event_loop, id, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.inner.about_to_wait(event_loop);
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        self.inner.suspended(event_loop);
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        self.inner.exiting(event_loop);
    }

    fn memory_warning(&mut self, event_loop: &ActiveEventLoop) {
        // Android (`onLowMemory`) : textures des écrans quittés libérées à la prochaine image.
        images::memory_warning();
        self.inner.memory_warning(event_loop);
    }
}

/// Relevés de `settings.toml` : rarement modifié à la main, une seconde suffit.
fn settings_watch() -> theme::WatchConfig {
    theme::WatchConfig { poll: if cfg!(target_os = "android") { ANDROID_POLL } else { Duration::from_secs(1) }, ..theme::WatchConfig::default() }
}

/// Android : personne ne modifie à la main les fichiers du stockage privé ;
/// un relevé par minute au lieu de quatre par seconde réveille moins le
/// processeur (en arrière-plan compris). Les écritures, elles, ne dépendent
/// pas de ce délai.
const ANDROID_POLL: Duration = Duration::from_secs(60);

fn theme_watch() -> theme::WatchConfig {
    if cfg!(target_os = "android") { theme::WatchConfig { poll: ANDROID_POLL, ..theme::WatchConfig::default() } } else { theme::WatchConfig::default() }
}

/// Ouvre la fenêtre et fait tourner l'application jusqu'à sa fermeture.
pub fn run() -> eframe::Result {
    let started = std::time::Instant::now();
    // Journal de performance (désactivé par défaut) : ouvert d'abord, pour
    // dater aussi les étapes d'avant la fenêtre.
    let rec = perf::from_env(started);
    let stage = |name: &'static str| {
        crash::note(name);
        if let Some(r) = &rec {
            r.stage(name);
        }
    };
    #[cfg(windows)]
    if let (Some(r), Some(age)) = (&rec, perf::process::age_ms()) {
        // Âge du processus au début de `run` : chargement de l'exécutable et des DLL.
        r.record_key(std::borrow::Cow::Borrowed("etape.processus_avant_run"), age);
    }
    // Toute panique arrête l'application, sauf celles d'un décodeur sur une donnée reçue.
    panics::install();
    crash::install(crash::default_dir());
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let args = install::single::parse_args(raw_args.iter().cloned());
    // Installation, désinstallation, ou proposition d'installer un exécutable
    // téléchargé (une seule fois ; jamais pour un exécutable de compilation).
    #[cfg(windows)]
    match install::setup::at_startup(&raw_args, args.link.is_none() && !args.minimized && !args.takeover) {
        install::setup::Next::Continue => {}
        install::setup::Next::Exit(0) => return Ok(()),
        install::setup::Next::Exit(code) => std::process::exit(code),
    }
    // Version téléchargée à installer, ou version à l'essai qui n'a pas
    // démarré : avant tout le reste, puis relance de l'exécutable.
    #[cfg(not(target_os = "android"))]
    if let Some(layout) = install::update::swap::Layout::current()
        && let Ok(install::update::swap::Startup::Relaunch) = install::update::swap::startup(&layout)
    {
        relaunch(&layout.exe, &args, false);
        return Ok(());
    }
    stage("remplacement");
    // Instance unique : un second lancement transmet son lien puis s'arrête.
    let wait = Duration::from_secs(if args.takeover { 10 } else { 2 });
    let requests = match install::single::claim(&install::single::pipe_name(), &args.request(), args.takeover, wait) {
        install::single::Claim::Forwarded => return Ok(()),
        install::single::Claim::First(l) => Some(l),
        install::single::Claim::Unavailable => None,
    };
    stage("instance_unique");
    let launch_args = args.clone();
    // `theme.toml` et `settings.toml` sont lus ici, avant la fenêtre : la
    // première image a le bon thème, et la fenêtre sa taille et sa place.
    // Les réglages d'abord : la langue enregistrée vaut avant tout autre
    // texte (erreurs de `theme.toml` comprises) et avant l'interface.
    let settings = settings::SettingsLink::open(settings::default_path(), true, settings_watch());
    let (window, language) = match &settings.1 {
        Some(settings::SettingsUpdate::Loaded(s)) => (s.window, s.language),
        _ => (settings::Window::default(), i18n::Choice::System),
    };
    i18n::set_language(language);
    stage("reglages");
    let theme = theme::ThemeLink::open(theme::default_path(), true, theme_watch());
    stage("theme");
    let env_token = session::env_token();
    let legacy = account::legacy_path();
    let lock = account::InstanceLock::new(account::lock_path(), account::lock::LOCK_WAIT);
    stage("avant_fenetre");
    let first_theme = match &theme.1 {
        Some(theme::Update::Loaded(t)) => Some(t),
        _ => None,
    };
    let options = with_backdrop(with_titlebar(native_options_for(&window), first_theme), backdrop_at_boot());
    let result = launch(
        "Ferrocord",
        options,
        window,
        Box::new(move |cc| {
            // Fenêtre et contexte OpenGL prêts : eframe les crée avant l'application.
            let stage = |name: &'static str| {
                crash::note(name);
                if let Some(r) = &rec {
                    r.stage(name);
                }
            };
            stage("fenetre_et_opengl");
            // Identité annoncée à Discord, figée pour ce lancement (réglage de local.toml).
            let identity = net::identity::Config::for_app();
            let session = session::Session::with_identity(identity.clone());
            let voice_rt = session.handle().clone();
            stage("session");
            let wake_ctx = cc.egui_ctx.clone();
            // Le CDN de production uniquement, comme pour l'API : aucune adresse réglable.
            let cdn: Arc<dyn images::Source> = Arc::new(net::cdn::Cdn::from_endpoints(&net::Endpoints { identity: identity.clone(), ..net::Endpoints::default() }));
            let desk_ctx = cc.egui_ctx.clone();
            let desk = platform::desk(Arc::new(move || desk_ctx.request_repaint()), cdn.clone(), images::default_dir());
            let images = images::Images::start(
                cc.egui_ctx.clone(),
                session.handle(),
                cdn,
                images::default_dir().map(images::DiskConfig::new),
                images::Config::default(),
                Arc::new(move || wake_ctx.request_repaint()),
            );
            stage("bureau_et_images");
            let accounts = account::AccountLink::start_with(Arc::new(account::SystemVault::new()), account::LinkOptions { lock, ..Default::default() });
            let boot = ui::Boot {
                backend: Box::new(session),
                store: {
                    // Liste des jeux détectables (lot C3), à côté du cache des images.
                    let mut s = state::Store::new();
                    s.activity.cache_path = images::default_dir().and_then(|d| Some(d.parent()?.join("detectable.json")));
                    // Usage local des emojis (lot FF), à côté des réglages.
                    s.gifs.frecency.local.dir = platform::config_dir().map(|d| d.join("ferrocord"));
                    s
                },
                theme,
                settings,
                accounts,
                env_token,
                legacy,
                images,
            };
            stage("comptes");
            let mut app = ui::App::new(cc, boot);
            stage("application");
            app.attach_desktop(desk);
            let local_ctx = cc.egui_ctx.clone();
            app.attach_machine(Box::new(machine::MachineFiles::start(machine::default_dir(), Arc::new(move || local_ctx.request_repaint()))));
            // Domaines de confiance des liens externes (lot VID), à côté de local.toml.
            app.attach_trusted_links(machine::default_dir().map(|d| d.join("domaines-de-confiance.txt")));
            // Rapport de plantage local (lot E5) : proposé au lancement suivant.
            app.attach_crash_reports(crash::default_dir());
            // Historique hors ligne (lot H) : chiffré, et seulement là où DPAPI existe.
            if offline::AVAILABLE {
                let offline_ctx = cc.egui_ctx.clone();
                app.attach_offline(Box::new(offline::OfflineFiles::start(offline::default_dir(), Arc::new(move || offline_ctx.request_repaint()))));
            }
            // Fichiers des plugins locaux (lot E1) : même protection ; le fil ne naît qu'avec un plugin activé.
            let vault_ctx = cc.egui_ctx.clone();
            app.attach_vault(offline::vault::Vault::new(offline::vault::default_root(), Arc::new(move || vault_ctx.request_repaint())));
            let audio_ctx = cc.egui_ctx.clone();
            app.attach_audio(Box::new(audio::io::service::DeviceService::new(Arc::new(move || audio_ctx.request_repaint()))));
            let capture_ctx = cc.egui_ctx.clone();
            // Sans capture (Android), pas de sélecteur : le bouton de partage reste masqué.
            if video::capture::AVAILABLE {
                app.attach_capture(Box::new(video::capture::service::CaptureDevices::new(Arc::new(move || capture_ctx.request_repaint()))));
                // Caméra de la page « Voix et vidéo » : rien ne tourne hors de « Tester la vidéo ».
                let camera_ctx = cc.egui_ctx.clone();
                app.attach_camera(Box::new(video::capture::service::CameraTest::new(Arc::new(move || camera_ctx.request_repaint()))));
            }
            let voice_ctx = cc.egui_ctx.clone();
            let mut voice_options = voice::Options::real();
            voice_options.voice.ws_headers = identity.ws_headers();
            app.attach_voice(Box::new(voice::Voice::new(voice_rt, voice_options, Arc::new(move || voice_ctx.request_repaint()))));
            if let Some(path) = desk::keys::default_path() {
                let keys_ctx = cc.egui_ctx.clone();
                app.attach_keys(Box::new(desk::keys::KeysFile::start(path, Arc::new(move || keys_ctx.request_repaint()))));
            }
            #[cfg(windows)]
            {
                let install_ctx = cc.egui_ctx.clone();
                let service = install::service::InstallService::start_with(install::service::Config::for_app(Box::new(install::hive::UserHive), std::env::current_exe().ok(), install::update::manifest::embedded_key(), Arc::new(move || install_ctx.request_repaint())));
                app.attach_install(Box::new(service), requests, &launch_args);
            }
            stage("services");
            // Ailleurs, ni installateur ni liens d'un second lancement.
            #[cfg(not(windows))]
            let _ = (requests, launch_args);
            // Android : hauteur du clavier virtuel suivie autour de l'interface.
            #[cfg(target_os = "android")]
            {
                return Ok(Box::new(platform::android::keyboard::Mobile::new(app)));
            }
            // Désactivé (par défaut) : l'application nue, sans aucune mesure.
            #[cfg_attr(target_os = "android", allow(unreachable_code))]
            if let Some(rec) = rec {
                return Ok(Box::new(perf::Traced::new(app, rec)));
            }
            Ok(Box::new(app))
        }),
    );
    // Erreur fatale hors panique (fenêtre, OpenGL, boucle d'événements) : rapport local (lot E5).
    if let Err(e) = &result {
        crash::fatal("erreur_fatale", &e.to_string());
    }
    // « Redémarrer pour installer » : la nouvelle instance attend que celle-ci libère le canal.
    if install::restart_requested()
        && let Ok(exe) = std::env::current_exe()
    {
        relaunch(&exe, &install::single::Args::default(), true);
    }
    result
}

/// Relance l'exécutable (après un remplacement, ou à la demande).
fn relaunch(exe: &std::path::Path, args: &install::single::Args, takeover: bool) {
    let mut cmd = std::process::Command::new(exe);
    if takeover {
        cmd.arg(install::single::TAKEOVER_ARG);
    }
    if args.minimized {
        cmd.arg(install::system::MINIMIZED_ARG);
    }
    if let Some(link) = &args.link {
        cmd.arg(link);
    }
    // Relance impossible : l'utilisateur relancera lui-même, rien n'est perdu.
    let _ = cmd.spawn();
}
