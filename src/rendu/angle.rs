//! Rendu par ANGLE (lot « gl ») : OpenGL ES traduit en Direct3D 11.
//!
//! Le pilote OpenGL du fabricant de la carte graphique est, de loin, le plus
//! gros poste de mémoire d'une fenêtre vide (mesuré sur un portable AMD à deux
//! cartes : 100 Mo privés par `atio6axx.dll` sur la carte dédiée, 167 Mo sur
//! l'intégrée). Le même contexte obtenu par ANGLE passe par le pilote
//! Direct3D 11, bien plus sobre : 60 Mo sur la carte dédiée, 39 Mo sur
//! l'intégrée, sans coût par image de plus. Chiffres et méthode :
//! `docs/mesures.md`, lot « gl ».
//!
//! ANGLE tient en deux bibliothèques de Google (licence BSD à trois clauses,
//! `assets/angle/LICENSE.txt`), embarquées compressées dans l'exécutable et
//! posées à côté de lui dans le dossier d'installation ([`install`]). Rien
//! n'est posé ailleurs : un exécutable lancé depuis un autre dossier (ou une
//! pose qui échoue) garde le rendu OpenGL de toujours, sans autre effet.
//!
//! La fenêtre est créée par eframe et `glutin` comme avant : seule change la
//! préférence « EGL d'abord » de notre copie de `glutin-winit`
//! (`vendor/glutin-winit`). Si EGL échoue malgré tout, `glutin` retombe de
//! lui-même sur WGL.
//!
//! Carte graphique : sur un ordinateur à deux cartes, une application de
//! discussion n'a pas à réveiller la carte dédiée. [`prefer_saving`] l'écrit
//! dans les préférences graphiques de Windows (celles de Paramètres › Système ›
//! Écran › Graphiques), pour cet exécutable seulement, et seulement si
//! l'utilisateur n'y a rien choisi.

use crate::install::hive::Hive;
use std::{
    io::{self, Read, Seek, Write},
    path::Path,
    sync::Mutex,
};

/// Une bibliothèque embarquée : nom du fichier posé, contenu compressé (gzip), taille et
/// sha256 (hexadécimal) une fois posée.
pub struct Library {
    pub name: &'static str,
    gz: &'static [u8],
    pub len: u64,
    pub sha256: &'static str,
}

/// Les deux bibliothèques d'ANGLE (Electron 43.8.10, ANGLE 2.1.28055), pour Windows x64.
#[cfg(all(windows, target_arch = "x86_64"))]
pub const LIBRARIES: &[Library] = &[
    Library { name: "libEGL.dll", gz: include_bytes!("../../assets/angle/libEGL.dll.gz"), len: 467_968, sha256: "ef6b85b6aaf7126aecad3b9d9c73c2017273a1d474cb7445c56fb946218516fe" },
    Library { name: "libGLESv2.dll", gz: include_bytes!("../../assets/angle/libGLESv2.dll.gz"), len: 8_076_800, sha256: "cc1fcfa494b64ccba3aefbe466009103e96a709affa1dbfbe23123dffe55663d" },
];
#[cfg(not(all(windows, target_arch = "x86_64")))]
pub const LIBRARIES: &[Library] = &[];

/// Noms des fichiers posés (la désinstallation les retire).
pub const FILES: [&str; 2] = ["libEGL.dll", "libGLESv2.dll"];

/// Témoin posé à côté de l'exécutable pendant un démarrage sous ANGLE, retiré une fois une
/// image affichée (ou la fenêtre fermée sans erreur). Il contient la version de Ferrocord
/// et reste ouvert, sans partage, tant que le démarrage dure : un autre exemplaire qui
/// démarre au même moment le voit « occupé », pas « raté ». Trouvé fermé au lancement
/// suivant avec la même version : ANGLE a fait échouer le démarrage (pilote Direct3D
/// défaillant…) ; OpenGL sert alors jusqu'à la version suivante de Ferrocord, jusqu'à ce
/// qu'une mise à jour repose les bibliothèques, ou jusqu'à ce que le fichier soit effacé.
pub const TRIAL: &str = "angle.essai";

/// Version écrite dans le témoin : un échec ne vaut que pour elle.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Suffixe des fichiers en cours d'écriture (`<nom>.<pid>.part`).
const PART: &str = ".part";

/// Octets comparés au début de chaque fichier : l'en-tête PE, qui porte la
/// date et la somme de contrôle de la compilation. Assez pour reconnaître une
/// autre version sans relire 8 Mo ; le contenu entier est vérifié (sha256) au
/// chargement, par [`activate`].
const HEAD: usize = 1024;

/// sha256 de `data`, en hexadécimal.
fn sha256(data: &[u8]) -> String {
    aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, data).as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

impl Library {
    /// Début du fichier attendu.
    fn head(&self) -> io::Result<Vec<u8>> {
        let mut head = vec![0; HEAD];
        flate2::read::GzDecoder::new(self.gz).read_exact(&mut head)?;
        Ok(head)
    }

    /// Le fichier posé dans `dir` est-il celui embarqué (taille et en-tête) ?
    fn present(&self, dir: &Path) -> bool {
        let path = dir.join(self.name);
        if std::fs::metadata(&path).map_or(true, |m| m.len() != self.len) {
            return false;
        }
        let mut found = vec![0; HEAD];
        std::fs::File::open(&path).and_then(|mut f| f.read_exact(&mut found)).is_ok() && self.head().is_ok_and(|h| h == found)
    }

    /// `data` est-il, octet pour octet, la bibliothèque embarquée ?
    fn matches(&self, data: &[u8]) -> bool {
        data.len() as u64 == self.len && sha256(data) == self.sha256
    }

    /// Pose le fichier dans `dir` : écrit à côté (un nom par processus : deux exemplaires
    /// lancés ensemble ne s'écrasent pas), vidé sur le disque, puis renommé ; jamais à
    /// moitié écrit. Un dossier non inscriptible ou un fichier en service rend l'erreur.
    fn write(&self, dir: &Path) -> io::Result<()> {
        let mut data = Vec::with_capacity(self.len as usize);
        flate2::read::GzDecoder::new(self.gz).read_to_end(&mut data)?;
        if !self.matches(&data) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "bibliothèque embarquée abîmée"));
        }
        let part = dir.join(format!("{}.{}{PART}", self.name, std::process::id()));
        let written = std::fs::File::create(&part).and_then(|mut f| {
            f.write_all(&data)?;
            f.sync_all()
        });
        written.and_then(|()| std::fs::rename(&part, dir.join(self.name))).inspect_err(|_| {
            let _ = std::fs::remove_file(&part);
        })
    }
}

/// Les bibliothèques d'ANGLE sont-elles posées dans `dir` (taille et en-tête de la version embarquée) ?
pub fn present(dir: &Path) -> bool {
    !LIBRARIES.is_empty() && LIBRARIES.iter().all(|l| l.present(dir))
}

/// Pose dans `dir` les bibliothèques qui y manquent ou qui datent d'une autre
/// version. Rien à faire (deux lectures d'un kilo-octet) quand elles y sont.
pub fn install(dir: &Path) -> io::Result<()> {
    if LIBRARIES.is_empty() {
        return Err(io::Error::new(io::ErrorKind::Unsupported, "ANGLE n'est embarqué que pour Windows x64"));
    }
    let missing: Vec<&Library> = LIBRARIES.iter().filter(|l| !l.present(dir)).collect();
    if missing.is_empty() {
        return Ok(());
    }
    sweep_parts(dir);
    missing.into_iter().try_for_each(|l| l.write(dir))?;
    // Autres bibliothèques qu'avant : un ancien échec ne vaut plus.
    let _ = std::fs::remove_file(dir.join(TRIAL));
    Ok(())
}

/// Retire les fichiers d'une pose interrompue (processus tué pendant l'écriture).
fn sweep_parts(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(PART) && FILES.iter().any(|f| name.starts_with(f)) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Désinstallation : retire de `dir` les bibliothèques, les restes d'une pose interrompue
/// et le témoin. Sans échec : un fichier encore chargé resterait, la désinstallation continue.
pub fn remove(dir: &Path) {
    sweep_parts(dir);
    for name in FILES {
        let _ = std::fs::remove_file(dir.join(name));
    }
    let _ = std::fs::remove_file(dir.join(TRIAL));
}

/// Ce que dit le témoin au début d'un démarrage sous ANGLE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trial {
    /// Témoin posé (ou impossible à poser : dossier non inscriptible) : ANGLE peut servir.
    Started,
    /// Un autre exemplaire est en train de démarrer et tient le témoin : ANGLE peut
    /// servir, sans témoin à nous.
    Busy,
    /// Le dernier démarrage de cette version sous ANGLE n'a pas atteint l'affichage.
    Failed,
}

/// Témoin tenu ouvert par ce processus pendant son démarrage.
static HELD: Mutex<Option<std::fs::File>> = Mutex::new(None);

/// Ouvre le témoin sans le partager : tant que ce processus vit et ne l'a pas rendu,
/// personne d'autre ne peut ni l'ouvrir ni l'effacer.
fn open_trial(path: &Path, new: bool) -> io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.read(true).write(true);
    if new {
        o.create_new(true);
    }
    #[cfg(windows)]
    std::os::windows::fs::OpenOptionsExt::share_mode(&mut o, 0);
    o.open(path)
}

/// Windows : « le fichier est utilisé par un autre processus » (ERROR_SHARING_VIOLATION).
fn shared_away(e: &io::Error) -> bool {
    cfg!(windows) && e.raw_os_error() == Some(32)
}

/// Le témoin de `dir` dit-il un démarrage raté de cette version ? (Tenu par un
/// exemplaire qui démarre : non.)
pub fn trial_failed(dir: &Path) -> bool {
    let mut text = String::new();
    open_trial(&dir.join(TRIAL), false).and_then(|mut f| f.read_to_string(&mut text)).is_ok() && text == VERSION
}

/// Début d'un démarrage sous ANGLE : pose le témoin [`TRIAL`] et le garde ouvert.
pub fn trial_begin(dir: &Path) -> Trial {
    let path = dir.join(TRIAL);
    let mut file = match open_trial(&path, true) {
        Ok(f) => f,
        Err(e) if shared_away(&e) => return Trial::Busy,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => match open_trial(&path, false) {
            Ok(mut f) => {
                let mut text = String::new();
                if f.read_to_string(&mut text).is_ok() && text == VERSION {
                    return Trial::Failed;
                }
                // Témoin d'une autre version (ou vidé par un exemplaire qui finit de démarrer) : nouvel essai.
                f
            }
            Err(e) if shared_away(&e) => return Trial::Busy,
            // Effacé entre-temps par l'exemplaire qui le tenait, ou illisible : pas de témoin.
            Err(_) => return Trial::Started,
        },
        // Dossier non inscriptible : pas de témoin.
        Err(_) => return Trial::Started,
    };
    let written = file.set_len(0).and_then(|()| file.seek(io::SeekFrom::Start(0))).and_then(|_| file.write_all(VERSION.as_bytes()));
    if written.is_ok()
        && let Ok(mut held) = HELD.lock()
    {
        *held = Some(file);
    }
    Trial::Started
}

/// Une image est affichée, la fenêtre s'est fermée sans erreur, ou ANGLE ne sert
/// finalement pas : le témoin de ce processus, s'il en tient un, est retiré. Il est vidé
/// avant d'être rendu : un exemplaire qui l'ouvrirait juste avant l'effacement n'y lit
/// pas un échec.
pub fn trial_end(dir: &Path) {
    let held = HELD.lock().ok().and_then(|mut h| h.take());
    if let Some(file) = held {
        let _ = file.set_len(0);
        drop(file);
        let _ = std::fs::remove_file(dir.join(TRIAL));
    }
}

/// Le rendu sous ANGLE a été perdu juste après le démarrage : le témoin est laissé comme
/// après un démarrage raté (ANGLE ne sert plus jusqu'à la version suivante).
pub fn trial_fail(dir: &Path) {
    drop(HELD.lock().ok().and_then(|mut h| h.take()));
    let _ = std::fs::write(dir.join(TRIAL), VERSION);
}

/// Dossier de l'exécutable en cours : celui où les bibliothèques sont posées.
pub fn exe_dir() -> Option<std::path::PathBuf> {
    std::env::current_exe().ok()?.parent().map(Path::to_path_buf)
}

/// Vérifie puis charge les bibliothèques posées dans `dir`, par leur chemin complet.
///
/// Chaque fichier est ouvert en interdisant l'écriture et l'effacement, lu en entier et
/// comparé au sha256 embarqué ; il reste ouvert jusqu'à ce que Windows l'ait chargé : ce
/// qui est chargé est ce qui a été vérifié. `glutin` demande ensuite `libEGL.dll` par son
/// seul nom : Windows lui rend le module déjà chargé, sans chercher dans le dossier
/// courant ni dans le PATH. Un fichier altéré n'est pas chargé ; il est effacé, pour être
/// reposé au lancement suivant de l'exécutable installé.
#[cfg(all(windows, target_arch = "x86_64"))]
fn load_verified(dir: &Path) -> Result<(), String> {
    use windows::Win32::System::LibraryLoader::{LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};
    use windows::core::HSTRING;
    if !dir.is_absolute() {
        return Err("dossier de l'exécutable relatif".to_string());
    }
    let mut verified = Vec::with_capacity(LIBRARIES.len());
    // libGLESv2.dll d'abord : libEGL.dll en dépend.
    for l in LIBRARIES.iter().rev() {
        let path = dir.join(l.name);
        let mut o = std::fs::OpenOptions::new();
        o.read(true);
        // FILE_SHARE_READ seul : ni écriture ni effacement tant que le fichier est tenu.
        std::os::windows::fs::OpenOptionsExt::share_mode(&mut o, 1);
        let mut file = o.open(&path).map_err(|e| format!("{} illisible : {e}", l.name))?;
        let mut data = Vec::with_capacity(l.len as usize + 1);
        file.read_to_end(&mut data).map_err(|e| format!("{} illisible : {e}", l.name))?;
        if !l.matches(&data) {
            drop(file);
            let _ = std::fs::remove_file(&path);
            return Err(format!("{} altérée (sha256 inattendu) : non chargée", l.name));
        }
        verified.push((file, path));
    }
    // Le compilateur de nuanceurs qu'ANGLE demande par son nom : celui de Windows, pas un
    // fichier du même nom posé à côté de l'exécutable.
    // SAFETY: nom de fichier constant, terminé par un zéro (HSTRING) ; aucun pointeur gardé.
    let _ = unsafe { LoadLibraryExW(&HSTRING::from("d3dcompiler_47.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) };
    for (_file, path) in &verified {
        // SAFETY: chemin complet d'un fichier vérifié et tenu ouvert, terminé par un zéro
        // (HSTRING) ; ses dépendances ne sont cherchées qu'à côté de lui et dans System32.
        // Le module reste chargé jusqu'à la fin du processus (la poignée n'est pas rendue).
        unsafe { LoadLibraryExW(&HSTRING::from(path.as_os_str()), None, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32) }.map_err(|e| format!("{} refusée par Windows : {e}", path.display()))?;
    }
    Ok(())
}

/// Demande le contexte à ANGLE pour la fenêtre à venir ; `Err` dit pourquoi
/// c'est OpenGL qui servira. À appeler avant de créer la fenêtre.
pub fn activate() -> Result<(), String> {
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        let dir = exe_dir().ok_or("dossier de l'exécutable inconnu")?;
        if !present(&dir) {
            return Err("bibliothèques ANGLE absentes du dossier de l'exécutable".to_string());
        }
        load_verified(&dir)?;
        glutin_winit::PREFER_EGL.store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        Err("ANGLE n'est embarqué que pour Windows x64".to_string())
    }
}

/// Préférences graphiques par application de Windows (HKCU) : une valeur par
/// exécutable, nommée par son chemin complet.
pub const GPU_KEY: &str = r"Software\Microsoft\DirectX\UserGpuPreferences";
/// « Économie d'énergie » : la carte intégrée d'un ordinateur à deux cartes.
pub const GPU_SAVING: &str = "GpuPreference=1;";

/// Demande la carte économe pour `exe`, sauf si une préférence existe déjà
/// (choix de l'utilisateur dans les Paramètres de Windows, ou la nôtre) : elle
/// n'est jamais remplacée. Rend vrai si la valeur a été écrite.
pub fn prefer_saving(h: &mut dyn Hive, exe: &Path) -> io::Result<bool> {
    let name = exe.display().to_string();
    if h.get(GPU_KEY, &name).is_some() {
        return Ok(false);
    }
    h.set(GPU_KEY, &name, GPU_SAVING)?;
    h.set(OURS_KEY, OURS_WROTE, "1")?;
    Ok(true)
}

/// Clé de Ferrocord où il note ce qu'il a fait de la préférence graphique.
const OURS_KEY: &str = r"Software\Ferrocord";
/// « 1 » : la préférence « économie d'énergie » de l'exécutable est la nôtre.
const OURS_WROTE: &str = "CarteEconomeEcrite";
/// « 1 » : la préférence d'une installation d'avant la 0.1.40 a été revue.
/// (« Revue2 » : la revue des versions 0.1.40 et 0.1.41, notée
/// « CarteEconomeRevue », se trompait sur les écrans et est refaite.)
const OURS_REVIEWED: &str = "CarteEconomeRevue2";
/// Note de la revue des versions 0.1.40 et 0.1.41, retirée à la désinstallation.
const OURS_REVIEWED_OLD: &str = "CarteEconomeRevue";

/// Règle la préférence d'après les écrans. `fits` : la carte économe pilote
/// tous les écrans (un portable à deux cartes) ; `None` : inconnu, rien ne
/// change.
///
/// Sur un ordinateur dont un écran est branché sur la carte dédiée, demander
/// la carte économe fait dessiner sur l'une et afficher par l'autre : les
/// deux pilotes sont chargés et chaque image est copiée de l'une à l'autre.
/// Mesuré sur un PC de bureau (RTX 4090 et UHD 770, 0.1.39, `preview --angle
/// --bench`) : 194 Mo privés et 178 Mo de travail avec la carte économe,
/// 125 Mo et 90 Mo sans préférence, images 20 à 30 % plus rapides. Notre
/// préférence est alors retirée ; celle que l'utilisateur a choisie dans les
/// Paramètres de Windows n'est jamais touchée.
pub fn tune_card(h: &mut dyn Hive, exe: &Path, fits: Option<bool>) -> io::Result<()> {
    let name = exe.display().to_string();
    // Avant la 0.1.40, rien ne disait qui avait écrit la valeur : la première
    // revue tient « économie d'énergie » pour la nôtre (elle était écrite au
    // premier lancement, avant tout réglage possible), une seule fois.
    let reviewed = h.get(OURS_KEY, OURS_REVIEWED).is_some();
    let ours = h.get(OURS_KEY, OURS_WROTE).is_some() || !reviewed;
    if !reviewed {
        h.set(OURS_KEY, OURS_REVIEWED, "1")?;
    }
    match fits {
        Some(true) => prefer_saving(h, exe).map(|_| ()),
        Some(false) if ours && h.get(GPU_KEY, &name).as_deref() == Some(GPU_SAVING) => {
            h.remove_value(GPU_KEY, &name)?;
            h.remove_value(OURS_KEY, OURS_WROTE)
        }
        _ => Ok(()),
    }
}

/// La carte économe pilote-t-elle tous les écrans ? `None` : une seule carte,
/// ou Windows ne le dit pas. N'ouvre aucun périphérique : aucun pilote
/// graphique n'est chargé par cette question.
///
/// Les écrans sont lus par `EnumDisplayDevices`, pas par DXGI : quand une
/// préférence de carte est déjà posée pour l'exécutable, DXGI range tous les
/// écrans sous la carte préférée (compatibilité des portables), et la
/// question recevait toujours « oui » (constaté sur un PC de bureau, 0.1.41 :
/// la préférence à retirer n'était jamais retirée).
#[cfg(windows)]
pub fn saving_card_drives_every_screen() -> Option<bool> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory6, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_GPU_PREFERENCE_MINIMUM_POWER};
    use windows::Win32::Graphics::Gdi::{EnumDisplayDevicesW, DISPLAY_DEVICEW, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP};
    let text = |w: &[u16]| String::from_utf16_lossy(w).trim_end_matches('\0').trim().to_string();
    // SAFETY: appels COM sur des objets valides, rendus par DXGI ; les
    // descriptions sont lues par valeur et rien n'est gardé.
    let (low, cards) = unsafe {
        let factory: IDXGIFactory6 = CreateDXGIFactory1().ok()?;
        let low: IDXGIAdapter1 = factory.EnumAdapterByGpuPreference(0, DXGI_GPU_PREFERENCE_MINIMUM_POWER).ok()?;
        let low = text(&low.GetDesc1().ok()?.Description);
        // Cartes matérielles de noms différents (une même carte peut paraître deux fois).
        let mut names: Vec<String> = Vec::new();
        let mut i = 0;
        while let Ok(a) = factory.EnumAdapters1(i) {
            i += 1;
            let Ok(d) = a.GetDesc1() else { continue };
            let name = text(&d.Description);
            if d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0 && !names.contains(&name) {
                names.push(name);
            }
        }
        (low, names.len())
    };
    if cards < 2 || low.is_empty() {
        return None;
    }
    let (mut on_low, mut elsewhere) = (false, false);
    for i in 0.. {
        let mut d = DISPLAY_DEVICEW { cb: size_of::<DISPLAY_DEVICEW>() as u32, ..Default::default() };
        // SAFETY: `d` est une structure locale de la taille annoncée (`cb`), remplie par Windows.
        if !unsafe { EnumDisplayDevicesW(None, i, &mut d, 0) }.as_bool() {
            break;
        }
        if d.StateFlags.0 & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP.0 != 0 {
            if text(&d.DeviceString) == low { on_low = true } else { elsewhere = true }
        }
    }
    (on_low || elsewhere).then_some(on_low && !elsewhere)
}

/// Désinstallation : retire la préférence de `exe` si c'est encore la nôtre
/// (un autre choix de l'utilisateur est laissé tel quel).
pub fn forget_saving(h: &mut dyn Hive, exe: &Path) -> io::Result<()> {
    let name = exe.display().to_string();
    // Nos deux notes partent avec elle, quel que soit le choix resté en place.
    h.remove_value(OURS_KEY, OURS_WROTE)?;
    h.remove_value(OURS_KEY, OURS_REVIEWED)?;
    h.remove_value(OURS_KEY, OURS_REVIEWED_OLD)?;
    if h.get(GPU_KEY, &name).as_deref() == Some(GPU_SAVING) { h.remove_value(GPU_KEY, &name) } else { Ok(()) }
}

/// Lancement de l'exécutable INSTALLÉ (`exe` dans son dossier d'installation) :
/// pose ANGLE au besoin (première fois, ou mise à jour qui en change la
/// version) et, s'il servira, demande la carte économe. Les erreurs ne
/// comptent pas : sans ANGLE, la fenêtre s'ouvre en OpenGL.
pub fn prepare_installed(h: &mut dyn Hive, exe: &Path) {
    let Some(dir) = exe.parent() else { return };
    if install(dir).is_ok() && !trial_failed(dir) && super::from_env().unwrap_or_default() == super::Rendu::Angle {
        #[cfg(windows)]
        let fits = saving_card_drives_every_screen();
        #[cfg(not(windows))]
        let fits = None;
        let _ = tune_card(h, exe, fits);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::hive::MemoryHive;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ferrocord-angle-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    fn bibliotheques_posees_reconnues_puis_remplacees_si_abimees() {
        let dir = temp("pose");
        assert!(!present(&dir));
        install(&dir).unwrap();
        assert!(present(&dir));
        for l in LIBRARIES {
            let data = std::fs::read(dir.join(l.name)).unwrap();
            assert_eq!(data.len() as u64, l.len);
            // Un exécutable Windows (« MZ »), pour processeur x64 seulement.
            assert_eq!(&data[..2], b"MZ");
            assert!(l.matches(&data));
        }
        assert_eq!(LIBRARIES.iter().map(|l| l.name).collect::<Vec<_>>(), FILES);
        // Même taille, autre contenu (autre version) : refusée, puis reposée.
        let egl = dir.join(FILES[0]);
        let mut data = std::fs::read(&egl).unwrap();
        data[200] ^= 0xFF;
        std::fs::write(&egl, &data).unwrap();
        assert!(!present(&dir));
        install(&dir).unwrap();
        assert!(present(&dir));
        // Aucun reste d'écriture.
        assert!(std::fs::read_dir(&dir).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().ends_with(PART)));
        // Démarrage sous ANGLE : le témoin est posé et tenu.
        assert_eq!(trial_begin(&dir), Trial::Started);
        // Un second exemplaire lancé pendant ce démarrage : occupé, pas raté.
        #[cfg(windows)]
        {
            assert_eq!(trial_begin(&dir), Trial::Busy);
            assert!(!trial_failed(&dir));
        }
        // Image affichée, ou fenêtre fermée sans erreur pendant le démarrage : retiré.
        trial_end(&dir);
        assert!(!dir.join(TRIAL).exists());
        // Processus mort pendant le démarrage : le témoin reste, avec la version…
        assert_eq!(trial_begin(&dir), Trial::Started);
        drop(HELD.lock().unwrap().take());
        assert_eq!(std::fs::read_to_string(dir.join(TRIAL)).unwrap(), VERSION);
        // … et interdit ANGLE aux lancements suivants de cette version…
        assert!(trial_failed(&dir));
        assert_eq!(trial_begin(&dir), Trial::Failed);
        assert_eq!(trial_begin(&dir), Trial::Failed);
        // Rendu perdu juste après le démarrage : même effet.
        std::fs::remove_file(dir.join(TRIAL)).unwrap();
        assert_eq!(trial_begin(&dir), Trial::Started);
        trial_fail(&dir);
        assert_eq!(trial_begin(&dir), Trial::Failed);
        // … jusqu'à ce que des bibliothèques soient reposées…
        std::fs::write(dir.join(FILES[1]), b"MZ").unwrap();
        install(&dir).unwrap();
        assert!(!dir.join(TRIAL).exists());
        // … ou jusqu'à la version suivante de Ferrocord (témoin d'une autre version, ou vide comme au lot « gl »).
        for old in ["0.0.0-autre", ""] {
            std::fs::write(dir.join(TRIAL), old).unwrap();
            assert!(!trial_failed(&dir));
            assert_eq!(trial_begin(&dir), Trial::Started);
            trial_end(&dir);
            assert!(!dir.join(TRIAL).exists());
        }
        // Altérée au-delà de l'en-tête : reconnue « posée », mais refusée au chargement et effacée.
        let gles = dir.join(FILES[1]);
        let mut data = std::fs::read(&gles).unwrap();
        data[100_000] ^= 0x01;
        std::fs::write(&gles, &data).unwrap();
        assert!(present(&dir));
        assert!(load_verified(&dir).unwrap_err().contains("sha256"));
        assert!(!gles.exists());
        install(&dir).unwrap();
        // Reste d'une pose interrompue : balayé à la pose suivante et à la désinstallation.
        std::fs::write(dir.join("libEGL.dll.123.part"), b"x").unwrap();
        std::fs::write(dir.join(FILES[1]), b"MZ").unwrap();
        install(&dir).unwrap();
        assert!(!dir.join("libEGL.dll.123.part").exists());
        // Dossier non inscriptible (ici : un fichier à la place du dossier) : une erreur, pas de panique.
        let not_a_dir = dir.join("fichier");
        std::fs::write(&not_a_dir, b"").unwrap();
        assert!(install(&not_a_dir).is_err());
        assert_eq!(trial_begin(&not_a_dir), Trial::Started);
        trial_end(&not_a_dir);
        std::fs::remove_file(&not_a_dir).unwrap();
        // Fichier tronqué : pareil.
        std::fs::write(dir.join(FILES[1]), b"MZ").unwrap();
        assert!(!present(&dir));
        install(&dir).unwrap();
        assert!(present(&dir));
        // Désinstallation : plus rien à nous dans le dossier.
        std::fs::write(dir.join("libGLESv2.dll.7.part"), b"x").unwrap();
        std::fs::write(dir.join(TRIAL), VERSION).unwrap();
        remove(&dir);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn dossier_sans_bibliotheques() {
        let dir = temp("vide");
        assert!(!present(&dir));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn carte_econome_sans_ecraser_le_choix_de_l_utilisateur() {
        let mut h = MemoryHive::default();
        let exe = Path::new(r"C:\Programmes\Ferrocord\ferrocord.exe");
        // Un écran sur la carte dédiée : notre préférence est retirée…
        {
            let mut h = MemoryHive::default();
            let name = exe.display().to_string();
            tune_card(&mut h, exe, Some(true)).unwrap();
            assert_eq!(h.get(GPU_KEY, &name).as_deref(), Some(GPU_SAVING));
            tune_card(&mut h, exe, None).unwrap();
            assert_eq!(h.get(GPU_KEY, &name).as_deref(), Some(GPU_SAVING), "inconnu : rien ne change");
            tune_card(&mut h, exe, Some(false)).unwrap();
            assert_eq!(h.get(GPU_KEY, &name), None);
            // … mais jamais celle que l'utilisateur a choisie ensuite dans Windows.
            h.set(GPU_KEY, &name, GPU_SAVING).unwrap();
            tune_card(&mut h, exe, Some(false)).unwrap();
            assert_eq!(h.get(GPU_KEY, &name).as_deref(), Some(GPU_SAVING));
            // Installation d'avant la 0.1.40 : la valeur est tenue pour la nôtre, une fois.
            let mut old = MemoryHive::default();
            old.set(GPU_KEY, &name, GPU_SAVING).unwrap();
            tune_card(&mut old, exe, Some(false)).unwrap();
            assert_eq!(old.get(GPU_KEY, &name), None);
            old.set(GPU_KEY, &name, "GpuPreference=2;").unwrap();
            tune_card(&mut old, exe, Some(false)).unwrap();
            assert_eq!(old.get(GPU_KEY, &name).as_deref(), Some("GpuPreference=2;"));
        }
        assert!(prefer_saving(&mut h, exe).unwrap());
        assert_eq!(h.get(GPU_KEY, &exe.display().to_string()).as_deref(), Some(GPU_SAVING));
        // Déjà là : rien n'est réécrit.
        assert!(!prefer_saving(&mut h, exe).unwrap());
        forget_saving(&mut h, exe).unwrap();
        assert!(h.is_empty());
        // Choix de l'utilisateur (« performances élevées ») : ni remplacé, ni retiré.
        h.set(GPU_KEY, &exe.display().to_string(), "GpuPreference=2;").unwrap();
        assert!(!prefer_saving(&mut h, exe).unwrap());
        forget_saving(&mut h, exe).unwrap();
        assert_eq!(h.get(GPU_KEY, &exe.display().to_string()).as_deref(), Some("GpuPreference=2;"));
    }
}
