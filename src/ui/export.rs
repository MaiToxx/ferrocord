//! Export d'une conversation (plugin, lot E1) : fenêtre ouverte depuis le
//! menu d'un salon ou d'un MP, choix du format et de la période, lecture de
//! l'historique manquant, pièces jointes sur demande, écriture du fichier.
//!
//! Ce qui part chez Discord, et seulement après un clic sur « Exporter… » :
//! la requête d'historique que le client fait déjà en remontant un salon
//! (`Cmd::History`), une page à la fois, par le limiteur du réseau, avec une
//! pause entre deux pages ; puis, si la case est cochée et après la
//! confirmation qui annonce leur taille, le téléchargement des pièces
//! jointes. Les pages reçues ne rejoignent pas la liste du salon. Le
//! fichier est fabriqué par `state::export`, qui ne voit que des messages.
//!
//! Plugin coupé : pas de ligne dans le menu, [`App::export_before`] et
//! [`App::export_tick`] rendent la main à la première ligne.

use super::widgets::{self, Kind as Button};
use super::App;
use crate::i18n::table::journal::{f as jf, t as jt};
use crate::plugins::{self, Id as Plugin};
use crate::state::export::{self as ex, Collector, Format, Head, Line, Range};
use crate::state::{Cmd, Event, HISTORY_PAGE};
use crate::theme::{icons::Icon, Tokens};
use eframe::egui::{self, RichText};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Pause entre deux pages d'historique : de cette durée à deux fois et demie
/// plus, au hasard (lot « fiable »). Une page toutes les 400 ms pile, mille
/// fois de suite, est une cadence de robot ; le limiteur du réseau borne le
/// débit mais pas la régularité.
const PAGE_GAP: Duration = Duration::from_millis(if cfg!(test) { 0 } else { 600 });

/// Pause tirée au hasard avant la page suivante (600 à 1500 ms).
fn page_gap() -> Duration {
    crate::net::backoff::between(PAGE_GAP, PAGE_GAP.mul_f64(2.5), crate::net::backoff::unit())
}
/// Attente avant de redemander une page abandonnée par le réseau.
const RETRY_GAP: Duration = Duration::from_millis(if cfg!(test) { 0 } else { 1500 });
const MAX_RETRIES: u8 = 3;
const WIDTH: f32 = 460.0;

/// Un fichier à télécharger : identifiant, adresse, chemin relatif, chemin complet.
struct Download {
    id: u64,
    url: String,
    rel: String,
    abs: PathBuf,
}

enum Stage {
    Form,
    /// La boîte « Enregistrer sous » est ouverte (dans son fil).
    Picking(mpsc::Receiver<Option<PathBuf>>),
    /// `waiting` : la page antérieure à cet identifiant est demandée.
    Paging { waiting: Option<u64>, next_at: Instant, retries: u8 },
    PageFailed(String),
    ConfirmFiles { count: usize, bytes: u64 },
    Downloading { queue: VecDeque<Download>, total: usize, busy: bool, tx: mpsc::Sender<(u64, Option<String>)>, rx: mpsc::Receiver<(u64, Option<String>)> },
    Writing(mpsc::Receiver<Result<(), String>>),
    Done,
    Failed(String),
}

struct Job {
    channel: u64,
    title: String,
    place: Option<String>,
    format: Format,
    from: String,
    to: String,
    local_only: bool,
    files: bool,
    error: Option<String>,
    stage: Stage,
    path: Option<PathBuf>,
    collector: Option<Collector>,
    /// L'historique plus ancien n'a pas pu être demandé (hors ligne).
    offline: bool,
    saved_files: usize,
    failed_files: usize,
}

#[derive(Default)]
pub(in crate::ui) struct ExportUi {
    job: Option<Box<Job>>,
}

#[cfg(windows)]
fn pick(ctx: egui::Context, title: String, name: String, format: Format) -> Option<mpsc::Receiver<Option<PathBuf>>> {
    let (tx, rx) = mpsc::channel();
    // La boîte de dialogue bloque : dans son propre fil.
    std::thread::Builder::new()
        .name("ferrocord-export".into())
        .spawn(move || {
            let ext = format.extension();
            let chosen = rfd::FileDialog::new().set_title(title).set_file_name(name).add_filter(ext, &[ext]).save_file();
            let _ = tx.send(chosen);
            ctx.request_repaint();
        })
        .ok()?;
    Some(rx)
}

#[cfg(not(windows))]
fn pick(_: egui::Context, _: String, _: String, _: Format) -> Option<mpsc::Receiver<Option<PathBuf>>> {
    None
}

impl App {
    /// Ligne « Exporter la conversation… » du menu d'un salon ou d'un MP.
    pub(in crate::ui) fn export_menu(&mut self, ui: &mut egui::Ui, channel: u64) {
        if !plugins::on(Plugin::Export) {
            return;
        }
        let t = self.theme.clone();
        if super::menu::item(ui, &t, jt::EX_MENU.get(self.lang), Some(Icon::File), false).clicked() {
            self.export_open(channel);
            ui.close();
        }
    }

    /// Ouvre la fenêtre d'export du salon ; faux si le plugin est coupé.
    pub fn export_open(&mut self, channel: u64) -> bool {
        if !plugins::on(Plugin::Export) {
            return false;
        }
        let lang = self.lang;
        let (title, place) = match self.store.dms.iter().find(|d| d.id == channel) {
            Some(d) => (super::words::dm_name(lang, &d.name).to_string(), None),
            None => {
                let name = self.store.channel_name(channel).unwrap_or_default();
                let guild = self.store.guilds.iter().find(|g| g.channels.iter().any(|c| c.id == channel)).map(|g| super::words::guild_name(lang, &g.name).to_string());
                (format!("#{name}"), guild)
            }
        };
        self.export.job = Some(Box::new(Job {
            channel,
            title,
            place,
            format: Format::Html,
            from: String::new(),
            to: String::new(),
            local_only: false,
            files: false,
            error: None,
            stage: Stage::Form,
            path: None,
            collector: None,
            offline: false,
            saved_files: 0,
            failed_files: 0,
        }));
        self.ctx.request_repaint();
        true
    }

    /// Une fenêtre d'export est ouverte.
    pub fn export_active(&self) -> bool {
        self.export.job.is_some()
    }

    fn export_range(&self, job: &mut Job) -> Option<Range> {
        let offset = chrono::Local::now().offset().local_minus_utc();
        match Range::parse(&job.from, &job.to, offset) {
            Ok(r) => {
                job.error = None;
                Some(r)
            }
            Err(v) => {
                let both_read = [&job.from, &job.to].iter().all(|d| Range::parse(d, "", offset).is_ok());
                job.error = Some(if both_read { jt::EX_BAD_RANGE.get(self.lang).to_string() } else { jf::ex_bad_date(self.lang, v) });
                None
            }
        }
    }

    /// Rassemble ce qui est en mémoire, puis lance la remontée s'il en manque.
    fn export_collect(&mut self, job: &mut Job, range: Range, path: PathBuf) {
        let (lang, h24) = (self.lang, self.h24());
        let mut c = Collector::new(range, crate::state::store::now_ms());
        if let Some(cs) = self.store.chans.get(&job.channel).filter(|cs| cs.loaded) {
            for m in &cs.msgs {
                if let Some(l) = Line::of(&self.store, m, lang, h24) {
                    c.add(l);
                }
            }
            // Fenêtre qui touche le présent : tout est là jusqu'à son premier message.
            if !cs.newer
                && let Some(first) = cs.msgs.front()
            {
                c.covered_down_to(first.id, cs.more);
            }
        }
        if job.local_only {
            c.done = true;
        } else if !c.done && !self.online() {
            c.done = true;
            job.offline = true;
        }
        let done = c.done;
        job.collector = Some(c);
        job.path = Some(path);
        if done {
            self.export_pages_done(job);
        } else {
            job.stage = Stage::Paging { waiting: None, next_at: Instant::now(), retries: 0 };
        }
    }

    fn export_pages_done(&mut self, job: &mut Job) {
        let (count, bytes) = job.collector.as_ref().map_or((0, 0), Collector::files);
        if job.files && count > 0 {
            job.stage = Stage::ConfirmFiles { count, bytes };
        } else {
            self.export_write(job);
        }
    }

    /// Prépare le dossier des pièces jointes, à côté du fichier, et la file des téléchargements.
    fn export_download(&mut self, job: &mut Job) {
        let (Some(path), Some(c)) = (job.path.clone(), job.collector.as_ref()) else { return };
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "export".into());
        let dir = format!("{stem}_fichiers");
        let abs_dir = path.with_file_name(&dir);
        if let Err(e) = std::fs::create_dir_all(&abs_dir) {
            job.stage = Stage::Failed(jf::ex_write_failed(self.lang, e.kind()));
            return;
        }
        let mut queue = VecDeque::new();
        for f in c.lines().flat_map(|l| &l.files) {
            let name = format!("{}_{}", f.id, ex::safe_name(&f.name));
            queue.push_back(Download { id: f.id, url: f.url.clone(), rel: format!("{dir}/{name}"), abs: abs_dir.join(&name) });
        }
        let (tx, rx) = mpsc::channel();
        job.stage = Stage::Downloading { total: queue.len(), queue, busy: false, tx, rx };
    }

    /// Écrit le fichier, hors du fil d'interface.
    fn export_write(&mut self, job: &mut Job) {
        let (Some(path), Some(c)) = (job.path.clone(), job.collector.as_ref()) else { return };
        let head = Head { title: job.title.clone(), place: job.place.clone(), exported_ms: crate::state::store::now_ms(), range: c.range, lang: self.lang, utc_offset_s: chrono::Local::now().offset().local_minus_utc() };
        let lines: Vec<Line> = c.lines().cloned().collect();
        let format = job.format;
        let (tx, rx) = mpsc::channel();
        let ctx = self.ctx.clone();
        let work = move || {
            let content = match format {
                Format::Html => ex::html(&head, lines.iter()),
                Format::Text => ex::text(&head, lines.iter()),
            };
            let _ = tx.send(std::fs::write(&path, content).map_err(|e| e.kind().to_string()));
            ctx.request_repaint();
        };
        // Les tests écrivent sur place : pas de fil à attendre.
        if cfg!(test) {
            work();
        } else if std::thread::Builder::new().name("ferrocord-export".into()).spawn(work).is_err() {
            job.stage = Stage::Failed(jf::ex_write_failed(self.lang, "thread"));
            return;
        }
        job.stage = Stage::Writing(rx);
    }

    /// Avant que le magasin reçoive `e` : la page d'historique attendue par
    /// l'export est rangée ici. Vrai si l'événement n'est que pour l'export
    /// (le magasin ne l'a pas demandé) et ne doit pas aller plus loin.
    pub(in crate::ui) fn export_before(&mut self, e: &Event) -> bool {
        let Some(job) = self.export.job.as_deref() else { return false };
        let Stage::Paging { waiting: Some(b), retries, .. } = job.stage else { return false };
        let mine = |channel_id: &u64, before: &Option<u64>| *channel_id == job.channel && *before == Some(b);
        let hit = match e {
            Event::History { channel_id, before, .. } => mine(channel_id, before),
            Event::HistoryError { channel_id, before, after: None, .. } | Event::HistoryCancelled { channel_id, before, after: None } => mine(channel_id, before),
            _ => false,
        };
        if !hit {
            return false;
        }
        let Some(mut job) = self.export.job.take() else { return false };
        let only_mine = !self.store.asked_older(job.channel, b);
        match e {
            Event::History { messages, .. } => {
                let (lang, h24) = (self.lang, self.h24());
                let full = messages.len() >= HISTORY_PAGE;
                let page = self.store.render_foreign(messages.clone());
                let ids: Vec<u64> = page.iter().map(|m| m.id).collect();
                let mut done = true;
                if let Some(c) = job.collector.as_mut() {
                    for m in &page {
                        if let Some(l) = Line::of(&self.store, m, lang, h24) {
                            c.add(l);
                        }
                    }
                    c.page(&ids, full);
                    done = c.done;
                }
                if done {
                    self.export_pages_done(&mut job);
                } else {
                    job.stage = Stage::Paging { waiting: None, next_at: Instant::now() + page_gap(), retries: 0 };
                }
            }
            Event::HistoryError { error, .. } => job.stage = Stage::PageFailed(super::words::problem(self.lang, error, self.h24())),
            // Demande abandonnée avant de partir (un autre salon a été ouvert) : redemandée, quelques fois.
            _ if retries < MAX_RETRIES => job.stage = Stage::Paging { waiting: None, next_at: Instant::now() + RETRY_GAP, retries: retries + 1 },
            _ => job.stage = Stage::PageFailed(jt::EX_PAGE_FAILED.get(self.lang).to_string()),
        }
        self.export.job = Some(job);
        self.ctx.request_repaint();
        only_mine
    }

    /// À chaque passage de logique. Sans export en cours : une comparaison.
    pub(in crate::ui) fn export_tick(&mut self) {
        let Some(mut job) = self.export.job.take() else { return };
        if !plugins::on(Plugin::Export) {
            // Plugin coupé pendant un export : il s'arrête là.
            return;
        }
        let mut next: Option<fn(&mut App, &mut Job)> = None;
        match &mut job.stage {
            Stage::Picking(rx) => match rx.try_recv() {
                Ok(Some(path)) => {
                    if let Some(range) = self.export_range(&mut job) {
                        self.export_collect(&mut job, range, path);
                    } else {
                        job.stage = Stage::Form;
                    }
                }
                Ok(None) | Err(mpsc::TryRecvError::Disconnected) => job.stage = Stage::Form,
                Err(mpsc::TryRecvError::Empty) => {}
            },
            Stage::Paging { waiting, next_at, .. } if waiting.is_none() => {
                let now = Instant::now();
                if now < *next_at {
                    self.ctx.request_repaint_after(*next_at - now);
                } else if let Some(c) = job.collector.as_ref() {
                    // La requête d'historique du client, telle quelle : une page, par le limiteur du réseau.
                    *waiting = Some(c.cursor);
                    self.backend.send(vec![Cmd::History { channel_id: job.channel, before: Some(c.cursor) }]);
                }
            }
            Stage::Downloading { queue, busy, tx, rx, .. } => {
                while let Ok((id, rel)) = rx.try_recv() {
                    *busy = false;
                    match rel {
                        Some(rel) => {
                            job.saved_files += 1;
                            if let Some(c) = job.collector.as_mut() {
                                for f in c.lines_mut().flat_map(|l| &mut l.files).filter(|f| f.id == id) {
                                    f.saved = Some(rel.clone());
                                }
                            }
                        }
                        None => job.failed_files += 1,
                    }
                }
                // Un fichier à la fois.
                while !*busy {
                    let Some(d) = queue.pop_front() else { break };
                    let (tx, ctx) = (tx.clone(), self.ctx.clone());
                    let started = d.url.starts_with("https://")
                        && self.images.fetch_raw(d.url, move |res| {
                            let saved = res.ok().and_then(|bytes| std::fs::write(&d.abs, bytes).ok()).map(|()| d.rel);
                            let _ = tx.send((d.id, saved));
                            ctx.request_repaint();
                        });
                    if started {
                        *busy = true;
                    } else {
                        job.failed_files += 1;
                    }
                }
                if !*busy && queue.is_empty() {
                    next = Some(App::export_write);
                }
            }
            Stage::Writing(rx) => match rx.try_recv() {
                Ok(Ok(())) => job.stage = Stage::Done,
                Ok(Err(why)) => job.stage = Stage::Failed(jf::ex_write_failed(self.lang, why)),
                Err(mpsc::TryRecvError::Disconnected) => job.stage = Stage::Failed(jf::ex_write_failed(self.lang, "thread")),
                Err(mpsc::TryRecvError::Empty) => {}
            },
            _ => {}
        }
        if let Some(f) = next {
            f(self, &mut job);
        }
        self.export.job = Some(job);
    }

    /// Fenêtre de l'export, selon l'étape.
    pub(in crate::ui) fn export_ui(&mut self, ctx: &egui::Context) {
        let Some(mut boxed) = self.export.job.take() else { return };
        let job: &mut Job = &mut boxed;
        let t = self.theme.clone();
        let lang = self.lang;
        // Ce que les boutons demandent, appliqué après la fenêtre.
        #[derive(PartialEq)]
        enum Ask {
            None,
            Cancel,
            Go,
            Retry,
            Anyway,
            Download,
            Without,
        }
        let mut ask = Ask::None;
        let count = job.collector.as_ref().map_or(0, Collector::len);
        let progress = job.collector.as_ref().map_or(0.0, |c| c.progress(job.channel));
        let bar = |ui: &mut egui::Ui, t: &Tokens, frac: f32| {
            ui.add_space(t.spacing.item_y);
            ui.add(egui::ProgressBar::new(frac).desired_width(ui.available_width()).desired_height(t.typography.small).fill(t.colors.accent).corner_radius(t.r_row()));
        };
        let line = |ui: &mut egui::Ui, text: &str, color| {
            ui.add(egui::Label::new(RichText::new(text).color(color)).wrap());
        };
        let out = widgets::modal(
            ctx,
            &t,
            egui::Id::unique("export-conversation"),
            WIDTH,
            jt::EX_TITLE.get(lang),
            crate::i18n::t::CLOSE_TIP.get(lang),
            |ui| {
                ui.spacing_mut().item_spacing.y = t.spacing.item_y;
                let name = match &job.place {
                    Some(p) => format!("{} — {p}", job.title),
                    None => job.title.clone(),
                };
                ui.label(RichText::new(name).family(super::fonts::demi()).color(t.colors.text_strong));
                match &job.stage {
                    Stage::Form => {
                        line(ui, jt::EX_NOTE.get(lang), t.colors.muted);
                        ui.label(RichText::new(jt::EX_FORMAT.get(lang)).small().color(t.colors.faint));
                        ui.radio_value(&mut job.format, Format::Html, jt::EX_HTML.get(lang));
                        ui.radio_value(&mut job.format, Format::Text, jt::EX_TEXT.get(lang));
                        ui.label(RichText::new(jt::EX_PERIOD.get(lang)).small().color(t.colors.faint));
                        ui.horizontal(|ui| {
                            for (label, value) in [(jt::EX_FROM, &mut job.from), (jt::EX_TO, &mut job.to)] {
                                ui.label(RichText::new(label.get(lang)).color(t.colors.text));
                                let field = ui.add(egui::TextEdit::singleline(value).hint_text(jt::EX_DATE_HINT.get(lang)).char_limit(10).desired_width(130.0));
                                super::a11y::name(ui.ctx(), field.id, label.get(lang));
                            }
                            ui.label(RichText::new(jt::EX_PERIOD_TIP.get(lang)).small().color(t.colors.muted));
                        });
                        ui.checkbox(&mut job.local_only, jt::EX_LOCAL_ONLY.get(lang)).on_hover_text(jt::EX_LOCAL_ONLY_TIP.get(lang));
                        ui.checkbox(&mut job.files, jt::EX_FILES.get(lang));
                        if job.files {
                            line(ui, jt::EX_FILES_TIP.get(lang), t.colors.muted);
                        }
                        if let Some(e) = &job.error {
                            line(ui, e, t.colors.danger);
                        }
                    }
                    Stage::Picking(_) => line(ui, jt::EX_PICKING.get(lang), t.colors.muted),
                    Stage::Paging { .. } => {
                        line(ui, &jf::ex_reading(lang, count), t.colors.text);
                        bar(ui, &t, progress);
                    }
                    Stage::PageFailed(why) => {
                        line(ui, jt::EX_PAGE_FAILED.get(lang), t.colors.danger);
                        line(ui, why, t.colors.muted);
                        line(ui, &jf::ex_reading(lang, count), t.colors.text);
                    }
                    Stage::ConfirmFiles { count, bytes } => {
                        line(ui, &jf::ex_files_ask(lang, count, super::attachments::file_size(lang, *bytes)), t.colors.text);
                        if *bytes > ex::MAX_FILE_BYTES {
                            line(ui, jt::EX_TOO_BIG.get(lang), t.colors.danger);
                        }
                    }
                    Stage::Downloading { queue, total, busy, .. } => {
                        let done = total - queue.len() - usize::from(*busy);
                        line(ui, &jf::ex_downloading(lang, done, total), t.colors.text);
                        bar(ui, &t, done as f32 / (*total).max(1) as f32);
                    }
                    Stage::Writing(_) => line(ui, jt::EX_WRITING.get(lang), t.colors.muted),
                    Stage::Done => {
                        line(ui, jt::EX_DONE.get(lang), t.colors.text_strong);
                        let path = job.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
                        line(ui, &jf::ex_done(lang, count, path), t.colors.text);
                        if job.saved_files + job.failed_files > 0 {
                            line(ui, &jf::ex_done_files(lang, job.saved_files, job.failed_files), t.colors.text);
                        }
                        if job.offline {
                            line(ui, jt::EX_OFFLINE.get(lang), t.colors.muted);
                        }
                        if job.collector.as_ref().is_some_and(|c| c.capped) {
                            line(ui, jt::EX_CAPPED.get(lang), t.colors.muted);
                        }
                    }
                    Stage::Failed(why) => line(ui, why, t.colors.danger),
                }
            },
            |ui| {
                let mut button = |ui: &mut egui::Ui, kind, text: crate::i18n::Text, to: Ask| {
                    if widgets::button(ui, &t, kind, text.get(lang)).clicked() {
                        ask = to;
                    }
                };
                match &job.stage {
                    Stage::Form => {
                        button(ui, Button::Primary, jt::EX_GO, Ask::Go);
                        button(ui, Button::Link, crate::i18n::t::CANCEL, Ask::Cancel);
                    }
                    Stage::PageFailed(_) => {
                        button(ui, Button::Primary, jt::EX_RETRY, Ask::Retry);
                        button(ui, Button::Secondary, jt::EX_ANYWAY, Ask::Anyway);
                        button(ui, Button::Link, crate::i18n::t::CANCEL, Ask::Cancel);
                    }
                    Stage::ConfirmFiles { bytes, .. } => {
                        if *bytes <= ex::MAX_FILE_BYTES {
                            button(ui, Button::Primary, jt::EX_DOWNLOAD, Ask::Download);
                        }
                        button(ui, Button::Secondary, jt::EX_WITHOUT, Ask::Without);
                        button(ui, Button::Link, crate::i18n::t::CANCEL, Ask::Cancel);
                    }
                    Stage::Done | Stage::Failed(_) => button(ui, Button::Primary, jt::EX_CLOSE, Ask::Cancel),
                    _ => button(ui, Button::Secondary, crate::i18n::t::CANCEL, Ask::Cancel),
                }
            },
        );
        match ask {
            Ask::Go => {
                if self.export_range(job).is_some() {
                    let name = ex::file_name(&job.title, job.format);
                    match pick(ctx.clone(), jt::EX_TITLE.get(lang).to_string(), name, job.format) {
                        Some(rx) => job.stage = Stage::Picking(rx),
                        None => job.error = Some(jt::EX_UNAVAILABLE.get(lang).to_string()),
                    }
                }
            }
            Ask::Retry => job.stage = Stage::Paging { waiting: None, next_at: Instant::now(), retries: 0 },
            Ask::Anyway => self.export_pages_done(job),
            Ask::Download => self.export_download(job),
            Ask::Without => self.export_write(job),
            Ask::Cancel | Ask::None => {}
        }
        // Annuler, la croix, Échap : tout s'arrête là, une page encore en route est ignorée.
        if ask != Ask::Cancel && !out.closed {
            self.export.job = Some(boxed);
        }
    }

    /// Lance l'export vers `path` avec les choix de la fenêtre, sans boîte
    /// « Enregistrer sous » (tests).
    #[cfg(test)]
    fn export_to(&mut self, path: PathBuf) -> bool {
        let Some(mut job) = self.export.job.take() else { return false };
        let range = self.export_range(&mut job);
        if let Some(r) = range {
            self.export_collect(&mut job, r, path);
        }
        self.export.job = Some(job);
        range.is_some()
    }

    /// Aperçu hors ligne : la fenêtre sur le salon ouvert (`reading` : en
    /// pleine lecture de l'historique).
    #[doc(hidden)]
    pub fn export_preview(&mut self, reading: bool) {
        self.local.loaded = true;
        self.set_plugin(Plugin::Export, true);
        let Some(channel) = self.store.channel else { return };
        self.export_open(channel);
        let Some(job) = self.export.job.as_mut() else { return };
        job.files = true;
        if reading {
            let now = crate::state::store::now_ms();
            let mut c = Collector::new(Range { from_ms: Some(now - 30 * 86_400_000), to_ms: None }, now);
            let first = crate::state::search::snowflake_at(now - 11 * 86_400_000);
            for i in 0..350u64 {
                c.add(Line { id: first + (i << 22), author: String::new(), text: String::new(), edited: false, files: Vec::new() });
            }
            c.page(&[first], true);
            job.stage = Stage::Paging { waiting: Some(first), next_at: Instant::now(), retries: 0 };
            job.collector = Some(c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Lang;
    use crate::state::journal::tests_support::message;
    use crate::state::model::{Dm, User};
    use crate::state::Problem;
    use crate::ui::tests::offline_boot;
    use std::sync::{Arc, Mutex};

    const C: u64 = 100;
    /// Identifiants datés (un message par seconde à partir du 1er février 2026, UTC).
    fn id(n: u64) -> u64 {
        crate::state::search::snowflake_at(1_769_904_000_000 + n as i64 * 1000)
    }

    #[derive(Clone, Default)]
    struct Net {
        events: Arc<Mutex<Vec<Event>>>,
        sent: Arc<Mutex<Vec<Cmd>>>,
    }

    impl crate::ui::Backend for Net {
        fn set_wake(&mut self, _: crate::ui::Wake) {}
        fn connect(&mut self, _: crate::account::Token) {}
        fn disconnect(&mut self) {}
        fn is_connected(&self) -> bool {
            true
        }
        fn poll(&mut self) -> Vec<Event> {
            std::mem::take(&mut *self.events.lock().unwrap())
        }
        fn send(&mut self, cmds: Vec<Cmd>) {
            self.sent.lock().unwrap().extend(cmds);
        }
    }

    fn page(from: u64, n: u64) -> Vec<Message> {
        (from..from + n)
            .map(|i| {
                let mut m = message(id(i), C, &format!("message {i}"));
                m.attachments.clear();
                m
            })
            .collect()
    }

    use crate::state::model::Message;

    /// MP ouvert dont la dernière page (pleine : il y a plus ancien) est en mémoire.
    fn app(net: Net, first: u64, n: u64) -> App {
        let mut s = crate::state::Store::new();
        s.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: Vec::new(), dms: vec![Dm::new(C, "Alice".into(), 0)] });
        s.select_dms();
        s.select_channel(C);
        s.take_commands();
        s.apply(Event::History { channel_id: C, before: None, messages: page(first, n) });
        let ctx = egui::Context::default();
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = App::with_context(ctx, offline_boot(Box::new(net), s, images));
        app.lang = Lang::Fr;
        app.local.loaded = true;
        app.logic_pass();
        app
    }

    fn file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ferrocord-test-export-{}-{name}", std::process::id()))
    }

    fn histories(net: &Net) -> Vec<Option<u64>> {
        net.sent.lock().unwrap().iter().filter_map(|c| if let Cmd::History { channel_id: C, before } = c { Some(*before) } else { None }).collect()
    }

    fn stage(app: &App) -> &Stage {
        &app.export.job.as_ref().expect("export en cours").stage
    }

    /// Plugin coupé : pas de fenêtre, rien n'est demandé ni écrit.
    #[test]
    fn disabled_plugin_exports_nothing() {
        let net = Net::default();
        let mut app = app(net.clone(), 100, 3);
        app.set_plugin(Plugin::Export, false);
        assert!(!app.export_open(C) && !app.export_active());
        assert!(!app.export_before(&Event::History { channel_id: C, before: Some(id(100)), messages: Vec::new() }));
        app.logic_pass();
        assert!(histories(&net).is_empty());
    }

    #[test]
    fn local_messages_are_written_without_asking_discord() {
        let net = Net::default();
        let mut app = app(net.clone(), 100, 3);
        assert!(app.export_open(C));
        let path = file("local.txt");
        {
            let job = app.export.job.as_mut().unwrap();
            job.format = Format::Text;
            job.local_only = true;
            // Une date illisible est refusée avant tout.
            job.from = "hier".into();
        }
        assert!(!app.export_to(path.clone()));
        assert!(matches!(stage(&app), Stage::Form) && app.export.job.as_ref().unwrap().error.is_some());
        app.export.job.as_mut().unwrap().from.clear();
        assert!(app.export_to(path.clone()));
        app.logic_pass();
        assert!(matches!(stage(&app), Stage::Done));
        let out = std::fs::read_to_string(&path).unwrap();
        assert!(out.starts_with("Alice\n"), "{out}");
        assert!(out.contains("Alice\u{a0}: message 100\n") && out.contains("message 102\n") && out.contains("3 messages"));
        assert!(histories(&net).is_empty(), "rien n'est demandé à Discord");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn older_history_is_read_page_by_page_and_stays_out_of_the_channel() {
        let net = Net::default();
        let mut app = app(net.clone(), 100, HISTORY_PAGE as u64);
        assert!(app.store.chans[&C].more);
        app.export_open(C);
        let path = file("pages.html");
        assert!(app.export_to(path.clone()));
        assert!(matches!(stage(&app), Stage::Paging { waiting: None, .. }));
        // Une page à la fois : la requête d'historique du client, rien d'autre.
        app.logic_pass();
        app.logic_pass();
        assert_eq!(histories(&net), vec![Some(id(100))]);
        // Une page d'un autre repère n'est pas pour l'export.
        assert!(!app.export_before(&Event::History { channel_id: C, before: Some(id(7)), messages: Vec::new() }));
        net.events.lock().unwrap().push(Event::History { channel_id: C, before: Some(id(100)), messages: page(50, HISTORY_PAGE as u64) });
        app.logic_pass();
        app.logic_pass();
        assert_eq!(histories(&net), vec![Some(id(100)), Some(id(50))]);
        // Demande abandonnée par le réseau : refaite ; puis la dernière page (incomplète).
        net.events.lock().unwrap().push(Event::HistoryCancelled { channel_id: C, before: Some(id(50)), after: None });
        app.logic_pass();
        app.logic_pass();
        assert_eq!(histories(&net).len(), 3);
        net.events.lock().unwrap().push(Event::History { channel_id: C, before: Some(id(50)), messages: page(47, 3) });
        app.logic_pass();
        app.logic_pass();
        assert!(matches!(stage(&app), Stage::Done), "terminé au début du salon");
        assert_eq!(histories(&net).len(), 3);
        let out = std::fs::read_to_string(&path).unwrap();
        assert_eq!(out.matches("<div class=\"m\">").count(), 103);
        assert!(out.find("message 47<").unwrap() < out.find("message 149<").unwrap());
        // La liste du salon n'a pas bougé.
        let cs = &app.store.chans[&C];
        assert_eq!((cs.msgs.len(), cs.msgs[0].id, cs.error.is_none()), (HISTORY_PAGE, id(100), true));
        assert!(net.sent.lock().unwrap().iter().all(|c| matches!(c, Cmd::History { .. } | Cmd::Relations(_))), "{:?}", net.sent.lock().unwrap());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_period_bounds_the_reading_and_cancel_stops_everything() {
        let net = Net::default();
        let mut app = app(net.clone(), 100, HISTORY_PAGE as u64);
        // Période : le 1er février seulement ; la mémoire la couvre déjà.
        app.export_open(C);
        {
            let job = app.export.job.as_mut().unwrap();
            job.format = Format::Text;
            job.from = "2026-02-01".into();
            job.to = "2026-02-01".into();
        }
        let path = file("periode.txt");
        // (Les tests tournent à l'heure locale : la période est élargie d'un jour de chaque côté si besoin.)
        let offset = chrono::Local::now().offset().local_minus_utc();
        if offset != 0 {
            let job = app.export.job.as_mut().unwrap();
            job.from = "2026-01-31".into();
            job.to = "2026-02-02".into();
        }
        assert!(app.export_to(path.clone()));
        app.logic_pass();
        app.logic_pass();
        // Il reste de l'historique avant la fenêtre : une page est demandée, et elle passe le début de la période.
        assert_eq!(histories(&net).len(), 1);
        let old: Vec<Message> = (0..HISTORY_PAGE as u64)
            .map(|i| {
                let mut m = message(crate::state::search::snowflake_at(1_769_904_000_000 - 5 * 86_400_000 + i as i64), C, "vieux");
                m.attachments.clear();
                m
            })
            .collect();
        net.events.lock().unwrap().push(Event::History { channel_id: C, before: Some(id(100)), messages: old });
        app.logic_pass();
        app.logic_pass();
        assert!(matches!(stage(&app), Stage::Done));
        let out = std::fs::read_to_string(&path).unwrap();
        assert!(!out.contains("vieux") && out.contains("message 100") && out.contains("50 messages"));
        let _ = std::fs::remove_file(&path);

        // Annuler pendant la lecture : plus de demande, pas de fichier, la page en route est ignorée.
        app.export_open(C);
        let path = file("annule.txt");
        assert!(app.export_to(path.clone()));
        app.logic_pass();
        let sent = histories(&net).len();
        app.export.job = None;
        assert!(!app.export_before(&Event::History { channel_id: C, before: Some(id(100)), messages: Vec::new() }));
        app.logic_pass();
        assert_eq!(histories(&net).len(), sent);
        assert!(!path.exists());

        // Échec d'une page : annoncé dans la fenêtre, pas dans le salon ; « Exporter ce qui est rassemblé ».
        app.export_open(C);
        assert!(app.export_to(path.clone()));
        app.logic_pass();
        net.events.lock().unwrap().push(Event::HistoryError { channel_id: C, before: Some(id(100)), after: None, error: Problem::Network("coupure".into()) });
        app.logic_pass();
        assert!(matches!(stage(&app), Stage::PageFailed(_)));
        assert!(app.store.chans[&C].error.is_none());
        let mut job = app.export.job.take().unwrap();
        app.export_pages_done(&mut job);
        app.export.job = Some(job);
        app.logic_pass();
        assert!(matches!(stage(&app), Stage::Done) && path.exists());
        let _ = std::fs::remove_file(&path);
    }

    /// Pièces jointes : rien n'est téléchargé sans la case, ni avant la confirmation.
    #[test]
    fn attachments_wait_for_the_box_and_the_confirmation() {
        let net = Net::default();
        let mut app = app(net.clone(), 100, 2);
        let mut with_file = message(id(200), C, "avec fichier");
        with_file.attachments[0].size = 2048;
        with_file.attachments[0].url = "https://cdn.exemple.invalid/plan.png".into();
        net.events.lock().unwrap().push(Event::MessageCreate(with_file));
        app.logic_pass();
        app.export_open(C);
        let path = file("fichiers.html");
        app.export.job.as_mut().unwrap().files = true;
        assert!(app.export_to(path.clone()));
        app.logic_pass();
        assert!(matches!(stage(&app), Stage::ConfirmFiles { count: 1, bytes: 2048 }), "taille annoncée avant tout téléchargement");
        assert!(!path.exists() && !path.with_file_name(format!("{}_fichiers", path.file_stem().unwrap().to_string_lossy())).exists());
        // « Télécharger et exporter » : ici le chargement est désactivé, le fichier est noté en échec.
        let mut job = app.export.job.take().unwrap();
        app.export_download(&mut job);
        app.export.job = Some(job);
        app.logic_pass();
        app.logic_pass();
        assert!(matches!(stage(&app), Stage::Done));
        assert_eq!(app.export.job.as_ref().unwrap().failed_files, 1);
        let out = std::fs::read_to_string(&path).unwrap();
        assert!(out.contains("plan.png</a> <span class=\"e\">(non inclus)</span>") && !out.contains("<img"));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(path.with_file_name(format!("{}_fichiers", path.file_stem().unwrap().to_string_lossy())));
    }
}
