//! Modération (lots A4 et A5) : expulser, bannir, exclure temporairement,
//! bannissements, suppression de messages en masse, journal d'audit,
//! AutoMod, page Sécurité. Chaque commande suit un geste de l'utilisateur
//! (confirmé dans l'interface pour tout ce qui est destructif) ; une seule
//! action de chaque sorte à la fois par cible. Le réseau est dans
//! `net::admin::moderation`.
//!
//! Mémoire bornée : une page à la fois (bannissements, journal, règles,
//! sécurité) ne garde que le serveur affiché ; ouvrir un autre serveur
//! remplace ses données. Les exécutions d'AutoMod reçues sont gardées
//! [`EXECUTIONS_MAX`] au plus (les plus anciennes sont oubliées).

use super::audit::{self, Entry, Filter};
use super::automod::{Execution, Rule};
use crate::state::event::Cmd;
use crate::state::perms::{Authority, Permissions, Verdict};
use crate::state::store::Store;
use crate::state::Problem;
use std::collections::{BTreeSet, HashSet, VecDeque};

/// Champ « Raison » (`X-Audit-Log-Reason`, 512 caractères au plus).
pub const REASON_MAX: usize = 512;
/// Exclusion temporaire : 60 s à 28 jours (Modify Guild Member).
pub const TIMEOUT_MIN_SECS: u64 = 60;
pub const TIMEOUT_MAX_SECS: u64 = 28 * 86_400;
/// Messages supprimés avec un bannissement : 7 jours au plus (Create Guild Ban).
pub const BAN_DELETE_MAX_SECS: u32 = 604_800;
/// Bannissement en masse : 200 utilisateurs au plus (Bulk Guild Ban).
pub const BULK_BAN_MAX: usize = 200;
/// Suppression groupée : 2 à 100 messages de moins de 14 jours (Bulk Delete Messages).
pub const BULK_DELETE_MIN: usize = 2;
pub const BULK_DELETE_MAX: usize = 100;
pub const BULK_DELETE_MAX_AGE_MS: i64 = 14 * 86_400 * 1000;
/// Page de bannissements (les comptes utilisateur doivent donner `limit`, 1 à 1000).
pub const BANS_PAGE: usize = 1000;
/// Pause des invitations et des MP : 24 h au plus (Modify Guild Incident Actions).
pub const INCIDENT_MAX_SECS: u64 = 86_400;
pub const EXECUTIONS_MAX: usize = 50;

/// Début de l'ère de Discord (ms Unix) : un identifiant porte son instant de création.
const DISCORD_EPOCH_MS: i64 = 1_420_070_400_000;

/// Instant de création (ms Unix) d'un identifiant de Discord.
pub fn snowflake_ms(id: u64) -> i64 {
    i64::try_from(id >> 22).unwrap_or(0).saturating_add(DISCORD_EPOCH_MS)
}

/// Raison nettoyée : sans espaces autour, bornée ; vide : aucune.
pub fn reason(text: &str) -> Option<String> {
    let t = text.trim();
    (!t.is_empty()).then(|| t.chars().take(REASON_MAX).collect())
}

/// Réglage de la page Sécurité modifié par l'utilisateur (Modify Guild).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SafetyChange {
    /// `verification_level` (0 à 4).
    Verification(u8),
    /// `explicit_content_filter` (0 à 2).
    ExplicitFilter(u8),
    /// `default_message_notifications` (0 ou 1).
    Notifications(u8),
}

/// Réglages de sécurité d'un serveur (objet Guild et `incidents_data`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Safety {
    pub verification: Option<u8>,
    pub explicit_filter: Option<u8>,
    pub notifications: Option<u8>,
    /// `mfa_level` : 1 = A2F exigée des modérateurs.
    pub mfa: Option<u8>,
    pub invites_disabled_until: Option<i64>,
    pub dms_disabled_until: Option<i64>,
    pub raid_detected_at: Option<i64>,
}

/// Interface -> réseau.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModCmd {
    /// `DELETE /guilds/{g}/members/{u}`.
    Kick { guild_id: u64, user_id: u64, reason: Option<String> },
    /// `PUT /guilds/{g}/bans/{u}`.
    Ban { guild_id: u64, user_id: u64, delete_seconds: u32, reason: Option<String> },
    /// `DELETE /guilds/{g}/bans/{u}`.
    Unban { guild_id: u64, user_id: u64, reason: Option<String> },
    /// `PATCH /guilds/{g}/members/{u}` (`communication_disabled_until`) ; `None` : retirer.
    Timeout { guild_id: u64, user_id: u64, until_ms: Option<i64>, reason: Option<String> },
    /// `GET /guilds/{g}/bans?limit=1000[&after=…]`.
    Bans { guild_id: u64, after: Option<u64> },
    /// `POST /guilds/{g}/bulk-ban`.
    BulkBan { guild_id: u64, user_ids: Vec<u64>, delete_seconds: u32, reason: Option<String> },
    /// Messages choisis d'un salon : `POST …/messages/bulk-delete` pour un
    /// bot (2 à 100 de moins de 14 jours), un par un sinon (la route
    /// groupée n'est pas ouverte aux comptes utilisateur). `now_ms` : heure
    /// de Discord au moment du geste.
    BulkDelete { channel_id: u64, ids: Vec<u64>, now_ms: i64 },
    /// `GET /guilds/{g}/audit-logs`.
    Audit { guild_id: u64, filter: Filter, before: Option<u64> },
    /// `GET /guilds/{g}/auto-moderation/rules`.
    Rules { guild_id: u64 },
    /// `POST …/rules` (`rule.id` 0) ou `PATCH …/rules/{id}`.
    SaveRule { guild_id: u64, rule: Rule },
    /// `DELETE …/rules/{id}`.
    DeleteRule { guild_id: u64, rule_id: u64 },
    /// `GET /guilds/{g}` (réglages de sécurité).
    Safety { guild_id: u64 },
    /// `PATCH /guilds/{g}`.
    SetSafety { guild_id: u64, change: SafetyChange },
    /// `PUT /guilds/{g}/incident-actions` ; `None` : pause levée.
    Incidents { guild_id: u64, invites_until_ms: Option<i64>, dms_until_ms: Option<i64> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModAction {
    Kick,
    Ban,
    Unban,
    Timeout,
    Bans,
    BulkBan,
    BulkDelete,
    Audit,
    Rules,
    SaveRule,
    DeleteRule,
    Safety,
    SetSafety,
    Incidents,
}

impl ModCmd {
    pub fn action(&self) -> ModAction {
        match self {
            ModCmd::Kick { .. } => ModAction::Kick,
            ModCmd::Ban { .. } => ModAction::Ban,
            ModCmd::Unban { .. } => ModAction::Unban,
            ModCmd::Timeout { .. } => ModAction::Timeout,
            ModCmd::Bans { .. } => ModAction::Bans,
            ModCmd::BulkBan { .. } => ModAction::BulkBan,
            ModCmd::BulkDelete { .. } => ModAction::BulkDelete,
            ModCmd::Audit { .. } => ModAction::Audit,
            ModCmd::Rules { .. } => ModAction::Rules,
            ModCmd::SaveRule { .. } => ModAction::SaveRule,
            ModCmd::DeleteRule { .. } => ModAction::DeleteRule,
            ModCmd::Safety { .. } => ModAction::Safety,
            ModCmd::SetSafety { .. } => ModAction::SetSafety,
            ModCmd::Incidents { .. } => ModAction::Incidents,
        }
    }

    /// Cible de l'action (membre, règle, salon ou serveur) : clé de l'action en cours.
    pub fn key(&self) -> u64 {
        match self {
            ModCmd::Kick { user_id, .. } | ModCmd::Ban { user_id, .. } | ModCmd::Unban { user_id, .. } | ModCmd::Timeout { user_id, .. } => *user_id,
            ModCmd::DeleteRule { rule_id, .. } => *rule_id,
            ModCmd::BulkDelete { channel_id, .. } => *channel_id,
            ModCmd::Bans { guild_id, .. }
            | ModCmd::BulkBan { guild_id, .. }
            | ModCmd::Audit { guild_id, .. }
            | ModCmd::Rules { guild_id }
            | ModCmd::SaveRule { guild_id, .. }
            | ModCmd::Safety { guild_id }
            | ModCmd::SetSafety { guild_id, .. }
            | ModCmd::Incidents { guild_id, .. } => *guild_id,
        }
    }

    pub fn guild(&self) -> Option<u64> {
        match self {
            ModCmd::BulkDelete { .. } => None,
            ModCmd::Kick { guild_id, .. }
            | ModCmd::Ban { guild_id, .. }
            | ModCmd::Unban { guild_id, .. }
            | ModCmd::Timeout { guild_id, .. }
            | ModCmd::Bans { guild_id, .. }
            | ModCmd::BulkBan { guild_id, .. }
            | ModCmd::Audit { guild_id, .. }
            | ModCmd::Rules { guild_id }
            | ModCmd::SaveRule { guild_id, .. }
            | ModCmd::DeleteRule { guild_id, .. }
            | ModCmd::Safety { guild_id }
            | ModCmd::SetSafety { guild_id, .. }
            | ModCmd::Incidents { guild_id, .. } => Some(*guild_id),
        }
    }
}

/// Utilisateur banni.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ban {
    pub user_id: u64,
    /// Nom affiché (nom global, sinon nom d'utilisateur).
    pub name: String,
    /// Nom d'utilisateur (`nom` ou `nom#1234`).
    pub handle: String,
    pub reason: Option<String>,
}

/// Réseau -> magasin.
#[derive(Debug)]
pub enum ModEvent {
    Kicked { guild_id: u64, user_id: u64 },
    Banned { guild_id: u64, user_id: u64 },
    Unbanned { guild_id: u64, user_id: u64 },
    TimedOut { guild_id: u64, user_id: u64, until_ms: Option<i64> },
    Bans { guild_id: u64, after: Option<u64>, bans: Vec<Ban> },
    BulkBanned { guild_id: u64, banned: Vec<u64>, failed: Vec<u64> },
    BulkDeleted { channel_id: u64, deleted: usize, failed: usize },
    Audit { guild_id: u64, filter: Filter, before: Option<u64>, entries: Vec<Entry> },
    /// GUILD_AUDIT_LOG_ENTRY_CREATE (s'il est reçu).
    AuditEntry { guild_id: u64, entry: Entry },
    Rules { guild_id: u64, rules: Vec<Rule> },
    /// Règle créée ou modifiée (réponse, ou AUTO_MODERATION_RULE_CREATE/UPDATE).
    RuleSaved { guild_id: u64, rule: Rule, mine: bool },
    RuleDeleted { guild_id: u64, rule_id: u64 },
    /// AUTO_MODERATION_ACTION_EXECUTION (s'il est reçu).
    Execution(Execution),
    /// Réglages de sécurité complets (réponse, ou GUILD_UPDATE).
    Safety { guild_id: u64, safety: Safety, mine: bool },
    /// GUILD_BAN_ADD / GUILD_BAN_REMOVE.
    BanAdd { guild_id: u64, ban: Ban },
    BanRemove { guild_id: u64, user_id: u64 },
    Failed { action: ModAction, guild_id: Option<u64>, key: u64, problem: Problem },
}

/// Message à afficher (mis en mots par l'interface).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModNotice {
    Done(ModAction),
    BulkBanned { banned: usize, failed: usize },
    BulkDeleted { deleted: usize, failed: usize },
    /// L'A2F exigée arrive comme `Problem::MfaRequired` (chemin unique de `net::rest`).
    Failed { action: ModAction, problem: Problem },
}

/// État d'une liste chargée à la demande.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Load<T> {
    Loading,
    /// `more` : une page pleine, il en reste peut-être.
    Ready { items: T, more: bool },
    Failed(Problem),
}

/// Bannissements du serveur affiché.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BanView {
    pub guild_id: u64,
    pub list: Load<Vec<Ban>>,
}

/// Journal d'audit du serveur affiché.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditView {
    pub guild_id: u64,
    pub filter: Filter,
    pub list: Load<Vec<Entry>>,
}

/// Règles AutoMod du serveur affiché.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleView {
    pub guild_id: u64,
    pub list: Load<Vec<Rule>>,
}

/// Réglages de sécurité du serveur affiché.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafetyView {
    pub guild_id: u64,
    pub state: Load<Safety>,
}

/// Messages choisis pour la suppression en masse (un salon à la fois).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub channel_id: u64,
    pub ids: BTreeSet<u64>,
}

/// Droits de modération sur un membre (l'interface n'affiche que ceux-là).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemberRights {
    pub kick: bool,
    pub ban: bool,
    pub timeout: bool,
    /// Exclusion temporaire en cours (ms Unix de fin), si connue.
    pub timed_out_until: Option<i64>,
}

impl MemberRights {
    pub fn any(&self) -> bool {
        self.kick || self.ban || self.timeout
    }
}

#[derive(Default)]
pub struct Moderation {
    pending: HashSet<(ModAction, u64)>,
    notices: VecDeque<ModNotice>,
    pub bans: Option<BanView>,
    pub audit: Option<AuditView>,
    pub rules: Option<RuleView>,
    pub safety: Option<SafetyView>,
    pub executions: VecDeque<Execution>,
    pub selection: Option<Selection>,
    /// Règle enregistrée à l'instant (l'éditeur se ferme).
    saved_rule: Option<u64>,
}

impl Moderation {
    pub fn clear(&mut self) {
        *self = Moderation::default();
    }

    pub fn busy(&self, action: ModAction, key: u64) -> bool {
        self.pending.contains(&(action, key))
    }

    pub fn take_notice(&mut self) -> Option<ModNotice> {
        self.notices.pop_front()
    }

    pub fn take_saved_rule(&mut self) -> Option<u64> {
        self.saved_rule.take()
    }

    pub fn is_selected(&self, channel_id: u64, id: u64) -> bool {
        self.selection.as_ref().is_some_and(|s| s.channel_id == channel_id && s.ids.contains(&id))
    }

    pub fn selecting(&self, channel_id: u64) -> bool {
        self.selection.as_ref().is_some_and(|s| s.channel_id == channel_id)
    }

    /// Ajoute ou retire un message ; `false` si la sélection est pleine.
    pub fn toggle(&mut self, channel_id: u64, id: u64) -> bool {
        let s = self.selection.get_or_insert_with(|| Selection { channel_id, ids: BTreeSet::new() });
        if s.channel_id != channel_id {
            *s = Selection { channel_id, ids: BTreeSet::new() };
        }
        if s.ids.remove(&id) {
            return true;
        }
        if s.ids.len() >= BULK_DELETE_MAX {
            return false;
        }
        s.ids.insert(id);
        true
    }
}

impl Store {
    fn mod_cmd(&mut self, cmd: ModCmd) -> bool {
        if !self.moderation.pending.insert((cmd.action(), cmd.key())) {
            return false;
        }
        self.push_command(Cmd::Moderation(cmd));
        true
    }

    /// Ai-je cette permission dans ce serveur ? Droits inconnus
    /// (`Authority::Unknown`) : non, rien n'est proposé (règle commune).
    pub fn guild_can(&self, guild: u64, p: Permissions) -> bool {
        self.guild_has(guild, p)
    }

    /// Hiérarchie des rôles (`Authority`, lot A3) : puis-je agir sur ce
    /// membre ? Mes droits ou ses rôles inconnus : non.
    pub fn outranks(&self, guild: u64, user: u64) -> bool {
        let Some(me) = self.me.as_ref().map(|u| u.id) else { return false };
        let Some(g) = self.guilds.iter().find(|g| g.id == guild) else { return false };
        if user == me || g.owner_id == Some(user) {
            return false;
        }
        let mine = self.role_authority(guild);
        if mine == Authority::Owner {
            return true;
        }
        self.member_top(guild, user).is_some_and(|top| mine.above(top))
    }

    /// Ce que je peux faire à ce membre de ce serveur.
    pub fn member_rights(&self, guild: u64, user: u64) -> MemberRights {
        if !self.outranks(guild, user) {
            return MemberRights::default();
        }
        let until = self.people.lock().member(guild, user).and_then(|m| m.timeout_until).filter(|t| *t > self.now());
        MemberRights {
            kick: self.guild_can(guild, Permissions::KICK_MEMBERS),
            ban: self.guild_can(guild, Permissions::BAN_MEMBERS),
            timeout: self.guild_can(guild, Permissions::MODERATE_MEMBERS),
            timed_out_until: until,
        }
    }

    pub fn kick_member(&mut self, guild_id: u64, user_id: u64, why: &str) -> bool {
        self.mod_cmd(ModCmd::Kick { guild_id, user_id, reason: reason(why) })
    }

    pub fn ban_member(&mut self, guild_id: u64, user_id: u64, delete_seconds: u32, why: &str) -> bool {
        self.mod_cmd(ModCmd::Ban { guild_id, user_id, delete_seconds: delete_seconds.min(BAN_DELETE_MAX_SECS), reason: reason(why) })
    }

    pub fn unban(&mut self, guild_id: u64, user_id: u64, why: &str) -> bool {
        self.mod_cmd(ModCmd::Unban { guild_id, user_id, reason: reason(why) })
    }

    /// Exclut pour `secs` (borné à 60 s – 28 j), ou lève l'exclusion (`None`).
    pub fn timeout_member(&mut self, guild_id: u64, user_id: u64, secs: Option<u64>, why: &str) -> bool {
        let now = self.now();
        let until_ms = secs.map(|s| now.saturating_add(i64::try_from(s.clamp(TIMEOUT_MIN_SECS, TIMEOUT_MAX_SECS) * 1000).unwrap_or(i64::MAX)));
        self.mod_cmd(ModCmd::Timeout { guild_id, user_id, until_ms, reason: reason(why) })
    }

    /// Bannissements : première page (liste remplacée) ou page suivante.
    pub fn load_bans(&mut self, guild_id: u64, more: bool) -> bool {
        let after = match (&self.moderation.bans, more) {
            (Some(BanView { guild_id: g, list: Load::Ready { items, more: true } }), true) if *g == guild_id => items.iter().map(|b| b.user_id).max(),
            (_, true) => return false,
            _ => None,
        };
        if !self.mod_cmd(ModCmd::Bans { guild_id, after }) {
            return false;
        }
        if after.is_none() {
            self.moderation.bans = Some(BanView { guild_id, list: Load::Loading });
        }
        true
    }

    /// Bannissement en masse d'une sélection explicite (200 au plus).
    pub fn bulk_ban(&mut self, guild_id: u64, user_ids: &[u64], delete_seconds: u32, why: &str) -> bool {
        let mut ids: Vec<u64> = user_ids.iter().copied().collect::<BTreeSet<_>>().into_iter().collect();
        if ids.is_empty() || ids.len() > BULK_BAN_MAX {
            return false;
        }
        ids.retain(|u| self.me.as_ref().is_none_or(|m| m.id != *u));
        self.mod_cmd(ModCmd::BulkBan { guild_id, user_ids: ids, delete_seconds: delete_seconds.min(BAN_DELETE_MAX_SECS), reason: reason(why) })
    }

    /// Puis-je supprimer les messages des autres dans ce salon ?
    pub fn can_bulk_delete(&self, channel_id: u64) -> bool {
        self.guild_of(channel_id).is_some() && self.can(channel_id, Permissions::MANAGE_MESSAGES) == Verdict::Permis
    }

    /// Messages de la sélection qu'une suppression groupée ne prend pas (plus de 14 jours).
    pub fn too_old(&self, ids: &BTreeSet<u64>) -> usize {
        let now = self.now();
        ids.iter().filter(|id| now - snowflake_ms(**id) >= BULK_DELETE_MAX_AGE_MS).count()
    }

    /// Supprime les messages choisis (après confirmation) ; la sélection se vide.
    pub fn delete_selection(&mut self) -> bool {
        let Some(sel) = self.moderation.selection.take() else { return false };
        if sel.ids.is_empty() || !self.can_bulk_delete(sel.channel_id) {
            return false;
        }
        let now_ms = self.now();
        self.mod_cmd(ModCmd::BulkDelete { channel_id: sel.channel_id, ids: sel.ids.into_iter().collect(), now_ms })
    }

    /// Journal d'audit : première page avec ces filtres, ou page plus ancienne.
    pub fn load_audit(&mut self, guild_id: u64, filter: Filter, more: bool) -> bool {
        let before = match (&self.moderation.audit, more) {
            (Some(AuditView { guild_id: g, filter: f, list: Load::Ready { items, more: true } }), true) if *g == guild_id && *f == filter => items.last().map(|e| e.id),
            (_, true) => return false,
            _ => None,
        };
        if !self.mod_cmd(ModCmd::Audit { guild_id, filter, before }) {
            return false;
        }
        if before.is_none() {
            self.moderation.audit = Some(AuditView { guild_id, filter, list: Load::Loading });
        }
        true
    }

    pub fn load_rules(&mut self, guild_id: u64) -> bool {
        if !self.mod_cmd(ModCmd::Rules { guild_id }) {
            return false;
        }
        self.moderation.rules = Some(RuleView { guild_id, list: Load::Loading });
        true
    }

    /// Enregistre une règle valable (sinon rien ne part).
    pub fn save_rule(&mut self, guild_id: u64, rule: Rule) -> bool {
        rule.check().is_ok() && self.mod_cmd(ModCmd::SaveRule { guild_id, rule })
    }

    pub fn delete_rule(&mut self, guild_id: u64, rule_id: u64) -> bool {
        self.mod_cmd(ModCmd::DeleteRule { guild_id, rule_id })
    }

    pub fn load_safety(&mut self, guild_id: u64) -> bool {
        if !self.mod_cmd(ModCmd::Safety { guild_id }) {
            return false;
        }
        self.moderation.safety = Some(SafetyView { guild_id, state: Load::Loading });
        true
    }

    pub fn set_safety(&mut self, guild_id: u64, change: SafetyChange) -> bool {
        let ok = match change {
            SafetyChange::Verification(v) => v <= 4,
            SafetyChange::ExplicitFilter(v) => v <= 2,
            SafetyChange::Notifications(v) => v <= 1,
        };
        ok && self.mod_cmd(ModCmd::SetSafety { guild_id, change })
    }

    /// Pause des invitations et des MP jusqu'à ces instants (24 h au plus,
    /// passés ou `None` : pas de pause). Les deux partent toujours ensemble :
    /// garder l'autre tel qu'il est revient à le renvoyer.
    pub fn set_incidents(&mut self, guild_id: u64, invites_until_ms: Option<i64>, dms_until_ms: Option<i64>) -> bool {
        let now = self.now();
        let max = now.saturating_add(i64::try_from(INCIDENT_MAX_SECS * 1000).unwrap_or(0));
        let at = |t: Option<i64>| t.filter(|t| *t > now).map(|t| t.min(max));
        self.mod_cmd(ModCmd::Incidents { guild_id, invites_until_ms: at(invites_until_ms), dms_until_ms: at(dms_until_ms) })
    }

    pub(crate) fn apply_moderation(&mut self, e: ModEvent) {
        let m = &mut self.moderation;
        let done = |m: &mut Moderation, a: ModAction, key: u64| {
            m.pending.remove(&(a, key));
        };
        match e {
            ModEvent::Kicked { user_id, .. } => {
                done(m, ModAction::Kick, user_id);
                m.notices.push_back(ModNotice::Done(ModAction::Kick));
            }
            ModEvent::Banned { user_id, .. } => {
                done(m, ModAction::Ban, user_id);
                m.notices.push_back(ModNotice::Done(ModAction::Ban));
            }
            ModEvent::Unbanned { guild_id, user_id } => {
                done(m, ModAction::Unban, user_id);
                m.notices.push_back(ModNotice::Done(ModAction::Unban));
                remove_ban(m, guild_id, user_id);
            }
            ModEvent::TimedOut { guild_id, user_id, until_ms } => {
                done(m, ModAction::Timeout, user_id);
                m.notices.push_back(ModNotice::Done(ModAction::Timeout));
                // Réponse de Discord (le membre modifié) : vaut ce que dira GUILD_MEMBER_UPDATE.
                let mut people = self.people.lock();
                if let Some(mut info) = people.member(guild_id, user_id).cloned() {
                    info.timeout_until = until_ms;
                    people.upsert_member(guild_id, user_id, info);
                }
            }
            ModEvent::Bans { guild_id, after, bans } => {
                done(m, ModAction::Bans, guild_id);
                let more = bans.len() >= BANS_PAGE;
                match &mut m.bans {
                    Some(BanView { guild_id: g, list: Load::Ready { items, more: old } }) if *g == guild_id && after.is_some() => {
                        items.extend(bans);
                        *old = more;
                    }
                    _ if after.is_none() => m.bans = Some(BanView { guild_id, list: Load::Ready { items: bans, more } }),
                    _ => {}
                }
            }
            ModEvent::BulkBanned { guild_id, banned, failed } => {
                done(m, ModAction::BulkBan, guild_id);
                m.notices.push_back(ModNotice::BulkBanned { banned: banned.len(), failed: failed.len() });
            }
            ModEvent::BulkDeleted { channel_id, deleted, failed } => {
                done(m, ModAction::BulkDelete, channel_id);
                m.notices.push_back(ModNotice::BulkDeleted { deleted, failed });
            }
            ModEvent::Audit { guild_id, filter, before, entries } => {
                done(m, ModAction::Audit, guild_id);
                let more = entries.len() >= audit::PAGE;
                if let Some(AuditView { guild_id: g, filter: f, list }) = &mut m.audit
                    && *g == guild_id
                    && *f == filter
                {
                    match (list, before) {
                        (Load::Ready { items, more: old }, Some(_)) => {
                            items.extend(entries);
                            // Au-delà, plus rien n'est chargé : la mémoire reste bornée.
                            items.truncate(audit::MAX_ENTRIES);
                            *old = more && items.len() < audit::MAX_ENTRIES;
                        }
                        (list, None) => *list = Load::Ready { items: entries, more },
                        _ => {}
                    }
                }
            }
            ModEvent::AuditEntry { guild_id, entry } => {
                if let Some(AuditView { guild_id: g, filter, list: Load::Ready { items, .. } }) = &mut m.audit
                    && *g == guild_id
                    && filter.user_id.is_none_or(|u| entry.user_id == Some(u))
                    && filter.action.is_none_or(|a| a == entry.action)
                    && !items.iter().any(|e| e.id == entry.id)
                {
                    items.insert(0, entry);
                    items.truncate(audit::MAX_ENTRIES);
                }
            }
            ModEvent::Rules { guild_id, rules } => {
                done(m, ModAction::Rules, guild_id);
                if m.rules.as_ref().is_some_and(|r| r.guild_id == guild_id) {
                    m.rules = Some(RuleView { guild_id, list: Load::Ready { items: rules, more: false } });
                }
            }
            ModEvent::RuleSaved { guild_id, rule, mine } => {
                if mine {
                    done(m, ModAction::SaveRule, guild_id);
                    m.saved_rule = Some(rule.id);
                    m.notices.push_back(ModNotice::Done(ModAction::SaveRule));
                }
                if let Some(RuleView { guild_id: g, list: Load::Ready { items, .. } }) = &mut m.rules
                    && *g == guild_id
                {
                    match items.iter_mut().find(|r| r.id == rule.id) {
                        Some(r) => *r = rule,
                        None => items.push(rule),
                    }
                }
            }
            ModEvent::RuleDeleted { guild_id, rule_id } => {
                if m.pending.remove(&(ModAction::DeleteRule, rule_id)) {
                    m.notices.push_back(ModNotice::Done(ModAction::DeleteRule));
                }
                if let Some(RuleView { guild_id: g, list: Load::Ready { items, .. } }) = &mut m.rules
                    && *g == guild_id
                {
                    items.retain(|r| r.id != rule_id);
                }
            }
            ModEvent::Execution(x) => {
                if m.executions.len() >= EXECUTIONS_MAX {
                    m.executions.pop_back();
                }
                m.executions.push_front(x);
            }
            ModEvent::Safety { guild_id, safety, mine } => {
                if mine {
                    // Réponse à une lecture, une écriture ou une pause.
                    for a in [ModAction::Safety, ModAction::SetSafety, ModAction::Incidents] {
                        if m.pending.remove(&(a, guild_id)) && a != ModAction::Safety {
                            m.notices.push_back(ModNotice::Done(a));
                        }
                    }
                }
                if let Some(v) = &mut m.safety
                    && v.guild_id == guild_id
                    && (mine || matches!(v.state, Load::Ready { .. }))
                {
                    v.state = Load::Ready { items: safety, more: false };
                }
            }
            ModEvent::BanAdd { guild_id, ban } => {
                if let Some(BanView { guild_id: g, list: Load::Ready { items, .. } }) = &mut m.bans
                    && *g == guild_id
                    && !items.iter().any(|b| b.user_id == ban.user_id)
                {
                    items.push(ban);
                }
            }
            ModEvent::BanRemove { guild_id, user_id } => remove_ban(m, guild_id, user_id),
            ModEvent::Failed { action, guild_id: _, key, problem } => {
                m.pending.remove(&(action, key));
                match action {
                    ModAction::Bans => {
                        if let Some(v) = m.bans.as_mut().filter(|v| v.guild_id == key) {
                            fail(&mut v.list, &problem);
                        }
                    }
                    ModAction::Audit => {
                        if let Some(v) = m.audit.as_mut().filter(|v| v.guild_id == key) {
                            fail(&mut v.list, &problem);
                        }
                    }
                    ModAction::Rules => {
                        if let Some(v) = m.rules.as_mut().filter(|v| v.guild_id == key) {
                            fail(&mut v.list, &problem);
                        }
                    }
                    ModAction::Safety => {
                        if let Some(v) = m.safety.as_mut().filter(|v| v.guild_id == key) {
                            fail(&mut v.state, &problem);
                        }
                    }
                    _ => {}
                }
                m.notices.push_back(ModNotice::Failed { action, problem });
            }
        }
    }
}

/// Une liste en chargement montre l'échec à la place.
fn fail<T>(list: &mut Load<T>, problem: &Problem) {
    if matches!(list, Load::Loading) {
        *list = Load::Failed(problem.clone());
    }
}

fn remove_ban(m: &mut Moderation, guild_id: u64, user_id: u64) {
    if let Some(BanView { guild_id: g, list: Load::Ready { items, .. } }) = &mut m.bans
        && *g == guild_id
    {
        items.retain(|b| b.user_id != user_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::event::Event;
    use crate::state::model::{Guild, Member, Role, User};
    use crate::state::people::MemberInfo;
    use crate::state::Permissions as P;

    const G: u64 = 10;
    const ME: u64 = 1;
    const BOB: u64 = 2;
    const BOSS: u64 = 3;

    fn role(id: u64, position: i32, permissions: P) -> Role {
        Role { id, name: format!("r{id}"), color: 0, position, permissions, mentionable: false }
    }

    fn store(my_perms: P) -> Store {
        let mut s = Store::new();
        let mut g = Guild::new(G, "Club".into(), Vec::new());
        g.owner_id = Some(99);
        g.roles = vec![role(G, 0, P::VIEW_CHANNEL), role(20, 5, my_perms), role(30, 9, P::NONE), role(40, 1, P::NONE)];
        g.me = Some(Member { roles: vec![20], nick: None, timeout_until: None });
        s.apply(Event::Ready { me: User::new(ME, "moi".into()), guilds: vec![g], dms: Vec::new() });
        let mut p = s.people.lock();
        p.upsert_member(G, BOB, MemberInfo { roles: Some(vec![40]), ..Default::default() });
        p.upsert_member(G, BOSS, MemberInfo { roles: Some(vec![30]), ..Default::default() });
        drop(p);
        s
    }

    fn cmds(s: &mut Store) -> Vec<ModCmd> {
        s.take_commands().into_iter().filter_map(|c| if let Cmd::Moderation(m) = c { Some(m) } else { None }).collect()
    }

    #[test]
    fn rights_follow_permissions_and_role_hierarchy() {
        let s = store(P::KICK_MEMBERS | P::MODERATE_MEMBERS);
        let r = s.member_rights(G, BOB);
        assert!(r.kick && r.timeout && !r.ban);
        // Rôle plus haut que le mien, propriétaire, moi : rien.
        assert!(!s.member_rights(G, BOSS).any());
        assert!(!s.member_rights(G, 99).any());
        assert!(!s.member_rights(G, ME).any());
        // Membre dont les rôles sont inconnus : rien n'est proposé (comme `Authority::Unknown`).
        assert!(!s.member_rights(G, 77).any());
    }

    #[test]
    fn moderation_commands_carry_reason_and_bounds() {
        let mut s = store(P::ALL);
        assert!(s.kick_member(G, BOB, "  spam  "));
        assert!(!s.kick_member(G, BOB, ""), "déjà en cours");
        assert!(s.ban_member(G, 77, 10_000_000, ""));
        assert!(s.timeout_member(G, 78, Some(1), "x"));
        let c = cmds(&mut s);
        assert_eq!(c[0], ModCmd::Kick { guild_id: G, user_id: BOB, reason: Some("spam".into()) });
        assert_eq!(c[1], ModCmd::Ban { guild_id: G, user_id: 77, delete_seconds: BAN_DELETE_MAX_SECS, reason: None });
        let ModCmd::Timeout { until_ms: Some(until), .. } = c[2] else { panic!("{c:?}") };
        assert!(until - s.now() <= 60_000 && until - s.now() > 55_000, "60 s au moins");
        s.apply(Event::Moderation(ModEvent::Kicked { guild_id: G, user_id: BOB }));
        assert_eq!(s.moderation.take_notice(), Some(ModNotice::Done(ModAction::Kick)));
        assert!(!s.moderation.busy(ModAction::Kick, BOB));
        assert_eq!(reason(&"é".repeat(600)).map(|r| r.chars().count()), Some(REASON_MAX));
    }

    #[test]
    fn bans_pages_and_gateway_updates() {
        let mut s = store(P::ALL);
        let ban = |id| Ban { user_id: id, name: format!("u{id}"), handle: format!("u{id}"), reason: None };
        assert!(s.load_bans(G, false));
        assert!(!s.load_bans(G, true), "rien à suivre avant la première page");
        s.apply(Event::Moderation(ModEvent::Bans { guild_id: G, after: None, bans: (1..=BANS_PAGE as u64).map(ban).collect() }));
        assert!(s.load_bans(G, true));
        assert_eq!(cmds(&mut s).last(), Some(&ModCmd::Bans { guild_id: G, after: Some(BANS_PAGE as u64) }));
        s.apply(Event::Moderation(ModEvent::Bans { guild_id: G, after: Some(BANS_PAGE as u64), bans: vec![ban(5000)] }));
        s.apply(Event::Moderation(ModEvent::BanRemove { guild_id: G, user_id: 1 }));
        s.apply(Event::Moderation(ModEvent::BanAdd { guild_id: G, ban: ban(6000) }));
        let Some(BanView { list: Load::Ready { items, more }, .. }) = &s.moderation.bans else { panic!() };
        assert_eq!((items.len(), *more), (BANS_PAGE + 1, false));
        assert!(!items.iter().any(|b| b.user_id == 1));
    }

    #[test]
    fn selection_is_bounded_and_needs_manage_messages() {
        let mut s = store(P::ALL);
        for i in 0..BULK_DELETE_MAX as u64 {
            assert!(s.moderation.toggle(500, 1000 + i));
        }
        assert!(!s.moderation.toggle(500, 5), "100 au plus");
        assert!(s.moderation.toggle(500, 1000), "retirer marche toujours");
        assert!(s.moderation.is_selected(500, 1001));
        // Salon inconnu (pas de serveur) : rien ne part.
        assert!(!s.delete_selection());
        assert!(s.moderation.selection.is_none());
        // Ancienneté lue dans l'identifiant.
        let old: BTreeSet<u64> = [1u64 << 22].into();
        assert_eq!(s.too_old(&old), 1);
        let fresh: BTreeSet<u64> = [u64::try_from(s.now() - DISCORD_EPOCH_MS).unwrap_or(0) << 22].into();
        assert_eq!(s.too_old(&fresh), 0);
    }

    #[test]
    fn two_factor_refusal_is_a_problem_of_its_own() {
        let mut s = store(P::ALL);
        assert!(s.load_safety(G));
        s.apply(Event::Moderation(ModEvent::Safety { guild_id: G, safety: Safety { mfa: Some(1), ..Default::default() }, mine: true }));
        assert!(s.ban_member(G, BOB, 0, ""));
        // Chemin unique (`net::rest`) : 60003 arrive en `Problem::MfaRequired`, rien n'est déduit ici.
        s.apply(Event::Moderation(ModEvent::Failed { action: ModAction::Ban, guild_id: Some(G), key: BOB, problem: Problem::MfaRequired }));
        assert_eq!(s.moderation.take_notice(), Some(ModNotice::Failed { action: ModAction::Ban, problem: Problem::MfaRequired }));
        assert!(!s.set_safety(G, SafetyChange::Verification(9)));
        assert!(s.set_safety(G, SafetyChange::Verification(2)));
    }

    #[test]
    fn audit_pages_filters_and_live_entries() {
        let mut s = store(P::ALL);
        let entry = |id, action| Entry { id, action, user_id: Some(BOB), user_name: None, target_id: None, target_name: None, reason: None, changes: Vec::new(), count: None, channel_id: None, extra: None };
        let f = Filter { user_id: None, action: Some(22) };
        assert!(s.load_audit(G, f, false));
        s.apply(Event::Moderation(ModEvent::Audit { guild_id: G, filter: f, before: None, entries: (0..audit::PAGE as u64).rev().map(|i| entry(100 + i, 22)).collect() }));
        assert!(s.load_audit(G, f, true));
        assert_eq!(cmds(&mut s).last(), Some(&ModCmd::Audit { guild_id: G, filter: f, before: Some(100) }));
        // Une autre action ne passe pas le filtre.
        s.apply(Event::Moderation(ModEvent::AuditEntry { guild_id: G, entry: entry(900, 20) }));
        s.apply(Event::Moderation(ModEvent::AuditEntry { guild_id: G, entry: entry(901, 22) }));
        let Some(AuditView { list: Load::Ready { items, .. }, .. }) = &s.moderation.audit else { panic!() };
        assert_eq!(items[0].id, 901);
        assert_eq!(items.len(), audit::PAGE + 1);
    }
}
