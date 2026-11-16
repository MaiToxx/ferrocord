//! Notifications de bureau : quand un message reçu doit notifier, ce que
//! dit la notification, la pastille (mentions non lues) et les réglages
//! locaux (`notify.toml`). Logique pure : ni egui, ni réseau, ni disque ;
//! l'interface la consulte à la réception et `desk` affiche le résultat.
//!
//! Règles, dans l'ordre (celles du client officiel, docs.discord.food,
//! « User Guild Settings » et « Message Notification Level ») :
//! 1. jamais pour mes propres messages, ni pour ceux d'un utilisateur bloqué ;
//! 2. jamais en statut « Ne pas déranger » ;
//! 3. jamais pour le salon ouvert quand la fenêtre est au premier plan ;
//! 4. jamais pour un salon muet (lui, sa catégorie ou son serveur) : la
//!    mention reste comptée dans la pastille, comme le fait le magasin ;
//! 5. niveau du salon, sinon de sa catégorie, sinon du serveur : « tout »,
//!    « mentions » (moi, @everyone/@here, un de mes rôles, sauf si le
//!    serveur les ignore) ou « rien ». Un MP notifie toujours.

use super::model::{Guild, Message};
use super::relations::RelKind;
use super::store::Store;
use super::usersettings::notif::{Level, Notifs};
use super::usersettings::schema::{Key, SValue};
use super::model::ImageRef;
use crate::i18n::{table::notify::t as nt, Lang};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Au plus une notification par salon pendant ce temps (les messages
/// suivants comptent dans la pastille, sans nouvelle fenêtre ni son).
pub const THROTTLE: Duration = Duration::from_secs(5);
/// Salons retenus par le regroupement, au plus : au-delà, les plus anciens sont oubliés.
pub const THROTTLE_CHANNELS: usize = 64;
/// Longueur du texte d'une notification (caractères), au plus.
pub const MAX_BODY: usize = 200;
/// Nom affiché dans le titre de la fenêtre.
pub const APP_NAME: &str = "Ferrocord";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Message ordinaire d'un salon réglé sur « tout » (ou d'un MP).
    Message,
    /// Je suis mentionné (directement, par @everyone ou par un de mes rôles).
    Mention,
    // Sons du vocal (phase 4, `ui::voice`) : arrivée, départ, micro coupé, rétabli.
    VoiceJoin,
    VoiceLeave,
    VoiceMute,
    VoiceUnmute,
    /// Appel entrant qui sonne (lot V6).
    Ring,
    // Sons par événement (lot FF).
    /// Casque coupé, rétabli.
    VoiceDeafen,
    VoiceUndeafen,
    /// Appel sortant : ça sonne chez l'autre.
    CallOut,
    /// Un direct commence, s'arrête dans mon salon vocal.
    StreamStart,
    StreamStop,
    /// Demande d'ami reçue.
    FriendRequest,
    /// Un événement auquel je suis intéressé commence.
    EventStart,
    /// Quelqu'un a réagi à l'un de mes messages (lot FF2).
    Reaction,
    /// Un rappel de message enregistré arrive à échéance (lot FF2).
    Reminder,
    /// J'entre dans un salon vocal, j'en sors (lot « sons ») : plus marqués que
    /// l'arrivée et le départ de quelqu'un d'autre.
    VoiceConnect,
    VoiceDisconnect,
}

impl Kind {
    /// Tous les sons, dans l'ordre de la page « Sons ».
    pub const ALL: [Kind; 18] = [
        Kind::Message,
        Kind::Mention,
        Kind::VoiceDeafen,
        Kind::VoiceUndeafen,
        Kind::VoiceMute,
        Kind::VoiceUnmute,
        Kind::VoiceJoin,
        Kind::VoiceLeave,
        Kind::VoiceConnect,
        Kind::VoiceDisconnect,
        Kind::CallOut,
        Kind::Ring,
        Kind::StreamStart,
        Kind::StreamStop,
        Kind::FriendRequest,
        Kind::EventStart,
        Kind::Reaction,
        Kind::Reminder,
    ];

    /// Bit de ce son dans [`DeskPrefs::muted_sounds`] (stable : écrit dans `notify.toml`).
    pub fn bit(self) -> u32 {
        1 << match self {
            Kind::Message => 0,
            Kind::Mention => 1,
            Kind::VoiceJoin => 2,
            Kind::VoiceLeave => 3,
            Kind::VoiceMute => 4,
            Kind::VoiceUnmute => 5,
            Kind::Ring => 6,
            Kind::VoiceDeafen => 7,
            Kind::VoiceUndeafen => 8,
            Kind::CallOut => 9,
            Kind::StreamStart => 10,
            Kind::StreamStop => 11,
            Kind::FriendRequest => 12,
            Kind::EventStart => 13,
            Kind::Reaction => 14,
            Kind::Reminder => 15,
            Kind::VoiceConnect => 16,
            Kind::VoiceDisconnect => 17,
        }
    }
}

/// Ce qu'il faut savoir d'un message pour décider (tiré du magasin par [`facts`]).
#[derive(Clone, Debug, Default)]
pub struct Facts<'a> {
    pub me: u64,
    pub author: u64,
    pub guild: Option<u64>,
    pub channel: u64,
    /// Catégorie du salon.
    pub parent: Option<u64>,
    /// Niveau choisi pour ce fil (lot N) : il l'emporte sur celui du serveur.
    pub thread: Option<Level>,
    pub my_roles: &'a [u64],
    pub mentions_me: bool,
    pub mention_everyone: bool,
    pub mention_roles: &'a [u64],
    /// Salon ouvert et fenêtre au premier plan.
    pub viewing: bool,
    pub dnd: bool,
    pub blocked: bool,
    pub now_ms: i64,
}

/// Niveau qui s'applique au salon : le sien, sinon celui de sa catégorie,
/// sinon celui du serveur (`Inherit` au niveau du serveur : « mentions »,
/// la valeur par défaut). Un MP vaut « tout » sauf réglage du salon.
pub fn level(n: &Notifs, guild: Option<u64>, channel: u64, parent: Option<u64>) -> Level {
    let g = n.get(guild);
    let own = g.channel(channel).level;
    if own != Level::Inherit {
        return own;
    }
    if let Some(p) = parent {
        let cat = g.channel(p).level;
        if cat != Level::Inherit {
            return cat;
        }
    }
    match (guild, g.level) {
        (None, _) => Level::All,
        (Some(_), Level::Inherit) => Level::Mentions,
        (Some(_), l) => l,
    }
}

/// Le message me mentionne-t-il, d'après les réglages du serveur ?
pub fn pings(n: &Notifs, f: &Facts) -> bool {
    let g = n.by_guild.get(&f.guild);
    let everyone = f.mention_everyone && !g.is_some_and(|g| g.suppress_everyone);
    let roles = !g.is_some_and(|g| g.suppress_roles) && f.mention_roles.iter().any(|r| f.my_roles.contains(r));
    f.mentions_me || everyone || roles
}

/// Faut-il notifier, et comment ?
pub fn decide(n: &Notifs, f: &Facts) -> Option<Kind> {
    if f.author == f.me || f.blocked || f.dnd || f.viewing {
        return None;
    }
    if n.muted(f.guild, f.channel, f.parent, f.now_ms) {
        return None;
    }
    let ping = pings(n, f);
    match f.thread.unwrap_or_else(|| level(n, f.guild, f.channel, f.parent)) {
        Level::All | Level::Inherit => Some(if ping { Kind::Mention } else { Kind::Message }),
        Level::Mentions => ping.then_some(Kind::Mention),
        Level::Nothing => None,
    }
}

fn guild_of(store: &Store, id: Option<u64>) -> Option<&Guild> {
    id.and_then(|g| store.guilds.iter().find(|x| x.id == g))
}

/// Catégorie d'un salon de serveur.
pub fn parent_of(store: &Store, guild: Option<u64>, channel: u64) -> Option<u64> {
    guild_of(store, guild)?.channels.iter().find(|c| c.id == channel)?.parent
}

/// Mes rôles dans ce serveur (vide s'ils ne sont pas connus).
pub fn my_roles(store: &Store, guild: Option<u64>) -> &[u64] {
    guild_of(store, guild).and_then(|g| g.me.as_ref()).map(|m| m.roles.as_slice()).unwrap_or(&[])
}

/// Un de mes rôles est-il mentionné (et le serveur ne l'ignore pas) ?
/// Sert aussi au magasin pour la pastille des salons.
pub fn role_ping(store: &Store, m: &Message) -> bool {
    !m.meta.mention_roles.is_empty()
        && !store.account.notifs.by_guild.get(&m.guild_id).is_some_and(|g| g.suppress_roles)
        && m.meta.mention_roles.iter().any(|r| my_roles(store, m.guild_id).contains(r))
}

/// Le message `m` me mentionne-t-il (fond ambre du fil) ? Mon identifiant dans
/// ses `mentions` (@moi, ou réponse à l'un de mes messages avec la mention
/// laissée active : sans elle, je n'y suis pas), @everyone / @here, ou un de
/// mes rôles — ces deux derniers sauf si le serveur les ignore.
pub fn mentioned(store: &Store, m: &Message) -> bool {
    let Some(me) = store.me.as_ref() else { return false };
    m.mentions.iter().any(|(id, _)| *id == me.id)
        || (m.mention_everyone && !store.account.notifs.by_guild.get(&m.guild_id).is_some_and(|g| g.suppress_everyone))
        || role_ping(store, m)
}

/// Mon statut est-il « Ne pas déranger » (réglage synchronisé du compte) ?
pub fn dnd(store: &Store) -> bool {
    matches!(store.account.value(Key::Status), Some(SValue::Str(s)) if s == "dnd")
}

/// Ce que le magasin sait du message `m` ; `focused` : la fenêtre est au premier plan.
pub fn facts<'a>(store: &'a Store, m: &'a Message, focused: bool) -> Facts<'a> {
    Facts {
        me: store.me.as_ref().map_or(0, |u| u.id),
        author: m.author_id,
        guild: m.guild_id,
        channel: m.channel_id,
        parent: parent_of(store, m.guild_id, m.channel_id),
        thread: store.threads.notif(m.channel_id),
        my_roles: my_roles(store, m.guild_id),
        mentions_me: store.me.as_ref().is_some_and(|me| m.mentions.iter().any(|(id, _)| *id == me.id)),
        mention_everyone: m.mention_everyone,
        mention_roles: &m.meta.mention_roles,
        viewing: focused && store.channel == Some(m.channel_id),
        dnd: dnd(store),
        // Un message `@silent` (F9) ne notifie pas plus que celui d'une personne bloquée.
        blocked: store.relations.relation(m.author_id).is_some_and(|r| r.kind == RelKind::Blocked) || m.meta.flags & super::builtin::SUPPRESS_NOTIFICATIONS != 0,
        now_ms: super::store::now_ms(),
    }
}

/// Contenu d'une notification de bureau.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alert {
    pub guild: Option<u64>,
    pub channel: u64,
    pub kind: Kind,
    /// Auteur.
    pub title: String,
    /// « Serveur › #salon », ou le nom d'un MP de groupe ; `None` pour un MP à deux.
    pub place: Option<String>,
    /// Texte sans mise en forme, mentions mises en mots, spoilers masqués.
    pub body: String,
    /// Avatar de l'auteur (montré seulement s'il est déjà dans le cache).
    pub avatar: Option<ImageRef>,
    /// Groupe du centre de notifications : le serveur, ou « Messages privés ».
    pub header: Option<String>,
}

impl Alert {
    /// La même notification sans le texte du message (réglage « Montrer le
    /// texte du message » désactivé) : elle dit seulement qui a écrit, et où.
    pub fn without_preview(mut self, lang: Lang) -> Alert {
        use crate::i18n::table::lotn::t as xt;
        match self.kind {
            Kind::Mention => self.body = xt::TOAST_HIDDEN_MENTION.get(lang).to_string(),
            Kind::Message => self.body = xt::TOAST_HIDDEN.get(lang).to_string(),
            // Sonnerie et sons du vocal : leur texte ne cite aucun message.
            Kind::VoiceJoin | Kind::VoiceLeave | Kind::VoiceMute | Kind::VoiceUnmute | Kind::Ring => {}
            Kind::VoiceDeafen | Kind::VoiceUndeafen | Kind::CallOut | Kind::StreamStart | Kind::StreamStop | Kind::FriendRequest | Kind::EventStart | Kind::Reaction | Kind::Reminder | Kind::VoiceConnect | Kind::VoiceDisconnect => {}
        }
        self
    }
}

/// Nom du groupe d'une notification : le serveur, ou « Messages privés ».
pub fn header(store: &Store, guild: Option<u64>, lang: Lang) -> Option<String> {
    match guild {
        Some(_) => guild_of(store, guild).map(|g| g.name.clone()),
        None => Some(crate::i18n::table::lotn::t::TOAST_DMS.get(lang).to_string()),
    }
}

/// Où le message a été écrit, en mots.
fn place(store: &Store, m: &Message) -> Option<String> {
    match guild_of(store, m.guild_id) {
        Some(g) => {
            let chan = store.channel_name(m.channel_id).unwrap_or("?");
            Some(format!("{} › #{chan}", g.name))
        }
        None => {
            let dm = store.dms.iter().find(|d| d.id == m.channel_id)?;
            // MP à deux : son nom est celui de l'auteur, déjà en titre.
            (dm.recipients.len() > 1 || dm.name != m.author).then(|| dm.name.clone()).filter(|n| !n.is_empty())
        }
    }
}

/// Texte d'une notification, tiré de la mise en forme du message (spoilers
/// remplacés, lignes jointes, coupé à [`MAX_BODY`] caractères).
pub fn body(store: &Store, m: &Message, lang: Lang, h24: bool) -> String {
    let doc = super::markdown::parse_message(store, m, lang, h24);
    let mut out = String::new();
    let mut in_spoiler = None;
    for b in &doc.blocks {
        if !out.is_empty() {
            out.push(' ');
        }
        for s in &b.spans {
            match s.style.spoiler {
                Some(id) if in_spoiler == Some(id) => {}
                Some(id) => {
                    in_spoiler = Some(id);
                    out.push_str(nt::SPOILER.get(lang));
                }
                None => {
                    in_spoiler = None;
                    out.push_str(&s.text);
                }
            }
        }
        if matches!(b.kind, super::markdown::BlockKind::Code { spoiler: Some(_), .. }) {
            in_spoiler = None;
        }
    }
    let text: String = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = if text.is_empty() {
        if let Some(kind) = m.system {
            system_text(kind).get(lang).to_string()
        } else if !m.attachments.is_empty() {
            nt::ATTACHMENT.get(lang).to_string()
        } else if !m.embeds.is_empty() {
            nt::EMBED.get(lang).to_string()
        } else {
            String::new()
        }
    } else {
        text
    };
    truncate(&text, MAX_BODY)
}

fn system_text(kind: super::SystemKind) -> crate::i18n::Text {
    super::system_msgs::generic(kind)
}

/// Coupe à `max` caractères (jamais au milieu d'un caractère), avec « … ».
pub fn truncate(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", s[..i].trim_end()),
        None => s.to_string(),
    }
}

/// Notification à montrer pour `m`, s'il en faut une.
pub fn alert(store: &Store, m: &Message, focused: bool, lang: Lang, h24: bool) -> Option<Alert> {
    let kind = decide(&store.account.notifs, &facts(store, m, focused))?;
    Some(alert_of(store, m, kind, lang, h24))
}

/// Contenu de la notification de `m`, sans décider s'il en faut une (mots-clés
/// surveillés, lot E1 : le plugin décide lui-même).
pub fn alert_of(store: &Store, m: &Message, kind: Kind, lang: Lang, h24: bool) -> Alert {
    Alert {
        guild: m.guild_id,
        channel: m.channel_id,
        kind,
        title: m.author.clone(),
        place: place(store, m),
        body: body(store, m, lang, h24),
        avatar: m.author_avatar.image,
        header: header(store, m.guild_id, lang),
    }
}

/// Regroupement : une notification par salon toutes les [`THROTTLE`] au plus.
#[derive(Debug, Default)]
pub struct Throttle {
    last: HashMap<u64, Instant>,
}

impl Throttle {
    /// Une notification pour `channel` peut-elle partir à `now` ? (Si oui, elle compte.)
    pub fn allow(&mut self, channel: u64, now: Instant) -> bool {
        if self.last.get(&channel).is_some_and(|t| now.saturating_duration_since(*t) < THROTTLE) {
            return false;
        }
        if self.last.len() >= THROTTLE_CHANNELS {
            self.last.retain(|_, t| now.saturating_duration_since(*t) < THROTTLE);
            if self.last.len() >= THROTTLE_CHANNELS
                && let Some(oldest) = self.last.iter().min_by_key(|(_, t)| **t).map(|(c, _)| *c)
            {
                self.last.remove(&oldest);
            }
        }
        self.last.insert(channel, now);
        true
    }

    pub fn len(&self) -> usize {
        self.last.len()
    }

    pub fn is_empty(&self) -> bool {
        self.last.is_empty()
    }
}

/// Mentions non lues par salon (pastille). Le magasin dit quels salons sont
/// mentionnés ; on compte ici les messages, comme Discord. Un salon que le
/// magasin ne tient plus pour mentionné est oublié : la table reste bornée
/// par les salons du compte.
#[derive(Debug, Default)]
pub struct Mentions {
    per: HashMap<u64, u32>,
}

impl Mentions {
    pub fn add(&mut self, channel: u64) {
        let n = self.per.entry(channel).or_insert(0);
        *n = n.saturating_add(1);
    }

    /// Total des mentions non lues : au moins une par salon mentionné.
    pub fn total(&mut self, mentioned: &HashSet<u64>) -> u32 {
        self.per.retain(|c, _| mentioned.contains(c));
        mentioned.iter().map(|c| self.per.get(c).copied().unwrap_or(1).max(1)).fold(0u32, u32::saturating_add)
    }

    pub fn clear(&mut self) {
        self.per.clear();
    }
}

/// Titre de la fenêtre : « (3) Ferrocord » s'il y a des mentions non lues.
pub fn window_title(mentions: u32) -> String {
    match mentions {
        0 => APP_NAME.to_string(),
        n => format!("({}) {APP_NAME}", badge_text(n)),
    }
}

/// Nombre écrit dans la pastille (« 99+ » au-delà).
pub fn badge_text(n: u32) -> String {
    if n > 99 { "99+".into() } else { n.to_string() }
}

/// Réglages de bureau de cet ordinateur (`notify.toml`), jamais envoyés à Discord.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeskPrefs {
    /// Notifications de Windows.
    pub toasts: bool,
    pub sounds: bool,
    /// 0 à 100.
    pub volume: u8,
    /// Compteur dans le titre et sur l'icône, clignotement à chaque mention.
    pub badge: bool,
    /// Icône de la zone de notification.
    pub tray: bool,
    /// La croix masque la fenêtre dans la zone de notification.
    pub close_to_tray: bool,
    /// Les notifications montrent le texte du message.
    pub preview: bool,
    /// Sons coupés un par un (bits de [`Kind::bit`]) ; 0 : tous joués.
    pub muted_sounds: u32,
}

impl Default for DeskPrefs {
    fn default() -> Self {
        DeskPrefs { toasts: true, sounds: true, volume: 60, badge: true, tray: true, close_to_tray: true, preview: true, muted_sounds: 0 }
    }
}

const K_TOASTS: &str = "notifications";
const K_SOUNDS: &str = "sons";
const K_VOLUME: &str = "volume";
const K_BADGE: &str = "pastille";
const K_TRAY: &str = "zone_de_notification";
// Clé renommée quand le défaut est passé à « activé » (comme Discord) :
// l'ancienne valeur enregistrée, désactivée par défaut, est ignorée.
const K_CLOSE: &str = "croix_dans_la_zone";
const K_PREVIEW: &str = "texte_du_message";
const K_MUTED: &str = "sons_coupes";

impl DeskPrefs {
    /// Ce son est-il joué (sons activés, volume non nul, son pas coupé) ?
    pub fn sound_on(&self, kind: Kind) -> bool {
        self.sounds && self.volume > 0 && self.muted_sounds & kind.bit() == 0
    }

    /// Texte de `notify.toml`.
    pub fn to_toml(&self) -> String {
        format!(
            "# Notifications de bureau de Ferrocord (réglages de cet ordinateur).\n{K_TOASTS} = {}\n{K_SOUNDS} = {}\n{K_VOLUME} = {}\n{K_BADGE} = {}\n{K_TRAY} = {}\n{K_CLOSE} = {}\n{K_PREVIEW} = {}\n{K_MUTED} = {}\n",
            self.toasts, self.sounds, self.volume, self.badge, self.tray, self.close_to_tray, self.preview, self.muted_sounds
        )
    }

    /// Lit `notify.toml`. Clé inconnue ou valeur illisible : ignorée (valeur
    /// par défaut), le fichier n'est jamais refusé en entier. `Err` : syntaxe.
    pub fn parse(text: &str) -> Result<DeskPrefs, String> {
        use toml::de::DeTable;
        let root = DeTable::parse(text).map_err(|e| e.message().to_string())?;
        let mut p = DeskPrefs::default();
        for (key, value) in root.get_ref().iter() {
            let v = value.get_ref();
            let flag = |slot: &mut bool| {
                if let Some(b) = v.as_bool() {
                    *slot = b;
                }
            };
            match key.get_ref().as_ref() {
                K_TOASTS => flag(&mut p.toasts),
                K_SOUNDS => flag(&mut p.sounds),
                K_BADGE => flag(&mut p.badge),
                K_TRAY => flag(&mut p.tray),
                K_CLOSE => flag(&mut p.close_to_tray),
                K_PREVIEW => flag(&mut p.preview),
                K_VOLUME => {
                    if let Some(i) = v.as_integer()
                        && let Ok(n) = i64::from_str_radix(i.as_str(), i.radix())
                    {
                        p.volume = n.clamp(0, 100) as u8;
                    }
                }
                K_MUTED => {
                    if let Some(i) = v.as_integer()
                        && let Ok(n) = i64::from_str_radix(i.as_str(), i.radix())
                    {
                        p.muted_sounds = u32::try_from(n).unwrap_or(0);
                    }
                }
                _ => {}
            }
        }
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::{Avatar, Channel, ChannelKind, Dm, Member, User};
    use crate::state::usersettings::notif::{ChannelNotif, GuildNotif, Mute};

    const ME: u64 = 1;
    const G: u64 = 9;
    const CAT: u64 = 50;
    const CHAN: u64 = 100;
    const ROLE: u64 = 7;

    fn f<'a>() -> Facts<'a> {
        Facts { me: ME, author: 2, guild: Some(G), channel: CHAN, parent: Some(CAT), ..Default::default() }
    }

    fn notifs(level: Level) -> Notifs {
        let mut n = Notifs::default();
        let mut g = GuildNotif::new(Some(G));
        g.level = level;
        n.by_guild.insert(Some(G), g);
        n
    }

    #[test]
    fn never_for_me_blocked_dnd_or_viewing() {
        let n = notifs(Level::All);
        assert_eq!(decide(&n, &f()), Some(Kind::Message));
        assert_eq!(decide(&n, &Facts { author: ME, ..f() }), None, "mon message");
        assert_eq!(decide(&n, &Facts { blocked: true, ..f() }), None, "bloqué");
        assert_eq!(decide(&n, &Facts { dnd: true, mentions_me: true, ..f() }), None, "Ne pas déranger");
        assert_eq!(decide(&n, &Facts { viewing: true, mentions_me: true, ..f() }), None, "salon ouvert, fenêtre active");
    }

    #[test]
    fn levels_of_channel_category_and_guild() {
        assert_eq!(decide(&Notifs::default(), &f()), None, "serveur sans réglage : mentions seulement");
        assert_eq!(decide(&Notifs::default(), &Facts { mentions_me: true, ..f() }), Some(Kind::Mention));
        assert_eq!(decide(&notifs(Level::Inherit), &f()), None, "niveau hérité du serveur : mentions");
        assert_eq!(decide(&notifs(Level::Nothing), &Facts { mentions_me: true, ..f() }), None, "rien");
        let mut n = notifs(Level::Nothing);
        n.by_guild.get_mut(&Some(G)).unwrap().channels.insert(CAT, ChannelNotif { mute: Mute::default(), level: Level::All, flags: 0 });
        assert_eq!(decide(&n, &f()), Some(Kind::Message), "catégorie sur « tout »");
        n.by_guild.get_mut(&Some(G)).unwrap().channels.insert(CHAN, ChannelNotif { mute: Mute::default(), level: Level::Mentions, flags: 0 });
        assert_eq!(decide(&n, &f()), None, "le salon l'emporte sur la catégorie");
        assert_eq!(decide(&n, &Facts { mentions_me: true, ..f() }), Some(Kind::Mention));
        assert_eq!(level(&n, Some(G), CHAN + 1, None), Level::Nothing, "autre salon : le serveur");
    }

    #[test]
    fn thread_level_wins_over_the_guild() {
        let n = notifs(Level::Mentions);
        let th = Facts { parent: None, thread: Some(Level::All), ..f() };
        assert_eq!(decide(&n, &th), Some(Kind::Message), "fil sur « tous les messages »");
        assert_eq!(decide(&notifs(Level::All), &Facts { thread: Some(Level::Nothing), mentions_me: true, ..f() }), None, "fil sur « rien »");
        assert_eq!(decide(&notifs(Level::All), &Facts { thread: Some(Level::Mentions), ..f() }), None);
    }

    #[test]
    fn direct_messages_always_notify_unless_muted() {
        let n = Notifs::default();
        let dm = Facts { guild: None, parent: None, ..f() };
        assert_eq!(decide(&n, &dm), Some(Kind::Message));
        let mut n = Notifs::default();
        let mut g = GuildNotif::new(None);
        g.channels.insert(CHAN, ChannelNotif { mute: Mute { on: true, until_ms: None }, level: Level::Inherit, flags: 0 });
        n.by_guild.insert(None, g);
        assert_eq!(decide(&n, &dm), None, "MP muet");
        assert_eq!(decide(&n, &Facts { channel: CHAN + 1, ..dm }), Some(Kind::Message));
    }

    #[test]
    fn muted_never_notifies_even_when_mentioned() {
        let mut n = notifs(Level::All);
        n.by_guild.get_mut(&Some(G)).unwrap().mute = Mute { on: true, until_ms: Some(1_000) };
        assert_eq!(decide(&n, &Facts { mentions_me: true, now_ms: 999, ..f() }), None);
        assert_eq!(decide(&n, &Facts { mentions_me: true, now_ms: 1_000, ..f() }), Some(Kind::Mention), "muet échu");
        let mut n = notifs(Level::All);
        n.by_guild.get_mut(&Some(G)).unwrap().channels.insert(CAT, ChannelNotif { mute: Mute { on: true, until_ms: None }, level: Level::Inherit, flags: 0 });
        assert_eq!(decide(&n, &f()), None, "catégorie muette");
    }

    #[test]
    fn everyone_and_roles_can_be_suppressed() {
        let roles = [ROLE];
        let base = Facts { my_roles: &roles, ..f() };
        let n = notifs(Level::Mentions);
        assert_eq!(decide(&n, &Facts { mention_everyone: true, ..base.clone() }), Some(Kind::Mention));
        assert_eq!(decide(&n, &Facts { mention_roles: &[ROLE], ..base.clone() }), Some(Kind::Mention));
        assert_eq!(decide(&n, &Facts { mention_roles: &[ROLE + 1], ..base.clone() }), None, "rôle qui n'est pas le mien");
        let mut n = notifs(Level::Mentions);
        let g = n.by_guild.get_mut(&Some(G)).unwrap();
        g.suppress_everyone = true;
        g.suppress_roles = true;
        assert_eq!(decide(&n, &Facts { mention_everyone: true, ..base.clone() }), None);
        assert_eq!(decide(&n, &Facts { mention_roles: &[ROLE], ..base.clone() }), None);
        assert_eq!(decide(&n, &Facts { mentions_me: true, mention_everyone: true, ..base }), Some(Kind::Mention), "mention directe");
        assert_eq!(decide(&notifs(Level::All), &Facts { mention_everyone: true, ..f() }), Some(Kind::Mention), "« tout » : son de mention");
    }

    #[test]
    fn throttle_one_per_channel_and_bounded() {
        let mut t = Throttle::default();
        let t0 = Instant::now();
        assert!(t.allow(1, t0));
        assert!(!t.allow(1, t0 + Duration::from_secs(1)));
        assert!(t.allow(2, t0 + Duration::from_secs(1)), "autre salon");
        assert!(t.allow(1, t0 + THROTTLE));
        for c in 0..(THROTTLE_CHANNELS as u64 * 3) {
            assert!(t.allow(1_000 + c, t0 + THROTTLE));
        }
        assert!(t.len() <= THROTTLE_CHANNELS);
        assert!(!t.allow(1_000 + THROTTLE_CHANNELS as u64 * 3 - 1, t0 + THROTTLE), "le plus récent est gardé");
    }

    #[test]
    fn mention_counts_follow_the_store() {
        let mut m = Mentions::default();
        let mut mentioned: HashSet<u64> = [1, 2].into();
        m.add(1);
        m.add(1);
        m.add(1);
        assert_eq!(m.total(&mentioned), 4, "3 dans le salon 1, au moins 1 dans le salon 2");
        mentioned.remove(&1);
        assert_eq!(m.total(&mentioned), 1, "salon lu : oublié");
        m.add(1);
        mentioned.insert(1);
        assert_eq!(m.total(&mentioned), 2);
        assert_eq!(window_title(0), "Ferrocord");
        assert_eq!(window_title(3), "(3) Ferrocord");
        assert_eq!(window_title(150), "(99+) Ferrocord");
    }

    #[test]
    fn prefs_round_trip_and_tolerate_bad_values() {
        let p = DeskPrefs { toasts: false, sounds: true, volume: 35, badge: false, tray: false, close_to_tray: true, preview: false, muted_sounds: 1 << 9 };
        assert_eq!(DeskPrefs::parse(&p.to_toml()), Ok(p));
        assert_eq!(DeskPrefs::parse(""), Ok(DeskPrefs::default()));
        let odd = DeskPrefs::parse("sons = \"oui\"\nvolume = 400\ninconnu = 1\nnotifications = false").unwrap();
        assert_eq!(odd, DeskPrefs { toasts: false, volume: 100, ..DeskPrefs::default() });
        assert_eq!(DeskPrefs::parse("volume = -3").unwrap().volume, 0);
        assert!(DeskPrefs::parse("sons = ").is_err());
        assert!(DeskPrefs::default().close_to_tray, "activé par défaut, comme Discord");
    }

    fn store() -> Store {
        let mut s = Store::new();
        s.me = Some(User::new(ME, "moi".into()));
        let chan = |id, kind, parent| Channel { id, name: format!("salon{id}"), kind, position: 0, parent, overwrites: Some(Vec::new()), slowmode: 0 };
        s.guilds.push(Guild {
            id: G,
            name: "Serveur".into(),
            channels: vec![chan(CAT, ChannelKind::Category, None), chan(CHAN, ChannelKind::Text, Some(CAT))],
            owner_id: None,
            roles: Vec::new(),
            me: Some(Member { roles: vec![ROLE], ..Default::default() }),
            roles_unreadable: false,
            icon: None,
            emojis: Vec::new(),
            premium_tier: 0,
            boosts: 0,
            banner: None, tag: None,
        });
        s.dms.push(Dm::new(300, "Bob".into(), 0));
        s.refresh_permissions();
        s
    }

    fn msg(channel: u64, guild: Option<u64>, content: &str) -> Message {
        Message {
            id: 10,
            channel_id: channel,
            guild_id: guild,
            author_id: 2,
            author: "Bob".into(),
            author_avatar: Avatar::default_for(2),
            content: content.into(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            mentions: Vec::new(),
            mention_everyone: false,
            edited: false,
            system: None,
            meta: Default::default(),
        }
    }

    /// Fond ambre du fil : c'est mon identifiant dans `mentions` qui décide
    /// (une réponse à mon message sans mention ne m'y met pas), ou @everyone,
    /// ou un de mes rôles.
    #[test]
    fn mentioned_follows_the_mentions_list() {
        let s = store();
        let plain = msg(CHAN, Some(G), "rien");
        assert!(!mentioned(&s, &plain));
        // Réponse à l'un de mes messages, « @ » désactivé : pas dans `mentions`.
        let mut reply = msg(CHAN, Some(G), "réponse sans mention");
        reply.mentions = vec![(ME + 1, "autre".into())];
        assert!(!mentioned(&s, &reply));
        reply.mentions.push((ME, "moi".into()));
        assert!(mentioned(&s, &reply));
        let mut all = msg(CHAN, Some(G), "@everyone");
        all.mention_everyone = true;
        assert!(mentioned(&s, &all));
        let mut role = msg(CHAN, Some(G), "<@&7>");
        role.meta.mention_roles = vec![ROLE];
        assert!(mentioned(&s, &role));
        role.meta.mention_roles = vec![ROLE + 1];
        assert!(!mentioned(&s, &role), "rôle qui n'est pas le mien");
        // Sans compte connu : jamais.
        assert!(!mentioned(&Store::new(), &all));
    }

    #[test]
    fn alert_from_the_store() {
        let mut s = store();
        let mut m = msg(CHAN, Some(G), "salut <@1> **toi** ||secret|| fin");
        m.mentions = vec![(ME, "moi".into())];
        let a = alert(&s, &m, false, Lang::Fr, true).expect("mention");
        assert_eq!(a.kind, Kind::Mention);
        assert_eq!(a.title, "Bob");
        assert_eq!(a.place.as_deref(), Some("Serveur › #salon100"));
        assert_eq!(a.body, "salut @moi toi (spoiler) fin");
        assert_eq!(a.header.as_deref(), Some("Serveur"));
        assert_eq!(a.clone().without_preview(Lang::Fr).body, "Vous a mentionné", "texte du message masqué");
        assert!(alert(&s, &m, true, Lang::Fr, true).is_some(), "fenêtre active, autre salon ouvert");
        s.select_channel(CHAN);
        assert!(alert(&s, &m, true, Lang::Fr, true).is_none(), "salon ouvert, fenêtre active");
        assert!(alert(&s, &m, false, Lang::Fr, true).is_some(), "salon ouvert, fenêtre en arrière-plan");
        let plain = msg(CHAN, Some(G), "rien");
        assert!(alert(&s, &plain, false, Lang::Fr, true).is_none(), "« mentions » par défaut");
        let mut role = msg(CHAN, Some(G), "<@&7>");
        role.meta.mention_roles = vec![ROLE];
        assert!(role_ping(&s, &role));
        assert_eq!(alert(&s, &role, false, Lang::Fr, true).map(|a| a.kind), Some(Kind::Mention));
        let dm = alert(&s, &msg(300, None, "coucou"), false, Lang::En, true).expect("MP");
        assert_eq!((dm.kind, dm.place.clone(), dm.body.as_str()), (Kind::Message, None, "coucou"));
        assert_eq!(dm.header.as_deref(), Some("Direct Messages"));
        assert_eq!(dm.without_preview(Lang::En).body, "New message");
    }

    #[test]
    fn body_fallbacks_and_truncation() {
        let s = store();
        let mut m = msg(300, None, "");
        m.system = Some(crate::state::SystemKind::Joined);
        assert_eq!(body(&s, &m, Lang::Fr, true), crate::i18n::t::SYSTEM_JOINED.get(Lang::Fr));
        let long = "é".repeat(MAX_BODY + 10);
        let b = body(&s, &msg(300, None, &long), Lang::Fr, true);
        assert_eq!(b.chars().count(), MAX_BODY + 1);
        assert!(b.ends_with('…'));
        assert_eq!(truncate("abc", 3), "abc");
        assert_eq!(body(&s, &msg(300, None, "a\n\n  b"), Lang::Fr, true), "a b");
    }
}
