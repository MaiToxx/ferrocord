//! Mise en mots des valeurs typées du magasin et du réseau (statuts,
//! erreurs, notifications), dans une langue et avec une horloge données.

use crate::i18n::{f, t, time, Lang};
use crate::state::{Cause, Notice, Problem, Status};

/// Heure seule (« 14:05 », « 2:05 PM »).
fn at(lang: Lang, ms: i64, h24: bool) -> String {
    time::clock(lang, ms, false, h24)
}

/// Nom d'un MP, ou « Conversation » s'il est arrivé sans nom.
pub fn dm_name(lang: Lang, name: &str) -> &str {
    if name.is_empty() { t::UNNAMED_DM.get(lang) } else { name }
}

/// Nom d'un serveur, ou « Serveur » s'il est arrivé sans nom.
pub fn guild_name(lang: Lang, name: &str) -> &str {
    if name.is_empty() { t::UNNAMED_GUILD.get(lang) } else { name }
}

/// Statut de la connexion (vide au repos).
pub fn status(lang: Lang, s: &Status, h24: bool) -> String {
    match s {
        Status::Idle => String::new(),
        Status::Connecting => t::CONNECTING.get(lang).into(),
        Status::Resuming => t::RESUMING.get(lang).into(),
        Status::Connected => t::CONNECTED.get(lang).into(),
        Status::IdentifyCap { retry_ms } => f::identify_cap(lang, at(lang, *retry_ms, h24)),
        Status::PacketTooLarge => t::PACKET_TOO_LARGE.get(lang).into(),
        Status::Disconnected { cause: c, no_compression: false } => f::disconnected(lang, cause(lang, c)),
        Status::Disconnected { cause: c, no_compression: true } => f::disconnected_no_compression(lang, cause(lang, c)),
    }
}

pub fn cause(lang: Lang, c: &Cause) -> String {
    match c {
        Cause::NoSessionState => t::NO_SESSION_STATE.get(lang).into(),
        Cause::BadHello => t::BAD_HELLO.get(lang).into(),
        Cause::CorruptStream => t::CORRUPT_STREAM.get(lang).into(),
        Cause::TooLargePacket => t::TOO_LARGE_PACKET.get(lang).into(),
        Cause::BadHeartbeat { ms } => f::bad_heartbeat_interval(lang, ms),
        Cause::DecodeInterrupted(e) => f::decode_interrupted(lang, e),
        Cause::Transport(e) => e.clone(),
    }
}

pub fn problem(lang: Lang, p: &Problem, h24: bool) -> String {
    match p {
        Problem::BadToken => t::BAD_TOKEN.get(lang).into(),
        Problem::Forbidden => t::FORBIDDEN.get(lang).into(),
        Problem::TooManyRequests => t::TOO_MANY.get(lang).into(),
        Problem::TooManyQueues => t::TOO_MANY_QUEUES.get(lang).into(),
        Problem::Network(e) => f::network(lang, e),
        Problem::Http(s) => f::http_status(lang, s),
        Problem::ReadyUnreadable(e) => f::ready_unreadable(lang, e),
        Problem::Api(m) => m.clone(),
        Problem::Blocked { retry_ms } => f::blocked(lang, at(lang, *retry_ms, h24)),
        Problem::Refused { code } => f::refused(lang, code),
        Problem::Timeout { until_ms } => f::timeout_reason(lang, time::until(lang, *until_ms, h24)),
        Problem::MfaRequired => crate::i18n::table::admin::t::MFA_REQUIRED.get(lang).into(),
        Problem::Profile(r) => {
            use crate::i18n::table::quick::t as qt;
            use crate::state::ProfileRefusal as R;
            match r {
                R::NameRateLimit => qt::ERR_NAME_RATE,
                R::ImageRateLimit => qt::ERR_IMAGE_RATE,
                R::ImageTooLarge => qt::ERR_IMAGE_LARGE,
                R::Nitro => qt::ERR_NITRO,
            }
            .get(lang)
            .into()
        }
    }
}

pub fn notice(lang: Lang, n: &Notice, h24: bool) -> String {
    match n {
        Notice::NotSent(p) => f::not_sent(lang, problem(lang, p, h24)),
        Notice::Unconfirmed => t::SEND_UNCONFIRMED.get(lang).into(),
        Notice::LostAccess => t::LOST_ACCESS.get(lang).into(),
        Notice::Error(p) => problem(lang, p, h24),
        Notice::SettingNotSaved(p) => crate::i18n::table::s::f::setting_not_saved(lang, problem(lang, p, h24)),
        Notice::SettingOutdated => crate::i18n::table::s::t::SETTING_OUTDATED.get(lang).into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// En français, les textes sont ceux d'avant le lot (espaces insécables près).
    #[test]
    fn french_wording_is_unchanged() {
        let fr = Lang::Fr;
        assert_eq!(status(fr, &Status::Connecting, true), "Connexion…");
        assert_eq!(status(fr, &Status::Idle, true), "");
        assert_eq!(
            status(fr, &Status::Disconnected { cause: Cause::Transport("IO error".into()), no_compression: false }, true),
            "Déconnecté (IO error), nouvelle tentative…"
        );
        assert_eq!(
            status(fr, &Status::Disconnected { cause: Cause::CorruptStream, no_compression: true }, true),
            "Déconnecté (flux compressé illisible), nouvelle tentative sans compression…"
        );
        assert_eq!(status(fr, &Status::Disconnected { cause: Cause::BadHeartbeat { ms: 12 }, no_compression: false }, true), "Déconnecté (heartbeat_interval anormal (12 ms)), nouvelle tentative…");
        assert_eq!(problem(fr, &Problem::Http(500), true), "Erreur HTTP 500");
        assert_eq!(problem(fr, &Problem::Api("Missing Access".into()), true), "Missing Access");
        assert_eq!(problem(fr, &Problem::Refused { code: 4013 }, true), "Connexion refusée par Discord (code 4013).");
        assert_eq!(notice(fr, &Notice::NotSent(Problem::TooManyRequests), true), "Message non envoyé\u{a0}: Trop de requêtes, réessayez plus tard");
        assert_eq!(notice(fr, &Notice::LostAccess, true), "Vous n'avez plus accès à ce salon.");
    }

    #[test]
    fn english_wording() {
        let en = Lang::En;
        assert_eq!(status(en, &Status::Resuming, true), "Resuming session…");
        assert_eq!(notice(en, &Notice::NotSent(Problem::Forbidden), true), "Message not sent: You don't have access to this channel");
        assert_eq!(problem(en, &Problem::Network("timeout".into()), true), "Network: timeout");
        assert_eq!(problem(en, &Problem::ReadyUnreadable("eof".into()), true), "Unreadable READY: eof");
        assert_eq!(notice(en, &Notice::Unconfirmed, false), "Sending not confirmed: the message may have been sent");
        // Heure suivie selon l'horloge choisie.
        let ms = 1_700_000_000_000;
        assert!(problem(en, &Problem::Blocked { retry_ms: ms }, false).ends_with('M'));
        assert!(problem(en, &Problem::Timeout { until_ms: ms }, true).starts_with("timed out until "));
    }

    /// Exclusion : l'échéance s'insère en milieu de phrase, sans majuscule
    /// (le détail jour même / autre jour est vérifié par `deadlines_in_both_languages`).
    #[test]
    fn timeout_deadline_mid_sentence() {
        let soon = crate::state::store::now_ms() + 60_000;
        for (lang, start) in [(Lang::Fr, "exclusion temporaire jusqu'"), (Lang::En, "timed out until ")] {
            for h24 in [true, false] {
                let s = problem(lang, &Problem::Timeout { until_ms: soon }, h24);
                assert!(s.starts_with(start), "{s}");
                assert!(!s.contains("Aujourd") && !s.contains("Today"), "{s}");
                assert_eq!(s.ends_with('M'), !h24, "{s}");
            }
        }
    }

    /// Serveur ou MP arrivé sans nom : un nom traduit.
    #[test]
    fn unnamed_dm_and_guild() {
        assert_eq!(dm_name(Lang::Fr, ""), "Conversation");
        assert_eq!(guild_name(Lang::Fr, ""), "Serveur");
        assert_eq!(guild_name(Lang::En, ""), "Server");
        assert_eq!(dm_name(Lang::En, "Les amis"), "Les amis");
    }
}
