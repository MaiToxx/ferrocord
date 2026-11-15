//! Commandes slash (lot T11) : liste des commandes quand la saisie commence
//! par `/`, formulaire des options typées à la place du champ de saisie,
//! propositions du bot (autocomplétion), suivi des interactions envoyées
//! au-dessus de la saisie (« Envoi de /x… », « X n'a pas répondu »), et
//! commandes de menu contextuel d'un utilisateur.
//!
//! Chaque envoi suit un geste (Entrée, clic) ; rien n'est renvoyé seul.
//! L'autocomplétion du bot part après une courte pause de la frappe.

use super::App;
use crate::i18n::table::interactions::{f, t as tx};
use crate::state::interactions::{ChoiceValue, CmdOption, CommandKind, Failure, Filled, OptionKind, PendingState, Value, What};
use crate::state::Store;
use crate::theme::icons::Icon;
use eframe::egui::{self, Id, Key, RichText};
use std::time::{Duration, Instant};

/// Commandes montrées au plus par la liste (la frappe affine).
const MAX_LISTED: usize = 8;
/// Entrées proposées au plus par un champ d'utilisateur, de rôle ou de salon.
const MAX_CANDIDATES: usize = 6;
/// Pause de la frappe avant de demander ses propositions au bot.
const AUTOCOMPLETE_PAUSE: Duration = Duration::from_millis(350);

/// Une commande (ou sous-commande) de la liste.
#[derive(Clone, Debug, PartialEq)]
struct Entry {
    command_id: u64,
    path: Vec<String>,
    /// « /role ajouter ».
    label: String,
    description: String,
    app: String,
}

/// Champ du formulaire : texte saisi et choix faits.
#[derive(Clone, Debug)]
pub(super) struct Field {
    pub opt: CmdOption,
    pub text: String,
    /// Proposition du bot prise (son nom est dans `text`).
    pub value: Option<ChoiceValue>,
    /// Choix fixe pris (rang dans `opt.choices`).
    pub choice: Option<usize>,
    pub flag: Option<bool>,
    /// Utilisateur, rôle ou salon choisi : (identifiant, nom).
    pub pick: Option<(u64, String)>,
    /// Fichier choisi pour une option pièce jointe.
    pub file: Option<crate::state::upload::OutFile>,
}

impl Field {
    fn new(opt: CmdOption) -> Self {
        Field { opt, text: String::new(), value: None, choice: None, flag: None, pick: None, file: None }
    }
}

/// Pourquoi un champ empêche l'envoi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Invalid {
    Missing,
    Integer,
    Number,
    Range,
    Length,
    Unsupported,
}

/// Valeur envoyée pour ce champ (`None` : champ facultatif laissé vide).
pub(super) fn filled(f: &Field) -> Result<Option<Filled>, Invalid> {
    let o = &f.opt;
    let done = |value| Ok(Some(Filled { name: o.name.clone(), kind: o.kind, value }));
    if let Some(c) = f.choice.and_then(|i| o.choices.get(i)) {
        return done(choice_value(&c.value, o.kind));
    }
    if let Some(v) = &f.value {
        return done(choice_value(v, o.kind));
    }
    let text = f.text.trim();
    let empty = match o.kind {
        OptionKind::Boolean => f.flag.is_none(),
        OptionKind::User | OptionKind::Channel | OptionKind::Role | OptionKind::Mentionable => f.pick.is_none(),
        OptionKind::Attachment => f.file.is_none(),
        _ => text.is_empty(),
    };
    if empty {
        return match (o.required, o.kind) {
            (true, _) => Err(Invalid::Missing),
            (false, _) => Ok(None),
        };
    }
    let in_range = |x: f64| o.min_value.is_none_or(|m| x >= m) && o.max_value.is_none_or(|m| x <= m);
    match o.kind {
        OptionKind::String => {
            let n = u32::try_from(text.chars().count()).unwrap_or(u32::MAX);
            if o.min_length.is_some_and(|m| n < m) || o.max_length.is_some_and(|m| n > m) {
                return Err(Invalid::Length);
            }
            done(Value::Str(text.to_string()))
        }
        OptionKind::Integer => {
            let n: i64 = text.parse().map_err(|_| Invalid::Integer)?;
            if !in_range(n as f64) {
                return Err(Invalid::Range);
            }
            done(Value::Int(n))
        }
        OptionKind::Number => {
            let x: f64 = text.replace(',', ".").parse().map_err(|_| Invalid::Number)?;
            if !x.is_finite() {
                return Err(Invalid::Number);
            }
            if !in_range(x) {
                return Err(Invalid::Range);
            }
            done(Value::Num(x))
        }
        OptionKind::Boolean => done(Value::Bool(f.flag.unwrap_or(false))),
        OptionKind::User | OptionKind::Channel | OptionKind::Role | OptionKind::Mentionable => done(Value::Id(f.pick.as_ref().map_or(0, |p| p.0))),
        // Rang provisoire : les fichiers sont numérotés à l'envoi (`number_files`).
        OptionKind::Attachment => done(Value::Attachment(0)),
        _ => Err(Invalid::Unsupported),
    }
}

/// Options à envoyer et fichiers de leurs pièces jointes : chaque option
/// pièce jointe reçoit le rang de son fichier (`files[n]`).
pub(super) fn number_files(fields: &[Field]) -> (Vec<Filled>, Vec<crate::state::upload::OutFile>) {
    let mut files = Vec::new();
    let mut options = Vec::new();
    for f in fields {
        let Some(mut o) = filled(f).ok().flatten() else { continue };
        if let (Value::Attachment(_), Some(file)) = (&o.value, &f.file) {
            o.value = Value::Attachment(u32::try_from(files.len()).unwrap_or(u32::MAX));
            files.push(file.clone());
        }
        options.push(o);
    }
    (options, files)
}

fn choice_value(v: &ChoiceValue, kind: OptionKind) -> Value {
    match (v, kind) {
        (ChoiceValue::Int(n), OptionKind::Number) => Value::Num(*n as f64),
        (ChoiceValue::Int(n), _) => Value::Int(*n),
        (ChoiceValue::Num(x), _) => Value::Num(*x),
        (ChoiceValue::Str(s), _) => Value::Str(s.clone()),
    }
}

/// Autocomplétion du bot pour un champ.
#[derive(Clone, Copy, Debug)]
struct Ac {
    field: usize,
    due: Instant,
    sent: bool,
}

/// Formulaire d'une commande choisie dans la liste.
#[derive(Clone, Debug)]
pub(super) struct Form {
    channel_id: u64,
    command_id: u64,
    label: String,
    app: String,
    path: Vec<String>,
    pub fields: Vec<Field>,
    /// Le premier champ prend le focus à la première image.
    focus: bool,
    ac: Option<Ac>,
}

/// État de l'interface des commandes (appartient à `App`).
#[derive(Default)]
pub(super) struct CommandsUi {
    /// Liste « / » montrée à l'image précédente : flèches, Tab et Entrée lui reviennent.
    popup: bool,
    sel: usize,
    /// Texte pour lequel Échap a fermé la liste.
    closed_for: Option<String>,
    listed: Vec<Entry>,
    form: Option<Form>,
    /// Boîte « Ouvrir » des options pièce jointe (`channel` : rang du champ).
    attach: Option<crate::attach::Attach>,
    /// Fenêtre modale d'un bot (`modal.rs`).
    pub(super) modal: Option<super::modal::ModalForm>,
}

impl CommandsUi {
    pub(super) fn clear(&mut self) {
        *self = CommandsUi::default();
    }
}

/// Rang d'un libellé pour `needle` (minuscules) : début, début d'un mot, ailleurs.
fn rank(label: &str, needle: &str) -> Option<u8> {
    let l = label.to_lowercase();
    if l.starts_with(needle) {
        Some(0)
    } else if l.split([' ', '-', '_']).any(|w| w.starts_with(needle)) {
        Some(1)
    } else if l.contains(needle) {
        Some(2)
    } else {
        None
    }
}

/// Commandes slash de ce salon qui correspondent à `needle` (texte après `/`).
fn entries(store: &Store, channel_id: u64, needle: &str) -> Vec<Entry> {
    let needle = needle.to_lowercase();
    let mut out: Vec<(u8, Entry)> = Vec::new();
    for r in store.app_commands(channel_id, CommandKind::ChatInput) {
        for (path, description, _) in r.cmd.leaves() {
            let name = std::iter::once(r.cmd.name.as_str()).chain(path.iter().copied()).collect::<Vec<_>>().join(" ");
            let Some(k) = rank(&name, &needle) else { continue };
            let e = Entry { command_id: r.cmd.id, path: path.iter().map(|s| s.to_string()).collect(), label: format!("/{name}"), description: description.to_string(), app: r.app_name().to_string() };
            out.push((k, e));
        }
    }
    out.sort_by_key(|(k, _)| *k);
    out.into_iter().map(|(_, e)| e).collect()
}

/// Commandes intégrées (F10) dont le nom commence par `needle` : sans
/// formulaire, elles complètent seulement la saisie.
fn builtin_entries(lang: crate::i18n::Lang, needle: &str) -> Vec<Entry> {
    use crate::i18n::table::ff::t as ft;
    let needle = needle.to_lowercase();
    crate::state::builtin::LIST
        .iter()
        .filter(|n| n.starts_with(&needle))
        .map(|n| {
            let description = match *n {
                "shrug" => ft::CMD_SHRUG,
                "tableflip" => ft::CMD_TABLEFLIP,
                "unflip" => ft::CMD_UNFLIP,
                "me" => ft::CMD_ME,
                "spoiler" => ft::CMD_SPOILER,
                "tts" => ft::CMD_TTS,
                _ => ft::CMD_NICK,
            };
            Entry { command_id: 0, path: Vec::new(), label: format!("/{n}"), description: description.get(lang).to_string(), app: ft::BUILTIN.get(lang).to_string() }
        })
        .collect()
}

/// Utilisateurs, rôles ou salons proposés pour une option, selon la recherche.
fn candidates(store: &Store, channel_id: u64, o: &CmdOption, query: &str) -> Vec<(u64, String)> {
    let q = query.trim().to_lowercase();
    let keep = |n: &str| q.is_empty() || n.to_lowercase().contains(&q);
    let users = || store.known_people(channel_id).into_iter().filter(|(_, n)| keep(n)).collect::<Vec<_>>();
    let roles = || {
        let g = store.guild_of(channel_id).and_then(|g| store.guilds.iter().find(|x| x.id == g));
        g.map(|g| g.roles.iter().filter(|r| keep(&r.name)).map(|r| (r.id, format!("@{}", r.name))).collect::<Vec<_>>()).unwrap_or_default()
    };
    let mut out = match o.kind {
        OptionKind::User => users(),
        OptionKind::Role => roles(),
        OptionKind::Mentionable => {
            let mut v = users();
            v.extend(roles());
            v
        }
        OptionKind::Channel => store
            .visible_channels(channel_id)
            .into_iter()
            .filter(|c| o.channel_types.is_empty() || o.channel_types.contains(&super::components::channel_code(c.kind)))
            .filter(|c| keep(&c.name))
            .map(|c| (c.id, format!("#{}", c.name)))
            .collect(),
        _ => Vec::new(),
    };
    out.truncate(MAX_CANDIDATES);
    out
}

fn invalid_text(lang: crate::i18n::Lang, o: &CmdOption, e: Invalid) -> String {
    let num = |x: f64| if x.fract() == 0.0 { format!("{x:.0}") } else { x.to_string() };
    match e {
        Invalid::Missing => crate::i18n::table::interactions::t::FILL_REQUIRED.get(lang).to_string(),
        Invalid::Integer => tx::INTEGER_EXPECTED.get(lang).to_string(),
        Invalid::Number => tx::NUMBER_EXPECTED.get(lang).to_string(),
        Invalid::Unsupported => tx::ATTACHMENT_UNSUPPORTED.get(lang).to_string(),
        Invalid::Length => match (o.min_length, o.max_length) {
            (Some(a), Some(b)) => f::between(lang, a, b),
            (Some(a), None) => f::min_length(lang, a),
            (None, Some(b)) => f::at_most(lang, b),
            (None, None) => String::new(),
        },
        Invalid::Range => match (o.min_value, o.max_value) {
            (Some(a), Some(b)) => f::between(lang, num(a), num(b)),
            (Some(a), None) => f::at_least(lang, num(a)),
            (None, Some(b)) => f::at_most(lang, num(b)),
            (None, None) => String::new(),
        },
    }
}

impl App {
    /// Flèches, Tab, Entrée et Échap pour la liste « / » ouverte à l'image
    /// précédente ; `true` si Entrée (ou Tab) a choisi une commande.
    pub(super) fn slash_keys(&mut self, ui: &mut egui::Ui, cid: u64, focused: bool) -> bool {
        if !self.cmds.popup || !focused || self.cmds.listed.is_empty() {
            return false;
        }
        let n = self.cmds.listed.len();
        let (down, up, pick, esc) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, Key::Enter) || i.consume_key(egui::Modifiers::NONE, Key::Tab),
                i.consume_key(egui::Modifiers::NONE, Key::Escape),
            )
        });
        if down {
            self.cmds.sel = (self.cmds.sel + 1) % n;
        }
        if up {
            self.cmds.sel = (self.cmds.sel + n - 1) % n;
        }
        if esc {
            self.cmds.closed_for = Some(self.composer_text().to_string());
            self.cmds.popup = false;
        }
        if pick && let Some(e) = self.cmds.listed.get(self.cmds.sel.min(n - 1)).cloned() {
            self.start_form(cid, &e);
            return true;
        }
        false
    }

    /// Liste des commandes au-dessus de la saisie, quand le texte commence par `/`.
    pub(super) fn slash_popup(&mut self, ui: &mut egui::Ui, cid: u64) {
        let text = self.composer_text().to_string();
        // « Suggestions de commandes » décoché (Texte et images) : pas de liste.
        let off = self.store.account.bool(crate::state::usersettings::schema::Key::CommandSuggestions) == Some(false);
        let word = text.strip_prefix('/').filter(|w| !off && !w.contains(char::is_whitespace));
        let closed = self.cmds.closed_for.as_deref() == Some(text.as_str());
        let Some(needle) = word.filter(|_| !closed).map(str::to_string) else {
            self.cmds.popup = false;
            self.cmds.listed.clear();
            if !text.starts_with('/') {
                self.cmds.closed_for = None;
            }
            return;
        };
        // Taper « / » est le geste qui demande l'index (une fois, puis gardé).
        self.store.want_commands(cid);
        let mut all = builtin_entries(self.lang, &needle);
        all.extend(entries(&self.store, cid, &needle));
        let loading = self.store.commands_loading(cid);
        let listed: Vec<Entry> = all.into_iter().take(MAX_LISTED).collect();
        if listed != self.cmds.listed {
            self.cmds.sel = 0;
        }
        self.cmds.listed = listed;
        self.cmds.popup = !self.cmds.listed.is_empty();
        if self.cmds.listed.is_empty() && !loading && needle.is_empty() {
            return;
        }
        let t = self.theme.clone();
        let lang = self.lang;
        let mut chosen = None;
        t.window_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(tx::COMMANDS.get(lang)).small().family(crate::ui::fonts::demi()).strong().color(t.colors.faint));
            if self.cmds.listed.is_empty() {
                let what = if loading { tx::LOADING_COMMANDS } else { tx::NO_COMMANDS };
                ui.label(RichText::new(what.get(lang)).color(t.colors.muted));
            }
            for (i, e) in self.cmds.listed.iter().enumerate() {
                let sel = i == self.cmds.sel;
                let row = ui.horizontal(|ui| {
                    let r = ui.selectable_label(sel, RichText::new(&e.label).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(&e.app).small().color(t.colors.faint));
                        ui.add(egui::Label::new(RichText::new(&e.description).color(t.colors.muted)).truncate());
                    });
                    r
                });
                if row.inner.clicked() {
                    chosen = Some(e.clone());
                }
            }
        });
        ui.add_space(t.spacing.chat_pad / 2.0);
        if let Some(e) = chosen {
            self.start_form(cid, &e);
        }
    }

    fn start_form(&mut self, cid: u64, e: &Entry) {
        // Commande intégrée : pas de formulaire, son nom est complété dans la saisie.
        if e.command_id == 0 {
            self.cmds.popup = false;
            self.cmds.listed.clear();
            return self.set_composer_text(&format!("{} ", e.label));
        }
        let opts = self.store.app_commands(cid, CommandKind::ChatInput).into_iter().find(|r| r.cmd.id == e.command_id).and_then(|r| r.cmd.leaf_options(&e.path).map(<[CmdOption]>::to_vec));
        let Some(opts) = opts else { return };
        // Options obligatoires d'abord, comme dans l'application officielle.
        let mut fields: Vec<Field> = opts.into_iter().map(Field::new).collect();
        fields.sort_by_key(|f| !f.opt.required);
        self.cmds.form = Some(Form { channel_id: cid, command_id: e.command_id, label: e.label.clone(), app: e.app.clone(), path: e.path.clone(), fields, focus: true, ac: None });
        self.cmds.popup = false;
        self.cmds.listed.clear();
        self.store.interactions.forget_choices();
        self.set_composer_text("");
    }

    /// Ouvre le formulaire de la commande `name` (aperçu, tests).
    pub fn open_command(&mut self, name: &str) -> bool {
        let Some(cid) = self.store.channel else { return false };
        let Some(e) = entries(&self.store, cid, name).into_iter().find(|e| e.label == format!("/{name}")) else { return false };
        self.start_form(cid, &e);
        true
    }

    /// Écrit `text` dans le champ `index` du formulaire ouvert (aperçu, tests).
    pub fn set_command_field(&mut self, index: usize, text: &str) {
        if let Some(fld) = self.cmds.form.as_mut().and_then(|f| f.fields.get_mut(index)) {
            fld.text = text.to_string();
        }
    }

    /// Formulaire de la commande choisie, à la place du champ de saisie ;
    /// `false` s'il n'y en a pas (le champ est dessiné).
    pub(super) fn command_form_ui(&mut self, ui: &mut egui::Ui, cid: u64) -> bool {
        if self.cmds.form.as_ref().is_some_and(|f| f.channel_id != cid) {
            self.cmds.form = None;
        }
        let Some(mut form) = self.cmds.form.take() else { return false };
        // Fichiers choisis pour les options pièce jointe (lus dans un fil à part).
        for s in self.cmds.attach.as_ref().map(crate::attach::Attach::poll).unwrap_or_default() {
            match s {
                crate::attach::Staged::File { channel, file, .. } => {
                    if let Some(fld) = usize::try_from(channel).ok().and_then(|i| form.fields.get_mut(i)) {
                        fld.file = Some(file);
                    }
                }
                crate::attach::Staged::Unreadable { .. } => self.notify(tx::ATTACHMENT_UNREADABLE),
            }
        }
        let picking = self.cmds.attach.as_ref().is_some_and(crate::attach::Attach::picking);
        let mut pick_for: Option<usize> = None;
        let t = self.theme.clone();
        let lang = self.lang;
        let store = &self.store;
        let (mut close, mut send) = (false, false);
        let mut ac_change: Option<usize> = None;
        let mut picked_choice: Option<(usize, String, ChoiceValue)> = None;
        let choices = store.interactions.choices().map(<[_]>::to_vec);
        let errors: Vec<Option<Invalid>> = form.fields.iter().map(|f| filled(f).err()).collect();
        let valid = errors.iter().all(Option::is_none);
        egui::Frame::new().fill(t.colors.bg3).corner_radius(t.r_composer()).inner_margin(t.composer_inner()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
            ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close |= super::util::icon_button(ui, &t, Icon::Close, tx::CLOSE.get(lang)).clicked();
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.label(RichText::new(&form.label).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
                    ui.label(RichText::new(&form.app).small().color(t.colors.faint));
                });
            });
            let first = std::mem::take(&mut form.focus);
            for (i, fld) in form.fields.iter_mut().enumerate() {
                ui.add_space(t.spacing.media_gap);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&fld.opt.name).family(crate::ui::fonts::demi()).strong().color(t.colors.text));
                    if fld.opt.required {
                        ui.label(RichText::new(tx::REQUIRED.get(lang)).small().color(t.colors.danger));
                    }
                    ui.add(egui::Label::new(RichText::new(&fld.opt.description).small().color(t.colors.muted)).truncate());
                });
                let o = fld.opt.clone();
                let salt = ("cmd-field", form.command_id, i);
                if !o.choices.is_empty() {
                    let shown = fld.choice.and_then(|c| o.choices.get(c)).map_or_else(|| tx::NOT_SET.get(lang).to_string(), |c| c.name.clone());
                    egui::ComboBox::from_id_salt(salt).width(ui.available_width()).truncate().selected_text(shown).show_ui(ui, |ui| {
                        if !o.required && ui.selectable_label(fld.choice.is_none(), tx::NOT_SET.get(lang)).clicked() {
                            fld.choice = None;
                        }
                        for (k, c) in o.choices.iter().enumerate() {
                            if ui.selectable_label(fld.choice == Some(k), &c.name).clicked() {
                                fld.choice = Some(k);
                            }
                        }
                    });
                    continue;
                }
                match o.kind {
                    OptionKind::Boolean => {
                        ui.horizontal(|ui| {
                            for (v, text) in [(Some(true), tx::YES), (Some(false), tx::NO), (None, tx::NOT_SET)] {
                                if v.is_none() && o.required {
                                    continue;
                                }
                                if ui.radio(fld.flag == v, text.get(lang)).clicked() {
                                    fld.flag = v;
                                }
                            }
                        });
                    }
                    OptionKind::Attachment => {
                        if let Some(file) = &fld.file {
                            if ui.button(format!("{}  ✕", file.name)).on_hover_text(tx::CLEAR.get(lang)).clicked() {
                                fld.file = None;
                            }
                        } else if crate::attach::Attach::CAN_PICK {
                            if ui.add_enabled(!picking, egui::Button::new(tx::ATTACHMENT_PICK.get(lang))).clicked() {
                                pick_for = Some(i);
                            }
                        } else {
                            ui.label(RichText::new(tx::ATTACHMENT_UNSUPPORTED.get(lang)).small().color(t.colors.muted));
                        }
                    }
                    OptionKind::User | OptionKind::Channel | OptionKind::Role | OptionKind::Mentionable => {
                        if let Some((_, name)) = &fld.pick {
                            if ui.button(format!("{name}  ✕")).on_hover_text(tx::CLEAR.get(lang)).clicked() {
                                fld.pick = None;
                            }
                        } else {
                            let r = ui.add(egui::TextEdit::singleline(&mut fld.text).id(Id::unique(salt)).hint_text(tx::SEARCH.get(lang)).desired_width(f32::INFINITY));
                            if first && i == 0 {
                                r.request_focus();
                            }
                            if r.has_focus() || !fld.text.is_empty() {
                                ui.horizontal_wrapped(|ui| {
                                    for (id, name) in candidates(store, cid, &o, &fld.text) {
                                        if ui.button(&name).clicked() {
                                            fld.pick = Some((id, name));
                                            fld.text.clear();
                                        }
                                    }
                                });
                            }
                        }
                    }
                    _ => {
                        let mut edit = egui::TextEdit::singleline(&mut fld.text).id(Id::unique(salt)).desired_width(f32::INFINITY);
                        if let Some(max) = o.max_length {
                            edit = edit.char_limit(max as usize);
                        }
                        let r = ui.add(edit);
                        if first && i == 0 {
                            r.request_focus();
                        }
                        if r.changed() {
                            fld.value = None;
                        }
                        if o.autocomplete && (r.changed() || r.gained_focus()) {
                            ac_change = Some(i);
                        }
                        send |= r.lost_focus() && ui.input(|inp| inp.key_pressed(Key::Enter));
                        // Propositions du bot pour ce champ.
                        if o.autocomplete
                            && form.ac.is_some_and(|a| a.field == i && a.sent)
                            && let Some(list) = &choices
                        {
                            t.card_frame().show(ui, |ui| {
                                ui.label(RichText::new(tx::SUGGESTIONS.get(lang)).small().color(t.colors.faint));
                                for c in list {
                                    if ui.selectable_label(false, &c.name).clicked() {
                                        picked_choice = Some((i, c.name.clone(), c.value.clone()));
                                    }
                                }
                            });
                        }
                    }
                }
                if let Some(e) = errors.get(i).copied().flatten().filter(|e| *e != Invalid::Missing) {
                    ui.label(RichText::new(invalid_text(lang, &o, e)).small().color(t.colors.danger));
                }
            }
            ui.add_space(t.spacing.media_gap);
            ui.horizontal(|ui| {
                let b = egui::Button::new(RichText::new(tx::SEND.get(lang)).color(t.colors.on_accent)).fill(t.colors.accent);
                send |= ui.add_enabled(valid, b).clicked();
                close |= ui.button(tx::CANCEL.get(lang)).clicked();
                if !valid {
                    ui.label(RichText::new(tx::FILL_REQUIRED.get(lang)).small().color(t.colors.faint));
                }
            });
            // Commande sans option : Entrée l'envoie.
            send |= form.fields.is_empty() && ui.input(|i| i.key_pressed(Key::Enter));
        });
        close |= ui.input(|i| i.key_pressed(Key::Escape));
        if let Some((i, name, value)) = picked_choice
            && let Some(fld) = form.fields.get_mut(i)
        {
            fld.text = name;
            fld.value = Some(value);
            form.ac = None;
            self.store.interactions.forget_choices();
        }
        if let Some(i) = ac_change {
            form.ac = Some(Ac { field: i, due: Instant::now() + AUTOCOMPLETE_PAUSE, sent: false });
        }
        if close {
            self.store.interactions.forget_choices();
            self.ctx.memory_mut(|m| m.request_focus(Id::unique("composer")));
            return true;
        }
        if let Some(i) = pick_for {
            let ctx = self.ctx.clone();
            let attach = self.cmds.attach.get_or_insert_with(|| crate::attach::Attach::new(std::sync::Arc::new(move || ctx.request_repaint())));
            #[cfg(windows)]
            attach.pick(i as u64, tx::ATTACHMENT_PICK.get(lang));
            #[cfg(not(windows))]
            let _ = (attach, i);
        }
        if send && valid {
            let (options, files) = number_files(&form.fields);
            if self.store.run_command_with_files(cid, form.command_id, form.path.clone(), options, None, files) {
                self.store.interactions.forget_choices();
                self.ctx.memory_mut(|m| m.request_focus(Id::unique("composer")));
                return true;
            }
            self.notify(if self.store.interactions.session().is_none() { tx::NOT_CONNECTED } else { tx::COMMAND_GONE });
        }
        self.autocomplete_due(&mut form, cid);
        self.cmds.form = Some(form);
        true
    }

    /// Demande au bot ses propositions quand la frappe s'est arrêtée ; un
    /// seul réveil est prévu en attendant.
    fn autocomplete_due(&mut self, form: &mut Form, cid: u64) {
        let Some(ac) = form.ac.filter(|a| !a.sent) else { return };
        let now = Instant::now();
        if now < ac.due {
            self.ctx.request_repaint_after(ac.due - now);
            return;
        }
        let Some(fld) = form.fields.get(ac.field) else { return };
        let focused = Filled { name: fld.opt.name.clone(), kind: fld.opt.kind, value: Value::Str(fld.text.clone()) };
        let others: Vec<Filled> = form.fields.iter().enumerate().filter(|(i, _)| *i != ac.field).filter_map(|(_, f)| filled(f).ok().flatten()).collect();
        self.store.autocomplete(cid, form.command_id, form.path.clone(), others, focused);
        form.ac = Some(Ac { sent: true, ..ac });
    }

    /// Interactions envoyées depuis ce salon : attente, puis échec éventuel
    /// (à fermer par l'utilisateur).
    pub(super) fn interaction_rows(&mut self, ui: &mut egui::Ui, cid: u64) {
        let t = self.theme.clone();
        let lang = self.lang;
        let h24 = self.h24();
        let rows: Vec<(u64, String, bool)> = self
            .store
            .interactions
            .pending()
            .iter()
            .filter(|p| p.channel_id == cid)
            .map(|p| {
                let app = p.app_name.as_str();
                match &p.state {
                    PendingState::Sending | PendingState::Waiting => {
                        let text = match &p.what {
                            What::Command(c) => f::sending(lang, c, app),
                            _ => f::sending_action(lang, app),
                        };
                        (p.nonce, text, false)
                    }
                    PendingState::Failed(Failure::NoResponse) => (p.nonce, if app.is_empty() { tx::NO_RESPONSE.get(lang).to_string() } else { f::no_response(lang, app) }, true),
                    PendingState::Failed(Failure::Code(c)) => (p.nonce, f::failed_code(lang, app, c), true),
                    PendingState::Failed(Failure::Problem(pb)) => (p.nonce, f::failed(lang, app, super::words::problem(lang, pb, h24)), true),
                }
            })
            .collect();
        let mut dismissed = None;
        for (nonce, text, failed) in rows {
            let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
            ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if failed && super::util::icon_button(ui, &t, Icon::Close, tx::CLOSE.get(lang)).clicked() {
                    dismissed = Some(nonce);
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let color = if failed { t.colors.danger } else { t.colors.muted };
                    ui.add(egui::Label::new(RichText::new(text).small().color(color)).truncate());
                });
            });
        }
        if let Some(n) = dismissed {
            self.store.dismiss_interaction(n);
        }
    }

    /// Délais des interactions : une échéance passée devient un échec ; un
    /// seul réveil est prévu pour la suivante.
    pub(super) fn interactions_tick(&mut self) {
        // Session fermée (déconnexion, autre compte) : formulaire et fenêtre de l'ancienne partent.
        if self.store.interactions.session().is_none() && (self.cmds.form.is_some() || self.cmds.modal.is_some()) {
            self.cmds.clear();
        }
        if let Some(end) = self.store.expire_interactions() {
            let wait = u64::try_from(end.saturating_sub(crate::state::store::now_ms())).unwrap_or(0);
            self.ctx.request_repaint_after(Duration::from_millis(wait));
        }
    }

    /// Commandes de menu contextuel d'un utilisateur dans le salon ouvert
    /// (menu ouvert : l'index est demandé s'il manque).
    pub(super) fn user_app_commands(&mut self) -> Vec<(u64, String)> {
        let Some(cid) = self.store.channel else { return Vec::new() };
        self.store.want_commands(cid);
        self.store.app_commands(cid, CommandKind::User).iter().map(|r| (r.cmd.id, r.cmd.name.clone())).collect()
    }

    pub(super) fn run_user_command(&mut self, command_id: u64, user_id: u64) {
        let Some(cid) = self.store.channel else { return };
        if !self.store.run_command(cid, command_id, Vec::new(), Vec::new(), Some(user_id)) {
            self.notify(if self.store.interactions.session().is_none() { tx::NOT_CONNECTED } else { tx::COMMAND_GONE });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opt(kind: OptionKind, required: bool) -> CmdOption {
        CmdOption {
            kind,
            name: "o".into(),
            description: String::new(),
            required,
            choices: Vec::new(),
            options: Vec::new(),
            channel_types: Vec::new(),
            min_value: None,
            max_value: None,
            min_length: None,
            max_length: None,
            autocomplete: false,
        }
    }

    fn field(kind: OptionKind, required: bool, text: &str) -> Field {
        Field { text: text.into(), ..Field::new(opt(kind, required)) }
    }

    fn value(f: &Field) -> Result<Option<Value>, Invalid> {
        filled(f).map(|x| x.map(|x| x.value))
    }

    #[test]
    fn typed_values_are_checked() {
        assert_eq!(value(&field(OptionKind::String, true, "")), Err(Invalid::Missing));
        assert_eq!(value(&field(OptionKind::String, false, "  ")), Ok(None));
        assert_eq!(value(&field(OptionKind::String, true, " salut ")), Ok(Some(Value::Str("salut".into()))));
        assert_eq!(value(&field(OptionKind::Integer, true, "12")), Ok(Some(Value::Int(12))));
        assert_eq!(value(&field(OptionKind::Integer, true, "1.5")), Err(Invalid::Integer));
        assert_eq!(value(&field(OptionKind::Number, true, "1,5")), Ok(Some(Value::Num(1.5))));
        assert_eq!(value(&field(OptionKind::Number, true, "NaN")), Err(Invalid::Number));
        assert_eq!(value(&field(OptionKind::Number, true, "inf")), Err(Invalid::Number));
        let mut ranged = field(OptionKind::Number, true, "50");
        ranged.opt.max_value = Some(48.0);
        assert_eq!(value(&ranged), Err(Invalid::Range));
        let mut long = field(OptionKind::String, true, "abcd");
        long.opt.max_length = Some(3);
        assert_eq!(value(&long), Err(Invalid::Length));
        let mut b = field(OptionKind::Boolean, true, "");
        assert_eq!(value(&b), Err(Invalid::Missing));
        b.flag = Some(false);
        assert_eq!(value(&b), Ok(Some(Value::Bool(false))));
        let mut u = field(OptionKind::User, true, "ali");
        assert_eq!(value(&u), Err(Invalid::Missing), "une recherche n'est pas un choix");
        u.pick = Some((7, "Alice".into()));
        assert_eq!(value(&u), Ok(Some(Value::Id(7))));
        assert_eq!(value(&field(OptionKind::Attachment, true, "")), Err(Invalid::Missing));
        assert_eq!(value(&field(OptionKind::Attachment, false, "")), Ok(None));
    }

    #[test]
    fn attachment_options_number_their_files() {
        use crate::state::upload::{OutFile, Source};
        let file = |name: &str| OutFile { source: Source::Memory(std::sync::Arc::from(vec![1u8])), name: name.into(), size: 1, description: String::new(), spoiler: false };
        let mut a = field(OptionKind::Attachment, true, "");
        a.file = Some(file("a.png"));
        let skipped = field(OptionKind::Attachment, false, "");
        let mut b = field(OptionKind::Attachment, false, "");
        b.file = Some(file("b.txt"));
        let n = field(OptionKind::Integer, true, "3");
        let (options, files) = number_files(&[a, skipped, n, b]);
        assert_eq!(options.iter().map(|o| o.value.clone()).collect::<Vec<_>>(), vec![Value::Attachment(0), Value::Int(3), Value::Attachment(1)]);
        assert_eq!(files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["a.png", "b.txt"]);
    }

    #[test]
    fn choices_and_bot_suggestions_send_their_value() {
        let mut c = field(OptionKind::Integer, true, "");
        c.opt.choices = vec![crate::state::interactions::Choice { name: "Un jour".into(), value: ChoiceValue::Int(1440) }];
        c.choice = Some(0);
        assert_eq!(value(&c), Ok(Some(Value::Int(1440))));
        let mut s = field(OptionKind::String, true, "Paris");
        s.value = Some(ChoiceValue::Str("paris".into()));
        assert_eq!(value(&s), Ok(Some(Value::Str("paris".into()))));
    }

    #[test]
    fn ranking_prefers_prefixes() {
        assert_eq!(rank("sondage", "so"), Some(0));
        assert_eq!(rank("role ajouter", "aj"), Some(1));
        assert_eq!(rank("sondage", "dag"), Some(2));
        assert_eq!(rank("sondage", "x"), None);
    }
}
