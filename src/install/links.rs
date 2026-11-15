//! Liens Discord : `discord://…` (protocole enregistré, lancement par un
//! autre programme) et `https://discord.com/…` (collés dans un message).
//! Analyse pure : ce qu'un lien demande d'ouvrir dans Ferrocord, ou rien.
//!
//! Formes reconnues (celles du client web et de l'application officielle) :
//! `/channels/{serveur|@me}/{salon}[/{message}]`, `/channels/{serveur}`,
//! `/invite/{code}`, `discord.gg/{code}`, `/users/{id}`, `/app` et
//! `/channels/@me` (accueil).

/// Ce qu'un lien ouvre dans Ferrocord.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Accueil (messages privés, page Amis).
    Home,
    Guild(u64),
    /// Salon d'un serveur, ou conversation privée (`guild` : `None`).
    Channel { guild: Option<u64>, channel: u64 },
    Message { guild: Option<u64>, channel: u64, message: u64 },
    Invite(String),
    User(u64),
}

/// Domaines du client web de Discord.
const WEB_HOSTS: [&str; 6] = ["discord.com", "www.discord.com", "ptb.discord.com", "canary.discord.com", "discordapp.com", "www.discordapp.com"];
/// Domaines des invitations courtes.
const INVITE_HOSTS: [&str; 2] = ["discord.gg", "www.discord.gg"];
/// Longueur maximale d'un lien accepté (un lien de message en fait moins de 100).
pub const MAX_LINK: usize = 512;

/// Lien Discord compris par Ferrocord ; `None` pour tout le reste (qui
/// s'ouvre alors dans le navigateur, ou nulle part pour `discord://`).
pub fn parse(link: &str) -> Option<Target> {
    let link = link.trim();
    if link.len() > MAX_LINK || link.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    let lower = link.to_ascii_lowercase();
    if lower.starts_with("discord:") {
        // `discord://-/channels/…`, `discord:///invite/…`, `discord://invite/…`.
        let rest = link["discord:".len()..].trim_start_matches('/');
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        let path = clean_path(path);
        return match host.to_ascii_lowercase().as_str() {
            "" | "-" | "app" => route(&path),
            h if WEB_HOSTS.contains(&h) => route(&path),
            // Pas d'hôte : le premier mot est déjà le chemin (`discord://invite/abc`).
            _ => route(&clean_path(rest)),
        };
    }
    let rest = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://"))?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = &rest[..end];
    // `discord.com@ailleurs.tld`, ports : refusés.
    if host.contains(['@', ':']) {
        return None;
    }
    // Le chemin d'origine (casse des codes d'invitation conservée).
    let offset = link.len() - rest.len() + end;
    let path = clean_path(&link[offset..]);
    if WEB_HOSTS.contains(&host) {
        route(&path)
    } else if INVITE_HOSTS.contains(&host) {
        let mut parts = path.split('/').filter(|s| !s.is_empty());
        let code = parts.next()?;
        if parts.next().is_some() {
            return None;
        }
        invite(code)
    } else {
        None
    }
}

/// Chemin sans la requête ni l'ancre.
fn clean_path(path: &str) -> String {
    let end = path.find(['?', '#']).unwrap_or(path.len());
    path[..end].to_string()
}

fn id(s: &str) -> Option<u64> {
    if s.is_empty() || s.len() > 20 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok().filter(|n| *n > 0)
}

fn invite(code: &str) -> Option<Target> {
    let ok = (2..=32).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    ok.then(|| Target::Invite(code.to_string()))
}

/// Chemin du client web → cible.
fn route(path: &str) -> Option<Target> {
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match parts.as_slice() {
        [] | ["app"] | ["channels"] | ["channels", "@me"] => Some(Target::Home),
        ["channels", g] => id(g).map(Target::Guild),
        ["channels", g, c] => Some(Target::Channel { guild: guild(g)?, channel: id(c)? }),
        ["channels", g, c, m] => Some(Target::Message { guild: guild(g)?, channel: id(c)?, message: id(m)? }),
        ["invite", code] => invite(code),
        ["users", u] => id(u).map(Target::User),
        _ => None,
    }
}

/// `@me` → `Some(None)` ; identifiant → `Some(Some(id))`.
fn guild(s: &str) -> Option<Option<u64>> {
    if s == "@me" { Some(None) } else { id(s).map(Some) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_links() {
        assert_eq!(parse("https://discord.com/channels/1/2/3"), Some(Target::Message { guild: Some(1), channel: 2, message: 3 }));
        assert_eq!(parse("https://ptb.discord.com/channels/@me/2/3/"), Some(Target::Message { guild: None, channel: 2, message: 3 }));
        assert_eq!(parse("https://discord.com/channels/1/2"), Some(Target::Channel { guild: Some(1), channel: 2 }));
        assert_eq!(parse("https://discord.com/channels/@me/2"), Some(Target::Channel { guild: None, channel: 2 }));
        assert_eq!(parse("https://discordapp.com/channels/1"), Some(Target::Guild(1)));
        assert_eq!(parse("https://discord.com/channels/@me"), Some(Target::Home));
        assert_eq!(parse("https://discord.com/app"), Some(Target::Home));
        assert_eq!(parse("https://discord.com/users/42?x=1#y"), Some(Target::User(42)));
        assert_eq!(parse("https://discord.gg/RustFr"), Some(Target::Invite("RustFr".into())));
        assert_eq!(parse("https://DISCORD.com/invite/abc-DEF"), Some(Target::Invite("abc-DEF".into())));
    }

    #[test]
    fn protocol_links() {
        assert_eq!(parse("discord://-/channels/1/2/3"), Some(Target::Message { guild: Some(1), channel: 2, message: 3 }));
        assert_eq!(parse("discord:///channels/@me/9"), Some(Target::Channel { guild: None, channel: 9 }));
        assert_eq!(parse("discord://invite/Code9"), Some(Target::Invite("Code9".into())));
        assert_eq!(parse("discord:///invite/Code9"), Some(Target::Invite("Code9".into())));
        assert_eq!(parse("discord://-/users/7"), Some(Target::User(7)));
        assert_eq!(parse("discord://discord.com/channels/5"), Some(Target::Guild(5)));
        assert_eq!(parse("discord://"), Some(Target::Home));
    }

    #[test]
    fn foreign_or_damaged_links_are_refused() {
        for bad in [
            "https://evil.tld/channels/1/2/3",
            "https://discord.com.evil.tld/channels/1/2",
            "https://discord.com@evil.tld/channels/1/2",
            "https://discord.com:8443/channels/1/2",
            "https://discord.com/channels/x/2",
            "https://discord.com/channels/1/2/3/4",
            "https://discord.com/channels/0/2",
            "https://discord.com/users/123456789012345678901",
            "https://discord.gg/a",
            "https://discord.gg/abc/def",
            "https://discord.gg/ab%2F",
            "https://discord.com/settings",
            "discord://-/channels/1/2 && calc",
            "ftp://discord.com/channels/1/2",
            "file:///C:/Windows",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
        assert_eq!(parse(&format!("https://discord.gg/{}", "a".repeat(MAX_LINK))), None);
    }
}
