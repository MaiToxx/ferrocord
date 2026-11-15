//! Serveurs (lot A1) : rejoindre par une invitation, quitter, créer (vide ou
//! depuis un modèle), inviter des personnes, lister et révoquer les
//! invitations. Chaque commande suit un geste de l'utilisateur ; une seule
//! action de chaque sorte à la fois. Le réseau est dans `net::guilds`.

use super::event::{Cmd, Event};
use super::model::ChannelKind;
use super::notice::Problem;
use super::perms::{base_permissions, Permissions, Verdict};
use super::store::{Effect, Store};
use std::collections::{HashMap, HashSet, VecDeque};

/// Nom d'un serveur : 2 à 100 caractères (documentation : Create Guild).
pub const NAME_MIN: usize = 2;
pub const NAME_MAX: usize = 100;
/// Invitations : durée (s) et nombre d'utilisations au plus (Create Channel Invite).
pub const MAX_AGE_MAX: u32 = 604_800;
pub const MAX_USES_MAX: u32 = 100;

/// Interface -> réseau.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerCmd {
    /// `POST /invites/{code}`.
    Join { code: String },
    /// `DELETE /users/@me/guilds/{id}`.
    Leave { guild_id: u64 },
    /// `POST /guilds` ; `icon` : image en `data:` (128 × 128).
    Create { name: String, icon: Option<String> },
    /// `GET /guilds/templates/{code}` : aperçu du modèle.
    Template { code: String },
    /// `POST /guilds/templates/{code}`.
    CreateFromTemplate { code: String, name: String, icon: Option<String> },
    /// `POST /channels/{id}/invites` ; `max_age` en secondes (0 : jamais), `max_uses` (0 : sans limite).
    CreateInvite { channel_id: u64, max_age: u32, max_uses: u32, temporary: bool },
    /// `POST /channels/{id}/invites` d'un MP de groupe : seul `max_age` est admis (lot FF2).
    CreateGroupInvite { channel_id: u64 },
    /// `GET /guilds/{id}/invites`.
    Invites { guild_id: u64 },
    /// `DELETE /invites/{code}`.
    Revoke { guild_id: u64, code: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ServerAction {
    Join,
    Leave,
    Create,
    Template,
    Invite,
    Invites,
    Revoke,
}

/// Pourquoi une action a échoué.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    Problem(Problem),
    /// Discord demande un captcha : jamais contourné, à faire dans l'application officielle.
    Captcha,
    /// Invitation ou modèle inconnu, expiré ou épuisé (404).
    NotFound,
    /// Le serveur n'accepte pas d'invitations pour l'instant (réponse 204 vide).
    InvitesDisabled,
}

/// Invitation (objet Invite avec ses métadonnées).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invite {
    pub code: String,
    pub guild_id: Option<u64>,
    pub channel_id: Option<u64>,
    pub channel_name: String,
    pub inviter: Option<String>,
    pub uses: u32,
    pub max_uses: u32,
    pub max_age: u32,
    pub temporary: bool,
    /// Fin de validité (ms Unix), `None` : jamais.
    pub expires_at_ms: Option<i64>,
}

impl Invite {
    pub fn url(&self) -> String {
        format!("https://discord.gg/{}", self.code)
    }
}

/// Aperçu d'un modèle de serveur.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateInfo {
    pub code: String,
    pub name: String,
    pub description: String,
    pub usage_count: u64,
    /// Nom du serveur d'origine (nom proposé pour le nouveau serveur).
    pub guild_name: String,
    /// Salons du modèle (aperçu).
    pub channels: Vec<String>,
}

/// Réseau -> magasin.
#[derive(Debug)]
pub enum ServerEvent {
    /// Invitation acceptée ; `guild_id` : `None` pour une invitation sans
    /// serveur (MP de groupe). `verify` : le serveur demande de remplir son
    /// formulaire d'adhésion (à faire dans l'application officielle).
    Joined { guild_id: Option<u64>, name: String, verify: bool },
    Left { guild_id: u64 },
    Created { guild_id: u64 },
    Template(TemplateInfo),
    InviteCreated(Invite),
    Invites { guild_id: u64, invites: Vec<Invite> },
    Revoked { guild_id: u64, code: String },
    Failed { action: ServerAction, failure: Failure },
}

/// Message à afficher (mis en mots par l'interface).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerNotice {
    Joined { name: String, verify: bool },
    Left { name: String },
    Failed { action: ServerAction, failure: Failure },
    /// Le texte saisi n'est pas un lien ni un code d'invitation (ou de modèle).
    BadCode,
    /// Le propriétaire ne peut pas quitter son serveur.
    Owner,
}

/// Invitations d'un serveur.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum InviteList {
    #[default]
    Loading,
    Ready(Vec<Invite>),
    Failed(Failure),
}

#[derive(Default)]
pub struct Servers {
    pending: HashSet<ServerAction>,
    /// Aperçu du dernier modèle demandé.
    pub template: Option<TemplateInfo>,
    /// Dernière invitation créée (« Inviter des personnes »).
    pub created: Option<Invite>,
    pub invites: HashMap<u64, InviteList>,
    revoking: HashSet<String>,
    /// Serveur rejoint ou créé, à ouvrir dès que GUILD_CREATE l'apporte.
    open_when_ready: Option<u64>,
    notices: VecDeque<ServerNotice>,
    /// Action terminée avec succès (la fenêtre qui l'a lancée se ferme).
    done: Option<ServerAction>,
    /// Noms des serveurs quittés, pour le message.
    leaving: HashMap<u64, String>,
}

impl Servers {
    pub fn clear(&mut self) {
        *self = Servers::default();
    }

    pub fn pending(&self, a: ServerAction) -> bool {
        self.pending.contains(&a)
    }

    pub fn revoking(&self, code: &str) -> bool {
        self.revoking.contains(code)
    }

    pub fn take_notice(&mut self) -> Option<ServerNotice> {
        self.notices.pop_front()
    }

    pub fn take_done(&mut self) -> Option<ServerAction> {
        self.done.take()
    }

    fn start(&mut self, a: ServerAction) -> bool {
        self.pending.insert(a)
    }
}

/// Code d'une invitation : lien `discord.gg/…`, `discord.com/invite/…`, ou
/// le code seul. Rien d'autre n'est accepté (le code part dans un chemin).
pub fn invite_code(input: &str) -> Option<String> {
    code_after(input, &["discord.gg/", "discord.com/invite/", "discordapp.com/invite/", "ptb.discord.com/invite/", "canary.discord.com/invite/"])
}

/// Code d'un modèle : `discord.new/…`, `discord.com/template/…`, ou le code seul.
pub fn template_code(input: &str) -> Option<String> {
    code_after(input, &["discord.new/", "discord.com/template/", "discordapp.com/template/"])
}

fn code_after<'a>(input: &'a str, prefixes: &[&str]) -> Option<String> {
    let s = input.trim();
    let strip = |s: &'a str, p: &str| s.get(..p.len()).filter(|h| h.eq_ignore_ascii_case(p)).map(|_| &s[p.len()..]);
    let s = strip(s, "https://").or_else(|| strip(s, "http://")).unwrap_or(s);
    let s = strip(s, "www.").unwrap_or(s);
    let rest = match prefixes.iter().find_map(|p| s.get(..p.len()).filter(|h| h.eq_ignore_ascii_case(p)).map(|_| &s[p.len()..])) {
        Some(r) => r,
        // Sans hôte reconnu : un code seul, sans barre.
        None if !s.contains('/') => s,
        None => return None,
    };
    let code = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let ok = (2..=64).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    ok.then(|| code.to_string())
}

fn valid_name(name: &str) -> Option<String> {
    let n = name.trim();
    (NAME_MIN..=NAME_MAX).contains(&n.chars().count()).then(|| n.to_string())
}

impl Store {
    fn server_cmd(&mut self, action: ServerAction, cmd: ServerCmd) -> bool {
        if !self.servers.start(action) {
            return false;
        }
        self.push_command(Cmd::Servers(cmd));
        true
    }

    /// Rejoint le serveur d'un lien ou code d'invitation. Réutilisable (cartes
    /// d'invitation des messages) : `false` si le texte n'est pas une
    /// invitation (annoncé) ou si une autre est en cours.
    pub fn join_invite(&mut self, input: &str) -> bool {
        let Some(code) = invite_code(input) else {
            self.servers.notices.push_back(ServerNotice::BadCode);
            return false;
        };
        self.server_cmd(ServerAction::Join, ServerCmd::Join { code })
    }

    /// Suis-je le propriétaire de ce serveur ? (Il ne peut pas le quitter.)
    pub fn owns_guild(&self, id: u64) -> bool {
        let me = self.me.as_ref().map(|u| u.id);
        self.guilds.iter().any(|g| g.id == id && g.owner_id.is_some() && g.owner_id == me)
    }

    pub fn leave_guild(&mut self, id: u64) -> bool {
        if self.owns_guild(id) {
            self.servers.notices.push_back(ServerNotice::Owner);
            return false;
        }
        let Some(g) = self.guilds.iter().find(|g| g.id == id) else { return false };
        let name = g.name.clone();
        if !self.server_cmd(ServerAction::Leave, ServerCmd::Leave { guild_id: id }) {
            return false;
        }
        self.servers.leaving.insert(id, name);
        true
    }

    /// Crée un serveur vide ; `false` si le nom ne convient pas ou si une création est en cours.
    pub fn create_guild(&mut self, name: &str, icon: Option<String>) -> bool {
        let Some(name) = valid_name(name) else { return false };
        self.server_cmd(ServerAction::Create, ServerCmd::Create { name, icon })
    }

    /// Demande l'aperçu d'un modèle (lien ou code).
    pub fn load_template(&mut self, input: &str) -> bool {
        let Some(code) = template_code(input) else {
            self.servers.notices.push_back(ServerNotice::BadCode);
            return false;
        };
        self.servers.template = None;
        self.server_cmd(ServerAction::Template, ServerCmd::Template { code })
    }

    pub fn create_from_template(&mut self, code: &str, name: &str, icon: Option<String>) -> bool {
        let (Some(code), Some(name)) = (template_code(code), valid_name(name)) else { return false };
        self.server_cmd(ServerAction::Create, ServerCmd::CreateFromTemplate { code, name, icon })
    }

    /// Salons d'où je peux inviter : textuels visibles, sans refus certain de
    /// CREATE_INSTANT_INVITE ; le salon ouvert d'abord.
    pub fn invite_channels(&self, guild: u64) -> Vec<(u64, String)> {
        let Some(g) = self.guilds.iter().find(|g| g.id == guild) else { return Vec::new() };
        let mut out: Vec<(u64, String, i32)> = g
            .channels
            .iter()
            .filter(|c| c.kind != ChannelKind::Category && !self.is_hidden(c.id) && self.can(c.id, Permissions::CREATE_INSTANT_INVITE) != Verdict::Refuse)
            .map(|c| (c.id, c.name.clone(), c.position))
            .collect();
        out.sort_by_key(|(id, _, pos)| (Some(*id) != self.channel, *pos, *id));
        out.into_iter().map(|(id, name, _)| (id, name)).collect()
    }

    /// « Gérer le serveur » : listes et révocation des invitations. Inconnu : proposé (Discord répondra).
    pub fn can_manage_guild(&self, guild: u64) -> bool {
        let (Some(me), Some(g)) = (self.me.as_ref(), self.guilds.iter().find(|g| g.id == guild)) else { return false };
        base_permissions(g, me.id).is_none_or(|p| p.contains(Permissions::MANAGE_GUILD))
    }

    pub fn create_invite(&mut self, channel_id: u64, max_age: u32, max_uses: u32, temporary: bool) -> bool {
        self.servers.created = None;
        let (max_age, max_uses) = (max_age.min(MAX_AGE_MAX), max_uses.min(MAX_USES_MAX));
        self.server_cmd(ServerAction::Invite, ServerCmd::CreateInvite { channel_id, max_age, max_uses, temporary })
    }

    /// Lien d'invitation d'un MP de groupe (lot FF2, F20) : valable un jour,
    /// la seule durée étant réglable pour un groupe.
    pub fn create_group_invite(&mut self, channel_id: u64) -> bool {
        if !self.relations.dm_info(channel_id).is_some_and(|d| d.group) {
            return false;
        }
        self.servers.created = None;
        self.server_cmd(ServerAction::Invite, ServerCmd::CreateGroupInvite { channel_id })
    }

    pub fn load_invites(&mut self, guild_id: u64) -> bool {
        if !self.server_cmd(ServerAction::Invites, ServerCmd::Invites { guild_id }) {
            return false;
        }
        self.servers.invites.insert(guild_id, InviteList::Loading);
        true
    }

    /// Révoque une invitation (retirée de la liste à la réponse).
    pub fn revoke_invite(&mut self, guild_id: u64, code: &str) -> bool {
        if !self.servers.revoking.insert(code.to_string()) {
            return false;
        }
        self.push_command(Cmd::Servers(ServerCmd::Revoke { guild_id, code: code.to_string() }));
        true
    }

    /// GUILD_CREATE : le serveur rejoint ou créé à l'instant s'ouvre.
    pub(super) fn guild_arrived(&mut self, id: u64) {
        if self.servers.open_when_ready == Some(id) {
            self.servers.open_when_ready = None;
            self.select_guild(id);
        }
    }

    fn open_or_wait(&mut self, id: u64) {
        if self.guilds.iter().any(|g| g.id == id) {
            self.select_guild(id);
        } else {
            self.servers.open_when_ready = Some(id);
        }
    }

    pub(super) fn apply_servers(&mut self, e: ServerEvent) -> Option<Effect> {
        let s = &mut self.servers;
        match e {
            ServerEvent::Joined { guild_id, name, verify } => {
                s.pending.remove(&ServerAction::Join);
                s.done = Some(ServerAction::Join);
                s.notices.push_back(ServerNotice::Joined { name, verify });
                // Cartes d'invitation des messages (lot T13) en attente.
                self.invites.end_joins(false);
                if let Some(id) = guild_id {
                    // Écran de bienvenue ou intégration (lot A7).
                    self.community_joined(id);
                    self.open_or_wait(id);
                }
            }
            ServerEvent::Left { guild_id } => {
                s.pending.remove(&ServerAction::Leave);
                s.done = Some(ServerAction::Leave);
                let name = s.leaving.remove(&guild_id).unwrap_or_default();
                s.notices.push_back(ServerNotice::Left { name });
                // Comme GUILD_DELETE (qui suivra, et ne trouvera plus rien).
                return self.apply(Event::GuildDelete(guild_id));
            }
            ServerEvent::Created { guild_id } => {
                s.pending.remove(&ServerAction::Create);
                s.done = Some(ServerAction::Create);
                s.template = None;
                self.open_or_wait(guild_id);
            }
            ServerEvent::Template(t) => {
                s.pending.remove(&ServerAction::Template);
                s.template = Some(t);
            }
            ServerEvent::InviteCreated(inv) => {
                s.pending.remove(&ServerAction::Invite);
                if let Some(InviteList::Ready(list)) = inv.guild_id.and_then(|g| s.invites.get_mut(&g)) {
                    list.retain(|i| i.code != inv.code);
                    list.insert(0, inv.clone());
                }
                s.created = Some(inv);
            }
            ServerEvent::Invites { guild_id, invites } => {
                s.pending.remove(&ServerAction::Invites);
                s.invites.insert(guild_id, InviteList::Ready(invites));
            }
            ServerEvent::Revoked { guild_id, code } => {
                s.revoking.remove(&code);
                if let Some(InviteList::Ready(list)) = s.invites.get_mut(&guild_id) {
                    list.retain(|i| i.code != code);
                }
                if s.created.as_ref().is_some_and(|i| i.code == code) {
                    s.created = None;
                }
            }
            ServerEvent::Failed { action, failure } => {
                s.pending.remove(&action);
                if action == ServerAction::Revoke {
                    s.revoking.clear();
                }
                if action == ServerAction::Invites {
                    // Une liste en chargement montre l'échec à la place.
                    for l in s.invites.values_mut().filter(|l| **l == InviteList::Loading) {
                        *l = InviteList::Failed(failure.clone());
                    }
                }
                if action == ServerAction::Join {
                    self.invites.end_joins(failure == Failure::Captcha);
                }
                if action == ServerAction::Leave {
                    s.leaving.clear();
                }
                s.notices.push_back(ServerNotice::Failed { action, failure });
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_and_template_codes() {
        for (input, code) in [
            ("abcDEF12", Some("abcDEF12")),
            ("  https://discord.gg/abc-12  ", Some("abc-12")),
            ("discord.gg/abc?event=1", Some("abc")),
            ("https://discord.com/invite/Xyz", Some("Xyz")),
            ("https://canary.discord.com/invite/Xyz/", Some("Xyz")),
            ("HTTPS://DISCORD.GG/abc", Some("abc")),
            ("https://example.com/invite/abc", None),
            ("discord.gg/../../users/@me", None),
            ("a b", None),
            ("x", None),
            ("", None),
        ] {
            assert_eq!(invite_code(input).as_deref(), code, "{input}");
        }
        assert_eq!(template_code("https://discord.new/2TffvPucqHkN").as_deref(), Some("2TffvPucqHkN"));
        assert_eq!(template_code("discord.com/template/abcd").as_deref(), Some("abcd"));
        assert_eq!(template_code("https://discord.gg/abcd"), None);
    }

    #[test]
    fn one_action_of_each_kind_at_a_time() {
        let mut s = Store::new();
        assert!(s.join_invite("discord.gg/abc"));
        assert!(!s.join_invite("discord.gg/def"), "déjà en cours");
        assert!(!s.join_invite("pas une invitation/"));
        assert_eq!(s.servers.take_notice(), Some(ServerNotice::BadCode));
        assert!(!s.create_guild(" a ", None), "nom trop court");
        assert!(s.create_guild("Mon serveur", None));
        assert_eq!(
            s.take_commands(),
            vec![Cmd::Servers(ServerCmd::Join { code: "abc".into() }), Cmd::Servers(ServerCmd::Create { name: "Mon serveur".into(), icon: None })]
        );
        s.apply(Event::Servers(ServerEvent::Failed { action: ServerAction::Join, failure: Failure::Captcha }));
        assert_eq!(s.servers.take_notice(), Some(ServerNotice::Failed { action: ServerAction::Join, failure: Failure::Captcha }));
        assert!(!s.servers.pending(ServerAction::Join));
        // Serveur créé : ouvert dès qu'il arrive.
        s.apply(Event::Servers(ServerEvent::Created { guild_id: 77 }));
        assert_eq!(s.servers.take_done(), Some(ServerAction::Create));
        assert_eq!(s.guild, None);
    }

    #[test]
    fn invites_list_create_and_revoke() {
        let mut s = Store::new();
        let inv = |code: &str| Invite {
            code: code.into(),
            guild_id: Some(5),
            channel_id: Some(6),
            channel_name: "général".into(),
            inviter: None,
            uses: 0,
            max_uses: 0,
            max_age: 0,
            temporary: false,
            expires_at_ms: None,
        };
        assert!(s.load_invites(5));
        assert_eq!(s.servers.invites.get(&5), Some(&InviteList::Loading));
        s.apply(Event::Servers(ServerEvent::Invites { guild_id: 5, invites: vec![inv("a"), inv("b")] }));
        assert!(s.create_invite(6, 86_400, 500, false));
        assert_eq!(s.take_commands().last(), Some(&Cmd::Servers(ServerCmd::CreateInvite { channel_id: 6, max_age: 86_400, max_uses: MAX_USES_MAX, temporary: false })));
        s.apply(Event::Servers(ServerEvent::InviteCreated(inv("c"))));
        assert_eq!(s.servers.created.as_ref().map(Invite::url).as_deref(), Some("https://discord.gg/c"));
        assert!(s.revoke_invite(5, "a"));
        assert!(!s.revoke_invite(5, "a"));
        s.apply(Event::Servers(ServerEvent::Revoked { guild_id: 5, code: "a".into() }));
        let Some(InviteList::Ready(list)) = s.servers.invites.get(&5) else { panic!() };
        assert_eq!(list.iter().map(|i| i.code.as_str()).collect::<Vec<_>>(), vec!["c", "b"]);
    }
}
