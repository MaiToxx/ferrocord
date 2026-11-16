//! Vocal des serveurs (phase 4, lot V4) : salons vocaux et de conférence,
//! qui y est connecté et dans quel état, ma propre connexion, réglages par
//! personne. Aucun son ni connexion vocale ici : le lot de câblage (V7)
//! remplit `speaking`, le ping et les échecs par [`VoiceEvent`], et lit
//! [`VoiceConnState::Connecting`] pour ouvrir la passerelle vocale.
//!
//! Formes documentées par Discord (vérifiées le 13 novembre 2026,
//! discord.com/developers/docs : Resources › Voice, Topics › Gateway
//! Events, Resources › Guild) : Voice State (`channel_id`, `user_id`,
//! `member`, `session_id`, `deaf`, `mute`, `self_deaf`, `self_mute`,
//! `self_stream`, `self_video`, `suppress`, `request_to_speak_timestamp`),
//! VOICE_STATE_UPDATE, VOICE_SERVER_UPDATE (`token`, `guild_id`,
//! `endpoint` nul quand le serveur vocal tombe), Update Voice State (op 4 :
//! `guild_id`, `channel_id` nul pour quitter, `self_mute`, `self_deaf`),
//! Modify Guild Member (`mute`, `deaf`, `channel_id` nul pour déconnecter ;
//! en-tête `X-Audit-Log-Reason`), salons de type 2 (vocal) et 13
//! (conférence) avec `user_limit`.

use super::model::{Avatar, Guild, Overwrite};
use super::notice::Problem;
use super::perms::{apply_overwrites, base_permissions, Permissions};
use super::{Cmd, Store};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Jeton du serveur vocal : même enveloppe que les adresses de webhooks
/// (`Debug` masqué), partagée avec `net::voice`.
pub use super::admin::webhooks::Secret;

/// Salon vocal (2) ou de conférence (13). Le magasin principal ne garde que
/// les salons écrits : les salons vocaux vivent ici.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceChannel {
    pub id: u64,
    pub guild_id: u64,
    pub name: String,
    pub stage: bool,
    pub position: i32,
    pub parent: Option<u64>,
    /// `None` si illisibles : permissions inconnues (le salon reste visible).
    pub overwrites: Option<Vec<Overwrite>>,
    /// 0 : pas de limite.
    pub user_limit: u32,
}

/// État vocal d'une personne (Voice State).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VoiceState {
    pub user_id: u64,
    pub guild_id: Option<u64>,
    pub channel_id: u64,
    pub self_mute: bool,
    pub self_deaf: bool,
    /// Rendu muet ou sourd par un modérateur.
    pub mute: bool,
    pub deaf: bool,
    /// Auditeur d'une conférence (ou sans droit de parole).
    pub suppress: bool,
    pub self_video: bool,
    /// Partage d'écran (« en direct »).
    pub self_stream: bool,
    /// Demande de parole dans une conférence (ms Unix).
    pub request_to_speak: Option<i64>,
    /// Nom et avatar si le paquet les donne (`member.user`).
    pub name: Option<String>,
    pub avatar: Option<Avatar>,
}

impl VoiceState {
    /// Muet pour les autres, quelle qu'en soit la cause.
    pub fn muted(&self) -> bool {
        self.self_mute || self.mute || self.self_deaf || self.deaf
    }

    pub fn deafened(&self) -> bool {
        self.self_deaf || self.deaf
    }
}

/// Où en est ma connexion vocale.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum VoiceConnState {
    #[default]
    Idle,
    /// Op 4 envoyée ; on attend la session et le serveur vocal de Discord.
    Joining,
    /// Tout est connu pour ouvrir la passerelle vocale (lot V7).
    Connecting { endpoint: String, token: Secret, session_id: String },
    Connected { ping_ms: Option<u32> },
    /// Message à afficher (venu de la connexion vocale).
    Failed(String),
}

/// Salon où je suis (ou que je rejoins).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MyVoice {
    pub guild_id: Option<u64>,
    pub channel_id: u64,
}

/// Geste de modération sur une personne connectée.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceMod {
    /// Muet côté serveur (`mute`), ou le retirer.
    Mute(bool),
    /// Sourdine côté serveur (`deaf`), ou la retirer.
    Deafen(bool),
    /// Déplacer vers ce salon (`channel_id`).
    Move(u64),
    /// Déconnecter (`channel_id` nul).
    Disconnect,
}

impl VoiceMod {
    /// Permission demandée par Discord pour ce geste.
    pub fn permission(self) -> Permissions {
        match self {
            VoiceMod::Mute(_) => Permissions::MUTE_MEMBERS,
            VoiceMod::Deafen(_) => Permissions::DEAFEN_MEMBERS,
            VoiceMod::Move(_) | VoiceMod::Disconnect => Permissions::MOVE_MEMBERS,
        }
    }
}

/// Interface -> réseau (et moteur audio, lot V7).
#[derive(Clone, Debug, PartialEq)]
pub enum VoiceCmd {
    /// `guild_id` : `None` pour un appel privé (lot V6).
    Join { guild_id: Option<u64>, channel_id: u64 },
    Leave,
    SetMute(bool),
    SetDeaf(bool),
    /// Volume d'une personne, de 0 à 2 (1 : normal).
    SetUserVolume(u64, f32),
    /// Muet pour moi seulement.
    SetUserMuted(u64, bool),
    /// Touche d'appui-pour-parler enfoncée ou relâchée.
    SetPtt(bool),
    /// Modération : `PATCH /guilds/{g}/members/{u}`.
    Moderate { guild_id: u64, user_id: u64, action: VoiceMod, reason: Option<String> },
}

// Les volumes sont bornés et finis (`clamp_volume`) : l'égalité est réflexive.
impl Eq for VoiceCmd {}

/// Volume borné à [0, 2] ; une valeur non finie vaut 1.
pub fn clamp_volume(v: f32) -> f32 {
    if v.is_finite() { v.clamp(0.0, 2.0) } else { 1.0 }
}

/// Réseau (et connexion vocale, lot V7) -> magasin.
#[derive(Debug)]
pub enum VoiceEvent {
    /// Nouvelle session (READY) : rien de l'ancienne ne vaut.
    Reset,
    /// Salons vocaux et états d'un serveur (READY, GUILD_CREATE) : remplacent l'existant.
    Guild { guild_id: u64, channels: Vec<VoiceChannel>, states: Vec<VoiceState> },
    ChannelUpsert(VoiceChannel),
    ChannelDelete { guild_id: u64, id: u64 },
    GuildGone(u64),
    /// États ajoutés sans rien remplacer (READY_SUPPLEMENTAL, lu par `net::calls`).
    States(Vec<VoiceState>),
    /// VOICE_STATE_UPDATE : `state` vaut `None` quand la personne quitte le vocal.
    StateUpdate { guild_id: Option<u64>, user_id: u64, state: Option<VoiceState>, session_id: Option<String> },
    /// VOICE_SERVER_UPDATE ; `endpoint` : `None` quand le serveur vocal est tombé.
    Server { guild_id: Option<u64>, endpoint: Option<String>, token: Secret },
    // Remplis par la connexion vocale (lot V7).
    Connected,
    Ping(u32),
    Speaking { user_id: u64, speaking: bool },
    Failed(String),
    Closed,
    /// Session vocale expirée (4006, 4009) : l'op 4 repart pour en obtenir une neuve.
    Expired,
    /// Périphérique audio en panne pendant l'appel (`None` : réparé).
    AudioProblem(Option<String>),
    /// Codes de vérification DAVE demandés.
    Codes(VoiceCodes),
    /// Réponse d'une modération.
    Moderated { guild_id: u64, user_id: u64, action: VoiceMod },
    ModFailed { guild_id: u64, user_id: u64, action: VoiceMod, problem: Problem },
}

/// Codes de vérification du chiffrement de bout en bout (DAVE), à comparer
/// de vive voix avec ceux que montrent les autres clients.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VoiceCodes {
    /// Code de l'appel (le même chez tous) ; `None` : pas de chiffrement de bout en bout.
    pub call: Option<String>,
    /// Code de paire avec chaque participant.
    pub users: Vec<(u64, Option<String>)>,
}

/// Corps de l'op 4 (Update Voice State).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Op4 {
    pub guild_id: Option<u64>,
    pub channel_id: Option<u64>,
    pub self_mute: bool,
    pub self_deaf: bool,
}

/// État vocal tenu par la passerelle : chaque commande qui le change donne
/// l'op 4 à envoyer (état complet : la dernière envoyée fait foi).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Op4State {
    guild_id: Option<u64>,
    channel_id: Option<u64>,
    self_mute: bool,
    self_deaf: bool,
}

impl Op4State {
    pub fn on_cmd(&mut self, c: &VoiceCmd) -> Option<Op4> {
        match *c {
            VoiceCmd::Join { guild_id, channel_id } => {
                self.guild_id = guild_id;
                self.channel_id = Some(channel_id);
            }
            VoiceCmd::Leave => {
                self.channel_id?;
                self.channel_id = None;
                // Le serveur quitté reste nommé : c'est sa connexion qui se ferme.
                return Some(self.payload());
            }
            VoiceCmd::SetMute(b) => {
                self.self_mute = b;
                self.channel_id?;
            }
            VoiceCmd::SetDeaf(b) => {
                self.self_deaf = b;
                self.channel_id?;
            }
            _ => return None,
        }
        Some(self.payload())
    }

    /// Nouvelle session : Discord a oublié mon salon vocal.
    pub fn forget_channel(&mut self) {
        self.channel_id = None;
    }

    /// Op 4 de l'état actuel, si je suis dans un salon (caméra, directs : phase 5).
    pub fn current(&self) -> Option<Op4> {
        self.channel_id?;
        Some(self.payload())
    }

    fn payload(&self) -> Op4 {
        // Sourdine = muet aussi, comme Discord.
        Op4 { guild_id: self.guild_id, channel_id: self.channel_id, self_mute: self.self_mute || self.self_deaf, self_deaf: self.self_deaf }
    }
}

/// Réglages d'une personne, retenus dans `local.toml`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserVoice {
    /// Pour cent, de 0 à 200.
    pub volume: u16,
    pub muted: bool,
}

impl Default for UserVoice {
    fn default() -> Self {
        UserVoice { volume: 100, muted: false }
    }
}

/// Personnes retenues au plus dans `local.toml` : au-delà, les plus petits
/// identifiants (les comptes les plus anciens) sont oubliés.
pub const MAX_USER_PREFS: usize = 2000;

/// Réglages vocaux de cet ordinateur (section `[vocal]` de `local.toml`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VoicePrefs {
    pub mute: bool,
    pub deaf: bool,
    pub users: BTreeMap<u64, UserVoice>,
}

impl VoicePrefs {
    pub fn user(&self, id: u64) -> UserVoice {
        self.users.get(&id).copied().unwrap_or_default()
    }

    /// Change les réglages d'une personne ; ceux par défaut ne sont pas gardés.
    pub fn set_user(&mut self, id: u64, u: UserVoice) {
        let u = UserVoice { volume: u.volume.min(200), ..u };
        if u == UserVoice::default() {
            self.users.remove(&id);
            return;
        }
        self.users.insert(id, u);
        while self.users.len() > MAX_USER_PREFS {
            self.users.pop_first();
        }
    }
}

/// Pourquoi rejoindre ce salon est impossible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinBlock {
    NoConnect,
    Full,
    Unknown,
}

/// Issue d'une modération, à annoncer une fois.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModOutcome {
    Done(VoiceMod),
    Failed(VoiceMod, Problem),
}

#[derive(Default)]
pub struct VoiceStates {
    /// Salons vocaux par serveur, triés par position.
    channels: HashMap<u64, Vec<VoiceChannel>>,
    /// Serveur de chaque salon vocal.
    chan_guild: HashMap<u64, u64>,
    /// États par personne (une personne n'est que dans un salon à la fois).
    states: HashMap<u64, VoiceState>,
    /// Personnes de chaque salon, dans l'ordre d'arrivée.
    by_channel: HashMap<u64, Vec<u64>>,
    /// Mes permissions dans chaque salon vocal (absent : inconnues),
    /// recalculées seulement quand rôles, membre ou salons changent.
    perms: HashMap<u64, Permissions>,
    /// Qui parle en ce moment (lot V7).
    pub speaking: HashSet<u64>,
    pub conn: VoiceConnState,
    pub me: Option<MyVoice>,
    /// Muet et sourdine choisis (gardés hors connexion, envoyés en rejoignant).
    pub self_mute: bool,
    pub self_deaf: bool,
    /// Muet choisi, hors celui qu'impose la sourdine (`self_mute` = choix ou sourdine).
    mute_choice: bool,
    /// Ma session et mon serveur vocal, reçus dans un ordre quelconque.
    session_id: Option<String>,
    server: Option<(String, Secret)>,
    /// Dernière modération terminée (lue une fois par l'interface).
    outcome: Option<ModOutcome>,
    /// Derniers codes de vérification reçus (lot V7).
    pub codes: Option<VoiceCodes>,
    /// Micro ou sortie en panne pendant l'appel (message affiché).
    pub audio_problem: Option<String>,
    /// Instants des dernières op 4 renvoyées d'elles-mêmes après une session
    /// vocale expirée (lot « fiable » : jamais en boucle).
    auto_rejoins: Vec<std::time::Instant>,
    /// Compteur des changements de présence vocale (qui est où, qui partage) :
    /// les listes qui en dépendent ne se refont qu'à son changement.
    rev: u64,
}

/// Op 4 renvoyées sans geste après une session vocale expirée (codes 4006,
/// 4009) : au plus [`AUTO_REJOIN_MAX`] par [`AUTO_REJOIN_WINDOW`] ; au-delà,
/// l'appel s'arrête sur un message et c'est à l'utilisateur de rejoindre.
pub const AUTO_REJOIN_MAX: usize = 3;
pub const AUTO_REJOIN_WINDOW: std::time::Duration = std::time::Duration::from_secs(300);

impl VoiceStates {
    /// Session vocale de la passerelle principale (aussi celle des directs).
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn clear(&mut self) {
        let (mute, deaf) = (self.self_mute, self.self_deaf);
        *self = VoiceStates { self_mute: mute, self_deaf: deaf, mute_choice: self.mute_choice, rev: self.rev.wrapping_add(1), ..Default::default() };
    }

    /// Révision des présences vocales : change à chaque arrivée, départ,
    /// déplacement ou changement d'état (muet, caméra, partage).
    pub fn rev(&self) -> u64 {
        self.rev
    }

    /// Salons vocaux d'un serveur, par position.
    pub fn channels(&self, guild_id: u64) -> &[VoiceChannel] {
        self.channels.get(&guild_id).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn channel(&self, id: u64) -> Option<&VoiceChannel> {
        let g = self.chan_guild.get(&id)?;
        self.channels.get(g)?.iter().find(|c| c.id == id)
    }

    /// Personnes connectées à ce salon, dans l'ordre d'arrivée.
    pub fn members(&self, channel_id: u64) -> impl Iterator<Item = &VoiceState> {
        self.by_channel.get(&channel_id).into_iter().flatten().filter_map(|u| self.states.get(u))
    }

    pub fn count(&self, channel_id: u64) -> usize {
        self.by_channel.get(&channel_id).map_or(0, Vec::len)
    }

    pub fn state(&self, user_id: u64) -> Option<&VoiceState> {
        self.states.get(&user_id)
    }

    /// Mes permissions dans ce salon vocal (`None` : inconnues).
    pub fn perms(&self, channel_id: u64) -> Option<Permissions> {
        self.perms.get(&channel_id).copied()
    }

    /// Salon masqué (pas de VIEW_CHANNEL connu).
    pub fn hidden(&self, channel_id: u64) -> bool {
        self.perms(channel_id).is_some_and(|p| !p.contains(Permissions::VIEW_CHANNEL))
    }

    /// Permission connue et accordée.
    pub fn can(&self, channel_id: u64, p: Permissions) -> bool {
        self.perms(channel_id).is_some_and(|x| x.contains(p))
    }

    pub fn is_full(&self, channel_id: u64) -> bool {
        self.channel(channel_id).is_some_and(|c| !c.stage && c.user_limit > 0 && self.count(channel_id) >= c.user_limit as usize)
    }

    /// Je peux parler dans mon salon (permission SPEAK, sans muet ni
    /// suppression imposés par le serveur). Lu par le moteur audio (lot V7).
    pub fn can_speak(&self, me: u64) -> bool {
        let Some(my) = self.me else { return false };
        let allowed = my.guild_id.is_none() || self.perms(my.channel_id).is_none_or(|p| p.contains(Permissions::SPEAK));
        allowed && self.states.get(&me).is_none_or(|s| !s.mute && !s.suppress)
    }

    /// Muet choisi par l'utilisateur, hors celui qu'impose la sourdine.
    pub fn mute_choice(&self) -> bool {
        self.mute_choice
    }

    /// Dernière modération terminée, rendue une seule fois.
    pub fn take_outcome(&mut self) -> Option<ModOutcome> {
        self.outcome.take()
    }

    /// Salon de `user` (dans ce serveur, ou en appel privé pour `None`).
    pub fn where_is(&self, guild: Option<u64>, user: u64) -> Option<u64> {
        self.states.get(&user).filter(|s| s.guild_id == guild).map(|s| s.channel_id)
    }

    /// `user` est-il connecté à ce salon ?
    pub fn in_channel(&self, user: u64, channel_id: u64) -> bool {
        self.states.get(&user).is_some_and(|s| s.channel_id == channel_id)
    }

    /// Liste complète d'un appel privé (CALL_CREATE, lot V6) : remplace l'ancienne.
    pub(super) fn replace_call(&mut self, channel_id: u64, states: Vec<VoiceState>) {
        self.clear_call(channel_id);
        for s in states.into_iter().filter(|s| s.channel_id == channel_id) {
            self.put_user(VoiceState { guild_id: None, ..s });
        }
    }

    /// Fin d'un appel privé (CALL_DELETE) : plus personne dedans.
    pub(super) fn clear_call(&mut self, channel_id: u64) {
        let gone: Vec<u64> = self.members(channel_id).filter(|s| s.guild_id.is_none()).map(|s| s.user_id).collect();
        for u in gone {
            self.remove_user(u);
        }
    }

    fn remove_user(&mut self, user_id: u64) {
        self.rev = self.rev.wrapping_add(1);
        if let Some(old) = self.states.remove(&user_id)
            && let Some(list) = self.by_channel.get_mut(&old.channel_id)
        {
            list.retain(|u| *u != user_id);
            if list.is_empty() {
                self.by_channel.remove(&old.channel_id);
            }
        }
        self.speaking.remove(&user_id);
    }

    fn put_user(&mut self, s: VoiceState) {
        let moved = self.states.get(&s.user_id).is_none_or(|old| old.channel_id != s.channel_id);
        if moved {
            self.remove_user(s.user_id);
            self.by_channel.entry(s.channel_id).or_default().push(s.user_id);
        }
        self.rev = self.rev.wrapping_add(1);
        self.states.insert(s.user_id, s);
    }

    fn forget_guild(&mut self, guild_id: u64) {
        if let Some(old) = self.channels.remove(&guild_id) {
            for c in old {
                self.chan_guild.remove(&c.id);
                self.perms.remove(&c.id);
            }
        }
        let gone: Vec<u64> = self.states.values().filter(|s| s.guild_id == Some(guild_id)).map(|s| s.user_id).collect();
        for u in gone {
            self.remove_user(u);
        }
    }

    fn leave_locally(&mut self) {
        self.me = None;
        self.conn = VoiceConnState::Idle;
        self.session_id = None;
        self.server = None;
        self.speaking.clear();
        self.codes = None;
        self.audio_problem = None;
    }

    /// Session et serveur connus : la passerelle vocale peut s'ouvrir.
    fn try_connecting(&mut self) {
        if self.me.is_none() || !matches!(self.conn, VoiceConnState::Joining) {
            return;
        }
        if let (Some(session_id), Some((endpoint, token))) = (&self.session_id, &self.server) {
            self.conn = VoiceConnState::Connecting { endpoint: endpoint.clone(), token: token.clone(), session_id: session_id.clone() };
        }
    }

    fn upsert_channel(&mut self, c: VoiceChannel) {
        self.rev = self.rev.wrapping_add(1);
        self.chan_guild.insert(c.id, c.guild_id);
        let list = self.channels.entry(c.guild_id).or_default();
        match list.iter_mut().find(|x| x.id == c.id) {
            Some(slot) => *slot = c,
            None => list.push(c),
        }
        list.sort_by_key(|c| (c.position, c.id));
    }
}

/// Mes permissions dans un salon vocal ; `None` si une information manque.
pub fn voice_permissions(guild: &Guild, me: u64, channel: &VoiceChannel, now_ms: i64) -> Option<Permissions> {
    let base = base_permissions(guild, me)?;
    if base == Permissions::ALL {
        return Some(Permissions::ALL);
    }
    let member = guild.me.as_ref()?;
    let mut perms = apply_overwrites(base, guild.id, me, member, channel.overwrites.as_deref()?);
    if perms.contains(Permissions::ADMINISTRATOR) {
        return Some(Permissions::ALL);
    }
    // Exclusion temporaire : on voit encore le salon, on ne s'y connecte plus.
    if member.timed_out(now_ms) {
        perms = perms & Permissions::VIEW_CHANNEL;
    }
    if !perms.contains(Permissions::VIEW_CHANNEL) {
        return Some(Permissions::NONE);
    }
    Some(perms)
}

impl Store {
    /// Recalcule mes permissions dans les salons vocaux d'un serveur (appelé
    /// avec celles des salons écrits, et quand les salons vocaux changent).
    pub(super) fn recompute_voice(&mut self, guild_id: u64) {
        let me = self.me.as_ref().map(|u| u.id);
        let now = self.now();
        let guild = self.guilds.iter().find(|g| g.id == guild_id);
        let v = &mut self.voice;
        for c in v.channels.get(&guild_id).into_iter().flatten() {
            match guild.zip(me).and_then(|(g, me)| voice_permissions(g, me, c, now)) {
                Some(p) => v.perms.insert(c.id, p),
                None => v.perms.remove(&c.id),
            };
        }
    }

    pub(super) fn apply_voice(&mut self, e: VoiceEvent) {
        let me = self.me.as_ref().map(|u| u.id);
        let v = &mut self.voice;
        match e {
            VoiceEvent::Reset => v.clear(),
            VoiceEvent::Guild { guild_id, channels, states } => {
                v.forget_guild(guild_id);
                for c in channels {
                    v.upsert_channel(c);
                }
                let mut mine = false;
                for s in states {
                    mine |= Some(s.user_id) == me;
                    v.put_user(s);
                }
                // Mon salon n'y est plus (session neuve, serveur rechargé) : je n'y suis plus.
                if !mine && v.me.is_some_and(|m| m.guild_id == Some(guild_id)) && v.conn != VoiceConnState::Joining {
                    v.leave_locally();
                }
                self.recompute_voice(guild_id);
            }
            VoiceEvent::ChannelUpsert(c) => {
                let g = c.guild_id;
                v.upsert_channel(c);
                self.recompute_voice(g);
            }
            VoiceEvent::ChannelDelete { guild_id, id } => {
                if let Some(list) = v.channels.get_mut(&guild_id) {
                    list.retain(|c| c.id != id);
                }
                v.chan_guild.remove(&id);
                v.perms.remove(&id);
                let gone: Vec<u64> = v.members(id).map(|s| s.user_id).collect();
                for u in gone {
                    v.remove_user(u);
                }
                if v.me.is_some_and(|m| m.channel_id == id) {
                    v.leave_locally();
                }
            }
            VoiceEvent::GuildGone(guild_id) => {
                v.forget_guild(guild_id);
                if v.me.is_some_and(|m| m.guild_id == Some(guild_id)) {
                    v.leave_locally();
                }
            }
            VoiceEvent::States(states) => {
                for s in states {
                    v.put_user(s);
                }
            }
            VoiceEvent::StateUpdate { guild_id, user_id, state, session_id } => {
                let is_me = Some(user_id) == me;
                match state {
                    Some(s) => {
                        if is_me {
                            let here = MyVoice { guild_id: s.guild_id, channel_id: s.channel_id };
                            // Déplacé par un modérateur, ou confirmation de mon op 4.
                            if v.me != Some(here) {
                                v.me = Some(here);
                                if !matches!(v.conn, VoiceConnState::Connecting { .. } | VoiceConnState::Connected { .. }) {
                                    v.conn = VoiceConnState::Joining;
                                }
                            }
                            if session_id.is_some() {
                                v.session_id = session_id;
                            }
                        }
                        v.put_user(s);
                        if is_me {
                            v.try_connecting();
                        }
                    }
                    None => {
                        // Seul l'état de ce serveur est retiré : en changeant de
                        // serveur, l'arrivée peut précéder le départ.
                        if v.states.get(&user_id).is_none_or(|s| s.guild_id == guild_id) {
                            v.remove_user(user_id);
                        }
                        if is_me && v.me.is_some_and(|m| m.guild_id == guild_id) {
                            v.leave_locally();
                        }
                    }
                }
            }
            VoiceEvent::Server { guild_id, endpoint, token } => {
                if v.me.is_some_and(|m| m.guild_id == guild_id) {
                    match endpoint {
                        Some(e) => {
                            v.server = Some((e, token));
                            // Nouveau serveur vocal : la connexion est à refaire.
                            if !matches!(v.conn, VoiceConnState::Idle) {
                                v.conn = VoiceConnState::Joining;
                            }
                            v.try_connecting();
                        }
                        None => {
                            v.server = None;
                            v.conn = VoiceConnState::Joining;
                        }
                    }
                }
            }
            VoiceEvent::Connected => {
                if v.me.is_some() {
                    v.conn = VoiceConnState::Connected { ping_ms: None };
                }
            }
            VoiceEvent::Ping(ms) => {
                if let VoiceConnState::Connected { ping_ms } = &mut v.conn {
                    *ping_ms = Some(ms);
                }
            }
            VoiceEvent::Speaking { user_id, speaking } => {
                if speaking {
                    v.speaking.insert(user_id);
                } else {
                    v.speaking.remove(&user_id);
                }
            }
            VoiceEvent::Failed(why) => {
                if v.me.is_some() {
                    v.conn = VoiceConnState::Failed(why);
                }
            }
            VoiceEvent::Closed => {
                v.speaking.clear();
                v.codes = None;
                if matches!(v.conn, VoiceConnState::Connected { .. }) {
                    v.conn = VoiceConnState::Joining;
                }
            }
            VoiceEvent::Expired => {
                let Some(my) = v.me else { return };
                v.speaking.clear();
                v.codes = None;
                // Une session qui expire sans cesse ne doit pas faire partir
                // une op 4 à chaque fois : après quelques reprises, on s'arrête.
                let now = std::time::Instant::now();
                v.auto_rejoins.retain(|t| now.duration_since(*t) < AUTO_REJOIN_WINDOW);
                if v.auto_rejoins.len() >= AUTO_REJOIN_MAX {
                    v.server = None;
                    v.conn = VoiceConnState::Failed(crate::i18n::tr(crate::i18n::table::voice::t::EXPIRED_LOOP).to_string());
                    return;
                }
                v.auto_rejoins.push(now);
                // Discord répond à l'op 4 par une session et un serveur neufs.
                v.server = None;
                v.conn = VoiceConnState::Joining;
                self.push_command(Cmd::Voice(VoiceCmd::Join { guild_id: my.guild_id, channel_id: my.channel_id }));
            }
            VoiceEvent::AudioProblem(p) => {
                if v.me.is_some() {
                    v.audio_problem = p;
                }
            }
            VoiceEvent::Codes(c) => {
                if v.me.is_some() {
                    v.codes = Some(c);
                }
            }
            VoiceEvent::Moderated { action, .. } => v.outcome = Some(ModOutcome::Done(action)),
            VoiceEvent::ModFailed { action, problem, .. } => v.outcome = Some(ModOutcome::Failed(action, problem)),
        }
    }

    /// Pourquoi je ne peux pas rejoindre ce salon (`None` : possible).
    /// Permissions inconnues : on laisse Discord trancher.
    pub fn voice_join_block(&self, channel_id: u64) -> Option<JoinBlock> {
        let v = &self.voice;
        v.channel(channel_id)?;
        match v.perms(channel_id) {
            Some(p) if !p.contains(Permissions::CONNECT) => Some(JoinBlock::NoConnect),
            // Salon plein : seuls ceux qui peuvent déplacer des membres y entrent.
            p if v.is_full(channel_id) && !p.is_some_and(|p| p.contains(Permissions::MOVE_MEMBERS)) => Some(JoinBlock::Full),
            _ => None,
        }
    }

    /// Rejoint un salon vocal de serveur (ou y change de salon).
    pub fn voice_join(&mut self, channel_id: u64) -> Result<(), JoinBlock> {
        let Some(guild_id) = self.voice.channel(channel_id).map(|c| c.guild_id) else { return Err(JoinBlock::Unknown) };
        if let Some(b) = self.voice_join_block(channel_id) {
            return Err(b);
        }
        self.voice_join_at(MyVoice { guild_id: Some(guild_id), channel_id });
        Ok(())
    }

    /// Rejoint l'appel d'un MP ou d'un groupe (lot V6 : « Appeler », « Accepter »).
    pub fn voice_join_call(&mut self, channel_id: u64) {
        self.voice_join_at(MyVoice { guild_id: None, channel_id });
    }

    fn voice_join_at(&mut self, here: MyVoice) {
        let MyVoice { guild_id, channel_id } = here;
        if self.voice.me == Some(here) && !matches!(self.voice.conn, VoiceConnState::Failed(_)) {
            return;
        }
        let v = &mut self.voice;
        // Hors de tout salon, la passerelle apprend d'abord mon muet et ma sourdine.
        if v.me.is_none() {
            let (mute, deaf) = (v.mute_choice, v.self_deaf);
            self.push_command(Cmd::Voice(VoiceCmd::SetMute(mute)));
            self.push_command(Cmd::Voice(VoiceCmd::SetDeaf(deaf)));
        }
        let v = &mut self.voice;
        // Changement de serveur : session et serveur vocal seront nouveaux.
        if v.me.is_some_and(|m| m.guild_id != here.guild_id) {
            v.session_id = None;
        }
        v.server = None;
        v.speaking.clear();
        v.me = Some(here);
        v.conn = VoiceConnState::Joining;
        self.push_command(Cmd::Voice(VoiceCmd::Join { guild_id, channel_id }));
    }

    pub fn voice_leave(&mut self) {
        if self.voice.me.is_none() {
            return;
        }
        self.voice.leave_locally();
        self.push_command(Cmd::Voice(VoiceCmd::Leave));
        // Caméra, partage et directs s'arrêtent avec l'appel (phase 5).
        self.video_follow_voice();
    }

    /// Muet : comme Discord, retirer le muet pendant la sourdine retire aussi
    /// la sourdine. `SetMute` porte le choix ; la sourdine rend muet d'elle-même.
    pub fn voice_set_mute(&mut self, on: bool) {
        let v = &mut self.voice;
        if !on && v.self_deaf {
            let was = v.mute_choice;
            v.self_deaf = false;
            v.self_mute = false;
            v.mute_choice = false;
            self.push_command(Cmd::Voice(VoiceCmd::SetDeaf(false)));
            if was {
                self.push_command(Cmd::Voice(VoiceCmd::SetMute(false)));
            }
            return;
        }
        if v.mute_choice == on && v.self_mute == on {
            return;
        }
        v.mute_choice = on;
        v.self_mute = on || v.self_deaf;
        self.push_command(Cmd::Voice(VoiceCmd::SetMute(on)));
    }

    /// Sourdine : elle rend muet ; la retirer rend le muet choisi.
    pub fn voice_set_deaf(&mut self, on: bool) {
        let v = &mut self.voice;
        if v.self_deaf == on {
            return;
        }
        v.self_deaf = on;
        v.self_mute = v.mute_choice || on;
        self.push_command(Cmd::Voice(VoiceCmd::SetDeaf(on)));
    }

    /// Muet et sourdine relus de `local.toml` (rien n'est envoyé : ils le
    /// seront en rejoignant un salon).
    pub fn voice_restore(&mut self, mute: bool, deaf: bool) {
        let v = &mut self.voice;
        if v.me.is_some() {
            return;
        }
        v.self_deaf = deaf;
        v.self_mute = mute || deaf;
        v.mute_choice = mute;
    }

    pub fn voice_set_ptt(&mut self, pressed: bool) {
        self.push_command(Cmd::Voice(VoiceCmd::SetPtt(pressed)));
    }

    pub fn voice_user(&mut self, user_id: u64, u: UserVoice) {
        self.push_command(Cmd::Voice(VoiceCmd::SetUserVolume(user_id, clamp_volume(f32::from(u.volume) / 100.0))));
        self.push_command(Cmd::Voice(VoiceCmd::SetUserMuted(user_id, u.muted)));
    }

    /// Ce geste de modération m'est permis sur cette personne (permission
    /// connue dans son salon ; pour un déplacement, CONNECT dans la destination).
    pub fn voice_can_moderate(&self, user_id: u64, action: VoiceMod) -> bool {
        let v = &self.voice;
        let Some(s) = v.state(user_id) else { return false };
        if s.guild_id.is_none() || !v.can(s.channel_id, action.permission()) {
            return false;
        }
        match action {
            VoiceMod::Move(to) => to != s.channel_id && v.channel(to).is_some_and(|c| Some(c.guild_id) == s.guild_id) && v.can(to, Permissions::CONNECT),
            _ => true,
        }
    }

    pub fn voice_moderate(&mut self, user_id: u64, action: VoiceMod, reason: Option<String>) -> bool {
        let Some(guild_id) = self.voice.state(user_id).and_then(|s| s.guild_id) else { return false };
        if !self.voice_can_moderate(user_id, action) {
            return false;
        }
        let reason = reason.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
        self.push_command(Cmd::Voice(VoiceCmd::Moderate { guild_id, user_id, action, reason }));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::{Channel, ChannelKind, Member, OverwriteKind, Role, User};
    use crate::state::Event;

    const G: u64 = 100;
    const ME: u64 = 1;
    const BOB: u64 = 2;
    const CAROL: u64 = 3;
    const VOC: u64 = 501;
    const LOCKED: u64 = 502;
    const SMALL: u64 = 503;
    const R_MOD: u64 = 201;

    fn chan(id: u64, limit: u32, overwrites: Vec<Overwrite>) -> VoiceChannel {
        VoiceChannel { id, guild_id: G, name: format!("v{id}"), stage: false, position: id as i32, parent: None, overwrites: Some(overwrites), user_limit: limit }
    }

    fn state(user_id: u64, channel_id: u64) -> VoiceState {
        VoiceState { user_id, guild_id: Some(G), channel_id, ..Default::default() }
    }

    fn deny_connect() -> Overwrite {
        Overwrite { id: G, kind: OverwriteKind::Role, allow: Permissions::NONE, deny: Permissions::CONNECT }
    }

    /// Serveur où @everyone voit, se connecte et parle ; le rôle R_MOD modère.
    fn store(my_roles: Vec<u64>) -> Store {
        let mut s = Store::new();
        let everyone = Permissions(Permissions::VIEW_CHANNEL.0 | Permissions::CONNECT.0 | Permissions::SPEAK.0 | Permissions::SEND_MESSAGES.0);
        let modp = Permissions(Permissions::MUTE_MEMBERS.0 | Permissions::DEAFEN_MEMBERS.0 | Permissions::MOVE_MEMBERS.0);
        let mut g = Guild::new(G, "g".into(), vec![Channel { id: 9, name: "t".into(), kind: ChannelKind::Text, position: 0, parent: None, overwrites: Some(vec![]), slowmode: 0 }]);
        g.owner_id = Some(99);
        g.roles = vec![
            Role { id: G, name: "@everyone".into(), color: 0, position: 0, permissions: everyone, mentionable: false },
            Role { id: R_MOD, name: "mod".into(), color: 0, position: 1, permissions: modp, mentionable: false },
        ];
        g.me = Some(Member { roles: my_roles, nick: None, timeout_until: None });
        s.apply(Event::Ready { me: User { id: ME, name: "moi".into(), avatar: Avatar::default_for(ME) }, guilds: vec![g], dms: vec![] });
        s.apply(Event::Voice(VoiceEvent::Guild {
            guild_id: G,
            channels: vec![chan(VOC, 0, vec![]), chan(LOCKED, 0, vec![deny_connect()]), chan(SMALL, 1, vec![])],
            states: vec![state(BOB, VOC), state(CAROL, SMALL)],
        }));
        s.take_commands();
        s
    }

    fn voice_cmds(s: &mut Store) -> Vec<VoiceCmd> {
        s.take_commands().into_iter().filter_map(|c| if let Cmd::Voice(v) = c { Some(v) } else { None }).collect()
    }

    #[test]
    fn op4_carries_the_whole_state_and_leave_names_the_guild() {
        let mut o = Op4State::default();
        assert_eq!(o.on_cmd(&VoiceCmd::SetMute(true)), None, "hors salon : rien à envoyer");
        assert_eq!(o.on_cmd(&VoiceCmd::Join { guild_id: Some(G), channel_id: VOC }), Some(Op4 { guild_id: Some(G), channel_id: Some(VOC), self_mute: true, self_deaf: false }));
        assert_eq!(o.on_cmd(&VoiceCmd::SetDeaf(true)), Some(Op4 { guild_id: Some(G), channel_id: Some(VOC), self_mute: true, self_deaf: true }));
        assert_eq!(o.on_cmd(&VoiceCmd::SetPtt(true)), None);
        assert_eq!(o.on_cmd(&VoiceCmd::Leave), Some(Op4 { guild_id: Some(G), channel_id: None, self_mute: true, self_deaf: true }));
        assert_eq!(o.on_cmd(&VoiceCmd::Leave), None, "déjà parti");
        o.on_cmd(&VoiceCmd::Join { guild_id: Some(G), channel_id: VOC });
        o.forget_channel();
        assert_eq!(o.on_cmd(&VoiceCmd::SetMute(false)), None);
    }

    #[test]
    fn states_follow_updates_and_moves() {
        let mut s = store(vec![]);
        assert_eq!(s.voice.members(VOC).map(|x| x.user_id).collect::<Vec<_>>(), vec![BOB]);
        // Carol passe dans VOC : elle arrive après Bob.
        s.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(G), user_id: CAROL, state: Some(VoiceState { self_video: true, ..state(CAROL, VOC) }), session_id: None }));
        assert_eq!(s.voice.members(VOC).map(|x| x.user_id).collect::<Vec<_>>(), vec![BOB, CAROL]);
        assert_eq!(s.voice.count(SMALL), 0);
        assert!(s.voice.state(CAROL).is_some_and(|x| x.self_video));
        s.apply(Event::Voice(VoiceEvent::Speaking { user_id: BOB, speaking: true }));
        assert!(s.voice.speaking.contains(&BOB));
        // Bob part : retiré, et ne parle plus.
        s.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(G), user_id: BOB, state: None, session_id: None }));
        assert_eq!(s.voice.members(VOC).map(|x| x.user_id).collect::<Vec<_>>(), vec![CAROL]);
        assert!(!s.voice.speaking.contains(&BOB));
        // Salon supprimé : ses occupants avec.
        s.apply(Event::Voice(VoiceEvent::ChannelDelete { guild_id: G, id: VOC }));
        assert_eq!(s.voice.count(VOC), 0);
        assert!(s.voice.state(CAROL).is_none());
        s.apply(Event::Voice(VoiceEvent::GuildGone(G)));
        assert!(s.voice.channels(G).is_empty());
    }

    #[test]
    fn joining_sends_mute_first_then_connects_when_session_and_server_are_known() {
        let mut s = store(vec![]);
        s.voice_set_deaf(true);
        assert!(s.voice.self_mute && s.voice.self_deaf, "la sourdine rend muet");
        voice_cmds(&mut s);
        assert_eq!(s.voice_join(VOC), Ok(()));
        assert_eq!(voice_cmds(&mut s), vec![VoiceCmd::SetMute(false), VoiceCmd::SetDeaf(true), VoiceCmd::Join { guild_id: Some(G), channel_id: VOC }]);
        assert_eq!(s.voice.conn, VoiceConnState::Joining);
        // Serveur d'abord, session ensuite : l'ordre n'importe pas.
        s.apply(Event::Voice(VoiceEvent::Server { guild_id: Some(G), endpoint: Some("voix.example.invalid:443".into()), token: Secret::new("secret-vocal".into()) }));
        assert_eq!(s.voice.conn, VoiceConnState::Joining);
        s.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(G), user_id: ME, state: Some(VoiceState { self_mute: true, self_deaf: true, ..state(ME, VOC) }), session_id: Some("sess".into()) }));
        let VoiceConnState::Connecting { endpoint, token, session_id } = &s.voice.conn else { panic!("{:?}", s.voice.conn) };
        assert_eq!((endpoint.as_str(), token.expose(), session_id.as_str()), ("voix.example.invalid:443", "secret-vocal", "sess"));
        assert!(!format!("{:?}", s.voice.conn).contains("secret-vocal"), "jeton masqué");
        s.apply(Event::Voice(VoiceEvent::Connected));
        s.apply(Event::Voice(VoiceEvent::Ping(42)));
        assert_eq!(s.voice.conn, VoiceConnState::Connected { ping_ms: Some(42) });
        // Même salon : rien ne part ; un autre : une seule op 4.
        assert_eq!(s.voice_join(VOC), Ok(()));
        assert!(voice_cmds(&mut s).is_empty());
        assert_eq!(s.voice_join(SMALL), Err(JoinBlock::Full));
        s.voice_leave();
        assert_eq!(voice_cmds(&mut s), vec![VoiceCmd::Leave]);
        assert_eq!((s.voice.me, &s.voice.conn), (None, &VoiceConnState::Idle));
    }

    #[test]
    fn mute_and_deafen_follow_discord() {
        let mut s = store(vec![]);
        s.voice_set_mute(true);
        s.voice_set_deaf(true);
        s.voice_set_deaf(false);
        assert!(s.voice.self_mute && !s.voice.self_deaf, "muet d'avant rendu");
        s.voice_set_mute(false);
        s.voice_set_deaf(true);
        voice_cmds(&mut s);
        // Retirer le muet pendant la sourdine retire les deux.
        s.voice_set_mute(false);
        assert!(!s.voice.self_mute && !s.voice.self_deaf);
        assert_eq!(voice_cmds(&mut s), vec![VoiceCmd::SetDeaf(false)]);
        // Muet choisi puis sourdine : une seule commande chacun, le muet choisi revient.
        s.voice_set_mute(true);
        s.voice_set_deaf(true);
        assert_eq!(voice_cmds(&mut s), vec![VoiceCmd::SetMute(true), VoiceCmd::SetDeaf(true)]);
        assert!(s.voice.mute_choice());
    }

    #[test]
    fn permissions_gate_join_and_moderation() {
        let mut s = store(vec![]);
        assert_eq!(s.voice_join(LOCKED), Err(JoinBlock::NoConnect));
        assert!(!s.voice.hidden(LOCKED));
        assert!(s.voice.can(VOC, Permissions::SPEAK));
        assert!(!s.voice_can_moderate(BOB, VoiceMod::Mute(true)));
        assert!(!s.voice_moderate(BOB, VoiceMod::Disconnect, None));
        // Rôle de modération reçu : les gestes deviennent permis, sauf vers un salon fermé.
        let mut s = store(vec![R_MOD]);
        assert!(s.voice_can_moderate(BOB, VoiceMod::Mute(true)));
        assert!(s.voice_can_moderate(BOB, VoiceMod::Move(SMALL)));
        assert!(!s.voice_can_moderate(BOB, VoiceMod::Move(LOCKED)));
        assert!(!s.voice_can_moderate(BOB, VoiceMod::Move(VOC)), "déjà là");
        assert_eq!(s.voice_join(SMALL), Ok(()), "salon plein, mais je peux déplacer des membres");
        voice_cmds(&mut s);
        assert!(s.voice_moderate(BOB, VoiceMod::Deafen(true), Some("  bruit  ".into())));
        assert_eq!(voice_cmds(&mut s), vec![VoiceCmd::Moderate { guild_id: G, user_id: BOB, action: VoiceMod::Deafen(true), reason: Some("bruit".into()) }]);
        s.apply(Event::Voice(VoiceEvent::Moderated { guild_id: G, user_id: BOB, action: VoiceMod::Deafen(true) }));
        assert_eq!(s.voice.take_outcome(), Some(ModOutcome::Done(VoiceMod::Deafen(true))));
        assert_eq!(s.voice.take_outcome(), None);
    }

    #[test]
    fn hidden_channels_and_timeout() {
        let mut s = store(vec![]);
        let hide = Overwrite { id: G, kind: OverwriteKind::Role, allow: Permissions::NONE, deny: Permissions::VIEW_CHANNEL };
        s.apply(Event::Voice(VoiceEvent::ChannelUpsert(chan(504, 0, vec![hide]))));
        assert!(s.voice.hidden(504));
        // Exclusion temporaire : plus de CONNECT nulle part.
        let until = crate::state::store::now_ms() + 60_000;
        s.apply(Event::MemberUpdate { guild_id: G, user_id: ME, member: Some(Member { roles: vec![], nick: None, timeout_until: Some(until) }) });
        assert_eq!(s.voice_join(VOC), Err(JoinBlock::NoConnect));
    }

    #[test]
    fn being_moved_or_dropped_by_discord_is_followed() {
        let mut s = store(vec![]);
        assert_eq!(s.voice_join(VOC), Ok(()));
        s.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(G), user_id: ME, state: Some(state(ME, SMALL)), session_id: Some("s".into()) }));
        assert_eq!(s.voice.me, Some(MyVoice { guild_id: Some(G), channel_id: SMALL }));
        s.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(G), user_id: ME, state: None, session_id: None }));
        assert_eq!((s.voice.me, &s.voice.conn), (None, &VoiceConnState::Idle));
        // Nouvelle session : tout est oublié, sauf mon muet.
        s.voice_set_mute(true);
        s.apply(Event::Voice(VoiceEvent::Reset));
        assert!(s.voice.channels(G).is_empty() && s.voice.self_mute);
    }

    #[test]
    fn a_voice_session_that_keeps_expiring_stops_rejoining() {
        let mut s = store(vec![]);
        assert_eq!(s.voice_join(VOC), Ok(()));
        voice_cmds(&mut s);
        for _ in 0..AUTO_REJOIN_MAX {
            s.apply(Event::Voice(VoiceEvent::Expired));
            assert_eq!(voice_cmds(&mut s), vec![VoiceCmd::Join { guild_id: Some(G), channel_id: VOC }]);
        }
        // Une de plus dans la même fenêtre : plus aucune op 4, un message à la place.
        s.apply(Event::Voice(VoiceEvent::Expired));
        assert!(voice_cmds(&mut s).is_empty(), "pas de boucle d'op 4");
        assert!(matches!(s.voice.conn, VoiceConnState::Failed(_)));
        s.apply(Event::Voice(VoiceEvent::Expired));
        assert!(voice_cmds(&mut s).is_empty());
        // Le geste de l'utilisateur reste possible.
        assert_eq!(s.voice_join(VOC), Ok(()));
        assert_eq!(voice_cmds(&mut s), vec![VoiceCmd::Join { guild_id: Some(G), channel_id: VOC }]);
    }

    #[test]
    fn orchestrator_want_follows_the_connection_and_expiry_rejoins() {
        use crate::state::audio_prefs::AudioPrefs;
        use crate::state::voice_link::CallWant;
        let audio = AudioPrefs::default();
        let mut s = store(vec![]);
        assert_eq!(s.voice_want(&audio).call, CallWant::Stop);
        assert_eq!(s.voice_join(VOC), Ok(()));
        assert_eq!(s.voice_want(&audio).call, CallWant::Wait);
        s.apply(Event::Voice(VoiceEvent::Server { guild_id: Some(G), endpoint: Some("v.example.invalid".into()), token: Secret::new("t".into()) }));
        s.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(G), user_id: ME, state: Some(state(ME, VOC)), session_id: Some("sess".into()) }));
        let CallWant::Open(t) = s.voice_want(&audio).call else { panic!("{:?}", s.voice_want(&audio).call) };
        assert_eq!((t.channel_id, t.user_id, t.endpoint, t.session_id), (VOC, ME, "v.example.invalid", "sess"));
        let w = s.voice_want(&audio);
        assert!(!w.muted && !w.deafened);
        s.apply(Event::Voice(VoiceEvent::Connected));
        assert_eq!(s.voice_want(&audio).call, CallWant::Keep);
        // Muet imposé par le serveur : le micro est coupé aussi.
        s.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(G), user_id: ME, state: Some(VoiceState { mute: true, ..state(ME, VOC) }), session_id: None }));
        assert!(s.voice_want(&audio).muted);
        s.voice_set_deaf(true);
        let w = s.voice_want(&audio);
        assert!(w.muted && w.deafened);
        voice_cmds(&mut s);
        // Session expirée : nouvelle op 4 pour le même salon, en attente de Discord.
        s.apply(Event::Voice(VoiceEvent::Expired));
        assert_eq!(voice_cmds(&mut s), vec![VoiceCmd::Join { guild_id: Some(G), channel_id: VOC }]);
        assert_eq!(s.voice_want(&audio).call, CallWant::Wait);
        s.apply(Event::Voice(VoiceEvent::Codes(VoiceCodes { call: Some("1".into()), users: vec![] })));
        assert!(s.voice.codes.is_some());
        s.voice_leave();
        assert!(s.voice.codes.is_none());
    }

    #[test]
    fn user_prefs_drop_defaults_and_stay_bounded() {
        let mut p = VoicePrefs::default();
        p.set_user(5, UserVoice { volume: 150, muted: false });
        p.set_user(6, UserVoice { volume: 900, muted: true });
        assert_eq!(p.user(6).volume, 200);
        p.set_user(5, UserVoice::default());
        assert!(!p.users.contains_key(&5));
        for i in 0..(MAX_USER_PREFS as u64 + 10) {
            p.set_user(1000 + i, UserVoice { volume: 50, muted: false });
        }
        assert_eq!(p.users.len(), MAX_USER_PREFS);
        assert_eq!(clamp_volume(f32::NAN), 1.0);
        assert_eq!(clamp_volume(3.0), 2.0);
    }
}
