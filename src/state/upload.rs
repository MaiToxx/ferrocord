//! Envoi de fichiers : pièces jointes d'un message, limites de Discord,
//! envois en cours (progression, annulation, échec gardé pour un nouvel essai).
//!
//! Forme de la requête : Create Message en `multipart/form-data`
//! (https://discord.com/developers/docs/reference#uploading-files) :
//! `payload_json` (le corps JSON habituel, avec `attachments` qui donne
//! l'`id`, le `filename` et la `description` de chaque fichier) et une
//! partie `files[n]` par fichier. Un fichier marqué « spoiler » voit son nom
//! préfixé de `SPOILER_` (même page). Ne dépend ni d'egui ni de `net/`.

use super::notice::Problem;
use super::social::SendKind;
use std::{path::PathBuf, sync::Arc};

/// Fichiers par message, au plus (« up to 10 files », même page).
pub const MAX_FILES: usize = 10;
/// Envois gardés en même temps (en cours ou en échec), au plus : au-delà,
/// le plus ancien échec est oublié ; s'il n'y en a pas, l'envoi est refusé.
pub const MAX_UPLOADS: usize = 20;
/// Taille d'un fichier sans Nitro (10 Mio depuis 2024 ; documentation de
/// Discord, « Uploading Files » : la limite dépend du compte et du serveur).
pub const DEFAULT_LIMIT: u64 = 10 << 20;
/// Limite la plus haute (Nitro) : au-delà, inutile de demander le profil.
pub const MAX_LIMIT: u64 = 500 << 20;

/// Limite par fichier selon `premium_type` (docs.discord.food, « User
/// Object › Premium Type » : 1 Nitro Classic, 2 Nitro, 3 Nitro Basic).
pub fn limit_for(premium: u8) -> u64 {
    match premium {
        2 => MAX_LIMIT,
        1 | 3 => 50 << 20,
        _ => DEFAULT_LIMIT,
    }
}

/// Contenu d'une pièce jointe : un fichier du disque (lu en flux au moment
/// de l'envoi), ou des octets déjà en mémoire (image collée, en PNG).
#[derive(Clone, PartialEq, Eq)]
pub enum Source {
    Path(PathBuf),
    Memory(Arc<[u8]>),
}

impl std::fmt::Debug for Source {
    // Jamais les octets eux-mêmes dans un journal.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Path(p) => f.debug_tuple("Path").field(p).finish(),
            Source::Memory(b) => write!(f, "Memory({} octets)", b.len()),
        }
    }
}

/// Une pièce jointe telle qu'elle partira.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutFile {
    pub source: Source,
    /// Nom choisi (sans le préfixe `SPOILER_`).
    pub name: String,
    /// Taille lue à l'ajout ; vérifiée de nouveau juste avant l'envoi.
    pub size: u64,
    /// Texte alternatif (vide : aucun).
    pub description: String,
    pub spoiler: bool,
}

/// Préfixe documenté d'une pièce jointe masquée.
pub const SPOILER_PREFIX: &str = "SPOILER_";

/// Longueur maximale d'un texte alternatif (`description`, « 1024 characters »
/// dans l'objet Attachment de la documentation).
pub const DESCRIPTION_MAX: usize = 1024;

impl OutFile {
    /// Nom envoyé : caractères de contrôle et séparateurs de chemin retirés,
    /// jamais vide, préfixé de `SPOILER_` si masqué.
    pub fn sent_name(&self) -> String {
        let clean: String = self.name.chars().filter(|c| !c.is_control() && !matches!(c, '/' | '\\')).collect();
        let clean = clean.trim();
        let base = if clean.is_empty() { "file" } else { clean };
        let base = base.strip_prefix(SPOILER_PREFIX).unwrap_or(base);
        if self.spoiler { format!("{SPOILER_PREFIX}{base}") } else { base.to_string() }
    }
}

/// Interface -> réseau.
#[derive(Debug, PartialEq, Eq)]
pub enum UploadCmd {
    /// Message avec pièces jointes, par la file d'envoi du salon. `nonce` :
    /// gardé pour un nouvel essai (pas de doublon, `enforce_nonce`).
    Send { channel_id: u64, nonce: u64, content: String, files: Vec<OutFile>, kind: Option<SendKind> },
    /// Arrête l'envoi (en attente ou en cours).
    Cancel { nonce: u64 },
}

/// Pourquoi un envoi n'a pas abouti.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// Refus ou erreur de Discord, ou réseau avant le départ.
    Problem(Problem),
    /// Issue inconnue (connexion coupée après l'envoi) : peut-être parti ;
    /// un nouvel essai avec le même nonce ne fait pas de doublon.
    Unknown(Problem),
    /// Fichier illisible (supprimé, verrouillé…).
    Unreadable(String),
    /// Fichier modifié depuis l'ajout (taille différente).
    Changed(String),
    /// Discord refuse la taille (413).
    TooLarge,
}

/// Réseau -> interface.
#[derive(Debug, PartialEq, Eq)]
pub enum UploadEvent {
    /// Octets partis sur le total de la requête (envoyé quelques fois par seconde au plus).
    Progress { nonce: u64, sent: u64, total: u64 },
    Sent { channel_id: u64, nonce: u64 },
    Failed { channel_id: u64, nonce: u64, failure: Failure },
    Cancelled { channel_id: u64, nonce: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// En file ou en cours.
    Sending,
    /// Annulation demandée, pas encore confirmée.
    Cancelling,
    Failed(Failure),
    Cancelled,
}

/// Un envoi en cours ou en échec (le message et ses fichiers sont gardés).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Upload {
    pub channel_id: u64,
    pub nonce: u64,
    pub content: String,
    pub files: Vec<OutFile>,
    pub kind: Option<SendKind>,
    pub sent: u64,
    pub total: u64,
    pub phase: Phase,
}

impl Upload {
    fn command(&self) -> UploadCmd {
        UploadCmd::Send { channel_id: self.channel_id, nonce: self.nonce, content: self.content.clone(), files: self.files.clone(), kind: self.kind.clone() }
    }

    /// Envoi arrêté (échec ou annulation) : à réessayer ou à supprimer.
    pub fn stopped(&self) -> bool {
        matches!(self.phase, Phase::Failed(_) | Phase::Cancelled)
    }
}

/// Pourquoi un envoi est refusé avant de partir.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Plus de [`MAX_FILES`] fichiers.
    TooMany,
    /// Ce fichier dépasse la limite (en octets).
    TooLarge { name: String, limit: u64 },
    /// La limite du compte est en cours de lecture (profil demandé) : réessayer.
    CheckingLimit,
    /// Trop d'envois gardés ([`MAX_UPLOADS`]), tous en cours.
    Busy,
    /// Envoi refusé de façon certaine dans ce salon (permission, exclusion).
    Blocked,
}

/// Envois gardés, du plus ancien au plus récent.
#[derive(Debug, Default)]
pub struct Uploads {
    list: Vec<Upload>,
}

impl Uploads {
    pub fn clear(&mut self) {
        self.list.clear();
    }

    pub fn of_channel(&self, channel_id: u64) -> impl Iterator<Item = &Upload> {
        self.list.iter().filter(move |u| u.channel_id == channel_id)
    }

    pub fn get(&self, nonce: u64) -> Option<&Upload> {
        self.list.iter().find(|u| u.nonce == nonce)
    }

    fn get_mut(&mut self, nonce: u64) -> Option<&mut Upload> {
        self.list.iter_mut().find(|u| u.nonce == nonce)
    }

    /// Garde un nouvel envoi et renvoie sa commande ; `Busy` si la liste est
    /// pleine d'envois en cours.
    pub(super) fn start(&mut self, channel_id: u64, nonce: u64, content: String, files: Vec<OutFile>, kind: Option<SendKind>) -> Result<UploadCmd, Refusal> {
        if self.list.len() >= MAX_UPLOADS {
            let Some(i) = self.list.iter().position(Upload::stopped) else { return Err(Refusal::Busy) };
            self.list.remove(i);
        }
        let total = files.iter().map(|f| f.size).sum();
        let u = Upload { channel_id, nonce, content, files, kind, sent: 0, total, phase: Phase::Sending };
        let cmd = u.command();
        self.list.push(u);
        Ok(cmd)
    }

    /// Nouvel essai d'un envoi arrêté, avec le même nonce.
    pub(super) fn retry(&mut self, nonce: u64) -> Option<UploadCmd> {
        let u = self.get_mut(nonce).filter(|u| u.stopped())?;
        u.phase = Phase::Sending;
        u.sent = 0;
        Some(u.command())
    }

    pub fn cancel(&mut self, nonce: u64) -> Option<UploadCmd> {
        let u = self.get_mut(nonce).filter(|u| u.phase == Phase::Sending)?;
        u.phase = Phase::Cancelling;
        Some(UploadCmd::Cancel { nonce })
    }

    /// Supprime un envoi arrêté (le message et ses fichiers sont abandonnés).
    pub fn discard(&mut self, nonce: u64) {
        self.list.retain(|u| u.nonce != nonce || !u.stopped());
    }

    /// Message créé (réponse de Discord ou MESSAGE_CREATE de ce nonce).
    pub(super) fn confirmed(&mut self, nonce: u64) {
        self.list.retain(|u| u.nonce != nonce);
    }

    pub(super) fn apply(&mut self, ev: UploadEvent) {
        match ev {
            UploadEvent::Progress { nonce, sent, total } => {
                if let Some(u) = self.get_mut(nonce).filter(|u| !u.stopped()) {
                    u.sent = sent.min(total);
                    u.total = total;
                }
            }
            UploadEvent::Sent { nonce, .. } => self.confirmed(nonce),
            UploadEvent::Failed { nonce, failure, .. } => {
                if let Some(u) = self.get_mut(nonce) {
                    u.phase = Phase::Failed(failure);
                }
            }
            UploadEvent::Cancelled { nonce, .. } => {
                if let Some(u) = self.get_mut(nonce) {
                    u.phase = Phase::Cancelled;
                }
            }
        }
    }
}

/// Vérifie le nombre et la taille des fichiers avant l'envoi.
pub fn check(files: &[OutFile], limit: u64) -> Result<(), Refusal> {
    if files.len() > MAX_FILES {
        return Err(Refusal::TooMany);
    }
    match files.iter().find(|f| f.size > limit) {
        Some(f) => Err(Refusal::TooLarge { name: f.name.clone(), limit }),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, size: u64) -> OutFile {
        OutFile { source: Source::Memory(Arc::from(vec![0u8; 4])), name: name.into(), size, description: String::new(), spoiler: false }
    }

    #[test]
    fn spoiler_and_unsafe_names() {
        let mut f = file("chat.png", 1);
        assert_eq!(f.sent_name(), "chat.png");
        f.spoiler = true;
        assert_eq!(f.sent_name(), "SPOILER_chat.png");
        f.name = "SPOILER_chat.png".into();
        assert_eq!(f.sent_name(), "SPOILER_chat.png", "pas de double préfixe");
        f.spoiler = false;
        assert_eq!(f.sent_name(), "chat.png");
        f.name = "..\\a/b\r\n.txt".into();
        assert_eq!(f.sent_name(), "..ab.txt");
        f.name = " \n ".into();
        assert_eq!(f.sent_name(), "file");
    }

    #[test]
    fn limits_follow_the_premium_type() {
        assert_eq!(limit_for(0), 10 << 20);
        assert_eq!(limit_for(1), 50 << 20);
        assert_eq!(limit_for(2), 500 << 20);
        assert_eq!(limit_for(3), 50 << 20);
        assert_eq!(limit_for(99), DEFAULT_LIMIT);
        let ten: Vec<OutFile> = (0..10).map(|i| file(&format!("{i}"), 1)).collect();
        assert_eq!(check(&ten, DEFAULT_LIMIT), Ok(()));
        let eleven: Vec<OutFile> = (0..11).map(|i| file(&format!("{i}"), 1)).collect();
        assert_eq!(check(&eleven, DEFAULT_LIMIT), Err(Refusal::TooMany));
        assert_eq!(check(&[file("gros", DEFAULT_LIMIT + 1)], DEFAULT_LIMIT), Err(Refusal::TooLarge { name: "gros".into(), limit: DEFAULT_LIMIT }));
        assert_eq!(check(&[file("juste", DEFAULT_LIMIT)], DEFAULT_LIMIT), Ok(()));
    }

    #[test]
    fn progress_failure_retry_and_discard() {
        let mut u = Uploads::default();
        let cmd = u.start(5, 77, "salut".into(), vec![file("a", 100)], None).unwrap();
        assert!(matches!(cmd, UploadCmd::Send { nonce: 77, .. }));
        u.apply(UploadEvent::Progress { nonce: 77, sent: 40, total: 200 });
        assert_eq!(u.get(77).map(|x| (x.sent, x.total)), Some((40, 200)));
        // Supprimer un envoi en cours : sans effet (il faut l'annuler).
        u.discard(77);
        assert!(u.get(77).is_some());
        u.apply(UploadEvent::Failed { channel_id: 5, nonce: 77, failure: Failure::TooLarge });
        assert!(u.get(77).is_some_and(Upload::stopped));
        // Nouvel essai : même nonce, mêmes fichiers.
        assert_eq!(u.retry(77), Some(UploadCmd::Send { channel_id: 5, nonce: 77, content: "salut".into(), files: vec![file("a", 100)], kind: None }));
        assert_eq!(u.retry(77), None, "déjà en cours");
        assert_eq!(u.cancel(77), Some(UploadCmd::Cancel { nonce: 77 }));
        assert_eq!(u.cancel(77), None);
        u.apply(UploadEvent::Cancelled { channel_id: 5, nonce: 77 });
        assert_eq!(u.get(77).map(|x| x.phase.clone()), Some(Phase::Cancelled));
        u.discard(77);
        assert!(u.get(77).is_none());
        // Confirmé par MESSAGE_CREATE : oublié.
        u.start(5, 78, String::new(), vec![file("b", 1)], None).unwrap();
        u.confirmed(78);
        assert_eq!(u.of_channel(5).count(), 0);
    }

    #[test]
    fn bounded_list_forgets_the_oldest_failure_first() {
        let mut u = Uploads::default();
        for n in 0..MAX_UPLOADS as u64 {
            u.start(1, n, String::new(), vec![file("x", 1)], None).unwrap();
        }
        assert_eq!(u.start(1, 100, String::new(), vec![], None), Err(Refusal::Busy));
        u.apply(UploadEvent::Failed { channel_id: 1, nonce: 3, failure: Failure::TooLarge });
        assert!(u.start(1, 100, String::new(), vec![], None).is_ok());
        assert!(u.get(3).is_none() && u.get(100).is_some());
    }
}
