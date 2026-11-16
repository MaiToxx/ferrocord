//! Messages système (lot T13) : une ligne « icône, phrase, heure » à la
//! place du message, cliquable quand elle vise quelque chose (message
//! épinglé, fil créé, sondage terminé, salon suivi), et l'en-tête
//! « X a utilisé /commande » des réponses d'application.
//!
//! La phrase est faite par le magasin (`state::system_msgs`) et rangée
//! dans le texte du message : elle est montrée telle quelle, sans mise en
//! forme (un nom avec des `*` ne devient pas de l'italique).

use super::msg_actions::{self, Acts, Glyph, Hit};
use super::polls::ExtraHit;
use crate::i18n::table::polls::f;
use crate::i18n::{time, Lang};
use crate::state::model::Message;
use crate::state::SystemKind;
use crate::theme::{icons::Icon, Clock, Tokens};
use eframe::egui::{self, RichText, Sense, Vec2};

enum Mark {
    Icon(Icon),
    Glyph(Glyph),
}

fn mark(kind: SystemKind) -> Mark {
    use SystemKind::*;
    match kind {
        Pinned => Mark::Glyph(Glyph::Pin),
        ThreadCreated => Mark::Icon(Icon::Thread),
        PollResult => Mark::Icon(Icon::Poll),
        FollowAdd => Mark::Icon(Icon::Megaphone),
        Call => Mark::Icon(Icon::Phone),
        ChannelName | ChannelIcon => Mark::Glyph(Glyph::Edit),
        RecipientRemove => Mark::Icon(Icon::Logout),
        AutoMod => Mark::Icon(Icon::Lock),
        _ => Mark::Icon(Icon::ArrowRight),
    }
}

/// Phrase d'un appel coupée autour du nom de son auteur : (avant, après).
fn call_parts(m: &Message) -> Option<(&str, &str)> {
    if m.author.is_empty() {
        return None;
    }
    m.content.split_once(m.author.as_str())
}

/// Ligne d'un message système. `ts` : heure du message.
pub(super) fn system_line(ui: &mut egui::Ui, t: &Tokens, lang: Lang, m: &Message, ts: i64, acts: &mut Acts) {
    let Some(kind) = m.system else { return };
    let h24 = t.layout.clock == Clock::H24;
    let target = m.meta.extra.as_ref().and_then(|x| x.target);
    // Clic utile seulement si la cible est dans un salon que je connais.
    let target = target.filter(|r| r.channel_id.is_none_or(|c| acts.store.channel_exists(c)));
    let color = if target.is_some() { t.colors.text } else { t.colors.muted };
    let side = Tokens::icon_side(t.typography.body);
    // Appel : combiné vert, rouge quand je l'ai manqué (comme dans le client).
    let call = kind == SystemKind::Call;
    let missed = call && crate::state::system_msgs::call_missed(m, acts.store.me.as_ref().map(|u| u.id));
    let ink = match (call, missed) {
        (true, true) => t.colors.danger,
        (true, false) => t.colors.online,
        _ => t.colors.faint,
    };
    let paint = |ui: &egui::Ui, rect: egui::Rect| match mark(kind) {
        Mark::Icon(i) => crate::theme::icons::paint(i, ui.painter(), rect, ink),
        Mark::Glyph(g) => msg_actions::paint_glyph(g, ui.painter(), rect, ink),
    };
    // Rend le milieu de la première ligne de la phrase : egui lui donne la
    // hauteur de la rangée (celle d'un contrôle) ; une étiquette simple y est
    // centrée, la phrase composée d'un appel posée en bas.
    let words = |ui: &mut egui::Ui| -> f32 {
        let cursor = ui.cursor();
        let line = ui.text_style_height(&egui::TextStyle::Body);
        let high = cursor.height().max(line);
        let mid = cursor.top() + if call_parts(m).is_some() && call { high - line / 2.0 } else { high / 2.0 };
        match call_parts(m).filter(|_| call) {
            // Le nom de l'auteur dans la police de son style de nom, le reste en gris.
            // Une seule phrase (même ligne de base, renvoi à la ligne propre).
            Some((before, after)) => {
                let worn = super::cosmetics::author(ui.ctx(), m.author_id);
                let body = egui::TextStyle::Body.resolve(ui.style());
                let name = super::cosmetics::name_font(ui.ctx(), body.size, worn.look.name_style());
                let rest = egui::TextFormat { font_id: body, color: t.colors.muted, ..Default::default() };
                let mut job = egui::text::LayoutJob::default();
                job.append(before, 0.0, rest.clone());
                job.append(&m.author, 0.0, egui::TextFormat { font_id: name, color: super::cosmetics::author_color(ui.ctx(), t, m.author_id, &worn), ..Default::default() });
                job.append(after, 0.0, rest);
                // L'heure courte des messages (« 16:28 », « Hier à 00:04 »), à droite du texte.
                let small = egui::TextStyle::Small.resolve(ui.style());
                job.append(&time::message(lang, ts, h24), t.spacing.item_x, egui::TextFormat { font_id: small, color: t.colors.faint, ..Default::default() });
                super::emoji::label_job(ui, job, false);
            }
            None => {
                ui.label(RichText::new(&m.content).color(color));
                ui.label(RichText::new(time::clock(lang, ts, true, h24)).small().color(t.colors.faint));
            }
        }
        mid
    };
    // Avatars affichés : l'icône au milieu de leur colonne, la phrase (et ses
    // lignes renvoyées) sur la colonne du texte des messages.
    if t.message_avatars() {
        let origin = ui.cursor().min;
        egui::Frame::new().inner_margin(t.avatar_indent()).show(ui, |ui| {
            let row = ui.horizontal_wrapped(words);
            // L'icône à la hauteur de la première ligne de la phrase.
            let at = egui::pos2(origin.x + (t.layout.avatar_size - side) / 2.0, row.inner - side / 2.0);
            paint(ui, super::util::pixel_square(ui, at, side));
            let row = row.response.rect;
            if let Some(r) = target {
                acts.hits.push((row, Hit::Extra(ExtraHit::Target(r))));
            }
            automod_alert(ui, t, lang, m, acts.store);
            super::reactions::pills(ui, t, m, acts);
        });
        return;
    }
    let row = ui
        .horizontal_wrapped(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
            paint(ui, rect);
            words(ui);
        })
        .response
        .rect;
    if let Some(r) = target {
        acts.hits.push((row, Hit::Extra(ExtraHit::Target(r))));
    }
    automod_alert(ui, t, lang, m, acts.store);
    super::reactions::pills(ui, t, m, acts);
}

/// Ce qu'une alerte AutoMod dit dans son embed (`auto_moderation_message`,
/// forme relevée dans discord-api-types et docs.discord.food : champs
/// `rule_name`, `keyword`, `channel_id`, `flagged_message_id` ; la
/// description est le texte en cause). Rien n'est demandé à Discord.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Alert {
    pub rule: Option<String>,
    pub keyword: Option<String>,
    pub content: Option<String>,
    /// Message laissé en place (alerte sans blocage) : (salon, message).
    pub flagged: Option<(u64, u64)>,
}

pub(super) fn alert_of(m: &Message) -> Option<Alert> {
    if m.system != Some(SystemKind::AutoMod) {
        return None;
    }
    m.embeds.first().map(alert_in)
}

fn alert_in(e: &crate::state::model::Embed) -> Alert {
    let field = |name: &str| e.fields.iter().find(|f| f.name == name).map(|f| f.value.trim().to_string()).filter(|v| !v.is_empty());
    let id = |name: &str| field(name).and_then(|v| v.parse::<u64>().ok()).filter(|v| *v != 0);
    Alert {
        rule: field("rule_name"),
        keyword: field("keyword"),
        content: e.description.as_deref().map(str::trim).filter(|d| !d.is_empty()).map(str::to_string),
        flagged: id("channel_id").zip(id("flagged_message_id")),
    }
}

/// Geste fait sur une alerte AutoMod, repris par `moderation_ui` à l'image suivante.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AlertPick {
    Jump { channel: u64, message: u64 },
    Member { guild: u64, user: u64, name: String, kind: super::moderation::ModKind },
    Rules(u64),
}

fn alert_pick_id() -> egui::Id {
    egui::Id::unique("ferrocord.automod.alerte")
}

pub(super) fn take_alert_pick(ctx: &egui::Context) -> Option<AlertPick> {
    ctx.data_mut(|d| {
        let pick = d.get_temp::<AlertPick>(alert_pick_id());
        d.remove::<AlertPick>(alert_pick_id());
        pick
    })
}

/// Détail d'une alerte AutoMod et ses actions : voir le message, exclure,
/// expulser ou bannir son auteur (selon mes droits), ouvrir les règles.
fn automod_alert(ui: &mut egui::Ui, t: &Tokens, lang: Lang, m: &Message, store: &crate::state::Store) {
    use crate::i18n::table::lotv::{f as lf, t as lt};
    let Some(a) = alert_of(m) else { return };
    let mut pick = None;
    egui::Frame::new().fill(t.card_fill()).stroke(t.border()).corner_radius(t.r_card()).inner_margin(t.spacing.item_x).show(ui, |ui| {
        ui.set_max_width(ui.available_width().min(t.layout.card_width * 1.4));
        ui.label(RichText::new(&m.author).family(super::fonts::demi()).color(t.colors.text_strong));
        if let Some(c) = &a.content {
            ui.add(egui::Label::new(RichText::new(c).color(t.colors.text)).wrap());
        }
        ui.horizontal_wrapped(|ui| {
            if let Some(r) = &a.rule {
                ui.label(RichText::new(lf::alert_rule(lang, r)).small().color(t.colors.muted));
            }
            if let Some(k) = &a.keyword {
                ui.label(RichText::new(lf::alert_keyword(lang, k)).small().color(t.colors.muted));
            }
        });
        ui.horizontal_wrapped(|ui| {
            if let Some((channel, message)) = a.flagged.filter(|(c, _)| store.channel_exists(*c))
                && ui.add(super::form::btn(t, lt::ALERT_JUMP.get(lang))).clicked()
            {
                pick = Some(AlertPick::Jump { channel, message });
            }
            let Some(guild) = m.guild_id else { return };
            for kind in super::moderation::picks_for(store, guild, m.author_id) {
                let label = kind.label().get(lang);
                if ui.add(if kind.danger() { super::form::danger(t, label) } else { super::form::btn(t, label) }).clicked() {
                    pick = Some(AlertPick::Member { guild, user: m.author_id, name: m.author.clone(), kind });
                }
            }
            if store.guild_has(guild, crate::state::Permissions::MANAGE_GUILD) && ui.add(super::form::link(t, lt::ALERT_RULES.get(lang))).clicked() {
                pick = Some(AlertPick::Rules(guild));
            }
        });
    });
    if let Some(p) = pick {
        ui.ctx().data_mut(|d| d.insert_temp(alert_pick_id(), p));
        ui.ctx().request_repaint();
    }
}

/// « X a utilisé /commande » au-dessus d'une réponse d'application.
pub(super) fn command_header(ui: &mut egui::Ui, t: &Tokens, lang: Lang, m: &Message) {
    let Some(c) = m.meta.extra.as_ref().and_then(|x| x.command.as_ref()) else { return };
    // Déjà dit par l'en-tête du lot T11 (`components::header`) : une seule ligne.
    if m.meta.app.as_deref().and_then(|a| a.interaction.as_ref()).is_some_and(|i| i.command.is_some()) {
        return;
    }
    let command = if c.name.is_empty() {
        String::new()
    } else if c.slash {
        format!("/{}", c.name)
    } else {
        c.name.clone()
    };
    let text = if command.is_empty() { c.user.clone() } else { f::command_used(lang, &c.user, &command) };
    ui.horizontal(|ui| {
        msg_actions::glyph(ui, Glyph::Reply, t.typography.small, t.colors.faint);
        ui.add(egui::Label::new(RichText::new(text).size(t.typography.small).color(t.colors.muted)).truncate());
    });
}

#[cfg(test)]
mod alert_tests {
    use super::*;
    use crate::state::model::{Embed, EmbedField};

    fn embed(fields: &[(&str, &str)]) -> Embed {
        let fields = fields.iter().map(|(n, v)| EmbedField { name: (*n).into(), value: (*v).into(), inline: false }).collect();
        Embed { description: Some("  du texte interdit ".into()), fields, ..Default::default() }
    }

    #[test]
    fn une_alerte_automod_est_lue_dans_son_embed() {
        let a = alert_in(&embed(&[("rule_name", "Gros mots"), ("keyword", "interdit"), ("channel_id", "5"), ("flagged_message_id", "77"), ("inconnu", "x")]));
        assert_eq!(a, Alert { rule: Some("Gros mots".into()), keyword: Some("interdit".into()), content: Some("du texte interdit".into()), flagged: Some((5, 77)) });
        // Message bloqué : rien à aller voir ; identifiant illisible : pas de saut.
        assert_eq!(alert_in(&embed(&[("channel_id", "5")])).flagged, None);
        assert_eq!(alert_in(&embed(&[("channel_id", "abc"), ("flagged_message_id", "77")])).flagged, None);
        assert_eq!(alert_in(&Embed::default()), Alert::default());
    }
}
