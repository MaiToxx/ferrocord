//! Envoi de fichiers côté magasin : vérifications avant le départ (salon,
//! permission, nombre, taille), commandes des envois et leur suivi. Module
//! enfant de `store` (nonce, mode lent et retour au présent sont privés).

use super::Store;
use crate::state::event::Cmd;
use crate::state::perms::{Permissions, Verdict};
use crate::state::social::SendKind;
use crate::state::upload::{self, OutFile, Refusal};
use crate::state::usersettings::Remote;

impl Store {
    /// Limite par fichier du compte, si elle est connue (profil lu).
    pub fn upload_limit(&self) -> Option<u64> {
        match &self.account.profile {
            Remote::Ready(p) => Some(upload::limit_for(p.premium)),
            // Profil illisible : la limite par défaut, sans redemander.
            Remote::Failed(_) => Some(upload::DEFAULT_LIMIT),
            Remote::Idle | Remote::Loading => None,
        }
    }

    /// Limite à appliquer pour juger un fichier : celle du compte si elle est
    /// connue, sinon la limite par défaut.
    pub fn upload_limit_or_default(&self) -> u64 {
        self.upload_limit().unwrap_or(upload::DEFAULT_LIMIT)
    }

    /// Limite qu'offre à tous ses membres le serveur de ce salon, selon son
    /// niveau de boost (limite par défaut hors serveur).
    pub fn guild_upload_limit(&self, channel_id: u64) -> u64 {
        self.interaction_guild(channel_id).map_or(upload::DEFAULT_LIMIT, |g| crate::state::admin::guild::perks(self.premium_tier(g)).upload)
    }

    /// Limite qui vaut dans ce salon : la plus haute du compte et du serveur.
    pub fn upload_limit_in(&self, channel_id: u64) -> u64 {
        self.upload_limit_or_default().max(self.guild_upload_limit(channel_id))
    }

    /// Un fichier plus gros que la limite par défaut vient d'être ajouté :
    /// la limite réelle du compte est demandée (`GET /users/@me`, une fois).
    pub fn learn_upload_limit(&mut self) {
        if matches!(self.account.profile, Remote::Idle) {
            self.account.load_profile();
        }
    }

    /// Met un message avec pièces jointes dans la file d'envoi du salon.
    /// `reply` : (message, mentionner l'auteur).
    pub fn send_files(&mut self, channel_id: u64, content: String, files: Vec<OutFile>, reply: Option<(u64, bool)>) -> Result<(), Refusal> {
        if self.send_block(channel_id).is_some() || !self.channel_exists(channel_id) || self.can(channel_id, Permissions::ATTACH_FILES) == Verdict::Refuse {
            return Err(Refusal::Blocked);
        }
        let guild = self.guild_upload_limit(channel_id);
        let limit = match self.upload_limit() {
            Some(l) => l.max(guild),
            None if files.iter().any(|f| f.size > guild && f.size <= upload::MAX_LIMIT) => {
                self.learn_upload_limit();
                upload::check(&files, upload::MAX_LIMIT)?;
                return Err(Refusal::CheckingLimit);
            }
            None => guild,
        };
        upload::check(&files, limit)?;
        let nonce = self.next_nonce();
        let kind = reply.map(|(message_id, mention)| SendKind::Reply { message_id, mention });
        let cmd = self.uploads.start(channel_id, nonce, content, files, kind)?;
        self.outbox.push(Cmd::Upload(cmd));
        self.note_sent(channel_id);
        self.return_to_present(channel_id);
        Ok(())
    }

    /// Le bouton du message vocal est proposé : envoi permis, avec la
    /// permission SEND_VOICE_MESSAGES (refus certain seulement).
    pub fn can_send_voice(&self, channel_id: u64) -> bool {
        self.send_block(channel_id).is_none() && self.channel_exists(channel_id) && self.can(channel_id, Permissions::SEND_VOICE_MESSAGES) != Verdict::Refuse
    }

    /// Envoie un message vocal enregistré (docs.discord.food, « Voice
    /// Messages ») : une seule pièce jointe Ogg/Opus, durée et onde, par la
    /// même file que les autres envois (nouvel essai, annulation).
    pub fn send_voice(&mut self, channel_id: u64, rec: &crate::state::voice_msg::Recorded) -> Result<(), Refusal> {
        if !self.can_send_voice(channel_id) {
            return Err(Refusal::Blocked);
        }
        let file = OutFile { source: upload::Source::Memory(rec.ogg.clone()), name: crate::state::voice_msg::FILE_NAME.into(), size: rec.ogg.len() as u64, description: String::new(), spoiler: false };
        upload::check(std::slice::from_ref(&file), self.upload_limit_in(channel_id))?;
        let nonce = self.next_nonce();
        let kind = SendKind::Voice { duration_ms: rec.duration_ms, waveform: crate::state::usersettings::b64::encode(&rec.waveform) };
        let cmd = self.uploads.start(channel_id, nonce, String::new(), vec![file], Some(kind))?;
        self.outbox.push(Cmd::Upload(cmd));
        self.note_sent(channel_id);
        self.return_to_present(channel_id);
        Ok(())
    }

    /// Nouvel essai d'un envoi arrêté (même nonce : pas de doublon).
    pub fn retry_upload(&mut self, nonce: u64) -> bool {
        let Some(channel_id) = self.uploads.get(nonce).map(|u| u.channel_id) else { return false };
        if self.send_block(channel_id).is_some() {
            return false;
        }
        match self.uploads.retry(nonce) {
            Some(cmd) => {
                self.outbox.push(Cmd::Upload(cmd));
                true
            }
            None => false,
        }
    }

    pub fn cancel_upload(&mut self, nonce: u64) {
        if let Some(cmd) = self.uploads.cancel(nonce) {
            self.outbox.push(Cmd::Upload(cmd));
        }
    }

    pub fn discard_upload(&mut self, nonce: u64) {
        self.uploads.discard(nonce);
    }

    /// L'échec est montré sur la ligne de l'envoi, au-dessus de la saisie :
    /// pas de notification en plus.
    pub(super) fn apply_upload(&mut self, ev: upload::UploadEvent) {
        let channel = match &ev {
            upload::UploadEvent::Failed { channel_id, failure, .. } if !matches!(failure, upload::Failure::Unknown(_)) => Some(*channel_id),
            upload::UploadEvent::Cancelled { channel_id, .. } => Some(*channel_id),
            _ => None,
        };
        self.uploads.apply(ev);
        // Rien n'est parti : le mode lent n'a pas commencé.
        if let Some(c) = channel {
            self.forget_sent(c);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::state::model::{Channel, ChannelKind, Guild, User};
    use crate::state::upload::{Failure, OutFile, Phase, Refusal, Source, UploadCmd, UploadEvent, DEFAULT_LIMIT};
    use crate::state::usersettings::{AccountEvent, Profile};
    use crate::state::{Cmd, Event, Store};
    use std::sync::Arc;

    const CHAN: u64 = 100;

    fn store() -> Store {
        let mut s = Store::new();
        let chan = Channel { id: CHAN, name: "g".into(), kind: ChannelKind::Text, position: 0, parent: None, overwrites: Some(Vec::new()), slowmode: 0 };
        s.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: vec![Guild::new(9, "G".into(), vec![chan])], dms: Vec::new() });
        s.select_guild(9);
        s.take_commands();
        s
    }

    fn file(size: u64) -> OutFile {
        OutFile { source: Source::Memory(Arc::from(vec![1u8, 2, 3])), name: "a.png".into(), size, description: String::new(), spoiler: false }
    }

    fn uploads(s: &mut Store) -> Vec<UploadCmd> {
        s.take_commands()
            .into_iter()
            .filter_map(|c| match c {
                Cmd::Upload(u) => Some(u),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn send_with_reply_then_confirmation() {
        let mut s = store();
        s.send_files(CHAN, "regarde".into(), vec![file(3)], Some((42, false))).unwrap();
        let cmds = uploads(&mut s);
        let [UploadCmd::Send { channel_id, nonce, content, files, kind }] = cmds.as_slice() else { panic!("{cmds:?}") };
        assert_eq!((*channel_id, content.as_str(), files.len()), (CHAN, "regarde", 1));
        assert_eq!(kind, &Some(crate::state::social::SendKind::Reply { message_id: 42, mention: false }));
        let nonce = *nonce;
        s.apply(Event::Upload(UploadEvent::Progress { nonce, sent: 1, total: 3 }));
        assert_eq!(s.uploads.get(nonce).map(|u| u.sent), Some(1));
        // MESSAGE_CREATE de ce nonce avant la réponse HTTP : l'envoi est terminé.
        s.apply(Event::SendConfirmed { channel_id: CHAN, nonce });
        assert!(s.uploads.get(nonce).is_none());
    }

    #[test]
    fn voice_message_goes_through_the_upload_queue() {
        use crate::state::voice_msg::Recorded;
        let mut s = store();
        let rec = Recorded { ogg: Arc::from(vec![b'O'; 10]), duration_ms: 1500, waveform: vec![0, 255] };
        assert!(s.can_send_voice(CHAN));
        s.send_voice(CHAN, &rec).unwrap();
        let cmds = uploads(&mut s);
        let [UploadCmd::Send { content, files, kind, .. }] = cmds.as_slice() else { panic!("{cmds:?}") };
        assert!(content.is_empty());
        assert_eq!((files.len(), files[0].name.as_str(), files[0].size), (1, "voice-message.ogg", 10));
        assert_eq!(kind, &Some(crate::state::social::SendKind::Voice { duration_ms: 1500, waveform: "AP8=".into() }));
        assert_eq!(s.send_voice(999, &rec), Err(Refusal::Blocked));
    }

    #[test]
    fn failure_is_kept_and_retried_with_the_same_nonce() {
        let mut s = store();
        s.send_files(CHAN, String::new(), vec![file(3)], None).unwrap();
        let nonce = match uploads(&mut s).as_slice() {
            [UploadCmd::Send { nonce, .. }] => *nonce,
            other => panic!("{other:?}"),
        };
        assert!(s.slowmode_left(CHAN, s.now()).is_none());
        s.apply(Event::Upload(UploadEvent::Failed { channel_id: CHAN, nonce, failure: Failure::TooLarge }));
        assert_eq!(s.uploads.get(nonce).map(|u| u.phase.clone()), Some(Phase::Failed(Failure::TooLarge)));
        assert!(s.retry_upload(nonce));
        assert!(matches!(uploads(&mut s).as_slice(), [UploadCmd::Send { nonce: n, .. }] if *n == nonce));
        s.cancel_upload(nonce);
        assert_eq!(uploads(&mut s), vec![UploadCmd::Cancel { nonce }]);
        s.apply(Event::Upload(UploadEvent::Cancelled { channel_id: CHAN, nonce }));
        s.discard_upload(nonce);
        assert!(s.uploads.get(nonce).is_none());
    }

    #[test]
    fn size_limit_waits_for_the_profile_then_applies_it() {
        let mut s = store();
        // Petit fichier : la limite par défaut suffit, rien n'est demandé.
        s.send_files(CHAN, String::new(), vec![file(DEFAULT_LIMIT)], None).unwrap();
        assert!(s.take_commands().iter().all(|c| !matches!(c, Cmd::Account(_))));
        // Gros fichier, limite inconnue : le profil est demandé, l'envoi attend.
        assert_eq!(s.send_files(CHAN, String::new(), vec![file(DEFAULT_LIMIT + 1)], None), Err(Refusal::CheckingLimit));
        assert!(s.take_commands().iter().any(|c| matches!(c, Cmd::Account(_))));
        // Sans Nitro : refusé, avec la limite.
        s.apply(Event::Account(AccountEvent::Profile(Ok(Profile { premium: 0, ..Default::default() }))));
        assert_eq!(s.send_files(CHAN, String::new(), vec![file(DEFAULT_LIMIT + 1)], None), Err(Refusal::TooLarge { name: "a.png".into(), limit: DEFAULT_LIMIT }));
        // Nitro : accepté.
        s.apply(Event::Account(AccountEvent::Profile(Ok(Profile { premium: 2, ..Default::default() }))));
        assert!(s.send_files(CHAN, String::new(), vec![file(DEFAULT_LIMIT + 1)], None).is_ok());
        // Onze fichiers : refusé.
        assert_eq!(s.send_files(CHAN, String::new(), (0..11).map(|_| file(1)).collect(), None), Err(Refusal::TooMany));
        // Salon inconnu : refusé.
        assert_eq!(s.send_files(999, String::new(), vec![file(1)], None), Err(Refusal::Blocked));
    }

    #[test]
    fn boosted_guild_raises_the_limit_without_asking_the_profile() {
        use crate::state::admin::guild::{AdminEvent, GuildPatch};
        let mut s = store();
        let big = 40 << 20;
        assert_eq!(s.upload_limit_in(CHAN), DEFAULT_LIMIT);
        // Niveau 2 (GUILD_UPDATE) : 50 Mio pour tous, sans lire le profil.
        s.apply(Event::GuildAdmin(AdminEvent::Updated { guild_id: 9, patch: GuildPatch { premium_tier: Some(2), ..Default::default() } }));
        assert_eq!(s.upload_limit_in(CHAN), 50 << 20);
        assert!(s.send_files(CHAN, String::new(), vec![file(big)], None).is_ok());
        assert!(s.take_commands().iter().all(|c| !matches!(c, Cmd::Account(_))));
        // Au-delà : le profil (Nitro) est demandé.
        assert_eq!(s.send_files(CHAN, String::new(), vec![file(60 << 20)], None), Err(Refusal::CheckingLimit));
        // Sans Nitro : la limite du serveur reste.
        s.apply(Event::Account(AccountEvent::Profile(Ok(Profile { premium: 0, ..Default::default() }))));
        assert_eq!(s.send_files(CHAN, String::new(), vec![file(60 << 20)], None), Err(Refusal::TooLarge { name: "a.png".into(), limit: 50 << 20 }));
        assert!(s.send_files(CHAN, String::new(), vec![file(big)], None).is_ok());
    }
}
