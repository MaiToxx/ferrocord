//! Rôles et membres d'un serveur (lot A3) : créer, dupliquer, modifier,
//! supprimer et réordonner les rôles, donner ou retirer un rôle à un
//! membre, changer le pseudo d'un autre membre, chercher des membres.
//!
//! Chaque écriture suit un geste ; l'état affiché vient des réponses de
//! Discord (objets Role, Guild Member) et des événements de la passerelle,
//! jamais d'une supposition. La hiérarchie (`perms::Authority`) est vérifiée
//! avant tout envoi. Le réseau est dans `net::admin::roles`.
//!
//! Les champs de l'objet Role que `model::Role` ne garde pas (affichage
//! séparé, icône, emoji, dégradé, rôle géré) sont tenus ici, par
//! (serveur, rôle) : mis à jour par READY, GUILD_CREATE et GUILD_ROLE_*,
//! retirés par GUILD_ROLE_DELETE et GUILD_DELETE, vidés à la déconnexion.

use crate::state::model::{ImageHash, Role};
use crate::state::perms::{authority, role_rank, top_rank, Authority, Permissions, Rank};
use crate::state::store::{Effect, Store};
use crate::state::{Cmd, Event, Problem};
use std::collections::{HashMap, HashSet, VecDeque};

/// Nom d'un rôle : 1 à 100 caractères (documentation : Role Object).
pub const NAME_MAX: usize = 100;
/// Pseudo : 32 caractères au plus (Modify Guild Member).
pub const NICK_MAX: usize = 32;
/// Résultats demandés à la recherche de membres (la documentation borne à 1 000).
pub const SEARCH_LIMIT: u32 = 100;
pub const QUERY_MAX: usize = 100;

/// Couleurs proposées par Discord pour un rôle (valeurs de rôle, pas des
/// couleurs d'interface : elles ne dépendent pas du thème).
pub const PALETTE: [u32; 20] = [
    0x1abc9c, 0x2ecc71, 0x3498db, 0x9b59b6, 0xe91e63, 0xf1c40f, 0xe67e22, 0xe74c3c, 0x95a5a6, 0x607d8b, //
    0x11806a, 0x1f8b4c, 0x206694, 0x71368a, 0xad1457, 0xc27c0e, 0xa84300, 0x992d22, 0x979c9f, 0x546e7a,
];

/// Étiquettes d'un rôle géré (objet Role Tags).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoleTags {
    /// Rôle d'un bot.
    pub bot_id: Option<u64>,
    /// Rôle d'une intégration.
    pub integration_id: Option<u64>,
    /// Rôle des boosters du serveur.
    pub premium_subscriber: bool,
    /// Rôle d'abonnement (offre du serveur).
    pub subscription: bool,
    /// Rôle lié à un compte connecté.
    pub guild_connections: bool,
}

/// Ce que `model::Role` ne garde pas de l'objet Role.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoleExtra {
    /// Affiché séparément dans la liste des membres.
    pub hoist: bool,
    pub icon: Option<ImageHash>,
    pub unicode_emoji: Option<String>,
    /// Géré par une intégration : ni supprimé ni donné à la main.
    pub managed: bool,
    /// Dégradé (`colors`) : deuxième et troisième couleurs, en lecture seule.
    pub secondary_color: Option<u32>,
    pub tertiary_color: Option<u32>,
    pub tags: RoleTags,
    pub flags: u64,
}

/// Ce que le niveau de boost ouvre pour les rôles (le niveau : `Guild::premium_tier`).
pub use super::Boost;

/// Champs d'un rôle à écrire (Create / Modify Guild Role) ; `None` : inchangé.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoleFields {
    pub name: Option<String>,
    pub permissions: Option<Permissions>,
    pub color: Option<u32>,
    pub hoist: Option<bool>,
    pub mentionable: Option<bool>,
    /// Image en `data:` ; `Some(None)` : retirée.
    pub icon: Option<Option<String>>,
    /// `Some(None)` : retiré.
    pub unicode_emoji: Option<Option<String>>,
}

impl RoleFields {
    pub fn is_empty(&self) -> bool {
        *self == RoleFields::default()
    }
}

/// Interface -> réseau.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RolesCmd {
    /// `POST /guilds/{id}/roles`.
    Create { guild_id: u64, fields: RoleFields },
    /// `PATCH /guilds/{id}/roles/{role}`.
    Edit { guild_id: u64, role_id: u64, fields: RoleFields },
    /// `DELETE /guilds/{id}/roles/{role}`.
    Delete { guild_id: u64, role_id: u64 },
    /// `PATCH /guilds/{id}/roles` : (rôle, position).
    Reorder { guild_id: u64, positions: Vec<(u64, i32)> },
    /// `PUT` (ou `DELETE` si `!add`) `/guilds/{id}/members/{user}/roles/{role}`,
    /// puis `GET /guilds/{id}/members/{user}` pour relire ses rôles.
    MemberRole { guild_id: u64, user_id: u64, role_id: u64, add: bool },
    /// `PATCH /guilds/{id}/members/{user}` (`nick` ; vide : retiré).
    Nick { guild_id: u64, user_id: u64, nick: String },
    /// `GET /guilds/{id}/members/search`.
    Search { guild_id: u64, query: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RoleAction {
    Create,
    Edit,
    Delete,
    Reorder,
    MemberRole,
    Nick,
    Search,
}

/// Réseau -> magasin.
#[derive(Debug)]
pub enum RolesEvent {
    /// Champs supplémentaires des rôles ; `replace` : liste complète du
    /// serveur (READY, GUILD_CREATE).
    Extras { guild_id: u64, roles: Vec<(u64, RoleExtra)>, replace: bool },
    Boost { guild_id: u64, boost: Boost },
    /// Rôle supprimé (`Some`) ou serveur quitté (`None`).
    Forget { guild_id: u64, role_id: Option<u64> },
    /// Rôles renvoyés par Discord après une création, une modification ou un
    /// changement d'ordre.
    Saved { action: RoleAction, guild_id: u64, roles: Vec<(Role, RoleExtra)> },
    Deleted { guild_id: u64, role_id: u64 },
    /// Rôle donné ou retiré, ou pseudo changé : le membre relu est déjà dans
    /// les tables des membres (`people`).
    MemberDone { action: RoleAction, guild_id: u64, user_id: u64, role_id: Option<u64> },
    Found { guild_id: u64, query: String, users: Vec<u64> },
    Failed { action: RoleAction, guild_id: u64, user_id: Option<u64>, role_id: Option<u64>, problem: Problem },
}

/// Message à afficher (mis en mots par l'interface).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RolesNotice {
    Saved(RoleAction),
    Failed { action: RoleAction, problem: Problem },
}

/// Recherche de membres d'un serveur.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Search {
    pub query: String,
    pub loading: bool,
    pub users: Vec<u64>,
}

#[derive(Default)]
pub struct RolesAdmin {
    extras: HashMap<(u64, u64), RoleExtra>,
    boost: HashMap<u64, Boost>,
    /// Une action de chaque sorte à la fois par serveur.
    pending: HashSet<(RoleAction, u64)>,
    /// (serveur, membre, rôle) en cours.
    member_roles: HashSet<(u64, u64, u64)>,
    /// (serveur, membre) dont le pseudo est en cours.
    nicks: HashSet<(u64, u64)>,
    search: HashMap<u64, Search>,
    notices: VecDeque<RolesNotice>,
    /// Rôle créé à l'instant, à ouvrir dans l'éditeur.
    created: Option<(u64, u64)>,
    /// Change à chaque réponse : les pages refont ce qu'elles gardent.
    rev: u64,
}

impl RolesAdmin {
    pub fn clear(&mut self) {
        *self = RolesAdmin::default();
    }

    pub fn extra(&self, guild_id: u64, role_id: u64) -> Option<&RoleExtra> {
        self.extras.get(&(guild_id, role_id))
    }

    pub fn boost(&self, guild_id: u64) -> Boost {
        self.boost.get(&guild_id).copied().unwrap_or_default()
    }

    pub fn pending(&self, action: RoleAction, guild_id: u64) -> bool {
        self.pending.contains(&(action, guild_id))
    }

    pub fn member_role_pending(&self, guild_id: u64, user_id: u64, role_id: u64) -> bool {
        self.member_roles.contains(&(guild_id, user_id, role_id))
    }

    pub fn nick_pending(&self, guild_id: u64, user_id: u64) -> bool {
        self.nicks.contains(&(guild_id, user_id))
    }

    pub fn search(&self, guild_id: u64) -> Option<&Search> {
        self.search.get(&guild_id)
    }

    pub fn take_notice(&mut self) -> Option<RolesNotice> {
        self.notices.pop_front()
    }

    pub fn take_created(&mut self) -> Option<(u64, u64)> {
        self.created.take()
    }

    pub fn rev(&self) -> u64 {
        self.rev
    }

    fn bump(&mut self) {
        self.rev = self.rev.wrapping_add(1);
    }
}

/// Rôles du serveur dans l'ordre de Discord (le plus haut d'abord, @everyone en dernier).
pub fn sorted(roles: &[Role]) -> Vec<&Role> {
    let mut v: Vec<&Role> = roles.iter().collect();
    v.sort_by_key(|r| std::cmp::Reverse(role_rank(r)));
    v
}

/// Nouvelles positions quand `moved` est déposé au rang `to` de la liste
/// affichée (sans @everyone). Seuls les rôles qu'on peut modifier bougent,
/// et seulement entre eux : ils forment le bas de la liste, et reprennent
/// leurs positions actuelles dans le nouvel ordre (1, 2… si elles ne sont
/// pas toutes distinctes). `None` : rien ne change ou c'est interdit.
pub fn reorder(roles: &[Role], guild_id: u64, auth: Authority, moved: u64, to: usize) -> Option<Vec<(u64, i32)>> {
    let mut list: Vec<&Role> = sorted(roles).into_iter().filter(|r| r.id != guild_id).collect();
    let from = list.iter().position(|r| r.id == moved)?;
    let first = list.iter().position(|r| auth.can_edit_role(r)).unwrap_or(list.len());
    if !list[first..].iter().all(|r| auth.can_edit_role(r)) || from < first || to < first || to >= list.len() || to == from {
        return None;
    }
    let r = list.remove(from);
    list.insert(to, r);
    let block = &list[first..];
    let mut slots: Vec<i32> = block.iter().map(|r| r.position).collect();
    slots.sort_unstable();
    if slots.windows(2).any(|w| w[0] == w[1]) || slots.first().is_some_and(|p| *p < 1) {
        slots = (1..=block.len() as i32).collect();
    }
    let n = block.len();
    let out: Vec<(u64, i32)> = block.iter().enumerate().filter_map(|(i, r)| (slots[n - 1 - i] != r.position).then_some((r.id, slots[n - 1 - i]))).collect();
    (!out.is_empty()).then_some(out)
}

fn clean_name(name: &str) -> Option<String> {
    let n = name.trim();
    (1..=NAME_MAX).contains(&n.chars().count()).then(|| n.to_string())
}

impl Store {
    pub fn role_authority(&self, guild_id: u64) -> Authority {
        match (self.me.as_ref(), self.guilds.iter().find(|g| g.id == guild_id)) {
            (Some(me), Some(g)) => authority(g, me.id),
            _ => Authority::Unknown,
        }
    }

    fn guild_role(&self, guild_id: u64, role_id: u64) -> Option<&Role> {
        self.guilds.iter().find(|g| g.id == guild_id)?.roles.iter().find(|r| r.id == role_id)
    }

    /// « Gérer les rôles » : page Rôles, rôles d'un membre.
    pub fn can_manage_roles(&self, guild_id: u64) -> bool {
        self.me.is_some() && self.guilds.iter().any(|g| g.id == guild_id) && self.role_authority(guild_id).has(Permissions::MANAGE_ROLES)
    }

    /// Une permission de base dans ce serveur (inconnue : proposée, Discord répondra).
    pub fn guild_has(&self, guild_id: u64, p: Permissions) -> bool {
        self.me.is_some() && self.guilds.iter().any(|g| g.id == guild_id) && self.role_authority(guild_id).has(p)
    }

    /// Rang du rôle le plus élevé d'un membre, si ses rôles sont connus.
    pub fn member_top(&self, guild_id: u64, user_id: u64) -> Option<Rank> {
        let g = self.guilds.iter().find(|g| g.id == guild_id)?;
        if self.me.as_ref().is_some_and(|u| u.id == user_id) {
            return g.me.as_ref().map(|m| top_rank(g, &m.roles));
        }
        let roles = self.people.lock().member(guild_id, user_id)?.roles.clone()?;
        Some(top_rank(g, &roles))
    }

    /// Ce rôle peut être donné ou retiré à la main par moi.
    pub fn can_assign(&self, guild_id: u64, role: &Role) -> bool {
        role.id != guild_id && !self.roles_admin.extra(guild_id, role.id).is_some_and(|x| x.managed) && self.role_authority(guild_id).can_edit_role(role)
    }

    /// Changer le pseudo d'un autre membre (« Gérer les pseudos », rang plus haut que le sien).
    pub fn can_set_nick(&self, guild_id: u64, user_id: u64) -> bool {
        let Some(g) = self.guilds.iter().find(|g| g.id == guild_id) else { return false };
        if self.me.as_ref().is_none_or(|u| u.id == user_id) {
            return false;
        }
        let a = self.role_authority(guild_id);
        match self.member_top(guild_id, user_id) {
            Some(top) => a.can_act_on(Permissions::MANAGE_NICKNAMES, top, g.owner_id == Some(user_id)),
            // Rôles du membre inconnus : proposé si j'ai la permission, Discord répondra.
            None => g.owner_id != Some(user_id) && a.has(Permissions::MANAGE_NICKNAMES),
        }
    }

    fn roles_cmd(&mut self, action: RoleAction, guild_id: u64, cmd: RolesCmd) -> bool {
        if !self.roles_admin.pending.insert((action, guild_id)) {
            return false;
        }
        self.push_command(Cmd::Roles(cmd));
        true
    }

    /// Nouveau rôle (Discord le place juste au-dessus de @everyone).
    pub fn create_role(&mut self, guild_id: u64, name: &str) -> bool {
        let Some(name) = clean_name(name) else { return false };
        if !self.can_manage_roles(guild_id) {
            return false;
        }
        self.roles_cmd(RoleAction::Create, guild_id, RolesCmd::Create { guild_id, fields: RoleFields { name: Some(name), ..Default::default() } })
    }

    /// Copie d'un rôle : nom, couleur, permissions (celles qu'on peut
    /// accorder), affichage séparé, mention, emoji. L'icône n'est pas copiée
    /// (il faudrait la télécharger puis la renvoyer).
    pub fn duplicate_role(&mut self, guild_id: u64, role_id: u64) -> bool {
        let Some(r) = self.guild_role(guild_id, role_id).cloned() else { return false };
        if r.id == guild_id || !self.can_manage_roles(guild_id) {
            return false;
        }
        let x = self.roles_admin.extra(guild_id, role_id).cloned().unwrap_or_default();
        let grant = self.role_authority(guild_id).grantable();
        let fields = RoleFields {
            name: Some(r.name.clone()),
            permissions: Some(r.permissions & grant),
            color: Some(r.color),
            hoist: Some(x.hoist),
            mentionable: Some(r.mentionable),
            icon: None,
            unicode_emoji: x.unicode_emoji.map(Some),
        };
        self.roles_cmd(RoleAction::Create, guild_id, RolesCmd::Create { guild_id, fields })
    }

    /// Enregistre les champs changés d'un rôle. Refusé (rien n'est envoyé) :
    /// rôle au-dessus du mien, nom invalide, permission que je n'ai pas
    /// ajoutée, nom, couleur, icône ou affichage de @everyone, nom d'un rôle géré.
    pub fn edit_role(&mut self, guild_id: u64, role_id: u64, mut fields: RoleFields) -> bool {
        let Some(r) = self.guild_role(guild_id, role_id).cloned() else { return false };
        let a = self.role_authority(guild_id);
        if !a.can_edit_role(&r) {
            return false;
        }
        let managed = self.roles_admin.extra(guild_id, role_id).is_some_and(|x| x.managed);
        if let Some(n) = &fields.name {
            if r.id == guild_id || managed {
                return false;
            }
            let Some(n) = clean_name(n) else { return false };
            fields.name = Some(n);
        }
        if r.id == guild_id && (fields.color.is_some() || fields.hoist.is_some() || fields.icon.is_some() || fields.unicode_emoji.is_some()) {
            return false;
        }
        if let Some(p) = fields.permissions
            && !a.grantable().contains(Permissions(p.0 & !r.permissions.0))
        {
            return false;
        }
        if fields.is_empty() {
            return false;
        }
        self.roles_cmd(RoleAction::Edit, guild_id, RolesCmd::Edit { guild_id, role_id, fields })
    }

    /// Supprime un rôle (après confirmation dans l'interface).
    pub fn delete_role(&mut self, guild_id: u64, role_id: u64) -> bool {
        let Some(r) = self.guild_role(guild_id, role_id).cloned() else { return false };
        let managed = self.roles_admin.extra(guild_id, role_id).is_some_and(|x| x.managed);
        if r.id == guild_id || managed || !self.role_authority(guild_id).can_edit_role(&r) {
            return false;
        }
        self.roles_cmd(RoleAction::Delete, guild_id, RolesCmd::Delete { guild_id, role_id })
    }

    /// Dépose `role_id` au rang `to` de la liste affichée (sans @everyone).
    pub fn move_role(&mut self, guild_id: u64, role_id: u64, to: usize) -> bool {
        let a = self.role_authority(guild_id);
        let Some(g) = self.guilds.iter().find(|g| g.id == guild_id) else { return false };
        let Some(positions) = reorder(&g.roles, guild_id, a, role_id, to) else { return false };
        self.roles_cmd(RoleAction::Reorder, guild_id, RolesCmd::Reorder { guild_id, positions })
    }

    /// Donne (`add`) ou retire un rôle à un membre.
    pub fn set_member_role(&mut self, guild_id: u64, user_id: u64, role_id: u64, add: bool) -> bool {
        let Some(r) = self.guild_role(guild_id, role_id).cloned() else { return false };
        if !self.can_assign(guild_id, &r) || !self.roles_admin.member_roles.insert((guild_id, user_id, role_id)) {
            return false;
        }
        self.push_command(Cmd::Roles(RolesCmd::MemberRole { guild_id, user_id, role_id, add }));
        true
    }

    /// Pseudo d'un autre membre ; vide : retiré.
    pub fn set_nick(&mut self, guild_id: u64, user_id: u64, nick: &str) -> bool {
        let nick = nick.trim();
        if nick.chars().count() > NICK_MAX || !self.can_set_nick(guild_id, user_id) || !self.roles_admin.nicks.insert((guild_id, user_id)) {
            return false;
        }
        self.push_command(Cmd::Roles(RolesCmd::Nick { guild_id, user_id, nick: nick.to_string() }));
        true
    }

    /// Recherche de membres par début de nom ou de pseudo (geste de l'utilisateur).
    pub fn search_members(&mut self, guild_id: u64, query: &str) -> bool {
        let q: String = query.trim().chars().take(QUERY_MAX).collect();
        if q.is_empty() || self.roles_admin.search(guild_id).is_some_and(|s| s.loading) {
            return false;
        }
        if !self.roles_cmd(RoleAction::Search, guild_id, RolesCmd::Search { guild_id, query: q.clone() }) {
            return false;
        }
        let users = self.roles_admin.search.remove(&guild_id).map(|s| s.users).unwrap_or_default();
        self.roles_admin.search.insert(guild_id, Search { query: q, loading: true, users });
        true
    }

    /// Membres connus d'un serveur (liste des membres, messages, recherche).
    pub fn known_members(&self, guild_id: u64) -> Vec<u64> {
        let mut v: Vec<u64> = self.people.lock().members_of(guild_id).map(|(u, _)| u).collect();
        if let Some(me) = &self.me
            && self.guilds.iter().any(|g| g.id == guild_id && g.me.is_some())
            && !v.contains(&me.id)
        {
            v.push(me.id);
        }
        v
    }

    /// Nombre de membres connus portant chaque rôle.
    pub fn known_role_counts(&self, guild_id: u64) -> HashMap<u64, usize> {
        let mut out = HashMap::new();
        let mine = self.guilds.iter().find(|g| g.id == guild_id).and_then(|g| g.me.as_ref());
        // Moi : mes rôles du serveur (READY) priment sur les tables.
        let me = mine.and(self.me.as_ref()).map(|u| u.id);
        for (u, m) in self.people.lock().members_of(guild_id) {
            if Some(u) == me {
                continue;
            }
            for r in m.roles.iter().flatten() {
                *out.entry(*r).or_insert(0) += 1;
            }
        }
        if let Some(m) = mine {
            for r in &m.roles {
                *out.entry(*r).or_insert(0) += 1;
            }
        }
        out
    }

    pub(crate) fn apply_roles(&mut self, e: RolesEvent) -> Option<Effect> {
        let s = &mut self.roles_admin;
        s.bump();
        match e {
            RolesEvent::Extras { guild_id, roles, replace } => {
                if replace {
                    s.extras.retain(|(g, _), _| *g != guild_id);
                }
                s.extras.extend(roles.into_iter().map(|(id, x)| ((guild_id, id), x)));
            }
            RolesEvent::Boost { guild_id, boost } => {
                s.boost.insert(guild_id, boost);
            }
            RolesEvent::Forget { guild_id, role_id: Some(r) } => {
                s.extras.remove(&(guild_id, r));
            }
            RolesEvent::Forget { guild_id, role_id: None } => {
                s.extras.retain(|(g, _), _| *g != guild_id);
                s.boost.remove(&guild_id);
                s.search.remove(&guild_id);
            }
            RolesEvent::Saved { action, guild_id, roles } => {
                s.pending.remove(&(action, guild_id));
                s.notices.push_back(RolesNotice::Saved(action));
                let mut eff = None;
                for (role, extra) in roles {
                    let id = role.id;
                    self.roles_admin.extras.insert((guild_id, id), extra);
                    if action == RoleAction::Create {
                        self.roles_admin.created = Some((guild_id, id));
                    }
                    eff = self.apply(Event::RoleUpsert { guild_id, role }).or(eff);
                }
                return eff;
            }
            RolesEvent::Deleted { guild_id, role_id } => {
                s.pending.remove(&(RoleAction::Delete, guild_id));
                s.extras.remove(&(guild_id, role_id));
                s.notices.push_back(RolesNotice::Saved(RoleAction::Delete));
                return self.apply(Event::RoleDelete { guild_id, role_id });
            }
            RolesEvent::MemberDone { action, guild_id, user_id, role_id } => match role_id {
                Some(r) => {
                    s.member_roles.remove(&(guild_id, user_id, r));
                }
                None => {
                    s.nicks.remove(&(guild_id, user_id));
                    s.notices.push_back(RolesNotice::Saved(action));
                }
            },
            RolesEvent::Found { guild_id, query, users } => {
                s.pending.remove(&(RoleAction::Search, guild_id));
                s.search.insert(guild_id, Search { query, loading: false, users });
            }
            RolesEvent::Failed { action, guild_id, user_id, role_id, problem } => {
                s.pending.remove(&(action, guild_id));
                match (action, user_id, role_id) {
                    (RoleAction::MemberRole, Some(u), Some(r)) => {
                        s.member_roles.remove(&(guild_id, u, r));
                    }
                    (RoleAction::Nick, Some(u), _) => {
                        s.nicks.remove(&(guild_id, u));
                    }
                    (RoleAction::Search, ..) => {
                        if let Some(x) = s.search.get_mut(&guild_id) {
                            x.loading = false;
                        }
                    }
                    _ => {}
                }
                s.notices.push_back(RolesNotice::Failed { action, problem });
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::{Guild, Member, User};
    use crate::state::people::MemberInfo;
    use Permissions as P;

    const G: u64 = 100;
    const ME: u64 = 1;
    const BOB: u64 = 2;
    const R1: u64 = 201;
    const R2: u64 = 202;
    const R_MOD: u64 = 203;
    const R_HAUT: u64 = 204;

    fn role(id: u64, position: i32, p: P) -> Role {
        Role { id, name: format!("r{id}"), color: 0, position, permissions: p, mentionable: false }
    }

    /// Moi : modérateur (position 3, « Gérer les rôles » et « Gérer les pseudos »).
    fn store() -> Store {
        let mut s = Store::new();
        let mut g = Guild::new(G, "g".into(), Vec::new());
        g.owner_id = Some(999);
        g.roles = vec![role(G, 0, P::VIEW_CHANNEL), role(R1, 1, P::NONE), role(R2, 2, P::SEND_MESSAGES), role(R_MOD, 3, P::MANAGE_ROLES | P::MANAGE_NICKNAMES), role(R_HAUT, 4, P::NONE)];
        g.me = Some(Member { roles: vec![R_MOD], ..Default::default() });
        s.apply(Event::Ready { me: User::new(ME, "moi".into()), guilds: vec![g], dms: Vec::new() });
        s
    }

    fn roles_cmds(s: &mut Store) -> Vec<RolesCmd> {
        s.take_commands().into_iter().filter_map(|c| if let Cmd::Roles(r) = c { Some(r) } else { None }).collect()
    }

    #[test]
    fn hierarchy_is_checked_before_sending() {
        let mut s = store();
        assert!(s.can_manage_roles(G));
        // Au-dessus ou égal à mon rôle le plus haut : rien ne part.
        assert!(!s.edit_role(G, R_HAUT, RoleFields { color: Some(1), ..Default::default() }));
        assert!(!s.edit_role(G, R_MOD, RoleFields { color: Some(1), ..Default::default() }));
        assert!(!s.delete_role(G, R_MOD));
        assert!(!s.set_member_role(G, BOB, R_HAUT, true));
        // Une permission que je n'ai pas ne s'ajoute pas ; une que le rôle a déjà peut rester.
        assert!(!s.edit_role(G, R1, RoleFields { permissions: Some(P::BAN_MEMBERS), ..Default::default() }));
        assert!(s.edit_role(G, R2, RoleFields { permissions: Some(P::SEND_MESSAGES | P::MANAGE_ROLES), ..Default::default() }));
        // @everyone : ni nom ni couleur.
        assert!(!s.edit_role(G, G, RoleFields { name: Some("x".into()), ..Default::default() }));
        // Nom vide ou trop long.
        assert!(!s.edit_role(G, R1, RoleFields { name: Some("  ".into()), ..Default::default() }));
        assert_eq!(roles_cmds(&mut s), vec![RolesCmd::Edit { guild_id: G, role_id: R2, fields: RoleFields { permissions: Some(P::SEND_MESSAGES | P::MANAGE_ROLES), ..Default::default() } }]);
        // Une modification à la fois par serveur.
        assert!(!s.edit_role(G, R1, RoleFields { color: Some(5), ..Default::default() }));
    }

    #[test]
    fn reorder_moves_only_manageable_roles() {
        let s = store();
        let a = s.role_authority(G);
        let g = &s.guilds[0];
        // Affichés : R_HAUT, R_MOD, R2, R1. R1 monté au-dessus de R2.
        assert_eq!(reorder(&g.roles, G, a, R1, 2), Some(vec![(R1, 2), (R2, 1)]));
        // Au-dessus de mon rôle, ou un rôle plus haut : refusé.
        assert_eq!(reorder(&g.roles, G, a, R1, 1), None);
        assert_eq!(reorder(&g.roles, G, a, R_HAUT, 3), None);
        assert_eq!(reorder(&g.roles, G, a, R1, 3), None, "pas de changement");
        // Propriétaire : tout bouge ; positions en double renumérotées (à
        // position égale, R1, plus ancien, est au-dessus de R2).
        let mut roles = g.roles.clone();
        roles[1].position = 2;
        let order = reorder(&roles, G, Authority::Owner, R_HAUT, 3).unwrap_or_default();
        assert_eq!(order, vec![(R_MOD, 4), (R1, 3), (R_HAUT, 1)]);
    }

    #[test]
    fn create_duplicate_and_responses() {
        let mut s = store();
        assert!(s.create_role(G, " nouveau rôle "));
        assert!(!s.create_role(G, "autre"), "une création à la fois");
        let cmds = roles_cmds(&mut s);
        assert_eq!(cmds, vec![RolesCmd::Create { guild_id: G, fields: RoleFields { name: Some("nouveau rôle".into()), ..Default::default() } }]);
        let created = role(300, 1, P::NONE);
        s.apply(Event::Roles(RolesEvent::Saved { action: RoleAction::Create, guild_id: G, roles: vec![(created, RoleExtra { hoist: true, ..Default::default() })] }));
        assert_eq!(s.roles_admin.take_created(), Some((G, 300)));
        assert!(s.guilds[0].roles.iter().any(|r| r.id == 300));
        assert!(s.roles_admin.extra(G, 300).is_some_and(|x| x.hoist));
        assert_eq!(s.roles_admin.take_notice(), Some(RolesNotice::Saved(RoleAction::Create)));
        // Copie : seulement les permissions que j'ai.
        s.guilds[0].roles[1].permissions = P::BAN_MEMBERS | P::MANAGE_ROLES;
        assert!(s.duplicate_role(G, R1));
        let cmds = roles_cmds(&mut s);
        let RolesCmd::Create { fields, .. } = &cmds[0] else { panic!("{cmds:?}") };
        assert_eq!(fields.permissions, Some(P::MANAGE_ROLES));
        // Échec : annoncé, l'action est de nouveau possible.
        s.apply(Event::Roles(RolesEvent::Failed { action: RoleAction::Create, guild_id: G, user_id: None, role_id: None, problem: Problem::MfaRequired }));
        assert_eq!(s.roles_admin.take_notice(), Some(RolesNotice::Failed { action: RoleAction::Create, problem: Problem::MfaRequired }));
        assert!(!s.roles_admin.pending(RoleAction::Create, G));
        // Suppression : rôle retiré à la réponse.
        assert!(s.delete_role(G, R1));
        s.apply(Event::Roles(RolesEvent::Deleted { guild_id: G, role_id: R1 }));
        assert!(!s.guilds[0].roles.iter().any(|r| r.id == R1));
    }

    #[test]
    fn member_roles_and_nicknames() {
        let mut s = store();
        s.people.lock().upsert_member(G, BOB, MemberInfo { roles: Some(vec![R1]), ..Default::default() });
        assert!(s.set_member_role(G, BOB, R2, true));
        assert!(!s.set_member_role(G, BOB, R2, true), "déjà en cours");
        assert!(s.roles_admin.member_role_pending(G, BOB, R2));
        // Rôle géré : jamais donné à la main.
        s.apply(Event::Roles(RolesEvent::Extras { guild_id: G, roles: vec![(R1, RoleExtra { managed: true, ..Default::default() })], replace: false }));
        assert!(!s.set_member_role(G, BOB, R1, false));
        assert!(!s.delete_role(G, R1));
        s.apply(Event::Roles(RolesEvent::MemberDone { action: RoleAction::MemberRole, guild_id: G, user_id: BOB, role_id: Some(R2) }));
        assert!(!s.roles_admin.member_role_pending(G, BOB, R2));
        // Pseudo : membre plus bas que moi ; pas le propriétaire, pas un membre plus haut, pas moi.
        assert!(s.can_set_nick(G, BOB));
        assert!(!s.can_set_nick(G, 999));
        assert!(!s.can_set_nick(G, ME));
        s.people.lock().upsert_member(G, 3, MemberInfo { roles: Some(vec![R_HAUT]), ..Default::default() });
        assert!(!s.can_set_nick(G, 3));
        assert!(!s.set_nick(G, BOB, &"x".repeat(NICK_MAX + 1)));
        assert!(s.set_nick(G, BOB, " Bobby "));
        let cmds = roles_cmds(&mut s);
        assert_eq!(cmds.last(), Some(&RolesCmd::Nick { guild_id: G, user_id: BOB, nick: "Bobby".into() }));
        // Comptes et membres connus.
        assert_eq!(s.known_role_counts(G).get(&R1), Some(&1));
        let mut known = s.known_members(G);
        known.sort_unstable();
        assert_eq!(known, vec![ME, BOB, 3]);
    }

    #[test]
    fn search_and_forget() {
        let mut s = store();
        assert!(!s.search_members(G, "  "));
        assert!(s.search_members(G, "al"));
        assert!(!s.search_members(G, "ali"), "une recherche à la fois");
        assert!(s.roles_admin.search(G).is_some_and(|x| x.loading));
        s.apply(Event::Roles(RolesEvent::Found { guild_id: G, query: "al".into(), users: vec![BOB] }));
        assert_eq!(s.roles_admin.search(G).map(|x| x.users.clone()), Some(vec![BOB]));
        s.apply(Event::Roles(RolesEvent::Extras { guild_id: G, roles: vec![(R1, RoleExtra::default()), (R2, RoleExtra::default())], replace: true }));
        s.apply(Event::Roles(RolesEvent::Forget { guild_id: G, role_id: Some(R1) }));
        assert!(s.roles_admin.extra(G, R1).is_none() && s.roles_admin.extra(G, R2).is_some());
        s.apply(Event::Roles(RolesEvent::Forget { guild_id: G, role_id: None }));
        assert!(s.roles_admin.extra(G, R2).is_none() && s.roles_admin.search(G).is_none());
    }
}
