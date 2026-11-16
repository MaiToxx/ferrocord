//! Journal de performance facultatif, pour comparer Ferrocord au client
//! officiel (`tools/mesures.ps1`, `docs/mesures.md`).
//!
//! Activé seulement si la variable d'environnement `FERROCORD_PERF` vaut `1`
//! au démarrage. Désactivé (par défaut), rien n'est créé : l'application
//! tourne sans l'enveloppe [`Traced`], et la seule dépense est la lecture de
//! la variable au lancement.
//!
//! Activé, chaque image et chaque changement de salon donnent une ligne
//! `t_ms,type,ms` dans `%LOCALAPPDATA%\ferrocord\perf.csv` (ou dans le
//! fichier nommé par `FERROCORD_PERF_FICHIER`). Le fichier ne contient que
//! des durées et des types fixes : aucun nom, identifiant, texte ni jeton.
//! L'écriture se fait dans un fil à part : le fil d'interface ne touche
//! jamais le disque.
//!
//! Lignes en plus des [`Kind`] (lot C8) : `etape.<nom>` (étapes du
//! démarrage, en ms depuis le début de `run`), `cause.<fichier>_<ligne>`
//! (code qui a demandé l'image, d'après egui), `cause.entree` (image due à
//! une entrée : souris, clavier, fenêtre) et `passerelle.<reveil|differe|ignore>.<paquet>`
//! (ce que la passerelle a fait de chaque événement, voir [`wakes`]) ; leur
//! colonne `ms` vaut 1 (un compte).
//!
//! Lot C9 : `entree.<sorte>` (sorte d'entrée de l'image : `souris`, `clic`,
//! `molette`, `clavier`, `fenetre`, `autre` ; un compte), `fil.<nom>` (temps
//! processeur de chaque fil depuis le relevé précédent, en ms, voir
//! [`threads`]) et `premier_salon` (image où le premier salon s'affiche au
//! démarrage : ce n'est pas un changement de salon, caches froids).
//!
//! Lot « ram » (Windows) : `mem.etape.<nom>` (mémoire privée à chaque étape du
//! démarrage), `mem.privee`, `mem.travail`, `mem.travail_privee` (compteurs du
//! processus, quand ils bougent d'au moins 1 Mo) et `mem.carte.*` (à qui
//! appartient la mémoire privée : modules, piles, tas engagé et alloué, reste ;
//! voir [`memory`]). Pour ces lignes, la colonne `ms` porte des Mo.

#[cfg(windows)]
pub mod memory;
#[cfg(windows)]
pub mod process;
pub mod threads;
pub mod wakes;

use crate::ui::App;
use eframe::egui;
use std::{
    borrow::Cow,
    fs::File,
    io::{BufWriter, Write},
    path::PathBuf,
    sync::mpsc,
    thread::JoinHandle,
    time::Instant,
};

/// Variable qui active le journal (`1`).
pub const VAR: &str = "FERROCORD_PERF";
/// Variable facultative : chemin du fichier écrit.
pub const FILE_VAR: &str = "FERROCORD_PERF_FICHIER";

/// En-tête du fichier ; les types possibles sont ceux de [`Kind`].
pub const HEADER: &str = "t_ms,type,ms";

/// Types de lignes du journal (seul texte écrit avec les nombres).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Du début de `run` à la fin de la première image construite.
    FirstFrame,
    /// Construction d'une image par l'application (`logic` + `ui`).
    Frame,
    /// Temps qu'eframe mesure pour l'image précédente (entrées, mise en
    /// page, tessellation, envoi des commandes OpenGL ; sans la synchro verticale).
    FrameCpu,
    /// Image où le salon ouvert a changé (clic, raccourci, saut).
    Switch,
    /// Image où le premier salon s'affiche (démarrage, historique hors ligne
    /// ou page reçue) : polices, mises en page et textures encore froides.
    FirstChannel,
}

impl Kind {
    pub const ALL: [Kind; 5] = [Kind::FirstFrame, Kind::Frame, Kind::FrameCpu, Kind::Switch, Kind::FirstChannel];

    pub fn key(self) -> &'static str {
        match self {
            Kind::FirstFrame => "premiere_image",
            Kind::Frame => "image",
            Kind::FrameCpu => "image_eframe",
            Kind::Switch => "changement_de_salon",
            Kind::FirstChannel => "premier_salon",
        }
    }
}

/// La variable vaut-elle « activé » ?
pub fn enabled(value: Option<&str>) -> bool {
    matches!(value.map(str::trim), Some("1"))
}

/// Fichier par défaut : `<cache>/ferrocord/perf.csv` (`%LOCALAPPDATA%` sous Windows).
pub fn default_path() -> Option<PathBuf> {
    Some(crate::platform::cache_dir()?.join("ferrocord").join("perf.csv"))
}

/// Journal selon l'environnement du processus ; `None` s'il est désactivé.
pub fn from_env(started: Instant) -> Option<Recorder> {
    let value = std::env::var(VAR).ok();
    from_values(value.as_deref(), std::env::var_os(FILE_VAR).map(PathBuf::from), started)
}

/// Journal selon les valeurs données (testable sans toucher à l'environnement).
pub fn from_values(value: Option<&str>, file: Option<PathBuf>, started: Instant) -> Option<Recorder> {
    if !enabled(value) {
        return None;
    }
    let path = file.or_else(default_path)?;
    match Recorder::start(path, started) {
        Ok(r) => Some(r),
        Err(e) => {
            eprintln!("{VAR} : journal impossible à créer ({e})");
            None
        }
    }
}

/// Écrit les mesures dans un fil à part ; le fichier est complet quand le
/// journal est rendu (`Drop` attend la fin de l'écriture).
pub struct Recorder {
    tx: Option<mpsc::Sender<(f64, Cow<'static, str>, f64)>>,
    writer: Option<JoinHandle<()>>,
    started: Instant,
}

impl Recorder {
    pub fn start(path: PathBuf, started: Instant) -> std::io::Result<Recorder> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut out = BufWriter::new(File::create(&path)?);
        writeln!(out, "{HEADER}")?;
        out.flush()?;
        let (tx, rx) = mpsc::channel::<(f64, Cow<'static, str>, f64)>();
        let writer = std::thread::Builder::new().name("ferrocord-perf".into()).spawn(move || {
            // Une rafale à la fois, puis vidée sur le disque : le fichier
            // reste lisible pendant que l'application tourne.
            while let Ok(first) = rx.recv() {
                let mut ok = line(&mut out, first);
                while let Ok(next) = rx.try_recv() {
                    ok = ok && line(&mut out, next);
                }
                if !ok || out.flush().is_err() {
                    return;
                }
            }
        })?;
        Ok(Recorder { tx: Some(tx), writer: Some(writer), started })
    }

    pub fn record(&self, kind: Kind, ms: f64) {
        self.record_key(Cow::Borrowed(kind.key()), ms);
    }

    /// Ligne d'un type nommé à la volée (`etape.*`, `cause.*`, `passerelle.*`) :
    /// le nom doit déjà être sûr (voir [`wakes::clean`]).
    pub fn record_key(&self, key: Cow<'static, str>, ms: f64) {
        if let Some(tx) = &self.tx {
            let _ = tx.send((self.started.elapsed().as_secs_f64() * 1000.0, key, ms));
        }
    }

    /// Second envoyeur vers le même fichier (fil des relevés par fil) ; il
    /// doit disparaître avant l'original, dont la fin de vie attend l'écriture.
    fn share(&self) -> Recorder {
        Recorder { tx: self.tx.clone(), writer: None, started: self.started }
    }

    /// Étape du démarrage atteinte maintenant (ms depuis le début de `run`).
    pub fn stage(&self, name: &'static str) {
        self.record_key(Cow::Owned(format!("etape.{name}")), self.started.elapsed().as_secs_f64() * 1000.0);
        // Mémoire privée à cette étape, en Mo (lot « ram ») : ce que chaque étape du démarrage engage.
        #[cfg(windows)]
        if let Some(u) = memory::usage() {
            self.record_key(Cow::Owned(format!("mem.etape.{name}")), u.private as f64 / 1_048_576.0);
        }
    }
}

fn line(out: &mut BufWriter<File>, (t, key, ms): (f64, Cow<'static, str>, f64)) -> bool {
    writeln!(out, "{t:.1},{key},{ms:.3}").is_ok()
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.tx = None;
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
    }
}

/// L'application, mesurée image par image. Utilisée seulement si le journal
/// est activé : sinon `run` donne l'`App` nue à eframe.
pub struct Traced {
    app: App,
    /// Déclaré avant `rec` : arrêté (et son envoyeur rendu) avant lui.
    _threads: Option<threads::Sampler>,
    rec: Recorder,
    frame_start: Option<Instant>,
    channel_before: Option<u64>,
    /// Un salon a déjà été affiché : les suivants sont des changements.
    channel_seen: bool,
    first_done: bool,
    /// Le moment où la fenêtre est devenue visible (après la première image peinte) est noté.
    shown_noted: bool,
    /// L'historique hors ligne affiché (lot H) est noté, une fois.
    offline_noted: bool,
}

impl Traced {
    pub fn new(app: App, rec: Recorder) -> Traced {
        wakes::enable();
        let threads = threads::Sampler::start(rec.share());
        Traced { app, _threads: threads, rec, frame_start: None, channel_before: None, channel_seen: false, first_done: false, shown_noted: false, offline_noted: false }
    }

    /// Ce qui a demandé cette image : code (fichier et ligne, d'après egui),
    /// entrée du système, et ce que la passerelle a décidé depuis l'image précédente.
    fn causes(&self, ctx: &egui::Context) {
        for c in ctx.repaint_causes() {
            self.rec.record_key(Cow::Owned(wakes::cause_key(c.file, c.line)), 1.0);
        }
        let kinds = ctx.input(|i| input_kinds(&i.raw.events));
        if kinds != 0 {
            self.rec.record_key(Cow::Borrowed("cause.entree"), 1.0);
            for (bit, name) in INPUT_KINDS.iter().enumerate() {
                if kinds & (1 << bit) != 0 {
                    self.rec.record_key(Cow::Borrowed(name), 1.0);
                }
            }
        }
        for n in wakes::take() {
            self.rec.record_key(Cow::Owned(n), 1.0);
        }
    }
}

/// Sortes d'entrée notées (`entree.*`), dans l'ordre des bits de [`input_kinds`].
const INPUT_KINDS: [&str; 6] = ["entree.souris", "entree.clic", "entree.molette", "entree.clavier", "entree.fenetre", "entree.autre"];

/// Sortes d'entrée présentes dans `events` (un bit par entrée de [`INPUT_KINDS`]).
fn input_kinds(events: &[egui::Event]) -> u8 {
    events.iter().fold(0, |acc, e| {
        acc | match e {
            egui::Event::PointerMoved(_) | egui::Event::MouseMoved(_) | egui::Event::PointerGone => 1,
            egui::Event::PointerButton { .. } | egui::Event::Touch { .. } => 2,
            egui::Event::MouseWheel { .. } | egui::Event::Zoom(_) | egui::Event::Rotate(_) => 4,
            egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::Ime(_) | egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_) => 8,
            egui::Event::WindowFocused(_) => 16,
            _ => 32,
        }
    })
}

impl eframe::App for Traced {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.first_done
            && let Some(cpu) = frame.info().cpu_usage
        {
            self.rec.record(Kind::FrameCpu, f64::from(cpu) * 1000.0);
        }
        // eframe montre la fenêtre juste après avoir peint la première image.
        if self.first_done && !self.shown_noted {
            self.shown_noted = true;
            self.rec.stage("fenetre_visible");
        }
        self.causes(ctx);
        self.frame_start = Some(Instant::now());
        self.channel_before = self.app.store().channel;
        eframe::App::logic(&mut self.app, ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        eframe::App::ui(&mut self.app, ui, frame);
        let Some(start) = self.frame_start.take() else { return };
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        self.rec.record(Kind::Frame, ms);
        let now = self.app.store().channel;
        if now.is_some() && now != self.channel_before {
            self.rec.record(if self.channel_seen { Kind::Switch } else { Kind::FirstChannel }, ms);
            self.channel_seen = true;
        }
        if !self.first_done {
            self.first_done = true;
            self.rec.record(Kind::FirstFrame, self.rec.started.elapsed().as_secs_f64() * 1000.0);
        }
        // Premier contenu tiré de l'historique hors ligne, dessiné à cette image (lot H).
        if !self.offline_noted && self.app.offline_shown_after().is_some() {
            self.offline_noted = true;
            self.rec.stage("historique_affiche");
        }
    }

    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        eframe::App::on_exit(&mut self.app, gl);
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        eframe::App::clear_color(&self.app, visuals)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ferrocord-perf-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn desactive_par_defaut_aucune_ecriture() {
        let d = dir("off");
        let file = d.join("perf.csv");
        for v in [None, Some(""), Some("0"), Some("non"), Some("2")] {
            assert!(!enabled(v));
            assert!(from_values(v, Some(file.clone()), Instant::now()).is_none());
        }
        assert!(!d.exists(), "désactivé, rien ne doit être créé");
    }

    #[test]
    fn active_ecrit_seulement_des_durees() {
        let d = dir("on");
        let file = d.join("sous").join("perf.csv");
        let rec = from_values(Some("1"), Some(file.clone()), Instant::now());
        let Some(rec) = rec else { panic!("journal non créé") };
        rec.record(Kind::Frame, 1.25);
        rec.record(Kind::FrameCpu, 2.5);
        rec.record(Kind::Switch, 3.0);
        rec.record(Kind::FirstFrame, 120.0);
        // Lot C8 : étapes, causes des images et décisions de la passerelle.
        rec.stage("fenetre_et_opengl");
        rec.record_key(Cow::Owned(wakes::cause_key("C:/Users/x/src/ui/mod.rs", 387)), 1.0);
        rec.record_key(Cow::Owned(format!("passerelle.differe.{}", wakes::clean("MESSAGE_CREATE"))), 1.0);
        drop(rec);
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some(HEADER));
        let keys: Vec<&str> = Kind::ALL.iter().map(|k| k.key()).collect();
        let mut n = 0;
        for l in lines {
            let f: Vec<&str> = l.split(',').collect();
            assert_eq!(f.len(), 3, "{l}");
            assert!(f[0].parse::<f64>().is_ok(), "{l}");
            assert!(keys.contains(&f[1]) || ["etape.", "cause.", "passerelle.", "entree.", "fil.", "mem."].iter().any(|p| f[1].starts_with(p)), "{l}");
            assert!(f[2].parse::<f64>().is_ok(), "{l}");
            n += 1;
        }
        // Sous Windows, l'étape note aussi la mémoire privée (`mem.etape.*`, lot « ram »).
        assert_eq!(n, if cfg!(windows) { 8 } else { 7 });
        assert!(text.contains("changement_de_salon,3.000"));
        assert!(text.contains(",cause.ui_mod.rs_387,1.000") && text.contains(",passerelle.differe.message_create,1.000") && text.contains(",etape.fenetre_et_opengl,"));
        // Rien d'autre que des chiffres, des types fixes et la ponctuation.
        assert!(text.chars().all(|c| c.is_ascii_digit() || c.is_ascii_lowercase() || ",._\n".contains(c)));
        let _ = std::fs::remove_dir_all(&d);
    }
}
