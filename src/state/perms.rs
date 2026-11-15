//! Permissions de Discord : champ de bits, constantes nommées et calcul des
//! permissions effectives d'un membre dans un salon, selon l'algorithme
//! public (https://discord.com/developers/docs/topics/permissions).
//!
//! Tout est pur : aucune horloge lue ici, l'instant est un paramètre.

use super::model::{Channel, Guild, Member, Overwrite, OverwriteKind, Role};
use std::ops::{BitAnd, BitOr, Not};

/// Champ de bits de permissions. Discord le transmet en chaîne décimale ;
/// seuls les 64 bits de poids faible sont gardés (les bits connus s'arrêtent
/// bien avant), une valeur plus large est donc lue sans erreur.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Permissions(pub u64);

macro_rules! flags {
    ($($name:ident = $bit:expr;)*) => {
        impl Permissions {
            $(pub const $name: Permissions = Permissions(1 << $bit);)*
            /// Toutes les permissions nommées, avec leur nom (pour les tests et l'affichage futur).
            pub const NAMED: &'static [(&'static str, Permissions)] = &[$((stringify!($name), Permissions::$name)),*];
        }
    };
}

flags! {
    CREATE_INSTANT_INVITE = 0;
    KICK_MEMBERS = 1;
    BAN_MEMBERS = 2;
    ADMINISTRATOR = 3;
    MANAGE_CHANNELS = 4;
    MANAGE_GUILD = 5;
    ADD_REACTIONS = 6;
    VIEW_AUDIT_LOG = 7;
    PRIORITY_SPEAKER = 8;
    STREAM = 9;
    VIEW_CHANNEL = 10;
    SEND_MESSAGES = 11;
    SEND_TTS_MESSAGES = 12;
    MANAGE_MESSAGES = 13;
    EMBED_LINKS = 14;
    ATTACH_FILES = 15;
    READ_MESSAGE_HISTORY = 16;
    MENTION_EVERYONE = 17;
    USE_EXTERNAL_EMOJIS = 18;
    VIEW_GUILD_INSIGHTS = 19;
    CONNECT = 20;
    SPEAK = 21;
    MUTE_MEMBERS = 22;
    DEAFEN_MEMBERS = 23;
    MOVE_MEMBERS = 24;
    USE_VAD = 25;
    CHANGE_NICKNAME = 26;
    MANAGE_NICKNAMES = 27;
    MANAGE_ROLES = 28;
    MANAGE_WEBHOOKS = 29;
    MANAGE_GUILD_EXPRESSIONS = 30;
    USE_APPLICATION_COMMANDS = 31;
    REQUEST_TO_SPEAK = 32;
    MANAGE_EVENTS = 33;
    MANAGE_THREADS = 34;
    CREATE_PUBLIC_THREADS = 35;
    CREATE_PRIVATE_THREADS = 36;
    USE_EXTERNAL_STICKERS = 37;
    SEND_MESSAGES_IN_THREADS = 38;
    USE_EMBEDDED_ACTIVITIES = 39;
    MODERATE_MEMBERS = 40;
    VIEW_CREATOR_MONETIZATION_ANALYTICS = 41;
    USE_SOUNDBOARD = 42;
    CREATE_GUILD_EXPRESSIONS = 43;
    CREATE_EVENTS = 44;
    USE_EXTERNAL_SOUNDS = 45;
    SEND_VOICE_MESSAGES = 46;
    SET_VOICE_CHANNEL_STATUS = 48;
    SEND_POLLS = 49;
    USE_EXTERNAL_APPS = 50;
    PIN_MESSAGES = 51;
    BYPASS_SLOWMODE = 52;
}

impl Permissions {
    pub const NONE: Permissions = Permissions(0);
    /// Propriétaire ou administrateur : tout, y compris les bits pas encore connus.
    pub const ALL: Permissions = Permissions(u64::MAX);

    /// Lit la chaîne décimale de l'API. Une valeur plus large que 64 bits
    /// garde ses 64 bits de poids faible (calcul modulo 2^64) ; tout autre
    /// texte (vide, signe, lettres) est refusé.
    pub fn parse(s: &str) -> Option<Permissions> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let v = s.bytes().fold(0u64, |acc, b| acc.wrapping_mul(10).wrapping_add(u64::from(b - b'0')));
        Some(Permissions(v))
    }

    pub fn contains(self, other: Permissions) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn intersects(self, other: Permissions) -> bool {
        self.0 & other.0 != 0
    }
}

impl BitOr for Permissions {
    type Output = Permissions;
    fn bitor(self, o: Permissions) -> Permissions {
        Permissions(self.0 | o.0)
    }
}

impl BitAnd for Permissions {
    type Output = Permissions;
    fn bitand(self, o: Permissions) -> Permissions {
        Permissions(self.0 & o.0)
    }
}

impl Not for Permissions {
    type Output = Permissions;
    fn not(self) -> Permissions {
        Permissions(!self.0)
    }
}

/// Réponse à « ai-je cette permission ici ? ».
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Permis,
    Refuse,
    /// Informations absentes ou incomplètes : l'app se comporte comme si
    /// tout était permis et laisse Discord répondre (403 au besoin).
    Inconnu,
}

/// Ce qu'une exclusion temporaire laisse.
const TIMEOUT_KEEPS: Permissions = Permissions(Permissions::VIEW_CHANNEL.0 | Permissions::READ_MESSAGE_HISTORY.0);

/// Ce qui n'a pas de sens sans SEND_MESSAGES. Les quatre premiers viennent
/// de la documentation ; messages vocaux et sondages sont des messages.
const NEEDS_SEND: Permissions = Permissions(
    Permissions::MENTION_EVERYONE.0
        | Permissions::SEND_TTS_MESSAGES.0
        | Permissions::ATTACH_FILES.0
        | Permissions::EMBED_LINKS.0
        | Permissions::SEND_VOICE_MESSAGES.0
        | Permissions::SEND_POLLS.0,
);

/// Permissions de base dans le serveur (@everyone + rôles du membre), ou
/// `None` si une information manque : membre inconnu, rôle @everyone absent,
/// ou rôle du membre absent de la liste du serveur (incohérence : on ne
/// devine pas).
pub fn base_permissions(guild: &Guild, me: u64) -> Option<Permissions> {
    // Propriétaire inconnu : je le suis peut-être, rien n'est supposé.
    let owner = guild.owner_id?;
    if owner == me {
        return Some(Permissions::ALL);
    }
    if guild.roles_unreadable {
        return None;
    }
    let member = guild.me.as_ref()?;
    let role = |id: u64| guild.roles.iter().find(|r| r.id == id).map(|r| r.permissions);
    let mut perms = role(guild.id)?;
    for id in &member.roles {
        perms = perms | role(*id)?;
    }
    Some(if perms.contains(Permissions::ADMINISTRATOR) { Permissions::ALL } else { perms })
}

/// Applique les dérogations d'un salon : @everyone, puis tous les rôles
/// ensemble (refus puis accords), puis le membre. Les dérogations sont
/// celles du salon tel que l'API le transmet : un salon synchronisé avec sa
/// catégorie porte déjà les mêmes dérogations qu'elle, Discord n'applique
/// aucun autre héritage.
pub fn apply_overwrites(base: Permissions, guild_id: u64, me: u64, member: &Member, overwrites: &[Overwrite]) -> Permissions {
    if base.contains(Permissions::ADMINISTRATOR) {
        return Permissions::ALL;
    }
    let mut perms = base;
    if let Some(o) = overwrites.iter().find(|o| o.kind == OverwriteKind::Role && o.id == guild_id) {
        perms = (perms & !o.deny) | o.allow;
    }
    let (mut allow, mut deny) = (Permissions::NONE, Permissions::NONE);
    for o in overwrites.iter().filter(|o| o.kind == OverwriteKind::Role && o.id != guild_id && member.roles.contains(&o.id)) {
        allow = allow | o.allow;
        deny = deny | o.deny;
    }
    perms = (perms & !deny) | allow;
    if let Some(o) = overwrites.iter().find(|o| o.kind == OverwriteKind::Member && o.id == me) {
        perms = (perms & !o.deny) | o.allow;
    }
    perms
}

/// Permissions effectives de `me` dans `channel` à l'instant `now_ms`
/// (ms Unix), ou `None` si on ne peut pas les connaître.
pub fn channel_permissions(guild: &Guild, me: u64, channel: &Channel, now_ms: i64) -> Option<Permissions> {
    with_base(guild, me, base_permissions(guild, me)?, channel, now_ms)
}

/// Comme [`channel_permissions`], les permissions de base (calculées une
/// fois par serveur) étant fournies.
pub fn with_base(guild: &Guild, me: u64, base: Permissions, channel: &Channel, now_ms: i64) -> Option<Permissions> {
    if base == Permissions::ALL {
        // Propriétaire ou administrateur : ni dérogation ni exclusion ne s'appliquent.
        return Some(Permissions::ALL);
    }
    let member = guild.me.as_ref()?;
    let overwrites = channel.overwrites.as_deref()?;
    let mut perms = apply_overwrites(base, guild.id, me, member, overwrites);
    if member.timed_out(now_ms) {
        perms = perms & TIMEOUT_KEEPS;
    }
    Some(implicit(perms))
}

/// Règles implicites : sans VIEW_CHANNEL rien n'est permis, sans
/// SEND_MESSAGES ce qui en dépend ne l'est pas non plus.
pub fn implicit(perms: Permissions) -> Permissions {
    if !perms.contains(Permissions::VIEW_CHANNEL) {
        return Permissions::NONE;
    }
    if !perms.contains(Permissions::SEND_MESSAGES) {
        return perms & !NEEDS_SEND;
    }
    perms
}

// ---------------------------------------------------------------- hiérarchie des rôles (lot A3)

/// Rang d'un rôle : sa position, puis, à position égale, le plus ancien
/// (plus petit identifiant) au-dessus, comme Discord les trie.
pub type Rank = (i32, std::cmp::Reverse<u64>);

pub fn role_rank(r: &Role) -> Rank {
    (r.position, std::cmp::Reverse(r.id))
}

/// Rang du plus haut rôle d'une liste (rôles inconnus ignorés) ; @everyone sans rôle.
pub fn top_rank(guild: &Guild, roles: &[u64]) -> Rank {
    let everyone = guild.roles.iter().find(|r| r.id == guild.id).map(role_rank).unwrap_or((0, std::cmp::Reverse(guild.id)));
    roles.iter().filter_map(|id| guild.roles.iter().find(|r| r.id == *id)).map(role_rank).fold(everyone, std::cmp::max)
}

/// Ce que mon rang permet sur les rôles et les membres d'un serveur.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Authority {
    Owner,
    /// `perms` : permissions de base (tout pour un administrateur) ; `top` :
    /// rang de mon rôle le plus élevé.
    Member { perms: Permissions, top: Rank },
    /// Informations incomplètes : rien n'est proposé (les pages d'administration
    /// ne s'affichent qu'avec une permission connue).
    Unknown,
}

/// Mon autorité dans `guild`.
pub fn authority(guild: &Guild, me: u64) -> Authority {
    match guild.owner_id {
        Some(o) if o == me => return Authority::Owner,
        None => return Authority::Unknown,
        _ => {}
    }
    let (Some(perms), Some(member)) = (base_permissions(guild, me), guild.me.as_ref()) else { return Authority::Unknown };
    Authority::Member { perms, top: top_rank(guild, &member.roles) }
}

impl Authority {
    pub fn has(self, p: Permissions) -> bool {
        match self {
            Authority::Owner => true,
            Authority::Unknown => false,
            Authority::Member { perms, .. } => perms.contains(p),
        }
    }

    /// Strictement au-dessus de ce rang (le propriétaire est au-dessus de tout).
    pub fn above(self, rank: Rank) -> bool {
        match self {
            Authority::Owner => true,
            Authority::Unknown => false,
            Authority::Member { top, .. } => rank < top,
        }
    }

    /// Modifier, supprimer, déplacer, donner ou retirer ce rôle : « Gérer les
    /// rôles » et un rôle strictement plus haut (même un administrateur).
    pub fn can_edit_role(self, r: &Role) -> bool {
        self.has(Permissions::MANAGE_ROLES) && self.above(role_rank(r))
    }

    /// Permissions qu'on peut accorder à un rôle : seulement celles qu'on a.
    pub fn grantable(self) -> Permissions {
        match self {
            Authority::Owner => Permissions::ALL,
            Authority::Unknown => Permissions::NONE,
            Authority::Member { perms, .. } => perms,
        }
    }

    /// Agir sur un membre (pseudo, modération) : `p`, un rang strictement plus
    /// haut que le sien, et jamais sur le propriétaire.
    pub fn can_act_on(self, p: Permissions, target_top: Rank, target_is_owner: bool) -> bool {
        !target_is_owner && self.has(p) && self.above(target_top)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::{ChannelKind, Role};

    const G: u64 = 100;
    const ME: u64 = 1;
    const OTHER: u64 = 2;
    const R_MOD: u64 = 201;
    const R_MEMBRE: u64 = 202;
    const NOW: i64 = 1_700_000_000_000;

    use Permissions as P;

    fn role(id: u64, p: P) -> Role {
        Role { id, name: format!("r{id}"), color: 0, position: 0, permissions: p, mentionable: false }
    }

    fn ow(kind: OverwriteKind, id: u64, allow: P, deny: P) -> Overwrite {
        Overwrite { id, kind, allow, deny }
    }

    fn guild(everyone: P, roles: &[(u64, P)], my_roles: &[u64]) -> Guild {
        let mut all = vec![role(G, everyone)];
        all.extend(roles.iter().map(|(id, p)| role(*id, *p)));
        Guild {
            id: G,
            name: "g".into(),
            channels: Vec::new(),
            owner_id: Some(OTHER),
            roles: all,
            me: Some(Member { roles: my_roles.to_vec(), nick: None, timeout_until: None }),
            roles_unreadable: false,
            icon: None,
            emojis: Vec::new(),
            premium_tier: 0,
            boosts: 0,
            banner: None, tag: None,
        }
    }

    fn chan(overwrites: Vec<Overwrite>) -> Channel {
        Channel { id: 300, name: "c".into(), kind: ChannelKind::Text, position: 0, parent: None, overwrites: Some(overwrites), slowmode: 0 }
    }

    fn eff(g: &Guild, c: &Channel) -> Option<P> {
        channel_permissions(g, ME, c, NOW)
    }

    const TEXT: P = P(P::VIEW_CHANNEL.0 | P::SEND_MESSAGES.0 | P::READ_MESSAGE_HISTORY.0 | P::ADD_REACTIONS.0 | P::ATTACH_FILES.0 | P::EMBED_LINKS.0);

    #[test]
    fn parse_decimal_strings() {
        assert_eq!(P::parse("0"), Some(P(0)));
        assert_eq!(P::parse("1024"), Some(P::VIEW_CHANNEL));
        assert_eq!(P::parse("2251799813685247"), Some(P((1 << 51) - 1)));
        assert_eq!(P::parse(&u64::MAX.to_string()), Some(P::ALL));
        // Plus large que 64 bits : bits de poids faible gardés, sans erreur.
        assert_eq!(P::parse("18446744073709551616"), Some(P(0)));
        assert_eq!(P::parse("18446744073709552640"), Some(P::VIEW_CHANNEL));
        // 2^128 + 1024
        assert_eq!(P::parse("340282366920938463463374607431768212480"), Some(P::VIEW_CHANNEL));
        for bad in ["", "-1", "+1", "1e3", "0x10", " 1", "abc"] {
            assert_eq!(P::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn named_flags_match_documentation() {
        assert_eq!(P::ADMINISTRATOR.0, 0x8);
        assert_eq!(P::VIEW_CHANNEL.0, 0x400);
        assert_eq!(P::SEND_MESSAGES.0, 0x800);
        assert_eq!(P::MANAGE_MESSAGES.0, 0x2000);
        assert_eq!(P::ATTACH_FILES.0, 0x8000);
        assert_eq!(P::READ_MESSAGE_HISTORY.0, 0x10000);
        assert_eq!(P::MENTION_EVERYONE.0, 0x20000);
        assert_eq!(P::MODERATE_MEMBERS.0, 1 << 40);
        assert_eq!(P::SEND_VOICE_MESSAGES.0, 1 << 46);
        assert_eq!(P::SET_VOICE_CHANNEL_STATUS.0, 1 << 48);
        assert_eq!(P::BYPASS_SLOWMODE.0, 1 << 52);
        // 52 permissions, bits distincts (le bit 47 n'est plus attribué).
        let mut seen = 0u64;
        for (name, p) in P::NAMED {
            assert_eq!(p.0.count_ones(), 1, "{name}");
            assert_eq!(seen & p.0, 0, "{name} en double");
            seen |= p.0;
        }
        assert_eq!(P::NAMED.len(), 52);
        assert_eq!(seen & (1 << 47), 0);
    }

    #[test]
    fn base_is_everyone_or_roles() {
        let g = guild(P::VIEW_CHANNEL, &[(R_MEMBRE, P::SEND_MESSAGES), (R_MOD, P::MANAGE_MESSAGES)], &[R_MEMBRE]);
        assert_eq!(base_permissions(&g, ME), Some(P::VIEW_CHANNEL | P::SEND_MESSAGES));
    }

    #[test]
    fn owner_and_administrator_have_everything() {
        let mut g = guild(P::NONE, &[], &[]);
        g.owner_id = Some(ME);
        // Le propriétaire n'a même pas besoin de son membre ni des rôles.
        g.me = None;
        g.roles.clear();
        let c = chan(vec![ow(OverwriteKind::Member, ME, P::NONE, P::VIEW_CHANNEL)]);
        assert_eq!(eff(&g, &c), Some(P::ALL));

        let g = guild(P::NONE, &[(R_MOD, P::ADMINISTRATOR)], &[R_MOD]);
        assert_eq!(base_permissions(&g, ME), Some(P::ALL));
        // Les dérogations ne retirent rien à un administrateur.
        let c = chan(vec![ow(OverwriteKind::Role, G, P::NONE, P::ALL), ow(OverwriteKind::Member, ME, P::NONE, P::ALL)]);
        assert_eq!(eff(&g, &c), Some(P::ALL));
        // ADMINISTRATOR accordé par @everyone vaut aussi.
        assert_eq!(base_permissions(&guild(P::ADMINISTRATOR, &[], &[]), ME), Some(P::ALL));
    }

    #[test]
    fn administrator_via_overwrite_is_not_a_bypass() {
        // Une dérogation qui accorderait ADMINISTRATOR ne change que ce bit :
        // le contournement n'est examiné que sur les permissions de base.
        let g = guild(TEXT, &[], &[]);
        let c = chan(vec![ow(OverwriteKind::Role, G, P::ADMINISTRATOR, P::SEND_MESSAGES)]);
        let p = eff(&g, &c).unwrap_or_default();
        assert!(!p.contains(P::SEND_MESSAGES));
        assert!(p.contains(P::ADMINISTRATOR));
    }

    #[test]
    fn overwrite_order_everyone_roles_member() {
        // Exemple de la documentation : @everyone refuse, un rôle accorde, le membre refuse.
        let g = guild(TEXT, &[(R_MEMBRE, P::NONE), (R_MOD, P::NONE)], &[R_MEMBRE, R_MOD]);
        let c = chan(vec![ow(OverwriteKind::Role, G, P::NONE, P::SEND_MESSAGES)]);
        assert!(!eff(&g, &c).unwrap_or_default().contains(P::SEND_MESSAGES));

        let c = chan(vec![ow(OverwriteKind::Role, G, P::NONE, P::SEND_MESSAGES), ow(OverwriteKind::Role, R_MEMBRE, P::SEND_MESSAGES, P::NONE)]);
        assert!(eff(&g, &c).unwrap_or_default().contains(P::SEND_MESSAGES));

        let c = chan(vec![
            ow(OverwriteKind::Role, G, P::NONE, P::SEND_MESSAGES),
            ow(OverwriteKind::Role, R_MEMBRE, P::SEND_MESSAGES, P::NONE),
            ow(OverwriteKind::Member, ME, P::NONE, P::SEND_MESSAGES),
        ]);
        assert!(!eff(&g, &c).unwrap_or_default().contains(P::SEND_MESSAGES));

        // L'accord d'un rôle l'emporte sur le refus d'un autre rôle.
        let c = chan(vec![ow(OverwriteKind::Role, R_MEMBRE, P::NONE, P::SEND_MESSAGES), ow(OverwriteKind::Role, R_MOD, P::SEND_MESSAGES, P::NONE)]);
        assert!(eff(&g, &c).unwrap_or_default().contains(P::SEND_MESSAGES));

        // Le membre peut rendre ce que les rôles retirent.
        let c = chan(vec![ow(OverwriteKind::Role, R_MEMBRE, P::NONE, P::SEND_MESSAGES), ow(OverwriteKind::Member, ME, P::SEND_MESSAGES, P::NONE)]);
        assert!(eff(&g, &c).unwrap_or_default().contains(P::SEND_MESSAGES));
    }

    #[test]
    fn overwrites_of_others_are_ignored() {
        let g = guild(TEXT, &[(R_MEMBRE, P::NONE), (R_MOD, P::NONE)], &[R_MEMBRE]);
        let c = chan(vec![
            ow(OverwriteKind::Role, R_MOD, P::NONE, P::VIEW_CHANNEL),
            ow(OverwriteKind::Member, OTHER, P::NONE, P::VIEW_CHANNEL),
            // Même identifiant que moi mais de type rôle : ce n'est pas moi.
            ow(OverwriteKind::Role, ME, P::NONE, P::VIEW_CHANNEL),
            // Dérogation « membre » portant l'identifiant du serveur : pas @everyone.
            ow(OverwriteKind::Member, G, P::NONE, P::VIEW_CHANNEL),
        ]);
        assert_eq!(eff(&g, &c), Some(TEXT));
    }

    #[test]
    fn private_channel_for_a_role() {
        let g = guild(TEXT, &[(R_MOD, P::NONE)], &[]);
        let c = chan(vec![ow(OverwriteKind::Role, G, P::NONE, P::VIEW_CHANNEL), ow(OverwriteKind::Role, R_MOD, P::VIEW_CHANNEL, P::NONE)]);
        assert_eq!(eff(&g, &c), Some(P::NONE));
        let mut g = g;
        if let Some(m) = g.me.as_mut() {
            m.roles.push(R_MOD);
        }
        assert_eq!(eff(&g, &c), Some(TEXT));
    }

    #[test]
    fn implicit_rules() {
        // Sans VIEW_CHANNEL : plus rien, même accordé ailleurs.
        assert_eq!(implicit(TEXT & !P::VIEW_CHANNEL), P::NONE);
        // Sans SEND_MESSAGES : ni pièces jointes, ni liens intégrés, ni @everyone, ni synthèse vocale.
        let all_text = TEXT | P::MENTION_EVERYONE | P::SEND_TTS_MESSAGES | P::SEND_VOICE_MESSAGES | P::SEND_POLLS;
        let p = implicit(all_text & !P::SEND_MESSAGES);
        assert_eq!(p, P::VIEW_CHANNEL | P::READ_MESSAGE_HISTORY | P::ADD_REACTIONS);
        assert_eq!(implicit(all_text), all_text);
        // Appliquées par le calcul complet.
        let g = guild(all_text, &[], &[]);
        let c = chan(vec![ow(OverwriteKind::Role, G, P::NONE, P::SEND_MESSAGES)]);
        assert_eq!(eff(&g, &c), Some(P::VIEW_CHANNEL | P::READ_MESSAGE_HISTORY | P::ADD_REACTIONS));
    }

    #[test]
    fn timeout_keeps_only_view_and_history() {
        let mut g = guild(TEXT | P::MANAGE_MESSAGES, &[], &[]);
        if let Some(m) = g.me.as_mut() {
            m.timeout_until = Some(NOW + 1);
        }
        let c = chan(Vec::new());
        assert_eq!(eff(&g, &c), Some(P::VIEW_CHANNEL | P::READ_MESSAGE_HISTORY));
        // Une dérogation ne rend rien pendant l'exclusion.
        let c2 = chan(vec![ow(OverwriteKind::Member, ME, P::SEND_MESSAGES, P::NONE)]);
        assert_eq!(eff(&g, &c2), Some(P::VIEW_CHANNEL | P::READ_MESSAGE_HISTORY));
        // Et ne rend pas visible un salon caché.
        let c3 = chan(vec![ow(OverwriteKind::Role, G, P::NONE, P::VIEW_CHANNEL)]);
        assert_eq!(eff(&g, &c3), Some(P::NONE));
        // Exclusion terminée (l'instant de fin compris).
        if let Some(m) = g.me.as_mut() {
            m.timeout_until = Some(NOW);
        }
        assert_eq!(eff(&g, &c), Some(TEXT | P::MANAGE_MESSAGES));
        // Administrateur et propriétaire : non concernés.
        let mut g = guild(TEXT, &[(R_MOD, P::ADMINISTRATOR)], &[R_MOD]);
        if let Some(m) = g.me.as_mut() {
            m.timeout_until = Some(NOW + 1);
        }
        assert_eq!(eff(&g, &c), Some(P::ALL));
        let mut g = guild(TEXT, &[], &[]);
        g.owner_id = Some(ME);
        if let Some(m) = g.me.as_mut() {
            m.timeout_until = Some(NOW + 1);
        }
        assert_eq!(eff(&g, &c), Some(P::ALL));
    }

    #[test]
    fn missing_information_is_unknown() {
        let c = chan(Vec::new());
        // Membre inconnu.
        let mut g = guild(TEXT, &[], &[]);
        g.me = None;
        assert_eq!(eff(&g, &c), None);
        // Rôle @everyone absent.
        let mut g = guild(TEXT, &[], &[]);
        g.roles.clear();
        assert_eq!(eff(&g, &c), None);
        // Un de mes rôles absent de la liste.
        let g = guild(TEXT, &[], &[R_MOD]);
        assert_eq!(eff(&g, &c), None);
        // Dérogations illisibles.
        let g = guild(TEXT, &[], &[]);
        let mut c2 = chan(Vec::new());
        c2.overwrites = None;
        assert_eq!(eff(&g, &c2), None);
        // Propriétaire inconnu, tout le reste connu : inconnu quand même
        // (je pourrais être le propriétaire), même sur un salon refusé.
        let mut g = guild(TEXT, &[], &[]);
        g.owner_id = None;
        assert_eq!(eff(&g, &c), None);
        let hidden = chan(vec![ow(OverwriteKind::Role, G, P::NONE, P::VIEW_CHANNEL)]);
        assert_eq!(eff(&g, &hidden), None);
        // Liste des rôles en partie illisible : inconnu.
        let mut g = guild(TEXT, &[], &[]);
        g.roles_unreadable = true;
        assert_eq!(eff(&g, &c), None);
    }

    fn ranked(id: u64, position: i32, p: P) -> Role {
        Role { id, name: format!("r{id}"), color: 0, position, permissions: p, mentionable: false }
    }

    #[test]
    fn role_hierarchy() {
        const R_HAUT: u64 = 203;
        let mut g = guild(P::NONE, &[], &[R_MOD]);
        g.roles = vec![ranked(G, 0, P::VIEW_CHANNEL), ranked(R_MEMBRE, 1, P::NONE), ranked(R_MOD, 2, P::MANAGE_ROLES), ranked(R_HAUT, 3, P::NONE)];
        let a = authority(&g, ME);
        assert!(matches!(a, Authority::Member { .. }));
        let role = |id: u64| g.roles.iter().find(|r| r.id == id).cloned().unwrap_or_else(|| ranked(id, 0, P::NONE));
        // Plus bas que mon rôle le plus haut : oui ; égal ou plus haut : non.
        assert!(a.can_edit_role(&role(R_MEMBRE)));
        assert!(a.can_edit_role(&role(G)), "@everyone est en dessous");
        assert!(!a.can_edit_role(&role(R_MOD)), "mon propre rôle le plus haut");
        assert!(!a.can_edit_role(&role(R_HAUT)));
        // À position égale, le plus petit identifiant est au-dessus.
        let twin_low = ranked(R_MOD + 10, 2, P::NONE);
        let twin_high = ranked(R_MOD - 100, 2, P::NONE);
        assert!(a.can_edit_role(&twin_low));
        assert!(!a.can_edit_role(&twin_high));
        // On n'accorde que ce qu'on a.
        assert_eq!(a.grantable(), P::VIEW_CHANNEL | P::MANAGE_ROLES);
        // Membres : rang strictement plus haut, jamais le propriétaire.
        assert!(a.can_act_on(P::MANAGE_ROLES, top_rank(&g, &[R_MEMBRE]), false));
        assert!(!a.can_act_on(P::MANAGE_ROLES, top_rank(&g, &[R_MOD]), false));
        assert!(!a.can_act_on(P::MANAGE_ROLES, top_rank(&g, &[]), true));
        assert!(!a.can_act_on(P::MANAGE_NICKNAMES, top_rank(&g, &[]), false), "permission absente");

        // Sans « Gérer les rôles » : rien, même plus bas.
        let mut g2 = g.clone();
        g2.roles[2].permissions = P::NONE;
        assert!(!authority(&g2, ME).can_edit_role(&role(R_MEMBRE)));
        // Administrateur : toutes les permissions, mais la hiérarchie tient.
        g2.roles[2].permissions = P::ADMINISTRATOR;
        let admin = authority(&g2, ME);
        assert!(admin.can_edit_role(&role(R_MEMBRE)));
        assert!(!admin.can_edit_role(&role(R_HAUT)));
        assert_eq!(admin.grantable(), P::ALL);
        // Propriétaire : tout.
        g2.owner_id = Some(ME);
        let owner = authority(&g2, ME);
        assert_eq!(owner, Authority::Owner);
        assert!(owner.can_edit_role(&role(R_HAUT)));
        // Inconnu : rien n'est proposé.
        g2.owner_id = None;
        let unknown = authority(&g2, ME);
        assert_eq!(unknown, Authority::Unknown);
        assert!(!unknown.can_edit_role(&role(R_MEMBRE)));
        assert_eq!(unknown.grantable(), P::NONE);
        // Aucun rôle : seul @everyone, qu'on ne peut pas modifier.
        let mut g3 = g.clone();
        g3.roles[0].permissions = P::MANAGE_ROLES;
        if let Some(m) = g3.me.as_mut() {
            m.roles.clear();
        }
        assert!(!authority(&g3, ME).can_edit_role(&role(G)));
    }

    #[test]
    fn unknown_bits_survive() {
        let high = P(1 << 63);
        let g = guild(TEXT | high, &[], &[]);
        assert_eq!(eff(&g, &chan(Vec::new())), Some(TEXT | high));
    }
}
