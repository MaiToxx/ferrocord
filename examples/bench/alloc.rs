//! Allocateur compteur de l'aperçu (lot « ram ») : l'allocateur du système,
//! plus quatre compteurs atomiques. Il n'existe que dans l'exemple `preview` :
//! l'exécutable livré garde l'allocateur du système tel quel.
//!
//! Il sert à séparer deux choses que la mémoire privée du processus confond :
//! les octets VIVANTS (ce que le programme tient vraiment dans le tas) et tout
//! le reste (pilote graphique, piles, tas fragmenté après un pic). Les relevés
//! du banc lisent [`live`] entre deux étapes du démarrage : la différence est
//! ce que l'étape a gardé ; [`peak`] donne la pointe atteinte entre-temps.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

pub struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static BLOCKS: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static TOTAL: AtomicUsize = AtomicUsize::new(0);

fn add(size: usize) {
    let now = LIVE.fetch_add(size, Relaxed) + size;
    PEAK.fetch_max(now, Relaxed);
    BLOCKS.fetch_add(1, Relaxed);
    TOTAL.fetch_add(1, Relaxed);
}

fn sub(size: usize) {
    LIVE.fetch_sub(size, Relaxed);
    BLOCKS.fetch_sub(1, Relaxed);
}

// SAFETY: chaque méthode délègue à `System` avec les mêmes arguments ; les
// compteurs ne touchent pas à la mémoire rendue.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: contrat de `GlobalAlloc::alloc`, transmis tel quel.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            add(layout.size());
            note(p, layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: contrat de `GlobalAlloc::alloc_zeroed`, transmis tel quel.
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            add(layout.size());
            note(p, layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        sub(layout.size());
        forget(ptr, layout.size());
        // SAFETY: contrat de `GlobalAlloc::dealloc`, transmis tel quel.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        forget(ptr, layout.size());
        // SAFETY: contrat de `GlobalAlloc::realloc`, transmis tel quel.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            sub(layout.size());
            add(new_size);
            note(p, new_size);
        }
        p
    }
}

/// Relevé du tas : octets vivants, blocs vivants, allocations faites depuis le début.
#[derive(Clone, Copy, Debug, Default)]
pub struct Heap {
    pub bytes: usize,
    pub blocks: usize,
    pub total: usize,
}

pub fn live() -> Heap {
    Heap { bytes: LIVE.load(Relaxed), blocks: BLOCKS.load(Relaxed), total: TOTAL.load(Relaxed) }
}

/// Pointe des octets vivants depuis le dernier [`reset_peak`].
pub fn peak() -> usize {
    PEAK.load(Relaxed)
}

/// La pointe repart du niveau actuel (début d'une étape).
pub fn reset_peak() {
    PEAK.store(LIVE.load(Relaxed), Relaxed);
}

// ---------------------------------------------------------------- gros blocs
//
// Avec `FERROCORD_BANC_TRACE=<octets>`, chaque bloc d'au moins cette taille est
// noté avec la pile d'appels qui l'a demandé ; `survivors` rend ceux qui
// vivent encore, regroupés par pile. Sert à nommer ce qui tient la mémoire
// (il faut un exécutable avec ses symboles). Sans la variable : un test d'entier.

use std::cell::Cell;
use std::sync::Mutex;

static TRACE_FROM: AtomicUsize = AtomicUsize::new(0);
/// Blocs notés, par adresse : taille, pile d'appels, époque.
type Traced = std::collections::HashMap<usize, (usize, std::backtrace::Backtrace, usize)>;
static TRACED: Mutex<Option<Traced>> = Mutex::new(None);
/// Époque des blocs notés : [`mark`] en ouvre une nouvelle, [`survivors_since`]
/// ne rend que les blocs demandés depuis (ce qu'un tour de plus a laissé).
static EPOCH: AtomicUsize = AtomicUsize::new(0);

/// Ouvre une nouvelle époque et la rend.
pub fn mark() -> usize {
    EPOCH.fetch_add(1, Relaxed) + 1
}

thread_local! {
    /// Vrai pendant qu'une pile est relevée : ses propres allocations ne sont pas notées.
    static BUSY: Cell<bool> = const { Cell::new(false) };
}

/// Lit `FERROCORD_BANC_TRACE` (à appeler une fois, au début).
pub fn trace_from_env() {
    if let Some(n) = std::env::var("FERROCORD_BANC_TRACE").ok().and_then(|v| v.parse::<usize>().ok()) {
        TRACE_FROM.store(n.max(1), Relaxed);
    }
}

fn traced(size: usize) -> bool {
    let from = TRACE_FROM.load(Relaxed);
    from != 0 && size >= from
}

// ---------------------------------------------------------------- profil des allocations
//
// Pendant une phase du banc (`FERROCORD_BANC_PROFIL=<phase>`), une allocation
// sur `PROFILE_EVERY` est notée avec sa pile d'appels ; `profile_report` les
// regroupe par site. C'est le profil du travail refait à chaque image
// (chaînes clonées, listes reconstruites) : il faut un exécutable avec ses
// symboles (profil `dev`), et les comptes par site ne dépendent pas de
// l'optimisation.

static PROFILING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static PROFILE_SEEN: AtomicUsize = AtomicUsize::new(0);
static PROFILED: Mutex<Vec<(usize, std::backtrace::Backtrace)>> = Mutex::new(Vec::new());
const PROFILE_EVERY: usize = 16;

/// Ouvre ou ferme le relevé des allocations par site.
pub fn profile(on: bool) {
    PROFILING.store(on, Relaxed);
}

fn profile_note(size: usize) {
    if !PROFILING.load(Relaxed) || !PROFILE_SEEN.fetch_add(1, Relaxed).is_multiple_of(PROFILE_EVERY) || BUSY.try_with(|b| b.replace(true)).unwrap_or(true) {
        return;
    }
    let trace = std::backtrace::Backtrace::force_capture();
    if let Ok(mut p) = PROFILED.lock() {
        p.push((size, trace));
    }
    let _ = BUSY.try_with(|b| b.set(false));
}

/// Sites d'allocation relevés : (allocations estimées, octets estimés, pile
/// réduite aux fonctions du programme), du plus fréquent au moins fréquent.
pub fn profile_report() -> Vec<(usize, usize, String)> {
    let _ = BUSY.try_with(|b| b.set(true));
    let mut groups: std::collections::HashMap<String, (usize, usize)> = std::collections::HashMap::new();
    if let Ok(p) = PROFILED.lock() {
        for (size, trace) in p.iter() {
            let text = trace.to_string();
            let frames: Vec<&str> = text
                .lines()
                .filter_map(|l| l.trim().split_once(": ").map(|(_, f)| f))
                .filter(|f| !f.contains("backtrace") && !f.starts_with("alloc::") && !f.contains("bench::alloc") && !f.contains("__rust") && !f.contains("__rdl") && !f.starts_with("core::") && !f.starts_with("std::") && !f.starts_with("hashbrown::"))
                .take(6)
                .collect();
            let e = groups.entry(frames.join(" < ")).or_default();
            e.0 += PROFILE_EVERY;
            e.1 += *size * PROFILE_EVERY;
        }
    }
    let mut v: Vec<(usize, usize, String)> = groups.into_iter().map(|(k, (n, b))| (n, b, k)).collect();
    v.sort_by_key(|a| std::cmp::Reverse(a.0));
    let _ = BUSY.try_with(|b| b.set(false));
    v
}

fn note(ptr: *mut u8, size: usize) {
    profile_note(size);
    if !traced(size) || BUSY.try_with(|b| b.replace(true)).unwrap_or(true) {
        return;
    }
    let trace = std::backtrace::Backtrace::force_capture();
    if let Ok(mut t) = TRACED.lock() {
        t.get_or_insert_with(Default::default).insert(ptr as usize, (size, trace, EPOCH.load(Relaxed)));
    }
    let _ = BUSY.try_with(|b| b.set(false));
}

fn forget(ptr: *mut u8, size: usize) {
    if !traced(size) || BUSY.try_with(|b| b.replace(true)).unwrap_or(true) {
        return;
    }
    if let Ok(mut t) = TRACED.lock()
        && let Some(map) = t.as_mut()
    {
        drop(map.remove(&(ptr as usize)));
    }
    let _ = BUSY.try_with(|b| b.set(false));
}

/// Gros blocs encore vivants, regroupés par pile : (octets, nombre, pile
/// réduite aux fonctions du programme et de ses bibliothèques), du plus lourd au plus léger.
pub fn survivors() -> Vec<(usize, usize, String)> {
    survivors_since(0)
}

/// Comme [`survivors`], pour les seuls blocs demandés depuis l'époque `epoch`.
pub fn survivors_since(epoch: usize) -> Vec<(usize, usize, String)> {
    let _ = BUSY.try_with(|b| b.set(true));
    let mut groups: std::collections::HashMap<String, (usize, usize)> = std::collections::HashMap::new();
    if let Ok(t) = TRACED.lock() {
        for (size, trace, _) in t.iter().flat_map(|m| m.values()).filter(|v| v.2 >= epoch) {
            let text = trace.to_string();
            // Une ligne sur deux nomme une fonction ; les cadres de l'allocateur et du relevé sont sautés.
            let frames: Vec<&str> = text
                .lines()
                .filter_map(|l| l.trim().split_once(": ").map(|(_, f)| f))
                .filter(|f| !f.contains("backtrace") && !f.contains("alloc::") && !f.contains("bench::alloc") && !f.contains("__rust") && !f.contains("__rdl"))
                .take(14)
                .collect();
            let e = groups.entry(frames.join(" < ")).or_default();
            e.0 += *size;
            e.1 += 1;
        }
    }
    let mut v: Vec<(usize, usize, String)> = groups.into_iter().map(|(k, (b, n))| (b, n, k)).collect();
    v.sort_by_key(|a| std::cmp::Reverse(a.0));
    let _ = BUSY.try_with(|b| b.set(false));
    v
}
