//! Journal d'audit (lot A4) : entrées telles que les rend Get Guild Audit Log
//! (documenté ; types d'action relus sur docs.discord.food le 13 novembre
//! 2026). Les valeurs des changements arrivent déjà mises en texte par le
//! réseau ; l'interface en fait une phrase selon [`kind`].

/// Entrées gardées par serveur, au plus (les plus anciennes sont oubliées).
pub const MAX_ENTRIES: usize = 1000;
/// Taille d'une page (documentation : 1 à 100, 50 par défaut).
pub const PAGE: usize = 50;
/// Changements gardés par entrée, au plus.
pub const MAX_CHANGES: usize = 30;
/// Longueur d'une valeur affichée, au plus (caractères).
pub const VALUE_MAX: usize = 200;

/// Un changement : clé, valeur d'avant, valeur d'après.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub key: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: u64,
    pub action: u32,
    pub user_id: Option<u64>,
    /// Nom de l'auteur (tableau `users` de la réponse), s'il est connu.
    pub user_name: Option<String>,
    pub target_id: Option<u64>,
    /// Nom de la cible quand la réponse le donne (utilisateur, règle, fil…).
    pub target_name: Option<String>,
    pub reason: Option<String>,
    pub changes: Vec<Change>,
    /// `options.count` (messages supprimés, membres déplacés…).
    pub count: Option<u64>,
    /// `options.channel_id`.
    pub channel_id: Option<u64>,
    /// `options.role_name`, `options.auto_moderation_rule_name`…
    pub extra: Option<String>,
}

/// Filtres de la page (`user_id`, `action_type`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Filter {
    pub user_id: Option<u64>,
    pub action: Option<u32>,
}

/// Famille d'une action : choisit la phrase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    GuildUpdate,
    ChannelCreate,
    ChannelUpdate,
    ChannelDelete,
    Overwrite,
    Kick,
    Prune,
    Ban,
    Unban,
    MemberUpdate,
    MemberRoles,
    MemberMove,
    MemberDisconnect,
    BotAdd,
    RoleCreate,
    RoleUpdate,
    RoleDelete,
    Invite,
    Webhook,
    Emoji,
    MessageDelete,
    MessageBulkDelete,
    Pin,
    Unpin,
    Integration,
    Stage,
    Sticker,
    Event,
    Thread,
    Commands,
    Sound,
    AutoModRule,
    AutoModBlock,
    AutoModAlert,
    AutoModTimeout,
    AutoModQuarantine,
    Onboarding,
    Other,
}

/// Types d'action documentés proposés dans le filtre, dans l'ordre de la documentation.
pub const FILTERS: [u32; 44] = [
    1, 10, 11, 12, 13, 14, 15, 20, 21, 22, 23, 24, 25, 26, 27, 28, 30, 31, 32, 40, 41, 42, 50, 51, 52, 60, 61, 62, 72, 73, 74, 75, 80, 90, 100, 110, 111, 112, 140, 141, 142, 143, 144, 145,
];

pub fn kind(action: u32) -> Kind {
    match action {
        1 => Kind::GuildUpdate,
        10 => Kind::ChannelCreate,
        11 => Kind::ChannelUpdate,
        12 => Kind::ChannelDelete,
        13..=15 => Kind::Overwrite,
        20 => Kind::Kick,
        21 => Kind::Prune,
        22 => Kind::Ban,
        23 => Kind::Unban,
        24 => Kind::MemberUpdate,
        25 => Kind::MemberRoles,
        26 => Kind::MemberMove,
        27 => Kind::MemberDisconnect,
        28 => Kind::BotAdd,
        30 => Kind::RoleCreate,
        31 => Kind::RoleUpdate,
        32 => Kind::RoleDelete,
        40..=42 => Kind::Invite,
        50..=52 => Kind::Webhook,
        60..=62 => Kind::Emoji,
        72 => Kind::MessageDelete,
        73 => Kind::MessageBulkDelete,
        74 => Kind::Pin,
        75 => Kind::Unpin,
        80..=82 => Kind::Integration,
        83..=85 => Kind::Stage,
        90..=92 => Kind::Sticker,
        100..=102 | 200..=202 => Kind::Event,
        110..=112 => Kind::Thread,
        121 => Kind::Commands,
        130..=132 => Kind::Sound,
        140..=142 => Kind::AutoModRule,
        143 => Kind::AutoModBlock,
        144 => Kind::AutoModAlert,
        145 => Kind::AutoModTimeout,
        146 => Kind::AutoModQuarantine,
        163..=167 => Kind::Onboarding,
        _ => Kind::Other,
    }
}

/// Création, modification ou suppression, d'après le dernier chiffre du
/// type (convention de la documentation : …0 créer, …1 modifier, …2 supprimer).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Create,
    Update,
    Delete,
}

pub fn verb(action: u32) -> Verb {
    match action {
        13 | 83 | 100 | 200 | 163 | 166 => Verb::Create,
        15 | 85 | 102 | 202 | 165 => Verb::Delete,
        14 | 84 | 101 | 201 | 164 | 167 => Verb::Update,
        a => match a % 10 {
            0 => Verb::Create,
            2 => Verb::Delete,
            _ => Verb::Update,
        },
    }
}

/// Valeur d'un changement en texte borné (le réseau l'appelle une fois à la réception).
pub fn clip(s: &str) -> String {
    if s.chars().count() <= VALUE_MAX {
        return s.to_string();
    }
    let mut out: String = s.chars().take(VALUE_MAX).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_verbs() {
        assert_eq!(kind(22), Kind::Ban);
        assert_eq!(kind(145), Kind::AutoModTimeout);
        assert_eq!(kind(9999), Kind::Other);
        assert_eq!(verb(40), Verb::Create);
        assert_eq!(verb(42), Verb::Delete);
        assert_eq!(verb(13), Verb::Create);
        assert_eq!(verb(15), Verb::Delete);
        assert_eq!(verb(111), Verb::Update);
        assert!(FILTERS.iter().all(|a| kind(*a) != Kind::Other));
        assert_eq!(clip(&"é".repeat(300)).chars().count(), VALUE_MAX + 1);
    }
}
