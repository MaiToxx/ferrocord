//! Rapport de plantage local (lot E5, plugin `rapport_plantage`).
//!
//! Quand l'application s'arrête sur une panique ou une erreur fatale, un
//! fichier texte lisible est écrit dans `%LOCALAPPDATA%\ferrocord\plantages\` :
//! date, version, endroit, message, pile d'appels, et les dernières lignes
//! d'un petit journal circulaire d'événements techniques. Au lancement
//! suivant, l'interface propose de l'ouvrir (`ui/crash_banner.rs`).
//!
//! **Rien ne part sur le réseau** : ce module n'ouvre aucune connexion.
//!
//! Vie privée, par construction puis par filtre :
//! - le journal circulaire ne reçoit que des noms d'événements écrits dans le
//!   code (`&'static str`) et un nombre : aucun texte venu de Discord ou de
//!   l'utilisateur ne peut y entrer ;
//! - le message de la panique, seul texte libre, passe par [`scrub`] : ce
//!   qui est entre guillemets ou accents graves est retiré (c'est là que Rust
//!   recopie les chaînes fautives), les longues suites sans espace (jetons) et
//!   les longs nombres (identifiants) sont masqués, comme le dossier et le nom
//!   de l'utilisateur et les mots déclarés privés ([`set_private`]) ; le
//!   message est enfin borné ;
//! - la pile d'appels ne contient que des noms de fonctions et de fichiers du
//!   programme ; le dossier de l'utilisateur et les longs nombres y sont masqués.
//!
//! Plugin coupé : [`note`] rend la main après une lecture atomique, et la
//! panique n'écrit rien.

use crate::plugins::{self, Id};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Rapports gardés : les plus anciens sont effacés.
pub const KEEP: usize = 10;
/// Événements techniques gardés en mémoire.
pub const EVENTS: usize = 48;
/// Longueur maximale du message gardé, en caractères.
pub const MESSAGE_MAX: usize = 600;
/// Taille maximale de la pile d'appels écrite, en octets.
const TRACE_MAX: usize = 24 << 10;
/// Fichier témoin : nom du rapport pas encore proposé à l'utilisateur.
const MARKER: &str = "nouveau";
const PREFIX: &str = "plantage-";
const MASK: &str = "<masqué>";

/// Un événement technique : nom écrit dans le code, nombre, instant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub what: &'static str,
    pub n: i64,
    /// Millisecondes depuis le lancement.
    pub at_ms: u64,
}

static RING: Mutex<VecDeque<Event>> = Mutex::new(VecDeque::new());
static STARTED: OnceLock<Instant> = OnceLock::new();
static DIR: OnceLock<PathBuf> = OnceLock::new();
static PRIVATE: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Dossier des rapports : `%LOCALAPPDATA%\ferrocord\plantages`.
pub fn default_dir() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    return None;
    #[cfg(not(target_os = "android"))]
    Some(dirs::data_local_dir()?.join("ferrocord").join("plantages"))
}

/// À appeler une fois au lancement : les rapports iront dans `dir`.
/// Le dossier n'est créé qu'au premier rapport.
pub fn install(dir: Option<PathBuf>) {
    STARTED.get_or_init(Instant::now);
    if let Some(d) = dir {
        let _ = DIR.set(d);
    }
}

/// Note un événement technique dans le journal circulaire. `what` est un nom
/// écrit dans le code : jamais un texte reçu.
pub fn note(what: &'static str) {
    note_n(what, 0);
}

/// Comme [`note`], avec un nombre (un compte, un code, une durée : jamais un identifiant).
pub fn note_n(what: &'static str, n: i64) {
    if !plugins::on(Id::CrashReport) {
        return;
    }
    let at_ms = STARTED.get_or_init(Instant::now).elapsed().as_millis() as u64;
    // Verrou tenu par un fil qui plante : l'événement est perdu, rien d'autre.
    if let Ok(mut ring) = RING.try_lock() {
        if ring.len() >= EVENTS {
            ring.pop_front();
        }
        ring.push_back(Event { what, n, at_ms });
    }
}

/// Mots à ne jamais écrire dans un rapport (nom et identifiant du compte
/// ouvert). Remplace la liste précédente.
pub fn set_private(words: Vec<String>) {
    if let Ok(mut p) = PRIVATE.lock() {
        *p = words.into_iter().filter(|w| w.chars().count() >= 3).collect();
    }
}

/// Dossier et nom de l'utilisateur de Windows, à masquer dans les chemins.
fn system_private() -> Vec<String> {
    let mut out = Vec::new();
    for var in ["USERPROFILE", "HOME", "USERNAME", "USER"] {
        if let Ok(v) = std::env::var(var)
            && v.chars().count() >= 3
        {
            out.push(v);
        }
    }
    out
}

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+' | '/' | '=')
}

/// Remplace chaque occurrence de `word` (sans tenir compte de la casse ASCII).
fn mask_word(text: &str, word: &str) -> String {
    let (hay, needle) = (text.to_ascii_lowercase(), word.to_ascii_lowercase());
    if needle.is_empty() || !hay.contains(&needle) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    while let Some(i) = hay[from..].find(&needle) {
        out.push_str(&text[from..from + i]);
        out.push_str(MASK);
        from += i + needle.len();
    }
    out.push_str(&text[from..]);
    out
}

/// Masque les suites de `min` chiffres ou plus (identifiants de Discord).
fn mask_digits(text: &str, min: usize) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        if run.len() >= min {
            out.push_str("<nombre>");
        } else {
            out.push_str(run);
        }
        run.clear();
    };
    for c in text.chars() {
        if c.is_ascii_digit() {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// Vide ce qui est entre deux `quote` : c'est là que Rust recopie une chaîne
/// fautive (`Err("…")`, `` of `…` ``). Un guillemet seul vide jusqu'à la fin.
fn mask_quoted(text: &str, quote: char) -> String {
    let mut out = String::with_capacity(text.len());
    let mut inside = false;
    for c in text.chars() {
        if c == quote {
            if !inside {
                out.push(c);
                out.push('…');
            } else {
                out.push(c);
            }
            inside = !inside;
        } else if !inside {
            out.push(c);
        }
    }
    out
}

/// Vide un caractère cité (`'é'`) ; une apostrophe dans une phrase est gardée.
fn mask_char_quotes(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\'' && chars.get(i + 2) == Some(&'\'') {
            out.push_str("'…'");
            i += 3;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Masque les suites de 24 caractères ou plus sans espace, faites de lettres
/// ASCII, chiffres et `_ - . + / =` : un jeton, une clé, une donnée encodée.
fn mask_long_runs(text: &str) -> String {
    const MIN: usize = 24;
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        if run.chars().count() >= MIN {
            out.push_str(MASK);
        } else {
            out.push_str(run);
        }
        run.clear();
    };
    for c in text.chars() {
        if is_token_char(c) {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// Filtre du message d'une panique ou d'une erreur fatale : rien de ce qui
/// peut être un jeton, un contenu de message, un nom ou un identifiant de
/// compte n'en sort. `private` : mots à masquer tels quels.
pub fn scrub(message: &str, private: &[String]) -> String {
    let mut s: String = message.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    // Les mots privés d'abord, tant qu'ils sont entiers ; les plus longs en premier.
    let mut words: Vec<&String> = private.iter().collect();
    words.sort_by_key(|w| std::cmp::Reverse(w.len()));
    for w in words {
        s = mask_word(&s, w);
    }
    for q in ['"', '`'] {
        s = mask_quoted(&s, q);
    }
    s = mask_char_quotes(&s);
    s = mask_long_runs(&s);
    s = mask_digits(&s, 15);
    if s.chars().count() > MESSAGE_MAX {
        s = s.chars().take(MESSAGE_MAX).chain("…".chars()).collect();
    }
    s
}

/// Filtre d'un chemin de fichier ou d'une pile d'appels : noms du programme
/// gardés, dossier de l'utilisateur et longs nombres masqués.
pub fn scrub_trace(trace: &str, private: &[String]) -> String {
    let mut s = trace.to_string();
    let mut words: Vec<&String> = private.iter().collect();
    words.sort_by_key(|w| std::cmp::Reverse(w.len()));
    for w in words {
        s = mask_word(&s, w);
    }
    s = mask_digits(&s, 15);
    if s.len() > TRACE_MAX {
        let mut cut = TRACE_MAX;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push_str("\n…");
    }
    s
}

/// Ce qu'un rapport contient, avant filtrage.
#[derive(Clone, Debug, Default)]
pub struct Report {
    /// Date locale, `AAAA-MM-JJ HH:MM:SS`.
    pub date: String,
    /// `panique` ou le nom de l'erreur fatale (écrit dans le code).
    pub kind: &'static str,
    pub thread: String,
    /// Fichier, ligne et colonne du code.
    pub location: String,
    pub message: String,
    pub trace: String,
    pub events: Vec<Event>,
    pub private: Vec<String>,
}

impl Report {
    /// Texte du fichier, filtré.
    pub fn text(&self) -> String {
        let private = &self.private;
        let mut out = String::new();
        out.push_str("Rapport de plantage de Ferrocord / Ferrocord crash report\n");
        out.push_str("Ce fichier est resté sur cet ordinateur : rien n'a été envoyé. Il ne contient ni jeton, ni message, ni nom de compte.\n");
        out.push_str("This file stayed on this computer: nothing was sent. It holds no token, no message, no account name.\n\n");
        out.push_str(&format!("Date    : {}\n", scrub_trace(&self.date, &[])));
        out.push_str(&format!("Version : {}\n", env!("CARGO_PKG_VERSION")));
        out.push_str(&format!("Système : {} {}\n", std::env::consts::OS, std::env::consts::ARCH));
        out.push_str(&format!("Genre   : {}\n", self.kind));
        out.push_str(&format!("Fil     : {}\n", scrub(&self.thread, private)));
        out.push_str(&format!("Endroit : {}\n", scrub_trace(&self.location, private)));
        out.push_str(&format!("Message : {}\n", scrub(&self.message, private)));
        out.push_str("\nDerniers événements techniques (le plus récent en bas)\n");
        if self.events.is_empty() {
            out.push_str("  (aucun)\n");
        }
        for e in &self.events {
            out.push_str(&format!("  +{:>5}.{:03} s  {}", e.at_ms / 1000, e.at_ms % 1000, e.what));
            if e.n != 0 {
                out.push_str(&format!(" = {}", e.n));
            }
            out.push('\n');
        }
        out.push_str("\nPile d'appels\n");
        out.push_str(&scrub_trace(&self.trace, private));
        out.push('\n');
        out
    }
}

/// Noms des rapports de `dir`, du plus ancien au plus récent.
pub fn list(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir).map(|d| d.flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|n| n.starts_with(PREFIX) && n.ends_with(".txt")).collect()).unwrap_or_default();
    names.sort();
    names
}

/// Écrit le rapport dans `dir`, pose le témoin, efface les rapports au-delà
/// de [`KEEP`]. Rend le chemin du fichier.
pub fn write_report(dir: &Path, report: &Report) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp: String = report.date.chars().map(|c| if c.is_ascii_digit() { c } else { '-' }).collect();
    let mut name = format!("{PREFIX}{stamp}.txt");
    let mut n = 1;
    while dir.join(&name).exists() && n < 100 {
        n += 1;
        name = format!("{PREFIX}{stamp}-{n:02}.txt");
    }
    let path = dir.join(&name);
    std::fs::write(&path, report.text())?;
    let _ = std::fs::write(dir.join(MARKER), &name);
    let names = list(dir);
    for old in names.iter().take(names.len().saturating_sub(KEEP)) {
        let _ = std::fs::remove_file(dir.join(old));
    }
    Ok(path)
}

/// Rapport pas encore proposé à l'utilisateur (témoin posé par le plantage).
pub fn pending(dir: &Path) -> Option<PathBuf> {
    let name = std::fs::read_to_string(dir.join(MARKER)).ok()?;
    let name = name.trim();
    // Le témoin ne désigne qu'un rapport de ce dossier : jamais un chemin.
    let ok = name.starts_with(PREFIX) && name.ends_with(".txt") && name.len() < 80 && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.'));
    let path = dir.join(name);
    (ok && path.is_file()).then_some(path)
}

/// Le rapport a été proposé : le témoin est retiré (les rapports restent).
pub fn dismiss(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(MARKER));
}

fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn snapshot() -> (Vec<Event>, Vec<String>) {
    let events = RING.try_lock().map(|r| r.iter().copied().collect()).unwrap_or_default();
    let mut private = system_private();
    if let Ok(p) = PRIVATE.try_lock() {
        private.extend(p.iter().cloned());
    }
    (events, private)
}

fn file(kind: &'static str, thread: String, location: String, message: String, trace: String) {
    if !plugins::on(Id::CrashReport) {
        return;
    }
    let Some(dir) = DIR.get() else { return };
    let (events, private) = snapshot();
    // Disque plein ou dossier refusé : pas de rapport, le plantage suit son cours.
    let _ = write_report(dir, &Report { date: now(), kind, thread, location, message, trace, events, private });
}

/// Crochet de panique (appelé par `panics::install`, hors des analyseurs
/// contenus) : écrit le rapport, puis rend la main à l'arrêt habituel.
pub fn on_panic(info: &std::panic::PanicHookInfo<'_>) {
    let message = info.payload().downcast_ref::<&str>().map(|s| (*s).to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
    let location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default();
    let thread = std::thread::current().name().unwrap_or("?").to_string();
    let trace = std::backtrace::Backtrace::force_capture().to_string();
    file("panique", thread, location, message, trace);
}

/// Erreur fatale hors panique (la fenêtre ou OpenGL n'ont pas pu être créés,
/// la boucle d'événements s'est arrêtée sur une erreur). `kind` : nom écrit
/// dans le code.
pub fn fatal(kind: &'static str, detail: &str) {
    let thread = std::thread::current().name().unwrap_or("?").to_string();
    file(kind, thread, String::new(), detail.to_string(), String::new());
}

/// Ouvre un rapport dans le Bloc-notes, dans un fil à part. Seulement un
/// fichier `.txt` de `dir` : rien d'autre n'est lancé.
pub fn open_report(dir: &Path, path: &Path) {
    let inside = path.parent() == Some(dir) && path.extension().is_some_and(|e| e == "txt") && path.is_file();
    if !inside || !cfg!(windows) {
        return;
    }
    let path = path.to_path_buf();
    let _ = std::thread::Builder::new().name("ferrocord-rapport".into()).spawn(move || {
        let mut cmd = std::process::Command::new("notepad.exe");
        cmd.arg(path).env_remove("DISCORD_TOKEN");
        if let Ok(mut child) = cmd.spawn() {
            let _ = child.wait();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ferrocord-plantages-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// Jeton inventé, au format de ceux de Discord (aucun compte réel) ; écrit en morceaux pour ne pas déclencher les détecteurs de secrets.
    const TOKEN: &str = concat!("MTIzNDU2Nzg5MDEyMzQ1Njc4", ".", "GAbCdE", ".", "abcdefghijklmnopqrstuvwxyz0123456789ab");
    const USER_ID: &str = "123456789012345678";

    fn hostile() -> Report {
        Report {
            date: "2026-11-15 14:03:22".into(),
            kind: "panique",
            thread: "main".into(),
            location: r"C:\Users\Camille Fictive\.cargo\registry\src\x\lib.rs:12:5".into(),
            message: format!("called `Result::unwrap()` on an `Err` value: \"mot de passe : hunter2-secret\" pour Camille Fictive ({USER_ID}) jeton {TOKEN} ; byte index 3 is not a char boundary; it is inside 'é' of `Salut tout le monde, rendez-vous ce soir`"),
            trace: format!("0: ferrocord::ui::App::update\n   at C:\\Users\\Camille Fictive\\projets\\ferrocord\\src\\ui\\mod.rs:120\n1: salon {USER_ID}"),
            events: vec![Event { what: "fenetre.prete", n: 0, at_ms: 1234 }, Event { what: "passerelle.code", n: 4004, at_ms: 65_002 }],
            private: vec![r"C:\Users\Camille Fictive".into(), "Camille Fictive".into(), "camillef".into(), USER_ID.into()],
        }
    }

    /// Le filtre : ni jeton, ni contenu de message, ni nom, ni identifiant.
    #[test]
    fn report_holds_no_token_message_name_or_id() {
        let text = hostile().text();
        for secret in [TOKEN, "MTIzNDU2Nzg5MDEyMzQ1Njc4", "abcdefghijklmnopqrstuvwxyz0123456789ab", "GAbCdE", USER_ID, "Camille", "Fictive", "hunter2", "mot de passe", "Salut tout le monde", "rendez-vous"] {
            assert!(!text.contains(secret), "« {secret} » dans le rapport :\n{text}");
        }
        // Ce qui sert au diagnostic est resté.
        for kept in ["2026-11-15 14:03:22", env!("CARGO_PKG_VERSION"), "panique", "main", r"\.cargo\registry\src\x\lib.rs:12:5", "called `…` on an `…` value", "byte index 3 is not a char boundary", "ferrocord::ui::App::update", r"src\ui\mod.rs:120", "fenetre.prete", "passerelle.code = 4004", "+   65.002 s"] {
            assert!(text.contains(kept), "« {kept} » absent :\n{text}");
        }
    }

    #[test]
    fn scrub_masks_each_kind_of_secret() {
        let none: [String; 0] = [];
        assert_eq!(scrub("index out of bounds: the len is 3 but the index is 5", &none), "index out of bounds: the len is 3 but the index is 5");
        assert_eq!(scrub(&format!("jeton {TOKEN} refusé"), &none), "jeton <masqué> refusé");
        assert_eq!(scrub("utilisateur 123456789012345678 absent, code 50013", &none), "utilisateur <nombre> absent, code 50013");
        assert_eq!(scrub("Err(\"contenu privé\") puis `autre contenu` et 'x'", &none), "Err(\"…\") puis `…` et '…'");
        // Guillemet jamais refermé : tout ce qui suit est retiré.
        assert_eq!(scrub("valeur: \"secret sans fin", &none), "valeur: \"…");
        assert_eq!(scrub("bonjour ALICE et alice", &["Alice".to_string()]), "bonjour <masqué> et <masqué>");
        // Mot privé trop court pour être masqué sans tout abîmer : ignoré à l'inscription.
        set_private(vec!["al".into(), "Alice Fictive".into()]);
        assert_eq!(*PRIVATE.lock().unwrap(), ["Alice Fictive"]);
        set_private(Vec::new());
        // Borné, sans caractère de contrôle.
        let long = scrub(&"mot ".repeat(1000), &none);
        assert_eq!(long.chars().count(), MESSAGE_MAX + 1);
        assert_eq!(scrub("a\nb\u{1b}[31mc", &none), "a b [31mc");
        // Octets quelconques : pas de panique.
        for cut in 0..TOKEN.len() {
            let _ = scrub(&format!("é\"{}`'", &TOKEN[..cut]), &["é\"".to_string()]);
        }
    }

    /// Dix rapports gardés, le témoin désigne le dernier, « Ignorer » le retire.
    #[test]
    fn reports_are_bounded_and_announced_once() {
        let dir = temp("bornes");
        assert_eq!(pending(&dir), None);
        let mut last = PathBuf::new();
        for i in 0..KEEP + 3 {
            let mut r = hostile();
            r.date = format!("2026-11-15 14:{i:02}:00");
            last = write_report(&dir, &r).unwrap();
        }
        let names = list(&dir);
        assert_eq!(names.len(), KEEP);
        assert_eq!(names.first().map(String::as_str), Some("plantage-2026-11-15-14-03-00.txt"), "les trois plus anciens sont effacés");
        assert_eq!(pending(&dir), Some(last.clone()));
        assert!(std::fs::read_to_string(&last).unwrap().contains("Pile d'appels"));
        // Même seconde : un autre fichier, rien n'est écrasé.
        let again = write_report(&dir, &Report { date: "2026-11-15 14:12:00".into(), ..hostile() }).unwrap();
        assert_ne!(again, last);
        dismiss(&dir);
        assert_eq!(pending(&dir), None);
        assert_eq!(list(&dir).len(), KEEP, "les rapports restent");
        // Témoin trafiqué : jamais suivi hors du dossier.
        for bad in ["..\\..\\Windows\\win.ini", "plantage-../x.txt", "autre.txt", "plantage-absent.txt", ""] {
            std::fs::write(dir.join(MARKER), bad).unwrap();
            assert_eq!(pending(&dir), None, "{bad}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn journal_is_a_bounded_ring_and_free_when_off() {
        // État des plugins de ce fil : activé par défaut.
        let prefs = crate::plugins::PluginPrefs::default();
        crate::plugins::publish(prefs.bits(), &prefs);
        RING.lock().unwrap().clear();
        for i in 0..(EVENTS as i64 + 20) {
            note_n("essai.tour", i);
        }
        let (events, _) = snapshot();
        let mine: Vec<_> = events.iter().filter(|e| e.what == "essai.tour").collect();
        assert!(mine.len() <= EVENTS && events.len() <= EVENTS);
        assert_eq!(mine.last().map(|e| e.n), Some(EVENTS as i64 + 19));
        // Plugin coupé : plus rien n'est noté.
        let mut off = prefs.clone();
        off.set(Id::CrashReport, false);
        crate::plugins::publish(off.bits(), &off);
        note("essai.coupe");
        assert!(snapshot().0.iter().all(|e| e.what != "essai.coupe"));
        crate::plugins::publish(prefs.bits(), &prefs);
    }
}
