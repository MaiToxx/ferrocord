//! Salons de conférence (instances, orateurs, auditeurs, mains levées) et
//! statut des salons vocaux (lot V6). Le réseau est dans `net::calls`.
//!
//! Conférences : documentées par Discord (ressources Stage Instance et
//! Voice, 13 novembre 2026). Un orateur a un état vocal sans `suppress` ;
//! une main levée porte `request_to_speak_timestamp`. Modérateur :
//! MANAGE_CHANNELS, MUTE_MEMBERS et MOVE_MEMBERS. Les droits viennent des
//! rôles du serveur (les dérogations des salons vocaux ne sont pas encore
//! gardées par le magasin) ; droits inconnus : rien n'est proposé.
//!
//! Statut d'un salon vocal : non documenté (docs.discord.food, « Channels »,
//! Modify Channel Status, `PUT /channels/{id}/voice-status`, 500 caractères
//! au plus ; VOICE_CHANNEL_STATUS_UPDATE).

use super::admin::community::Venue;
use super::voice::VoiceState;
use super::event::{Cmd, Event};
use super::notice::Problem;
use super::perms::Permissions;
use super::store::{Effect, Store};
use std::collections::{HashMap, HashSet, VecDeque};

/// Sujet d'une conférence : 1 à 120 caractères.
pub const TOPIC_MAX: usize = 120;
/// Statut d'un salon vocal : 500 caractères au plus.
pub const STATUS_MAX: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageInstance {
    pub id: u64,
    pub guild_id: u64,
    pub channel_id: u64,
    pub topic: String,
    pub event_id: Option<u64>,
}

/// Place d'une personne dans une conférence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Speaker,
    Hand,
    Listener,
}

impl Role {
    pub fn of(s: &VoiceState) -> Role {
        match (s.suppress, s.request_to_speak) {
            (false, _) => Role::Speaker,
            (true, Some(_)) => Role::Hand,
            (true, None) => Role::Listener,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StageAction {
    Start,
    Topic,
    End,
    Hand,
    Speaker,
    Status,
}

/// Interface -> réseau.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StageCmd {
    /// `POST /stage-instances`.
    Start { guild_id: u64, channel_id: u64, topic: String, notify: bool },
    /// `PATCH /stage-instances/{channel}`.
    Topic { channel_id: u64, topic: String },
    /// `DELETE /stage-instances/{channel}`.
    End { channel_id: u64 },
    /// `PATCH /guilds/{id}/voice-states/@me` : lever (`Some(instant)`) ou baisser la main.
    Hand { guild_id: u64, channel_id: u64, at_ms: Option<i64> },
    /// `PATCH /guilds/{id}/voice-states/{user|@me}` : orateur (`speak`) ou auditeur.
    Speaker { guild_id: u64, channel_id: u64, user_id: Option<u64>, speak: bool },
    /// `PUT /channels/{id}/voice-status`.
    Status { channel_id: u64, status: Option<String> },
}

impl StageCmd {
    /// Action et clé : (action, salon) ; « orateur » : par personne.
    pub fn busy(&self) -> (StageAction, u64, u64) {
        match self {
            StageCmd::Start { channel_id, .. } => (StageAction::Start, *channel_id, 0),
            StageCmd::Topic { channel_id, .. } => (StageAction::Topic, *channel_id, 0),
            StageCmd::End { channel_id } => (StageAction::End, *channel_id, 0),
            StageCmd::Hand { channel_id, .. } => (StageAction::Hand, *channel_id, 0),
            StageCmd::Speaker { channel_id, user_id, .. } => (StageAction::Speaker, *channel_id, user_id.unwrap_or(0)),
            StageCmd::Status { channel_id, .. } => (StageAction::Status, *channel_id, 0),
        }
    }
}

/// Réseau -> magasin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StageEvent {
    /// STAGE_INSTANCE_CREATE / UPDATE, ou réponse de création.
    Upsert(StageInstance),
    /// STAGE_INSTANCE_DELETE, ou réponse de fin.
    Delete { channel_id: u64 },
    /// Serveur reçu (READY, GUILD_CREATE) : conférences et statuts des salons.
    Guild { guild_id: u64, instances: Vec<StageInstance>, statuses: Vec<(u64, String)> },
    /// VOICE_CHANNEL_STATUS_UPDATE (`None` : retiré).
    Status { channel_id: u64, status: Option<String> },
    Done { busy: (StageAction, u64, u64) },
    /// `problem` : `None` pour un captcha (jamais contourné).
    Failed { busy: (StageAction, u64, u64), problem: Option<Problem> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageNotice {
    pub action: StageAction,
    pub problem: Option<Problem>,
}

/// Saisie refusée avant tout envoi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invalid {
    Forbidden,
    Busy,
    Topic,
    Status,
}

#[derive(Default)]
pub struct Stage {
    instances: HashMap<u64, StageInstance>,
    statuses: HashMap<u64, String>,
    busy: HashSet<(StageAction, u64, u64)>,
    notices: VecDeque<StageNotice>,
}

impl Stage {
    pub fn clear(&mut self) {
        *self = Stage::default();
    }

    pub fn instance(&self, channel: u64) -> Option<&StageInstance> {
        self.instances.get(&channel)
    }

    pub fn status(&self, channel: u64) -> Option<&str> {
        self.statuses.get(&channel).map(String::as_str)
    }

    pub fn is_busy(&self, action: StageAction, channel: u64, user: u64) -> bool {
        self.busy.contains(&(action, channel, user))
    }

    pub fn take_notice(&mut self) -> Option<StageNotice> {
        self.notices.pop_front()
    }
}

/// Salon vocal ou de conférence connu du magasin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Room {
    pub guild_id: u64,
    pub venue: Venue,
}

/// Texte saisi ramené à sa forme envoyée : espaces retirés, longueur vérifiée.
pub fn clean(text: &str, max: usize) -> Option<String> {
    let t = text.trim();
    (t.chars().count() <= max).then(|| t.to_string())
}

impl Store {
    /// Salon vocal ou de conférence (dans les serveurs reçus).
    pub fn voice_room(&self, channel: u64) -> Option<Room> {
        let c = self.voice.channel(channel)?;
        Some(Room { guild_id: c.guild_id, venue: Venue { id: c.id, name: c.name.clone(), stage: c.stage, position: c.position } })
    }

    /// Comme [`Store::voice_room`], seulement dans ce serveur.
    pub fn voice_room_in(&self, guild: u64, channel: u64) -> Option<Room> {
        self.voice_room(channel).filter(|r| r.guild_id == guild)
    }

    fn me_id(&self) -> Option<u64> {
        self.me.as_ref().map(|u| u.id)
    }

    /// Modérateur des conférences de ce serveur (définition de Discord).
    pub fn stage_moderator(&self, guild: u64) -> bool {
        self.guild_has(guild, Permissions::MANAGE_CHANNELS | Permissions::MUTE_MEMBERS | Permissions::MOVE_MEMBERS)
    }

    /// Changer le statut du salon : SET_VOICE_CHANNEL_STATUS, et être connecté
    /// au salon ou avoir MANAGE_CHANNELS.
    pub fn can_set_voice_status(&self, guild: u64, channel: u64) -> bool {
        self.guild_has(guild, Permissions::SET_VOICE_CHANNEL_STATUS) && (self.in_voice(channel) || self.guild_has(guild, Permissions::MANAGE_CHANNELS))
    }

    /// Personnes d'une conférence, rangées : orateurs, mains levées (la plus
    /// ancienne d'abord), auditeurs.
    pub fn stage_people(&self, channel: u64) -> Vec<(u64, Role)> {
        let mut v: Vec<(u64, Role, i64)> = self.voice.members(channel).map(|s| (s.user_id, Role::of(s), s.request_to_speak.unwrap_or(0))).collect();
        v.sort_by_key(|(u, r, at)| (*r, *at, *u));
        v.into_iter().map(|(u, r, _)| (u, r)).collect()
    }

    pub fn my_stage_role(&self, channel: u64) -> Option<Role> {
        self.voice.state(self.me_id()?).filter(|s| s.channel_id == channel).map(Role::of)
    }

    fn stage_cmd(&mut self, cmd: StageCmd) -> Result<(), Invalid> {
        if !self.stage.busy.insert(cmd.busy()) {
            return Err(Invalid::Busy);
        }
        self.push_command(Cmd::Stage(cmd));
        Ok(())
    }

    pub fn start_stage(&mut self, channel_id: u64, topic: &str, notify: bool) -> Result<(), Invalid> {
        let room = self.voice_room(channel_id).filter(|r| r.venue.stage).ok_or(Invalid::Forbidden)?;
        if !self.stage_moderator(room.guild_id) {
            return Err(Invalid::Forbidden);
        }
        let topic = clean(topic, TOPIC_MAX).filter(|t| !t.is_empty()).ok_or(Invalid::Topic)?;
        // @everyone n'est prévenu qu'avec MENTION_EVERYONE (sinon Discord l'ignore).
        let notify = notify && self.guild_has(room.guild_id, Permissions::MENTION_EVERYONE);
        self.stage_cmd(StageCmd::Start { guild_id: room.guild_id, channel_id, topic, notify })
    }

    pub fn set_stage_topic(&mut self, channel_id: u64, topic: &str) -> Result<(), Invalid> {
        let i = self.stage.instance(channel_id).ok_or(Invalid::Forbidden)?;
        if !self.stage_moderator(i.guild_id) {
            return Err(Invalid::Forbidden);
        }
        let topic = clean(topic, TOPIC_MAX).filter(|t| !t.is_empty()).ok_or(Invalid::Topic)?;
        self.stage_cmd(StageCmd::Topic { channel_id, topic })
    }

    pub fn end_stage(&mut self, channel_id: u64) -> Result<(), Invalid> {
        let i = self.stage.instance(channel_id).ok_or(Invalid::Forbidden)?;
        if !self.stage_moderator(i.guild_id) {
            return Err(Invalid::Forbidden);
        }
        self.stage_cmd(StageCmd::End { channel_id })
    }

    /// Lever ou baisser la main (auditeur connecté à la conférence).
    pub fn raise_hand(&mut self, channel_id: u64, up: bool) -> Result<(), Invalid> {
        let room = self.voice_room(channel_id).ok_or(Invalid::Forbidden)?;
        if !self.in_voice(channel_id) {
            return Err(Invalid::Forbidden);
        }
        let at_ms = up.then(|| self.now());
        self.stage_cmd(StageCmd::Hand { guild_id: room.guild_id, channel_id, at_ms })
    }

    /// Inviter à parler (`speak`) ou rendre auditeur ; `user` : `None` pour
    /// moi. Je peux toujours redevenir auditeur ; le reste est aux modérateurs.
    pub fn set_speaker(&mut self, channel_id: u64, user: Option<u64>, speak: bool) -> Result<(), Invalid> {
        let room = self.voice_room(channel_id).ok_or(Invalid::Forbidden)?;
        let me = self.me_id();
        let user = user.filter(|u| Some(*u) != me);
        let allowed = if user.is_none() && !speak { self.in_voice(channel_id) } else { self.guild_has(room.guild_id, Permissions::MUTE_MEMBERS) };
        if !allowed {
            return Err(Invalid::Forbidden);
        }
        self.stage_cmd(StageCmd::Speaker { guild_id: room.guild_id, channel_id, user_id: user, speak })
    }

    pub fn set_voice_status(&mut self, channel_id: u64, status: &str) -> Result<(), Invalid> {
        let room = self.voice_room(channel_id).ok_or(Invalid::Forbidden)?;
        if !self.can_set_voice_status(room.guild_id, channel_id) {
            return Err(Invalid::Forbidden);
        }
        let status = clean(status, STATUS_MAX).ok_or(Invalid::Status)?;
        self.stage_cmd(StageCmd::Status { channel_id, status: (!status.is_empty()).then_some(status) })
    }

    pub(in crate::state) fn apply_stage(&mut self, e: StageEvent) -> Option<Effect> {
        let st = &mut self.stage;
        match e {
            StageEvent::Upsert(i) => {
                st.instances.insert(i.channel_id, i);
            }
            StageEvent::Delete { channel_id } => {
                st.instances.remove(&channel_id);
            }
            StageEvent::Guild { guild_id, instances, statuses } => {
                st.instances.retain(|_, i| i.guild_id != guild_id);
                st.instances.extend(instances.into_iter().map(|i| (i.channel_id, i)));
                // Statuts : seulement ceux de ce serveur sont remplacés.
                for v in self.voice.channels(guild_id) {
                    st.statuses.remove(&v.id);
                }
                st.statuses.extend(statuses.into_iter().filter(|(_, s)| !s.is_empty()));
            }
            StageEvent::Status { channel_id, status } => match status.filter(|s| !s.is_empty()) {
                Some(s) => {
                    st.statuses.insert(channel_id, s);
                }
                None => {
                    st.statuses.remove(&channel_id);
                }
            },
            StageEvent::Done { busy } => {
                st.busy.remove(&busy);
            }
            StageEvent::Failed { busy, problem } => {
                st.busy.remove(&busy);
                if let Some(Problem::BadToken) = problem {
                    return Some(Effect::Fatal { problem: Problem::BadToken, bad_token: true });
                }
                st.notices.push_back(StageNotice { action: busy.0, problem });
            }
        }
        None
    }
}

impl From<StageEvent> for Event {
    fn from(e: StageEvent) -> Self {
        Event::Stage(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::voice::VoiceEvent;
    use crate::state::model::{Guild, Role as GuildRole, User};

    const ME: u64 = 1;
    const ALICE: u64 = 2;
    const BOB: u64 = 3;
    const G: u64 = 500;
    const STAGE: u64 = 501;
    const VOICE: u64 = 502;

    fn store(perms: Permissions) -> Store {
        let mut s = Store::new();
        s.me = Some(User { id: ME, name: "moi".into(), avatar: crate::state::model::Avatar::of_user(ME, None, None) });
        let mut g = Guild::new(G, "Club".into(), Vec::new());
        g.owner_id = Some(99);
        g.roles = vec![GuildRole { id: G, name: "@everyone".into(), color: 0, position: 0, permissions: perms, mentionable: false }];
        g.me = Some(crate::state::model::Member { roles: Vec::new(), ..Default::default() });
        s.apply(Event::GuildCreate(g));
        for (id, stage) in [(STAGE, true), (VOICE, false)] {
            s.apply(Event::Voice(VoiceEvent::ChannelUpsert(crate::state::voice::VoiceChannel { id, guild_id: G, name: "salon".into(), stage, position: 0, parent: None, overwrites: Some(Vec::new()), user_limit: 0 })));
        }
        s
    }

    fn join(s: &mut Store, user: u64, suppress: bool, request_ms: Option<i64>) {
        let st = VoiceState { guild_id: Some(G), channel_id: STAGE, user_id: user, suppress, request_to_speak: request_ms, ..Default::default() };
        s.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(G), user_id: user, state: Some(st), session_id: None }));
    }

    fn mod_perms() -> Permissions {
        Permissions::VIEW_CHANNEL | Permissions::MANAGE_CHANNELS | Permissions::MUTE_MEMBERS | Permissions::MOVE_MEMBERS
    }

    #[test]
    fn people_are_sorted_by_role_then_hand_age() {
        let mut s = store(Permissions::VIEW_CHANNEL);
        join(&mut s, BOB, true, Some(20));
        join(&mut s, ALICE, false, None);
        join(&mut s, ME, true, None);
        join(&mut s, 7, true, Some(10));
        assert_eq!(s.stage_people(STAGE), vec![(ALICE, Role::Speaker), (7, Role::Hand), (BOB, Role::Hand), (ME, Role::Listener)]);
        assert_eq!(s.my_stage_role(STAGE), Some(Role::Listener));
    }

    #[test]
    fn listener_raises_hand_but_cannot_moderate() {
        let mut s = store(Permissions::VIEW_CHANNEL);
        assert_eq!(s.raise_hand(STAGE, true), Err(Invalid::Forbidden), "pas connecté");
        join(&mut s, ME, true, None);
        assert_eq!(s.raise_hand(STAGE, true), Ok(()));
        let now = s.now();
        let cmds = s.take_commands();
        let [Cmd::Stage(StageCmd::Hand { guild_id: G, channel_id: STAGE, at_ms: Some(at) })] = cmds.as_slice() else { panic!("{cmds:?}") };
        assert!((now - at).abs() < 1000);
        assert_eq!(s.raise_hand(STAGE, false), Err(Invalid::Busy));
        s.apply(Event::Stage(StageEvent::Done { busy: (StageAction::Hand, STAGE, 0) }));
        assert_eq!(s.raise_hand(STAGE, false), Ok(()));
        assert_eq!(s.set_speaker(STAGE, Some(ALICE), true), Err(Invalid::Forbidden));
        assert_eq!(s.start_stage(STAGE, "Sujet", false), Err(Invalid::Forbidden));
        // Je peux toujours redevenir auditeur.
        assert_eq!(s.set_speaker(STAGE, None, false), Ok(()));
    }

    #[test]
    fn moderator_starts_edits_and_ends() {
        let mut s = store(mod_perms());
        assert_eq!(s.start_stage(STAGE, "   ", false), Err(Invalid::Topic));
        assert_eq!(s.start_stage(STAGE, &"x".repeat(121), false), Err(Invalid::Topic));
        assert_eq!(s.start_stage(VOICE, "Sujet", false), Err(Invalid::Forbidden), "pas une scène");
        assert_eq!(s.start_stage(STAGE, " Sujet ", true), Ok(()));
        assert_eq!(s.take_commands(), vec![Cmd::Stage(StageCmd::Start { guild_id: G, channel_id: STAGE, topic: "Sujet".into(), notify: false })]);
        s.apply(Event::Stage(StageEvent::Upsert(StageInstance { id: 9, guild_id: G, channel_id: STAGE, topic: "Sujet".into(), event_id: None })));
        s.apply(Event::Stage(StageEvent::Done { busy: (StageAction::Start, STAGE, 0) }));
        assert_eq!(s.set_stage_topic(STAGE, "Autre"), Ok(()));
        assert_eq!(s.set_speaker(STAGE, Some(ALICE), true), Ok(()));
        assert_eq!(s.end_stage(STAGE), Ok(()));
        let cmds = s.take_commands();
        assert_eq!(cmds[1], Cmd::Stage(StageCmd::Speaker { guild_id: G, channel_id: STAGE, user_id: Some(ALICE), speak: true }));
        assert_eq!(cmds[2], Cmd::Stage(StageCmd::End { channel_id: STAGE }));
        s.apply(Event::Stage(StageEvent::Failed { busy: (StageAction::End, STAGE, 0), problem: Some(Problem::MfaRequired) }));
        assert_eq!(s.stage.take_notice(), Some(StageNotice { action: StageAction::End, problem: Some(Problem::MfaRequired) }));
        s.apply(Event::Stage(StageEvent::Delete { channel_id: STAGE }));
        assert!(s.stage.instance(STAGE).is_none());
    }

    #[test]
    fn unknown_rights_propose_nothing() {
        let mut s = store(mod_perms());
        if let Some(g) = s.guilds.iter_mut().find(|g| g.id == G) {
            g.me = None;
        }
        assert!(!s.stage_moderator(G));
        assert_eq!(s.start_stage(STAGE, "Sujet", false), Err(Invalid::Forbidden));
    }

    #[test]
    fn voice_status_needs_permission_and_presence() {
        let mut s = store(Permissions::VIEW_CHANNEL | Permissions::SET_VOICE_CHANNEL_STATUS);
        assert_eq!(s.set_voice_status(VOICE, "On joue"), Err(Invalid::Forbidden), "pas connecté");
        s.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(G), user_id: ME, state: Some(VoiceState { guild_id: Some(G), channel_id: VOICE, user_id: ME, ..Default::default() }), session_id: None }));
        assert_eq!(s.set_voice_status(VOICE, &"x".repeat(501)), Err(Invalid::Status));
        assert_eq!(s.set_voice_status(VOICE, "  "), Ok(()));
        assert_eq!(s.take_commands(), vec![Cmd::Stage(StageCmd::Status { channel_id: VOICE, status: None })]);
        s.apply(Event::Stage(StageEvent::Status { channel_id: VOICE, status: Some("On joue".into()) }));
        assert_eq!(s.stage.status(VOICE), Some("On joue"));
        s.apply(Event::Stage(StageEvent::Guild { guild_id: G, instances: Vec::new(), statuses: vec![(STAGE, "Live".into())] }));
        assert_eq!((s.stage.status(VOICE), s.stage.status(STAGE)), (None, Some("Live")));
    }
}
