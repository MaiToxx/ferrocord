//! Commandes et composants des applications (lot T11) : index des commandes
//! d'un salon, interactions envoyées et leur suivi (`INTERACTION_CREATE`,
//! `_SUCCESS`, `_FAILURE`), fenêtres modales des bots, autocomplétion
//! fournie par un bot, composants affichés dans les messages.
//!
//! Formes vérifiées sur docs.discord.food (« Application Commands »,
//! « Receiving and Responding », « Message Components », « Gateway Events »,
//! « Message » : drapeaux `EPHEMERAL`, `LOADING`, `IS_COMPONENTS_V2`).
//! Chaque envoi suit un geste de l'utilisateur ; rien n'est jamais renvoyé
//! automatiquement (un échec est montré, l'utilisateur refait le geste).

use super::model::Attachment;
use super::notice::Problem;
use super::social::Emoji;
use std::collections::HashMap;

/// Index gardés au plus (serveurs, MP, compte) : au-delà, le plus ancien part.
pub const MAX_INDEXES: usize = 32;
/// Un index plus ancien est redemandé à la prochaine ouverture de la liste.
pub const INDEX_TTL_MS: i64 = 5 * 60_000;
/// Après un échec, l'index n'est pas redemandé avant ce délai.
pub const INDEX_RETRY_MS: i64 = 30_000;
/// Interactions suivies au plus (envoyées, en attente ou en échec).
pub const MAX_PENDING: usize = 32;
/// Interactions parties d'ici dont on se souvient (nonce, identifiant) pour
/// reconnaître la fenêtre modale qui leur répond ; les plus anciennes sont
/// oubliées.
pub const MAX_MINE: usize = 32;
/// Sans nouvelle de Discord après ce délai, l'interaction est tenue pour
/// échouée (Discord envoie lui-même `INTERACTION_FAILURE` après 3 s).
pub const PENDING_TIMEOUT_MS: i64 = 15_000;
/// Propositions d'autocomplétion gardées au plus (Discord en envoie 25).
pub const MAX_CHOICES: usize = 25;

/// Drapeaux de message (docs.discord.food, « Message Flags »).
pub const FLAG_EPHEMERAL: u64 = 1 << 6;
pub const FLAG_LOADING: u64 = 1 << 7;
pub const FLAG_COMPONENTS_V2: u64 = 1 << 15;

/// Identifiant de la session de passerelle (envoyé avec chaque interaction
/// pour en recevoir les événements). Jamais affiché ni journalisé.
#[derive(Clone, PartialEq, Eq)]
pub struct SessionId(pub String);

impl std::fmt::Debug for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionId(…)")
    }
}

/// D'où vient un index de commandes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Applications installées sur le serveur.
    Guild(u64),
    /// MP ou groupe : application du bot, intégrations du groupe.
    Channel(u64),
    /// Applications installées sur mon compte.
    User,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppInfo {
    pub id: u64,
    pub name: String,
    pub bot_id: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandKind {
    /// Commande slash.
    ChatInput,
    /// Menu contextuel d'un utilisateur.
    User,
    /// Menu contextuel d'un message.
    Message,
    Other(u8),
}

impl CommandKind {
    pub fn from_code(c: u8) -> Self {
        match c {
            1 => CommandKind::ChatInput,
            2 => CommandKind::User,
            3 => CommandKind::Message,
            n => CommandKind::Other(n),
        }
    }

    pub fn code(self) -> u8 {
        match self {
            CommandKind::ChatInput => 1,
            CommandKind::User => 2,
            CommandKind::Message => 3,
            CommandKind::Other(n) => n,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionKind {
    SubCommand,
    Group,
    String,
    Integer,
    Boolean,
    User,
    Channel,
    Role,
    Mentionable,
    Number,
    Attachment,
    Unknown(u8),
}

impl OptionKind {
    pub fn from_code(c: u8) -> Self {
        use OptionKind::*;
        match c {
            1 => SubCommand,
            2 => Group,
            3 => String,
            4 => Integer,
            5 => Boolean,
            6 => User,
            7 => Channel,
            8 => Role,
            9 => Mentionable,
            10 => Number,
            11 => Attachment,
            n => Unknown(n),
        }
    }

    pub fn code(self) -> u8 {
        use OptionKind::*;
        match self {
            SubCommand => 1,
            Group => 2,
            String => 3,
            Integer => 4,
            Boolean => 5,
            User => 6,
            Channel => 7,
            Role => 8,
            Mentionable => 9,
            Number => 10,
            Attachment => 11,
            Unknown(n) => n,
        }
    }
}

/// Valeur d'un choix proposé (choix fixes ou autocomplétion du bot).
#[derive(Clone, Debug, PartialEq)]
pub enum ChoiceValue {
    Str(String),
    Int(i64),
    Num(f64),
}

impl ChoiceValue {
    /// Texte mis dans le champ quand ce choix est pris.
    pub fn text(&self) -> String {
        match self {
            ChoiceValue::Str(s) => s.clone(),
            ChoiceValue::Int(n) => n.to_string(),
            ChoiceValue::Num(x) => x.to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub name: String,
    pub value: ChoiceValue,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CmdOption {
    pub kind: OptionKind,
    pub name: String,
    pub description: String,
    pub required: bool,
    pub choices: Vec<Choice>,
    /// Sous-commandes d'un groupe, options d'une sous-commande.
    pub options: Vec<CmdOption>,
    /// Types de salons acceptés (option salon) ; vide : tous.
    pub channel_types: Vec<u8>,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    pub min_length: Option<u32>,
    pub max_length: Option<u32>,
    /// Le bot propose lui-même les valeurs pendant la frappe.
    pub autocomplete: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    pub id: u64,
    pub app_id: u64,
    pub version: u64,
    pub kind: CommandKind,
    pub name: String,
    pub description: String,
    pub options: Vec<CmdOption>,
}

impl Command {
    /// Feuilles de la commande : chaque sous-commande (dans son groupe)
    /// est proposée à part, comme dans l'application officielle. Chemin vide :
    /// la commande elle-même.
    pub fn leaves(&self) -> Vec<(Vec<&str>, &str, &[CmdOption])> {
        let subs = |o: &CmdOption| matches!(o.kind, OptionKind::SubCommand | OptionKind::Group);
        if !self.options.iter().any(subs) {
            return vec![(Vec::new(), self.description.as_str(), &self.options)];
        }
        let mut out = Vec::new();
        for o in &self.options {
            match o.kind {
                OptionKind::SubCommand => out.push((vec![o.name.as_str()], o.description.as_str(), &o.options[..])),
                OptionKind::Group => {
                    for s in o.options.iter().filter(|s| s.kind == OptionKind::SubCommand) {
                        out.push((vec![o.name.as_str(), s.name.as_str()], s.description.as_str(), &s.options[..]));
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// Options de la feuille désignée par `path`.
    pub fn leaf_options(&self, path: &[String]) -> Option<&[CmdOption]> {
        let mut opts = &self.options[..];
        for name in path {
            opts = &opts.iter().find(|o| o.name == *name && matches!(o.kind, OptionKind::SubCommand | OptionKind::Group))?.options;
        }
        Some(opts)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Index {
    pub apps: Vec<AppInfo>,
    pub commands: Vec<Command>,
}

/// Valeur donnée à une option par l'utilisateur.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Str(String),
    Int(i64),
    Num(f64),
    Bool(bool),
    /// Utilisateur, salon, rôle ou « mentionnable ».
    Id(u64),
    /// Option pièce jointe : rang du fichier dans `Outgoing::files`.
    Attachment(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Filled {
    pub name: String,
    pub kind: OptionKind,
    pub value: Value,
}

// ====================================================================== composants

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ButtonStyle {
    Primary,
    Secondary,
    Success,
    Danger,
    Link,
    Premium,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Button {
    pub style: ButtonStyle,
    pub label: String,
    pub emoji: Option<Emoji>,
    pub custom_id: Option<String>,
    pub url: Option<String>,
    pub disabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SelectKind {
    String,
    User,
    Role,
    Mentionable,
    Channel,
}

impl SelectKind {
    /// Type de composant (docs.discord.food, « Component Types »).
    pub fn code(self) -> u8 {
        match self {
            SelectKind::String => 3,
            SelectKind::User => 5,
            SelectKind::Role => 6,
            SelectKind::Mentionable => 7,
            SelectKind::Channel => 8,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SelectOption {
    pub label: String,
    pub value: String,
    pub description: Option<String>,
    pub emoji: Option<Emoji>,
    pub default: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Select {
    pub kind: SelectKind,
    pub custom_id: String,
    pub placeholder: Option<String>,
    pub min_values: u8,
    pub max_values: u8,
    pub options: Vec<SelectOption>,
    pub disabled: bool,
    pub required: bool,
    pub channel_types: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextInput {
    pub custom_id: String,
    pub paragraph: bool,
    pub label: String,
    pub min_length: Option<u32>,
    pub max_length: Option<u32>,
    pub required: bool,
    pub value: String,
    pub placeholder: Option<String>,
}

/// Composant d'un message ou d'une fenêtre modale. Les types inconnus sont
/// gardés vides (`Unknown`) : un message n'est jamais rendu illisible.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Component {
    Row(Vec<Component>),
    Button(Button),
    Select(Select),
    TextInput(TextInput),
    /// Textes (1 à 3) et accessoire à droite (vignette ou bouton).
    Section { texts: Vec<Component>, accessory: Option<Box<Component>> },
    /// Texte en markdown (analysé avec le message, `Doc::components`).
    Text(String),
    Thumbnail(Box<Attachment>),
    Gallery(Vec<Attachment>),
    File(Box<Attachment>),
    Separator { divider: bool, large: bool },
    Container { accent: Option<u32>, spoiler: bool, children: Vec<Component> },
    /// Étiquette d'un champ de fenêtre modale.
    Label { label: String, description: Option<String>, child: Box<Component> },
    Unknown,
}

impl Component {
    /// Textes markdown dans l'ordre d'affichage (parcours en profondeur) :
    /// l'analyse et le rendu les numérotent de la même façon.
    pub fn texts<'a>(list: &'a [Component], out: &mut Vec<&'a str>) {
        for c in list {
            match c {
                Component::Text(s) => out.push(s),
                Component::Row(v) | Component::Container { children: v, .. } => Self::texts(v, out),
                Component::Section { texts, .. } => Self::texts(texts, out),
                _ => {}
            }
        }
    }

    /// Identifiants des pièces jointes montrées par les composants (elles ne
    /// sont pas répétées sous le message).
    pub fn media_ids(list: &[Component], out: &mut Vec<u64>) {
        for c in list {
            match c {
                Component::Thumbnail(a) | Component::File(a) => out.push(a.id),
                Component::Gallery(v) => out.extend(v.iter().map(|a| a.id)),
                Component::Row(v) | Component::Container { children: v, .. } => Self::media_ids(v, out),
                Component::Section { texts, accessory } => {
                    Self::media_ids(texts, out);
                    if let Some(a) = accessory {
                        Self::media_ids(std::slice::from_ref(a), out);
                    }
                }
                _ => {}
            }
        }
    }
}

/// Qui a lancé l'interaction dont ce message est la réponse.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct InteractionMeta {
    pub user_id: u64,
    pub user_name: String,
    /// Nom de la commande (sous-commandes comprises), pour une commande.
    pub command: Option<String>,
}

/// Ce qui fait d'un message une réponse d'application : composants,
/// réponse éphémère ou différée (« réfléchit… »), interaction d'origine.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct AppMeta {
    pub flags: u64,
    pub components: Vec<Component>,
    pub interaction: Option<InteractionMeta>,
    pub application_id: Option<u64>,
}

impl AppMeta {
    pub fn ephemeral(&self) -> bool {
        self.flags & FLAG_EPHEMERAL != 0
    }

    pub fn loading(&self) -> bool {
        self.flags & FLAG_LOADING != 0
    }

    pub fn v2(&self) -> bool {
        self.flags & FLAG_COMPONENTS_V2 != 0
    }

    /// Rien d'utile : le message n'en porte pas.
    pub fn is_empty(&self) -> bool {
        self.components.is_empty() && self.interaction.is_none() && !self.ephemeral() && !self.loading()
    }
}

// ====================================================================== fenêtres modales

#[derive(Clone, Debug, PartialEq)]
pub struct Modal {
    /// Interaction qui a ouvert la fenêtre (renvoyée avec la réponse).
    pub id: u64,
    pub channel_id: u64,
    pub custom_id: String,
    pub title: String,
    pub app_id: u64,
    pub app_name: String,
    pub components: Vec<Component>,
    /// Nonce de l'interaction de cette session à laquelle la fenêtre répond.
    pub nonce: Option<u64>,
}

/// Valeur d'un champ de fenêtre modale, avec son conteneur d'origine.
#[derive(Clone, Debug, PartialEq)]
pub struct ModalValue {
    /// Type du composant qui porte la valeur (4 texte, 3 liste…).
    pub kind: u8,
    pub custom_id: String,
    /// Texte saisi, ou valeurs choisies (listes).
    pub text: Option<String>,
    pub values: Vec<String>,
    /// Conteneur : rangée (1, forme d'avant) ou étiquette (18).
    pub wrapper: u8,
}

// ====================================================================== commandes et événements

/// Interaction prête à partir (`POST /interactions`).
#[derive(Clone, Debug, PartialEq)]
pub struct Outgoing {
    pub nonce: u64,
    pub session: SessionId,
    pub application_id: u64,
    pub guild_id: Option<u64>,
    pub channel_id: u64,
    pub kind: OutKind,
    /// Fichiers des options pièce jointe (envoi en `multipart/form-data`).
    pub files: Vec<super::upload::OutFile>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum OutKind {
    /// Type 2 : commande slash ou menu contextuel (`target_id`).
    Command { id: u64, version: u64, name: String, kind: CommandKind, path: Vec<String>, options: Vec<Filled>, target_id: Option<u64> },
    /// Type 4 : autocomplétion ; `focused` : l'option en cours de frappe,
    /// avec le texte tapé (`Value::Str`, quel que soit son type).
    Autocomplete { id: u64, version: u64, name: String, path: Vec<String>, options: Vec<Filled>, focused: Filled },
    /// Type 3 : bouton ou menu de sélection d'un message.
    Component { message_id: u64, message_flags: u64, component_type: u8, custom_id: String, values: Option<Vec<String>> },
    /// Type 5 : réponse à une fenêtre modale.
    ModalSubmit { interaction_id: u64, custom_id: String, values: Vec<ModalValue> },
}

impl OutKind {
    pub fn code(&self) -> u8 {
        match self {
            OutKind::Command { .. } => 2,
            OutKind::Component { .. } => 3,
            OutKind::Autocomplete { .. } => 4,
            OutKind::ModalSubmit { .. } => 5,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum InteractionCmd {
    Index { scope: Scope },
    Send(Box<Outgoing>),
}

// `Cmd` est comparé en entier (tests) ; les nombres saisis sont toujours finis
// (`f64::is_finite` vérifié à la saisie), donc l'égalité est réflexive.
impl Eq for InteractionCmd {}

#[derive(Clone, Debug, PartialEq)]
pub enum InteractionEvent {
    /// Nouvelle session de passerelle (READY).
    Session(SessionId),
    Index { scope: Scope, index: Index },
    IndexFailed { scope: Scope },
    /// Discord a accepté la requête (204).
    Sent { nonce: u64 },
    SendFailed { nonce: u64, problem: Problem },
    Created { nonce: u64, id: u64 },
    Succeeded { nonce: u64, id: u64 },
    Failed { nonce: u64, id: u64, reason: u32 },
    Modal(Box<Modal>),
    /// `nonce` : celui de la demande d'autocomplétion.
    Autocomplete { nonce: u64, choices: Vec<Choice> },
    /// MESSAGE_UPDATE qui change composants, drapeaux ou interaction.
    /// `edited` : `edited_timestamp` du paquet.
    MessageApp { channel_id: u64, id: u64, app: Option<Box<AppMeta>>, edited: bool },
}

// ====================================================================== suivi

#[derive(Clone, Debug, PartialEq)]
pub enum What {
    Command(String),
    Component { message_id: u64, custom_id: String },
    Modal(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum PendingState {
    Sending,
    /// Requête acceptée : en attente de la réponse de l'application.
    Waiting,
    Failed(Failure),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Failure {
    /// Pas de réponse à temps (`INTERACTION_FAILURE` 2, ou délai local).
    NoResponse,
    /// Autre raison de `INTERACTION_FAILURE`.
    Code(u32),
    /// Requête refusée ou impossible.
    Problem(Problem),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pending {
    pub nonce: u64,
    pub channel_id: u64,
    pub app_name: String,
    pub what: What,
    pub id: Option<u64>,
    pub since: i64,
    pub state: PendingState,
}

#[derive(Clone, Debug, PartialEq)]
enum Slot {
    Loading,
    Ready(Index),
    Failed,
}

/// État des interactions de la session (appartient au magasin).
#[derive(Debug, Default)]
pub struct Interactions {
    session: Option<SessionId>,
    indexes: HashMap<Scope, (Slot, i64)>,
    pending: Vec<Pending>,
    /// Interactions envoyées par cette session : (nonce, identifiant connu).
    mine: std::collections::VecDeque<(u64, Option<u64>)>,
    last_nonce: u64,
    /// Fenêtre modale ouverte par un bot, en attente d'être montrée.
    pub modal: Option<Box<Modal>>,
    /// Dernière demande d'autocomplétion et ses propositions.
    ac_nonce: Option<u64>,
    ac_choices: Option<Vec<Choice>>,
    /// Change quand un index change (l'interface refait ses listes).
    pub rev: u64,
}

impl Interactions {
    pub fn clear(&mut self) {
        *self = Interactions { last_nonce: self.last_nonce, ..Interactions::default() };
    }

    pub fn session(&self) -> Option<&SessionId> {
        self.session.as_ref()
    }

    /// Nonce d'une interaction : snowflake de l'instant, strictement croissant.
    pub fn next_nonce(&mut self, now_ms: i64) -> u64 {
        let ms = u64::try_from(now_ms).unwrap_or(0).saturating_sub(super::model::DISCORD_EPOCH);
        self.last_nonce = (ms << 22).max(self.last_nonce + 1);
        self.last_nonce
    }

    /// Index à demander pour `scope` (absent, périmé, ou échec ancien).
    pub fn wants(&self, scope: Scope, now_ms: i64) -> bool {
        match self.indexes.get(&scope) {
            None => true,
            Some((Slot::Loading, _)) => false,
            Some((Slot::Ready(_), at)) => now_ms - at > INDEX_TTL_MS,
            Some((Slot::Failed, at)) => now_ms - at > INDEX_RETRY_MS,
        }
    }

    pub fn mark_loading(&mut self, scope: Scope, now_ms: i64) {
        // Un index déjà là reste montré pendant qu'il est redemandé.
        if let Some((Slot::Ready(_), at)) = self.indexes.get_mut(&scope) {
            *at = now_ms;
            return;
        }
        self.bound();
        self.indexes.insert(scope, (Slot::Loading, now_ms));
    }

    fn bound(&mut self) {
        while self.indexes.len() >= MAX_INDEXES {
            let Some(old) = self.indexes.iter().min_by_key(|(_, (_, at))| *at).map(|(s, _)| *s) else { break };
            self.indexes.remove(&old);
        }
    }

    pub fn index(&self, scope: Scope) -> Option<&Index> {
        match self.indexes.get(&scope) {
            Some((Slot::Ready(i), _)) => Some(i),
            _ => None,
        }
    }

    pub fn loading(&self, scope: Scope) -> bool {
        matches!(self.indexes.get(&scope), Some((Slot::Loading, _)))
    }

    pub fn pending(&self) -> &[Pending] {
        &self.pending
    }

    /// Ce bouton ou ce menu attend déjà sa réponse : un second clic n'envoie rien.
    pub fn component_busy(&self, message_id: u64, custom_id: &str) -> bool {
        self.pending.iter().any(|p| {
            matches!(p.state, PendingState::Sending | PendingState::Waiting) && matches!(&p.what, What::Component { message_id: m, custom_id: c } if *m == message_id && c == custom_id)
        })
    }

    pub fn track(&mut self, p: Pending) {
        // Les échecs anciens partent d'abord.
        while self.pending.len() >= MAX_PENDING {
            let i = self.pending.iter().position(|p| matches!(p.state, PendingState::Failed(_))).unwrap_or(0);
            self.pending.remove(i);
        }
        if self.mine.len() >= MAX_MINE {
            self.mine.pop_front();
        }
        self.mine.push_back((p.nonce, None));
        self.pending.push(p);
    }

    pub fn dismiss(&mut self, nonce: u64) {
        self.pending.retain(|p| p.nonce != nonce);
    }

    /// Propositions du bot pour la dernière demande d'autocomplétion.
    pub fn choices(&self) -> Option<&[Choice]> {
        self.ac_choices.as_deref()
    }

    pub fn ask_choices(&mut self, nonce: u64) {
        self.ac_nonce = Some(nonce);
    }

    pub fn forget_choices(&mut self) {
        self.ac_nonce = None;
        self.ac_choices = None;
    }

    /// Délais dépassés : l'interaction est tenue pour échouée. Renvoie la
    /// prochaine échéance (pour un seul réveil).
    pub fn expire(&mut self, now_ms: i64) -> Option<i64> {
        let mut next = None::<i64>;
        for p in &mut self.pending {
            if matches!(p.state, PendingState::Sending | PendingState::Waiting) {
                let end = p.since + PENDING_TIMEOUT_MS;
                if end <= now_ms {
                    p.state = PendingState::Failed(Failure::NoResponse);
                } else {
                    next = Some(next.map_or(end, |n| n.min(end)));
                }
            }
        }
        next
    }

    /// Événement reçu. Renvoie `true` si l'interface a de quoi se redessiner.
    pub fn apply(&mut self, e: InteractionEvent, now_ms: i64) {
        match e {
            InteractionEvent::Session(s) => {
                // Session neuve : les interactions de la précédente n'auront plus de nouvelles.
                let last = self.last_nonce;
                *self = Interactions { session: Some(s), last_nonce: last, rev: self.rev + 1, ..Interactions::default() };
            }
            InteractionEvent::Index { scope, index } => {
                if !self.indexes.contains_key(&scope) {
                    self.bound();
                }
                self.indexes.insert(scope, (Slot::Ready(index), now_ms));
                self.rev += 1;
            }
            InteractionEvent::IndexFailed { scope } => {
                if !matches!(self.indexes.get(&scope), Some((Slot::Ready(_), _))) {
                    self.indexes.insert(scope, (Slot::Failed, now_ms));
                    self.rev += 1;
                }
            }
            InteractionEvent::Sent { nonce } => self.set(nonce, |p| {
                if p.state == PendingState::Sending {
                    p.state = PendingState::Waiting;
                }
            }),
            InteractionEvent::SendFailed { nonce, problem } => {
                if self.ac_nonce == Some(nonce) {
                    self.ac_nonce = None;
                }
                self.set(nonce, |p| p.state = PendingState::Failed(Failure::Problem(problem)));
            }
            InteractionEvent::Created { nonce, id } => {
                if let Some(m) = self.mine.iter_mut().find(|m| m.0 == nonce) {
                    m.1 = Some(id);
                }
                self.set(nonce, |p| p.id = Some(id));
            }
            InteractionEvent::Succeeded { nonce, id } => {
                self.pending.retain(|p| p.nonce != nonce && (p.id != Some(id) || id == 0));
            }
            InteractionEvent::Failed { nonce, id, reason } => {
                let failure = if reason == 2 { Failure::NoResponse } else { Failure::Code(reason) };
                for p in self.pending.iter_mut().filter(|p| p.nonce == nonce || (id != 0 && p.id == Some(id))) {
                    p.state = PendingState::Failed(failure.clone());
                }
            }
            InteractionEvent::Modal(m) => {
                // Seulement en réponse à une interaction partie d'ici : la
                // passerelle relaie aussi celles des autres appareils du
                // compte, dont la fenêtre n'a pas à s'ouvrir ici.
                // L'interaction peut avoir déjà quitté `pending` (succès ou
                // échec reçu avant la fenêtre) : on regarde `mine`.
                if !self.mine.iter().any(|&(n, id)| m.nonce == Some(n) || id == Some(m.id)) {
                    return;
                }
                // La fenêtre est la réponse : rien n'est plus attendu.
                self.pending.retain(|p| m.nonce != Some(p.nonce) && p.id != Some(m.id));
                self.modal = Some(m);
            }
            InteractionEvent::Autocomplete { nonce, mut choices } => {
                if self.ac_nonce == Some(nonce) {
                    choices.truncate(MAX_CHOICES);
                    self.ac_choices = Some(choices);
                }
            }
            InteractionEvent::MessageApp { .. } => {}
        }
    }

    fn set(&mut self, nonce: u64, f: impl FnOnce(&mut Pending)) {
        if let Some(p) = self.pending.iter_mut().find(|p| p.nonce == nonce) {
            f(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(nonce: u64, since: i64) -> Pending {
        Pending { nonce, channel_id: 1, app_name: "Bot".into(), what: What::Command("ping".into()), id: None, since, state: PendingState::Sending }
    }

    #[test]
    fn session_id_is_never_printed() {
        assert_eq!(format!("{:?}", SessionId("secret".into())), "SessionId(…)");
        let e = InteractionEvent::Session(SessionId("secret".into()));
        assert!(!format!("{e:?}").contains("secret"));
    }

    #[test]
    fn leaves_split_subcommands_and_groups() {
        let opt = |kind, name: &str, options| CmdOption {
            kind,
            name: name.into(),
            description: format!("d-{name}"),
            required: false,
            choices: Vec::new(),
            options,
            channel_types: Vec::new(),
            min_value: None,
            max_value: None,
            min_length: None,
            max_length: None,
            autocomplete: false,
        };
        let leaf = opt(OptionKind::String, "texte", Vec::new());
        let cmd = Command {
            id: 1,
            app_id: 2,
            version: 3,
            kind: CommandKind::ChatInput,
            name: "role".into(),
            description: "Rôles".into(),
            options: vec![opt(OptionKind::SubCommand, "voir", vec![leaf.clone()]), opt(OptionKind::Group, "gerer", vec![opt(OptionKind::SubCommand, "ajouter", vec![leaf.clone()])])],
        };
        let l = cmd.leaves();
        assert_eq!(l.iter().map(|(p, d, _)| (p.join(" "), *d)).collect::<Vec<_>>(), vec![("voir".into(), "d-voir"), ("gerer ajouter".into(), "d-ajouter")]);
        assert_eq!(cmd.leaf_options(&["gerer".into(), "ajouter".into()]).map(|o| o.len()), Some(1));
        assert!(cmd.leaf_options(&["inconnue".into()]).is_none());
        let flat = Command { options: vec![leaf], ..cmd };
        assert_eq!(flat.leaves()[0].0.len(), 0);
    }

    #[test]
    fn pending_lifecycle_and_timeout() {
        let mut s = Interactions::default();
        s.track(pending(10, 0));
        s.apply(InteractionEvent::Sent { nonce: 10 }, 1);
        assert_eq!(s.pending()[0].state, PendingState::Waiting);
        s.apply(InteractionEvent::Created { nonce: 10, id: 77 }, 2);
        assert_eq!(s.pending()[0].id, Some(77));
        s.apply(InteractionEvent::Succeeded { nonce: 10, id: 77 }, 3);
        assert!(s.pending().is_empty());

        s.track(pending(11, 0));
        s.apply(InteractionEvent::Failed { nonce: 11, id: 0, reason: 2 }, 1);
        assert_eq!(s.pending()[0].state, PendingState::Failed(Failure::NoResponse));
        s.dismiss(11);

        s.track(pending(12, 1000));
        assert_eq!(s.expire(1000), Some(1000 + PENDING_TIMEOUT_MS));
        assert_eq!(s.expire(1000 + PENDING_TIMEOUT_MS), None);
        assert_eq!(s.pending()[0].state, PendingState::Failed(Failure::NoResponse));
        // Un événement d'une autre interaction ne change rien.
        s.apply(InteractionEvent::Succeeded { nonce: 99, id: 98 }, 0);
        assert_eq!(s.pending().len(), 1);
    }

    #[test]
    fn modal_only_for_our_own_interactions() {
        let modal = |id, nonce| {
            Box::new(Modal { id, channel_id: 1, custom_id: "c".into(), title: "T".into(), app_id: 2, app_name: "Bot".into(), components: Vec::new(), nonce })
        };
        let mut s = Interactions::default();
        // Fenêtre d'une interaction d'un autre appareil : ignorée.
        s.apply(InteractionEvent::Modal(modal(500, Some(42))), 0);
        s.apply(InteractionEvent::Modal(modal(500, None)), 0);
        assert!(s.modal.is_none());
        // La nôtre, par le nonce.
        s.track(pending(10, 0));
        s.apply(InteractionEvent::Modal(modal(501, Some(10))), 1);
        assert_eq!(s.modal.as_ref().map(|m| m.id), Some(501));
        assert!(s.pending().is_empty());
        // Par l'identifiant (nonce absent), même après le succès.
        s.modal = None;
        s.track(pending(11, 0));
        s.apply(InteractionEvent::Created { nonce: 11, id: 777 }, 1);
        s.apply(InteractionEvent::Succeeded { nonce: 11, id: 777 }, 2);
        s.apply(InteractionEvent::Modal(modal(777, None)), 3);
        assert_eq!(s.modal.as_ref().map(|m| m.id), Some(777));
        // Mémoire bornée : les plus anciennes sont oubliées.
        s.modal = None;
        for n in 100..(100 + MAX_MINE as u64) {
            s.track(pending(n, 0));
        }
        s.apply(InteractionEvent::Modal(modal(9, Some(10))), 4);
        assert!(s.modal.is_none());
    }

    #[test]
    fn pending_is_bounded_failures_first() {
        let mut s = Interactions::default();
        let mut failed = pending(1, 0);
        failed.state = PendingState::Failed(Failure::Code(1));
        s.track(failed);
        for n in 2..(MAX_PENDING as u64 + 5) {
            s.track(pending(n, 0));
        }
        assert_eq!(s.pending().len(), MAX_PENDING);
        assert!(!s.pending().iter().any(|p| p.nonce == 1));
    }

    #[test]
    fn indexes_are_bounded_and_refreshed() {
        let mut s = Interactions::default();
        assert!(s.wants(Scope::User, 0));
        s.mark_loading(Scope::User, 0);
        assert!(!s.wants(Scope::User, 1) && s.loading(Scope::User));
        s.apply(InteractionEvent::Index { scope: Scope::User, index: Index::default() }, 10);
        assert!(s.index(Scope::User).is_some());
        assert!(!s.wants(Scope::User, 10 + INDEX_TTL_MS));
        assert!(s.wants(Scope::User, 11 + INDEX_TTL_MS));
        // Échec après un index reçu : l'ancien reste.
        s.apply(InteractionEvent::IndexFailed { scope: Scope::User }, 20);
        assert!(s.index(Scope::User).is_some());
        for g in 0..(MAX_INDEXES as u64 + 3) {
            s.mark_loading(Scope::Guild(g), 100 + g as i64);
        }
        assert!(s.indexes.len() <= MAX_INDEXES);
        assert!(s.index(Scope::User).is_none(), "le plus ancien est parti");
    }

    #[test]
    fn autocomplete_keeps_only_the_latest_answer() {
        let mut s = Interactions::default();
        s.ask_choices(5);
        s.apply(InteractionEvent::Autocomplete { nonce: 4, choices: vec![Choice { name: "vieux".into(), value: ChoiceValue::Int(1) }] }, 0);
        assert!(s.choices().is_none());
        let many = (0..40).map(|i| Choice { name: i.to_string(), value: ChoiceValue::Str(i.to_string()) }).collect();
        s.apply(InteractionEvent::Autocomplete { nonce: 5, choices: many }, 0);
        assert_eq!(s.choices().map(<[Choice]>::len), Some(MAX_CHOICES));
    }

    #[test]
    fn nonces_increase() {
        let mut s = Interactions::default();
        let a = s.next_nonce(1_700_000_000_000);
        let b = s.next_nonce(1_700_000_000_000);
        assert!(b > a && b.to_string().len() <= 19);
    }
}
