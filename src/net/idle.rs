//! Statut « inactif » automatique (F2) : comme le client de bureau, la
//! présence passe en inactif après dix minutes sans clavier ni souris sur
//! cet ordinateur, et revient au premier geste.
//!
//! Aucune requête : la tâche lit, à un rythme lent et hors du fil
//! d'interface, l'horloge de dernière entrée de Windows
//! (`GetLastInputInfo`, valable pour toute la session de l'utilisateur, pas
//! seulement pour la fenêtre de Ferrocord). Elle n'envoie un événement, donc
//! ne réveille l'interface, qu'aux deux transitions (inactif, de retour) ;
//! au repos, rien n'est redessiné. La présence part ensuite par l'op 3
//! documentée (« Update Presence » : `since`, `status`, `afk`), à travers
//! le même garde-fou que les jeux ([`super::games::PresenceGate`]).

use super::Out;
use crate::state::activity::ActivityEvent;
use crate::state::Event;
use std::time::{Duration, Instant};

/// Délai d'inactivité du client de bureau de Discord (dix minutes).
pub const IDLE_AFTER: Duration = Duration::from_secs(600);
/// Rythme de lecture de l'horloge de dernière entrée.
pub(super) const POLL_EVERY: Duration = Duration::from_secs(20);

/// Suit l'horloge de dernière entrée d'un relevé à l'autre. Elle n'est
/// jamais soustraite (elle reboucle tous les 49 jours) : seule compte la
/// durée pendant laquelle elle n'a pas changé.
pub(super) struct IdleWatch {
    tick: Option<u32>,
    /// Premier relevé où `tick` avait cette valeur.
    since: Instant,
    idle: bool,
}

impl IdleWatch {
    pub(super) fn new(now: Instant) -> Self {
        IdleWatch { tick: None, since: now, idle: false }
    }

    /// Relevé `tick` fait à `now`. Rend la transition, s'il y en a une :
    /// `Some(Some(d))` : inactif depuis `d` ; `Some(None)` : de retour.
    pub(super) fn step(&mut self, tick: u32, now: Instant, after: Duration) -> Option<Option<Duration>> {
        if self.tick != Some(tick) {
            self.tick = Some(tick);
            self.since = now;
            return std::mem::take(&mut self.idle).then_some(None);
        }
        let quiet = now.saturating_duration_since(self.since);
        if !self.idle && quiet >= after {
            self.idle = true;
            return Some(Some(quiet));
        }
        None
    }
}

/// Tâche de fond : un relevé par [`POLL_EVERY`], un événement par transition.
/// S'arrête d'elle-même là où le système ne donne pas l'horloge.
pub(super) async fn idle_task(out: Out) {
    let mut watch = IdleWatch::new(Instant::now());
    loop {
        let Some(tick) = crate::platform::idle::last_input_tick() else { return };
        match watch.step(tick, Instant::now(), IDLE_AFTER) {
            Some(Some(quiet)) => {
                let since = chrono::Utc::now().timestamp_millis() - i64::try_from(quiet.as_millis()).unwrap_or(0);
                out.send(Event::Activity(ActivityEvent::Idle(Some(since))));
            }
            Some(None) => out.send(Event::Activity(ActivityEvent::Idle(None))),
            None => {}
        }
        tokio::time::sleep(POLL_EVERY).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_after_the_delay_and_back_on_input() {
        let t0 = Instant::now();
        let s = Duration::from_secs;
        let mut w = IdleWatch::new(t0);
        assert_eq!(w.step(100, t0, s(600)), None);
        assert_eq!(w.step(100, t0 + s(300), s(600)), None);
        // Un geste remet le compte à zéro.
        assert_eq!(w.step(250, t0 + s(320), s(600)), None);
        assert_eq!(w.step(250, t0 + s(900), s(600)), None);
        assert_eq!(w.step(250, t0 + s(920), s(600)), Some(Some(s(600))));
        // Une seule annonce tant que rien ne bouge.
        assert_eq!(w.step(250, t0 + s(4000), s(600)), None);
        // Retour, y compris quand l'horloge a rebouclé (valeur plus petite).
        assert_eq!(w.step(3, t0 + s(4020), s(600)), Some(None));
        assert_eq!(w.step(3, t0 + s(4040), s(600)), None);
    }
}
