//! Paramètres d'un serveur (lot A8 + A9) : vue d'ensemble (`PATCH
//! /guilds/{id}`), niveau de boost et ses avantages, et la file commune des
//! commandes d'administration de ce lot (webhooks et intégrations compris).
//!
//! Rien ne part sans geste : ouvrir la fenêtre demande le serveur une fois,
//! chaque écriture suit un clic. Les valeurs affichées viennent de Discord
//! (réponse, ou GUILD_UPDATE) : jamais d'état supposé après une écriture.
//!
//! Mémoire bornée : seules les données du serveur dont la fenêtre est
//! ouverte sont gardées ([`Store::forget_guild_admin`] à la fermeture).

use super::integrations::{CommandPerms, Integration};
use super::webhooks::{HookScope, Webhook};
use crate::state::event::Cmd;
use crate::state::notice::Problem;
use crate::state::perms::{base_permissions, Permissions};
use crate::state::store::Store;
use std::collections::{HashMap, HashSet, VecDeque};

/// Délais d'inactivité acceptés pour le salon AFK (secondes ; documentation, Guild).
pub const AFK_TIMEOUTS: [u32; 5] = [60, 300, 900, 1800, 3600];

/// Options du salon système (System Channel Flags, documentation) : un bit
/// levé COUPE le message correspondant.
pub mod sysflags {
    pub const SUPPRESS_JOIN: u64 = 1 << 0;
    pub const SUPPRESS_BOOSTS: u64 = 1 << 1;
    pub const SUPPRESS_TIPS: u64 = 1 << 2;
    pub const SUPPRESS_JOIN_REPLIES: u64 = 1 << 3;
}

/// Notifications par défaut (`default_message_notifications`).
pub const NOTIFY_ALL: u8 = 0;
pub const NOTIFY_MENTIONS: u8 = 1;

/// Langues proposées pour `preferred_locale` (liste « Locales » de la documentation).
pub const LOCALES: &[&str] = &[
    "id", "da", "de", "en-GB", "en-US", "es-ES", "es-419", "fr", "hr", "it", "lt", "hu", "nl", "no", "pl", "pt-BR", "ro", "fi", "sv-SE", "vi", "tr", "cs", "el", "bg", "ru",
    "uk", "hi", "th", "zh-CN", "ja", "zh-TW", "ko",
];

/// Boosts nécessaires à chaque niveau (aide de Discord, « Server Boosting FAQ »).
pub const BOOSTS_FOR_TIER: [u32; 4] = [0, 2, 7, 14];

/// Avantages d'un niveau de boost (aide de Discord, « Server Boosting FAQ » ;
/// la documentation de l'API ne donne que les noms des niveaux).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Perks {
    pub emoji_slots: u32,
    pub sticker_slots: u32,
    pub soundboard_slots: u32,
    pub bitrate_kbps: u32,
    /// Taille d'envoi par fichier pour tous les membres (octets).
    pub upload: u64,
    /// Qualité de diffusion : hauteur d'image et images par seconde.
    pub stream: (u32, u32),
    pub animated_icon: bool,
    pub invite_splash: bool,
    pub banner: bool,
    pub role_icons: bool,
    pub vanity_url: bool,
    pub animated_banner: bool,
}

/// Avantages du niveau `tier` (0 à 3 ; au-delà, ceux du niveau 3).
pub fn perks(tier: u8) -> Perks {
    let base = Perks {
        emoji_slots: 50,
        sticker_slots: 5,
        soundboard_slots: 8,
        bitrate_kbps: 96,
        upload: crate::state::upload::DEFAULT_LIMIT,
        stream: (720, 30),
        animated_icon: false,
        invite_splash: false,
        banner: false,
        role_icons: false,
        vanity_url: false,
        animated_banner: false,
    };
    match tier {
        0 => base,
        1 => Perks { emoji_slots: 100, sticker_slots: 15, soundboard_slots: 24, bitrate_kbps: 128, stream: (720, 60), animated_icon: true, invite_splash: true, ..base },
        2 => Perks { emoji_slots: 150, sticker_slots: 30, soundboard_slots: 36, bitrate_kbps: 256, upload: 50 << 20, stream: (1080, 60), banner: true, role_icons: true, ..perks(1) },
        _ => Perks { emoji_slots: 250, sticker_slots: 60, soundboard_slots: 48, bitrate_kbps: 384, upload: 100 << 20, vanity_url: true, animated_banner: true, ..perks(2) },
    }
}

/// Paramètres d'un serveur tels que Discord les donne.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GuildProfile {
    pub name: String,
    pub owner_id: Option<u64>,
    /// Empreintes des images (`None` : aucune).
    pub icon: Option<String>,
    pub banner: Option<String>,
    pub splash: Option<String>,
    pub description: Option<String>,
    pub system_channel_id: Option<u64>,
    pub system_channel_flags: u64,
    pub afk_channel_id: Option<u64>,
    pub afk_timeout: u32,
    pub default_message_notifications: u8,
    pub preferred_locale: String,
    pub vanity_url_code: Option<String>,
    pub premium_tier: u8,
    pub premium_subscription_count: u32,
    pub premium_progress_bar_enabled: bool,
    pub features: Vec<String>,
    /// 1 : l'A2F est exigée des modérateurs.
    pub mfa_level: u8,
    pub member_count: Option<u64>,
}

impl GuildProfile {
    pub fn has_feature(&self, f: &str) -> bool {
        self.features.iter().any(|x| x == f)
    }

    /// Bannière permise (fonction BANNER, débloquée au niveau 2).
    pub fn can_banner(&self) -> bool {
        self.has_feature("BANNER") || perks(self.premium_tier).banner
    }

    /// Image d'invitation permise (fonction INVITE_SPLASH, niveau 1).
    pub fn can_splash(&self) -> bool {
        self.has_feature("INVITE_SPLASH") || perks(self.premium_tier).invite_splash
    }

    pub fn merge(&mut self, p: &GuildPatch) {
        fn set<T: Clone>(dst: &mut T, src: &Option<T>) {
            if let Some(v) = src {
                *dst = v.clone();
            }
        }
        set(&mut self.name, &p.name);
        set(&mut self.owner_id, &p.owner_id.map(Some));
        set(&mut self.icon, &p.icon);
        set(&mut self.banner, &p.banner);
        set(&mut self.splash, &p.splash);
        set(&mut self.description, &p.description);
        set(&mut self.system_channel_id, &p.system_channel_id);
        set(&mut self.system_channel_flags, &p.system_channel_flags);
        set(&mut self.afk_channel_id, &p.afk_channel_id);
        set(&mut self.afk_timeout, &p.afk_timeout);
        set(&mut self.default_message_notifications, &p.default_message_notifications);
        set(&mut self.preferred_locale, &p.preferred_locale);
        set(&mut self.vanity_url_code, &p.vanity_url_code);
        set(&mut self.premium_tier, &p.premium_tier);
        set(&mut self.premium_subscription_count, &p.premium_subscription_count);
        set(&mut self.premium_progress_bar_enabled, &p.premium_progress_bar_enabled);
        set(&mut self.features, &p.features);
        set(&mut self.mfa_level, &p.mfa_level);
        if p.member_count.is_some() {
            self.member_count = p.member_count;
        }
    }
}

/// Champs d'un objet Guild reçu : `None` = absent du paquet (rien ne
/// change) ; pour un champ qui peut être vide, `Some(None)` = `null`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GuildPatch {
    pub name: Option<String>,
    pub owner_id: Option<u64>,
    pub icon: Option<Option<String>>,
    pub banner: Option<Option<String>>,
    pub splash: Option<Option<String>>,
    pub description: Option<Option<String>>,
    pub system_channel_id: Option<Option<u64>>,
    pub system_channel_flags: Option<u64>,
    pub afk_channel_id: Option<Option<u64>>,
    pub afk_timeout: Option<u32>,
    pub default_message_notifications: Option<u8>,
    pub preferred_locale: Option<String>,
    pub vanity_url_code: Option<Option<String>>,
    pub premium_tier: Option<u8>,
    pub premium_subscription_count: Option<u32>,
    pub premium_progress_bar_enabled: Option<bool>,
    pub features: Option<Vec<String>>,
    pub mfa_level: Option<u8>,
    pub member_count: Option<u64>,
}

/// Modification envoyée (`PATCH /guilds/{id}`) : seuls les champs `Some`
/// partent ; `Some(None)` envoie `null` (image retirée, salon aucun).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GuildEdit {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    /// Image en `data:` (PNG recadré), ou `None` pour la retirer.
    pub icon: Option<Option<String>>,
    pub banner: Option<Option<String>>,
    pub splash: Option<Option<String>>,
    pub system_channel_id: Option<Option<u64>>,
    pub system_channel_flags: Option<u64>,
    pub afk_channel_id: Option<Option<u64>>,
    pub afk_timeout: Option<u32>,
    pub default_message_notifications: Option<u8>,
    pub preferred_locale: Option<String>,
    pub premium_progress_bar_enabled: Option<bool>,
}

impl GuildEdit {
    pub fn is_empty(&self) -> bool {
        *self == GuildEdit::default()
    }
}

/// Lien d'invitation personnalisé (`GET /guilds/{id}/vanity-url`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Vanity {
    pub code: Option<String>,
    pub uses: u32,
}

/// Interface -> réseau (lot A8 + A9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdminCmd {
    /// `GET /guilds/{id}?with_counts=true`.
    Load { guild_id: u64 },
    /// `PATCH /guilds/{id}`.
    Modify { guild_id: u64, edit: Box<GuildEdit> },
    /// `GET /guilds/{id}/vanity-url`.
    Vanity { guild_id: u64 },
    /// `GET /guilds/{id}/webhooks` ou `GET /channels/{id}/webhooks`.
    Webhooks { guild_id: u64, scope: HookScope },
    /// `POST /channels/{id}/webhooks`.
    CreateWebhook { guild_id: u64, channel_id: u64, name: String, avatar: Option<String> },
    /// `PATCH /webhooks/{id}` ; `avatar` : `Some(None)` le retire.
    EditWebhook { guild_id: u64, id: u64, name: Option<String>, avatar: Option<Option<String>>, channel_id: Option<u64> },
    /// `DELETE /webhooks/{id}`.
    DeleteWebhook { guild_id: u64, id: u64, reason: Option<String> },
    /// `GET /guilds/{id}/integrations`.
    Integrations { guild_id: u64 },
    /// `DELETE /guilds/{id}/integrations/{integration}`.
    RemoveIntegration { guild_id: u64, id: u64, reason: Option<String> },
    /// `GET /applications/{app}/guilds/{guild}/commands/permissions`.
    CommandPerms { guild_id: u64, app_id: u64 },
    /// Widget, modèle de serveur, élagage (lot V, `extras.rs`).
    Extra(super::extras::ExtraCmd),
}

/// Sorte d'action, pour le message d'échec.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AdminAction {
    Load,
    Save,
    Vanity,
    Webhooks,
    SaveWebhook,
    DeleteWebhook,
    Integrations,
    RemoveIntegration,
    CommandPerms,
    Extra(super::extras::ExtraAction),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    Problem(Problem),
    /// 403 (code 50013 compris) : permission manquante.
    Forbidden,
    NotFound,
    /// Réponse sans la forme attendue.
    Unreadable,
}

/// Réseau -> magasin.
#[derive(Debug)]
pub enum AdminEvent {
    /// Serveur lu (`GET`) ou enregistré (réponse du `PATCH`).
    Loaded { guild_id: u64, patch: GuildPatch, saved: bool },
    /// GUILD_UPDATE.
    Updated { guild_id: u64, patch: GuildPatch },
    Vanity { guild_id: u64, vanity: Vanity },
    Webhooks { scope: HookScope, list: Vec<Webhook> },
    WebhookSaved { guild_id: u64, hook: Webhook, created: bool },
    WebhookDeleted { guild_id: u64, id: u64 },
    /// WEBHOOKS_UPDATE : une liste chargée est à relire.
    WebhooksChanged { guild_id: u64, channel_id: Option<u64> },
    Integrations { guild_id: u64, list: Vec<Integration> },
    IntegrationRemoved { guild_id: u64, id: u64 },
    /// GUILD_INTEGRATIONS_UPDATE.
    IntegrationsChanged { guild_id: u64 },
    CommandPerms { guild_id: u64, app_id: u64, list: Vec<CommandPerms> },
    Extra(super::extras::ExtraEvent),
    Failed { guild_id: u64, action: AdminAction, failure: Failure, target: Option<u64> },
}

/// Message à afficher (mis en mots par l'interface).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdminNotice {
    Saved,
    WebhookCreated,
    WebhookDeleted,
    IntegrationRemoved,
    Extra(super::extras::ExtraNotice),
    Failed { action: AdminAction, failure: Failure },
}

/// Donnée demandée à Discord.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Load<T> {
    Loading,
    Ready(T),
    Failed(Failure),
}

impl<T> Load<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Load::Ready(t) => Some(t),
            _ => None,
        }
    }
}

#[derive(Default)]
pub struct GuildAdmin {
    /// Serveur dont la fenêtre est ouverte : les réponses d'un autre sont écartées.
    pub open: Option<u64>,
    pub profile: Option<Load<GuildProfile>>,
    pub vanity: Option<Load<Vanity>>,
    pub saving: bool,
    pub hooks: HashMap<HookScope, Load<Vec<Webhook>>>,
    /// Listes de webhooks à relire (WEBHOOKS_UPDATE reçu).
    pub(super) stale_hooks: HashSet<HookScope>,
    /// Webhook en cours d'écriture (0 : création).
    pub(super) hook_busy: HashSet<u64>,
    /// Dernier webhook créé (son adresse peut être copiée tout de suite).
    pub created_hook: Option<u64>,
    pub integrations: Option<Load<Vec<Integration>>>,
    pub(super) stale_integrations: bool,
    pub(super) removing: HashSet<u64>,
    pub command_perms: HashMap<u64, Load<Vec<CommandPerms>>>,
    /// Widget, modèle de serveur, élagage (lot V).
    pub extras: super::extras::Extras,
    notices: VecDeque<AdminNotice>,
}

impl GuildAdmin {
    pub fn take_notice(&mut self) -> Option<AdminNotice> {
        self.notices.pop_front()
    }

    pub(super) fn notice(&mut self, n: AdminNotice) {
        // Au plus quelques messages en attente : l'interface les lit à chaque image.
        if self.notices.len() >= 8 {
            self.notices.pop_front();
        }
        self.notices.push_back(n);
    }

    pub fn hook_busy(&self, id: u64) -> bool {
        self.hook_busy.contains(&id)
    }

    pub fn removing(&self, id: u64) -> bool {
        self.removing.contains(&id)
    }

    fn fail(&mut self, action: AdminAction, failure: Failure, target: Option<u64>) {
        match action {
            AdminAction::Extra(x) => return self.fail_extra(x, failure),
            AdminAction::Load => self.profile = Some(Load::Failed(failure.clone())),
            AdminAction::Save => self.saving = false,
            AdminAction::Vanity => self.vanity = Some(Load::Failed(failure.clone())),
            AdminAction::Webhooks => {
                for l in self.hooks.values_mut().filter(|l| matches!(l, Load::Loading)) {
                    *l = Load::Failed(failure.clone());
                }
            }
            AdminAction::SaveWebhook | AdminAction::DeleteWebhook => {
                self.hook_busy.remove(&target.unwrap_or(0));
            }
            AdminAction::Integrations => self.integrations = Some(Load::Failed(failure.clone())),
            AdminAction::RemoveIntegration => {
                if let Some(id) = target {
                    self.removing.remove(&id);
                }
            }
            AdminAction::CommandPerms => {
                if let Some(app) = target {
                    self.command_perms.insert(app, Load::Failed(failure.clone()));
                }
            }
        }
        // Les lectures disent leur échec sur leur page ; les écritures l'annoncent.
        if matches!(action, AdminAction::Save | AdminAction::SaveWebhook | AdminAction::DeleteWebhook | AdminAction::RemoveIntegration) {
            self.notice(AdminNotice::Failed { action, failure });
        }
    }
}

/// Permissions qui donnent accès à au moins une page de la fenêtre
/// « Paramètres du serveur » (toutes les pages de la phase 3).
pub const ANY_MANAGE: Permissions = Permissions(
    Permissions::MANAGE_GUILD.0
        | Permissions::MANAGE_ROLES.0
        | Permissions::MANAGE_CHANNELS.0
        | Permissions::MANAGE_WEBHOOKS.0
        | Permissions::MANAGE_GUILD_EXPRESSIONS.0
        | Permissions::MANAGE_EVENTS.0
        | Permissions::CREATE_EVENTS.0
        | Permissions::KICK_MEMBERS.0
        | Permissions::BAN_MEMBERS.0
        | Permissions::MODERATE_MEMBERS.0
        | Permissions::MANAGE_NICKNAMES.0
        | Permissions::VIEW_AUDIT_LOG.0
        | Permissions::VIEW_GUILD_INSIGHTS.0,
);

impl Store {
    /// Mes permissions de serveur ; `None` : inconnues (les pages sont
    /// alors proposées, Discord répondra).
    pub fn guild_perms(&self, guild: u64) -> Option<Permissions> {
        let (me, g) = (self.me.as_ref()?, self.guilds.iter().find(|g| g.id == guild)?);
        base_permissions(g, me.id)
    }

    /// Ai-je `p` sur ce serveur (inconnu : oui) ?
    pub fn may_in_guild(&self, guild: u64, p: Permissions) -> bool {
        self.guilds.iter().any(|g| g.id == guild) && self.guild_perms(guild).is_none_or(|have| have.contains(p))
    }

    /// La fenêtre « Paramètres du serveur » a-t-elle au moins une page pour moi ?
    pub fn can_open_guild_settings(&self, guild: u64) -> bool {
        let Some(g) = self.guilds.iter().find(|g| g.id == guild) else { return false };
        // Droits inconnus : rien n'est proposé (règle commune, `Authority::Unknown`).
        if self.role_authority(guild) == crate::state::perms::Authority::Unknown {
            return false;
        }
        // Gérer les webhooks peut n'être permis que dans un salon.
        self.guild_perms(guild).is_some_and(|have| have.0 & ANY_MANAGE.0 != 0) || g.channels.iter().any(|c| self.can(c.id, Permissions::MANAGE_WEBHOOKS) == crate::state::perms::Verdict::Permis)
    }

    pub(crate) fn admin_cmd(&mut self, cmd: AdminCmd) {
        self.push_command(Cmd::GuildAdmin(cmd));
    }

    /// Fenêtre ouverte sur ce serveur : ce qui concernait un autre est oublié,
    /// le serveur est demandé (une fois par ouverture).
    pub fn open_guild_admin(&mut self, guild_id: u64) {
        if self.guild_admin.open != Some(guild_id) {
            self.forget_guild_admin();
            self.guild_admin.open = Some(guild_id);
        }
        if !matches!(self.guild_admin.profile, Some(Load::Loading | Load::Ready(_))) {
            self.guild_admin.profile = Some(Load::Loading);
            self.admin_cmd(AdminCmd::Load { guild_id });
        }
    }

    /// Fenêtre fermée : rien n'est gardé.
    pub fn forget_guild_admin(&mut self) {
        let notices = std::mem::take(&mut self.guild_admin.notices);
        self.guild_admin = GuildAdmin { notices, ..GuildAdmin::default() };
    }

    /// Lien personnalisé : demandé seulement si le serveur en a la fonction.
    pub fn load_vanity(&mut self, guild_id: u64) -> bool {
        let has = self.guild_admin.profile.as_ref().and_then(Load::ready).is_some_and(|p| p.has_feature("VANITY_URL"));
        if !has || self.guild_admin.vanity.is_some() || self.guild_admin.open != Some(guild_id) {
            return false;
        }
        self.guild_admin.vanity = Some(Load::Loading);
        self.admin_cmd(AdminCmd::Vanity { guild_id });
        true
    }

    /// Enregistre les champs modifiés ; `false` si rien à envoyer, nom
    /// invalide ou enregistrement déjà en cours.
    pub fn save_guild(&mut self, guild_id: u64, mut edit: GuildEdit) -> bool {
        if let Some(n) = &edit.name {
            let n = n.trim();
            if !(crate::state::servers::NAME_MIN..=crate::state::servers::NAME_MAX).contains(&n.chars().count()) {
                return false;
            }
            edit.name = Some(n.to_string());
        }
        if edit.afk_timeout.is_some_and(|t| !AFK_TIMEOUTS.contains(&t)) {
            edit.afk_timeout = None;
        }
        if edit.is_empty() || self.guild_admin.saving || !self.may_in_guild(guild_id, Permissions::MANAGE_GUILD) {
            return false;
        }
        self.guild_admin.saving = true;
        self.admin_cmd(AdminCmd::Modify { guild_id, edit: Box::new(edit) });
        true
    }

    /// Niveau de boost connu d'un serveur (GUILD_CREATE, GUILD_UPDATE).
    pub fn premium_tier(&self, guild: u64) -> u8 {
        self.guilds.iter().find(|g| g.id == guild).map_or(0, |g| g.premium_tier)
    }

    /// Boosters connus (membres vus avec `premium_since`), les plus anciens d'abord.
    pub fn boosters(&self, guild: u64) -> Vec<(u64, i64)> {
        let mut v = self.people.lock().boosters(guild);
        v.sort_by_key(|(id, since)| (*since, *id));
        v
    }

    /// Champs du serveur reçus (GUILD_UPDATE, réponse) : le rail et la
    /// limite d'envoi suivent tout de suite.
    fn guild_patched(&mut self, guild_id: u64, patch: &GuildPatch) {
        if let Some(g) = self.guilds.iter_mut().find(|g| g.id == guild_id) {
            if let Some(n) = &patch.name {
                g.name = n.clone();
            }
            if let Some(t) = patch.premium_tier {
                g.premium_tier = t;
            }
            if let Some(c) = patch.premium_subscription_count {
                g.boosts = c;
            }
            // Bannière changée ou retirée : l'en-tête de la colonne des salons suit (lot V).
            if let Some(b) = &patch.banner {
                g.banner = b.as_deref().and_then(crate::state::image::ImageHash::parse).map(|hash| crate::state::image::ImageRef::Banner { user_id: guild_id, hash });
            }
        }
        if self.guild_admin.open == Some(guild_id)
            && let Some(Load::Ready(p)) = &mut self.guild_admin.profile
        {
            p.merge(patch);
        }
    }

    pub(crate) fn apply_guild_admin(&mut self, e: AdminEvent) {
        let open = self.guild_admin.open;
        match e {
            AdminEvent::Updated { guild_id, patch } => self.guild_patched(guild_id, &patch),
            AdminEvent::Loaded { guild_id, patch, saved } => {
                self.guild_patched(guild_id, &patch);
                if open != Some(guild_id) {
                    return;
                }
                let a = &mut self.guild_admin;
                if saved {
                    a.saving = false;
                    a.notice(AdminNotice::Saved);
                }
                if !matches!(a.profile, Some(Load::Ready(_))) {
                    let mut p = GuildProfile::default();
                    p.merge(&patch);
                    a.profile = Some(Load::Ready(p));
                }
            }
            AdminEvent::Vanity { guild_id, vanity } => {
                if open == Some(guild_id) {
                    self.guild_admin.vanity = Some(Load::Ready(vanity));
                }
            }
            AdminEvent::Failed { guild_id, action, failure, target } => {
                if open == Some(guild_id) {
                    self.guild_admin.fail(action, failure, target);
                }
            }
            e @ (AdminEvent::Webhooks { .. } | AdminEvent::WebhookSaved { .. } | AdminEvent::WebhookDeleted { .. } | AdminEvent::WebhooksChanged { .. }) => self.apply_webhooks(e),
            e @ (AdminEvent::Integrations { .. } | AdminEvent::IntegrationRemoved { .. } | AdminEvent::IntegrationsChanged { .. } | AdminEvent::CommandPerms { .. }) => {
                self.apply_integrations(e)
            }
            AdminEvent::Extra(e) => self.apply_extras(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::{Guild, User};
    use crate::state::Event;

    pub(crate) fn store() -> Store {
        let mut s = Store::new();
        s.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: vec![Guild::new(9, "G".into(), Vec::new())], dms: Vec::new() });
        s.take_commands();
        s
    }

    #[test]
    fn perks_grow_with_the_tier() {
        assert_eq!(perks(0).upload, 10 << 20);
        assert_eq!(perks(1).upload, 10 << 20);
        assert!(perks(1).invite_splash && !perks(1).banner);
        assert_eq!((perks(2).upload, perks(2).emoji_slots), (50 << 20, 150));
        assert_eq!((perks(3).upload, perks(3).bitrate_kbps), (100 << 20, 384));
        assert_eq!(perks(9), perks(3));
        assert!(perks(3).invite_splash && perks(3).banner && perks(3).vanity_url);
    }

    #[test]
    fn open_loads_once_and_save_sends_only_what_changed() {
        let mut s = store();
        s.open_guild_admin(9);
        s.open_guild_admin(9);
        assert_eq!(s.take_commands(), vec![Cmd::GuildAdmin(AdminCmd::Load { guild_id: 9 })]);
        let patch = GuildPatch { name: Some("G2".into()), premium_tier: Some(2), premium_subscription_count: Some(8), afk_timeout: Some(300), ..Default::default() };
        s.apply(Event::GuildAdmin(AdminEvent::Loaded { guild_id: 9, patch, saved: false }));
        assert_eq!(s.guilds[0].name, "G2");
        assert_eq!(s.premium_tier(9), 2);
        assert!(!s.save_guild(9, GuildEdit::default()), "rien à envoyer");
        assert!(!s.save_guild(9, GuildEdit { name: Some(" x ".into()), ..Default::default() }), "nom trop court");
        let edit = GuildEdit { name: Some("  Club  ".into()), afk_timeout: Some(42), ..Default::default() };
        assert!(s.save_guild(9, edit));
        assert!(!s.save_guild(9, GuildEdit { name: Some("Autre".into()), ..Default::default() }), "un enregistrement à la fois");
        let sent = GuildEdit { name: Some("Club".into()), ..Default::default() };
        assert_eq!(s.take_commands(), vec![Cmd::GuildAdmin(AdminCmd::Modify { guild_id: 9, edit: Box::new(sent) })]);
        // La valeur affichée vient de la réponse, pas du brouillon.
        assert_eq!(s.guild_admin.profile.as_ref().and_then(Load::ready).map(|p| p.name.as_str()), Some("G2"));
        s.apply(Event::GuildAdmin(AdminEvent::Loaded { guild_id: 9, patch: GuildPatch { name: Some("Club".into()), ..Default::default() }, saved: true }));
        assert_eq!(s.guild_admin.profile.as_ref().and_then(Load::ready).map(|p| p.name.as_str()), Some("Club"));
        assert_eq!(s.guild_admin.take_notice(), Some(AdminNotice::Saved));
        assert!(!s.guild_admin.saving);
    }

    #[test]
    fn refusal_is_announced_and_other_guilds_are_ignored() {
        let mut s = store();
        s.open_guild_admin(9);
        s.apply(Event::GuildAdmin(AdminEvent::Loaded { guild_id: 9, patch: GuildPatch::default(), saved: false }));
        assert!(s.save_guild(9, GuildEdit { afk_timeout: Some(60), ..Default::default() }));
        s.apply(Event::GuildAdmin(AdminEvent::Failed { guild_id: 9, action: AdminAction::Save, failure: Failure::Problem(Problem::MfaRequired), target: None }));
        assert!(!s.guild_admin.saving);
        assert_eq!(s.guild_admin.take_notice(), Some(AdminNotice::Failed { action: AdminAction::Save, failure: Failure::Problem(Problem::MfaRequired) }));
        // GUILD_UPDATE d'un autre serveur : seul le rail suit.
        s.apply(Event::GuildAdmin(AdminEvent::Updated { guild_id: 77, patch: GuildPatch { afk_timeout: Some(3600), ..Default::default() } }));
        assert_eq!(s.guild_admin.profile.as_ref().and_then(Load::ready).map(|p| p.afk_timeout), Some(0));
        s.apply(Event::GuildAdmin(AdminEvent::Updated { guild_id: 9, patch: GuildPatch { afk_timeout: Some(3600), banner: Some(Some("b".into())), ..Default::default() } }));
        let p = s.guild_admin.profile.as_ref().and_then(Load::ready).cloned().unwrap_or_default();
        assert_eq!((p.afk_timeout, p.banner.as_deref()), (3600, Some("b")));
        s.forget_guild_admin();
        assert!(s.guild_admin.profile.is_none());
    }

    #[test]
    fn vanity_only_with_the_feature() {
        let mut s = store();
        s.open_guild_admin(9);
        s.apply(Event::GuildAdmin(AdminEvent::Loaded { guild_id: 9, patch: GuildPatch::default(), saved: false }));
        s.take_commands();
        assert!(!s.load_vanity(9));
        s.apply(Event::GuildAdmin(AdminEvent::Updated { guild_id: 9, patch: GuildPatch { features: Some(vec!["VANITY_URL".into()]), ..Default::default() } }));
        assert!(s.load_vanity(9));
        assert!(!s.load_vanity(9), "une fois");
        assert_eq!(s.take_commands(), vec![Cmd::GuildAdmin(AdminCmd::Vanity { guild_id: 9 })]);
    }

    #[test]
    fn settings_window_needs_a_manage_permission() {
        let s = store();
        // Droits inconnus : rien n'est proposé (`Authority::Unknown`).
        assert!(!s.can_open_guild_settings(9));
        assert!(!s.can_open_guild_settings(10));
    }
}
