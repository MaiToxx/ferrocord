//! Installation et désinstallation par l'exécutable lui-même (lot distrib) :
//! un seul fichier à télécharger, aucun outil, aucun droit d'administrateur.
//!
//! - copie de `ferrocord.exe` dans `%LOCALAPPDATA%\Programs\Ferrocord` ;
//! - raccourci du menu Démarrer (et du bureau, en option) ;
//! - entrée dans « Applications installées » (HKCU), dont le désinstalleur
//!   est ce même exécutable lancé avec `--desinstaller` ;
//! - démarrage avec Windows, en option.
//!
//! La désinstallation retire tout cela, le démarrage avec Windows et le
//! protocole `discord://` s'ils pointent vers cette installation, et garde
//! les réglages et le cache, sauf demande explicite.
//!
//! Tout passe par [`Hive`], [`Shell`] et [`Places`] : les tests travaillent
//! dans un dossier temporaire, sur un registre en mémoire.

use super::hive::Hive;
use super::shortcut::{Shell, Shortcut};
use super::system::{self, Autostart};
use super::update::swap;
use std::io;
use std::path::{Path, PathBuf};

pub const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Ferrocord";
/// Ce que Ferrocord garde dans le registre (ancien gestionnaire de `discord://`).
pub const APP_KEY: &str = r"Software\Ferrocord";
/// Nom et icône des notifications.
pub const TOAST_KEY: &str = r"Software\Classes\AppUserModelId\Ferrocord";
pub const EXE: &str = "ferrocord.exe";
pub const LINK: &str = "Ferrocord.lnk";
const DESCRIPTION: &str = "Client Discord natif et léger (non officiel)";
/// Désinstalleur des installations faites par l'ancien `tools/installer.ps1`.
const LEGACY_UNINSTALLER: &str = "desinstaller.ps1";
const COPY: &str = "ferrocord.exe.copie";

pub const INSTALL_ARG: &str = "--installer";
pub const UNINSTALL_ARG: &str = "--desinstaller";
pub const PORTABLE_ARG: &str = "--portable";
pub const SILENT_ARG: &str = "--silencieux";
pub const DESKTOP_ARG: &str = "--bureau";
pub const AUTOSTART_ARG: &str = "--demarrage";
pub const WIPE_ARG: &str = "--supprimer-donnees";

/// Options d'une installation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Raccourci sur le bureau.
    pub desktop: bool,
    /// Démarrage avec Windows.
    pub autostart: bool,
}

/// Ce que demande la ligne de commande.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Lancement ordinaire.
    Run,
    /// Lancement sans installer, sans proposer de le faire.
    Portable,
    Install { silent: bool, options: Options },
    Uninstall { silent: bool, wipe: bool },
}

pub fn parse_cli<S: AsRef<str>>(args: &[S]) -> Command {
    let has = |name: &str| args.iter().any(|a| a.as_ref() == name);
    // Installer et désinstaller ne se demandent qu'en premier argument : le
    // protocole `discord://` lance `"exe" "%1"`, et un lien aux guillemets
    // non encodés pouvait glisser « --desinstaller --supprimer-donnees »
    // derrière lui (audit 0.1.33).
    let setup = args.first().is_some_and(|a| [UNINSTALL_ARG, INSTALL_ARG].contains(&a.as_ref()));
    if setup && has(UNINSTALL_ARG) {
        Command::Uninstall { silent: has(SILENT_ARG), wipe: has(WIPE_ARG) }
    } else if setup && has(INSTALL_ARG) {
        Command::Install { silent: has(SILENT_ARG), options: Options { desktop: has(DESKTOP_ARG), autostart: has(AUTOSTART_ARG) } }
    } else if has(PORTABLE_ARG) {
        Command::Portable
    } else {
        Command::Run
    }
}

/// Où l'installation écrit.
#[derive(Clone, Debug)]
pub struct Places {
    /// Dossier de l'installation (`…\Programs\Ferrocord`).
    pub programs: PathBuf,
    /// Dossier « Programmes » du menu Démarrer.
    pub start_menu: PathBuf,
    pub desktop: Option<PathBuf>,
    /// Réglages et cache, effacés seulement sur demande.
    pub data: Vec<PathBuf>,
}

impl Places {
    /// Les dossiers de l'utilisateur courant.
    #[cfg(windows)]
    pub fn system() -> Option<Places> {
        let local = dirs::data_local_dir()?;
        let roaming = dirs::data_dir()?;
        Some(Places {
            programs: local.join("Programs").join("Ferrocord"),
            start_menu: roaming.join("Microsoft").join("Windows").join("Start Menu").join("Programs"),
            desktop: dirs::desktop_dir(),
            data: vec![roaming.join("ferrocord"), local.join("ferrocord")],
        })
    }

    /// L'exécutable installé.
    pub fn target(&self) -> PathBuf {
        self.programs.join(EXE)
    }

    fn links(&self) -> impl Iterator<Item = PathBuf> + '_ {
        std::iter::once(self.start_menu.join(LINK)).chain(self.desktop.iter().map(|d| d.join(LINK)))
    }
}

/// Deux chemins du même fichier (Windows ne distingue pas la casse).
pub fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase();
    norm(a) == norm(b)
}

fn quoted(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// Exécutable d'un dossier de compilation (`target\…`, ou la disposition de
/// cargo à côté de lui) : on ne propose jamais de l'installer.
pub fn dev_build(exe: &Path) -> bool {
    let in_target = exe.components().any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case("target"));
    let cargo_layout = exe.parent().is_some_and(|d| d.join("deps").is_dir() && d.join(".fingerprint").is_dir());
    in_target || cargo_layout
}

/// Nom sous lequel un exécutable est retenu dans le mémo de la proposition.
pub fn offer_key(exe: &Path) -> String {
    exe.to_string_lossy().to_lowercase()
}

/// Exécutable lancé hors de son dossier d'installation et hors d'un dossier
/// de compilation : il peut s'installer (bouton des paramètres).
pub fn installable(exe: &Path, places: &Places) -> bool {
    !dev_build(exe) && !same_path(exe, &places.target())
}

/// Faut-il proposer l'installation à ce lancement ? Seulement pour un
/// lancement ordinaire (ni lien, ni démarrage réduit), d'un exécutable qui
/// n'est ni installé ni dans un dossier de compilation, et une seule fois
/// par exécutable (`seen` : la proposition a déjà été montrée pour celui-ci).
/// Un autre exécutable téléchargé la fait revenir : une seule fois pour
/// tout l'ordinateur, elle ne revenait jamais après une fenêtre fermée par mégarde.
pub fn should_offer(exe: &Path, places: &Places, command: Command, plain_launch: bool, seen: bool) -> bool {
    command == Command::Run && plain_launch && !seen && !dev_build(exe) && !same_path(exe, &places.target())
}

fn remove_if_present(p: &Path) -> io::Result<()> {
    match std::fs::remove_file(p) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Installe `source` (l'exécutable lancé) pour l'utilisateur courant ; rend
/// le chemin de l'exécutable installé. Refaire l'installation par-dessus une
/// plus ancienne la remplace.
pub fn install(h: &mut dyn Hive, sh: &mut dyn Shell, places: &Places, source: &Path, version: &str, o: Options) -> io::Result<PathBuf> {
    let target = places.target();
    std::fs::create_dir_all(&places.programs)?;
    if !same_path(source, &target) {
        // Copie à côté puis renommage : jamais d'exécutable à moitié écrit.
        let copy = places.programs.join(COPY);
        std::fs::copy(source, &copy)?;
        if let Err(e) = std::fs::rename(&copy, &target) {
            let _ = std::fs::remove_file(&copy);
            return Err(e);
        }
        // Mise à jour en attente de l'installation remplacée : elle pourrait
        // être plus ancienne que ce qu'on vient d'installer.
        for name in [swap::NEW, swap::OLD, swap::FAILED, swap::STATE] {
            remove_if_present(&places.programs.join(name))?;
        }
    }
    remove_if_present(&places.programs.join(LEGACY_UNINSTALLER))?;
    let shortcut = Shortcut { target: &target, dir: &places.programs, description: DESCRIPTION };
    std::fs::create_dir_all(&places.start_menu)?;
    sh.create(&places.start_menu.join(LINK), &shortcut)?;
    if let (true, Some(desktop)) = (o.desktop, &places.desktop) {
        sh.create(&desktop.join(LINK), &shortcut)?;
    }
    // « Applications installées ».
    let uninstall = format!("{} {UNINSTALL_ARG}", quoted(&target));
    for (name, value) in [
        ("DisplayName", "Ferrocord".to_string()),
        ("DisplayVersion", version.to_string()),
        ("Publisher", "Ferrocord (non officiel)".to_string()),
        ("InstallLocation", places.programs.display().to_string()),
        ("DisplayIcon", format!("{},0", target.display())),
        ("QuietUninstallString", format!("{uninstall} {SILENT_ARG}")),
        ("UninstallString", uninstall),
    ] {
        h.set(UNINSTALL_KEY, name, &value)?;
    }
    let kb = std::fs::metadata(&target).map_or(0, |m| m.len().div_ceil(1024));
    for (name, value) in [("NoModify", 1), ("NoRepair", 1), ("EstimatedSize", u32::try_from(kb).unwrap_or(u32::MAX))] {
        h.set_u32(UNINSTALL_KEY, name, value)?;
    }
    if o.autostart {
        system::set_autostart(h, &target, Autostart::On { minimized: false })?;
    }
    Ok(target)
}

/// Ce que la désinstallation n'a pas pu retirer elle-même : l'exécutable en
/// cours (Windows interdit de supprimer un programme qui tourne) et son
/// dossier. À confier à [`finish_later`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Leftover {
    pub exe: PathBuf,
    pub dir: PathBuf,
}

/// Désinstalle. `wipe` : effacer aussi les réglages et le cache. Les jetons
/// enregistrés restent dans le Gestionnaire d'identifiants de Windows.
pub fn uninstall(h: &mut dyn Hive, places: &Places, wipe: bool) -> io::Result<Option<Leftover>> {
    let target = places.target();
    // Démarrage avec Windows, s'il pointe vers cette installation.
    if h.get(system::RUN_KEY, system::RUN_VALUE).is_some_and(|v| v.to_lowercase().starts_with(&quoted(&target).to_lowercase())) {
        h.remove_value(system::RUN_KEY, system::RUN_VALUE)?;
    }
    // `discord://` : rendu au programme d'avant, ou retiré, s'il est à nous.
    system::unregister_protocol(h, &target)?;
    h.remove_tree(APP_KEY)?;
    h.remove_tree(TOAST_KEY)?;
    for link in places.links() {
        remove_if_present(&link)?;
    }
    h.remove_tree(UNINSTALL_KEY)?;
    // Préférence de carte graphique, si c'est encore la nôtre, et bibliothèques ANGLE (lot « gl »).
    crate::rendu::angle::forget_saving(h, &target)?;
    crate::rendu::angle::remove(&places.programs);
    if wipe {
        for dir in &places.data {
            // Garde-fou : seulement un dossier nommé « ferrocord ».
            if dir.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("ferrocord")) {
                match std::fs::remove_dir_all(dir) {
                    Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                    _ => {}
                }
            }
        }
    }
    // Le dossier : nos fichiers un par un, jamais une suppression récursive.
    for name in [swap::NEW, swap::OLD, swap::FAILED, swap::STATE, "ferrocord.new.exe.part", "ferrocord-maj.json.tmp", COPY, LEGACY_UNINSTALLER] {
        remove_if_present(&places.programs.join(name))?;
    }
    if remove_if_present(&target).is_ok() {
        // Le dossier reste s'il contient autre chose que nos fichiers.
        let _ = std::fs::remove_dir(&places.programs);
        return Ok(None);
    }
    Ok(Some(Leftover { exe: target, dir: places.programs.clone() }))
}

/// Supprime l'exécutable puis son dossier (s'il est vide) une fois ce
/// processus terminé : un `cmd` détaché réessaie pendant une minute. Les
/// chemins lui sont passés par l'environnement, jamais dans la commande.
#[cfg(windows)]
pub fn finish_later(left: &Leftover) -> io::Result<()> {
    use std::os::windows::process::CommandExt;
    /// Pas de fenêtre de console.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let system32 = PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into())).join("System32");
    let script = r#"/d /s /c "for /l %i in (1,1,30) do (del /f /q "%FERROCORD_EXE%" 2>nul & if not exist "%FERROCORD_EXE%" (rmdir "%FERROCORD_DIR%" 2>nul & exit) else ("%FERROCORD_PING%" -n 3 127.0.0.1 >nul))""#;
    std::process::Command::new(system32.join("cmd.exe"))
        .raw_arg(script)
        .env("FERROCORD_EXE", &left.exe)
        .env("FERROCORD_DIR", &left.dir)
        .env("FERROCORD_PING", system32.join("ping.exe"))
        .env_remove("DISCORD_TOKEN")
        .current_dir(std::env::temp_dir())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(drop)
}

/// Un autre Ferrocord tourne-t-il depuis `target` ? (son exécutable ne peut
/// alors être ni remplacé ni supprimé).
pub fn running(target: &Path) -> bool {
    let me = std::process::id();
    crate::procs::list().into_iter().filter(|p| p.pid != me && p.file.eq_ignore_ascii_case(EXE)).any(|p| crate::procs::details(p.pid).is_some_and(|d| same_path(Path::new(&d.path), target)))
}

/// Suite du lancement après [`at_startup`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    /// Lancer l'application.
    Continue,
    /// Tout est fait : quitter avec ce code.
    Exit(i32),
}

/// Relance `exe` (l'installation, ou cet exécutable en mode portable).
#[cfg(windows)]
fn spawn(exe: &Path, args: &[&str]) {
    // Relance impossible : l'utilisateur relancera lui-même.
    let _ = std::process::Command::new(exe).args(args).spawn();
}

/// Au tout début du lancement : commandes d'installation de la ligne de
/// commande, ou proposition d'installer (une seule fois, jamais pour un
/// exécutable de compilation). `plain_launch` : ni lien, ni démarrage réduit.
#[cfg(windows)]
pub fn at_startup(raw: &[String], plain_launch: bool) -> Next {
    use super::hive::UserHive;
    use super::shortcut::WindowsShell;
    let command = parse_cli(raw);
    if matches!(command, Command::Portable) {
        return Next::Continue;
    }
    let (Some(places), Ok(exe)) = (Places::system(), std::env::current_exe()) else {
        return if command == Command::Run { Next::Continue } else { Next::Exit(1) };
    };
    let version = env!("CARGO_PKG_VERSION");
    match command {
        Command::Portable => Next::Continue,
        Command::Install { silent, options } => {
            if running(&places.target()) {
                return Next::Exit(1);
            }
            match install(&mut UserHive, &mut WindowsShell, &places, &exe, version, options) {
                Ok(target) => {
                    if !silent {
                        spawn(&target, &[]);
                    }
                    Next::Exit(0)
                }
                Err(_) => Next::Exit(1),
            }
        }
        Command::Uninstall { silent: true, wipe } => {
            if running(&places.target()) {
                return Next::Exit(1);
            }
            match uninstall(&mut UserHive, &places, wipe) {
                Ok(left) => {
                    if let Some(l) = left {
                        let _ = finish_later(&l);
                    }
                    Next::Exit(0)
                }
                Err(_) => Next::Exit(1),
            }
        }
        Command::Uninstall { silent: false, .. } => {
            let left = crate::ui::setup::run(crate::ui::setup::Mode::Uninstall, places, exe);
            if let crate::ui::setup::Outcome::Uninstalled(Some(l)) = &left {
                let _ = finish_later(l);
            }
            Next::Exit(0)
        }
        Command::Run => {
            // D'abord ce qui ne coûte rien : un exécutable installé ou de
            // compilation ne lit même pas le fichier.
            // Jamais pendant une mesure (`tools/mesures.ps1`, FERROCORD_PERF).
            if !should_offer(&exe, &places, command, plain_launch, false) || std::env::var_os("FERROCORD_PERF").is_some() {
                return Next::Continue;
            }
            let Some(memo_path) = super::memo::default_path() else { return Next::Continue };
            let mut memo = super::memo::read(&memo_path);
            let key = offer_key(&exe);
            if memo.proposition_chemin.as_deref() == Some(key.as_str()) {
                return Next::Continue;
            }
            // Retenu avant d'afficher : si l'écriture échoue, on ne propose
            // pas (la question reviendrait à chaque lancement).
            memo.proposition_vue = true;
            memo.proposition_chemin = Some(key);
            if super::memo::write(&memo_path, &memo).is_err() {
                return Next::Continue;
            }
            match crate::ui::setup::run(crate::ui::setup::Mode::Offer, places, exe.clone()) {
                crate::ui::setup::Outcome::Installed(target) => spawn(&target, &[]),
                // « Lancer sans installer », ou fenêtre fermée : l'application
                // s'ouvre quand même (une fenêtre par processus : on relance).
                _ => spawn(&exe, &[PORTABLE_ARG]),
            }
            Next::Exit(0)
        }
    }
}

/// Installe l'exécutable en cours pour l'utilisateur (bouton « Installer »
/// des paramètres) ; rend l'exécutable installé, à relancer.
#[cfg(windows)]
pub fn install_running() -> io::Result<PathBuf> {
    use super::hive::UserHive;
    use super::shortcut::WindowsShell;
    let places = Places::system().ok_or_else(|| io::Error::other("dossiers de l'utilisateur introuvables"))?;
    let exe = std::env::current_exe()?;
    install(&mut UserHive, &mut WindowsShell, &places, &exe, env!("CARGO_PKG_VERSION"), Options::default())
}

/// L'exécutable en cours peut-il s'installer ? (ni installé, ni de compilation)
#[cfg(windows)]
pub fn running_installable() -> bool {
    match (Places::system(), std::env::current_exe()) {
        (Some(places), Ok(exe)) => installable(&exe, &places),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::hive::MemoryHive;
    use crate::install::shortcut::PlainShell;
    use crate::install::Protocol;

    /// Faux profil d'utilisateur dans un dossier temporaire.
    struct Profile {
        root: PathBuf,
        places: Places,
        /// L'exécutable « téléchargé ».
        download: PathBuf,
    }

    impl Profile {
        fn new(name: &str) -> Profile {
            let root = std::env::temp_dir().join(format!("ferrocord-setup-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let places = Places {
                programs: root.join("Local").join("Programs").join("Ferrocord"),
                start_menu: root.join("Roaming").join("Menu").join("Programs"),
                desktop: Some(root.join("Bureau")),
                data: vec![root.join("Roaming").join("ferrocord"), root.join("Local").join("ferrocord")],
            };
            std::fs::create_dir_all(root.join("Bureau")).unwrap();
            let download = root.join("Telechargements").join("Ferrocord-1.2.3-x64.exe");
            std::fs::create_dir_all(download.parent().unwrap()).unwrap();
            std::fs::write(&download, vec![b'x'; 3000]).unwrap();
            Profile { root, places, download }
        }
    }

    impl Drop for Profile {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// La proposition est retenue par exécutable : un autre téléchargement la fait revenir.
    #[test]
    fn offer_is_remembered_per_executable() {
        let p = Profile::new("par-exe");
        assert_eq!(offer_key(Path::new(r"C:\Users\A\Downloads\Ferrocord-1.0.0-x64.exe")), r"c:\users\a\downloads\ferrocord-1.0.0-x64.exe");
        assert_ne!(offer_key(Path::new(r"C:\dl\Ferrocord-1.0.0-x64.exe")), offer_key(Path::new(r"C:\dl\Ferrocord-1.0.1-x64.exe")));
        assert!(installable(&p.download, &p.places));
        assert!(!installable(&p.places.target(), &p.places), "déjà installé");
        assert!(!installable(Path::new(r"C:\src\ferrocord\target\release\ferrocord.exe"), &p.places), "compilation");
    }

    #[test]
    fn command_line() {
        let cli = |a: &[&str]| parse_cli(a);
        assert_eq!(cli(&[]), Command::Run);
        assert_eq!(cli(&["--reduit", "discord://-/channels/1/2"]), Command::Run);
        assert_eq!(cli(&["--portable"]), Command::Portable);
        assert_eq!(cli(&["--installer"]), Command::Install { silent: false, options: Options::default() });
        assert_eq!(cli(&["--installer", "--silencieux", "--bureau", "--demarrage"]), Command::Install { silent: true, options: Options { desktop: true, autostart: true } });
        assert_eq!(cli(&["--desinstaller"]), Command::Uninstall { silent: false, wipe: false });
        assert_eq!(cli(&["--desinstaller", "--silencieux", "--supprimer-donnees"]), Command::Uninstall { silent: true, wipe: true });
        // Derrière un lien, ces options ne sont jamais des commandes.
        assert_eq!(cli(&["discord://x", "--desinstaller", "--silencieux", "--supprimer-donnees"]), Command::Run);
        assert_eq!(cli(&["--reduit", "--installer"]), Command::Run);
        // Désinstaller l'emporte : jamais d'installation par erreur.
        assert_eq!(cli(&["--installer", "--desinstaller"]), Command::Uninstall { silent: false, wipe: false });
    }

    #[test]
    fn offer_only_once_and_never_for_a_build_or_an_install() {
        let p = Profile::new("offre");
        let offer = |exe: &Path, cmd, plain, seen| should_offer(exe, &p.places, cmd, plain, seen);
        assert!(offer(&p.download, Command::Run, true, false));
        assert!(!offer(&p.download, Command::Run, true, true), "déjà montrée");
        assert!(!offer(&p.download, Command::Run, false, false), "lien ou démarrage réduit");
        assert!(!offer(&p.download, Command::Portable, true, false));
        assert!(!offer(&p.places.target(), Command::Run, true, false), "déjà installé");
        let upper = PathBuf::from(p.places.target().to_string_lossy().to_uppercase());
        assert!(!offer(&upper, Command::Run, true, false), "casse du chemin");
        // Dossiers de compilation : `target\release`, ou un CARGO_TARGET_DIR ailleurs.
        for dev in [r"C:\src\ferrocord\target\release\ferrocord.exe", r"C:\src\ferrocord\Target\debug\ferrocord.exe", r"D:\target\x86_64-pc-windows-msvc\release\ferrocord.exe"] {
            assert!(dev_build(Path::new(dev)), "{dev}");
            assert!(!offer(Path::new(dev), Command::Run, true, false), "{dev}");
        }
        let elsewhere = p.root.join("cache-de-compilation").join("release");
        std::fs::create_dir_all(elsewhere.join("deps")).unwrap();
        std::fs::create_dir_all(elsewhere.join(".fingerprint")).unwrap();
        assert!(dev_build(&elsewhere.join("ferrocord.exe")));
        assert!(!dev_build(&p.download));
    }

    #[test]
    fn install_then_uninstall_leaves_nothing_but_the_data() {
        let p = Profile::new("complet");
        let mut h = MemoryHive::default();
        // Un autre programme avait `discord://`, et l'utilisateur a des réglages.
        h.set(r"Software\Classes\discord", "", "URL:Discord Protocol").unwrap();
        h.set(r"Software\Classes\discord", "URL Protocol", "").unwrap();
        h.set(r"Software\Classes\discord\shell\open\command", "", "\"D:\\officiel.exe\" \"%1\"").unwrap();
        h.set(r"Software\Autre", "x", "garde").unwrap();
        let before = format!("{h:?}");
        std::fs::create_dir_all(&p.places.data[0]).unwrap();
        std::fs::write(p.places.data[0].join("local.toml"), b"reglages").unwrap();

        let target = install(&mut h, &mut PlainShell, &p.places, &p.download, "1.2.3", Options { desktop: true, autostart: true }).unwrap();
        assert_eq!(target, p.places.target());
        assert_eq!(std::fs::read(&target).unwrap().len(), 3000);
        assert!(!p.places.programs.join(COPY).exists());
        let menu = std::fs::read_to_string(p.places.start_menu.join(LINK)).unwrap();
        assert!(menu.starts_with(&target.display().to_string()), "{menu}");
        assert!(p.root.join("Bureau").join(LINK).exists());
        let q = format!("\"{}\"", target.display());
        assert_eq!(h.get(UNINSTALL_KEY, "DisplayName").as_deref(), Some("Ferrocord"));
        assert_eq!(h.get(UNINSTALL_KEY, "DisplayVersion").as_deref(), Some("1.2.3"));
        // « Applications installées » montre l'icône de l'exécutable installé (son seul groupe, index 0).
        assert_eq!(h.get(UNINSTALL_KEY, "DisplayIcon"), Some(format!("{},0", target.display())));
        assert_eq!(h.get(UNINSTALL_KEY, "UninstallString"), Some(format!("{q} --desinstaller")));
        assert_eq!(h.get(UNINSTALL_KEY, "QuietUninstallString"), Some(format!("{q} --desinstaller --silencieux")));
        assert_eq!(h.get(UNINSTALL_KEY, "InstallLocation"), Some(p.places.programs.display().to_string()));
        assert_eq!(h.get(UNINSTALL_KEY, "EstimatedSize").as_deref(), Some("3"));
        assert_eq!(h.get(UNINSTALL_KEY, "NoModify").as_deref(), Some("1"));
        assert_eq!(system::autostart(&h), Autostart::On { minimized: false });
        assert_eq!(h.get(system::RUN_KEY, system::RUN_VALUE), Some(q.clone()));
        // L'installation en service : liens Discord pris, mise à jour en attente.
        system::register_protocol(&mut h, &target).unwrap();
        assert_eq!(system::protocol(&h, &target), Protocol::Ours);
        h.set(TOAST_KEY, "DisplayName", "Ferrocord").unwrap();
        std::fs::write(p.places.programs.join(swap::NEW), b"nouvelle").unwrap();
        std::fs::write(p.places.programs.join(swap::STATE), b"{}").unwrap();

        // Désinstallation ordinaire : tout est rendu, les réglages restent.
        assert_eq!(uninstall(&mut h, &p.places, false).unwrap(), None);
        assert!(!p.places.programs.exists(), "dossier de l'installation retiré");
        assert!(!p.places.start_menu.join(LINK).exists());
        assert!(!p.root.join("Bureau").join(LINK).exists());
        assert_eq!(format!("{h:?}"), before, "registre rendu tel quel, protocole compris");
        assert_eq!(std::fs::read(p.places.data[0].join("local.toml")).unwrap(), b"reglages");
        // La refaire ne casse rien.
        assert_eq!(uninstall(&mut h, &p.places, false).unwrap(), None);
    }

    #[test]
    fn options_are_optional_and_wipe_is_explicit() {
        let p = Profile::new("options");
        let mut h = MemoryHive::default();
        install(&mut h, &mut PlainShell, &p.places, &p.download, "1.2.3", Options::default()).unwrap();
        assert!(p.places.start_menu.join(LINK).exists());
        assert!(!p.root.join("Bureau").join(LINK).exists(), "bureau : seulement sur demande");
        assert_eq!(system::autostart(&h), Autostart::Off);
        // Un démarrage avec Windows qui lance un autre Ferrocord n'est pas à nous.
        h.set(system::RUN_KEY, system::RUN_VALUE, "\"D:\\ailleurs\\ferrocord.exe\"").unwrap();
        for dir in &p.places.data {
            std::fs::create_dir_all(dir.join("sous")).unwrap();
            std::fs::write(dir.join("sous").join("f"), b"x").unwrap();
        }
        // Un fichier étranger dans le dossier : gardé, donc le dossier aussi.
        std::fs::write(p.places.programs.join("notes.txt"), b"a moi").unwrap();
        assert_eq!(uninstall(&mut h, &p.places, true).unwrap(), None);
        assert!(p.places.data.iter().all(|d| !d.exists()), "réglages et cache effacés sur demande");
        assert!(!p.places.target().exists());
        assert!(p.places.programs.join("notes.txt").exists(), "rien d'autre que nos fichiers n'est supprimé");
        assert_eq!(h.len(), 1, "{h:?}");
        assert!(h.get(system::RUN_KEY, system::RUN_VALUE).is_some());
    }

    #[test]
    fn reinstalling_replaces_and_drops_a_pending_update() {
        let p = Profile::new("refaire");
        let mut h = MemoryHive::default();
        install(&mut h, &mut PlainShell, &p.places, &p.download, "1.2.3", Options::default()).unwrap();
        // Restes d'une installation par l'ancien script, et mise à jour en attente.
        std::fs::write(p.places.programs.join(LEGACY_UNINSTALLER), b"ps1").unwrap();
        std::fs::write(p.places.programs.join(swap::NEW), b"1.2.4").unwrap();
        std::fs::write(p.places.programs.join(swap::STATE), b"{\"pending\":{\"version\":\"1.2.4\",\"sha256\":\"\",\"size\":5}}").unwrap();
        std::fs::write(&p.download, vec![b'y'; 5000]).unwrap();
        install(&mut h, &mut PlainShell, &p.places, &p.download, "1.3.0", Options::default()).unwrap();
        assert_eq!(std::fs::read(p.places.target()).unwrap(), vec![b'y'; 5000]);
        assert_eq!(h.get(UNINSTALL_KEY, "DisplayVersion").as_deref(), Some("1.3.0"));
        assert_eq!(h.get(UNINSTALL_KEY, "EstimatedSize").as_deref(), Some("5"));
        for gone in [LEGACY_UNINSTALLER, swap::NEW, swap::STATE] {
            assert!(!p.places.programs.join(gone).exists(), "{gone}");
        }
        // Lancé depuis l'installation même (`--installer` relancé) : rien à copier.
        let target = p.places.target();
        install(&mut h, &mut PlainShell, &p.places, &target, "1.3.0", Options::default()).unwrap();
        assert_eq!(std::fs::read(&target).unwrap().len(), 5000);
    }

    /// L'exécutable en cours ne peut pas se supprimer : la suite est confiée
    /// à un `cmd` détaché. Ici sur un dossier temporaire et un faux
    /// exécutable, d'abord verrouillé comme le serait le programme qui tourne.
    #[cfg(windows)]
    #[test]
    fn locked_executable_is_removed_once_released() {
        use std::os::windows::fs::OpenOptionsExt;
        let p = Profile::new("verrou");
        let mut h = MemoryHive::default();
        let target = install(&mut h, &mut PlainShell, &p.places, &p.download, "1.2.3", Options::default()).unwrap();
        // Sans partage : ni lecture, ni écriture, ni suppression par un autre.
        let lock = std::fs::OpenOptions::new().read(true).share_mode(0).open(&target).unwrap();
        let left = uninstall(&mut h, &p.places, false).unwrap().expect("exécutable verrouillé : il reste");
        assert_eq!(left, Leftover { exe: target.clone(), dir: p.places.programs.clone() });
        assert!(h.is_empty(), "le registre, lui, est déjà propre : {h:?}");
        finish_later(&left).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(700));
        assert!(target.exists(), "encore verrouillé");
        drop(lock);
        let start = std::time::Instant::now();
        while p.places.programs.exists() && start.elapsed() < std::time::Duration::from_secs(20) {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(!p.places.programs.exists(), "exécutable puis dossier supprimés après coup");
    }
}
