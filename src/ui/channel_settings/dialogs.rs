//! Fenêtres « Créer un salon » / « Créer une catégorie » (salon privé
//! compris), « Cloner le salon » et « Supprimer » (confirmation par le nom).

use super::App;
use crate::i18n::table::chadmin::{f, t as tx};
use crate::i18n::Text;
use crate::state::admin::channels::{private_overwrites, valid_name, ChanAction, NewChannel, NewKind, NAME_MAX};
use crate::state::model::ChannelKind;
use crate::theme::icons::Icon;
use eframe::egui::{self, Id, RichText};

pub(super) struct Create {
    guild: u64,
    parent: Option<u64>,
    category: bool,
    kind: NewKind,
    name: String,
    private: bool,
    roles: Vec<u64>,
}

pub(super) struct Clone {
    channel: u64,
    name: String,
}

pub(super) struct Delete {
    channel: u64,
    typed: String,
}

/// Types proposés, dans l'ordre de Discord : (type, icône, nom, explication).
const KINDS: [(NewKind, Icon, Text, Text); 6] = [
    (NewKind::Text, Icon::Hash, tx::KIND_TEXT, tx::KIND_TEXT_TIP),
    (NewKind::Voice, Icon::Speaker, tx::KIND_VOICE, tx::KIND_VOICE_TIP),
    (NewKind::Forum, Icon::Forum, tx::KIND_FORUM, tx::KIND_FORUM_TIP),
    (NewKind::Announcement, Icon::Megaphone, tx::KIND_ANNOUNCEMENT, tx::KIND_ANNOUNCEMENT_TIP),
    (NewKind::Stage, Icon::Stage, tx::KIND_STAGE, tx::KIND_STAGE_TIP),
    (NewKind::Media, Icon::Media, tx::KIND_MEDIA, tx::KIND_MEDIA_TIP),
];

impl App {
    /// « Créer un salon » (dans `parent`) ou « Créer une catégorie ».
    pub(super) fn open_create_channel(&mut self, guild: u64, parent: Option<u64>, category: bool) {
        if !self.store.can_manage_channels(guild) {
            return;
        }
        let kind = if category { NewKind::Category } else { NewKind::Text };
        self.chan_ui.create = Some(Create { guild, parent: parent.filter(|_| !category), category, kind, name: String::new(), private: false, roles: Vec::new() });
    }

    /// « Créer un salon » (aperçu, tests).
    pub fn show_create_channel(&mut self, guild: u64, parent: Option<u64>, category: bool) -> bool {
        self.open_create_channel(guild, parent, category);
        self.chan_ui.create.is_some()
    }

    /// « Cloner le salon » : ses détails sont demandés pour reprendre le sujet.
    pub(super) fn open_clone_channel(&mut self, channel: u64) {
        let Some((guild, name)) = self.store.guilds.iter().find_map(|g| g.channels.iter().find(|c| c.id == channel).map(|c| (g.id, c.name.clone()))) else { return };
        if !self.store.can_manage_channels(guild) {
            return;
        }
        if self.store.chan_admin.details_of(channel).is_none() {
            self.store.load_channel(channel);
        }
        self.chan_ui.clone = Some(Clone { channel, name });
    }

    /// Demande la suppression d'un salon ou d'une catégorie (confirmée par son nom).
    pub(super) fn ask_delete_channel(&mut self, channel: u64) {
        if self.store.can_edit_channel(channel) {
            self.chan_ui.delete = Some(Delete { channel, typed: String::new() });
        }
    }

    #[cfg(test)]
    pub(super) fn type_delete_name(&mut self, name: &str) {
        if let Some(d) = self.chan_ui.delete.as_mut() {
            d.typed = name.to_string();
        }
    }

    pub(super) fn create_channel_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(guild) = self.chan_ui.create.as_ref().map(|c| c.guild) else { return };
        let Some(g) = self.store.guilds.iter().find(|g| g.id == guild) else {
            self.chan_ui.create = None;
            return;
        };
        let t = self.theme.clone();
        let lang = self.lang;
        let parent_name = self.chan_ui.create.as_ref().and_then(|c| c.parent).and_then(|p| g.channels.iter().find(|c| c.id == p)).map(|c| c.name.clone());
        let mut roles: Vec<(u64, String, i32)> = g.roles.iter().filter(|r| r.id != guild).map(|r| (r.id, r.name.clone(), r.position)).collect();
        roles.sort_by_key(|r| std::cmp::Reverse(r.2));
        let me = self.store.private_needs_me(guild);
        let busy = self.store.chan_admin.pending(ChanAction::Create);
        let (mut create, mut close) = (false, false);
        let Some(c) = self.chan_ui.create.as_mut() else { return };
        let title = if c.category { tx::CREATE_CATEGORY } else { tx::CREATE_CHANNEL };
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.2);
        let ok = !busy && valid_name(&c.name).is_some();
        let mut enter = false;
        let modal = crate::ui::widgets::modal(ui.ctx(), &t, Id::unique("creer-salon"), width, title.get(lang), crate::i18n::t::CLOSE_TIP.get(lang), |ui| {
            crate::ui::settings::shell::page_style(ui, &t);
            if let Some(p) = &parent_name {
                ui.label(RichText::new(f::create_in(lang, p)).small().color(t.colors.muted));
            }
            if !c.category {
                ui.add_space(t.spacing.item_y);
                ui.label(RichText::new(tx::CHANNEL_TYPE.get(lang).to_uppercase()).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
                for (kind, icon, name, tip) in KINDS {
                    let on = c.kind == kind;
                    let r = egui::Frame::new().fill(if on { t.colors.selected } else { t.colors.bg1 }).corner_radius(t.r_row()).inner_margin(t.embed_margin()).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            super::super::util::inline_icon(ui, icon, t.typography.title, t.colors.muted);
                            ui.vertical(|ui| {
                                ui.label(RichText::new(name.get(lang)).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
                                ui.add(egui::Label::new(RichText::new(tip.get(lang)).small().color(t.colors.faint)).wrap());
                            });
                        });
                    });
                    let resp = ui.interact(r.response.rect, ui.scope_id().with(("type-salon", kind.code())), egui::Sense::click());
                    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, on, name.get(lang)));
                    if resp.clicked() {
                        c.kind = kind;
                    }
                }
            }
            ui.add_space(t.spacing.item_y);
            let label = if c.category { tx::CATEGORY_NAME } else { tx::CHANNEL_NAME };
            ui.label(RichText::new(label.get(lang).to_uppercase()).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
            let field = ui.add(egui::TextEdit::singleline(&mut c.name).char_limit(NAME_MAX).desired_width(f32::INFINITY));
            if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                enter = true;
            }
            ui.add_space(t.spacing.item_y);
            let private = if c.category { tx::PRIVATE_CATEGORY } else { tx::PRIVATE_CHANNEL };
            crate::ui::settings::shell::switch_label(ui, &mut c.private, RichText::new(private.get(lang)).family(crate::ui::fonts::demi()).strong());
            ui.label(RichText::new(tx::PRIVATE_TIP.get(lang)).small().color(t.colors.faint));
            if c.private {
                ui.label(RichText::new(tx::WHO_CAN_SEE.get(lang)).family(crate::ui::fonts::demi()).strong());
                if roles.is_empty() {
                    ui.label(RichText::new(tx::NO_ROLES.get(lang)).small().color(t.colors.muted));
                }
                egui::ScrollArea::vertical().max_height(t.layout.card_width * 0.5).show(ui, |ui| {
                    for (id, name, _) in &roles {
                        let mut on = c.roles.contains(id);
                        if crate::ui::settings::shell::switch_label(ui, &mut on, name.as_str()).changed() {
                            if on {
                                c.roles.push(*id);
                            } else {
                                c.roles.retain(|r| r != id);
                            }
                        }
                    }
                });
            }
        }, |ui| {
            create = ui.add_enabled_ui(ok, |ui| crate::ui::widgets::button(ui, &t, crate::ui::widgets::Kind::Primary, tx::CREATE.get(lang))).inner.clicked();
            close = crate::ui::widgets::button(ui, &t, crate::ui::widgets::Kind::Link, tx::CANCEL.get(lang)).clicked();
            if busy {
                ui.spinner();
            }
        });
        create |= enter;
        if create && !busy && valid_name(&c.name).is_some() {
            let overwrites = if c.private { private_overwrites(guild, c.kind, &c.roles, &[], me) } else { Vec::new() };
            let new = NewChannel { name: c.name.clone(), kind: c.kind, parent: c.parent, overwrites, topic: String::new(), nsfw: false, slowmode: 0 };
            self.store.create_channel(guild, new);
        }
        if close || modal.closed {
            self.chan_ui.create = None;
        }
    }

    pub(super) fn clone_channel_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(channel) = self.chan_ui.clone.as_ref().map(|c| c.channel) else { return };
        let Some(original) = self.store.channel_name(channel).map(str::to_string) else {
            self.chan_ui.clone = None;
            return;
        };
        let t = self.theme.clone();
        let lang = self.lang;
        // Sujet repris : on attend les détails (ou leur échec).
        let waiting = self.store.chan_admin.pending(ChanAction::Load) && self.store.chan_admin.details_of(channel).is_none();
        let busy = self.store.chan_admin.pending(ChanAction::Create);
        let (mut go, mut close) = (false, false);
        let Some(c) = self.chan_ui.clone.as_mut() else { return };
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.0);
        let ok = !busy && !waiting && valid_name(&c.name).is_some();
        let modal = crate::ui::widgets::modal(ui.ctx(), &t, Id::unique("cloner-salon"), width, &f::clone_title(lang, &original), crate::i18n::t::CLOSE_TIP.get(lang), |ui| {
            crate::ui::settings::shell::page_style(ui, &t);
            ui.add(egui::Label::new(RichText::new(tx::CLONE_TIP.get(lang)).small().color(t.colors.faint)).wrap());
            ui.label(RichText::new(tx::CHANNEL_NAME.get(lang).to_uppercase()).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
            ui.add(egui::TextEdit::singleline(&mut c.name).char_limit(NAME_MAX).desired_width(f32::INFINITY));
        }, |ui| {
            go = ui.add_enabled_ui(ok, |ui| crate::ui::widgets::button(ui, &t, crate::ui::widgets::Kind::Primary, tx::CLONE.get(lang))).inner.clicked();
            close = crate::ui::widgets::button(ui, &t, crate::ui::widgets::Kind::Link, tx::CANCEL.get(lang)).clicked();
            if busy || waiting {
                ui.spinner();
            }
        });
        if go {
            let name = c.name.clone();
            self.store.clone_channel(channel, &name);
        }
        if close || modal.closed {
            self.chan_ui.clone = None;
        }
    }

    pub(super) fn delete_channel_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(channel) = self.chan_ui.delete.as_ref().map(|d| d.channel) else { return };
        let Some((name, category)) = self.store.guilds.iter().flat_map(|g| g.channels.iter()).find(|c| c.id == channel).map(|c| (c.name.clone(), c.kind == ChannelKind::Category)) else {
            self.chan_ui.delete = None;
            return;
        };
        let t = self.theme.clone();
        let lang = self.lang;
        let busy = self.store.chan_admin.pending(ChanAction::Delete);
        let (mut go, mut close) = (false, false);
        let Some(d) = self.chan_ui.delete.as_mut() else { return };
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.0);
        let ok = !busy && d.typed.trim() == name;
        let modal = crate::ui::widgets::modal(ui.ctx(), &t, Id::unique("supprimer-salon"), width, &f::delete_title(lang, &name), crate::i18n::t::CLOSE_TIP.get(lang), |ui| {
            crate::ui::settings::shell::page_style(ui, &t);
            ui.label(RichText::new(tx::DELETE_IRREVERSIBLE.get(lang)).color(t.colors.danger));
            if category {
                ui.label(tx::DELETE_CATEGORY_NOTE.get(lang));
            }
            ui.label(RichText::new(tx::TYPE_NAME.get(lang)).small().color(t.colors.muted));
            ui.add(egui::TextEdit::singleline(&mut d.typed).hint_text(name.as_str()).desired_width(f32::INFINITY));
        }, |ui| {
            go = ui.add_enabled_ui(ok, |ui| crate::ui::widgets::button(ui, &t, crate::ui::widgets::Kind::Danger, tx::DELETE.get(lang))).inner.clicked();
            close = crate::ui::widgets::button(ui, &t, crate::ui::widgets::Kind::Link, tx::CANCEL.get(lang)).clicked();
            if busy {
                ui.spinner();
            }
        });
        if go {
            self.store.delete_channel(channel);
        }
        if close || modal.closed {
            self.chan_ui.delete = None;
        }
    }
}
