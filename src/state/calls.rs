//! Appels privés et de groupe, tableau de sons (lot V6). Qui est dans quel
//! salon vocal : `state::voice` (lot V4). Le réseau est dans `net::calls`.
//!
//! Appels : non documentés par Discord ; forme vérifiée sur docs.discord.food
//! (« Gateway Events » : CALL_CREATE / UPDATE / DELETE ; « Channels » : Ring
//! et Stop Ringing Channel Recipients) le 13 novembre 2026. Sonner exige un
//! appel en cours : « Appeler » rejoint d'abord le salon (connexion vocale,
//! hors de ce lot), puis fait sonner dès que l'appel existe (CALL_CREATE).
//!
//! Tableau de sons : documenté (ressource Soundboard). Envoyer un son exige
//! d'être connecté au salon, sans sourdine ni micro coupé.
//!
//! Rien ne part sans geste de l'utilisateur.

use super::admin::expressions::Sound;
use super::event::{Cmd, Event};
use super::model::DISCORD_EPOCH;
use super::notice::Problem;
use super::store::{Effect, Store};
use super::voice::VoiceState;
use std::collections::{HashMap, HashSet, VecDeque};

/// Sons joués par les autres gardés pour l'affichage, au plus (les plus anciens partent).
pub const MAX_EFFECTS: usize = 16;
/// Sons à jouer en attente du moteur audio (V7), au plus.
pub const MAX_PLAYS: usize = 16;
/// Durée d'affichage d'un son joué par quelqu'un (ms).
pub const EFFECT_SHOWN_MS: i64 = 5_000;

/// Appel en cours dans un MP ou un groupe.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Call {
    pub channel_id: u64,
    /// Message système de l'appel : son identifiant date le début de l'appel.
    pub message_id: Option<u64>,
    pub region: Option<String>,
    /// Personnes qu'on fait sonner en ce moment.
    pub ringing: Vec<u64>,
    /// Appel indisponible (panne chez Discord).
    pub unavailable: bool,
}

impl Call {
    /// Début de l'appel (ms Unix), tiré de l'identifiant de son message.
    pub fn started_ms(&self) -> Option<i64> {
        self.message_id.map(snowflake_ms)
    }
}

pub fn snowflake_ms(id: u64) -> i64 {
    i64::try_from((id >> 22) + DISCORD_EPOCH).unwrap_or(i64::MAX)
}

/// Emoji d'un effet vocal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EffectEmoji {
    Unicode(String),
    Custom { id: u64, name: String },
}

/// VOICE_CHANNEL_EFFECT_SEND : quelqu'un a joué un son ou envoyé une réaction.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectSend {
    pub guild_id: Option<u64>,
    pub channel_id: u64,
    pub user_id: u64,
    pub sound_id: Option<u64>,
    /// 0 à 1.
    pub sound_volume: Option<f32>,
    pub emoji: Option<EffectEmoji>,
}

/// Son joué par quelqu'un, montré quelques secondes.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectSeen {
    pub effect: EffectSend,
    /// Arrivée (ms Unix, heure de Discord).
    pub at_ms: i64,
}

/// Son à jouer sur ce poste : la lecture est faite par le moteur audio (V7),
/// qui vide la file par [`Store::take_sound_plays`].
#[derive(Clone, Debug, PartialEq)]
pub struct SoundPlay {
    pub channel_id: u64,
    pub sound_id: u64,
    /// 0 à 1.
    pub volume: f32,
}

/// Chargement d'une liste.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Loadable<T> {
    #[default]
    Absent,
    Loading,
    Ready(T),
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CallAction {
    Ring,
    StopRinging,
    DefaultSounds,
    GuildSounds,
    SendSound,
}

/// Interface -> réseau.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallCmd {
    /// `POST /channels/{id}/call/ring` ; `None` : tout le monde.
    Ring { channel_id: u64, recipients: Option<Vec<u64>> },
    /// `POST /channels/{id}/call/stop-ringing`.
    StopRinging { channel_id: u64, recipients: Vec<u64> },
    /// `GET /soundboard-default-sounds`.
    DefaultSounds,
    /// `GET /guilds/{id}/soundboard-sounds` (la réponse va à `state::admin::expressions`).
    GuildSounds { guild_id: u64 },
    /// `POST /channels/{id}/send-soundboard-sound`.
    SendSound { channel_id: u64, sound_id: u64, source_guild_id: Option<u64> },
}

impl CallCmd {
    pub fn action(&self) -> (CallAction, u64) {
        match self {
            CallCmd::Ring { channel_id, .. } => (CallAction::Ring, *channel_id),
            CallCmd::StopRinging { channel_id, .. } => (CallAction::StopRinging, *channel_id),
            CallCmd::DefaultSounds => (CallAction::DefaultSounds, 0),
            CallCmd::GuildSounds { guild_id } => (CallAction::GuildSounds, *guild_id),
            CallCmd::SendSound { channel_id, .. } => (CallAction::SendSound, *channel_id),
        }
    }
}

/// Réseau -> magasin.
#[derive(Clone, Debug, PartialEq)]
pub enum CallEvent {
    /// CALL_CREATE (`voice_states` présent, rangés dans `state::voice`) ou CALL_UPDATE.
    Upsert { call: Call, voice_states: Option<Vec<VoiceState>> },
    /// CALL_DELETE.
    Delete { channel_id: u64, unavailable: bool },
    /// VOICE_CHANNEL_EFFECT_SEND.
    Effect(EffectSend),
    DefaultSounds(Vec<Sound>),
    Done { action: CallAction, key: u64 },
    /// `problem` : `None` pour un captcha (jamais contourné).
    Failed { action: CallAction, key: u64, problem: Option<Problem> },
}

/// Ce que l'interface annonce après un échec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallNotice {
    pub action: CallAction,
    pub problem: Option<Problem>,
}

#[derive(Default)]
pub struct Calls {
    calls: HashMap<u64, Call>,
    /// Appels entrants refusés ou acceptés ici : plus de sonnerie locale.
    answered: HashSet<u64>,
    /// « Appeler » : faire sonner dès que l'appel existe.
    pending_ring: HashSet<u64>,
    default_sounds: Loadable<Vec<Sound>>,
    guild_sounds_asked: HashSet<u64>,
    effects: VecDeque<EffectSeen>,
    plays: VecDeque<SoundPlay>,
    busy: HashSet<(CallAction, u64)>,
    notices: VecDeque<CallNotice>,
}

impl Calls {
    pub fn clear(&mut self) {
        *self = Calls::default();
    }

    /// Session neuve (READY) : les appels et états connus peuvent être périmés.
    pub fn session_reset(&mut self) {
        self.calls.clear();
        self.answered.clear();
        self.pending_ring.clear();
        self.busy.clear();
        self.effects.clear();
    }

    pub fn call(&self, channel: u64) -> Option<&Call> {
        self.calls.get(&channel)
    }

    pub fn default_sounds(&self) -> &Loadable<Vec<Sound>> {
        &self.default_sounds
    }

    pub fn is_busy(&self, action: CallAction, key: u64) -> bool {
        self.busy.contains(&(action, key))
    }

    /// « Appeler » demandé, sonnerie pas encore partie.
    pub fn calling(&self, channel: u64) -> bool {
        self.pending_ring.contains(&channel)
    }

    /// Sons joués par les autres dans ce salon depuis moins de [`EFFECT_SHOWN_MS`].
    pub fn recent_effects(&self, channel: u64, now_ms: i64) -> impl Iterator<Item = &EffectSeen> + '_ {
        self.effects.iter().filter(move |e| e.effect.channel_id == channel && now_ms - e.at_ms < EFFECT_SHOWN_MS)
    }

    /// Fin d'affichage du plus récent son montré (pour un réveil à ce moment-là).
    pub fn effects_until(&self, now_ms: i64) -> Option<i64> {
        self.effects.iter().map(|e| e.at_ms + EFFECT_SHOWN_MS).filter(|t| *t > now_ms).max()
    }

    pub fn take_notice(&mut self) -> Option<CallNotice> {
        self.notices.pop_front()
    }
}

impl Store {
    fn my_id(&self) -> Option<u64> {
        self.me.as_ref().map(|u| u.id)
    }

    fn call_cmd(&mut self, cmd: CallCmd) -> bool {
        if !self.calls.busy.insert(cmd.action()) {
            return false;
        }
        self.push_command(Cmd::Calls(cmd));
        true
    }

    /// Salon vocal où je suis connecté (serveur, ou MP pour `None`).
    pub fn my_voice_channel(&self, guild: Option<u64>) -> Option<u64> {
        self.voice.where_is(guild, self.my_id()?)
    }

    /// Suis-je dans ce salon ?
    pub fn in_voice(&self, channel: u64) -> bool {
        self.my_id().is_some_and(|me| self.voice.in_channel(me, channel))
    }

    /// Appels qui me font sonner, auxquels je n'ai pas encore répondu.
    pub fn incoming_calls(&self) -> Vec<u64> {
        let Some(me) = self.my_id() else { return Vec::new() };
        let mut v: Vec<u64> = self
            .calls
            .calls
            .values()
            .filter(|c| c.ringing.contains(&me) && !self.calls.answered.contains(&c.channel_id) && !self.in_voice(c.channel_id))
            .map(|c| c.channel_id)
            .collect();
        v.sort_unstable();
        v
    }

    /// « Appeler » depuis un MP ou un groupe : rejoindre, puis faire sonner les
    /// autres quand l'appel existe. Appel déjà en cours : on le rejoint seulement.
    pub fn start_call(&mut self, channel_id: u64) {
        // Jamais d'appel vers le compte système de Discord.
        if self.is_official_dm(channel_id) {
            return;
        }
        if !self.calls.calls.contains_key(&channel_id) {
            self.calls.pending_ring.insert(channel_id);
        }
        self.calls.answered.insert(channel_id);
        self.voice_join_call(channel_id);
    }

    /// « Accepter » un appel entrant : rejoindre.
    pub fn accept_call(&mut self, channel_id: u64) {
        self.calls.answered.insert(channel_id);
        self.voice_join_call(channel_id);
    }

    /// « Refuser » : ma sonnerie s'arrête, chez moi et chez Discord.
    pub fn decline_call(&mut self, channel_id: u64) -> bool {
        let Some(me) = self.my_id() else { return false };
        self.calls.answered.insert(channel_id);
        self.call_cmd(CallCmd::StopRinging { channel_id, recipients: vec![me] })
    }

    /// Faire sonner quelqu'un (ou tout le monde) pendant l'appel.
    pub fn ring(&mut self, channel_id: u64, user: Option<u64>) -> bool {
        if !self.calls.calls.contains_key(&channel_id) {
            return false;
        }
        self.call_cmd(CallCmd::Ring { channel_id, recipients: user.map(|u| vec![u]) })
    }

    /// Arrêter la sonnerie des autres.
    pub fn stop_ringing(&mut self, channel_id: u64) -> bool {
        let me = self.my_id();
        let Some(c) = self.calls.calls.get(&channel_id) else { return false };
        let recipients: Vec<u64> = c.ringing.iter().copied().filter(|u| Some(*u) != me).collect();
        if recipients.is_empty() {
            return false;
        }
        self.call_cmd(CallCmd::StopRinging { channel_id, recipients })
    }

    /// Ouvre le tableau de sons : sons par défaut et sons du serveur, une
    /// fois par session (ensuite tenus à jour par la passerelle).
    pub fn load_soundboard(&mut self, guild: Option<u64>) {
        if matches!(self.calls.default_sounds, Loadable::Absent | Loadable::Failed) && self.call_cmd(CallCmd::DefaultSounds) {
            self.calls.default_sounds = Loadable::Loading;
        }
        if let Some(g) = guild
            && self.expressions.sounds(g).is_none()
            && self.calls.guild_sounds_asked.insert(g)
        {
            self.call_cmd(CallCmd::GuildSounds { guild_id: g });
        }
    }

    /// Puis-je jouer un son dans ce salon ? Connecté, sans micro coupé, sans
    /// sourdine, pas auditeur d'une conférence (règles de Send Soundboard Sound).
    pub fn can_play_sounds(&self, channel: u64) -> bool {
        let Some(me) = self.my_id() else { return false };
        self.voice.state(me).is_some_and(|s| s.channel_id == channel && !s.muted() && !s.deafened() && !s.suppress)
    }

    /// Joue un son dans le salon où je suis ; `sound_guild` : serveur du son
    /// (`None` : son par défaut).
    pub fn play_sound(&mut self, channel_id: u64, sound_id: u64, sound_guild: Option<u64>) -> bool {
        if !self.can_play_sounds(channel_id) {
            return false;
        }
        let here = self.my_id().and_then(|me| self.voice.where_is(sound_guild, me)) == Some(channel_id);
        let source_guild_id = sound_guild.filter(|_| !here);
        self.call_cmd(CallCmd::SendSound { channel_id, sound_id, source_guild_id })
    }

    /// Sons à jouer sur ce poste (moteur audio, V7).
    pub fn take_sound_plays(&mut self) -> Vec<SoundPlay> {
        self.calls.plays.drain(..).collect()
    }

    pub(in crate::state) fn apply_calls(&mut self, e: CallEvent) -> Option<Effect> {
        let me = self.my_id();
        let now = self.now();
        let c = &mut self.calls;
        match e {
            CallEvent::Upsert { call, voice_states } => {
                let channel_id = call.channel_id;
                if let Some(states) = voice_states {
                    self.voice.replace_call(channel_id, states);
                }
                let in_call = me.is_some_and(|m| self.voice.in_channel(m, channel_id));
                // Plus de sonnerie pour moi : une nouvelle sonnerie sonnera de nouveau.
                if me.is_some_and(|m| !call.ringing.contains(&m)) && !c.pending_ring.contains(&channel_id) && !in_call {
                    c.answered.remove(&channel_id);
                }
                c.calls.insert(channel_id, call);
                if c.pending_ring.remove(&channel_id) {
                    self.call_cmd(CallCmd::Ring { channel_id, recipients: None });
                }
            }
            CallEvent::Delete { channel_id, unavailable } => {
                if unavailable {
                    if let Some(call) = c.calls.get_mut(&channel_id) {
                        call.unavailable = true;
                    }
                } else {
                    c.calls.remove(&channel_id);
                    c.answered.remove(&channel_id);
                    c.pending_ring.remove(&channel_id);
                    self.voice.clear_call(channel_id);
                }
            }
            CallEvent::Effect(effect) => {
                if let Some(sound_id) = effect.sound_id {
                    if c.plays.len() >= MAX_PLAYS {
                        c.plays.pop_front();
                    }
                    c.plays.push_back(SoundPlay { channel_id: effect.channel_id, sound_id, volume: effect.sound_volume.unwrap_or(1.0).clamp(0.0, 1.0) });
                }
                if c.effects.len() >= MAX_EFFECTS {
                    c.effects.pop_front();
                }
                c.effects.push_back(EffectSeen { effect, at_ms: now });
            }
            CallEvent::DefaultSounds(list) => {
                c.busy.remove(&(CallAction::DefaultSounds, 0));
                c.default_sounds = Loadable::Ready(list);
            }
            CallEvent::Done { action, key } => {
                c.busy.remove(&(action, key));
            }
            CallEvent::Failed { action, key, problem } => {
                c.busy.remove(&(action, key));
                match action {
                    CallAction::DefaultSounds => c.default_sounds = Loadable::Failed,
                    // Une autre ouverture du tableau redemandera.
                    CallAction::GuildSounds => {
                        c.guild_sounds_asked.remove(&key);
                    }
                    _ => {}
                }
                if let Some(Problem::BadToken) = problem {
                    return Some(Effect::Fatal { problem: Problem::BadToken, bad_token: true });
                }
                c.notices.push_back(CallNotice { action, problem });
            }
        }
        None
    }
}

impl From<CallEvent> for Event {
    fn from(e: CallEvent) -> Self {
        Event::Calls(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::User;
    use crate::state::model::Avatar;
    use crate::state::voice::VoiceEvent;

    const ME: u64 = 1;
    const ALICE: u64 = 2;
    const BOB: u64 = 3;
    const DM: u64 = 40;
    const G: u64 = 500;
    const ROOM: u64 = 501;

    fn store() -> Store {
        let mut s = Store::new();
        s.me = Some(User { id: ME, name: "moi".into(), avatar: Avatar::of_user(ME, None, None) });
        s
    }

    fn vs(guild: Option<u64>, channel: u64, user: u64) -> VoiceState {
        VoiceState { guild_id: guild, channel_id: channel, user_id: user, ..Default::default() }
    }

    /// VOICE_STATE_UPDATE, tenu par `state::voice`.
    fn put(s: &mut Store, st: VoiceState) {
        s.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: st.guild_id, user_id: st.user_id, state: Some(st), session_id: None }));
    }

    /// Commandes sans celles de la connexion vocale (op 4, testées dans `state::voice`).
    fn call_cmds(s: &mut Store) -> Vec<Cmd> {
        s.take_commands().into_iter().filter(|c| !matches!(c, Cmd::Voice(_))).collect()
    }

    fn call(ringing: Vec<u64>) -> Call {
        Call { channel_id: DM, message_id: Some(5 << 22), region: None, ringing, unavailable: false }
    }

    #[test]
    fn call_states_live_in_voice_states() {
        let mut s = store();
        put(&mut s, vs(Some(G), ROOM, BOB));
        s.apply(Event::Calls(CallEvent::Upsert { call: call(vec![]), voice_states: Some(vec![vs(None, DM, ALICE), vs(None, 999, ME)]) }));
        assert_eq!(s.voice.members(DM).map(|v| v.user_id).collect::<Vec<_>>(), vec![ALICE]);
        assert_eq!(s.voice.where_is(Some(G), BOB), Some(ROOM), "le serveur n'est pas touché");
        assert_eq!(s.voice.where_is(None, ALICE), Some(DM));
        s.apply(Event::Calls(CallEvent::Delete { channel_id: DM, unavailable: false }));
        assert_eq!(s.voice.count(DM), 0);
        assert_eq!(s.voice.count(ROOM), 1);
    }
    #[test]
    fn incoming_call_rings_until_answered() {
        let mut s = store();
        s.apply(Event::Calls(CallEvent::Upsert { call: call(vec![ME]), voice_states: Some(vec![vs(None, DM, ALICE)]) }));
        assert_eq!(s.incoming_calls(), vec![DM]);
        assert!(s.decline_call(DM));
        assert_eq!(call_cmds(&mut s), vec![Cmd::Calls(CallCmd::StopRinging { channel_id: DM, recipients: vec![ME] })]);
        assert!(s.incoming_calls().is_empty());
        // Discord retire ma sonnerie, puis on me rappelle : ça sonne de nouveau.
        s.apply(Event::Calls(CallEvent::Upsert { call: call(vec![]), voice_states: None }));
        s.apply(Event::Calls(CallEvent::Upsert { call: call(vec![ME]), voice_states: None }));
        assert_eq!(s.incoming_calls(), vec![DM]);
        s.accept_call(DM);
        assert!(s.take_commands().contains(&Cmd::Voice(crate::state::voice::VoiceCmd::Join { guild_id: None, channel_id: DM })), "rejoint l'appel");
        assert!(s.incoming_calls().is_empty());
        s.apply(Event::Calls(CallEvent::Delete { channel_id: DM, unavailable: false }));
        assert!(s.calls.call(DM).is_none());
        assert_eq!(s.voice.count(DM), 0);
    }

    #[test]
    fn start_call_rings_once_the_call_exists() {
        let mut s = store();
        s.start_call(DM);
        assert!(s.take_commands().contains(&Cmd::Voice(crate::state::voice::VoiceCmd::Join { guild_id: None, channel_id: DM })), "rejoint l'appel");
        assert!(s.calls.calling(DM));
        assert!(call_cmds(&mut s).is_empty(), "rien ne sonne avant l'appel");
        s.apply(Event::Calls(CallEvent::Upsert { call: call(vec![]), voice_states: Some(vec![vs(None, DM, ME)]) }));
        assert_eq!(call_cmds(&mut s), vec![Cmd::Calls(CallCmd::Ring { channel_id: DM, recipients: None })]);
        assert!(!s.calls.calling(DM));
        // Pas de doublon pendant que la sonnerie part.
        assert!(!s.ring(DM, None));
        s.apply(Event::Calls(CallEvent::Done { action: CallAction::Ring, key: DM }));
        s.apply(Event::Calls(CallEvent::Upsert { call: call(vec![ALICE, BOB]), voice_states: None }));
        assert!(s.stop_ringing(DM));
        assert_eq!(s.take_commands(), vec![Cmd::Calls(CallCmd::StopRinging { channel_id: DM, recipients: vec![ALICE, BOB] })]);
        s.apply(Event::Calls(CallEvent::Failed { action: CallAction::StopRinging, key: DM, problem: Some(Problem::Forbidden) }));
        assert_eq!(s.calls.take_notice().map(|n| n.action), Some(CallAction::StopRinging));
        assert!(s.ring(DM, Some(BOB)));
        assert_eq!(s.take_commands(), vec![Cmd::Calls(CallCmd::Ring { channel_id: DM, recipients: Some(vec![BOB]) })]);
    }

    #[test]
    fn soundboard_needs_an_unmuted_connection() {
        let mut s = store();
        assert!(!s.play_sound(ROOM, 7, Some(G)));
        put(&mut s, VoiceState { self_mute: true, ..vs(Some(G), ROOM, ME) });
        assert!(!s.play_sound(ROOM, 7, Some(G)), "micro coupé");
        put(&mut s, vs(Some(G), ROOM, ME));
        assert!(s.play_sound(ROOM, 7, Some(G)));
        assert_eq!(call_cmds(&mut s), vec![Cmd::Calls(CallCmd::SendSound { channel_id: ROOM, sound_id: 7, source_guild_id: None })]);
        s.apply(Event::Calls(CallEvent::Done { action: CallAction::SendSound, key: ROOM }));
        // Son d'un autre serveur, puis son par défaut.
        assert!(s.play_sound(ROOM, 8, Some(G + 9)));
        s.apply(Event::Calls(CallEvent::Done { action: CallAction::SendSound, key: ROOM }));
        assert!(s.play_sound(ROOM, 1, None));
        assert_eq!(
            s.take_commands(),
            vec![
                Cmd::Calls(CallCmd::SendSound { channel_id: ROOM, sound_id: 8, source_guild_id: Some(G + 9) }),
                Cmd::Calls(CallCmd::SendSound { channel_id: ROOM, sound_id: 1, source_guild_id: None })
            ]
        );
    }

    #[test]
    fn soundboard_lists_are_asked_once() {
        let mut s = store();
        s.load_soundboard(Some(G));
        s.load_soundboard(Some(G));
        assert_eq!(s.take_commands(), vec![Cmd::Calls(CallCmd::DefaultSounds), Cmd::Calls(CallCmd::GuildSounds { guild_id: G })]);
        assert_eq!(s.calls.default_sounds(), &Loadable::Loading);
        s.apply(Event::Calls(CallEvent::Failed { action: CallAction::DefaultSounds, key: 0, problem: None }));
        s.apply(Event::Calls(CallEvent::Failed { action: CallAction::GuildSounds, key: G, problem: None }));
        s.load_soundboard(Some(G));
        assert_eq!(s.take_commands().len(), 2, "redemandé après un échec");
    }

    #[test]
    fn effects_are_bounded_and_queued_for_playback() {
        let mut s = store();
        for i in 0..(MAX_EFFECTS as u64 + 4) {
            s.apply(Event::Calls(CallEvent::Effect(EffectSend { guild_id: Some(G), channel_id: ROOM, user_id: ALICE, sound_id: Some(i), sound_volume: Some(3.0), emoji: None })));
        }
        let now = s.now();
        assert_eq!(s.calls.recent_effects(ROOM, now).count(), MAX_EFFECTS);
        assert_eq!(s.calls.recent_effects(ROOM, now + EFFECT_SHOWN_MS + 1).count(), 0);
        let plays = s.take_sound_plays();
        assert_eq!(plays.len(), MAX_PLAYS);
        assert_eq!(plays[0].volume, 1.0);
        assert!(s.take_sound_plays().is_empty());
    }

    #[test]
    fn call_start_comes_from_its_message() {
        assert_eq!(call(vec![]).started_ms(), Some(DISCORD_EPOCH as i64 + 5));
    }
}
