//! Temps processeur de chaque fil du processus, pour savoir ce qui occupe
//! le démarrage (« prêt » de `tools/mesures.ps1`) sans deviner (lot C9).
//!
//! Un fil d'échantillonnage relève, toutes les [`FAST`] pendant [`FAST_FOR`]
//! puis toutes les [`SLOW`], le temps processeur consommé par chaque fil
//! depuis le relevé précédent, cumulé par nom de fil (`fil.<nom>`, colonne
//! `ms`). Nom : la description du fil (`std::thread::Builder::name`, tokio),
//! `principal` pour le fil d'interface ; un fil sans description (créé par
//! une bibliothèque du système ou par le pilote graphique) porte le nom du
//! module où il démarre : `sans_nom.<module>` (`sans_nom.ntdll` : réserve de
//! fils de Windows ; `sans_nom.atio6axx` ou `sans_nom.nvoglv64` : pilote
//! OpenGL). Rien d'autre. Actif seulement avec `FERROCORD_PERF=1`.
//!
//! Lot « perf » : le relevé ne doit pas peser sur ce qu'il mesure. La liste
//! des fils (un instantané de tous les fils du système : une vingtaine de
//! millisecondes sur la machine mesurée) était refaite à chaque relevé, soit
//! 8 à 9 % d'un cœur pendant les vingt premières secondes : à elle seule,
//! elle tenait le processus au-dessus du seuil de calme de la mesure (5 %),
//! et « prêt » valait toujours une vingtaine de secondes. Les fils connus
//! sont maintenant gardés ouverts : un relevé ne lit que leurs compteurs
//! (quelques microsecondes par fil). La liste n'est refaite que si du temps
//! processeur du processus n'est attribué à aucun fil connu (fil né depuis),
//! au plus toutes les [`RESCAN_MIN`], et sinon toutes les [`RESCAN`].

use super::{wakes, Recorder};
use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// Rythme serré du démarrage, puis relâché.
pub const FAST: Duration = Duration::from_millis(250);
pub const FAST_FOR: Duration = Duration::from_secs(20);
pub const SLOW: Duration = Duration::from_secs(5);
/// La liste des fils est refaite au plus à ce rythme (quand du temps n'est
/// attribué à aucun fil connu)…
pub const RESCAN_MIN: Duration = Duration::from_secs(2);
/// … et au moins à celui-ci (fils finis rendus au système).
pub const RESCAN: Duration = Duration::from_secs(60);
/// Temps du processus attribué à aucun fil connu (unités de 100 ns) au-delà
/// duquel la liste est refaite : deux tics de l'horloge du système.
const GAP: u64 = 312_500;
/// En dessous (ms sur l'intervalle), pas de ligne : le fichier reste court.
const MIN_MS: f64 = 0.05;

/// Fil d'échantillonnage ; arrêté à la fin de vie.
pub struct Sampler {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Sampler {
    /// À appeler depuis le fil d'interface (noté `principal`).
    pub fn start(rec: Recorder) -> Option<Sampler> {
        let main = os::current_thread();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let handle = std::thread::Builder::new()
            .name("ferrocord-perf-fils".into())
            .spawn(move || {
                let started = Instant::now();
                let mut reader = os::Reader::default();
                let mut scan = Scan::default();
                let mut before: HashMap<u32, u64> = HashMap::new();
                while !flag.load(Ordering::Relaxed) {
                    let now = scan.read(&mut reader, Instant::now());
                    for (name, ms) in deltas(&mut before, now, main) {
                        rec.record_key(Cow::Owned(format!("fil.{name}")), ms);
                    }
                    let wait = if started.elapsed() < FAST_FOR { FAST } else { SLOW };
                    // Un seul sommeil par relevé ; la fin de vie du relevé le réveille (`unpark`).
                    let until = Instant::now() + wait;
                    while !flag.load(Ordering::Relaxed) {
                        let left = until.saturating_duration_since(Instant::now());
                        if left.is_zero() {
                            break;
                        }
                        std::thread::park_timeout(left);
                    }
                }
            })
            .ok()?;
        Some(Sampler { stop, handle: Some(handle) })
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            h.thread().unpark();
            let _ = h.join();
        }
    }
}

/// Quand refaire la liste des fils (coûteux) plutôt que relire les fils connus.
#[derive(Default)]
struct Scan {
    last: Option<Instant>,
    /// Temps du processus hors des fils connus, juste après la dernière liste.
    gap: u64,
}

impl Scan {
    /// La liste doit-elle être refaite ? `gap` : temps du processus attribué à aucun fil connu.
    fn due(&self, now: Instant, gap: u64) -> bool {
        match self.last {
            None => true,
            Some(t) => {
                let since = now.saturating_duration_since(t);
                since >= RESCAN || (since >= RESCAN_MIN && gap.saturating_sub(self.gap) >= GAP)
            }
        }
    }

    fn read(&mut self, reader: &mut os::Reader, now: Instant) -> Vec<Seen> {
        let mut seen = reader.read();
        let gap = |seen: &[Seen]| os::process_cpu().saturating_sub(seen.iter().map(|t| t.cpu).sum());
        if self.due(now, gap(&seen)) {
            reader.rescan();
            seen = reader.read();
            self.last = Some(now);
            self.gap = gap(&seen);
        }
        seen
    }
}

/// Temps processeur cumulé (ms) du fil appelant, puis de tous les autres fils
/// du processus (pilote graphique, décodage des images, réseau) : le banc
/// `preview --bench` en tire le coût d'une image hors du fil d'interface (lot C10).
pub fn totals() -> (f64, f64) {
    let me = os::current_thread();
    let (mut mine, mut others) = (0.0, 0.0);
    let mut reader = os::Reader::default();
    reader.rescan();
    for t in reader.read() {
        *(if t.id == me { &mut mine } else { &mut others }) += t.cpu as f64 / 10_000.0;
    }
    (mine, others)
}

/// Un fil vu au relevé : identifiant, nom, temps processeur total (100 ns).
pub struct Seen {
    pub id: u32,
    pub name: Option<String>,
    pub cpu: u64,
}

/// Temps consommé depuis le relevé précédent, cumulé par nom (ms) ; le
/// premier relevé d'un fil compte tout son temps (fil né dans l'intervalle,
/// ou premier relevé : ce qui s'est passé avant).
fn deltas(before: &mut HashMap<u32, u64>, now: Vec<Seen>, main: u32) -> Vec<(String, f64)> {
    let mut by_name: HashMap<String, f64> = HashMap::new();
    let mut next = HashMap::with_capacity(now.len());
    for t in now {
        let spent = t.cpu.saturating_sub(before.get(&t.id).copied().unwrap_or(0));
        next.insert(t.id, t.cpu);
        let name = if t.id == main {
            "principal".to_string()
        } else {
            t.name.as_deref().map(wakes::clean).filter(|n| !n.is_empty()).unwrap_or_else(|| "sans_nom".into())
        };
        *by_name.entry(name).or_default() += spent as f64 / 10_000.0;
    }
    *before = next;
    let mut v: Vec<(String, f64)> = by_name.into_iter().filter(|(_, ms)| *ms >= MIN_MS).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

#[cfg(windows)]
mod os {
    use super::Seen;
    use std::collections::HashMap;
    use windows::Wdk::System::Threading::{NtQueryInformationThread, ThreadQuerySetWin32StartAddress};
    use windows::Win32::Foundation::{CloseHandle, LocalFree, FILETIME, HANDLE, HLOCAL, HMODULE};
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32};
    use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT};
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId, GetProcessTimes, GetThreadDescription, GetThreadTimes, OpenThread, THREAD_QUERY_INFORMATION, THREAD_QUERY_LIMITED_INFORMATION,
    };

    /// Fils au-delà : ignorés (borne de sûreté ; le système en a quelques milliers).
    const MAX_THREADS: usize = 65_536;

    struct Owned(HANDLE);

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: le descripteur vient d'un appel réussi qui le donne à
            // l'appelant ; il n'est fermé qu'ici, une seule fois.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    pub fn current_thread() -> u32 {
        // SAFETY: aucun argument, aucun effet ; rend l'identifiant du fil appelant.
        unsafe { GetCurrentThreadId() }
    }

    fn ticks(ft: FILETIME) -> u64 {
        (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime)
    }

    /// Temps processeur du processus entier (100 ns), fils finis compris.
    pub fn process_cpu() -> u64 {
        let (mut c, mut x, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        // SAFETY: pseudo-descripteur de ce processus (jamais à fermer) ; les
        // quatre pointeurs désignent des FILETIME locaux vivants pendant l'appel.
        match unsafe { GetProcessTimes(GetCurrentProcess(), &mut c, &mut x, &mut k, &mut u) } {
            Ok(()) => ticks(k) + ticks(u),
            Err(_) => 0,
        }
    }

    /// Un fil connu : gardé ouvert d'un relevé à l'autre, nommé une seule fois.
    struct Known {
        handle: Owned,
        name: Option<String>,
    }

    /// Fils connus de ce processus. Créé et utilisé par un seul fil (celui des relevés).
    #[derive(Default)]
    pub struct Reader {
        known: HashMap<u32, Known>,
    }

    impl Reader {
        /// Temps processeur des fils connus : un appel au système par fil, rien d'autre.
        pub fn read(&self) -> Vec<Seen> {
            self.known.iter().filter_map(|(id, k)| Some(Seen { id: *id, name: k.name.clone(), cpu: cpu(&k.handle)? })).collect()
        }

        /// Refait la liste des fils du processus (instantané de tous les fils
        /// du système : coûteux) : nouveaux fils ouverts et nommés, fils finis rendus.
        pub fn rescan(&mut self) {
            // SAFETY: aucun argument hors drapeaux ; rend l'identifiant de ce processus.
            let me = unsafe { GetCurrentProcessId() };
            // SAFETY: drapeau et identifiant valides ; le descripteur rendu est
            // possédé par `Owned`, qui le ferme.
            let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }) else { return };
            let snap = Owned(snap);
            let mut e = THREADENTRY32 { dwSize: std::mem::size_of::<THREADENTRY32>() as u32, ..Default::default() };
            // SAFETY: `e` est une structure locale de la bonne taille (`dwSize`),
            // vivante pendant l'appel ; l'instantané est ouvert.
            let mut ok = unsafe { Thread32First(snap.0, &mut e) }.is_ok();
            let mut alive = Vec::new();
            let mut seen = 0;
            while ok && seen < MAX_THREADS {
                seen += 1;
                if e.th32OwnerProcessID == me {
                    alive.push(e.th32ThreadID);
                }
                // SAFETY: comme pour Thread32First.
                ok = unsafe { Thread32Next(snap.0, &mut e) }.is_ok();
            }
            self.known.retain(|id, _| alive.contains(id));
            for id in alive {
                if !self.known.contains_key(&id)
                    && let Some(k) = open(id)
                {
                    self.known.insert(id, k);
                }
            }
        }
    }

    fn cpu(h: &Owned) -> Option<u64> {
        let (mut c, mut x, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        // SAFETY: descripteur ouvert ; les quatre pointeurs désignent des
        // FILETIME locaux vivants pendant l'appel, que la fonction remplit.
        unsafe { GetThreadTimes(h.0, &mut c, &mut x, &mut k, &mut u) }.ok()?;
        Some(ticks(k) + ticks(u))
    }

    fn open(id: u32) -> Option<Known> {
        // SAFETY: droits de lecture seule sur un fil de ce processus (le droit
        // restreint si le droit complet est refusé) ; le descripteur rendu est
        // possédé par `Owned`.
        let h = unsafe { OpenThread(THREAD_QUERY_INFORMATION, false, id).or_else(|_| OpenThread(THREAD_QUERY_LIMITED_INFORMATION, false, id)) }.ok()?;
        let handle = Owned(h);
        // SAFETY: descripteur ouvert. En cas de succès, la chaîne rendue est
        // allouée par le système, terminée par un zéro, et doit être libérée
        // par LocalFree : lue d'abord, puis libérée une seule fois.
        let name = unsafe {
            match GetThreadDescription(handle.0) {
                Ok(p) if !p.is_null() => {
                    let s = p.to_string().ok();
                    let _ = LocalFree(Some(HLOCAL(p.0.cast())));
                    s
                }
                _ => None,
            }
        };
        let name = name.filter(|n| !n.is_empty()).or_else(|| start_module(&handle).map(|m| format!("sans_nom.{m}")));
        Some(Known { handle, name })
    }

    /// Module (nom du fichier, sans extension) où démarre un fil sans
    /// description : dit à qui il appartient (pilote graphique, système).
    fn start_module(h: &Owned) -> Option<String> {
        let mut start: usize = 0;
        // SAFETY: descripteur ouvert ; la classe ThreadQuerySetWin32StartAddress
        // écrit une adresse (taille d'un pointeur) dans `start`, variable locale
        // de cette taille, vivante pendant l'appel ; la longueur rendue n'est pas demandée.
        let status = unsafe { NtQueryInformationThread(h.0, ThreadQuerySetWin32StartAddress, (&raw mut start).cast(), std::mem::size_of::<usize>() as u32, std::ptr::null_mut()) };
        if status.is_err() || start == 0 {
            return None;
        }
        let mut module = HMODULE::default();
        // SAFETY: avec FROM_ADDRESS, le second argument est une adresse à
        // chercher dans les modules chargés (jamais lue comme une chaîne) ;
        // UNCHANGED_REFCOUNT : aucun compte à rendre. `module` est une
        // variable locale vivante pendant l'appel.
        unsafe { GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, windows::core::PCWSTR(start as *const u16), &mut module) }.ok()?;
        let mut path = [0u16; 512];
        // SAFETY: module trouvé ci-dessus dans ce processus ; le tampon local
        // est donné avec sa longueur, la fonction n'écrit pas au-delà.
        let n = unsafe { GetModuleFileNameW(Some(module), &mut path) } as usize;
        let path = String::from_utf16_lossy(path.get(..n)?);
        let file = path.rsplit(['\\', '/']).next()?;
        let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
        (!stem.is_empty()).then(|| stem.to_string())
    }
}

#[cfg(not(windows))]
mod os {
    use super::Seen;

    pub fn current_thread() -> u32 {
        0
    }

    pub fn process_cpu() -> u64 {
        0
    }

    #[derive(Default)]
    pub struct Reader;

    impl Reader {
        pub fn read(&self) -> Vec<Seen> {
            Vec::new()
        }

        pub fn rescan(&mut self) {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deltas_group_by_name_and_count_only_new_time() {
        let mut before = HashMap::new();
        let seen = |id, name: Option<&str>, cpu| Seen { id, name: name.map(String::from), cpu };
        // Premier relevé : tout le temps passé ; deux fils tokio cumulés.
        let first = deltas(&mut before, vec![seen(1, None, 20_000), seen(2, Some("tokio-runtime-worker"), 10_000), seen(3, Some("tokio-runtime-worker"), 5_000), seen(4, None, 1)], 1);
        assert_eq!(first, vec![("principal".into(), 2.0), ("tokio_runtime_worker".into(), 1.5)], "moins de 0,05 ms : pas de ligne");
        // Second relevé : seulement l'écart ; un fil disparu ne compte plus.
        let second = deltas(&mut before, vec![seen(1, None, 30_000), seen(2, Some("tokio-runtime-worker"), 10_000), seen(5, Some("Polices é"), 2_000)], 1);
        assert_eq!(second, vec![("polices__".into(), 0.2), ("principal".into(), 1.0)]);
        // Fil sans description, nommé d'après le module où il démarre.
        let third = deltas(&mut before, vec![seen(6, Some("sans_nom.atio6axx"), 30_000)], 1);
        assert_eq!(third, vec![("sans_nom.atio6axx".into(), 3.0)]);
    }

    /// Lot « perf » : la liste des fils (coûteuse) est refaite au premier
    /// relevé, puis rarement ; jamais à chaque relevé.
    #[test]
    fn the_thread_list_is_rarely_rebuilt() {
        let t0 = Instant::now();
        let mut scan = Scan::default();
        assert!(scan.due(t0, 0), "premier relevé");
        scan.last = Some(t0);
        scan.gap = 1_000_000;
        // Rien de nouveau : pas de liste avant RESCAN.
        assert!(!scan.due(t0 + FAST, 1_000_000));
        assert!(!scan.due(t0 + RESCAN_MIN * 5, 1_000_000 + GAP - 1));
        assert!(scan.due(t0 + RESCAN, 1_000_000));
        // Du temps attribué à aucun fil connu : liste refaite, mais pas plus souvent que RESCAN_MIN.
        assert!(!scan.due(t0 + FAST, 1_000_000 + GAP * 10));
        assert!(scan.due(t0 + RESCAN_MIN, 1_000_000 + GAP));
    }

    #[cfg(windows)]
    #[test]
    fn this_process_threads_are_seen_with_their_name() {
        let h = std::thread::Builder::new()
            .name("fil-de-test-c9".into())
            .spawn(|| {
                let mut r = os::Reader::default();
                r.rescan();
                let t = r.read();
                assert!(t.iter().any(|t| t.name.as_deref() == Some("fil-de-test-c9")), "nom du fil relevé");
                assert!(t.iter().any(|t| t.id == os::current_thread()));
            })
            .unwrap();
        h.join().unwrap();
    }

    /// Lot « perf », bout en bout : le relevé tourne trois secondes au rythme
    /// du démarrage et dit lui-même ce qu'il a consommé (`fil.ferrocord_perf_fils`).
    /// Avant le lot, chaque relevé refaisait la liste des fils (une trentaine
    /// de millisecondes ici), soit plus de 10 % d'un cœur : la mesure « prêt »
    /// (5 % sur 2 s) ne pouvait pas aboutir avant la fin du rythme serré.
    #[cfg(windows)]
    #[test]
    fn the_sampler_stays_far_below_the_calm_threshold() {
        let dir = std::env::temp_dir().join(format!("ferrocord-perf-fils-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("perf.csv");
        let Ok(rec) = Recorder::start(file.clone(), Instant::now()) else { panic!("journal non créé") };
        let sampler = Sampler::start(rec.share());
        assert!(sampler.is_some());
        let run = Duration::from_secs(3);
        std::thread::sleep(run);
        let stopping = Instant::now();
        drop(sampler);
        assert!(stopping.elapsed() < Duration::from_millis(200), "l'arrêt réveille le fil du relevé");
        drop(rec);
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&dir);
        let own: Vec<f64> = text.lines().filter_map(|l| l.split_once(",fil.ferrocord_perf_fils,")).filter_map(|(_, ms)| ms.parse().ok()).collect();
        let spent: f64 = own.iter().sum();
        // Le premier relevé compte le passé de chaque fil : au moins une ligne.
        assert!(text.lines().any(|l| l.contains(",fil.")), "{text}");
        // Au plus deux listes en 3 s (au départ, puis après RESCAN_MIN) : moins de 5 % d'un cœur, contre plus de 10 % avant.
        assert!(spent <= run.as_secs_f64() * 1000.0 * 0.05, "le relevé a consommé {spent} ms en {run:?} : {own:?}");
    }

    /// Lot « perf » : relire les fils connus coûte très peu (le relevé ne doit
    /// pas tenir le processeur au-dessus du seuil de calme de la mesure).
    #[cfg(windows)]
    #[test]
    fn a_reading_of_known_threads_is_cheap() {
        let mut r = os::Reader::default();
        let at = Instant::now();
        r.rescan();
        let scan = at.elapsed();
        let at = Instant::now();
        const N: u32 = 200;
        for _ in 0..N {
            assert!(!r.read().is_empty());
        }
        let read = at.elapsed() / N;
        let names: Vec<String> = r.read().into_iter().filter_map(|t| t.name).collect();
        eprintln!("liste des fils : {scan:?} ; relevé des fils connus : {read:?} ; noms : {names:?}");
        // 4 relevés par seconde à moins de 2 ms : moins de 1 % d'un cœur (marge large : machine chargée).
        assert!(read < Duration::from_millis(2), "{read:?}");
    }
}
