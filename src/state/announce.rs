//! Salons d'annonces (lot T13) : « Publier » un message (`POST
//! /channels/{id}/messages/{id}/crosspost`, documenté), « Suivre » un salon
//! (`POST /channels/{id}/followers`, documenté), drapeaux reçus. Aussi les
//! adresses de pièces jointes expirées (`POST /attachments/refresh-urls`,
//! non documenté : docs.discord.food, Message › Refresh Attachment URLs).

use super::notice::Problem;
use std::collections::{HashSet, VecDeque};

/// Adresses demandées par requête, au plus (documenté : 1 à 50).
pub const REFRESH_BATCH: usize = 50;
/// Adresses déjà rafraîchies (ou demandées) gardées, au plus : chacune n'est
/// demandée qu'une fois ; au-delà, les plus anciennes sont oubliées.
pub const MAX_REFRESHED: usize = 1024;

/// Réseau -> magasin.
#[derive(Debug)]
pub enum AnnounceEvent {
    /// Drapeaux d'un message changés (MESSAGE_UPDATE ou réponse de « Publier »).
    Flags { channel_id: u64, message_id: u64, crossposted: bool },
    PublishFailed { message_id: u64, problem: Problem },
    Followed { source: u64 },
    FollowFailed { source: u64, problem: Problem },
    /// Paires (adresse d'origine, nouvelle adresse).
    Refreshed { pairs: Vec<(String, String)> },
}

/// Magasin -> réseau.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnnounceCmd {
    Publish { channel_id: u64, message_id: u64 },
    Follow { source: u64, target: u64 },
    /// 1 à `REFRESH_BATCH` adresses `cdn.discordapp.com`.
    Refresh { urls: Vec<String> },
}

/// Ce qui s'est passé, à annoncer une fois.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnnounceNotice {
    Published,
    Followed,
    Failed { follow: bool, problem: Problem },
}

#[derive(Default)]
pub struct Announce {
    publishing: HashSet<u64>,
    following: HashSet<u64>,
    refreshed: HashSet<String>,
    refreshed_order: VecDeque<String>,
    pub(super) notice: Option<AnnounceNotice>,
}

impl Announce {
    pub fn clear(&mut self) {
        *self = Announce::default();
    }

    pub fn publishing(&self, message_id: u64) -> bool {
        self.publishing.contains(&message_id)
    }

    pub fn following(&self, source: u64) -> bool {
        self.following.contains(&source)
    }

    pub(super) fn begin_publish(&mut self, id: u64) -> bool {
        self.publishing.insert(id)
    }

    pub(super) fn end_publish(&mut self, id: u64) {
        self.publishing.remove(&id);
    }

    pub(super) fn begin_follow(&mut self, id: u64) -> bool {
        self.following.insert(id)
    }

    pub(super) fn end_follow(&mut self, id: u64) {
        self.following.remove(&id);
    }

    /// `true` la première fois qu'on voit cette adresse : elle est à demander.
    pub(super) fn want_refresh(&mut self, url: &str) -> bool {
        if !self.refreshed.insert(url.to_string()) {
            return false;
        }
        self.refreshed_order.push_back(url.to_string());
        while self.refreshed_order.len() > MAX_REFRESHED {
            if let Some(old) = self.refreshed_order.pop_front() {
                self.refreshed.remove(&old);
            }
        }
        true
    }

    /// Une nouvelle adresse ne sera jamais redemandée (un seul nouvel essai).
    pub(super) fn note_refreshed(&mut self, url: &str) {
        self.want_refresh(url);
    }
}

/// Fin de validité (ms Unix) d'une adresse signée du CDN de Discord
/// (paramètre `ex`, secondes en hexadécimal).
pub fn expiry_ms(url: &str) -> Option<i64> {
    let query = url.split_once('?')?.1;
    let ex = query.split('&').find_map(|kv| kv.strip_prefix("ex="))?;
    i64::from_str_radix(ex, 16).ok()?.checked_mul(1000)
}

pub fn is_expired(url: &str, now_ms: i64) -> bool {
    expiry_ms(url).is_some_and(|e| e <= now_ms)
}

/// Adresse du proxy média refaite avec la signature d'une adresse
/// rafraîchie : même chemin, paramètres `ex`, `is`, `hm` de la nouvelle
/// (les autres, comme la taille, sont gardés).
pub fn resign(proxy: &str, refreshed: &str) -> Option<String> {
    let (path, old_q) = proxy.split_once('?').unwrap_or((proxy, ""));
    let new_q = refreshed.split_once('?')?.1;
    let sig = |kv: &&str| ["ex=", "is=", "hm="].iter().any(|p| kv.starts_with(p));
    let mut parts: Vec<&str> = new_q.split('&').filter(sig).collect();
    parts.extend(old_q.split('&').filter(|kv| !kv.is_empty() && !sig(kv)));
    Some(format!("{path}?{}", parts.join("&")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_from_signed_urls() {
        let u = "https://cdn.discordapp.com/attachments/1/2/a.png?ex=65a0b1c2&is=659f6042&hm=abc&";
        assert_eq!(expiry_ms(u), Some(0x65a0_b1c2 * 1000));
        assert!(is_expired(u, 0x65a0_b1c2 * 1000) && !is_expired(u, 0x65a0_b1c2 * 1000 - 1));
        assert_eq!(expiry_ms("https://cdn.discordapp.com/a.png"), None);
        assert_eq!(expiry_ms("https://x/a.png?ex=zz"), None);
        assert!(!is_expired("https://cdn.discordapp.com/a.png", i64::MAX));
    }

    #[test]
    fn proxy_takes_the_new_signature() {
        let proxy = "https://media.discordapp.net/attachments/1/2/a.png?ex=1&is=2&hm=3&width=10";
        let fresh = "https://cdn.discordapp.com/attachments/1/2/a.png?ex=a&is=b&hm=c&";
        assert_eq!(resign(proxy, fresh).as_deref(), Some("https://media.discordapp.net/attachments/1/2/a.png?ex=a&is=b&hm=c&width=10"));
        assert_eq!(resign(proxy, "https://cdn.discordapp.com/sans-requete"), None);
    }

    #[test]
    fn each_url_is_refreshed_once() {
        let mut a = Announce::default();
        assert!(a.want_refresh("u") && !a.want_refresh("u"));
        for i in 0..MAX_REFRESHED {
            a.want_refresh(&i.to_string());
        }
        assert!(a.want_refresh("u"), "oubliée au-delà de la borne");
    }
}
