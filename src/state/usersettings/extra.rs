//! Réglages du compte hors protobuf ajoutés avec les pages « Données et
//! confidentialité » et « Profil » : consentements (statistiques
//! d'utilisation, personnalisation) et envoi d'avatar ou de bannière.
//!
//! Sources :
//! - consentements : docs.discord.food, « User Settings », Get User Consents
//!   (`GET /users/@me/consent`) et Modify User Consents
//!   (`POST /users/@me/consent`, `{ "grant": [...], "revoke": [...] }`) ;
//! - avatar et bannière : Modify Current User (`PATCH /users/@me`, champs
//!   `avatar` et `banner`, « image data », documentés par Discord) et Modify
//!   Current Guild Member (`PATCH /guilds/{id}/members/@me`, mêmes champs,
//!   docs.discord.food ; Nitro exigé par Discord).

use super::{Refusal, Remote};
use crate::state::crop::ImageKind;
use crate::state::notice::{Notice, Problem};

/// Type de consentement, avec son nom dans l'API.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConsentKind {
    UsageStatistics,
    Personalization,
}

impl ConsentKind {
    pub fn code(self) -> &'static str {
        match self {
            ConsentKind::UsageStatistics => "usage_statistics",
            ConsentKind::Personalization => "personalization",
        }
    }
}

/// Consentements connus (`None` : absent de la réponse).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Consents {
    pub usage_statistics: Option<bool>,
    pub personalization: Option<bool>,
}

impl Consents {
    pub fn get(&self, k: ConsentKind) -> Option<bool> {
        match k {
            ConsentKind::UsageStatistics => self.usage_statistics,
            ConsentKind::Personalization => self.personalization,
        }
    }

    fn set(&mut self, k: ConsentKind, v: Option<bool>) {
        match k {
            ConsentKind::UsageStatistics => self.usage_statistics = v,
            ConsentKind::Personalization => self.personalization = v,
        }
    }

    /// Réponse de la modification : les types qu'elle donne remplacent les nôtres.
    fn merge(&mut self, other: Consents) {
        for k in [ConsentKind::UsageStatistics, ConsentKind::Personalization] {
            if let Some(v) = other.get(k) {
                self.set(k, Some(v));
            }
        }
    }
}

/// Où va l'image : le compte, ou le profil d'un serveur.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageTarget {
    pub kind: ImageKind,
    pub guild: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtraCmd {
    LoadConsents,
    SetConsent { kind: ConsentKind, on: bool },
    /// `data` : `data:image/png;base64,…`, ou `None` pour retirer l'image.
    SetImage { target: ImageTarget, data: Option<String> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExtraEvent {
    Consents(Result<Consents, Problem>),
    ConsentWritten { kind: ConsentKind, result: Result<Consents, Problem> },
    ImageWritten { target: ImageTarget, removed: bool, result: Result<(), Problem> },
}

/// Ce qu'est devenu le dernier envoi d'image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageOutcome {
    Sent,
    Removed,
    Failed,
}

#[derive(Default)]
pub struct Extras {
    pub consents: Remote<Consents>,
    /// Consentement en cours d'écriture, et sa valeur d'avant.
    consent_saving: Option<(ConsentKind, Option<bool>)>,
    pub image_saving: Option<ImageTarget>,
    pub image_outcome: Option<(ImageTarget, ImageOutcome)>,
    outbox: Vec<ExtraCmd>,
}

impl Extras {
    pub fn take_commands(&mut self) -> Vec<ExtraCmd> {
        std::mem::take(&mut self.outbox)
    }

    pub fn load_consents(&mut self) {
        if !matches!(self.consents, Remote::Loading) && self.consent_saving.is_none() {
            self.consents = Remote::Loading;
            self.outbox.push(ExtraCmd::LoadConsents);
        }
    }

    pub fn consent_busy(&self) -> bool {
        self.consent_saving.is_some()
    }

    /// Accorde ou retire un consentement (optimiste).
    pub fn set_consent(&mut self, kind: ConsentKind, on: bool) -> Result<(), Refusal> {
        if self.consent_saving.is_some() {
            return Err(Refusal::Busy);
        }
        let Remote::Ready(c) = &mut self.consents else { return Err(Refusal::Unknown) };
        self.consent_saving = Some((kind, c.get(kind)));
        c.set(kind, Some(on));
        self.outbox.push(ExtraCmd::SetConsent { kind, on });
        Ok(())
    }

    /// Envoie (ou retire, `None`) un avatar ou une bannière.
    pub fn set_image(&mut self, target: ImageTarget, data: Option<String>) -> Result<(), Refusal> {
        if self.image_saving.is_some() {
            return Err(Refusal::Busy);
        }
        self.image_saving = Some(target);
        self.image_outcome = None;
        self.outbox.push(ExtraCmd::SetImage { target, data });
        Ok(())
    }

    pub fn apply(&mut self, ev: ExtraEvent) -> Option<Notice> {
        match ev {
            ExtraEvent::Consents(r) => {
                if self.consent_saving.is_none() {
                    self.consents = r.map_or_else(Remote::Failed, Remote::Ready);
                }
            }
            ExtraEvent::ConsentWritten { kind, result } => {
                let (k, before) = self.consent_saving.take_if(|(k, _)| *k == kind)?;
                match result {
                    Ok(confirmed) => {
                        if let Remote::Ready(c) = &mut self.consents {
                            c.merge(confirmed);
                        }
                    }
                    Err(p) => {
                        if let Remote::Ready(c) = &mut self.consents {
                            c.set(k, before);
                        }
                        return Some(Notice::SettingNotSaved(p));
                    }
                }
            }
            ExtraEvent::ImageWritten { target, removed, result } => {
                if self.image_saving == Some(target) {
                    self.image_saving = None;
                }
                let outcome = match &result {
                    Ok(()) if removed => ImageOutcome::Removed,
                    Ok(()) => ImageOutcome::Sent,
                    Err(_) => ImageOutcome::Failed,
                };
                self.image_outcome = Some((target, outcome));
                if let Err(p) = result {
                    return Some(Notice::SettingNotSaved(p));
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refused_consent_is_rolled_back() {
        let mut x = Extras::default();
        assert_eq!(x.set_consent(ConsentKind::Personalization, true), Err(Refusal::Unknown));
        x.load_consents();
        assert_eq!(x.take_commands(), vec![ExtraCmd::LoadConsents]);
        x.apply(ExtraEvent::Consents(Ok(Consents { usage_statistics: Some(true), personalization: Some(false) })));
        x.set_consent(ConsentKind::Personalization, true).unwrap();
        assert_eq!(x.set_consent(ConsentKind::UsageStatistics, false), Err(Refusal::Busy));
        assert_eq!(x.consents.ready().and_then(|c| c.personalization), Some(true), "optimiste");
        let n = x.apply(ExtraEvent::ConsentWritten { kind: ConsentKind::Personalization, result: Err(Problem::Api("non".into())) });
        assert_eq!(n, Some(Notice::SettingNotSaved(Problem::Api("non".into()))));
        assert_eq!(x.consents.ready().and_then(|c| c.personalization), Some(false));
        x.set_consent(ConsentKind::UsageStatistics, false).unwrap();
        x.apply(ExtraEvent::ConsentWritten { kind: ConsentKind::UsageStatistics, result: Ok(Consents { personalization: Some(false), usage_statistics: None }) });
        assert_eq!(x.consents.ready().copied(), Some(Consents { usage_statistics: Some(false), personalization: Some(false) }));
    }

    #[test]
    fn one_image_at_a_time() {
        let mut x = Extras::default();
        let t = ImageTarget { kind: ImageKind::Avatar, guild: None };
        x.set_image(t, Some("data:".into())).unwrap();
        assert_eq!(x.set_image(t, None), Err(Refusal::Busy));
        x.apply(ExtraEvent::ImageWritten { target: t, removed: false, result: Ok(()) });
        assert_eq!((x.image_saving, x.image_outcome), (None, Some((t, ImageOutcome::Sent))));
    }
}
