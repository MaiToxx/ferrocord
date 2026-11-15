//! Bureau hors Windows : `notify.toml` lu et écrit dans un fil à part, et
//! les notifications confiées à `notify` quand le système en a (Android).
//! Ni sons ni icône de zone de notification : sans objet ici.

use crate::desk::{self, DeskEvent, Desktop, TrayState};
use crate::net::Wake;
use crate::state::notify::{Alert, DeskPrefs, Kind};
use std::sync::mpsc;

/// Montre une notification du système (dans le fil du bureau).
pub type Notify = Box<dyn Fn(&Alert) + Send>;

enum Job {
    Load,
    Save(DeskPrefs),
    Toast(Alert),
}

pub struct PrefsDesk {
    jobs: mpsc::Sender<Job>,
    events: mpsc::Receiver<DeskEvent>,
}

impl PrefsDesk {
    pub fn start(wake: Wake, notify: Option<Notify>) -> PrefsDesk {
        let (jobs, rx) = mpsc::channel::<Job>();
        let (tx, events) = mpsc::channel();
        // Sans fil, les demandes sont perdues sans bruit : l'application reste utilisable.
        let _ = std::thread::Builder::new().name("ferrocord-bureau".into()).spawn(move || {
            for job in rx {
                let event = match job {
                    Job::Load => Some(DeskEvent::Prefs(desk::default_path().map(|p| desk::read_prefs(&p)).unwrap_or_default())),
                    Job::Save(prefs) => match desk::default_path().map(|p| desk::write_prefs(&p, &prefs)) {
                        Some(Err(e)) => Some(DeskEvent::SaveFailed(e.to_string())),
                        _ => None,
                    },
                    Job::Toast(alert) => {
                        if let Some(n) = &notify {
                            n(&alert);
                        }
                        None
                    }
                };
                if let Some(e) = event
                    && tx.send(e).is_ok()
                {
                    wake();
                }
            }
        });
        PrefsDesk { jobs, events }
    }
}

impl Desktop for PrefsDesk {
    fn load_prefs(&mut self) {
        let _ = self.jobs.send(Job::Load);
    }

    fn save_prefs(&mut self, prefs: DeskPrefs) {
        let _ = self.jobs.send(Job::Save(prefs));
    }

    fn toast(&mut self, alert: &Alert) {
        let _ = self.jobs.send(Job::Toast(alert.clone()));
    }

    fn sound(&mut self, _: Kind, _: u8) {}

    fn tray(&mut self, _: &TrayState) {}

    fn poll(&mut self) -> Vec<DeskEvent> {
        self.events.try_iter().collect()
    }
}
