//! Mémoire du processus et sa répartition (lot « ram »), pour le journal de
//! performance (`FERROCORD_PERF`) et le banc de l'aperçu. Rien n'est appelé
//! quand le journal est désactivé.
//!
//! Deux relevés :
//! - [`usage`] : les compteurs de Windows (mémoire privée engagée, mémoire de
//!   travail, part privée de la mémoire de travail : la colonne « Mémoire » du
//!   Gestionnaire des tâches) ;
//! - [`map`] : à qui appartient la mémoire privée engagée. L'espace d'adressage
//!   est parcouru région par région : pages modifiables des modules (par DLL :
//!   le pilote OpenGL y pèse lourd), piles des fils, tas (engagé contre
//!   réellement alloué : l'écart est la fragmentation), et le reste
//!   (`VirtualAlloc` directs, surtout le pilote graphique).
//!
//! Seuls des tailles et des noms de fichiers de modules (sans dossier) en
//! sortent : aucun chemin, aucun contenu.

use std::{borrow::Cow, collections::HashMap, time::Duration};
use windows::Win32::{
    Foundation::{HANDLE, HMODULE},
    System::{
        LibraryLoader::GetModuleFileNameW,
        Memory::{
            GetProcessHeaps, HeapSummary, VirtualQuery, HEAP_SUMMARY, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_IMAGE, MEM_MAPPED, MEM_PRIVATE, PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_READWRITE,
            PAGE_WRITECOPY,
        },
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2},
        Threading::GetCurrentProcess,
    },
};

/// Compteurs de Windows, en octets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// Mémoire privée engagée (`PrivateUsage`, celle de `PrivateMemorySize64`).
    pub private: u64,
    /// Mémoire de travail (pages présentes en mémoire vive, partagées comprises).
    pub working: u64,
    /// Part privée de la mémoire de travail (colonne « Mémoire » du Gestionnaire des tâches).
    pub private_working: u64,
    pub peak_working: u64,
}

/// Compteurs du processus ; `None` si Windows ne les donne pas.
pub fn usage() -> Option<Usage> {
    let mut c = PROCESS_MEMORY_COUNTERS_EX2 { cb: size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32, ..Default::default() };
    // SAFETY: GetCurrentProcess rend un pseudo-handle constant. Le pointeur
    // désigne une structure locale de la taille annoncée (`cb`), vivante
    // pendant l'appel, que la fonction se contente de remplir ; le transtypage
    // vers la structure de base est celui que demande l'API pour ses variantes.
    unsafe { GetProcessMemoryInfo(GetCurrentProcess(), (&raw mut c).cast::<PROCESS_MEMORY_COUNTERS>(), c.cb) }.ok()?;
    Some(Usage { private: c.PrivateUsage as u64, working: c.WorkingSetSize as u64, private_working: c.PrivateWorkingSetSize as u64, peak_working: c.PeakWorkingSetSize as u64 })
}

/// Répartition de la mémoire privée engagée, en octets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Map {
    /// Pages modifiables des modules (exécutable et DLL), toutes confondues.
    pub images: u64,
    /// Les mêmes, par module (nom de fichier en minuscules), du plus lourd au plus léger.
    pub by_module: Vec<(String, u64)>,
    /// Piles des fils (partie engagée) et leur nombre.
    pub stacks: u64,
    pub stack_count: usize,
    /// Tas de Windows : engagé, et réellement alloué (blocs vivants).
    pub heaps: u64,
    pub heaps_allocated: u64,
    pub heap_count: usize,
    /// Reste de la mémoire privée (ni module, ni pile, ni tas) : réservations
    /// directes, surtout celles du pilote graphique.
    pub other: u64,
    /// Sections partagées engagées (hors mémoire privée ; pour mémoire).
    pub mapped: u64,
}

impl Map {
    /// Total attribué (comparable à [`Usage::private`]).
    pub fn total(&self) -> u64 {
        self.images + self.stacks + self.heaps + self.other
    }
}

/// Parcourt l'espace d'adressage du processus. Coûteux (quelques
/// millisecondes) : réservé aux relevés, jamais dans une image.
pub fn map() -> Map {
    let mut m = Map::default();
    let mut modules: HashMap<usize, u64> = HashMap::new();
    // Par réservation (`AllocationBase`) : octets privés engagés, et page de garde vue (pile).
    let mut private: HashMap<usize, (u64, bool)> = HashMap::new();
    let mut addr = 0usize;
    loop {
        let mut info = MEMORY_BASIC_INFORMATION::default();
        // SAFETY: `VirtualQuery` ne lit pas la mémoire visée : il décrit la
        // région qui contient `addr` dans la structure locale, de la taille
        // donnée ; une adresse hors de l'espace utilisateur rend 0.
        let n = unsafe { VirtualQuery(Some(addr as *const core::ffi::c_void), &mut info, size_of::<MEMORY_BASIC_INFORMATION>()) };
        if n == 0 || info.RegionSize == 0 {
            break;
        }
        let size = info.RegionSize as u64;
        let base = info.AllocationBase as usize;
        if info.State == MEM_COMMIT {
            let p = info.Protect;
            let guard = p.contains(PAGE_GUARD);
            let plain = windows::Win32::System::Memory::PAGE_PROTECTION_FLAGS(p.0 & 0xFF);
            if info.Type == MEM_IMAGE {
                // Seules les pages modifiables d'un module sont engagées pour ce processus.
                if [PAGE_READWRITE, PAGE_WRITECOPY, PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY].contains(&plain) {
                    m.images += size;
                    *modules.entry(base).or_default() += size;
                }
            } else if info.Type == MEM_PRIVATE {
                let e = private.entry(base).or_default();
                e.0 += size;
                e.1 |= guard;
            } else if info.Type == MEM_MAPPED {
                m.mapped += size;
            }
        }
        match (info.BaseAddress as usize).checked_add(info.RegionSize) {
            Some(next) if next > addr => addr = next,
            _ => break,
        }
    }
    let mut all_private = 0u64;
    for (bytes, stack) in private.into_values() {
        all_private += bytes;
        if stack {
            m.stacks += bytes;
            m.stack_count += 1;
        }
    }
    let (committed, allocated, count) = heaps();
    m.heaps = committed;
    m.heaps_allocated = allocated;
    m.heap_count = count;
    m.other = all_private.saturating_sub(m.stacks).saturating_sub(m.heaps);
    m.by_module = modules.into_iter().map(|(base, bytes)| (module_name(base), bytes)).collect();
    m.by_module.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    m
}

/// Tas du processus : (octets engagés, octets alloués, nombre de tas).
fn heaps() -> (u64, u64, usize) {
    let mut handles = [HANDLE::default(); 256];
    // SAFETY: le tableau local reçoit au plus sa longueur de handles ; la
    // valeur rendue est le nombre de tas existants (peut dépasser le tableau).
    let n = (unsafe { GetProcessHeaps(&mut handles) } as usize).min(handles.len());
    let (mut committed, mut allocated) = (0u64, 0u64);
    for h in &handles[..n] {
        let mut s = HEAP_SUMMARY { cb: size_of::<HEAP_SUMMARY>() as u32, ..Default::default() };
        // SAFETY: `h` vient de `GetProcessHeaps` à l'instant ; la structure
        // locale a la taille annoncée. Un tas détruit entre-temps par un
        // autre fil (cas rare : relevé de diagnostic seulement) ferait
        // échouer l'appel, dont le résultat est alors ignoré.
        if unsafe { HeapSummary(*h, 0, &mut s) }.as_bool() {
            committed += s.cbCommitted as u64;
            allocated += s.cbAllocated as u64;
        }
    }
    (committed, allocated, n)
}

/// Nom de fichier (minuscules, sans dossier) du module chargé à `base`.
fn module_name(base: usize) -> String {
    let mut buf = [0u16; 512];
    // SAFETY: `base` est l'adresse de base d'une image chargée, relevée par
    // `VirtualQuery` : c'est la valeur d'un HMODULE. La fonction écrit au plus
    // la longueur du tampon local et rend le nombre d'unités écrites (0 si le
    // module a été déchargé entre-temps).
    let n = unsafe { GetModuleFileNameW(Some(HMODULE(base as *mut core::ffi::c_void)), &mut buf) } as usize;
    let path = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
    let file = path.rsplit(['\\', '/']).next().unwrap_or_default().to_lowercase();
    if file.is_empty() { "module_inconnu".to_string() } else { file }
}

/// Lignes `mem.*` du journal de performance, écrites par le fil des relevés
/// (`perf::threads`) : valeurs en Mo dans la colonne `ms`.
///
/// - `mem.privee`, `mem.travail`, `mem.travail_privee` : à chaque relevé où
///   l'une a bougé d'au moins [`STEP`] ;
/// - `mem.carte.*` : la répartition de [`map`] (`modules`, `module.<nom>` pour
///   les plus lourds, `piles`, `tas`, `tas_alloue`, `autre`), 10 s, 30 s et
///   60 s après le début, puis toutes les cinq minutes : assez pour voir le
///   démarrage, le repos, et une croissance lente.
#[derive(Default)]
pub struct Journal {
    last: Usage,
    maps: usize,
}

/// Variation (octets) en dessous de laquelle les compteurs ne sont pas réécrits.
const STEP: u64 = 1 << 20;
/// Instants des premières cartes, puis période des suivantes.
const MAPS_AT: [Duration; 3] = [Duration::from_secs(10), Duration::from_secs(30), Duration::from_secs(60)];
const MAP_EVERY: Duration = Duration::from_secs(300);
/// Modules nommés dans une carte, au plus, et taille minimale pour l'être.
const MAP_MODULES: usize = 6;

fn mo(bytes: u64) -> f64 {
    bytes as f64 / 1_048_576.0
}

impl Journal {
    /// `since` : temps écoulé depuis le début des relevés.
    pub fn sample(&mut self, rec: &super::Recorder, since: Duration) {
        if let Some(u) = usage()
            && (u.private.abs_diff(self.last.private) >= STEP || u.private_working.abs_diff(self.last.private_working) >= STEP || u.working.abs_diff(self.last.working) >= STEP)
        {
            self.last = u;
            rec.record_key(Cow::Borrowed("mem.privee"), mo(u.private));
            rec.record_key(Cow::Borrowed("mem.travail"), mo(u.working));
            rec.record_key(Cow::Borrowed("mem.travail_privee"), mo(u.private_working));
        }
        let due = match MAPS_AT.get(self.maps) {
            Some(at) => *at,
            None => MAPS_AT[MAPS_AT.len() - 1] + MAP_EVERY * (self.maps - MAPS_AT.len() + 1) as u32,
        };
        if since >= due {
            self.maps += 1;
            let m = map();
            for (key, bytes) in [("mem.carte.modules", m.images), ("mem.carte.piles", m.stacks), ("mem.carte.tas", m.heaps), ("mem.carte.tas_alloue", m.heaps_allocated), ("mem.carte.autre", m.other)] {
                rec.record_key(Cow::Borrowed(key), mo(bytes));
            }
            for (name, bytes) in m.by_module.iter().take(MAP_MODULES).filter(|(_, b)| *b >= STEP) {
                rec.record_key(Cow::Owned(format!("mem.carte.module.{}", super::wakes::clean(name))), mo(*bytes));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_is_plausible() {
        let u = usage().expect("compteurs du processus");
        assert!(u.private > 1 << 20 && u.working > 1 << 20, "{u:?}");
        assert!(u.private_working <= u.working && u.working <= u.peak_working, "{u:?}");
    }

    #[test]
    fn map_accounts_for_the_private_memory() {
        // Un gros bloc vivant : il doit se retrouver dans les tas (ou, au-delà
        // du seuil des grands blocs, dans une réservation du tas comptée de même).
        let block = vec![1u8; 8 << 20];
        let (m, u) = (map(), usage().expect("compteurs du processus"));
        assert!(m.heaps_allocated <= m.heaps, "{m:?}");
        assert!(m.heaps + m.other >= block.len() as u64, "{m:?}");
        assert!(m.stack_count >= 1 && m.stacks > 0, "{m:?}");
        assert!(m.by_module.iter().all(|(name, _)| !name.contains('\\') && !name.contains('/')), "{m:?}");
        // La carte et le compteur de Windows disent la même chose, à peu près
        // (d'autres fils allouent entre les deux relevés).
        let (a, b) = (m.total() as f64, u.private as f64);
        assert!((a - b).abs() < b * 0.25 + (4 << 20) as f64, "carte {a} contre compteur {b} : {m:?}");
    }
}
