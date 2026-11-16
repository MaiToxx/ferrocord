//! Réactions, réponses, transferts, épingles, modification et suppression
//! des messages : données du modèle, événements et commandes (lots T5/T6).
//!
//! Tout est regroupé dans une variante de `Event` (`Event::Social`) et une
//! de `Cmd` (`Cmd::Social`) pour que les autres lots n'aient presque rien à
//! fusionner dans `event.rs`.

use super::model::Message;
use super::notice::Problem;

/// Emoji d'une réaction : caractère Unicode, ou emoji personnalisé d'un serveur.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Emoji {
    Unicode(String),
    Custom { id: u64, name: String, animated: bool },
}

impl Emoji {
    /// Même emoji (un emoji personnalisé se reconnaît à son identifiant :
    /// son nom peut changer ou manquer).
    pub fn same(&self, other: &Emoji) -> bool {
        match (self, other) {
            (Emoji::Custom { id: a, .. }, Emoji::Custom { id: b, .. }) => a == b,
            (Emoji::Unicode(a), Emoji::Unicode(b)) => a == b,
            _ => false,
        }
    }

    /// Texte affiché : le caractère (sans sélecteur de variante, que les
    /// polices n'ont pas), ou `:nom:`.
    pub fn display(&self) -> String {
        match self {
            Emoji::Unicode(s) => s.chars().filter(|c| *c != '\u{FE0F}').collect(),
            Emoji::Custom { name, .. } if name.is_empty() => ":?:".into(),
            Emoji::Custom { name, .. } => format!(":{name}:"),
        }
    }
}

/// Réaction d'un message. `normal` et `burst` : comptes des réactions
/// ordinaires et des super-réactions (`count_details`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Reaction {
    pub emoji: Emoji,
    pub normal: u32,
    pub burst: u32,
    pub me: bool,
    pub me_burst: bool,
}

/// Message auquel un message répond (`message_reference` + `referenced_message`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ReplyRef {
    pub message_id: Option<u64>,
    pub state: ReplyState,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ReplyState {
    /// Auteur et extrait (rendu une fois par le magasin à la réception).
    Loaded { author_id: u64, author: String, excerpt: String },
    /// `referenced_message` nul : le message a été supprimé.
    Deleted,
    /// Absent : Discord ne l'a pas cherché.
    Unknown,
}

/// Instantané d'un message transféré (`message_snapshots[0].message`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Forward {
    /// Texte source (mis en forme avec le message).
    pub content: String,
    /// Pièces jointes, embeds et autocollants de l'instantané, montrés comme
    /// ceux d'un message (images, vidéos, fichiers, cartes).
    pub attachments: Vec<super::model::Attachment>,
    pub embeds: Vec<super::model::Embed>,
    pub stickers: Vec<super::emoji::StickerItem>,
    /// Salon d'origine, s'il est connu.
    pub channel_id: Option<u64>,
    pub guild_id: Option<u64>,
    pub message_id: Option<u64>,
    /// Mentions de l'instantané, pour sa mise en forme avec le message.
    pub mentions: Vec<(u64, String)>,
}

/// Ce que ce lot ajoute à un message. Un seul champ dans `Message` pour que
/// les constructions de messages des autres lots n'aient qu'une ligne à ajouter.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct MessageMeta {
    pub reactions: Vec<Reaction>,
    pub reply: Option<Box<ReplyRef>>,
    pub forward: Option<Box<Forward>>,
    pub pinned: bool,
    /// Texte brut de l'extrait de la réponse (borné par le magasin), mis en forme avec le message.
    pub reply_raw: Option<Box<RawExcerpt>>,
    /// Rôles mentionnés (`mention_roles`), pour les notifications (lot S5).
    pub mention_roles: Vec<u64>,
    /// Fil né de ce message (lot T10) : « N messages » sous le message.
    pub thread: Option<Box<super::threads::ThreadStub>>,
    /// Autocollants du message (`sticker_items`, lot T4).
    pub stickers: Vec<super::emoji::StickerItem>,
    /// Composants, réponse éphémère ou différée d'une application (lot T11).
    pub app: Option<Box<super::interactions::AppMeta>>,
    /// Sondage, invitations, annonce, message système (lot T13).
    pub extra: Option<Box<super::system_msgs::Extra>>,
    /// Indicateurs du message (`flags`), pour les reposer tous quand on en
    /// change un (lot M : « Supprimer les intégrations »).
    pub flags: u64,
    /// Envoyé avec `/tts` (champ `tts` du message, documenté).
    pub tts: bool,
    /// Journal local (plugin, lot E1) : supprimé chez Discord mais gardé,
    /// versions précédentes. `None` tant que le journal n'a rien sur ce message.
    pub trace: Option<Box<super::journal::Trace>>,
    /// Traduction affichée sous le message (plugin, lot E3) ; `None` : aucune.
    pub translated: Option<Box<super::translate::Shown>>,
}

impl MessageMeta {
    /// Message supprimé chez Discord, gardé par le journal local (lot E1).
    pub fn is_gone(&self) -> bool {
        self.trace.as_ref().is_some_and(|t| t.gone_ms.is_some())
    }
}

/// `SUPPRESS_EMBEDS` (documentation de Discord, « Message Flags »).
pub const SUPPRESS_EMBEDS: u64 = 1 << 2;

/// Texte brut d'un extrait et ses mentions, avant rendu.
pub type RawExcerpt = (String, Vec<(u64, String)>);

/// Emoji personnalisé d'un serveur (GUILD_CREATE, GUILD_EMOJIS_UPDATE).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GuildEmoji {
    pub id: u64,
    pub name: String,
    pub animated: bool,
    /// `available` : un emoji perdu avec les boosts ne peut pas servir.
    pub available: bool,
}

impl GuildEmoji {
    pub fn emoji(&self) -> Emoji {
        Emoji::Custom { id: self.id, name: self.name.clone(), animated: self.animated }
    }
}

/// Utilisateur de la liste de qui a réagi.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reactor {
    pub id: u64,
    pub name: String,
}

/// Message épinglé (panneau des épingles).
#[derive(Clone, Debug)]
pub struct Pin {
    /// Horodatage ISO 8601 de l'épinglage, renvoyé tel quel pour la page suivante.
    pub pinned_at: String,
    pub message: Message,
}

/// Envoi particulier gardé avec le nonce, pour qu'un nouvel essai le refasse à l'identique.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendKind {
    /// Réponse ; `mention` : mentionner l'auteur du message (`replied_user`).
    Reply { message_id: u64, mention: bool },
    /// Transfert d'un message d'un autre salon (référence de type FORWARD).
    Forward { channel_id: u64, message_id: u64, guild_id: Option<u64> },
    /// Autocollant d'un serveur (`sticker_ids`, lot T4) ; le texte ne sert qu'à l'affichage d'un échec.
    Sticker { sticker_id: u64 },
    /// Message vocal (phase 6, `state::voice_msg`) : sa seule pièce jointe
    /// porte durée et onde (base64).
    Voice { duration_ms: u32, waveform: String },
}

/// Réseau -> magasin.
#[derive(Debug)]
pub enum SocialEvent {
    /// MESSAGE_REACTION_ADD / _REMOVE ; `add` : ajout.
    Reaction { channel_id: u64, message_id: u64, user_id: u64, emoji: Emoji, burst: bool, add: bool },
    /// Quelqu'un a réagi à un message dont l'auteur est `author_id`
    /// (`message_author_id` de MESSAGE_REACTION_ADD, documenté) : sert à la
    /// notification « a réagi à votre message » (F11). Envoyé en plus de
    /// `Reaction` ; `name` : pseudo ou nom porté par l'événement.
    ReactedTo { guild_id: Option<u64>, channel_id: u64, message_id: u64, user_id: u64, author_id: u64, emoji: Emoji, name: Option<String> },
    ReactionRemoveAll { channel_id: u64, message_id: u64 },
    ReactionRemoveEmoji { channel_id: u64, message_id: u64, emoji: Emoji },
    /// Réponse à `SocialCmd::React` : `error` absent si Discord a accepté.
    ReactDone { channel_id: u64, message_id: u64, emoji: Emoji, add: bool, error: Option<Problem> },
    /// Liste de qui a réagi (une page), ou son échec.
    Reactors { message_id: u64, emoji: Emoji, burst: bool, users: Result<Vec<Reactor>, Problem> },
    EditFailed { channel_id: u64, message_id: u64, content: String, error: Problem },
    DeleteFailed { channel_id: u64, message_id: u64, error: Problem },
    /// Épinglage changé (réponse REST ou MESSAGE_UPDATE qui porte `pinned`).
    Pinned { channel_id: u64, message_id: u64, pinned: bool },
    PinFailed { channel_id: u64, message_id: u64, error: Problem },
    /// Intégrations retirées du message (lot M).
    EmbedsSuppressed { channel_id: u64, message_id: u64 },
    /// Page d'épingles (`before` : celui de la demande).
    Pins { channel_id: u64, before: Option<String>, pins: Result<(Vec<Pin>, bool), Problem> },
    /// CHANNEL_PINS_UPDATE : la liste en mémoire est périmée.
    PinsChanged { channel_id: u64 },
    GuildEmojis { guild_id: u64, emojis: Vec<GuildEmoji> },
}

/// Magasin -> réseau. Chaque commande suit un geste de l'utilisateur.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SocialCmd {
    React { channel_id: u64, message_id: u64, emoji: Emoji, add: bool },
    /// Une page (au plus `REACTORS_PAGE`) de qui a réagi.
    Reactors { channel_id: u64, message_id: u64, emoji: Emoji, burst: bool },
    Edit { channel_id: u64, message_id: u64, content: String },
    Delete { channel_id: u64, message_id: u64 },
    Pin { channel_id: u64, message_id: u64, pin: bool },
    /// « Supprimer les intégrations » : `flags` complets, `SUPPRESS_EMBEDS` compris.
    SuppressEmbeds { channel_id: u64, message_id: u64, flags: u64 },
    Pins { channel_id: u64, before: Option<String> },
    /// Envoi d'une réponse ou d'un transfert, par la file d'envoi du salon
    /// (même nonce, mêmes événements d'issue que `Cmd::Send`).
    Send { channel_id: u64, content: String, nonce: u64, kind: SendKind },
}

/// Utilisateurs demandés par liste de qui a réagi (borne de l'affichage).
pub const REACTORS_PAGE: usize = 25;
