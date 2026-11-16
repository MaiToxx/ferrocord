//! Cartes d'invitation (lot T13) : codes lus dans le texte d'un message à
//! sa réception, invitation demandée une fois par code et par session
//! (`GET /invites/{code}?with_counts=true`, documenté), gardée dans un cache
//! borné, et « Rejoindre », qui passe par `Store::join_invite` du lot
//! serveurs (`POST /invites/{code}`, `state::servers`).

use super::image::ImageRef;
use super::notice::Problem;
use std::collections::{HashMap, VecDeque};

/// Invitations gardées au plus : les plus anciennes partent d'abord (une
/// invitation partie peut être redemandée si son message revient à l'écran).
pub const MAX_INVITES: usize = 128;
/// Invitations demandées en même temps (lot « fiable »).
pub const MAX_IN_FLIGHT: usize = 4;
/// Longueur d'un code d'invitation acceptée (les codes de Discord font de 2
/// à 32 caractères, lettres, chiffres et tirets).
const CODE_MAX: usize = 32;

/// Ce que montre une carte.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Invite {
    pub code: String,
    /// 0 : serveur, 1 : groupe, 2 : ami (documenté).
    pub kind: u8,
    pub guild_id: Option<u64>,
    /// Nom du serveur, ou du groupe, ou de qui invite.
    pub name: String,
    pub icon: Option<ImageRef>,
    pub online: Option<u32>,
    pub members: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InviteState {
    Loading,
    Ready(Invite),
    /// Code inconnu ou expiré (404).
    Invalid,
    Failed(Problem),
}

/// Réseau -> magasin.
#[derive(Debug)]
pub enum InviteEvent {
    Loaded { code: String, invite: Result<Invite, Problem> },
    Invalid { code: String },
}

/// Magasin -> réseau.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InviteCmd {
    Get { code: String },
}

/// Où en est « Rejoindre » pour un code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinState {
    Joining,
    Captcha,
}

#[derive(Default)]
pub struct Invites {
    map: HashMap<String, InviteState>,
    order: VecDeque<String>,
    joins: HashMap<String, JoinState>,
}

impl Invites {
    pub fn clear(&mut self) {
        *self = Invites::default();
    }

    pub fn get(&self, code: &str) -> Option<&InviteState> {
        self.map.get(code)
    }

    pub fn join_state(&self, code: &str) -> Option<JoinState> {
        self.joins.get(code).copied()
    }

    /// `true` si le code est nouveau (à demander).
    pub(super) fn want(&mut self, code: &str) -> bool {
        if self.map.contains_key(code) {
            return false;
        }
        // Un écran plein d'invitations ne fait pas partir des dizaines de
        // requêtes d'un coup : les suivantes partent quand les premières reviennent.
        if self.map.values().filter(|s| matches!(s, InviteState::Loading)).count() >= MAX_IN_FLIGHT {
            return false;
        }
        self.set(code, InviteState::Loading);
        true
    }

    pub(super) fn set(&mut self, code: &str, v: InviteState) {
        if self.map.insert(code.to_string(), v).is_none() {
            self.order.push_back(code.to_string());
            while self.order.len() > MAX_INVITES {
                if let Some(old) = self.order.pop_front() {
                    self.map.remove(&old);
                    self.joins.remove(&old);
                }
            }
        }
    }

    pub(super) fn set_join(&mut self, code: &str, s: Option<JoinState>) {
        match s {
            Some(s) => {
                self.joins.insert(code.to_string(), s);
            }
            None => {
                self.joins.remove(code);
            }
        }
    }

    /// Fin d'un « Rejoindre » (réponse de `state::servers`) : les cartes en
    /// cours reviennent au bouton, ou montrent le captcha (jamais contourné).
    pub(super) fn end_joins(&mut self, captcha: bool) {
        if captcha {
            self.joins.values_mut().filter(|s| **s == JoinState::Joining).for_each(|s| *s = JoinState::Captcha);
        } else {
            self.joins.retain(|_, s| *s != JoinState::Joining);
        }
    }
}

fn code_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-'
}

/// Codes d'invitation du texte (`discord.gg/…`, `discord.com/invite/…`,
/// `discordapp.com/invite/…`), sans doublon, au plus `max`.
pub fn invite_codes(text: &str, max: usize) -> Vec<String> {
    const PREFIXES: [&str; 4] = ["discord.gg/", "discord.com/invite/", "discordapp.com/invite/", "discord.gg/invite/"];
    let mut out: Vec<String> = Vec::new();
    let lower = text.to_ascii_lowercase();
    let mut i = 0;
    while i < lower.len() && out.len() < max {
        let rest = &lower[i..];
        let Some((at, prefix)) = PREFIXES.iter().filter_map(|p| rest.find(p).map(|a| (a, *p))).min_by_key(|(a, p)| (*a, std::cmp::Reverse(p.len()))) else { break };
        let start = i + at + prefix.len();
        // Le domaine doit commencer un mot (pas `notdiscord.gg`).
        let before = lower[..i + at].chars().next_back();
        let host_ok = before.is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '.' || c == '-'));
        let code: String = text[start..].chars().take_while(|c| code_char(*c)).collect();
        if host_ok && (2..=CODE_MAX).contains(&code.len()) && !out.contains(&code) {
            out.push(code.clone());
        }
        // Toujours sur une limite de caractère : le code est en ASCII.
        i = start + if code.is_empty() { text[start..].chars().next().map_or(1, char::len_utf8) } else { code.len() };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_in_text() {
        assert_eq!(invite_codes("Venez https://discord.gg/abcDEF12 !", 3), vec!["abcDEF12"]);
        assert_eq!(invite_codes("discord.com/invite/rust-fr et https://discordapp.com/invite/xyz et discord.gg/rust-fr", 3), vec!["rust-fr", "xyz"]);
        assert_eq!(invite_codes("notdiscord.gg/abc discord.gg/a", 3), Vec::<String>::new(), "faux domaine, code trop court");
        assert_eq!(invite_codes("discord.gg/aa discord.gg/bb discord.gg/cc discord.gg/dd", 2), vec!["aa", "bb"]);
        assert_eq!(invite_codes("<https://DISCORD.GG/Zz9>", 3), vec!["Zz9"]);
        assert!(invite_codes("discord.gg/", 3).is_empty());
        assert!(invite_codes("é discord.gg/éé", 3).is_empty());
    }

    #[test]
    fn cache_is_bounded() {
        let mut c = Invites::default();
        for i in 0..(MAX_INVITES + 5) {
            assert!(c.want(&format!("c{i}")));
            c.set(&format!("c{i}"), InviteState::Invalid);
        }
        assert!(!c.want(&format!("c{}", MAX_INVITES + 4)), "une requête par code");
        assert!(c.get("c0").is_none() && c.get("c5").is_some());
    }
    #[test]
    fn only_a_few_invites_are_asked_at_once() {
        let mut c = Invites::default();
        for i in 0..MAX_IN_FLIGHT {
            assert!(c.want(&format!("c{i}")));
        }
        assert!(!c.want("suivante"), "les premières ne sont pas revenues");
        assert!(c.get("suivante").is_none(), "elle sera redemandée par sa carte");
        c.set("c0", InviteState::Invalid);
        assert!(c.want("suivante"), "une place s'est libérée");
        assert!(!c.want("encore"));
    }
}
