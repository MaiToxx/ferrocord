//! Débit d'envoi sur la passerelle. Fonctions pures : l'heure est toujours
//! passée en paramètre, pour des tests instantanés et déterministes.
//!
//! Documentation publique de Discord (Gateway › Rate Limiting) : au plus
//! 120 commandes par 60 s et par connexion, sous peine de déconnexion
//! immédiate ; au plus un Identify toutes les 5 s ; `session_start_limit`
//! (nombre de sessions par jour) n'est donné que par `GET /gateway/bot`,
//! réservé aux bots, que ce lot n'appelle pas : un plafond local prudent le
//! remplace.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

/// Commandes autorisées par fenêtre et par connexion (documentation).
pub const COMMANDS_PER_WINDOW: usize = 120;
pub const WINDOW: Duration = Duration::from_secs(60);
/// Écart minimal entre deux Identify (documentation : `max_concurrency` = 1).
pub const IDENTIFY_INTERVAL: Duration = Duration::from_secs(5);
/// Sessions démarrées par 24 h au plus : valeur `total` par défaut de
/// `session_start_limit` documentée pour les bots ; au-delà Discord peut
/// réinitialiser le jeton.
pub const IDENTIFY_PER_DAY: usize = 1000;
pub const DAY: Duration = Duration::from_secs(24 * 3600);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    /// Heartbeat (périodique ou demandé par op 1) : la place lui est réservée.
    Heartbeat,
    /// Toute autre commande (Identify, Resume, et plus tard présence, souscriptions…).
    Normal,
}

/// Fenêtre glissante des envois d'une connexion.
pub struct SendLimiter {
    max: usize,
    window: Duration,
    /// Places que les commandes normales ne peuvent pas prendre.
    reserve: usize,
    sent: VecDeque<Instant>,
}

impl SendLimiter {
    /// `heartbeat_interval` : annoncé par Hello ; il fixe combien de
    /// heartbeats tombent dans une fenêtre, et donc la réserve.
    pub fn new(max: usize, window: Duration, heartbeat_interval: Duration) -> Self {
        let per_window = if heartbeat_interval.is_zero() {
            max
        } else {
            usize::try_from(window.as_millis().div_ceil(heartbeat_interval.as_millis().max(1))).unwrap_or(max)
        };
        // Deux de plus pour les heartbeats demandés par le serveur (op 1).
        let reserve = per_window.saturating_add(2).min(max / 2);
        Self { max, window, reserve, sent: VecDeque::with_capacity(max) }
    }

    fn prune(&mut self, now: Instant) {
        while let Some(&t) = self.sent.front() {
            if now.saturating_duration_since(t) >= self.window {
                self.sent.pop_front();
            } else {
                break;
            }
        }
    }

    fn cap(&self, p: Priority) -> usize {
        match p {
            Priority::Heartbeat => self.max,
            Priority::Normal => self.max - self.reserve,
        }
    }

    /// `None` : l'envoi peut partir maintenant (et il est compté) ; sinon,
    /// heure à laquelle réessayer.
    pub fn try_send(&mut self, p: Priority, now: Instant) -> Option<Instant> {
        self.prune(now);
        let cap = self.cap(p);
        if self.sent.len() < cap {
            self.sent.push_back(now);
            return None;
        }
        // Il faut que `len - cap + 1` envois sortent de la fenêtre.
        let idx = self.sent.len() - cap;
        Some(self.sent.get(idx).map_or(now, |&t| t + self.window))
    }

    #[cfg(test)]
    pub fn reserve(&self) -> usize {
        self.reserve
    }
}

/// Espacement des Identify, pour toute la durée de vie de la tâche de
/// passerelle (toutes connexions confondues).
pub struct IdentifyLimiter {
    interval: Duration,
    per_day: usize,
    recent: VecDeque<Instant>,
}

impl IdentifyLimiter {
    pub fn new(interval: Duration, per_day: usize) -> Self {
        Self { interval, per_day: per_day.max(1), recent: VecDeque::new() }
    }

    /// Heure à partir de laquelle un Identify peut partir.
    pub fn next_allowed(&mut self, now: Instant) -> Instant {
        while self.recent.front().is_some_and(|&t| now.saturating_duration_since(t) >= DAY) {
            self.recent.pop_front();
        }
        let mut at = now;
        if let Some(&last) = self.recent.back() {
            at = at.max(last + self.interval);
        }
        if self.recent.len() >= self.per_day
            && let Some(&oldest) = self.recent.front()
        {
            at = at.max(oldest + DAY);
        }
        at
    }

    /// Le plafond journalier retient-il le prochain Identify ? (à annoncer)
    pub fn daily_cap_reached(&mut self, now: Instant) -> bool {
        self.next_allowed(now);
        self.recent.len() >= self.per_day
    }

    /// Note un Identify envoyé à `now`.
    pub fn record(&mut self, now: Instant) {
        if self.recent.len() >= self.per_day {
            self.recent.pop_front();
        }
        self.recent.push_back(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HB: Duration = Duration::from_millis(41_250);

    #[test]
    fn reserve_follows_heartbeat_interval() {
        assert_eq!(SendLimiter::new(120, WINDOW, HB).reserve(), 4);
        assert_eq!(SendLimiter::new(120, WINDOW, Duration::from_secs(1)).reserve(), 60);
        assert_eq!(SendLimiter::new(120, WINDOW, Duration::ZERO).reserve(), 60);
    }

    #[test]
    fn command_121_is_delayed_until_the_window_frees() {
        let t0 = Instant::now();
        let mut l = SendLimiter::new(120, WINDOW, HB);
        let r = l.reserve() as u64;
        let ms = Duration::from_millis;
        for i in 0..120 - r {
            assert_eq!(l.try_send(Priority::Normal, t0 + ms(i)), None, "envoi {i}");
        }
        // La commande normale suivante attend que la première sorte de la fenêtre.
        assert_eq!(l.try_send(Priority::Normal, t0 + ms(200)), Some(t0 + WINDOW));
        // Les places réservées restent aux heartbeats…
        for i in 0..r {
            assert_eq!(l.try_send(Priority::Heartbeat, t0 + ms(300 + i)), None);
        }
        // … et le 121e envoi, même heartbeat, est retardé.
        assert_eq!(l.try_send(Priority::Heartbeat, t0 + ms(400)), Some(t0 + WINDOW));
        // À l'heure dite, il passe ; la commande normale attend davantage.
        assert_eq!(l.try_send(Priority::Heartbeat, t0 + WINDOW), None);
        assert_eq!(l.try_send(Priority::Normal, t0 + WINDOW), Some(t0 + WINDOW + ms(r + 1)));
    }

    #[test]
    fn heartbeats_are_never_starved_by_commands() {
        let t0 = Instant::now();
        let mut l = SendLimiter::new(120, WINDOW, HB);
        let mut sent_normal = 0;
        let mut now = t0;
        // Dix minutes de commandes en continu (une toutes les 100 ms) et un
        // heartbeat toutes les 41,25 s : chaque heartbeat part à l'heure.
        let end = t0 + Duration::from_secs(600);
        let mut next_hb = t0 + HB;
        while now < end {
            if now >= next_hb {
                assert_eq!(l.try_send(Priority::Heartbeat, now), None, "heartbeat retardé à {:?}", now - t0);
                next_hb += HB;
            }
            if l.try_send(Priority::Normal, now).is_none() {
                sent_normal += 1;
            }
            // Jamais plus de 120 envois dans une fenêtre glissante.
            assert!(l.sent.len() <= 120);
            now += Duration::from_millis(100);
        }
        // Les commandes ont bien été bridées à environ 116 par minute.
        assert!((1100..=1170).contains(&sent_normal), "{sent_normal}");
    }

    #[test]
    fn identify_spacing_and_daily_cap() {
        let t0 = Instant::now();
        let s = Duration::from_secs;
        let mut l = IdentifyLimiter::new(IDENTIFY_INTERVAL, 3);
        assert_eq!(l.next_allowed(t0), t0);
        l.record(t0);
        assert_eq!(l.next_allowed(t0 + s(1)), t0 + s(5));
        assert_eq!(l.next_allowed(t0 + s(9)), t0 + s(9));
        l.record(t0 + s(9));
        l.record(t0 + s(20));
        // Plafond journalier atteint : attendre que le plus ancien ait 24 h.
        assert!(!IdentifyLimiter::new(IDENTIFY_INTERVAL, 3).daily_cap_reached(t0));
        assert!(l.daily_cap_reached(t0 + s(30)));
        assert_eq!(l.next_allowed(t0 + s(30)), t0 + DAY);
        assert_eq!(l.next_allowed(t0 + DAY + s(1)), t0 + DAY + s(1));
        assert_eq!(l.recent.len(), 2);
        // La table ne grandit jamais au-delà du plafond.
        let mut l = IdentifyLimiter::new(Duration::ZERO, 3);
        for i in 0..10 {
            l.record(t0 + s(i));
        }
        assert_eq!(l.recent.len(), 3);
    }
}
