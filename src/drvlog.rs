//! Qui charge un pilote graphique ? Noté dans le journal technique
//! ([`crate::sharelog`]), avec le fil et la chaîne de bibliothèques.
//!
//! Sur un PC à deux cartes, Ferrocord démarre avec le seul pilote de la carte
//! économe ; constaté en 0.1.43 : après deux heures, le pilote de l'autre
//! carte est chargé lui aussi (une centaine de Mo), sans partage ni caméra.
//! Sons, décodeurs et lecteur vidéo ont été écartés par essai : plutôt qu'une
//! supposition de plus, Windows nous prévient à chaque bibliothèque chargée,
//! et celles d'un pilote graphique sont notées avec la pile d'appels.
//!
//! Rien de privé : des noms de fichiers de bibliothèques et des décalages.

#![cfg(windows)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Mutex;

/// Débuts de noms des bibliothèques de pilotes graphiques (NVIDIA, Intel, AMD).
const DRIVERS: [&str; 10] = ["nvwgf2um", "nvd3dum", "nvoglv", "nvldumd", "igd10", "igd11", "igd12", "igdumd", "atidxx", "amdxx"];
/// Appels relevés au plus par chargement.
const DEPTH: usize = 48;
/// Chargements gardés au plus en attente d'écriture.
const MAX_PENDING: usize = 16;

struct Load {
    name: String,
    thread: u32,
    stack: Vec<usize>,
}

static PENDING: Mutex<Vec<Load>> = Mutex::new(Vec::new());
static SOME: AtomicBool = AtomicBool::new(false);

fn watched(name: &str, extra: &[&str]) -> bool {
    let low = name.to_ascii_lowercase();
    DRIVERS.iter().chain(extra).any(|p| low.starts_with(p))
}

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *const u16,
}

/// `LDR_DLL_LOADED_NOTIFICATION_DATA` (même forme au déchargement).
#[repr(C)]
struct Notification {
    flags: u32,
    full_name: *const UnicodeString,
    base_name: *const UnicodeString,
    base: *const c_void,
    size: u32,
}

type OnLoad = unsafe extern "system" fn(u32, *const Notification, *const c_void);
/// `LdrRegisterDllNotification` de ntdll : absente de sa bibliothèque
/// d'édition de liens, cherchée à l'exécution.
type Register = unsafe extern "system" fn(u32, OnLoad, *const c_void, *mut *mut c_void) -> i32;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn RtlCaptureStackBackTrace(skip: u32, count: u32, frames: *mut *mut c_void, hash: *mut u32) -> u16;
}

#[cfg(test)]
const EXTRA: &[&str] = &["msftedit"];
#[cfg(not(test))]
const EXTRA: &[&str] = &[];

/// Appelée par Windows pendant le chargement, verrou du chargeur tenu : rien
/// d'autre ici que relever le nom et la pile ; l'écriture attend [`drain`].
unsafe extern "system" fn on_load(reason: u32, data: *const Notification, _context: *const c_void) {
    const LOADED: u32 = 1;
    if reason != LOADED || data.is_null() {
        return;
    }
    // SAFETY: pendant l'appel, Windows garantit `data` et ses chaînes
    // valides ; `length` est en octets, la chaîne n'est lue que sur cette longueur.
    let name = unsafe {
        let base = (*data).base_name;
        if base.is_null() || (*base).buffer.is_null() {
            return;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts((*base).buffer, usize::from((*base).length) / 2))
    };
    if !watched(&name, EXTRA) {
        return;
    }
    let mut frames = [std::ptr::null_mut::<c_void>(); DEPTH];
    // SAFETY: `frames` a `DEPTH` cases, nombre annoncé ; la fonction rend le nombre de cases remplies.
    let n = unsafe { RtlCaptureStackBackTrace(0, DEPTH as u32, frames.as_mut_ptr(), std::ptr::null_mut()) };
    // SAFETY: sans argument, sans échec possible.
    let thread = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
    // Verrou pris ailleurs (écriture en cours) : ce chargement n'est pas noté, rien n'attend ici.
    if let Ok(mut p) = PENDING.try_lock()
        && p.len() < MAX_PENDING
    {
        p.push(Load { name, thread, stack: frames[..usize::from(n).min(DEPTH)].iter().map(|f| *f as usize).collect() });
        SOME.store(true, Relaxed);
    }
}

/// À appeler une fois au lancement, avant la fenêtre.
pub fn install() {
    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.swap(true, Relaxed) {
        return;
    }
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    let mut cookie = std::ptr::null_mut();
    // SAFETY: ntdll est chargée dans tout programme ; la fonction trouvée
    // sous ce nom a la signature documentée (`Register`). `on_load` est une
    // fonction du programme, valable toute sa vie ; l'inscription n'est
    // jamais retirée, `cookie` reçoit sa poignée.
    unsafe {
        let Ok(ntdll) = GetModuleHandleW(windows::core::w!("ntdll.dll")) else { return };
        let Some(f) = GetProcAddress(ntdll, windows::core::s!("LdrRegisterDllNotification")) else { return };
        let register: Register = std::mem::transmute(f);
        let _ = register(0, on_load, std::ptr::null(), &mut cookie);
    }
}

/// Nom du fichier de la bibliothèque qui contient `addr`, et décalage dedans.
fn module_of(addr: usize) -> Option<(String, usize)> {
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT};
    let mut module = HMODULE::default();
    let mut path = [0u16; 512];
    // SAFETY: avec FROM_ADDRESS, le « nom » est une adresse quelconque, seulement
    // comparée aux bibliothèques chargées ; `path` a la taille annoncée.
    let n = unsafe {
        GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, windows::core::PCWSTR(addr as *const u16), &mut module).ok()?;
        GetModuleFileNameW(Some(module), &mut path) as usize
    };
    let full = String::from_utf16_lossy(&path[..n.min(path.len())]);
    let file = full.rsplit(['\\', '/']).next().unwrap_or_default().to_string();
    Some((file, addr.wrapping_sub(module.0 as usize)))
}

/// Nom donné au fil `id` (ceux de Ferrocord sont nommés), s'il vit encore.
fn thread_name(id: u32) -> String {
    use windows::Win32::Foundation::{CloseHandle, LocalFree, HLOCAL};
    use windows::Win32::System::Threading::{GetThreadDescription, OpenThread, THREAD_QUERY_LIMITED_INFORMATION};
    // SAFETY: poignée ouverte ici et refermée ; la chaîne rendue par Windows
    // est copiée puis libérée par LocalFree, comme sa documentation le demande.
    unsafe {
        let Ok(h) = OpenThread(THREAD_QUERY_LIMITED_INFORMATION, false, id) else { return String::new() };
        let name = GetThreadDescription(h).ok().map(|p| {
            let s = p.to_string().unwrap_or_default();
            let _ = LocalFree(Some(HLOCAL(p.0.cast())));
            s
        });
        let _ = CloseHandle(h);
        name.unwrap_or_default()
    }
}

/// Lignes à écrire pour les chargements relevés depuis le dernier appel.
/// Sans chargement : une lecture d'un booléen, rien d'autre.
pub fn drain() -> Vec<String> {
    if !SOME.swap(false, Relaxed) {
        return Vec::new();
    }
    let loads = std::mem::take(&mut *PENDING.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
    loads
        .into_iter()
        .map(|l| {
            // Une bibliothèque par maillon : appels voisins de la même réunis,
            // décalages gardés pour l'exécutable seul (il n'a pas de symboles).
            let mut chain: Vec<String> = Vec::new();
            let mut last = String::new();
            for addr in l.stack {
                let Some((file, off)) = module_of(addr) else { continue };
                if file.to_ascii_lowercase().ends_with(".exe") {
                    chain.push(format!("{file}+{off:#x}"));
                } else if file != last {
                    chain.push(file.clone());
                }
                last = file;
            }
            format!("pilote : {} chargé par le fil {} « {} » : {}", l.name, l.thread, thread_name(l.thread), chain.join(" < "))
        })
        .collect()
}

/// Écrit dans le journal technique ce qui a été relevé.
pub fn tick() {
    for line in drain() {
        crate::sharelog::line(format_args!("{line}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_names_are_recognised() {
        assert!(watched("nvwgf2umx.dll", &[]) && watched("IGD10UM64XE.DLL", &[]) && watched("atidxx64.dll", &[]));
        assert!(!watched("kernel32.dll", &[]) && !watched("nvapi64.dll", &[]) && !watched("", &[]));
    }

    /// Un vrai chargement, par une bibliothèque de Windows sans effet, surveillée pour l'essai.
    #[test]
    fn a_load_is_noted_with_who_asked() {
        install();
        let done = std::thread::Builder::new().name("essai-pilote".into()).spawn(|| {
            // SAFETY: nom constant terminé par zéro ; la bibliothèque reste chargée.
            unsafe { windows::Win32::System::LibraryLoader::LoadLibraryW(windows::core::w!("msftedit.dll")) }.is_ok()
        });
        assert!(done.unwrap().join().unwrap());
        // Le fil est fini : son nom n'est plus lisible, la chaîne d'appels si.
        let lines = drain();
        let line = lines.iter().find(|l| l.to_ascii_lowercase().contains("msftedit.dll")).unwrap_or_else(|| panic!("rien de noté : {lines:?}"));
        assert!(line.contains("ntdll.dll") && line.contains(".exe+0x"), "{line}");
        assert!(drain().is_empty());
    }
}
