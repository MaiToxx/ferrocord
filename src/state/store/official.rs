//! MP du compte système de Discord (« notifications officielles ») : un
//! salon en lecture seule. Le client officiel n'y propose ni saisie, ni
//! réaction, ni appel : rien de tel ne doit partir d'ici non plus.
//!
//! Deux protections : les gestes sont refusés à la source ([`Store::send_block`],
//! [`Store::rights`], [`Store::start_call`]), et tout ce qui sort du magasin
//! repasse par [`Store::drop_official_writes`] (une commande posée par un
//! autre chemin, un envoi mis en file avant que le correspondant ne soit
//! connu), où une écriture vers ce salon est retirée.
//!
//! Reconnaissance : champ `system` de l'utilisateur (documentation de
//! Discord, objet User : « an Official Discord System user (part of the
//! urgent message system) »), ou l'identifiant du compte « Discord » officiel.

use super::Store;
use crate::state::calls::CallCmd;
use crate::state::event::Cmd;
use crate::state::polls::PollCmd;
use crate::state::social::SocialCmd;
use crate::state::upload::UploadCmd;
use crate::state::voice::VoiceCmd;

/// Compte système « Discord » (celui des notifications officielles).
pub const DISCORD_SYSTEM_USER: u64 = 643_945_264_868_098_049;

impl Store {
    /// Correspondant du MP à deux `channel_id` s'il est un compte système de Discord.
    pub fn official_peer(&self, channel_id: u64) -> Option<u64> {
        if self.chan_guild.contains_key(&channel_id) {
            return None;
        }
        let people = self.people.lock();
        let peer = people.dm_peer(channel_id)?;
        (peer == DISCORD_SYSTEM_USER || people.user(peer).is_some_and(|u| u.system)).then_some(peer)
    }

    /// MP réservé aux notifications officielles de Discord : lecture seule.
    pub fn is_official_dm(&self, channel_id: u64) -> bool {
        self.official_peer(channel_id).is_some()
    }

    /// Retire de `cmds` toute écriture vers un MP officiel. Les lectures
    /// (historique, épingles, qui a réagi, accusé de lecture) restent.
    pub(super) fn drop_official_writes(&self, cmds: &mut Vec<Cmd>) {
        // Cas courant : aucune commande, ou aucune qui écrive dans un salon.
        if !cmds.iter().any(|c| written_channel(c).is_some()) {
            return;
        }
        cmds.retain(|c| !written_channel(c).is_some_and(|ch| self.is_official_dm(ch)));
    }
}

/// Salon dans lequel `cmd` écrit (message, saisie, réaction, modification,
/// épingle, pièce jointe, sondage, sonnerie, appel) ; `None` : lecture, ou
/// commande sans salon.
fn written_channel(cmd: &Cmd) -> Option<u64> {
    match cmd {
        Cmd::Send { channel_id, .. } | Cmd::Typing { channel_id } => Some(*channel_id),
        Cmd::Social(s) => match s {
            SocialCmd::React { channel_id, .. }
            | SocialCmd::Edit { channel_id, .. }
            | SocialCmd::Delete { channel_id, .. }
            | SocialCmd::Pin { channel_id, .. }
            | SocialCmd::SuppressEmbeds { channel_id, .. }
            | SocialCmd::Send { channel_id, .. } => Some(*channel_id),
            SocialCmd::Reactors { .. } | SocialCmd::Pins { .. } => None,
        },
        Cmd::Upload(UploadCmd::Send { channel_id, .. }) => Some(*channel_id),
        Cmd::Polls(PollCmd::Vote { channel_id, .. } | PollCmd::End { channel_id, .. } | PollCmd::Create { channel_id, .. }) => Some(*channel_id),
        Cmd::Calls(CallCmd::Ring { channel_id, .. } | CallCmd::SendSound { channel_id, .. }) => Some(*channel_id),
        // Appel privé (op 4 sans serveur) : jamais vers le compte système.
        Cmd::Voice(VoiceCmd::Join { guild_id: None, channel_id }) => Some(*channel_id),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::Dm;
    use crate::state::people::UserInfo;
    use crate::state::social::Emoji;
    use crate::state::store::SendBlock;
    use crate::state::{DmKnown, Event};

    const OFFICIAL: u64 = 60;
    const FRIEND: u64 = 61;

    fn user(id: u64, system: bool) -> UserInfo {
        UserInfo { id, username: format!("u{id}"), global_name: None, discriminator: None, avatar: crate::state::model::Avatar::default_for(id), banner: None, accent: None, flags: None, bot: system, system, look: Default::default() }
    }

    /// Magasin avec deux MP : le compte système (salon `OFFICIAL`) et un ami (`FRIEND`).
    fn store() -> Store {
        let mut s = Store::new();
        s.apply(Event::Ready { me: crate::state::model::User::new(1, "moi".into()), guilds: Vec::new(), dms: Vec::new() });
        for (dm, peer, system) in [(OFFICIAL, 600, true), (FRIEND, 601, false)] {
            s.apply(Event::DmUpsert { dm: Dm::new(dm, format!("mp{dm}"), dm), known: DmKnown::ALL });
            let mut people = s.people.lock();
            people.upsert_user(user(peer, system));
            people.note_dm(peer, dm);
        }
        s.take_commands();
        s
    }

    #[test]
    fn official_dm_is_recognised_by_flag_or_by_id() {
        let s = store();
        assert!(s.is_official_dm(OFFICIAL) && !s.is_official_dm(FRIEND) && !s.is_official_dm(4242));
        // Identifiant officiel, même sans le champ `system` (utilisateur partiel).
        let mut s = Store::new();
        s.people.lock().note_dm(DISCORD_SYSTEM_USER, 70);
        assert_eq!(s.official_peer(70), Some(DISCORD_SYSTEM_USER));
        s.take_commands();
    }

    #[test]
    fn nothing_can_be_written_to_the_official_dm() {
        let mut s = store();
        assert_eq!(s.send_block(OFFICIAL), Some(SendBlock::Official));
        assert_eq!(s.send_block(FRIEND), None);
        let r = s.rights(OFFICIAL);
        assert!(!r.send && !r.react && !r.react_existing && !r.pin && !r.thread, "{r:?}");
        assert!(s.rights(FRIEND).send && s.rights(FRIEND).react);
        assert!(!s.send_message(OFFICIAL, "bonjour".into()));
        assert!(!s.toggle_reaction(OFFICIAL, 1, Emoji::Unicode("👍".into())));
        assert!(s.send_files(OFFICIAL, String::new(), Vec::new(), None).is_err());
        s.start_call(OFFICIAL);
        assert!(s.take_commands().is_empty(), "aucune commande vers le MP officiel");
        assert!(s.sending.is_empty() && s.voice.me.is_none());
    }

    /// Une écriture posée par un autre chemin est retirée à la sortie du
    /// magasin ; les lectures et les autres salons passent.
    #[test]
    fn writes_are_dropped_on_the_way_out() {
        let mut s = store();
        let emoji = || Emoji::Unicode("👍".into());
        for ch in [OFFICIAL, FRIEND] {
            s.push_command(Cmd::Send { channel_id: ch, content: "x".into(), nonce: 1 });
            s.push_command(Cmd::Typing { channel_id: ch });
            s.push_command(Cmd::Social(SocialCmd::React { channel_id: ch, message_id: 2, emoji: emoji(), add: true }));
            s.push_command(Cmd::Social(SocialCmd::Pin { channel_id: ch, message_id: 2, pin: true }));
            s.push_command(Cmd::Upload(UploadCmd::Send { channel_id: ch, nonce: 3, content: String::new(), files: Vec::new(), kind: None }));
            s.push_command(Cmd::Calls(CallCmd::Ring { channel_id: ch, recipients: None }));
            s.push_command(Cmd::Voice(VoiceCmd::Join { guild_id: None, channel_id: ch }));
            s.push_command(Cmd::Polls(PollCmd::Vote { channel_id: ch, message_id: 2, answers: vec![1], previous: Vec::new() }));
            // Lectures : gardées partout.
            s.push_command(Cmd::History { channel_id: ch, before: None });
            s.push_command(Cmd::Social(SocialCmd::Pins { channel_id: ch, before: None }));
        }
        let out = s.take_commands();
        let to = |ch: u64| out.iter().filter(|c| format!("{c:?}").contains(&format!("channel_id: {ch}"))).count();
        assert_eq!(to(OFFICIAL), 2, "seules les deux lectures restent : {out:?}");
        assert_eq!(to(FRIEND), 10);
        assert!(out.iter().all(|c| written_channel(c) != Some(OFFICIAL)));
    }
}
