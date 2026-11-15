//! Onglet « Permissions » de l'éditeur de rôle : toutes les permissions
//! documentées, groupées et décrites comme dans Discord, avec recherche.
//! Une permission que je n'ai pas ne peut pas être cochée (Discord refuserait).

use crate::i18n::table::roles::t as rt;
use crate::i18n::{Lang, Text};
use crate::state::Permissions;
use crate::theme::Tokens;
use eframe::egui::{self, RichText};

macro_rules! group {
    ($title:ident: $($p:ident),* $(,)?) => {
        (rt::$title, &[$((Permissions::$p, paste_name!($p), paste_desc!($p))),*])
    };
}

// Noms des textes : `P_<NOM>` et `D_<NOM>` (écrits à la main, sans dépendance).
macro_rules! paste_name {
    ($p:ident) => {
        names::$p.0
    };
}
macro_rules! paste_desc {
    ($p:ident) => {
        names::$p.1
    };
}

/// (nom, description) de chaque permission.
#[allow(non_upper_case_globals)]
mod names {
    use crate::i18n::table::roles::t::*;
    use crate::i18n::Text;

    macro_rules! pairs {
        ($($p:ident = $n:ident, $d:ident;)*) => { $(pub const $p: (Text, Text) = ($n, $d);)* };
    }

    pairs! {
        VIEW_CHANNEL = P_VIEW_CHANNEL, D_VIEW_CHANNEL;
        MANAGE_CHANNELS = P_MANAGE_CHANNELS, D_MANAGE_CHANNELS;
        MANAGE_ROLES = P_MANAGE_ROLES, D_MANAGE_ROLES;
        CREATE_GUILD_EXPRESSIONS = P_CREATE_GUILD_EXPRESSIONS, D_CREATE_GUILD_EXPRESSIONS;
        MANAGE_GUILD_EXPRESSIONS = P_MANAGE_GUILD_EXPRESSIONS, D_MANAGE_GUILD_EXPRESSIONS;
        VIEW_AUDIT_LOG = P_VIEW_AUDIT_LOG, D_VIEW_AUDIT_LOG;
        VIEW_GUILD_INSIGHTS = P_VIEW_GUILD_INSIGHTS, D_VIEW_GUILD_INSIGHTS;
        VIEW_CREATOR_MONETIZATION_ANALYTICS = P_VIEW_CREATOR_MONETIZATION_ANALYTICS, D_VIEW_CREATOR_MONETIZATION_ANALYTICS;
        MANAGE_WEBHOOKS = P_MANAGE_WEBHOOKS, D_MANAGE_WEBHOOKS;
        MANAGE_GUILD = P_MANAGE_GUILD, D_MANAGE_GUILD;
        CREATE_INSTANT_INVITE = P_CREATE_INSTANT_INVITE, D_CREATE_INSTANT_INVITE;
        CHANGE_NICKNAME = P_CHANGE_NICKNAME, D_CHANGE_NICKNAME;
        MANAGE_NICKNAMES = P_MANAGE_NICKNAMES, D_MANAGE_NICKNAMES;
        KICK_MEMBERS = P_KICK_MEMBERS, D_KICK_MEMBERS;
        BAN_MEMBERS = P_BAN_MEMBERS, D_BAN_MEMBERS;
        MODERATE_MEMBERS = P_MODERATE_MEMBERS, D_MODERATE_MEMBERS;
        SEND_MESSAGES = P_SEND_MESSAGES, D_SEND_MESSAGES;
        SEND_MESSAGES_IN_THREADS = P_SEND_MESSAGES_IN_THREADS, D_SEND_MESSAGES_IN_THREADS;
        CREATE_PUBLIC_THREADS = P_CREATE_PUBLIC_THREADS, D_CREATE_PUBLIC_THREADS;
        CREATE_PRIVATE_THREADS = P_CREATE_PRIVATE_THREADS, D_CREATE_PRIVATE_THREADS;
        EMBED_LINKS = P_EMBED_LINKS, D_EMBED_LINKS;
        ATTACH_FILES = P_ATTACH_FILES, D_ATTACH_FILES;
        ADD_REACTIONS = P_ADD_REACTIONS, D_ADD_REACTIONS;
        USE_EXTERNAL_EMOJIS = P_USE_EXTERNAL_EMOJIS, D_USE_EXTERNAL_EMOJIS;
        USE_EXTERNAL_STICKERS = P_USE_EXTERNAL_STICKERS, D_USE_EXTERNAL_STICKERS;
        MENTION_EVERYONE = P_MENTION_EVERYONE, D_MENTION_EVERYONE;
        MANAGE_MESSAGES = P_MANAGE_MESSAGES, D_MANAGE_MESSAGES;
        PIN_MESSAGES = P_PIN_MESSAGES, D_PIN_MESSAGES;
        BYPASS_SLOWMODE = P_BYPASS_SLOWMODE, D_BYPASS_SLOWMODE;
        MANAGE_THREADS = P_MANAGE_THREADS, D_MANAGE_THREADS;
        READ_MESSAGE_HISTORY = P_READ_MESSAGE_HISTORY, D_READ_MESSAGE_HISTORY;
        SEND_TTS_MESSAGES = P_SEND_TTS_MESSAGES, D_SEND_TTS_MESSAGES;
        SEND_VOICE_MESSAGES = P_SEND_VOICE_MESSAGES, D_SEND_VOICE_MESSAGES;
        SEND_POLLS = P_SEND_POLLS, D_SEND_POLLS;
        CONNECT = P_CONNECT, D_CONNECT;
        SPEAK = P_SPEAK, D_SPEAK;
        STREAM = P_STREAM, D_STREAM;
        USE_SOUNDBOARD = P_USE_SOUNDBOARD, D_USE_SOUNDBOARD;
        USE_EXTERNAL_SOUNDS = P_USE_EXTERNAL_SOUNDS, D_USE_EXTERNAL_SOUNDS;
        USE_VAD = P_USE_VAD, D_USE_VAD;
        PRIORITY_SPEAKER = P_PRIORITY_SPEAKER, D_PRIORITY_SPEAKER;
        MUTE_MEMBERS = P_MUTE_MEMBERS, D_MUTE_MEMBERS;
        DEAFEN_MEMBERS = P_DEAFEN_MEMBERS, D_DEAFEN_MEMBERS;
        MOVE_MEMBERS = P_MOVE_MEMBERS, D_MOVE_MEMBERS;
        SET_VOICE_CHANNEL_STATUS = P_SET_VOICE_CHANNEL_STATUS, D_SET_VOICE_CHANNEL_STATUS;
        USE_APPLICATION_COMMANDS = P_USE_APPLICATION_COMMANDS, D_USE_APPLICATION_COMMANDS;
        USE_EMBEDDED_ACTIVITIES = P_USE_EMBEDDED_ACTIVITIES, D_USE_EMBEDDED_ACTIVITIES;
        USE_EXTERNAL_APPS = P_USE_EXTERNAL_APPS, D_USE_EXTERNAL_APPS;
        REQUEST_TO_SPEAK = P_REQUEST_TO_SPEAK, D_REQUEST_TO_SPEAK;
        CREATE_EVENTS = P_CREATE_EVENTS, D_CREATE_EVENTS;
        MANAGE_EVENTS = P_MANAGE_EVENTS, D_MANAGE_EVENTS;
        ADMINISTRATOR = P_ADMINISTRATOR, D_ADMINISTRATOR;
    }
}

type Group = (Text, &'static [(Permissions, Text, Text)]);

/// Groupes dans l'ordre de Discord.
const GROUPS: &[Group] = &[
    group!(GROUP_GENERAL: VIEW_CHANNEL, MANAGE_CHANNELS, MANAGE_ROLES, CREATE_GUILD_EXPRESSIONS, MANAGE_GUILD_EXPRESSIONS, VIEW_AUDIT_LOG, VIEW_GUILD_INSIGHTS, VIEW_CREATOR_MONETIZATION_ANALYTICS, MANAGE_WEBHOOKS, MANAGE_GUILD),
    group!(GROUP_MEMBERSHIP: CREATE_INSTANT_INVITE, CHANGE_NICKNAME, MANAGE_NICKNAMES, KICK_MEMBERS, BAN_MEMBERS, MODERATE_MEMBERS),
    group!(GROUP_TEXT: SEND_MESSAGES, SEND_MESSAGES_IN_THREADS, CREATE_PUBLIC_THREADS, CREATE_PRIVATE_THREADS, EMBED_LINKS, ATTACH_FILES, ADD_REACTIONS, USE_EXTERNAL_EMOJIS, USE_EXTERNAL_STICKERS, MENTION_EVERYONE, MANAGE_MESSAGES, PIN_MESSAGES, BYPASS_SLOWMODE, MANAGE_THREADS, READ_MESSAGE_HISTORY, SEND_TTS_MESSAGES, SEND_VOICE_MESSAGES, SEND_POLLS),
    group!(GROUP_VOICE: CONNECT, SPEAK, STREAM, USE_SOUNDBOARD, USE_EXTERNAL_SOUNDS, USE_VAD, PRIORITY_SPEAKER, MUTE_MEMBERS, DEAFEN_MEMBERS, MOVE_MEMBERS, SET_VOICE_CHANNEL_STATUS),
    group!(GROUP_APPS: USE_APPLICATION_COMMANDS, USE_EMBEDDED_ACTIVITIES, USE_EXTERNAL_APPS),
    group!(GROUP_STAGE: REQUEST_TO_SPEAK),
    group!(GROUP_EVENTS: CREATE_EVENTS, MANAGE_EVENTS),
    group!(GROUP_ADVANCED: ADMINISTRATOR),
];

/// Permissions documentées (toutes celles des groupes).
fn documented() -> Permissions {
    GROUPS.iter().flat_map(|(_, list)| list.iter()).fold(Permissions::NONE, |acc, (p, _, _)| acc | *p)
}

/// Liste des permissions de `perms` ; `grantable` : celles que je peux donner ou retirer.
pub(super) fn show(ui: &mut egui::Ui, t: &Tokens, lang: Lang, perms: &mut Permissions, grantable: Permissions, editable: bool, search: &mut String) {
    crate::ui::mobile::row(ui, |ui| {
        let w = (ui.available_width() * 0.6).max(t.layout.button_height);
        ui.add(egui::TextEdit::singleline(search).hint_text(rt::SEARCH_PERMISSIONS.get(lang)).desired_width(w));
        // Efface ce que je peux retirer ; le reste et les bits inconnus restent.
        let clearable = *perms & grantable & documented();
        if editable && ui.add_enabled(clearable != Permissions::NONE, crate::ui::form::btn(t, rt::CLEAR_PERMISSIONS.get(lang))).clicked() {
            *perms = *perms & !clearable;
        }
    });
    if perms.contains(Permissions::ADMINISTRATOR) {
        ui.add_space(t.spacing.item_y);
        ui.add(egui::Label::new(RichText::new(rt::ADMIN_WARNING.get(lang)).color(t.colors.danger)).wrap());
    }
    let q = search.trim().to_lowercase();
    let mut any = false;
    for (title, list) in GROUPS {
        let shown: Vec<_> = list.iter().filter(|(_, n, d)| super::roles::matches(n.get(lang), &q) || super::roles::matches(d.get(lang), &q)).collect();
        if shown.is_empty() {
            continue;
        }
        any = true;
        ui.add_space(t.spacing.category_gap);
        ui.label(RichText::new(title.get(lang).to_uppercase()).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
        for (p, name, desc) in shown {
            let can = editable && grantable.contains(*p);
            let mut on = perms.contains(*p);
            ui.add_space(t.spacing.item_y);
            ui.horizontal(|ui| {
                let label = RichText::new(name.get(lang)).color(t.colors.text_strong);
                let resp = ui.add_enabled(can, egui::Checkbox::new(&mut on, label));
                if editable && !grantable.contains(*p) {
                    resp.on_disabled_hover_text(rt::NOT_GRANTABLE.get(lang));
                }
            });
            ui.add(egui::Label::new(RichText::new(desc.get(lang)).small().color(t.colors.muted)).wrap());
            if on != perms.contains(*p) {
                *perms = if on { *perms | *p } else { *perms & !*p };
            }
        }
    }
    if !any {
        ui.label(RichText::new(rt::NO_PERMISSION_FOUND.get(lang)).color(t.colors.muted));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_permission_once() {
        let mut seen = 0u64;
        let mut n = 0;
        for (_, list) in GROUPS {
            for (p, _, _) in *list {
                assert_eq!(seen & p.0, 0, "en double");
                seen |= p.0;
                n += 1;
            }
        }
        assert_eq!(n, Permissions::NAMED.len());
        for (name, p) in Permissions::NAMED {
            assert!(seen & p.0 != 0, "{name} absente");
        }
        let sizes: Vec<usize> = GROUPS.iter().map(|(_, l)| l.len()).collect();
        assert_eq!(sizes, [10, 6, 18, 11, 3, 1, 2, 1]);
    }
}
