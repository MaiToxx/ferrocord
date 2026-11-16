//! Messages enregistrés et rappels (lot ND) : les onglets « Messages marqués » et « Rappels » de
//! la boîte de réception sont dans `inbox.rs` ; ici, l'annonce du résultat d'un « Enregistrer le
//! message » ou « Me le rappeler » fait depuis le menu d'un message.

use super::App;
use crate::i18n::table::nd::t as nt;
use crate::state::quests::Refused;
use crate::state::saved::SavedNotice;

impl App {
    /// Résultat d'une écriture, annoncé une fois (appelé à chaque image de la discussion).
    pub(super) fn saved_notices(&mut self) {
        match self.store.saved.take_notice() {
            Some(SavedNotice::Saved) => self.notify(nt::SAVED_DONE),
            Some(SavedNotice::Reminder) => self.notify(nt::REMINDER_DONE),
            Some(SavedNotice::Refused(Refused::Captcha)) => self.notify(nt::CAPTCHA),
            Some(SavedNotice::Refused(Refused::Other(_))) => self.notify(nt::SAVED_REFUSED),
            None => {}
        }
    }
}
