//! Relations (amis, demandes, bloqués, ignorés), conversations privées vues
//! par leurs membres et présences des amis. Tout vient du READY et des
//! événements de la passerelle : rien n'est demandé en boucle.
//!
//! Les écritures (demande d'ami, blocage, groupe…) partent comme `RelationsCmd`
//! dans la file de commandes du magasin, toujours après un geste de
//! l'utilisateur ; la réponse revient par les événements de la passerelle
//! (RELATIONSHIP_*, CHANNEL_*), ou par `RelationsEvent::Failed`.

use super::model::Avatar;
use super::notice::Problem;
use std::collections::{HashMap, VecDeque};

/// Utilisateurs gardés au plus (Discord plafonne à 1000 amis et 5000
/// bloqués ; s'y ajoutent les membres des MP). Au-delà, les nouveaux ne sont
/// pas retenus ; tout est reconstruit à chaque READY.
pub const MAX_PEOPLE: usize = 20_000;
/// Membres d'un MP de groupe, au plus (documentation : 10, 25 pour le personnel de Discord).
pub const MAX_GROUP: usize = 10;
/// Longueur maximale du message joint à une demande d'ami (docs.discord.food).
pub const NOTE_MAX: usize = 120;
/// Notifications en attente d'affichage, au plus.
const MAX_NOTICES: usize = 8;

/// Type de relation (champ `type`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RelKind {
    /// 1
    Friend,
    /// 2
    Blocked,
    /// 3 : demande reçue.
    Incoming,
    /// 4 : demande envoyée.
    Outgoing,
    /// 0 (aucune, par exemple un utilisateur seulement ignoré), 5 (implicite), ou inconnu.
    Other,
}

/// Écriture sur une relation que Discord vient d'accepter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Done {
    Blocked,
    /// Demande reçue acceptée.
    Accepted,
    Ignored,
}

impl RelKind {
    pub fn from_code(code: u64) -> RelKind {
        match code {
            1 => RelKind::Friend,
            2 => RelKind::Blocked,
            3 => RelKind::Incoming,
            4 => RelKind::Outgoing,
            _ => RelKind::Other,
        }
    }
}

/// Utilisateur tel que l'affiche la page Amis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub id: u64,
    /// Nom affiché (`global_name`, sinon `username`).
    pub name: String,
    pub username: String,
    pub avatar: Avatar,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relation {
    /// Identifiant de l'autre utilisateur.
    pub id: u64,
    pub kind: RelKind,
    pub nickname: Option<String>,
    pub ignored: bool,
    /// Absent d'un paquet partiel : la personne déjà connue est gardée.
    pub user: Option<Person>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum OnlineStatus {
    #[default]
    Offline,
    Online,
    Idle,
    Dnd,
}

impl OnlineStatus {
    /// Valeur documentée du champ `status` (`invisible` vu d'ailleurs = hors ligne).
    pub fn parse(s: &str) -> OnlineStatus {
        match s {
            "online" => OnlineStatus::Online,
            "idle" => OnlineStatus::Idle,
            "dnd" => OnlineStatus::Dnd,
            _ => OnlineStatus::Offline,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityKind {
    Playing,
    Streaming,
    Listening,
    Watching,
    /// Statut personnalisé : `name` est son texte (`state`).
    Custom,
    Competing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activity {
    pub kind: ActivityKind,
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Presence {
    pub status: OnlineStatus,
    /// Première activité lisible (le statut personnalisé passe après les autres).
    pub activity: Option<Activity>,
}

/// Conversation privée vue par ses membres (le reste est dans `model::Dm`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DmInfo {
    pub id: u64,
    pub group: bool,
    pub owner: Option<u64>,
    /// Destinataires, sans moi.
    pub recipients: Vec<u64>,
    /// Demande de message en attente (`is_message_request`, lot U).
    pub request: bool,
    /// Demande rangée par Discord dans le dossier de spam (`is_spam`).
    pub spam: bool,
}

/// État de lecture d'un salon (READY `read_state`, type CHANNEL).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadState {
    pub channel_id: u64,
    /// Dernier message lu (0 : aucun).
    pub last_acked: u64,
    pub mentions: u32,
}

/// Réseau -> magasin.
#[derive(Debug)]
pub enum RelationsEvent {
    /// Données sociales du READY. `reads` : `None` sans `read_state` (compte
    /// de bot) : les non-lus restent alors calculés localement.
    /// `heads` : dernier message de chaque salon (serveurs et MP).
    Ready { relations: Vec<Relation>, people: Vec<Person>, dms: Vec<DmInfo>, reads: Option<Vec<ReadState>>, heads: Vec<(u64, u64)> },
    /// RELATIONSHIP_ADD : relation complète (création ou changement de type).
    RelationAdd(Relation),
    /// RELATIONSHIP_UPDATE : surnom ou « ignoré » ; `None` : inchangé.
    /// `kind` : le type porté par le paquet, s'il en porte un (un déblocage peut
    /// arriver ainsi, la relation restant pour un surnom ou un « ignoré »).
    RelationUpdate { id: u64, kind: Option<RelKind>, nickname: Option<Option<String>>, ignored: Option<bool> },
    RelationRemove { id: u64 },
    /// Écriture acceptée par Discord (réponse de la requête) : appliquée sans
    /// attendre le RELATIONSHIP_* de la passerelle, qui la répétera sans effet.
    Done { id: u64, what: Done },
    /// MP créé ou modifié (CHANNEL_CREATE / CHANNEL_UPDATE, réponse REST),
    /// avec les personnes qu'il nomme.
    Dm { info: DmInfo, people: Vec<Person>, known_recipients: bool },
    RecipientAdd { channel_id: u64, user: Person },
    RecipientRemove { channel_id: u64, user_id: u64 },
    /// MESSAGE_ACK (cet appareil ou un autre). `mentions` : `None` = inchangé.
    Ack { channel_id: u64, message_id: u64, mentions: Option<u32> },
    /// Le MP avec `user_id` demandé par l'utilisateur existe : il est ouvert.
    DmOpened { channel_id: u64, user_id: Option<u64> },
    /// Demande d'ami partie (204).
    RequestSent { username: String },
    /// Écriture refusée ou impossible.
    Failed { action: RelationsAction, problem: Problem },
    /// Discord exige un captcha : l'action doit se faire dans l'application officielle.
    Captcha { action: RelationsAction },
}

/// Magasin -> réseau. Chaque variante suit un geste de l'utilisateur.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelationsCmd {
    /// Accusé de lecture d'un salon jusqu'à ce message.
    Ack { channel_id: u64, message_id: u64 },
    /// « Marquer le serveur comme lu » : (salon, dernier message).
    AckBulk(Vec<(u64, u64)>),
    /// « Marquer comme non lu » (lot M) : l'accusé recule juste avant ce
    /// message ; `mentions` : mentions qui redeviennent non lues.
    Unack { channel_id: u64, message_id: u64, mentions: u32 },
    /// `note` : message joint à la demande (120 caractères au plus ;
    /// docs.discord.food, « Send Friend Request »), absent si rien n'est écrit.
    AddFriend { username: String, note: Option<String> },
    /// Accepter une demande reçue (PUT sans type).
    Accept { user_id: u64 },
    Block { user_id: u64 },
    /// Retirer un ami, refuser ou annuler une demande, débloquer.
    Remove { user_id: u64 },
    Unignore { user_id: u64 },
    OpenDm { user_id: u64 },
    /// `recipients` : les autres membres (moi en plus s'il n'y en a qu'un).
    CreateGroup { recipients: Vec<u64> },
    AddRecipient { channel_id: u64, user_id: u64 },
    RemoveRecipient { channel_id: u64, user_id: u64 },
    Rename { channel_id: u64, name: String },
    /// Icône d'un groupe (lot FF2, F20) : image `data:` de 128 × 128.
    SetIcon { channel_id: u64, icon: String },
    /// Transfert de la propriété d'un groupe à l'un de ses membres (F20).
    Transfer { channel_id: u64, user_id: u64 },
    /// Fermer un MP ou quitter un groupe.
    Close { channel_id: u64 },
    /// « Ignorer » (lot U).
    Ignore { user_id: u64 },
    /// Pseudo d'ami ; `None` le retire.
    Nickname { user_id: u64, nickname: Option<String> },
    /// Demande de message : accepter.
    AcceptRequest { channel_id: u64 },
    /// Demande de message : ignorer (la conversation est supprimée).
    RejectRequest { channel_id: u64 },
}

impl RelationsCmd {
    pub fn action(&self) -> RelationsAction {
        match self {
            RelationsCmd::Ack { .. } | RelationsCmd::AckBulk(_) | RelationsCmd::Unack { .. } => RelationsAction::Ack,
            RelationsCmd::AddFriend { .. } => RelationsAction::AddFriend,
            RelationsCmd::Accept { .. } => RelationsAction::Accept,
            RelationsCmd::Block { .. } => RelationsAction::Block,
            RelationsCmd::Remove { .. } => RelationsAction::Remove,
            RelationsCmd::Unignore { .. } => RelationsAction::Unignore,
            RelationsCmd::OpenDm { .. } => RelationsAction::OpenDm,
            RelationsCmd::CreateGroup { .. } => RelationsAction::CreateGroup,
            RelationsCmd::AddRecipient { .. } => RelationsAction::AddMember,
            RelationsCmd::RemoveRecipient { .. } => RelationsAction::RemoveMember,
            RelationsCmd::Rename { .. } => RelationsAction::Rename,
            RelationsCmd::SetIcon { .. } | RelationsCmd::Transfer { .. } => RelationsAction::EditGroup,
            RelationsCmd::Close { .. } => RelationsAction::Close,
            RelationsCmd::Ignore { .. } => RelationsAction::Ignore,
            RelationsCmd::Nickname { .. } => RelationsAction::Nickname,
            RelationsCmd::AcceptRequest { .. } => RelationsAction::AcceptRequest,
            RelationsCmd::RejectRequest { .. } => RelationsAction::RejectRequest,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelationsAction {
    Ack,
    AddFriend,
    Accept,
    Block,
    Remove,
    Unignore,
    OpenDm,
    CreateGroup,
    AddMember,
    RemoveMember,
    Rename,
    /// Icône ou propriétaire d'un groupe (lot FF2).
    EditGroup,
    Close,
    Ignore,
    Nickname,
    AcceptRequest,
    RejectRequest,
}

/// Ce que l'interface doit dire (mis en mots par `ui/`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelationsNotice {
    RequestSent { username: String },
    Failed { action: RelationsAction, problem: Problem },
    Captcha { action: RelationsAction },
}

/// Onglet de la page Amis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FriendTab {
    #[default]
    Online,
    All,
    Pending,
    Blocked,
}

#[derive(Default)]
pub struct Relations {
    relations: HashMap<u64, Relation>,
    people: HashMap<u64, Person>,
    dms: HashMap<u64, DmInfo>,
    /// Change à chaque modification de ce que montre la page Amis : elle ne
    /// refait ses listes (tri, filtre) que dans ce cas.
    rev: u64,
    notices: VecDeque<RelationsNotice>,
    /// MP demandé avec cet utilisateur : ouvert dès sa création.
    pub(super) opening: Option<u64>,
    /// Demandes d'ami reçues pendant la session, pas encore annoncées (lot FF).
    fresh_incoming: Vec<u64>,
}

impl Relations {
    pub fn rev(&self) -> u64 {
        self.rev
    }

    fn bump(&mut self) {
        self.rev = self.rev.wrapping_add(1);
    }

    pub fn clear(&mut self) {
        let rev = self.rev;
        *self = Relations::default();
        self.rev = rev.wrapping_add(1);
    }

    pub fn relation(&self, id: u64) -> Option<&Relation> {
        self.relations.get(&id)
    }

    pub fn person(&self, id: u64) -> Option<&Person> {
        self.people.get(&id)
    }

    pub fn dm_info(&self, channel: u64) -> Option<&DmInfo> {
        self.dms.get(&channel)
    }

    /// Demandes de message en attente (`spam` : celles du dossier de spam), les plus récentes d'abord.
    pub fn requests(&self, spam: bool) -> Vec<u64> {
        let mut v: Vec<u64> = self.dms.values().filter(|d| d.request && d.spam == spam).map(|d| d.id).collect();
        v.sort_unstable_by(|a, b| b.cmp(a));
        v
    }

    /// Nombre de demandes de message, spam compris.
    pub fn request_count(&self) -> usize {
        self.dms.values().filter(|d| d.request).count()
    }

    /// Ce MP est une demande de message en attente.
    pub fn is_request(&self, channel: u64) -> bool {
        self.dms.get(&channel).is_some_and(|d| d.request)
    }

    /// Surnom d'ami donné à `id`, s'il y en a un.
    pub fn nickname(&self, id: u64) -> Option<&str> {
        self.relations.get(&id).and_then(|r| r.nickname.as_deref()).filter(|n| !n.is_empty())
    }

    /// Nom affiché : surnom d'ami, sinon nom de l'utilisateur ; vide si inconnu.
    pub fn display_name(&self, id: u64) -> &str {
        self.relations
            .get(&id)
            .and_then(|r| r.nickname.as_deref())
            .filter(|n| !n.is_empty())
            .or_else(|| self.people.get(&id).map(|p| p.name.as_str()))
            .unwrap_or("")
    }

    /// MP à deux avec cet utilisateur, s'il est connu.
    pub fn dm_with(&self, user: u64) -> Option<u64> {
        self.dms.values().filter(|d| !d.group && d.recipients == [user]).map(|d| d.id).max()
    }

    /// Y a-t-il une relation de cette sorte ? (Onglets de la page Amis.)
    pub fn any(&self, kind: RelKind) -> bool {
        self.relations.values().any(|r| r.kind == kind)
    }

    /// Y a-t-il quelqu'un de bloqué ou d'ignoré ? (Onglet « Bloqués ».)
    pub fn any_blocked(&self) -> bool {
        self.relations.values().any(|r| r.kind == RelKind::Blocked || r.ignored)
    }

    pub fn friends(&self) -> impl Iterator<Item = &Relation> {
        self.relations.values().filter(|r| r.kind == RelKind::Friend)
    }

    /// Demandes reçues (pastille de l'onglet « En attente »).
    pub fn incoming(&self) -> usize {
        self.relations.values().filter(|r| r.kind == RelKind::Incoming).count()
    }

    /// Relations d'un onglet, dont le nom ou le nom d'utilisateur contient
    /// `query` (sans casse), triées par nom. Appelé seulement quand `rev`,
    /// l'onglet ou la recherche changent.
    /// `online` : la présence vient du magasin des présences (`state::people`).
    pub fn list(&self, tab: FriendTab, query: &str, online: impl Fn(u64) -> bool) -> Vec<u64> {
        let q = query.trim().to_lowercase();
        let mut out: Vec<(String, u64)> = self
            .relations
            .values()
            .filter(|r| match tab {
                FriendTab::Online => r.kind == RelKind::Friend && online(r.id),
                FriendTab::All => r.kind == RelKind::Friend,
                FriendTab::Pending => matches!(r.kind, RelKind::Incoming | RelKind::Outgoing),
                FriendTab::Blocked => r.kind == RelKind::Blocked || r.ignored,
            })
            .map(|r| (self.display_name(r.id).to_lowercase(), r.id))
            .filter(|(name, id)| q.is_empty() || name.contains(&q) || self.people.get(id).is_some_and(|p| p.username.to_lowercase().contains(&q)))
            .collect();
        // Demandes reçues en premier dans « En attente ».
        let incoming_first = |id: &u64| u8::from(!(tab == FriendTab::Pending && self.relations.get(id).is_some_and(|r| r.kind == RelKind::Incoming)));
        out.sort_by(|a, b| (incoming_first(&a.1), &a.0, a.1).cmp(&(incoming_first(&b.1), &b.0, b.1)));
        out.into_iter().map(|(_, id)| id).collect()
    }

    fn remember(&mut self, p: Person) {
        if self.people.len() < MAX_PEOPLE || self.people.contains_key(&p.id) {
            self.people.insert(p.id, p);
        }
    }

    pub(super) fn notify(&mut self, n: RelationsNotice) {
        if self.notices.len() >= MAX_NOTICES {
            self.notices.pop_front();
        }
        self.notices.push_back(n);
    }

    pub fn take_notice(&mut self) -> Option<RelationsNotice> {
        self.notices.pop_front()
    }

    /// Données du READY : tout ce qui était connu est remplacé.
    pub(super) fn reset(&mut self, relations: Vec<Relation>, people: Vec<Person>, dms: Vec<DmInfo>) {
        self.clear();
        for p in people {
            self.remember(p);
        }
        for r in relations {
            self.add(r);
        }
        for d in dms {
            self.dms.insert(d.id, d);
        }
    }

    /// Demandes d'ami arrivées depuis le dernier appel (RELATIONSHIP_ADD
    /// reçu en cours de session ; celles du READY n'y sont pas).
    pub fn take_incoming(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.fresh_incoming)
    }

    pub(super) fn add(&mut self, mut r: Relation) {
        if let Some(p) = r.user.take() {
            self.remember(p);
        }
        if r.kind == RelKind::Incoming && self.relations.get(&r.id).is_none_or(|old| old.kind != RelKind::Incoming) && self.fresh_incoming.len() < MAX_NOTICES {
            self.fresh_incoming.push(r.id);
        }
        self.relations.insert(r.id, r);
        self.bump();
    }

    pub(super) fn update(&mut self, id: u64, kind: Option<RelKind>, nickname: Option<Option<String>>, ignored: Option<bool>) {
        if let Some(r) = self.relations.get_mut(&id) {
            if let Some(k) = kind {
                r.kind = k;
            }
            if let Some(n) = nickname {
                r.nickname = n;
            }
            if let Some(i) = ignored {
                r.ignored = i;
            }
            self.bump();
        }
    }

    /// Écriture acceptée par Discord. Une relation connue garde son surnom et sa
    /// personne ; une acceptation ne vaut que pour une demande reçue (la même
    /// requête, sans demande, en envoie une).
    pub(super) fn done(&mut self, id: u64, what: Done) {
        let new = |kind, ignored| Relation { id, kind, nickname: None, ignored, user: None };
        match (what, self.relations.get_mut(&id)) {
            (Done::Blocked, Some(r)) => r.kind = RelKind::Blocked,
            (Done::Blocked, None) => {
                self.relations.insert(id, new(RelKind::Blocked, false));
            }
            (Done::Accepted, Some(r)) if r.kind == RelKind::Incoming => r.kind = RelKind::Friend,
            (Done::Accepted, _) => return,
            (Done::Ignored, Some(r)) => r.ignored = true,
            (Done::Ignored, None) => {
                self.relations.insert(id, new(RelKind::Other, true));
            }
        }
        self.bump();
    }

    pub(super) fn remove(&mut self, id: u64) {
        if self.relations.remove(&id).is_some() {
            self.bump();
        }
    }


    /// `known_recipients` : faux si le paquet ne listait pas les destinataires
    /// (CHANNEL_UPDATE partiel) : ceux déjà connus sont gardés.
    pub(super) fn upsert_dm(&mut self, mut info: DmInfo, people: Vec<Person>, known_recipients: bool) {
        for p in people {
            self.remember(p);
        }
        if !known_recipients && let Some(old) = self.dms.get(&info.id) {
            info.recipients = old.recipients.clone();
            info.owner = info.owner.or(old.owner);
        }
        self.dms.insert(info.id, info);
        self.bump();
    }

    pub(super) fn forget_dm(&mut self, channel: u64) {
        if self.dms.remove(&channel).is_some() {
            self.bump();
        }
    }

    pub(super) fn recipient_add(&mut self, channel: u64, user: Person) {
        let id = user.id;
        self.remember(user);
        if let Some(d) = self.dms.get_mut(&channel)
            && !d.recipients.contains(&id)
        {
            d.recipients.push(id);
            self.bump();
        }
    }

    pub(super) fn recipient_remove(&mut self, channel: u64, user: u64) {
        if let Some(d) = self.dms.get_mut(&channel) {
            d.recipients.retain(|r| *r != user);
            self.bump();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(id: u64, name: &str) -> Person {
        Person { id, name: name.into(), username: name.to_lowercase(), avatar: Avatar::default_for(id) }
    }

    fn rel(id: u64, kind: RelKind, name: &str) -> Relation {
        Relation { id, kind, nickname: None, ignored: false, user: Some(person(id, name)) }
    }

    fn sample() -> Relations {
        let mut s = Relations::default();
        s.reset(
            vec![rel(1, RelKind::Friend, "Zoé"), rel(2, RelKind::Friend, "Alice"), rel(3, RelKind::Incoming, "Bob"), rel(4, RelKind::Outgoing, "Ada"), rel(5, RelKind::Blocked, "Troll")],
            vec![],
            vec![DmInfo { id: 50, group: false, owner: None, recipients: vec![2], ..Default::default() }, DmInfo { id: 51, group: true, owner: Some(9), recipients: vec![2, 1], ..Default::default() }],
        );
        s
    }

    #[test]
    fn tabs_filter_and_sort() {
        let s = sample();
        assert_eq!(s.list(FriendTab::All, "", |_| false), vec![2, 1]);
        assert_eq!(s.list(FriendTab::Online, "", |_| false), Vec::<u64>::new());
        // En ligne : selon la présence fournie (1 absent, 99 inconnu des relations).
        assert_eq!(s.list(FriendTab::Online, "", |id| id == 1 || id == 99), vec![1]);
        // Demandes reçues d'abord.
        assert_eq!(s.list(FriendTab::Pending, "", |_| false), vec![3, 4]);
        assert_eq!(s.list(FriendTab::Blocked, "", |_| false), vec![5]);
        assert_eq!(s.incoming(), 1);
        // Recherche sans casse, sur le nom ou le nom d'utilisateur.
        assert_eq!(s.list(FriendTab::All, "ZO", |_| false), vec![1]);
        assert_eq!(s.list(FriendTab::All, "xyz", |_| false), Vec::<u64>::new());
    }

    #[test]
    fn events_change_relations_and_revision() {
        let mut s = sample();
        let r0 = s.rev();
        // Demande acceptée : RELATIONSHIP_ADD avec le nouveau type.
        s.add(Relation { user: None, ..rel(3, RelKind::Friend, "Bob") });
        assert!(s.rev() != r0);
        assert_eq!(s.list(FriendTab::All, "", |_| false), vec![2, 3, 1]);
        assert_eq!(s.display_name(3), "Bob", "la personne connue est gardée");
        s.update(3, None, Some(Some("Bobby".into())), Some(true));
        assert_eq!(s.display_name(3), "Bobby");
        assert_eq!(s.list(FriendTab::Blocked, "", |_| false), vec![3, 5], "ignoré : listé avec les bloqués");
        // Écritures acceptées : inconnu bloqué ou ignoré, demande reçue acceptée.
        let mut d = sample();
        d.done(90, Done::Blocked);
        d.done(91, Done::Ignored);
        assert_eq!(d.relation(90).map(|r| (r.kind, r.ignored)), Some((RelKind::Blocked, false)));
        assert_eq!(d.relation(91).map(|r| (r.kind, r.ignored)), Some((RelKind::Other, true)));
        d.done(92, Done::Accepted);
        assert!(d.relation(92).is_none(), "sans demande reçue : rien");
        d.add(Relation { id: 92, kind: RelKind::Incoming, nickname: None, ignored: false, user: None });
        d.done(92, Done::Accepted);
        assert_eq!(d.relation(92).map(|r| r.kind), Some(RelKind::Friend));
        // Type porté par la mise à jour : un blocage levé ne reste pas « bloqué ».
        s.update(5, Some(RelKind::Other), None, None);
        assert_eq!(s.relation(5).map(|r| r.kind), Some(RelKind::Other));
        s.remove(3);
        assert!(s.relation(3).is_none());
        // Mise à jour d'une relation inconnue : rien.
        let r1 = s.rev();
        s.update(77, None, None, Some(true));
        assert_eq!(s.rev(), r1);
    }

    #[test]
    fn dms_and_members() {
        let mut s = sample();
        assert_eq!(s.dm_with(2), Some(50));
        assert_eq!(s.dm_with(1), None, "seul un MP à deux compte");
        s.recipient_add(51, person(7, "Eve"));
        assert_eq!(s.dm_info(51).map(|d| d.recipients.clone()), Some(vec![2, 1, 7]));
        s.recipient_remove(51, 2);
        assert_eq!(s.dm_info(51).map(|d| d.recipients.clone()), Some(vec![1, 7]));
        // Paquet partiel : les destinataires connus restent.
        s.upsert_dm(DmInfo { id: 51, group: true, owner: None, recipients: vec![], ..Default::default() }, vec![], false);
        assert_eq!(s.dm_info(51).map(|d| (d.recipients.clone(), d.owner)), Some((vec![1, 7], Some(9))));
        s.forget_dm(51);
        assert!(s.dm_info(51).is_none());
    }

    #[test]
    fn notices_are_bounded() {
        let mut s = Relations::default();
        for i in 0..20 {
            s.notify(RelationsNotice::RequestSent { username: i.to_string() });
        }
        let mut n = 0;
        while s.take_notice().is_some() {
            n += 1;
        }
        assert_eq!(n, MAX_NOTICES);
    }
}
