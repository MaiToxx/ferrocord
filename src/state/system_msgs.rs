//! Messages système (lot T13) : type documenté de l'objet Message vers
//! `SystemKind`, et leur phrase dans la langue courante, faite une fois à la
//! réception par le magasin (`Store::render`). Aussi ce que le lot ajoute à
//! un message (`Extra`) : sondage, invitations, publication d'annonce,
//! référence d'un message système, commande utilisée, appel.

use super::model::{snowflake_ms, Message};
use super::notice::SystemKind;
use super::polls::Poll;
use super::social::Emoji;
use crate::i18n::table::polls::{f, p, t};
use crate::i18n::{Lang, Text};

/// Message visé par une référence (`message_reference`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Ref {
    pub guild_id: Option<u64>,
    pub channel_id: Option<u64>,
    pub message_id: Option<u64>,
}

/// Commande d'application utilisée (`interaction_metadata`, `interaction`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Command {
    /// Nom affiché de qui l'a utilisée.
    pub user: String,
    /// Nom de la commande (sans « / »), vide s'il est inconnu.
    pub name: String,
    /// Commande de discussion (« /nom ») plutôt que menu contextuel.
    pub slash: bool,
}

/// Appel d'un MP (`call`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Call {
    pub participants: Vec<u64>,
    pub ended_ms: Option<i64>,
}

/// Résultat d'un sondage (embed `poll_result` d'un message de type 46).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct PollResult {
    pub question: String,
    /// Réponse gagnante ; absente en cas d'égalité ou sans vote.
    pub winner: Option<(String, Option<Emoji>)>,
    pub winner_votes: u32,
    pub total: u32,
}

/// Ce que le lot T13 ajoute à un message (dans `MessageMeta::extra`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Extra {
    pub poll: Option<Poll>,
    /// Codes d'invitation cités dans le texte (au plus `MAX_INVITES`).
    pub invites: Vec<String>,
    /// CROSSPOSTED : mon annonce a été publiée.
    pub crossposted: bool,
    /// IS_CROSSPOST : message venu d'un salon suivi.
    pub source: Option<Ref>,
    /// Ce que vise un message système (message épinglé, fil, sondage…).
    pub target: Option<Ref>,
    pub command: Option<Command>,
    pub call: Option<Call>,
    pub result: Option<PollResult>,
}

impl Extra {
    pub fn is_empty(&self) -> bool {
        *self == Extra::default()
    }
}

/// Invitations lues au plus par message.
pub const MAX_INVITES: usize = 3;

/// Indicateurs documentés d'un message (`flags`).
pub const CROSSPOSTED: u64 = 1 << 0;
pub const IS_CROSSPOST: u64 = 1 << 1;

/// Type de message vers `SystemKind`. `has_content` : texte, pièce jointe ou
/// embed présent. Les types qui portent leur valeur dans `content` (nouveau
/// nom, fil créé, salon suivi…) restent système avec un texte ; un autre type
/// inconnu avec un texte est montré comme un message ordinaire.
pub fn kind_of(code: u8, has_content: bool) -> Option<SystemKind> {
    use SystemKind::*;
    let k = match code {
        0 | 19 | 20 | 21 | 23 => return None,
        1 => RecipientAdd,
        2 => RecipientRemove,
        3 => Call,
        4 => return Some(ChannelName),
        5 => ChannelIcon,
        6 => Pinned,
        7 => Joined,
        8 => return Some(Boosted),
        9..=11 => return Some(BoostTier(code - 8)),
        12 => return Some(FollowAdd),
        14 => DiscoveryDisqualified,
        15 => DiscoveryRequalified,
        16 | 17 => DiscoveryWarning,
        18 => return Some(ThreadCreated),
        22 => InviteReminder,
        24 => return Some(AutoMod),
        25 => RoleSubscription,
        27 => return Some(StageStart),
        28 => return Some(StageEnd),
        29 => StageSpeaker,
        31 => return Some(StageTopic),
        32 => AppSubscription,
        36 => IncidentOn,
        37 => IncidentOff,
        38 => IncidentRaid,
        39 => IncidentFalseAlarm,
        44 => Purchase,
        46 => return Some(PollResult),
        _ => Other,
    };
    (!has_content).then_some(k)
}

/// Ce que la phrase d'un message système doit connaître hors du message.
#[derive(Clone, Copy, Debug, Default)]
pub struct Names<'a> {
    pub me: Option<u64>,
    /// Nom du serveur du message.
    pub guild: Option<&'a str>,
}

/// Durée d'un appel en mots, comme dans le client : « quelques secondes »,
/// « une minute », « N minutes », « une heure », « N heures ».
pub fn call_duration(lang: Lang, ms: i64) -> String {
    let s = ms.max(0) / 1000;
    match s {
        0..60 => t::FEW_SECONDS.get(lang).to_string(),
        60..120 => t::ONE_MINUTE.get(lang).to_string(),
        120..3600 => p::minutes(lang, (s / 60) as u64),
        3600..7200 => t::ONE_HOUR.get(lang).to_string(),
        _ => p::hours(lang, (s / 3600) as u64),
    }
}

/// Appel terminé auquel `me` n'a pas pris part (ni lancé, ni rejoint) : « Appel manqué ».
pub fn call_missed(m: &Message, me: Option<u64>) -> bool {
    let Some(call) = m.meta.extra.as_deref().and_then(|e| e.call.as_ref()) else { return false };
    call.ended_ms.is_some() && !me.is_some_and(|me| me == m.author_id || call.participants.contains(&me))
}

/// Phrase d'un message système ; `m.content` est son texte d'origine.
pub fn text(lang: Lang, kind: SystemKind, m: &Message, names: Names) -> String {
    use SystemKind::*;
    let name = m.author.as_str();
    let value = m.content.trim();
    let extra = m.meta.extra.as_deref();
    let target = m.mentions.first().map(|(_, n)| n.as_str());
    match kind {
        Joined => {
            // Variante tirée de l'heure du message : la même à chaque rendu.
            let joins: [fn(Lang, &str) -> String; 8] = [
                |l, n| f::join_0(l, n),
                |l, n| f::join_1(l, n),
                |l, n| f::join_2(l, n),
                |l, n| f::join_3(l, n),
                |l, n| f::join_4(l, n),
                |l, n| f::join_5(l, n),
                |l, n| f::join_6(l, n),
                |l, n| f::join_7(l, n),
            ];
            let i = snowflake_ms(m.id).rem_euclid(joins.len() as i64) as usize;
            joins.get(i).map_or_else(|| f::join_2(lang, name), |j| j(lang, name))
        }
        RecipientAdd => f::recipient_add(lang, name, target.unwrap_or("?")),
        RecipientRemove => match m.mentions.first() {
            Some((id, n)) if *id != m.author_id => f::recipient_remove(lang, name, n),
            _ => f::recipient_left(lang, name),
        },
        Call => match extra.and_then(|e| e.call.as_ref()).and_then(|c| c.ended_ms) {
            Some(_) if call_missed(m, names.me) => f::call_missed(lang, name),
            Some(end) => f::call_lasted(lang, name, call_duration(lang, end - snowflake_ms(m.id))),
            None => f::call_started(lang, name),
        },
        ChannelName => f::channel_name(lang, name, value),
        ChannelIcon => f::channel_icon(lang, name),
        Pinned => f::pinned(lang, name),
        Boosted => match value.parse::<u64>() {
            Ok(n) if n > 1 => f::boost_times(lang, name, n),
            _ => f::boost(lang, name),
        },
        BoostTier(tier) => f::boost_tier(lang, name, names.guild.unwrap_or("?"), tier),
        FollowAdd => f::follow_add(lang, name, value),
        DiscoveryDisqualified => t::SYS_DISCOVERY_DISQUALIFIED.get(lang).to_string(),
        DiscoveryRequalified => t::SYS_DISCOVERY_REQUALIFIED.get(lang).to_string(),
        DiscoveryWarning => t::SYS_DISCOVERY_WARNING.get(lang).to_string(),
        ThreadCreated => f::thread_created(lang, name, value),
        InviteReminder => t::SYS_INVITE_REMINDER.get(lang).to_string(),
        AutoMod => t::SYS_AUTOMOD.get(lang).to_string(),
        RoleSubscription => f::role_subscription(lang, name),
        StageStart => f::stage_start(lang, name, value),
        StageEnd => f::stage_end(lang, name, value),
        StageSpeaker => f::stage_speaker(lang, name),
        StageTopic => f::stage_topic(lang, name, value),
        AppSubscription => f::app_subscription(lang, name),
        IncidentOn => f::incident_on(lang, name),
        IncidentOff => f::incident_off(lang, name),
        IncidentRaid => f::incident_raid(lang, name),
        IncidentFalseAlarm => f::incident_false(lang, name),
        Purchase => f::purchase(lang, name),
        PollResult => {
            let r = extra.and_then(|e| e.result.as_ref());
            let question = r.map_or("", |r| r.question.as_str());
            match r {
                // L'emoji de la réponse n'entre pas dans la phrase : la police
                // de l'interface n'a pas ses glyphes (carré vide).
                Some(self::PollResult { winner: Some((answer, _)), winner_votes, total, .. }) => {
                    f::poll_won(lang, name, question, answer,p::votes(lang, u64::from(*winner_votes)), total)
                }
                Some(r) if r.total > 0 => f::poll_tie(lang, name, question),
                _ => f::poll_empty(lang, name, question),
            }
        }
        Other => crate::i18n::t::SYSTEM_OTHER.get(lang).to_string(),
    }
}

/// Texte court, sans le message (notifications).
pub fn generic(kind: SystemKind) -> Text {
    use crate::i18n::t as tt;
    match kind {
        SystemKind::Joined => tt::SYSTEM_JOINED,
        SystemKind::Boosted | SystemKind::BoostTier(_) => tt::SYSTEM_BOOSTED,
        SystemKind::Pinned => tt::SYSTEM_PINNED,
        SystemKind::PollResult => t::POLL_ENDED,
        SystemKind::AutoMod => t::SYS_AUTOMOD,
        _ => tt::SYSTEM_OTHER,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::Avatar;

    fn sys(id: u64, content: &str, mentions: Vec<(u64, String)>) -> Message {
        Message {
            id,
            channel_id: 1,
            guild_id: Some(2),
            author_id: 3,
            author: "Bob".into(),
            author_avatar: Avatar::default_for(3),
            content: content.into(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            mentions,
            mention_everyone: false,
            edited: false,
            system: None,
            meta: Default::default(),
        }
    }

    #[test]
    fn documented_types() {
        assert_eq!(kind_of(0, false), None);
        assert_eq!(kind_of(19, false), None);
        assert_eq!(kind_of(20, true), None);
        assert_eq!(kind_of(7, false), Some(SystemKind::Joined));
        assert_eq!(kind_of(7, true), None, "texte inattendu : montré tel quel");
        assert_eq!(kind_of(4, true), Some(SystemKind::ChannelName));
        assert_eq!(kind_of(10, true), Some(SystemKind::BoostTier(2)));
        assert_eq!(kind_of(18, true), Some(SystemKind::ThreadCreated));
        assert_eq!(kind_of(46, true), Some(SystemKind::PollResult));
        assert_eq!(kind_of(99, false), Some(SystemKind::Other));
        assert_eq!(kind_of(99, true), None);
    }

    #[test]
    fn sentences_name_the_people() {
        let n = Names { me: Some(9), guild: Some("Alpha") };
        let fr = Lang::Fr;
        assert_eq!(text(fr, SystemKind::RecipientAdd, &sys(1, "", vec![(5, "Alice".into())]), n), "Bob a ajouté Alice au groupe.");
        assert_eq!(text(fr, SystemKind::RecipientRemove, &sys(1, "", vec![(3, "Bob".into())]), n), "Bob a quitté le groupe.");
        assert_eq!(text(Lang::En, SystemKind::ThreadCreated, &sys(1, "Sujet", Vec::new()), n), "Bob started a thread: Sujet");
        assert_eq!(text(fr, SystemKind::BoostTier(3), &sys(1, "", Vec::new()), n), "Bob a boosté le serveur\u{a0}! Alpha passe au niveau 3\u{a0}!");
        assert_eq!(text(fr, SystemKind::Boosted, &sys(1, "3", Vec::new()), n), "Bob a boosté le serveur 3 fois\u{a0}!");
        // Même message : même variante d'arrivée ; deux heures voisines peuvent différer.
        let a = text(fr, SystemKind::Joined, &sys(5 << 22, "", Vec::new()), n);
        assert_eq!(a, text(fr, SystemKind::Joined, &sys(5 << 22, "", Vec::new()), n));
        assert!(a.contains("Bob"));
        let variants: std::collections::HashSet<String> = (0..8).map(|i| text(fr, SystemKind::Joined, &sys(i << 22, "", Vec::new()), n)).collect();
        assert_eq!(variants.len(), 8);
    }

    /// Durée d'un appel en mots, dans les deux langues.
    #[test]
    fn call_durations_in_words() {
        for (ms, fr, en) in [
            (-5, "quelques secondes", "a few seconds"),
            (0, "quelques secondes", "a few seconds"),
            (59_999, "quelques secondes", "a few seconds"),
            (60_000, "une minute", "a minute"),
            (119_000, "une minute", "a minute"),
            (120_000, "2 minutes", "2 minutes"),
            (59 * 60_000 + 59_000, "59 minutes", "59 minutes"),
            (3_600_000, "une heure", "an hour"),
            (7_199_000, "une heure", "an hour"),
            (7_200_000, "2 heures", "2 hours"),
            (30 * 3_600_000, "30 heures", "30 hours"),
        ] {
            assert_eq!(call_duration(Lang::Fr, ms), fr, "{ms}");
            assert_eq!(call_duration(Lang::En, ms), en, "{ms}");
        }
    }

    #[test]
    fn calls_and_poll_results() {
        let n = Names { me: Some(9), guild: None };
        let mut m = sys(1 << 22, "", Vec::new());
        let start = snowflake_ms(m.id);
        m.meta.extra = Some(Box::new(Extra { call: Some(Call { participants: vec![3, 9], ended_ms: Some(start + 5 * 60_000) }), ..Extra::default() }));
        assert_eq!(text(Lang::Fr, SystemKind::Call, &m, n), "Bob a commencé un appel qui a duré 5 minutes.");
        m.meta.extra = Some(Box::new(Extra { call: Some(Call { participants: vec![3, 9], ended_ms: None }), ..Extra::default() }));
        assert_eq!(text(Lang::Fr, SystemKind::Call, &m, n), "Bob a commencé un appel.");
        assert!(!call_missed(&m, n.me), "appel en cours : pas manqué");
        m.meta.extra = Some(Box::new(Extra { call: Some(Call { participants: vec![3], ended_ms: Some(start + 1000) }), ..Extra::default() }));
        assert_eq!(text(Lang::En, SystemKind::Call, &m, n), "Missed call from Bob.");
        assert_eq!(text(Lang::Fr, SystemKind::Call, &m, n), "Appel manqué de Bob.");
        assert!(call_missed(&m, n.me) && !call_missed(&m, Some(3)), "l'auteur ne manque pas son appel");
        let result = PollResult { question: "Pizza ?".into(), winner: Some(("Oui".into(), None)), winner_votes: 3, total: 5 };
        m.meta.extra = Some(Box::new(Extra { result: Some(result), ..Extra::default() }));
        assert_eq!(text(Lang::En, SystemKind::PollResult, &m, n), "Bob's poll has closed (“Pizza ?”): Oui won with 3 votes of 5.");
        m.meta.extra = Some(Box::new(Extra { result: Some(PollResult { question: "Q".into(), winner: None, winner_votes: 0, total: 4 }), ..Extra::default() }));
        assert!(text(Lang::Fr, SystemKind::PollResult, &m, n).ends_with("égalité."));
    }
}
