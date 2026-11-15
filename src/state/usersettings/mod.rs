//! Réglages du compte Discord : réglages synchronisés (protobuf), réglages
//! de notification par serveur, profil, sessions, comptes liés, applications.
//!
//! Toutes les écritures suivent les mêmes règles :
//! - elles ne partent que d'un geste (une méthode appelée par l'interface) ;
//! - elles sont optimistes : la valeur change tout de suite, et revient à
//!   l'ancienne avec un message si Discord refuse ;
//! - une seule écriture à la fois par réglage (le réglage protobuf, un
//!   serveur, le profil…) : un deuxième geste pendant l'écriture est refusé ;
//! - une écriture protobuf n'envoie que le champ de premier niveau modifié,
//!   en entier (règle de Discord : « the entire top-level field being
//!   modified must be sent »), champs inconnus compris, tels quels.

pub mod b64;
pub mod extra;
pub mod notif;
pub mod proto;
pub mod schema;

use super::notice::{Notice, Problem};
use notif::{Change, GuildNotif, GuildPatch, Notifs};
use proto::{Field, Msg};
use schema::{CustomStatus, Folder, Key, SValue};
use std::collections::HashMap;

/// Donnée chargée à la demande (à l'ouverture d'une page).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Remote<T> {
    #[default]
    Idle,
    Loading,
    Ready(T),
    Failed(Problem),
}

impl<T> Remote<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Remote::Ready(t) => Some(t),
            _ => None,
        }
    }
}

/// `GET /users/@me` (docs.discord.food, « User Object »).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Profile {
    pub id: u64,
    pub username: String,
    pub global_name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub verified: bool,
    pub mfa: bool,
    /// 0 : sans Nitro ; 1 à 3 : Nitro Classic, Nitro, Nitro Basic.
    pub premium: u8,
    pub bio: String,
    pub pronouns: String,
    /// 0xRRGGBB.
    pub accent: Option<u32>,
}

/// Modification du profil : seuls les champs `Some` partent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileEdit {
    pub global_name: Option<Option<String>>,
    pub bio: Option<String>,
    pub pronouns: Option<String>,
    pub accent: Option<Option<u32>>,
    /// Couleurs du thème du profil (principale, secondaire), 0xRRGGBB ;
    /// `Some(None)` : retirées. Réservé à Nitro par Discord.
    pub theme: Option<Option<[u32; 2]>>,
    /// Effet de profil (`profile_effect_id`, lot ND) ; `Some(None)` : retiré.
    pub effect: Option<Option<u64>>,
}

/// Modification du profil propre à un serveur (`PATCH
/// /guilds/{id}/profile/@me`, docs.discord.food, « Modify Guild Member
/// Profile ») : seuls les champs `Some` partent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GuildProfileEdit {
    pub bio: Option<String>,
    pub pronouns: Option<String>,
    pub theme: Option<Option<[u32; 2]>>,
}

impl GuildProfileEdit {
    pub fn is_empty(&self) -> bool {
        *self == GuildProfileEdit::default()
    }
}

impl ProfileEdit {
    pub fn is_empty(&self) -> bool {
        *self == ProfileEdit::default()
    }

    fn apply(&self, p: &mut Profile) {
        if let Some(v) = &self.global_name {
            p.global_name = v.clone();
        }
        if let Some(v) = &self.bio {
            p.bio = v.clone();
        }
        if let Some(v) = &self.pronouns {
            p.pronouns = v.clone();
        }
        if let Some(v) = self.accent {
            p.accent = v;
        }
    }
}

/// Session ouverte (`GET /auth/sessions`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthSession {
    pub id_hash: String,
    pub last_used_ms: Option<i64>,
    pub os: Option<String>,
    pub platform: Option<String>,
    pub location: Option<String>,
}

/// Compte lié (`GET /users/@me/connections`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedAccount {
    pub kind: String,
    pub name: String,
    pub verified: bool,
    pub visible: bool,
    pub revoked: bool,
}

/// Application autorisée (`GET /oauth2/tokens`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizedApp {
    pub id: u64,
    pub name: String,
    pub scopes: Vec<String>,
}

/// Réponse à une écriture des réglages protobuf.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtoReply {
    /// Réglages renvoyés (décodés du base64), s'il y en a.
    pub bytes: Option<Vec<u8>>,
    /// Écriture écartée : les réglages avaient changé ailleurs.
    pub out_of_date: bool,
}

/// Réseau -> magasin.
#[derive(Debug)]
pub enum AccountEvent {
    /// Réglages protobuf (READY, USER_SETTINGS_PROTO_UPDATE ou lecture).
    /// `bytes` : `None` si le base64 était illisible.
    Proto { bytes: Option<Vec<u8>>, partial: bool },
    ProtoLoadFailed(Problem),
    ProtoWritten { id: u64, result: Result<ProtoReply, Problem> },
    /// `user_guild_settings` du READY ; `partial` : seulement ceux qui ont changé.
    GuildSettings { all: Vec<GuildNotif>, partial: bool },
    GuildSettingsUpdate(GuildNotif),
    GuildWritten { id: u64, guild: Option<u64>, result: Result<Option<GuildNotif>, Problem> },
    Profile(Result<Profile, Problem>),
    ProfileWritten { id: u64, result: Result<(), Problem> },
    NickWritten { guild: u64, result: Result<(), Problem> },
    GuildProfileWritten { id: u64, guild: u64, result: Result<(), Problem> },
    Sessions(Result<Vec<AuthSession>, Problem>),
    SessionLoggedOut { hash: String, result: Result<(), Problem> },
    Connections(Result<Vec<LinkedAccount>, Problem>),
    Apps(Result<Vec<AuthorizedApp>, Problem>),
    AppRevoked { id: u64, result: Result<(), Problem> },
    /// Consentements, avatar et bannière (`extra`).
    Extra(extra::ExtraEvent),
}

/// Magasin -> réseau. Chaque commande suit un geste de l'utilisateur.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccountCmd {
    LoadProto,
    /// `body` : message protobuf ne contenant que le champ de premier niveau modifié.
    PatchProto { id: u64, body: Vec<u8>, required_version: Option<u32> },
    PatchGuild { id: u64, guild: Option<u64>, patch: GuildPatch },
    LoadProfile,
    PatchProfile { id: u64, edit: ProfileEdit },
    SetNick { guild: u64, nick: Option<String> },
    PatchGuildProfile { id: u64, guild: u64, edit: GuildProfileEdit },
    LoadSessions,
    LogoutSession(String),
    LoadConnections,
    LoadApps,
    RevokeApp(u64),
    Extra(extra::ExtraCmd),
}

/// Pourquoi un geste n'a rien envoyé.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Réglages pas encore reçus de Discord (ou illisibles) : rien n'est écrit
    /// par-dessus ce qu'on ne connaît pas.
    Unknown,
    /// Une écriture du même réglage est en cours.
    Busy,
}

/// État des réglages protobuf reçus.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ProtoState {
    /// Rien reçu (READY sans réglages : compte de bot, ou champ absent).
    #[default]
    Missing,
    Loading,
    /// Reçus mais illisibles : affichés comme inconnus, jamais réécrits.
    Unreadable,
    Ready(Msg),
}

struct PendingProto {
    id: u64,
    top: u32,
    before: Vec<Field>,
    after: Vec<Field>,
}

struct PendingGuild {
    id: u64,
    before: Option<GuildNotif>,
    after: GuildNotif,
}

/// Valeurs lues une fois à chaque changement des réglages (jamais à l'affichage).
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub values: HashMap<Key, SValue>,
    pub custom_status: Option<CustomStatus>,
    pub folders: Vec<Folder>,
}

#[derive(Default)]
pub struct AccountState {
    proto: ProtoState,
    snapshot: Snapshot,
    pending_proto: Option<PendingProto>,
    pub notifs: Notifs,
    pending_guild: HashMap<Option<u64>, PendingGuild>,
    /// Replis demandés pendant une écriture du même serveur : envoyés à sa suite.
    collapse_wait: HashMap<Option<u64>, Vec<(u64, bool)>>,
    pub profile: Remote<Profile>,
    pending_profile: Option<(u64, Profile)>,
    pub nick_saving: Option<u64>,
    /// Écriture du profil d'un serveur en cours (numéro, serveur).
    pending_guild_profile: Option<(u64, u64)>,
    /// Dernier refus d'une écriture du profil (nom, bio, pseudo, image,
    /// profil de serveur) : l'édition rapide l'affiche sur place.
    profile_failure: Option<Problem>,
    pub sessions: Remote<Vec<AuthSession>>,
    /// Sessions retirées de la liste, en attente de la réponse.
    logging_out: Vec<AuthSession>,
    pub connections: Remote<Vec<LinkedAccount>>,
    pub apps: Remote<Vec<AuthorizedApp>>,
    revoking: Vec<AuthorizedApp>,
    /// Consentements, avatar et bannière.
    pub extra: extra::Extras,
    next_id: u64,
    outbox: Vec<AccountCmd>,
}

impl AccountState {
    pub fn take_commands(&mut self) -> Vec<AccountCmd> {
        let mut out = std::mem::take(&mut self.outbox);
        out.extend(self.extra.take_commands().into_iter().map(AccountCmd::Extra));
        out
    }

    pub fn proto_state(&self) -> &ProtoState {
        &self.proto
    }

    /// Les réglages synchronisés sont-ils lisibles et modifiables ?
    pub fn proto_ready(&self) -> bool {
        matches!(self.proto, ProtoState::Ready(_))
    }

    pub fn proto_busy(&self) -> bool {
        self.pending_proto.is_some()
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    /// Valeur d'un réglage synchronisé (`None` : réglages inconnus).
    pub fn value(&self, key: Key) -> Option<&SValue> {
        self.snapshot.values.get(&key)
    }

    pub fn bool(&self, key: Key) -> Option<bool> {
        match self.value(key)? {
            SValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn refresh(&mut self) {
        self.snapshot = match &self.proto {
            ProtoState::Ready(root) => {
                const ALL: [Key; 31] = [
                    Key::GuildDmsRestricted,
                    Key::MessageRequestsRestricted,
                    Key::FriendSources,
                    Key::ShowCurrentGame,
                    Key::ExplicitGuilds,
                    Key::ExplicitFriendDm,
                    Key::ExplicitOtherDm,
                    Key::DmSpamFilter,
                    Key::InlineAttachments,
                    Key::InlineEmbeds,
                    Key::RenderEmbeds,
                    Key::RenderReactions,
                    Key::AnimateEmoji,
                    Key::GifAutoPlay,
                    Key::AnimateStickers,
                    Key::ConvertEmoticons,
                    Key::ImageDescriptions,
                    Key::CommandSuggestions,
                    Key::TtsCommand,
                    Key::MessageCompact,
                    Key::InAppNotifications,
                    Key::GoLiveNotify,
                    Key::BurstReactionNotify,
                    Key::Status,
                    Key::Locale,
                    Key::Theme,
                    Key::DeveloperMode,
                    Key::AccessibilityDetection,
                    Key::ActivityJoinFriends,
                    Key::ActivityJoinVoice,
                    Key::ActivityGuildsDefault,
                ];
                Snapshot {
                    // Un réglage illisible manque, et seul lui : sa ligne dit « inconnu ».
                    values: ALL.iter().filter_map(|k| Some((*k, schema::read(root, *k).ok()?))).collect(),
                    custom_status: schema::read_custom_status(root).ok().flatten(),
                    folders: schema::read_folders(root).unwrap_or_default(),
                }
            }
            _ => Snapshot::default(),
        };
    }

    /// Lit les réglages synchronisés depuis Discord (quand le READY n'en
    /// avait pas ou qu'ils étaient illisibles).
    pub fn load_proto(&mut self) {
        if matches!(self.proto, ProtoState::Missing | ProtoState::Unreadable) {
            self.proto = ProtoState::Loading;
            self.outbox.push(AccountCmd::LoadProto);
        }
    }

    /// Écrit un réglage synchronisé.
    pub fn write(&mut self, key: Key, value: SValue) -> Result<(), Refusal> {
        self.edit_top(key.top(), |root| schema::write(root, key, &value).is_ok())
    }

    /// Remplace (ou retire, `None`) le statut personnalisé.
    pub fn set_custom_status(&mut self, status: Option<CustomStatus>, now_ms: i64) -> Result<(), Refusal> {
        let status = status.filter(|s| !s.is_empty());
        self.edit_top(schema::top::STATUS, |root| schema::write_custom_status(root, status.as_ref(), now_ms).is_ok())
    }

    /// Écriture d'un champ de premier niveau (dossiers de serveurs : `state::guild_folders`).
    pub(crate) fn edit_top(&mut self, top: u32, edit: impl FnOnce(&mut Msg) -> bool) -> Result<(), Refusal> {
        if self.pending_proto.is_some() {
            return Err(Refusal::Busy);
        }
        let ProtoState::Ready(root) = &mut self.proto else { return Err(Refusal::Unknown) };
        let before = root.raw_field(top);
        let mut edited = root.clone();
        // Champ de premier niveau illisible : rien n'est modifié ni envoyé.
        if !edit(&mut edited) {
            return Err(Refusal::Unknown);
        }
        let after = edited.raw_field(top);
        let body = Msg { fields: after.clone() }.encode();
        let required_version = schema::data_version(&edited);
        *root = edited;
        let id = self.id();
        self.outbox.push(AccountCmd::PatchProto { id, body, required_version });
        self.pending_proto = Some(PendingProto { id, top, before, after });
        self.refresh();
        Ok(())
    }

    /// Change un réglage de notification d'un serveur (`guild` : `None` pour
    /// les messages privés) ou d'un de ses salons.
    pub fn change_notif(&mut self, guild: Option<u64>, channel: Option<u64>, change: Change, window_secs: i64) -> Result<(), Refusal> {
        if self.pending_guild.contains_key(&guild) {
            return Err(Refusal::Busy);
        }
        let before = self.notifs.by_guild.get(&guild).cloned();
        let patch = GuildPatch { channel, change, window_secs };
        let mut after = self.notifs.get(guild);
        patch.apply(&mut after);
        self.notifs.by_guild.insert(guild, after.clone());
        let id = self.id();
        self.pending_guild.insert(guild, PendingGuild { id, before, after });
        self.outbox.push(AccountCmd::PatchGuild { id, guild, patch });
        Ok(())
    }

    /// Replie ou déplie des catégories. Le geste est toujours pris : pendant une
    /// écriture du même serveur, il s'affiche tout de suite et part à sa suite.
    pub fn set_collapsed(&mut self, guild: Option<u64>, list: Vec<(u64, bool)>) {
        if list.is_empty() {
            return;
        }
        if self.pending_guild.contains_key(&guild) {
            let mut shown = self.notifs.get(guild);
            GuildPatch { channel: None, change: Change::Collapsed(list.clone()), window_secs: 0 }.apply(&mut shown);
            self.notifs.by_guild.insert(guild, shown);
            let wait = self.collapse_wait.entry(guild).or_default();
            wait.retain(|(id, _)| list.iter().all(|(l, _)| l != id));
            wait.extend(list);
        } else {
            // Libre : `change_notif` ne refuse rien.
            let _ = self.change_notif(guild, None, Change::Collapsed(list), 0);
        }
    }

    pub fn guild_busy(&self, guild: Option<u64>) -> bool {
        self.pending_guild.contains_key(&guild)
    }

    pub fn load_profile(&mut self) {
        if !matches!(self.profile, Remote::Loading) && self.pending_profile.is_none() {
            self.profile = Remote::Loading;
            self.outbox.push(AccountCmd::LoadProfile);
        }
    }

    pub fn profile_busy(&self) -> bool {
        self.pending_profile.is_some()
    }

    pub fn save_profile(&mut self, edit: ProfileEdit) -> Result<(), Refusal> {
        if self.pending_profile.is_some() {
            return Err(Refusal::Busy);
        }
        let Remote::Ready(p) = &mut self.profile else { return Err(Refusal::Unknown) };
        if edit.is_empty() {
            return Ok(());
        }
        let before = p.clone();
        edit.apply(p);
        let id = self.id();
        self.pending_profile = Some((id, before));
        self.outbox.push(AccountCmd::PatchProfile { id, edit });
        Ok(())
    }

    pub fn guild_profile_busy(&self) -> bool {
        self.pending_guild_profile.is_some()
    }

    /// Écrit le profil propre à `guild` (pas d'écriture optimiste : ce profil
    /// vit dans les tables de `people`, mises à jour à la réponse).
    pub fn save_guild_profile(&mut self, guild: u64, edit: GuildProfileEdit) -> Result<(), Refusal> {
        if self.pending_guild_profile.is_some() {
            return Err(Refusal::Busy);
        }
        if edit.is_empty() {
            return Ok(());
        }
        let id = self.id();
        self.pending_guild_profile = Some((id, guild));
        self.outbox.push(AccountCmd::PatchGuildProfile { id, guild, edit });
        Ok(())
    }

    /// Refus de la dernière écriture du profil, rendu une fois.
    pub fn take_profile_failure(&mut self) -> Option<Problem> {
        self.profile_failure.take()
    }

    pub fn set_nick(&mut self, guild: u64, nick: Option<String>) -> Result<(), Refusal> {
        if self.nick_saving.is_some() {
            return Err(Refusal::Busy);
        }
        self.nick_saving = Some(guild);
        self.outbox.push(AccountCmd::SetNick { guild, nick: nick.filter(|n| !n.trim().is_empty()) });
        Ok(())
    }

    pub fn load_sessions(&mut self) {
        if !matches!(self.sessions, Remote::Loading) {
            self.sessions = Remote::Loading;
            self.outbox.push(AccountCmd::LoadSessions);
        }
    }

    pub fn logout_session(&mut self, hash: &str) -> Result<(), Refusal> {
        if self.logging_out.iter().any(|s| s.id_hash == hash) {
            return Err(Refusal::Busy);
        }
        let Remote::Ready(list) = &mut self.sessions else { return Err(Refusal::Unknown) };
        let Some(pos) = list.iter().position(|s| s.id_hash == hash) else { return Err(Refusal::Unknown) };
        self.logging_out.push(list.remove(pos));
        self.outbox.push(AccountCmd::LogoutSession(hash.to_string()));
        Ok(())
    }

    pub fn load_connections(&mut self) {
        if !matches!(self.connections, Remote::Loading) {
            self.connections = Remote::Loading;
            self.outbox.push(AccountCmd::LoadConnections);
        }
    }

    pub fn load_apps(&mut self) {
        if !matches!(self.apps, Remote::Loading) {
            self.apps = Remote::Loading;
            self.outbox.push(AccountCmd::LoadApps);
        }
    }

    pub fn revoke_app(&mut self, id: u64) -> Result<(), Refusal> {
        if self.revoking.iter().any(|a| a.id == id) {
            return Err(Refusal::Busy);
        }
        let Remote::Ready(list) = &mut self.apps else { return Err(Refusal::Unknown) };
        let Some(pos) = list.iter().position(|a| a.id == id) else { return Err(Refusal::Unknown) };
        self.revoking.push(list.remove(pos));
        self.outbox.push(AccountCmd::RevokeApp(id));
        Ok(())
    }

    /// Applique un événement ; renvoie un message à afficher.
    pub fn apply(&mut self, ev: AccountEvent) -> Option<Notice> {
        match ev {
            AccountEvent::Extra(e) => {
                if let extra::ExtraEvent::ImageWritten { result: Err(p), .. } = &e {
                    self.profile_failure = Some(p.clone());
                }
                return self.extra.apply(e);
            }
            AccountEvent::GuildProfileWritten { id, result, .. } => {
                self.pending_guild_profile.take_if(|(pid, _)| *pid == id)?;
                if let Err(p) = result {
                    self.profile_failure = Some(p.clone());
                    return Some(Notice::SettingNotSaved(p));
                }
            }
            AccountEvent::Proto { bytes, partial } => {
                let decoded = bytes.and_then(|b| Msg::decode(&b).ok());
                self.proto = match (decoded, std::mem::take(&mut self.proto)) {
                    (None, _) => ProtoState::Unreadable,
                    (Some(update), ProtoState::Ready(mut root)) if partial => {
                        for top in update.top_numbers() {
                            root.restore_field(top, update.raw_field(top));
                        }
                        ProtoState::Ready(root)
                    }
                    // Mise à jour partielle sans base connue : on ne sait pas le reste.
                    (Some(_), _) if partial => ProtoState::Unreadable,
                    (Some(m), _) => ProtoState::Ready(m),
                };
                self.refresh();
            }
            AccountEvent::ProtoLoadFailed(p) => {
                if self.proto == ProtoState::Loading {
                    self.proto = ProtoState::Missing;
                }
                return Some(Notice::Error(p));
            }
            AccountEvent::ProtoWritten { id, result } => {
                let pending = self.pending_proto.take_if(|p| p.id == id)?;
                let notice = match result {
                    Ok(ProtoReply { bytes, out_of_date: false }) => {
                        // Réponse : les réglages tels que Discord les a gardés.
                        if let Some(m) = bytes.and_then(|b| Msg::decode(&b).ok())
                            && let ProtoState::Ready(root) = &mut self.proto
                        {
                            for top in m.top_numbers() {
                                root.restore_field(top, m.raw_field(top));
                            }
                        }
                        None
                    }
                    Ok(ProtoReply { bytes, out_of_date: true }) => {
                        self.revert_proto(pending);
                        if let Some(m) = bytes.and_then(|b| Msg::decode(&b).ok()) {
                            self.proto = ProtoState::Ready(m);
                        }
                        Some(Notice::SettingOutdated)
                    }
                    Err(p) => {
                        self.revert_proto(pending);
                        Some(Notice::SettingNotSaved(p))
                    }
                };
                self.refresh();
                return notice;
            }
            AccountEvent::GuildSettings { all, partial } => {
                if !partial {
                    self.notifs.by_guild.clear();
                }
                for g in all {
                    self.notifs.by_guild.insert(g.guild, g);
                }
            }
            AccountEvent::GuildSettingsUpdate(g) => {
                self.notifs.by_guild.insert(g.guild, g);
            }
            AccountEvent::GuildWritten { id, guild, result } => {
                if self.pending_guild.get(&guild).is_none_or(|p| p.id != id) {
                    return None;
                }
                let pending = self.pending_guild.remove(&guild)?;
                let notice = match result {
                    Ok(Some(g)) => {
                        self.notifs.by_guild.insert(guild, g);
                        None
                    }
                    Ok(None) => None,
                    Err(p) => {
                        // Rétabli seulement si rien n'est arrivé entre-temps.
                        if self.notifs.by_guild.get(&guild) == Some(&pending.after) {
                            match pending.before {
                                Some(b) => self.notifs.by_guild.insert(guild, b),
                                None => self.notifs.by_guild.remove(&guild),
                            };
                        }
                        Some(Notice::SettingNotSaved(p))
                    }
                };
                if let Some(list) = self.collapse_wait.remove(&guild) {
                    self.set_collapsed(guild, list);
                }
                return notice;
            }
            AccountEvent::Profile(r) => {
                if self.pending_profile.is_none() {
                    self.profile = match r {
                        Ok(p) => Remote::Ready(p),
                        Err(e) => Remote::Failed(e),
                    };
                }
            }
            AccountEvent::ProfileWritten { id, result } => {
                let (_, before) = self.pending_profile.take_if(|(pid, _)| *pid == id)?;
                if let Err(p) = result {
                    self.profile = Remote::Ready(before);
                    self.profile_failure = Some(p.clone());
                    return Some(Notice::SettingNotSaved(p));
                }
            }
            AccountEvent::NickWritten { guild, result } => {
                if self.nick_saving == Some(guild) {
                    self.nick_saving = None;
                }
                if let Err(p) = result {
                    self.profile_failure = Some(p.clone());
                    return Some(Notice::SettingNotSaved(p));
                }
            }
            AccountEvent::Sessions(r) => self.sessions = r.map_or_else(Remote::Failed, Remote::Ready),
            AccountEvent::SessionLoggedOut { hash, result } => {
                let pos = self.logging_out.iter().position(|s| s.id_hash == hash)?;
                let s = self.logging_out.remove(pos);
                if let Err(p) = result {
                    if let Remote::Ready(list) = &mut self.sessions {
                        list.push(s);
                    }
                    return Some(Notice::SettingNotSaved(p));
                }
            }
            AccountEvent::Connections(r) => self.connections = r.map_or_else(Remote::Failed, Remote::Ready),
            AccountEvent::Apps(r) => self.apps = r.map_or_else(Remote::Failed, Remote::Ready),
            AccountEvent::AppRevoked { id, result } => {
                let pos = self.revoking.iter().position(|a| a.id == id)?;
                let a = self.revoking.remove(pos);
                if let Err(p) = result {
                    if let Remote::Ready(list) = &mut self.apps {
                        list.push(a);
                    }
                    return Some(Notice::SettingNotSaved(p));
                }
            }
        }
        None
    }

    /// Remet le champ écrit à sa valeur d'avant, s'il n'a pas changé depuis.
    fn revert_proto(&mut self, p: PendingProto) {
        if let ProtoState::Ready(root) = &mut self.proto
            && root.raw_field(p.top) == p.after
        {
            root.restore_field(p.top, p.before);
        }
    }

    pub fn is_logging_out(&self, hash: &str) -> bool {
        self.logging_out.iter().any(|s| s.id_hash == hash)
    }
}

#[cfg(test)]
mod tests {
    use super::notif::{Level, Mute};
    use super::proto::{wrapper, Value};
    use super::schema::top;
    use super::*;

    /// Réglages reçus : un réglage de texte connu, un inconnu dans la même
    /// section, une section entière inconnue.
    fn received() -> Vec<u8> {
        let mut text = Msg::default();
        text.set(10, Value::Len(wrapper(Value::Varint(1)).encode()));
        text.set(500, Value::Len(b"inconnu".to_vec()));
        let mut versions = Msg::default();
        versions.set(3, Value::Varint(4));
        let mut root = Msg::default();
        root.set(top::VERSIONS, Value::Len(versions.encode()));
        root.set(top::TEXT_AND_IMAGES, Value::Len(text.encode()));
        root.set(250, Value::Len(b"section future".to_vec()));
        root.encode()
    }

    fn ready() -> AccountState {
        let mut s = AccountState::default();
        s.apply(AccountEvent::Proto { bytes: Some(received()), partial: false });
        s
    }

    #[test]
    fn write_sends_only_the_modified_top_level_field_with_its_unknowns() {
        let mut s = ready();
        assert_eq!(s.bool(Key::InlineEmbeds), Some(true));
        s.write(Key::InlineEmbeds, SValue::Bool(false)).unwrap();
        assert_eq!(s.bool(Key::InlineEmbeds), Some(false), "optimiste");
        let cmds = s.take_commands();
        let [AccountCmd::PatchProto { id, body, required_version }] = cmds.as_slice() else { panic!("{cmds:?}") };
        assert_eq!(*required_version, Some(4));
        let sent = Msg::decode(body).unwrap();
        assert_eq!(sent.top_numbers(), vec![top::TEXT_AND_IMAGES], "un seul champ de premier niveau");
        let text = sent.message(top::TEXT_AND_IMAGES).unwrap().unwrap();
        assert_eq!(text.bytes(500), Some(&b"inconnu"[..]), "réglage inconnu renvoyé tel quel");
        // Deuxième geste pendant l'écriture : refusé, rien n'est envoyé.
        assert_eq!(s.write(Key::GifAutoPlay, SValue::Bool(false)), Err(Refusal::Busy));
        assert!(s.take_commands().is_empty());
        assert_eq!(s.apply(AccountEvent::ProtoWritten { id: *id, result: Ok(ProtoReply { bytes: None, out_of_date: false }) }), None);
        assert!(!s.proto_busy());
        let ProtoState::Ready(root) = s.proto_state() else { panic!() };
        assert_eq!(root.bytes(250), Some(&b"section future"[..]));
    }

    #[test]
    fn refused_write_is_rolled_back_with_a_message() {
        let mut s = ready();
        let before = s.proto_state().clone();
        s.write(Key::InlineEmbeds, SValue::Bool(false)).unwrap();
        let AccountCmd::PatchProto { id, .. } = s.take_commands().remove(0) else { panic!() };
        let n = s.apply(AccountEvent::ProtoWritten { id, result: Err(Problem::Http(400)) });
        assert_eq!(n, Some(Notice::SettingNotSaved(Problem::Http(400))));
        assert_eq!(s.bool(Key::InlineEmbeds), Some(true));
        assert_eq!(s.proto_state(), &before, "octet pour octet");
        // Discord : modifié ailleurs entre-temps.
        s.write(Key::InlineEmbeds, SValue::Bool(false)).unwrap();
        let AccountCmd::PatchProto { id, .. } = s.take_commands().remove(0) else { panic!() };
        let n = s.apply(AccountEvent::ProtoWritten { id, result: Ok(ProtoReply { bytes: Some(received()), out_of_date: true }) });
        assert_eq!(n, Some(Notice::SettingOutdated));
        assert_eq!(s.bool(Key::InlineEmbeds), Some(true));
    }

    #[test]
    fn nothing_is_written_over_unknown_settings() {
        let mut s = AccountState::default();
        assert_eq!(s.write(Key::InlineEmbeds, SValue::Bool(false)), Err(Refusal::Unknown));
        s.apply(AccountEvent::Proto { bytes: Some(vec![0, 0, 0]), partial: false });
        assert_eq!(s.proto_state(), &ProtoState::Unreadable);
        assert_eq!(s.write(Key::InlineEmbeds, SValue::Bool(false)), Err(Refusal::Unknown));
        assert!(s.take_commands().is_empty());
        s.load_proto();
        assert_eq!(s.take_commands(), vec![AccountCmd::LoadProto]);
        // Mise à jour partielle sans réglages connus : toujours inconnus.
        s.apply(AccountEvent::Proto { bytes: Some(received()), partial: true });
        assert_eq!(s.proto_state(), &ProtoState::Unreadable);
    }

    #[test]
    fn partial_update_replaces_only_its_top_level_fields() {
        let mut s = ready();
        let mut text = Msg::default();
        text.set(10, Value::Len(Vec::new()));
        let mut update = Msg::default();
        update.set(top::TEXT_AND_IMAGES, Value::Len(text.encode()));
        s.apply(AccountEvent::Proto { bytes: Some(update.encode()), partial: true });
        assert_eq!(s.bool(Key::InlineEmbeds), Some(false));
        let ProtoState::Ready(root) = s.proto_state() else { panic!() };
        assert_eq!(root.bytes(250), Some(&b"section future"[..]));
    }

    #[test]
    fn guild_notification_write_is_optimistic_and_single() {
        let mut s = AccountState::default();
        let mute = Mute { on: true, until_ms: Some(5000) };
        s.change_notif(Some(1), None, Change::Mute(mute), 3600).unwrap();
        assert!(s.notifs.muted(Some(1), 9, None, 0));
        assert_eq!(s.change_notif(Some(1), Some(9), Change::Level(Level::All), 0), Err(Refusal::Busy));
        // Un autre serveur peut être modifié en même temps.
        s.change_notif(Some(2), None, Change::SuppressEveryone(true), 0).unwrap();
        let cmds = s.take_commands();
        assert_eq!(cmds.len(), 2);
        let AccountCmd::PatchGuild { id, .. } = &cmds[0] else { panic!() };
        let n = s.apply(AccountEvent::GuildWritten { id: *id, guild: Some(1), result: Err(Problem::Forbidden) });
        assert_eq!(n, Some(Notice::SettingNotSaved(Problem::Forbidden)));
        assert!(!s.notifs.muted(Some(1), 9, None, 0), "annulé");
        let AccountCmd::PatchGuild { id, .. } = &cmds[1] else { panic!() };
        let mut confirmed = GuildNotif::new(Some(2));
        confirmed.suppress_everyone = true;
        confirmed.level = Level::All;
        assert_eq!(s.apply(AccountEvent::GuildWritten { id: *id, guild: Some(2), result: Ok(Some(confirmed.clone())) }), None);
        assert_eq!(s.notifs.get(Some(2)), confirmed);
    }

    #[test]
    fn profile_sessions_and_apps() {
        let mut s = AccountState::default();
        assert_eq!(s.save_profile(ProfileEdit { bio: Some("x".into()), ..Default::default() }), Err(Refusal::Unknown));
        s.load_profile();
        assert_eq!(s.take_commands(), vec![AccountCmd::LoadProfile]);
        s.apply(AccountEvent::Profile(Ok(Profile { bio: "avant".into(), ..Default::default() })));
        s.save_profile(ProfileEdit { bio: Some("après".into()), ..Default::default() }).unwrap();
        assert_eq!(s.profile.ready().map(|p| p.bio.as_str()), Some("après"));
        let AccountCmd::PatchProfile { id, .. } = s.take_commands().remove(0) else { panic!() };
        s.apply(AccountEvent::ProfileWritten { id, result: Err(Problem::Http(400)) });
        assert_eq!(s.profile.ready().map(|p| p.bio.as_str()), Some("avant"));
        // Le refus est gardé pour l'édition rapide, et rendu une seule fois.
        assert_eq!(s.take_profile_failure(), Some(Problem::Http(400)));
        assert_eq!(s.take_profile_failure(), None);

        // Profil d'un serveur : une écriture à la fois, rien d'envoyé si rien ne change.
        assert_eq!(s.save_guild_profile(9, GuildProfileEdit::default()), Ok(()));
        assert!(s.take_commands().is_empty() && !s.guild_profile_busy());
        let edit = GuildProfileEdit { bio: Some("ici".into()), theme: Some(Some([1, 2])), ..Default::default() };
        s.save_guild_profile(9, edit.clone()).unwrap();
        assert_eq!(s.save_guild_profile(9, edit.clone()), Err(Refusal::Busy));
        let cmds = s.take_commands();
        let [AccountCmd::PatchGuildProfile { id, guild: 9, edit: sent }] = cmds.as_slice() else { panic!("{cmds:?}") };
        assert_eq!(sent, &edit);
        let nitro = Problem::Profile(crate::state::ProfileRefusal::Nitro);
        assert_eq!(s.apply(AccountEvent::GuildProfileWritten { id: *id, guild: 9, result: Err(nitro.clone()) }), Some(Notice::SettingNotSaved(nitro.clone())));
        assert!(!s.guild_profile_busy());
        assert_eq!(s.take_profile_failure(), Some(nitro));

        let sess = |h: &str| AuthSession { id_hash: h.into(), last_used_ms: None, os: None, platform: None, location: None };
        s.apply(AccountEvent::Sessions(Ok(vec![sess("a"), sess("b")])));
        s.logout_session("a").unwrap();
        assert_eq!(s.sessions.ready().map(Vec::len), Some(1));
        assert_eq!(s.logout_session("a"), Err(Refusal::Busy));
        s.apply(AccountEvent::SessionLoggedOut { hash: "a".into(), result: Err(Problem::Http(401)) });
        assert_eq!(s.sessions.ready().map(Vec::len), Some(2), "rétablie");

        s.apply(AccountEvent::Apps(Ok(vec![AuthorizedApp { id: 3, name: "Bot".into(), scopes: vec![] }])));
        s.revoke_app(3).unwrap();
        assert_eq!(s.apps.ready().map(Vec::len), Some(0));
        assert_eq!(s.apply(AccountEvent::AppRevoked { id: 3, result: Ok(()) }), None);
        assert_eq!(s.apps.ready().map(Vec::len), Some(0));
    }
}
