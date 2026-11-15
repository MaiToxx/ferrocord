//! Migration de l'ancien fichier de jeton en clair
//! (`<config>/ferrocord/token`, écrit par les versions précédentes) vers le
//! coffre, en deux étapes, sur le fil des comptes, verrou entre instances
//! tenu à chaque étape.
//!
//! Étape 1, à chaque lancement ([`migrate`]) :
//! 1. lire le fichier (borné en taille) ; vide, illisible ou jeton abîmé :
//!    rien n'est touché ;
//! 2. regarder l'entrée [`AccountKey::imported`] du coffre (une réponse
//!    « absent » est vérifiée par une deuxième lecture) :
//!    - même jeton que le fichier : rien n'est écrit ([`Migration::Stored`]) ;
//!    - autre jeton : le coffre l'emporte (jeton renouvelé et enregistré
//!      depuis) ; le fichier n'est pas migré par-dessus et reste signalé ;
//!    - aucune : le jeton est écrit, puis relu et comparé
//!      ([`Migration::Written`]).
//!
//! **Le fichier reste en place.** La session s'ouvre avec le jeton du coffre.
//!
//! Étape 2 ([`finish`]), seulement après READY obtenu avec ce jeton, et
//! **jamais dans le processus qui a écrit l'entrée, ni moins de 24 h après
//! l'écriture** : une écriture que le coffre vient d'accepter peut encore
//! n'être qu'en mémoire (LSASS) et se perdre dans une panne de courant, même
//! si un autre processus la relit quelques secondes plus tard. L'instant de
//! la copie (heure murale, jamais le jeton) est retenu dans `settings.toml`
//! (`[migration] copie_depuis`) ; l'interface ne programme l'étape 2 qu'au
//! lancement où l'étape 1 a trouvé « même jeton » ET où cet instant existe,
//! date d'au moins 24 h, n'est pas dans le futur et précède le démarrage
//! du processus. Entrée trouvée sans instant (ou instant à plus de 48 h
//! dans le futur) : il est retenu maintenant, et l'on attend 24 h ; à moins
//! de 48 h dans le futur (horloge en retard), il est gardé. Ordre, verrou tenu :
//! 1. relire le coffre : même jeton (absence vérifiée deux fois) ;
//! 2. ouvrir le fichier UNE fois, en lecture et écriture, sans partage
//!    d'écriture ni de suppression (personne ne peut le renommer ni le
//!    remplacer pendant ce temps), sans suivre un lien ; tenu ailleurs : on
//!    ne touche à rien ;
//! 3. sur ce même descripteur : fichier ordinaire, un seul nom, taille
//!    bornée, contenu identique au jeton du coffre ;
//! 4. zéros sur ce descripteur, écrits sur le disque ;
//! 5. descripteur fermé puis rouvert (partage de suppression, nécessaire
//!    pour supprimer) : le chemin est supprimé seulement s'il désigne encore
//!    un fichier ordinaire vide ou de zéros, de même taille (relu sur ce
//!    descripteur) ; sinon rien n'est supprimé.
//!
//! Zéros écrits mais suppression impossible (ou processus arrêté entre les
//! deux) : le fichier ne contient plus le jeton ([`Finish::Emptied`]). Un
//! fichier vide ou qui ne contient que des zéros n'est jamais présenté
//! comme une copie du jeton ; au lancement suivant, il est supprimé comme
//! vestige ([`Migration::Vestige`]).
//!
//! Toute action confirmée (retrait d'un compte, déconnexion, suppression du
//! fichier) l'est sur un état relu ([`Survey`]) : au moment de l'exécuter,
//! verrou repris, coffre et fichier sont relus ; s'ils ont changé, rien
//! n'est fait et la question est reposée ([`Checked::Stale`]).
//!
//! Aucun autre chemin automatique ne supprime le fichier : un refus de
//! Discord, un compte retiré ou une déconnexion n'y touchent pas ; seul
//! « Supprimer le fichier de jeton en clair », confirmé, le fait.
//!
//! Au moindre échec, le fichier reste intact et son jeton sert pour la
//! session en cours. Une entrée du coffre n'est jamais remplacée par un autre
//! jeton sans confirmation explicite (question posée, état relu verrou tenu
//! avant d'écrire) ; la migration n'en remplace aucune. Elle n'est
//! retirée que si on vient de l'écrire soi-même, que la relecture l'a
//! trouvée différente (ou absente deux fois), que le coffre a répondu à
//! temps et que le fichier contient toujours le jeton.

use super::{
    lock::InstanceLock,
    vault::{get_confirmed, Vault},
    AccountKey, Token, TokenError,
};
use crate::i18n::{
    language,
    verbatim::{f as vf, t as vt},
    Text,
};
use std::{
    fs,
    io::{self, Read, Seek, Write},
    path::{Path, PathBuf},
};

/// Emplacement de l'ancien fichier de jeton.
pub fn legacy_path() -> Option<PathBuf> {
    Some(crate::platform::config_dir()?.join("ferrocord").join("token"))
}

/// Un jeton fait moins de 100 caractères : au-delà, ce n'est pas un jeton.
const MAX_SIZE: u64 = 16 * 1024;

/// Au plus autant de zéros écrits par-dessus un fichier à supprimer.
const WIPE_MAX: u64 = MAX_SIZE * 64;

/// Signalement d'un fichier sans son contenu : taille et date de modification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileMark {
    pub size: u64,
    /// Nanosecondes depuis 1970.
    pub modified_ns: u64,
}

/// Taille et date de modification du fichier (sans le lire) ; `None` s'il
/// n'existe pas ou n'est pas lisible.
pub fn file_mark(path: &Path) -> Option<FileMark> {
    let m = fs::symlink_metadata(path).ok()?;
    let modified_ns = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok().map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))?;
    Some(FileMark { size: m.len(), modified_ns })
}

/// Résultat de l'étape 1 de la migration.
#[derive(Debug, PartialEq, Eq)]
pub enum Migration {
    /// Pas d'ancien fichier.
    NoFile,
    /// Fichier présent mais vide (ou seulement des espaces, ou déjà écrasé
    /// par des zéros) : laissé tel quel.
    Empty,
    /// Fichier illisible ou jeton abîmé : laissé tel quel, non migré.
    Unreadable(String),
    /// Vestige d'une suppression interrompue (que des zéros, aucun jeton) :
    /// supprimé (`None`), ou pas (`Some` : la raison ; signalé comme vestige).
    Vestige(Option<String>),
    /// L'utilisateur a refusé l'import de ce fichier-là (même taille, même
    /// date) : rien n'est lu ni écrit, le fichier est seulement signalé.
    Declined,
    /// Jeton du fichier écrit dans le coffre à l'instant et relu. Le fichier
    /// reste : il ne sera supprimé qu'à un lancement suivant (étape 2).
    Written(Token),
    /// Le coffre avait déjà, au lancement, le jeton du fichier (écrit lors
    /// d'un lancement précédent) : étape 2 à faire après READY.
    Stored(Token),
    /// Le coffre avait déjà un autre jeton pour le compte importé : c'est
    /// lui qui sert (`token`) ; le fichier, qui contient `file_token`, n'est
    /// pas migré et reste à supprimer par l'utilisateur.
    VaultNewer { token: Token, file_token: Token, why: String },
    /// Échec : fichier intact, jeton utilisable pour cette session seulement.
    Failed { token: Token, why: String },
}

impl Migration {
    /// Jeton disponible pour cette session.
    pub fn token(&self) -> Option<&Token> {
        match self {
            Migration::Written(t) | Migration::Stored(t) | Migration::VaultNewer { token: t, .. } | Migration::Failed { token: t, .. } => Some(t),
            Migration::NoFile | Migration::Empty | Migration::Unreadable(_) | Migration::Vestige(_) | Migration::Declined => None,
        }
    }

    /// Jeton que contient le fichier, s'il est lisible (et lu).
    pub fn file_token(&self) -> Option<&Token> {
        match self {
            Migration::VaultNewer { file_token: t, .. } => Some(t),
            other => other.token(),
        }
    }

    /// Le coffre contient maintenant un jeton sous [`AccountKey::imported`].
    pub fn stored(&self) -> bool {
        matches!(self, Migration::Written(_) | Migration::Stored(_) | Migration::VaultNewer { .. })
    }

    /// Étape 2 à faire après READY : le jeton que fichier et coffre
    /// partagent, seulement si le coffre l'avait déjà au lancement.
    pub fn pending(&self) -> Option<&Token> {
        match self {
            Migration::Stored(t) => Some(t),
            _ => None,
        }
    }

    /// Un ancien fichier en clair reste sur le disque, à supprimer par
    /// l'utilisateur (il n'est pas migré, ou il est illisible) ; avec la raison.
    pub fn file_left(&self) -> Option<&str> {
        match self {
            Migration::VaultNewer { why, .. } | Migration::Unreadable(why) => Some(why),
            _ => None,
        }
    }
}

/// Résultat de l'étape 2.
#[derive(Debug, PartialEq, Eq)]
pub enum Finish {
    /// Fichier écrasé puis supprimé (ou déjà absent).
    Wiped,
    /// Fichier gardé intact (il contient toujours le jeton), avec la raison.
    Kept(String),
    /// Fichier vidé (zéros écrits sur le disque) mais pas supprimé : il ne
    /// contient plus le jeton. Ce n'est plus une copie, seulement un vestige.
    Emptied(String),
}

/// Ce que l'on sait du fichier avant de proposer de le supprimer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WipeCheck {
    /// Le coffre contient aussi ce jeton (relu à l'instant).
    AlsoInVault,
    /// Le coffre ne le contient pas : le fichier en est la seule copie.
    OnlyCopy,
    /// Le fichier est vide (ou ne contient que des zéros) : aucun jeton.
    NoToken,
    /// Le fichier n'a pas pu être lu, ou son contenu n'est pas reconnu :
    /// c'est peut-être le seul exemplaire d'un jeton.
    Unreadable(String),
    /// Le coffre n'a pas pu être lu : le fichier est peut-être la seule copie.
    Unknown(String),
    /// Le fichier n'existe plus.
    Gone,
}

/// Contenu du fichier en clair, tel qu'il vient d'être lu.
#[derive(Clone, Debug)]
pub enum FileState {
    Absent,
    /// Vide, espaces ou zéros : aucun jeton.
    Empty,
    Token(Token),
    /// Illisible, ou contenu qui n'est pas un jeton reconnu : on ne sait pas.
    Unknown(String),
}

impl FileState {
    /// Même état (la raison d'un contenu inconnu ne compte pas).
    fn same(&self, other: &FileState) -> bool {
        match (self, other) {
            (FileState::Absent, FileState::Absent) | (FileState::Empty, FileState::Empty) | (FileState::Unknown(_), FileState::Unknown(_)) => true,
            (FileState::Token(a), FileState::Token(b)) => a == b,
            _ => false,
        }
    }

    fn of_text(text: String) -> FileState {
        match parse_text(text) {
            Ok((_, t)) => FileState::Token(t),
            Err(Migration::Empty) => FileState::Empty,
            Err(_) => FileState::Unknown(vt::UNRECOGNIZED_CONTENT.get(language()).into()),
        }
    }
}

/// État relu du coffre et du fichier : ce que décrit une question de
/// confirmation. L'action confirmée relit tout, verrou tenu, et ne
/// s'exécute que si l'état n'a pas changé (sinon la question est reposée).
#[derive(Clone, Debug)]
pub struct Survey {
    pub file: FileState,
    /// Entrées relues (absence vérifiée deux fois), ou l'erreur.
    pub vault: Result<Vec<(AccountKey, Option<Token>)>, String>,
}

impl Survey {
    /// Même état (une erreur de lecture du coffre égale une autre).
    pub fn same(&self, other: &Survey) -> bool {
        let vault = match (&self.vault, &other.vault) {
            (Ok(a), Ok(b)) => a == b,
            (Err(_), Err(_)) => true,
            _ => false,
        };
        vault && self.file.same(&other.file)
    }

    /// Contenu relu de l'entrée `key` (`Err` : coffre illisible ; `Ok(None)` : absente).
    pub fn entry(&self, key: &AccountKey) -> Result<Option<&Token>, &str> {
        match &self.vault {
            Ok(v) => Ok(v.iter().find(|(k, _)| k == key).and_then(|(_, t)| t.as_ref())),
            Err(e) => Err(e),
        }
    }

    /// Si l'entrée `key` est retirée, une copie de son jeton reste-t-elle
    /// (fichier en clair ou autre entrée) ? `None` : on ne sait pas.
    pub fn copy_remains(&self, key: &AccountKey) -> Option<CopyLeft> {
        let entries = self.vault.as_ref().ok()?;
        let token = entries.iter().find(|(k, _)| k == key).and_then(|(_, t)| t.as_ref())?;
        if matches!(&self.file, FileState::Token(t) if t == token) {
            return Some(CopyLeft::File);
        }
        if entries.iter().any(|(k, t)| k != key && t.as_ref() == Some(token)) {
            return Some(CopyLeft::Entry);
        }
        match self.file {
            FileState::Unknown(_) => None,
            _ => Some(CopyLeft::Nothing),
        }
    }

    /// Ce que dit la question « Supprimer le fichier de jeton en clair ».
    pub fn wipe_check(&self) -> WipeCheck {
        let token = match &self.file {
            FileState::Absent => return WipeCheck::Gone,
            FileState::Empty => return WipeCheck::NoToken,
            FileState::Unknown(why) => return WipeCheck::Unreadable(why.clone()),
            FileState::Token(t) => t,
        };
        match &self.vault {
            Err(e) => WipeCheck::Unknown(e.clone()),
            Ok(v) if v.iter().any(|(_, t)| t.as_ref() == Some(token)) => WipeCheck::AlsoInVault,
            Ok(_) => WipeCheck::OnlyCopy,
        }
    }
}

/// Copie restante après le retrait d'une entrée.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyLeft {
    /// Le fichier en clair contient encore ce jeton.
    File,
    /// Une autre entrée du coffre contient ce jeton.
    Entry,
    /// Aucune : c'est le seul exemplaire enregistré.
    Nothing,
}

/// Contenu actuel du fichier (`None` : pas de fichier en clair).
fn file_state(path: Option<&Path>) -> FileState {
    let Some(path) = path else { return FileState::Absent };
    match read(path) {
        Ok(None) => FileState::Absent,
        Ok(Some(text)) => FileState::of_text(text),
        Err(e) => FileState::Unknown(e.to_string()),
    }
}

/// Entrées `keys` relues (absence vérifiée deux fois).
fn read_entries(vault: &dyn Vault, keys: &[AccountKey]) -> Result<Vec<(AccountKey, Option<Token>)>, String> {
    keys.iter().map(|k| get_confirmed(vault, k).map(|t| (k.clone(), t)).map_err(|e| e.to_string())).collect()
}

/// Relit le fichier et les entrées `keys` (pour une question de confirmation).
pub fn survey(path: Option<&Path>, vault: &dyn Vault, keys: &[AccountKey]) -> Survey {
    Survey { file: file_state(path), vault: read_entries(vault, keys) }
}

/// Fin d'une action confirmée.
#[derive(Debug)]
pub enum Checked<T> {
    /// État inchangé : l'action a été faite, avec son résultat.
    Done(T),
    /// L'état n'est plus celui de la question : rien n'est fait, la
    /// question est reposée avec cet état.
    Stale(Survey),
}

/// Retire l'entrée `key` (action confirmée sur l'état `expected`) : verrou
/// tenu, coffre et fichier relus ; rien n'est retiré s'ils ont changé.
pub fn forget_checked(path: Option<&Path>, vault: &dyn Vault, lock: &InstanceLock, key: &AccountKey, keys: &[AccountKey], expected: &Survey) -> Checked<Result<(), String>> {
    let _held = match lock.acquire() {
        Ok(h) => h,
        Err(why) => return Checked::Done(Err(why)),
    };
    let now = survey(path, vault, keys);
    if !now.same(expected) {
        return Checked::Stale(now);
    }
    // Relue absente (deux fois) : rien à retirer, aucun appel de suppression.
    if matches!(&now.vault, Ok(v) if v.iter().any(|(k, t)| k == key && t.is_none())) {
        return Checked::Done(Ok(()));
    }
    Checked::Done(vault.delete(key).map_err(|e| e.to_string()))
}

/// Supprime le fichier (action confirmée sur l'état `expected`) : verrou
/// tenu, coffre relu, fichier ouvert une seule fois en exclusivité et relu
/// sur ce descripteur ; rien n'est touché si l'un ou l'autre a changé.
pub fn wipe_checked(path: &Path, vault: &dyn Vault, lock: &InstanceLock, keys: &[AccountKey], expected: &Survey) -> Checked<Result<(), WipeFailure>> {
    let _held = match lock.acquire() {
        Ok(h) => h,
        Err(why) => return Checked::Done(Err(WipeFailure::Untouched(why))),
    };
    let entries = read_entries(vault, keys);
    let stale = |file: FileState| {
        let now = Survey { file, vault: entries.clone() };
        if now.same(expected) { None } else { Some(Checked::Stale(now)) }
    };
    let opened = match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Checked::Done(Err(WipeFailure::Untouched(e.to_string()))),
        Ok(m) => match check_kind(&m).and_then(|()| open_locked(path).map_err(held_elsewhere)) {
            Ok(f) => Some(f),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Checked::Done(Err(WipeFailure::Untouched(e.to_string()))),
        },
    };
    let Some(mut f) = opened else {
        return stale(FileState::Absent).unwrap_or(Checked::Done(Ok(())));
    };
    let len = match checked(&f) {
        Ok(l) => l,
        Err(e) => return Checked::Done(Err(WipeFailure::Untouched(e.to_string()))),
    };
    let file = match read_bounded(&mut f) {
        Ok(text) => FileState::of_text(text),
        Err(e) => FileState::Unknown(e.to_string()),
    };
    if let Some(s) = stale(file) {
        return s;
    }
    Checked::Done(zero_then_remove(f, len, path))
}

/// Échec d'une suppression du fichier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WipeFailure {
    /// Rien n'a été touché : le fichier est intact.
    Untouched(String),
    /// Zéros écrits sur le disque, mais le fichier n'a pas pu être supprimé :
    /// il ne contient plus le jeton.
    Emptied(String),
}

impl std::fmt::Display for WipeFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WipeFailure::Untouched(e) => write!(f, "{e}"),
            WipeFailure::Emptied(e) => f.write_str(&vf::emptied_not_deleted(language(), e)),
        }
    }
}

/// Lit le fichier (au plus [`MAX_SIZE`] octets) ; `Ok(None)` s'il n'existe pas.
pub(super) fn read(path: &Path) -> io::Result<Option<String>> {
    let mut f = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if !f.metadata()?.is_file() {
        return Err(io::Error::other(vt::NOT_A_FILE.get(language())));
    }
    read_bounded(&mut f).map(Some)
}

/// Contenu d'un fichier ouvert, borné, en UTF-8.
fn read_bounded(f: &mut fs::File) -> io::Result<String> {
    let mut bytes = Vec::new();
    Read::take(f, MAX_SIZE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_SIZE {
        return Err(io::Error::other(vt::FILE_TOO_BIG.get(language())));
    }
    String::from_utf8(bytes).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, vt::NOT_UTF8.get(language())))
}

/// Refus de toucher à un chemin qui n'est pas un fichier ordinaire à un seul nom.
fn not_ordinary(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, vf::nothing_overwritten(language(), what))
}

/// Ce chemin désigne un fichier ordinaire (ni lien symbolique, ni dossier).
fn check_kind(meta: &fs::Metadata) -> io::Result<()> {
    if meta.file_type().is_symlink() {
        Err(not_ordinary(vt::IS_LINK.get(language())))
    } else if !meta.is_file() {
        Err(not_ordinary(vt::NOT_ORDINARY.get(language())))
    } else {
        Ok(())
    }
}

/// Le fichier ouvert n'a qu'un nom : des zéros écrits dedans n'atteignent
/// aucun autre fichier.
#[cfg(windows)]
fn check_single_name(f: &fs::File, _: &fs::Metadata) -> io::Result<()> {
    match winapi_util::file::information(f)?.number_of_links() {
        1 => Ok(()),
        _ => Err(not_ordinary(vt::HARD_LINKS.get(language()))),
    }
}

#[cfg(unix)]
fn check_single_name(_: &fs::File, meta: &fs::Metadata) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    match meta.nlink() {
        1 => Ok(()),
        _ => Err(not_ordinary(vt::HARD_LINKS.get(language()))),
    }
}

#[cfg(not(any(windows, unix)))]
fn check_single_name(_: &fs::File, _: &fs::Metadata) -> io::Result<()> {
    Ok(())
}

/// Comme [`open_exclusive`], sans partage de suppression (Windows) : tant
/// que ce descripteur est ouvert, personne ne renomme, ne remplace ni ne
/// supprime le fichier. Sert à la vérification et aux zéros.
fn open_locked(path: &Path) -> io::Result<fs::File> {
    let mut o = fs::OpenOptions::new();
    o.read(true).write(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ seul.
        o.share_mode(0x1);
        o.custom_flags(0x0020_0000);
    }
    o.open(path)
}

/// Ouvre le fichier UNE fois pour le vérifier puis l'écraser : lecture et
/// écriture, sans suivre un lien (Windows : le lien lui-même est ouvert, et
/// refusé ensuite), et sans partage d'écriture (Windows : les autres
/// programmes peuvent seulement lire ou supprimer ; un programme qui le
/// tient déjà ouvert en écriture fait échouer l'ouverture).
fn open_exclusive(path: &Path) -> io::Result<fs::File> {
    let mut o = fs::OpenOptions::new();
    o.read(true).write(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_DELETE : personne d'autre n'écrit pendant ce temps.
        o.share_mode(0x1 | 0x4);
        // FILE_FLAG_OPEN_REPARSE_POINT : ne pas suivre un lien symbolique ou une jonction.
        o.custom_flags(0x0020_0000);
    }
    o.open(path)
}

/// Vérifie le descripteur ouvert (type, un seul nom) et rend sa taille.
fn checked(f: &fs::File) -> io::Result<u64> {
    let meta = f.metadata()?;
    check_kind(&meta)?;
    check_single_name(f, &meta)?;
    Ok(meta.len())
}

/// Zéros sur ce descripteur (ouvert par [`open_locked`] : au plus `len`
/// octets, bornés), écrits sur le disque ; puis descripteur fermé, chemin
/// rouvert (partage de suppression) et relu : supprimé seulement s'il
/// désigne encore un fichier vide ou de zéros de même taille.
fn zero_then_remove(mut f: fs::File, len: u64, path: &Path) -> Result<(), WipeFailure> {
    let zeros = |f: &mut fs::File| -> io::Result<()> {
        f.seek(io::SeekFrom::Start(0))?;
        let len = usize::try_from(len.min(WIPE_MAX)).unwrap_or(0);
        f.write_all(&vec![0u8; len])?;
        f.sync_all()
    };
    // Écriture des zéros interrompue : le contenu est peut-être en partie
    // écrasé, mais rien ne permet de dire qu'il n'y a plus de jeton.
    zeros(&mut f).map_err(|e| WipeFailure::Untouched(vf::overwrite_failed(language(), e)))?;
    drop(f);
    // Rouvert pour pouvoir supprimer : le chemin doit encore désigner un
    // fichier sans jeton (renommé ou remplacé entre-temps : rien n'est supprimé).
    let mut g = match open_exclusive(path) {
        Ok(g) => g,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(WipeFailure::Emptied(held_elsewhere(e).to_string())),
    };
    if !still_zeros(&mut g, len) {
        return Err(WipeFailure::Emptied(vt::PATH_CHANGED.get(language()).into()));
    }
    match remove_path(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(WipeFailure::Emptied(e.to_string())),
        Ok(()) => Ok(()),
    }
}

/// Le descripteur désigne un fichier ordinaire à un seul nom, de taille
/// `len`, vide ou qui ne contient que des zéros (sur la partie écrasée).
fn still_zeros(g: &mut fs::File, len: u64) -> bool {
    if checked(g).ok() != Some(len) {
        return false;
    }
    let mut bytes = Vec::new();
    if Read::take(&mut *g, len.min(WIPE_MAX)).read_to_end(&mut bytes).is_err() {
        return false;
    }
    u64::try_from(bytes.len()).ok() == Some(len.min(WIPE_MAX)) && bytes.iter().all(|b| *b == 0)
}

/// Suppression du chemin (tests : refusée pour les chemins de [`REMOVE_FAILS`]).
fn remove_path(path: &Path) -> io::Result<()> {
    #[cfg(test)]
    if REMOVE_FAILS.lock().map(|v| v.iter().any(|p| p == path)).unwrap_or(false) {
        return Err(io::Error::other("suppression refusée (test)"));
    }
    fs::remove_file(path)
}

/// Chemins dont la suppression échoue (tests : arrêt ou échec entre les zéros et la suppression).
#[cfg(test)]
pub(crate) static REMOVE_FAILS: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// Écrase le contenu du fichier par des zéros (même longueur, bornée),
/// force l'écriture sur le disque, puis supprime le fichier, le tout sur un
/// seul descripteur ouvert sans partage d'écriture. Absent : rien à faire.
/// Un lien, un dossier, un fichier à plusieurs noms ou un fichier tenu
/// ouvert ailleurs est refusé sans rien toucher.
///
/// Ce n'est pas un effacement physique garanti : un SSD, un cliché du
/// système ou une sauvegarde peuvent garder l'ancien contenu.
///
/// Tests seulement : l'application passe par [`wipe_checked`] (état relu,
/// verrou tenu, contenu comparé à celui annoncé par la question).
#[cfg(test)]
fn wipe_file(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
        Ok(m) => check_kind(&m)?,
    }
    let f = open_locked(path).map_err(held_elsewhere)?;
    let len = checked(&f)?;
    zero_then_remove(f, len, path).map_err(|e| io::Error::other(e.to_string()))
}

/// Ouverture exclusive impossible : on le dit simplement.
fn held_elsewhere(e: io::Error) -> io::Error {
    match e.kind() {
        io::ErrorKind::NotFound => e,
        _ => io::Error::new(e.kind(), vf::held_elsewhere(language(), e)),
    }
}

/// Remplace le contenu par autant de zéros, écrits sur le disque (`sync`),
/// seulement dans un fichier ordinaire à un seul nom (tests).
#[cfg(test)]
fn overwrite_with_zeros(path: &Path) -> io::Result<()> {
    check_kind(&fs::symlink_metadata(path)?)?;
    let mut f = open_exclusive(path)?;
    let len = checked(&f)?;
    let len = usize::try_from(len.min(WIPE_MAX)).unwrap_or(0);
    f.write_all(&vec![0u8; len])?;
    f.sync_all()
}

/// Étape 1 : migre `path` vers `vault`, verrou entre instances tenu.
/// `room` : la liste des comptes peut recevoir le compte importé (sinon
/// rien n'est écrit dans le coffre : l'entrée y serait invisible).
/// `declined` : fichier dont l'utilisateur a refusé l'import (pas relu s'il
/// n'a pas changé). Ne panique sur aucune entrée.
pub fn migrate(path: &Path, vault: &dyn Vault, lock: &InstanceLock, room: bool, declined: Option<FileMark>) -> Migration {
    // Pas de fichier : rien à verrouiller.
    if matches!(read(path), Ok(None)) {
        return Migration::NoFile;
    }
    if declined.is_some() && file_mark(path) == declined {
        return Migration::Declined;
    }
    let _held = match lock.acquire() {
        Ok(h) => h,
        // Verrou indisponible : rien n'est touché, le jeton du fichier sert pour la session.
        Err(why) => {
            return match read_token(path) {
                Ok((_, token)) => Migration::Failed { token, why: vf::not_copied(language(), why, KEPT_FOR_SESSION.get(language())) },
                Err(m) => m,
            };
        }
    };
    // Lu verrou tenu : une autre instance a pu finir pendant qu'on attendait.
    match read_token(path) {
        Ok((text, token)) => migrate_locked(path, &text, token, vault, room),
        // Que des zéros : vestige d'une suppression interrompue, supprimé.
        Err(Migration::Empty) => remove_vestige(path).unwrap_or(Migration::Empty),
        Err(m) => m,
    }
}

/// Suite d'un échec de l'étape 1. Le jeton du fichier n'ouvre la session
/// que si aucun compte enregistré n'est rouvert ; sinon il est seulement
/// proposé dans le sélecteur : « utilisable », pas « sert ».
const KEPT_FOR_SESSION: Text = vt::KEPT_FOR_SESSION;

/// Texte du fichier et jeton qu'il contient, ou le résultat à rendre.
fn read_token(path: &Path) -> Result<(String, Token), Migration> {
    let text = match read(path) {
        Ok(None) => return Err(Migration::NoFile),
        Ok(Some(t)) => t,
        Err(e) => return Err(Migration::Unreadable(vf::old_file_unreadable(language(), e))),
    };
    parse_text(text)
}

fn parse_text(text: String) -> Result<(String, Token), Migration> {
    match Token::parse(&text) {
        Ok(t) => Ok((text, t)),
        // Un fichier déjà écrasé (zéros) compte comme vide.
        Err(TokenError::Empty) => Err(Migration::Empty),
        Err(_) if text.bytes().all(|b| b == 0) => Err(Migration::Empty),
        Err(TokenError::Invalid(why)) => Err(Migration::Unreadable(vf::old_file_refused(language(), why))),
    }
}

/// Étape 1, verrou tenu. Le fichier n'est jamais modifié ici.
fn migrate_locked(path: &Path, text: &str, token: Token, vault: &dyn Vault, room: bool) -> Migration {
    let key = AccountKey::imported();
    let failed = |token: Token, why: String| Migration::Failed { token, why: format!("{why}{}", KEPT_FOR_SESSION.get(language())) };
    match get_confirmed(vault, &key) {
        Err(e) => failed(token, vf::vault_read_failed(language(), e)),
        // Déjà copié (lancement précédent, ou autre instance) : rien à écrire.
        Ok(Some(existing)) if existing == token => Migration::Stored(token),
        Ok(Some(existing)) => Migration::VaultNewer {
            token: existing,
            file_token: token,
            why: vt::VAULT_HAS_OTHER.get(language()).into(),
        },
        Ok(None) if !room => failed(token, vt::LIST_FULL.get(language()).into()),
        Ok(None) => match write_verified(vault, &key, &token) {
            Ok(()) => Migration::Written(token),
            Err(why) => {
                undo(path, text, vault, &key);
                failed(token, why)
            }
        },
    }
}

/// Étape 2, après READY obtenu avec `token` (le jeton du coffre), à un
/// lancement où le coffre avait déjà ce jeton : voir l'ordre en tête du module.
pub fn finish(path: &Path, vault: &dyn Vault, lock: &InstanceLock, token: &Token) -> Finish {
    let kept = |why: String| Finish::Kept(vf::old_file_kept(language(), why));
    let _held = match lock.acquire() {
        Ok(h) => h,
        Err(why) => return kept(why),
    };
    // 1. Le coffre d'abord.
    match get_confirmed(vault, &AccountKey::imported()) {
        Ok(Some(t)) if &t == token => {}
        Ok(_) => return kept(vt::VAULT_CHANGED_TOKEN.get(language()).into()),
        Err(e) => return kept(vf::vault_reread_failed(language(), e)),
    }
    // 2. Le fichier, ouvert une seule fois, sans partage d'écriture.
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Finish::Wiped,
        Err(e) => return kept(vf::file_inaccessible(language(), e)),
        Ok(m) => {
            if let Err(e) = check_kind(&m) {
                return kept(e.to_string());
            }
        }
    }
    let mut f = match open_locked(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Finish::Wiped,
        Err(e) => return kept(held_elsewhere(e).to_string()),
    };
    // 3. Sur ce descripteur : type, un seul nom, taille, contenu.
    let len = match checked(&f) {
        Ok(l) => l,
        Err(e) => return kept(e.to_string()),
    };
    match read_bounded(&mut f).map(parse_text) {
        Ok(Ok((_, t))) if &t == token => {}
        // Vide ou déjà écrasé par des zéros (suppression interrompue) : aucun jeton dedans.
        Ok(Err(Migration::Empty)) => {}
        Ok(_) => return kept(vt::OLD_FILE_OTHER_TOKEN.get(language()).into()),
        Err(e) => return kept(vf::old_file_changed(language(), e)),
    }
    // 4 et 5. Zéros sur ce même descripteur, puis suppression.
    match zero_then_remove(f, len, path) {
        Ok(()) => Finish::Wiped,
        Err(WipeFailure::Untouched(e)) => Finish::Kept(vf::old_file_delete_failed(language(), e)),
        Err(WipeFailure::Emptied(e)) => Finish::Emptied(vf::old_file_emptied(language(), e)),
    }
}

/// Avant de proposer « Supprimer le fichier de jeton en clair » : ce que
/// contient le fichier, et si le coffre a une copie de son jeton (sous
/// l'une de ces clés). L'état relu accompagne la réponse : la suppression
/// confirmée ne se fait que s'il n'a pas changé.
pub fn check_wipe(path: &Path, vault: &dyn Vault, keys: &[AccountKey]) -> (WipeCheck, Survey) {
    let s = survey(Some(path), vault, keys);
    (s.wipe_check(), s)
}

/// Vestige d'une suppression interrompue (fichier qui ne contient que des
/// zéros) : supprimé, verrou tenu, après relecture sur un descripteur
/// exclusif. `None` : ce n'est pas un vestige.
fn remove_vestige(path: &Path) -> Option<Migration> {
    let mut f = open_exclusive(path).ok()?;
    let len = checked(&f).ok()?;
    let mut bytes = Vec::new();
    Read::take(&mut f, MAX_SIZE + 1).read_to_end(&mut bytes).ok()?;
    if len == 0 || bytes.is_empty() || bytes.iter().any(|b| *b != 0) {
        return None;
    }
    Some(Migration::Vestige(remove_path(path).err().map(|e| vf::vestige_not_deleted(language(), e))))
}

/// Écrit puis relit (une absence est relue une deuxième fois : elle mène à
/// retirer l'entrée).
fn write_verified(vault: &dyn Vault, key: &AccountKey, token: &Token) -> Result<(), String> {
    vault.set(key, token).map_err(|e| vf::vault_write_failed(language(), e))?;
    match get_confirmed(vault, key) {
        Ok(Some(back)) if &back == token => Ok(()),
        Ok(Some(_)) => Err(vt::VAULT_OTHER_TOKEN.get(language()).into()),
        Ok(None) => Err(vt::VAULT_LOST_TOKEN.get(language()).into()),
        Err(e) => Err(vf::vault_reread_failed(language(), e)),
    }
}

/// Échec après notre écriture : l'entrée était absente avant (vérifié deux
/// fois), on retire ce qu'on a écrit, mais seulement si le coffre répond
/// encore à temps (sinon l'appel abandonné peut toujours aboutir : on ne
/// touche plus à rien) et si le fichier contient toujours le jeton (sinon
/// l'entrée est peut-être la seule copie : elle est laissée).
fn undo(path: &Path, text: &str, vault: &dyn Vault, key: &AccountKey) {
    if !vault.indeterminate() && matches!(read(path), Ok(Some(now)) if now == text) {
        let _ = vault.delete(key);
    }
}

#[cfg(test)]
mod tests {
    use super::super::vault::{Faults, MemoryVault, TimedVault, VaultError};
    use super::*;
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            mpsc, Arc, Mutex,
        },
        time::Duration,
    };

    /// Dossier temporaire propre à chaque test, supprimé à la fin.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            static N: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!("ferrocord-migre-{}-{}-{name}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{dir:?} : {e}"));
            Self(dir)
        }
        fn token(&self) -> PathBuf {
            self.0.join("token")
        }
        fn lock(&self) -> InstanceLock {
            InstanceLock::new(Some(self.0.join("comptes.lock")), Duration::from_secs(5))
        }
        fn write(&self, bytes: &[u8]) {
            fs::write(self.token(), bytes).unwrap_or_else(|e| panic!("{e}"));
        }
        fn content(&self) -> Option<Vec<u8>> {
            fs::read(self.token()).ok()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const FAKE: &str = "MTk4.Gx7Qz.TEST-9qWr";
    const RENEWED: &str = "MTk4.Rn3Wd.TEST-4kPz";

    fn imported() -> AccountKey {
        AccountKey::imported()
    }

    /// Étape 1.
    fn run(dir: &TempDir, v: &dyn Vault) -> Migration {
        migrate(&dir.token(), v, &dir.lock(), true, None)
    }

    /// Étape 2 (après un READY simulé avec le jeton du coffre).
    fn end(dir: &TempDir, v: &dyn Vault, token: &str) -> Finish {
        finish(&dir.token(), v, &dir.lock(), &Token::from(token))
    }

    #[test]
    fn success_copies_then_removes_after_ready() {
        let dir = TempDir::new("ok");
        dir.write(FAKE.as_bytes());
        let v = MemoryVault::new();
        // Premier lancement : écrit à l'instant, aucune étape 2 programmée.
        let m = run(&dir, &v);
        assert_eq!(m, Migration::Written(Token::from(FAKE)));
        assert_eq!(m.pending(), None, "jamais d'étape 2 dans la session qui a écrit");
        assert_eq!(v.peek(&imported()), Some(Token::from(FAKE)));
        assert!(dir.token().exists(), "fichier gardé à la fermeture du premier lancement");
        // Deuxième lancement : « même jeton », étape 2 après READY.
        let m = run(&dir, &v);
        assert_eq!(m.pending(), Some(&Token::from(FAKE)));
        assert_eq!(end(&dir, &v, FAKE), Finish::Wiped);
        assert!(!dir.token().exists(), "fichier supprimé après READY");
        // Deuxième lancement : plus rien à faire.
        assert_eq!(run(&dir, &v), Migration::NoFile);
        assert_eq!(v.peek(&imported()), Some(Token::from(FAKE)));
    }

    #[test]
    fn surrounding_whitespace_and_byte_order_mark_are_removed() {
        for (name, raw) in [("espaces", format!("  \t{FAKE}\r\n\n")), ("bom", format!("\u{feff}{FAKE}\r\n"))] {
            let dir = TempDir::new(name);
            dir.write(raw.as_bytes());
            let v = MemoryVault::new();
            assert_eq!(run(&dir, &v), Migration::Written(Token::from(FAKE)), "{name}");
            assert_eq!(v.peek(&imported()), Some(Token::from(FAKE)), "{name} : jeton intact dans le coffre");
            assert_eq!(end(&dir, &v, FAKE), Finish::Wiped, "{name}");
            assert!(!dir.token().exists());
        }
    }

    /// Étape 1 seulement au lancement : le jeton est copié et relu, le
    /// fichier reste (il n'est supprimé qu'après READY, à l'étape 2).
    #[test]
    fn launch_copies_the_token_but_keeps_the_file() {
        let dir = TempDir::new("etape1");
        dir.write(FAKE.as_bytes());
        let v = MemoryVault::new();
        let m = run(&dir, &v);
        assert_eq!(m.token(), Some(&Token::from(FAKE)), "{m:?}");
        assert_eq!(v.peek(&imported()), Some(Token::from(FAKE)));
        assert_eq!(dir.content(), Some(FAKE.as_bytes().to_vec()), "fichier encore là après l'étape 1");
    }

    /// Application fermée entre les deux étapes : au lancement suivant, rien
    /// n'est réécrit (« même jeton »), et l'étape 2 reprend après READY.
    #[test]
    fn closed_between_steps_resumes_at_step_two() {
        let dir = TempDir::new("reprise");
        dir.write(FAKE.as_bytes());
        let v = MemoryVault::new();
        assert_eq!(run(&dir, &v), Migration::Written(Token::from(FAKE)));
        // Fermeture ici. Relance : le coffre ne serait pas réécrit.
        v.set_faults(Faults { fail_set: true, fail_delete: true, ..Faults::default() });
        assert_eq!(run(&dir, &v), Migration::Stored(Token::from(FAKE)));
        assert!(dir.token().exists());
        assert_eq!(end(&dir, &v, FAKE), Finish::Wiped);
        assert_eq!(v.peek(&imported()), Some(Token::from(FAKE)));
    }

    /// L'étape 2 ne supprime le fichier que si fichier et coffre ont le même jeton.
    #[test]
    fn step_two_needs_the_same_token_in_both() {
        let dir = TempDir::new("etape2");
        dir.write(FAKE.as_bytes());
        // Coffre vide, autre jeton, illisible : le fichier reste.
        for (what, v) in [
            ("vide", MemoryVault::new()),
            ("autre", MemoryVault::with([(imported(), Token::from(RENEWED))])),
            ("illisible", {
                let v = MemoryVault::with([(imported(), Token::from(FAKE))]);
                v.set_faults(Faults { fail_get: true, ..Faults::default() });
                v
            }),
        ] {
            assert!(matches!(end(&dir, &v, FAKE), Finish::Kept(_)), "{what}");
            assert_eq!(dir.content(), Some(FAKE.as_bytes().to_vec()), "{what}");
        }
        // Le fichier a un autre jeton que celui de la session.
        let v = MemoryVault::with([(imported(), Token::from(RENEWED))]);
        assert!(matches!(end(&dir, &v, RENEWED), Finish::Kept(why) if why.contains("autre jeton")));
        assert!(dir.token().exists());
        // Fichier déjà écrasé par des zéros (suppression interrompue) : supprimé.
        dir.write(&[0u8; 20]);
        assert_eq!(end(&dir, &v, RENEWED), Finish::Wiped);
        assert!(!dir.token().exists());
    }

    /// Un jeton abîmé (caractère invisible, espace interne) n'est jamais
    /// migré : il serait refusé par Discord puis perdu.
    #[test]
    fn damaged_token_is_not_migrated() {
        let dir = TempDir::new("abime");
        let raw = "MTk4.Gx7\u{200b}Qz.TEST\n";
        dir.write(raw.as_bytes());
        let v = MemoryVault::new();
        let m = run(&dir, &v);
        assert!(matches!(&m, Migration::Unreadable(why) if why.contains("non migré")), "{m:?}");
        assert!(m.file_left().is_some(), "fichier signalé");
        assert_eq!(dir.content(), Some(raw.as_bytes().to_vec()), "fichier intact");
        assert_eq!(v.calls(), 0);
    }

    #[test]
    fn empty_file_is_left_alone() {
        for (name, content) in [("vide", &b""[..]), ("blanc", &b" \r\n\t\n"[..])] {
            let dir = TempDir::new(name);
            dir.write(content);
            let v = MemoryVault::new();
            assert_eq!(run(&dir, &v), Migration::Empty, "{name}");
            assert_eq!(dir.content(), Some(content.to_vec()), "fichier intact");
            assert_eq!(v.calls(), 0, "coffre pas même consulté");
        }
    }

    /// Fichier qui ne contient que des zéros (étape 2 arrêtée entre les zéros
    /// et la suppression) : vestige sans jeton, supprimé au lancement
    /// suivant, ou signalé comme vestige si la suppression échoue.
    #[test]
    fn zeroed_file_is_removed_as_a_vestige() {
        let dir = TempDir::new("vestige");
        dir.write(&[0u8; 20]);
        let v = MemoryVault::new();
        assert_eq!(run(&dir, &v), Migration::Vestige(None));
        assert!(!dir.token().exists());
        assert_eq!(v.calls(), 0, "coffre pas même consulté");
        dir.write(&[0u8; 20]);
        REMOVE_FAILS.lock().unwrap_or_else(|e| e.into_inner()).push(dir.token());
        match run(&dir, &v) {
            Migration::Vestige(Some(why)) => assert!(why.contains("vidé") && why.contains("sans jeton"), "{why}"),
            other => panic!("{other:?}"),
        }
        REMOVE_FAILS.lock().unwrap_or_else(|e| e.into_inner()).retain(|p| p != &dir.token());
        assert_eq!(dir.content(), Some(vec![0u8; 20]));
    }

    /// Étape 2 : zéros écrits, suppression impossible : résultat distinct,
    /// le fichier ne contient plus le jeton (jamais présenté comme une copie).
    #[test]
    fn step_two_reports_an_emptied_file() {
        let dir = TempDir::new("vide-apres");
        dir.write(FAKE.as_bytes());
        let v = MemoryVault::with([(imported(), Token::from(FAKE))]);
        REMOVE_FAILS.lock().unwrap_or_else(|e| e.into_inner()).push(dir.token());
        let f = end(&dir, &v, FAKE);
        REMOVE_FAILS.lock().unwrap_or_else(|e| e.into_inner()).retain(|p| p != &dir.token());
        match f {
            Finish::Emptied(why) => assert!(why.contains("vidé") && !crate::account::token::leaks(&why, FAKE), "{why}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(dir.content(), Some(vec![0u8; FAKE.len()]), "zéros sur le disque");
        let (check, survey) = check_wipe(&dir.token(), &v, &[imported()]);
        assert_eq!(check, WipeCheck::NoToken);
        assert_eq!(survey.copy_remains(&imported()), Some(CopyLeft::Nothing), "le fichier vidé n'est pas une copie");
        assert_eq!(run(&dir, &v), Migration::Vestige(None), "supprimé au lancement suivant");
    }

    /// Suppression confirmée : verrou, coffre et fichier relus ; si l'état
    /// n'est plus celui de la question, rien n'est touché.
    #[test]
    fn confirmed_wipe_checks_the_announced_state() {
        let keys = [imported()];
        // Fichier réécrit entre la question et la confirmation.
        let dir = TempDir::new("perime-fichier");
        dir.write(FAKE.as_bytes());
        let v = MemoryVault::with([(imported(), Token::from(FAKE))]);
        let (check, asked) = check_wipe(&dir.token(), &v, &keys);
        assert_eq!(check, WipeCheck::AlsoInVault);
        dir.write(RENEWED.as_bytes());
        match wipe_checked(&dir.token(), &v, &dir.lock(), &keys, &asked) {
            Checked::Stale(now) => assert_eq!(now.wipe_check(), WipeCheck::OnlyCopy),
            other => panic!("{other:?}"),
        }
        assert_eq!(dir.content(), Some(RENEWED.as_bytes().to_vec()), "intact");
        // Entrée retirée ailleurs : le fichier est devenu la seule copie.
        dir.write(FAKE.as_bytes());
        let (_, asked) = check_wipe(&dir.token(), &v, &keys);
        v.delete(&imported()).unwrap_or_else(|e| panic!("{e}"));
        match wipe_checked(&dir.token(), &v, &dir.lock(), &keys, &asked) {
            Checked::Stale(now) => assert_eq!(now.wipe_check(), WipeCheck::OnlyCopy),
            other => panic!("{other:?}"),
        }
        assert_eq!(dir.content(), Some(FAKE.as_bytes().to_vec()), "intact");
        // État inchangé : supprimé.
        let (_, asked) = check_wipe(&dir.token(), &v, &keys);
        assert!(matches!(wipe_checked(&dir.token(), &v, &dir.lock(), &keys, &asked), Checked::Done(Ok(()))));
        assert!(!dir.token().exists());
        // Déjà supprimé ailleurs : la question est reposée (fichier absent).
        match wipe_checked(&dir.token(), &v, &dir.lock(), &keys, &asked) {
            Checked::Stale(now) => assert_eq!(now.wipe_check(), WipeCheck::Gone),
            other => panic!("{other:?}"),
        }
    }

    /// Fichier illisible : « contenu inconnu », jamais « vide ».
    #[test]
    fn unreadable_file_is_not_called_empty() {
        let dir = TempDir::new("illisible-verif");
        dir.write(&[0xff, 0xfe, 0x41, 0x00, 0x42]);
        let (check, survey) = check_wipe(&dir.token(), &MemoryVault::new(), &[imported()]);
        assert!(matches!(check, WipeCheck::Unreadable(_)), "{check:?}");
        assert!(matches!(survey.file, FileState::Unknown(_)));
        let v = MemoryVault::with([(imported(), Token::from(FAKE))]);
        let (_, survey) = check_wipe(&dir.token(), &v, &[imported()]);
        assert_eq!(survey.copy_remains(&imported()), None, "on ne sait pas s'il reste une copie");
    }

    /// Retrait confirmé : rien n'est retiré si coffre ou fichier ont changé.
    #[test]
    fn confirmed_forget_checks_the_announced_state() {
        let dir = TempDir::new("perime-retrait");
        dir.write(FAKE.as_bytes());
        let keys = [imported()];
        let v = MemoryVault::with([(imported(), Token::from(FAKE))]);
        let asked = survey(Some(&dir.token()), &v, &keys);
        assert_eq!(asked.copy_remains(&imported()), Some(CopyLeft::File));
        // Autre instance : étape 2 faite entre-temps (fichier supprimé).
        fs::remove_file(dir.token()).unwrap_or_else(|e| panic!("{e}"));
        match forget_checked(Some(&dir.token()), &v, &dir.lock(), &imported(), &keys, &asked) {
            Checked::Stale(now) => assert_eq!(now.copy_remains(&imported()), Some(CopyLeft::Nothing)),
            other => panic!("{other:?}"),
        }
        assert_eq!(v.peek(&imported()), Some(Token::from(FAKE)), "pas retiré");
        let asked = survey(Some(&dir.token()), &v, &keys);
        assert!(matches!(forget_checked(Some(&dir.token()), &v, &dir.lock(), &imported(), &keys, &asked), Checked::Done(Ok(()))));
        assert_eq!(v.peek(&imported()), None);
    }

    /// Retrait confirmé d'une entrée relue absente (retirée ailleurs) : aucun
    /// appel de suppression (il pourrait toucher une entrée réapparue entre la
    /// relecture et lui), et le retrait réussit.
    #[test]
    fn forget_of_an_absent_entry_deletes_nothing() {
        let keys = [imported()];
        let v = MemoryVault::new();
        let asked = survey(None, &v, &keys);
        // Une suppression échouerait : le retrait ne doit pas en demander.
        v.set_faults(Faults { fail_delete: true, ..Faults::default() });
        let dir = TempDir::new("retrait-absent");
        assert!(matches!(forget_checked(None, &v, &dir.lock(), &imported(), &keys, &asked), Checked::Done(Ok(()))));
    }

    fn assert_failed_and_intact(dir: &TempDir, v: &dyn Vault, raw: &[u8], what: &str) {
        let m = run(dir, v);
        match &m {
            Migration::Failed { token, why } => {
                assert_eq!(token, &Token::from(FAKE), "{what}");
                assert!(why.contains("gardé"), "{what} : {why}");
                assert!(!crate::account::token::leaks(why, FAKE), "{what} : {why}");
            }
            other => panic!("{what} : échec attendu, reçu {other:?}"),
        }
        assert_eq!(m.token(), Some(&Token::from(FAKE)), "la session continue avec le fichier");
        assert_eq!(dir.content(), Some(raw.to_vec()), "{what} : fichier intact");
    }

    #[test]
    fn write_failure_keeps_file() {
        let dir = TempDir::new("ecriture");
        let raw = format!("{FAKE}\n");
        dir.write(raw.as_bytes());
        let v = MemoryVault::new();
        v.set_faults(Faults { fail_set: true, ..Faults::default() });
        assert_failed_and_intact(&dir, &v, raw.as_bytes(), "écriture refusée");
        assert!(v.is_empty());
    }

    #[test]
    fn different_read_back_keeps_file_and_cleans_vault() {
        let dir = TempDir::new("relecture");
        dir.write(FAKE.as_bytes());
        let v = MemoryVault::new();
        v.set_faults(Faults { corrupt_set: true, ..Faults::default() });
        assert_failed_and_intact(&dir, &v, FAKE.as_bytes(), "relecture différente");
        assert_eq!(v.peek(&imported()), None, "notre entrée douteuse retirée");
    }

    /// Relecture différente puis retrait refusé (chaîne de perte b) : l'entrée
    /// douteuse reste, mais le fichier aussi, et il n'est jamais supprimé
    /// ensuite (le coffre ne rend pas son jeton).
    #[test]
    fn stuck_bad_entry_never_costs_the_file() {
        let dir = TempDir::new("chaine-b");
        dir.write(FAKE.as_bytes());
        let v = MemoryVault::new();
        v.set_faults(Faults { corrupt_set: true, fail_delete: true, ..Faults::default() });
        assert_failed_and_intact(&dir, &v, FAKE.as_bytes(), "relecture différente, retrait refusé");
        v.set_faults(Faults::default());
        let m = run(&dir, &v);
        assert!(matches!(&m, Migration::VaultNewer { file_token, .. } if file_token == &Token::from(FAKE)), "relance : {m:?}");
        assert!(matches!(end(&dir, &v, FAKE), Finish::Kept(_)));
        assert_eq!(dir.content(), Some(FAKE.as_bytes().to_vec()));
    }

    /// La lecture du coffre échoue dès le départ : rien n'est écrit.
    #[test]
    fn read_failure_keeps_file() {
        let dir = TempDir::new("lecture");
        dir.write(FAKE.as_bytes());
        let v = MemoryVault::new();
        v.set_faults(Faults { fail_get: true, ..Faults::default() });
        assert_failed_and_intact(&dir, &v, FAKE.as_bytes(), "lecture impossible");
        assert_eq!(v.peek(&imported()), None);
    }

    #[test]
    fn unavailable_vault_keeps_file() {
        let dir = TempDir::new("indisponible");
        dir.write(FAKE.as_bytes());
        let v = MemoryVault::new();
        v.set_faults(Faults { unavailable: true, ..Faults::default() });
        assert_failed_and_intact(&dir, &v, FAKE.as_bytes(), "coffre indisponible");
        // Et le coffre réparé, la migration suivante réussit.
        v.set_faults(Faults::default());
        assert_eq!(run(&dir, &v), Migration::Written(Token::from(FAKE)));
        assert!(dir.token().exists());
    }

    /// Liste des comptes pleine : rien n'est écrit (l'entrée serait invisible).
    #[test]
    fn full_list_writes_nothing() {
        let dir = TempDir::new("pleine");
        dir.write(FAKE.as_bytes());
        let v = MemoryVault::new();
        let m = migrate(&dir.token(), &v, &dir.lock(), false, None);
        assert!(matches!(&m, Migration::Failed { why, .. } if why.contains("pleine")), "{m:?}");
        assert!(v.is_empty());
        // Déjà dans le coffre : reconnu sans rien écrire.
        let v = MemoryVault::with([(imported(), Token::from(FAKE))]);
        assert_eq!(migrate(&dir.token(), &v, &dir.lock(), false, None), Migration::Stored(Token::from(FAKE)));
    }

    /// Relecture abandonnée après le délai : l'entrée écrite n'est pas
    /// retirée (l'appel peut encore aboutir ; identique au fichier, elle est
    /// inoffensive) et le fichier reste.
    #[test]
    fn timeout_during_read_back_removes_nothing() {
        // La relecture reste bloquée jusqu'à ce que le test la libère : elle
        // dépasse toujours le délai quelle que soit la charge, et ce délai est
        // assez large pour que les appels rapides ne l'atteignent jamais
        // (avec 50 ms, une lecture rapide pouvait l'atteindre sous charge).
        struct SlowRead(MemoryVault, AtomicUsize, Mutex<mpsc::Receiver<()>>);
        impl Vault for SlowRead {
            fn get(&self, k: &AccountKey) -> Result<Option<Token>, VaultError> {
                // Lectures 0 et 1 : absence vérifiée ; lecture 2 : relecture, bloquée.
                if self.1.fetch_add(1, Ordering::SeqCst) == 2 {
                    let _ = self.2.lock().unwrap_or_else(|e| e.into_inner()).recv();
                }
                self.0.get(k)
            }
            fn set(&self, k: &AccountKey, t: &Token) -> Result<(), VaultError> {
                self.0.set(k, t)
            }
            fn delete(&self, k: &AccountKey) -> Result<(), VaultError> {
                self.0.delete(k)
            }
            fn name(&self) -> Option<&'static str> {
                None
            }
        }
        let dir = TempDir::new("delai");
        dir.write(FAKE.as_bytes());
        let (release, gate) = mpsc::channel();
        let inner = Arc::new(SlowRead(MemoryVault::new(), AtomicUsize::new(0), Mutex::new(gate)));
        let v = TimedVault::new(inner.clone(), Duration::from_secs(2));
        let m = run(&dir, &v);
        assert!(matches!(&m, Migration::Failed { .. }), "{m:?}");
        assert_eq!(inner.1.load(Ordering::SeqCst), 3, "c'est la relecture qui a dépassé le délai");
        assert!(v.busy());
        let _ = release.send(());
        // Attendre la fin de l'appel abandonné, sans délai fixe.
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        while v.busy() && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(!v.busy());
        assert_eq!(inner.0.peek(&imported()), Some(Token::from(FAKE)), "pas de retrait après un délai dépassé");
        assert_eq!(dir.content(), Some(FAKE.as_bytes().to_vec()));
        // Et l'étape 2 n'est pas tentée sur un coffre indéterminé.
        assert!(matches!(end(&dir, &v, FAKE), Finish::Kept(_)));
        assert!(dir.token().exists());
    }

    /// Le coffre a déjà ce jeton (fichier gardé au lancement précédent) :
    /// rien n'est réécrit.
    #[test]
    fn same_token_already_in_vault_is_not_rewritten() {
        let dir = TempDir::new("deja");
        dir.write(format!("{FAKE}\n").as_bytes());
        let v = MemoryVault::with([(imported(), Token::from(FAKE))]);
        // Une écriture échouerait : elle n'est pas tentée.
        v.set_faults(Faults { fail_set: true, fail_delete: true, ..Faults::default() });
        assert_eq!(run(&dir, &v), Migration::Stored(Token::from(FAKE)));
        assert_eq!(end(&dir, &v, FAKE), Finish::Wiped);
        assert!(!dir.token().exists());
        assert_eq!(v.peek(&imported()), Some(Token::from(FAKE)));
    }

    /// Le coffre a un autre jeton (renouvelé depuis) : il l'emporte, le
    /// fichier n'est pas migré par-dessus et reste signalé.
    #[test]
    fn newer_token_in_vault_wins() {
        let dir = TempDir::new("plus-recent");
        dir.write(FAKE.as_bytes());
        let v = MemoryVault::with([(imported(), Token::from(RENEWED))]);
        let m = run(&dir, &v);
        assert!(matches!(&m, Migration::VaultNewer { token, file_token, .. } if token == &Token::from(RENEWED) && file_token == &Token::from(FAKE)), "{m:?}");
        assert!(m.stored() && m.pending().is_none() && m.file_left().is_some());
        assert_eq!(v.peek(&imported()), Some(Token::from(RENEWED)), "jeton récent intact");
        assert_eq!(dir.content(), Some(FAKE.as_bytes().to_vec()), "fichier gardé, à supprimer par l'utilisateur");
        assert!(!crate::account::token::leaks(m.file_left().unwrap_or_default(), FAKE));
        assert!(!crate::account::token::leaks(&format!("{m:?}"), FAKE));
    }

    /// Le fichier tenu ouvert par un autre programme qui n'autorise que la
    /// lecture (antivirus, éditeur) : il se lit, mais ne peut être ni écrasé
    /// ni supprimé. Rendu quand la valeur est lâchée.
    #[cfg(windows)]
    fn hold_read_only(dir: &TempDir) -> fs::File {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ seul : ni écriture ni suppression par d'autres.
        const FILE_SHARE_READ: u32 = 1;
        fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(dir.token()).unwrap_or_else(|e| panic!("{e}"))
    }

    /// Scénario complet : au lancement N, la suppression du fichier échoue ;
    /// un jeton renouvelé est ensuite enregistré pour ce compte ; au
    /// lancement N+1, le nouveau jeton survit, et le fichier aussi.
    #[cfg(windows)]
    #[test]
    fn renewed_token_survives_a_kept_file() {
        let dir = TempDir::new("renouvele");
        dir.write(FAKE.as_bytes());
        let held = hold_read_only(&dir);
        let v = MemoryVault::new();
        assert_eq!(run(&dir, &v), Migration::Written(Token::from(FAKE)));
        assert!(matches!(end(&dir, &v, FAKE), Finish::Kept(why) if why.contains("autre programme")));
        drop(held);
        assert_eq!(dir.content(), Some(FAKE.as_bytes().to_vec()), "fichier gardé, intact");
        // Jeton renouvelé, enregistré pour le même compte.
        assert_eq!(v.set(&imported(), &Token::from(RENEWED)), Ok(()));
        let n1 = run(&dir, &v);
        assert!(matches!(&n1, Migration::VaultNewer { token, .. } if token == &Token::from(RENEWED)), "lancement N+1 : {n1:?}");
        assert_eq!(v.peek(&imported()), Some(Token::from(RENEWED)), "le nouveau jeton survit");
        assert!(dir.token().exists());
    }

    /// Relance après « fichier gardé » (sans renouvellement) : le fichier est
    /// supprimé à ce lancement-là, après READY, sans réécrire le coffre.
    #[cfg(windows)]
    #[test]
    fn kept_file_is_removed_on_next_launch() {
        let dir = TempDir::new("relance");
        dir.write(FAKE.as_bytes());
        let held = hold_read_only(&dir);
        let v = MemoryVault::new();
        assert_eq!(run(&dir, &v), Migration::Written(Token::from(FAKE)));
        assert!(matches!(end(&dir, &v, FAKE), Finish::Kept(_)));
        drop(held);
        let calls = v.calls();
        assert_eq!(run(&dir, &v), Migration::Stored(Token::from(FAKE)));
        assert_eq!(v.calls(), calls + 1, "une seule lecture, aucune écriture");
        assert_eq!(end(&dir, &v, FAKE), Finish::Wiped);
        assert!(!dir.token().exists());
    }

    /// Coffre qui « perd » ce qu'on lui écrit.
    struct Forgetful;

    impl Vault for Forgetful {
        fn get(&self, _: &AccountKey) -> Result<Option<Token>, VaultError> {
            Ok(None)
        }
        fn set(&self, _: &AccountKey, _: &Token) -> Result<(), VaultError> {
            Ok(())
        }
        fn delete(&self, _: &AccountKey) -> Result<(), VaultError> {
            Ok(())
        }
        fn name(&self) -> Option<&'static str> {
            None
        }
    }

    /// Coffre pendant l'écriture duquel une ancienne version réécrit le fichier.
    struct Racing(PathBuf, MemoryVault);

    impl Vault for Racing {
        fn get(&self, k: &AccountKey) -> Result<Option<Token>, VaultError> {
            self.1.get(k)
        }
        fn set(&self, k: &AccountKey, t: &Token) -> Result<(), VaultError> {
            fs::write(&self.0, RENEWED).map_err(|e| VaultError::Failed(e.to_string()))?;
            self.1.set(k, t)
        }
        fn delete(&self, k: &AccountKey) -> Result<(), VaultError> {
            self.1.delete(k)
        }
        fn name(&self) -> Option<&'static str> {
            None
        }
    }

    #[test]
    fn file_rewritten_during_migration_is_kept() {
        let dir = TempDir::new("course");
        dir.write(FAKE.as_bytes());
        let v = Racing(dir.token(), MemoryVault::new());
        assert_eq!(run(&dir, &v), Migration::Written(Token::from(FAKE)));
        assert!(matches!(end(&dir, &v, FAKE), Finish::Kept(_)));
        assert_eq!(dir.content(), Some(RENEWED.as_bytes().to_vec()), "le fichier réécrit n'est pas supprimé");
    }

    /// Un autre programme (ancienne version, restauration) écrit un jeton
    /// plus récent dans le fichier pendant que l'étape 2 relit le coffre :
    /// le contenu est vérifié sur le descripteur qui sert à l'écraser, le
    /// jeton plus récent survit.
    #[test]
    fn newer_token_written_during_step_two_survives() {
        struct WritesDuringGet(PathBuf, MemoryVault);
        impl Vault for WritesDuringGet {
            fn get(&self, k: &AccountKey) -> Result<Option<Token>, VaultError> {
                fs::write(&self.0, RENEWED).map_err(|e| VaultError::Failed(e.to_string()))?;
                self.1.get(k)
            }
            fn set(&self, k: &AccountKey, t: &Token) -> Result<(), VaultError> {
                self.1.set(k, t)
            }
            fn delete(&self, k: &AccountKey) -> Result<(), VaultError> {
                self.1.delete(k)
            }
            fn name(&self) -> Option<&'static str> {
                None
            }
        }
        let dir = TempDir::new("ecrivain");
        dir.write(FAKE.as_bytes());
        let v = WritesDuringGet(dir.token(), MemoryVault::with([(imported(), Token::from(FAKE))]));
        assert!(matches!(end(&dir, &v, FAKE), Finish::Kept(why) if why.contains("autre jeton")));
        assert_eq!(dir.content(), Some(RENEWED.as_bytes().to_vec()), "le jeton écrit entre-temps n'est pas détruit");
    }

    /// Fichier tenu ouvert en écriture par un autre programme : l'ouverture
    /// exclusive échoue, rien n'est touché ; une fois rendu, l'étape 2 passe.
    #[cfg(windows)]
    #[test]
    fn file_held_open_for_writing_is_not_touched() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = TempDir::new("tenu-ecriture");
        dir.write(FAKE.as_bytes());
        let writer = fs::OpenOptions::new().read(true).write(true).share_mode(0x7).open(dir.token()).unwrap_or_else(|e| panic!("{e}"));
        let v = MemoryVault::with([(imported(), Token::from(FAKE))]);
        assert!(matches!(end(&dir, &v, FAKE), Finish::Kept(why) if why.contains("autre programme")));
        assert!(wipe_file(&dir.token()).is_err(), "suppression demandée : refusée aussi");
        drop(writer);
        assert_eq!(dir.content(), Some(FAKE.as_bytes().to_vec()), "rien n'est touché");
        assert_eq!(end(&dir, &v, FAKE), Finish::Wiped);
        assert!(!dir.token().exists());
    }

    /// Import refusé par l'utilisateur : ce fichier-là n'est plus lu ni
    /// migré ; un fichier changé depuis l'est de nouveau.
    #[test]
    fn declined_file_is_not_imported_again() {
        let dir = TempDir::new("refuse");
        dir.write(FAKE.as_bytes());
        let mark = file_mark(&dir.token());
        assert!(mark.is_some());
        let v = MemoryVault::new();
        assert_eq!(migrate(&dir.token(), &v, &dir.lock(), true, mark), Migration::Declined);
        assert_eq!(v.calls(), 0);
        dir.write(format!("{RENEWED}\n").as_bytes());
        assert_eq!(migrate(&dir.token(), &v, &dir.lock(), true, mark), Migration::Written(Token::from(RENEWED)));
    }

    /// Avant « Supprimer le fichier de jeton en clair » : le coffre a-t-il une copie ?
    #[test]
    fn wipe_check_says_whether_the_vault_has_a_copy() {
        let dir = TempDir::new("verif");
        let keys = [imported(), AccountKey::of_user(5)];
        dir.write(FAKE.as_bytes());
        assert_eq!(check_wipe(&dir.token(), &MemoryVault::new(), &keys).0, WipeCheck::OnlyCopy);
        assert_eq!(check_wipe(&dir.token(), &MemoryVault::with([(AccountKey::of_user(5), Token::from(FAKE))]), &keys).0, WipeCheck::AlsoInVault);
        let broken = MemoryVault::new();
        broken.set_faults(Faults { fail_get: true, ..Faults::default() });
        assert!(matches!(check_wipe(&dir.token(), &broken, &keys).0, WipeCheck::Unknown(_)));
        dir.write(&[0u8; 8]);
        assert_eq!(check_wipe(&dir.token(), &MemoryVault::new(), &keys).0, WipeCheck::NoToken);
        let _ = fs::remove_file(dir.token());
        assert_eq!(check_wipe(&dir.token(), &MemoryVault::new(), &keys).0, WipeCheck::Gone);
    }

    /// Relecture « absent » une seule fois (coffre qui sert encore une
    /// réponse périmée) : l'entrée écrite n'est pas retirée sur cette foi.
    #[test]
    fn single_absent_read_back_is_read_again() {
        struct OnceMissing(MemoryVault, AtomicUsize);
        impl Vault for OnceMissing {
            fn get(&self, k: &AccountKey) -> Result<Option<Token>, VaultError> {
                // Lectures 0 et 1 : absence vérifiée avant d'écrire ; lecture 2 : relecture, à tort absente.
                if self.1.fetch_add(1, Ordering::SeqCst) == 2 {
                    return Ok(None);
                }
                self.0.get(k)
            }
            fn set(&self, k: &AccountKey, t: &Token) -> Result<(), VaultError> {
                self.0.set(k, t)
            }
            fn delete(&self, k: &AccountKey) -> Result<(), VaultError> {
                self.0.delete(k)
            }
            fn name(&self) -> Option<&'static str> {
                None
            }
        }
        let dir = TempDir::new("relecture-absente");
        dir.write(FAKE.as_bytes());
        let v = OnceMissing(MemoryVault::new(), AtomicUsize::new(0));
        assert_eq!(run(&dir, &v), Migration::Written(Token::from(FAKE)));
        assert_eq!(v.0.peek(&imported()), Some(Token::from(FAKE)));
    }

    #[test]
    fn vault_that_forgets_keeps_file() {
        let dir = TempDir::new("oubli");
        dir.write(FAKE.as_bytes());
        let m = run(&dir, &Forgetful);
        assert!(matches!(m, Migration::Failed { .. }), "{m:?}");
        assert_eq!(dir.content(), Some(FAKE.as_bytes().to_vec()));
    }

    /// Verrou tenu par une autre instance : rien n'est touché, la session
    /// continue avec le fichier ; l'étape 2 non plus.
    #[test]
    fn busy_lock_keeps_everything() {
        let dir = TempDir::new("verrou");
        dir.write(FAKE.as_bytes());
        let other = dir.lock().acquire().unwrap_or_else(|e| panic!("{e}"));
        let v = MemoryVault::with([(imported(), Token::from(FAKE))]);
        let short = InstanceLock::new(Some(dir.0.join("comptes.lock")), Duration::from_millis(30));
        let m = migrate(&dir.token(), &v, &short, true, None);
        assert!(matches!(&m, Migration::Failed { why, .. } if why.contains("autre instance")), "{m:?}");
        // Le texte dit ce qui se passe : rien de copié maintenant, jeton du fichier
        // seulement utilisable (un autre compte peut être rouvert), nouvel essai ensuite.
        let Migration::Failed { why, .. } = &m else { unreachable!() };
        assert!(why.contains("rien n'est copié dans le coffre à ce lancement") && why.contains("reste utilisable pour cette session") && why.contains("nouvel essai au lancement suivant"), "{why}");
        assert!(!why.contains("sert pour cette session"), "{why}");
        assert!(matches!(finish(&dir.token(), &v, &short, &Token::from(FAKE)), Finish::Kept(_)));
        assert_eq!(v.calls(), 0, "coffre pas touché");
        assert_eq!(dir.content(), Some(FAKE.as_bytes().to_vec()));
        drop(other);
    }

    /// Deux instances entrelacées sans verrou commun (pire cas) : A écrit,
    /// B passe entièrement (étapes 1 et 2) pendant la relecture de A, puis
    /// cette relecture échoue. A ne retire pas l'entrée (le fichier n'a plus
    /// le jeton) : à la fin, le jeton est toujours dans le coffre.
    #[test]
    fn interleaved_instances_never_lose_the_token() {
        struct Interleave {
            shared: Arc<MemoryVault>,
            path: PathBuf,
            gets: Mutex<usize>,
            b: Mutex<Option<(Migration, Finish)>>,
        }
        impl Vault for Interleave {
            fn get(&self, k: &AccountKey) -> Result<Option<Token>, VaultError> {
                let n = {
                    let mut g = self.gets.lock().unwrap_or_else(|e| e.into_inner());
                    *g += 1;
                    *g
                };
                if n == 3 {
                    // Relecture de A (après deux lectures « absent ») : B passe entièrement.
                    let lock = InstanceLock::none();
                    let m = migrate(&self.path, self.shared.as_ref(), &lock, true, None);
                    let f = finish(&self.path, self.shared.as_ref(), &lock, &Token::from(FAKE));
                    *self.b.lock().unwrap_or_else(|e| e.into_inner()) = Some((m, f));
                    return Err(VaultError::Failed("relecture interrompue (test)".into()));
                }
                self.shared.get(k)
            }
            fn set(&self, k: &AccountKey, t: &Token) -> Result<(), VaultError> {
                self.shared.set(k, t)
            }
            fn delete(&self, k: &AccountKey) -> Result<(), VaultError> {
                self.shared.delete(k)
            }
            fn name(&self) -> Option<&'static str> {
                None
            }
        }
        let dir = TempDir::new("entrelace");
        dir.write(FAKE.as_bytes());
        let shared = Arc::new(MemoryVault::new());
        let a = Interleave { shared: shared.clone(), path: dir.token(), gets: Mutex::new(0), b: Mutex::new(None) };
        let ma = migrate(&dir.token(), &a, &InstanceLock::none(), true, None);
        assert!(matches!(ma, Migration::Failed { .. }), "A : {ma:?}");
        let mb = a.b.lock().unwrap_or_else(|e| e.into_inner()).take();
        assert_eq!(mb, Some((Migration::Stored(Token::from(FAKE)), Finish::Wiped)), "B a fini la migration");
        assert!(!dir.token().exists());
        assert_eq!(shared.peek(&imported()), Some(Token::from(FAKE)), "le jeton est toujours quelque part");
    }

    /// Deux instances en même temps, verrou commun, coffre en mémoire
    /// partagé : quel que soit l'ordre, le jeton est dans le coffre et le
    /// fichier supprimé une fois les deux étapes faites.
    #[test]
    fn concurrent_instances_with_lock() {
        for round in 0..20 {
            let dir = TempDir::new("simultane");
            dir.write(FAKE.as_bytes());
            let shared = Arc::new(MemoryVault::new());
            let results: Vec<(Migration, Option<Finish>)> = std::thread::scope(|s| {
                let hs: Vec<_> = (0..2)
                    .map(|_| {
                        let (path, lock, v) = (dir.token(), dir.lock(), shared.clone());
                        s.spawn(move || {
                            let m = migrate(&path, v.as_ref(), &lock, true, None);
                            let f = m.pending().map(|t| finish(&path, v.as_ref(), &lock, t));
                            (m, f)
                        })
                    })
                    .collect();
                hs.into_iter().map(|h| h.join().unwrap_or_else(|_| panic!("fil"))).collect()
            });
            assert_eq!(shared.peek(&imported()), Some(Token::from(FAKE)), "tour {round} : {results:?}");
            assert!(results.iter().any(|(m, _)| *m == Migration::Written(Token::from(FAKE))), "tour {round} : {results:?}");
            assert!(results.iter().all(|r| matches!(r, (Migration::Stored(_), Some(Finish::Wiped)) | (Migration::Written(_), None))), "tour {round} : {results:?}");
            assert!(!dir.token().exists());
        }
    }

    /// Avant suppression, le contenu est remplacé par des zéros de même longueur.
    #[test]
    fn wipe_overwrites_then_removes() {
        let dir = TempDir::new("effacement");
        let raw = format!("{FAKE}\r\n");
        dir.write(raw.as_bytes());
        assert_eq!(overwrite_with_zeros(&dir.token()).map_err(|e| e.to_string()), Ok(()));
        assert_eq!(dir.content(), Some(vec![0u8; raw.len()]), "zéros, même longueur");
        dir.write(raw.as_bytes());
        assert_eq!(wipe_file(&dir.token()).map_err(|e| e.to_string()), Ok(()));
        assert!(!dir.token().exists());
        assert_eq!(wipe_file(&dir.token()).map_err(|e| e.to_string()), Ok(()), "déjà absent");
    }

    /// Un fichier qui a un autre nom (lien physique) n'est pas écrasé : les
    /// zéros atteindraient aussi l'autre nom.
    #[test]
    fn wipe_refuses_hard_links_and_directories() {
        let dir = TempDir::new("liens");
        dir.write(FAKE.as_bytes());
        let other = dir.0.join("autre-nom");
        fs::hard_link(dir.token(), &other).unwrap_or_else(|e| panic!("{e}"));
        assert!(wipe_file(&dir.token()).is_err(), "lien physique refusé");
        assert_eq!(fs::read(&other).ok(), Some(FAKE.as_bytes().to_vec()), "l'autre nom n'est pas écrasé");
        assert_eq!(dir.content(), Some(FAKE.as_bytes().to_vec()));
        // Un dossier : refusé, intact.
        let d2 = TempDir::new("dossier-efface");
        fs::create_dir_all(d2.token().join("dedans")).unwrap_or_else(|e| panic!("{e}"));
        assert!(wipe_file(&d2.token()).is_err());
        assert!(d2.token().join("dedans").is_dir());
    }

    /// Un lien symbolique n'est pas suivi : sa cible n'est pas écrasée.
    /// (Créer un lien demande un droit que Windows n'accorde pas toujours :
    /// sans lui, ce cas est sauté, et dit.)
    #[test]
    fn wipe_does_not_follow_symbolic_links() {
        let dir = TempDir::new("lien-symbolique");
        let target = dir.0.join("cible");
        fs::write(&target, FAKE).unwrap_or_else(|e| panic!("{e}"));
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(&target, dir.token());
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&target, dir.token());
        #[cfg(not(any(windows, unix)))]
        let made: io::Result<()> = Err(io::Error::other("pas de lien"));
        if let Err(e) = made {
            eprintln!("lien symbolique impossible à créer ici ({e}) : cas sauté");
            return;
        }
        assert!(wipe_file(&dir.token()).is_err(), "lien refusé");
        assert_eq!(fs::read(&target).ok(), Some(FAKE.as_bytes().to_vec()), "cible intacte");
    }

    /// Une lecture qui répond à tort « absent » n'entraîne pas d'écriture
    /// par-dessus un jeton existant : l'absence est vérifiée une deuxième fois.
    #[test]
    fn false_absence_never_overwrites() {
        struct Flaky {
            inner: MemoryVault,
            first: Mutex<bool>,
        }
        impl Vault for Flaky {
            fn get(&self, k: &AccountKey) -> Result<Option<Token>, VaultError> {
                if std::mem::take(&mut *self.first.lock().unwrap_or_else(|e| e.into_inner())) {
                    return Ok(None);
                }
                self.inner.get(k)
            }
            fn set(&self, k: &AccountKey, t: &Token) -> Result<(), VaultError> {
                self.inner.set(k, t)
            }
            fn delete(&self, k: &AccountKey) -> Result<(), VaultError> {
                self.inner.delete(k)
            }
            fn name(&self) -> Option<&'static str> {
                None
            }
        }
        let dir = TempDir::new("faux-absent");
        dir.write(FAKE.as_bytes());
        let v = Flaky { inner: MemoryVault::with([(imported(), Token::from(RENEWED))]), first: Mutex::new(true) };
        let m = run(&dir, &v);
        assert_eq!(v.inner.peek(&imported()), Some(Token::from(RENEWED)), "jeton récent pas écrasé ({m:?})");
        assert_eq!(dir.content(), Some(FAKE.as_bytes().to_vec()));
    }

    #[test]
    fn unreadable_file_is_left_alone() {
        // Pas du texte.
        let dir = TempDir::new("binaire");
        dir.write(&[0xff, 0xfe, 0x00, 0x41]);
        let v = MemoryVault::new();
        assert!(matches!(run(&dir, &v), Migration::Unreadable(_)));
        assert_eq!(dir.content(), Some(vec![0xff, 0xfe, 0x00, 0x41]));
        // Trop gros pour être un jeton.
        let big = vec![b'a'; MAX_SIZE as usize + 1];
        dir.write(&big);
        assert!(matches!(run(&dir, &v), Migration::Unreadable(_)));
        assert_eq!(dir.content().map(|c| c.len()), Some(big.len()));
        // Un dossier à la place du fichier.
        let d2 = TempDir::new("dossier");
        fs::create_dir_all(d2.token()).unwrap_or_else(|e| panic!("{e}"));
        assert!(matches!(run(&d2, &v), Migration::Unreadable(_)));
        assert!(d2.token().is_dir());
        assert!(v.is_empty());
    }

    #[test]
    fn missing_file_does_nothing() {
        let dir = TempDir::new("absent");
        let v = MemoryVault::new();
        assert_eq!(run(&dir, &v), Migration::NoFile);
        assert_eq!(v.calls(), 0);
    }

    /// Coffre partagé entre processus, simulé par un fichier (jamais le vrai
    /// coffre) : une ligne `clé<TAB>jeton` par entrée, réécrit en entier.
    struct FileVault {
        path: PathBuf,
        /// Pause avant chaque appel (processus tué au milieu).
        pause: Duration,
    }

    impl FileVault {
        fn entries(&self) -> Result<Vec<(String, String)>, VaultError> {
            match fs::read_to_string(&self.path) {
                Ok(t) => Ok(t.lines().filter_map(|l| l.split_once('\t')).map(|(k, v)| (k.to_string(), v.to_string())).collect()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
                Err(e) => Err(VaultError::Failed(e.to_string())),
            }
        }
        fn save(&self, entries: &[(String, String)]) -> Result<(), VaultError> {
            let text: String = entries.iter().map(|(k, v)| format!("{k}\t{v}\n")).collect();
            let tmp = self.path.with_extension(format!("tmp{}", std::process::id()));
            fs::write(&tmp, text).and_then(|()| fs::rename(&tmp, &self.path)).map_err(|e| VaultError::Failed(e.to_string()))
        }
    }

    impl Vault for FileVault {
        fn get(&self, k: &AccountKey) -> Result<Option<Token>, VaultError> {
            std::thread::sleep(self.pause);
            Ok(self.entries()?.into_iter().find(|(key, _)| key == k.as_str()).map(|(_, v)| Token::from(v)))
        }
        fn set(&self, k: &AccountKey, t: &Token) -> Result<(), VaultError> {
            std::thread::sleep(self.pause);
            let mut e = self.entries()?;
            e.retain(|(key, _)| key != k.as_str());
            e.push((k.as_str().to_string(), t.expose().to_string()));
            self.save(&e)
        }
        fn delete(&self, k: &AccountKey) -> Result<(), VaultError> {
            std::thread::sleep(self.pause);
            let mut e = self.entries()?;
            e.retain(|(key, _)| key != k.as_str());
            self.save(&e)
        }
        fn name(&self) -> Option<&'static str> {
            Some("coffre de test (fichier)")
        }
    }

    const CHILD_DIR: &str = "FERROCORD_TEST_MIGRATION_DOSSIER";
    const CHILD_ROLE: &str = "FERROCORD_TEST_MIGRATION_ROLE";
    const SELF_TEST: &str = "account::migrate::tests::two_real_processes_share_lock_and_migration";

    /// Rôle d'un processus enfant : migrer (étapes 1 et 2), migrer lentement
    /// (pour être tué au milieu), ou tenir le verrou jusqu'à être tué.
    fn child(dir: &Path, role: &str) {
        let lock = InstanceLock::new(Some(dir.join("comptes.lock")), Duration::from_secs(20));
        let pause = if role == "lent" { Duration::from_millis(150) } else { Duration::ZERO };
        let v = FileVault { path: dir.join("coffre.txt"), pause };
        let path = dir.join("token");
        match role {
            "tenir" => {
                let _held = lock.acquire().unwrap_or_else(|e| panic!("{e}"));
                println!("verrou-tenu");
                let _ = io::stdout().flush();
                std::thread::sleep(Duration::from_secs(60));
            }
            _ => {
                let m = migrate(&path, &v, &lock, true, None);
                let f = m.pending().map(|t| finish(&path, &v, &lock, t));
                println!("resultat {m:?} {f:?}");
            }
        }
    }

    /// Deux vrais processus (ce binaire de test relancé) en concurrence pour
    /// le verrou et la migration, dans un dossier temporaire, avec un coffre
    /// partagé simulé par un fichier. À la fin, le jeton existe quelque
    /// part ; le verrou d'un processus tué est rendu.
    #[test]
    fn two_real_processes_share_lock_and_migration() {
        use std::process::{Command, Stdio};
        if let (Some(dir), Ok(role)) = (std::env::var_os(CHILD_DIR), std::env::var(CHILD_ROLE)) {
            return child(Path::new(&dir), &role);
        }
        let dir = TempDir::new("processus");
        let exe = std::env::current_exe().unwrap_or_else(|e| panic!("{e}"));
        let spawn = |role: &str| {
            Command::new(&exe)
                .args(["--exact", SELF_TEST, "--nocapture", "--test-threads=1"])
                .env(CHILD_DIR, &dir.0)
                .env(CHILD_ROLE, role)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap_or_else(|e| panic!("{e}"))
        };
        let vault = FileVault { path: dir.0.join("coffre.txt"), pause: Duration::ZERO };
        let somewhere = |what: &str| {
            let in_file = read(&dir.token()).ok().flatten().and_then(|t| Token::parse(&t).ok()) == Some(Token::from(FAKE));
            let in_vault = vault.get(&imported()) == Ok(Some(Token::from(FAKE)));
            assert!(in_file || in_vault, "{what} : jeton perdu");
        };

        // 1. Deux processus migrent en même temps.
        dir.write(FAKE.as_bytes());
        let (a, b) = (spawn("migrer"), spawn("migrer"));
        for c in [a, b] {
            let out = c.wait_with_output().unwrap_or_else(|e| panic!("{e}"));
            let text = String::from_utf8_lossy(&out.stdout);
            assert!(out.status.success() && text.contains("resultat"), "{text}");
            assert!(!text.contains(FAKE), "jeton dans la sortie");
        }
        somewhere("deux processus");
        assert_eq!(vault.get(&imported()), Ok(Some(Token::from(FAKE))));
        assert!(!dir.token().exists(), "fichier supprimé après les deux étapes");

        // 2. Un processus tué au milieu de sa migration (coffre lent).
        let _ = fs::remove_file(&vault.path);
        dir.write(FAKE.as_bytes());
        let mut slow = spawn("lent");
        std::thread::sleep(Duration::from_millis(400));
        let _ = slow.kill();
        let _ = slow.wait();
        somewhere("processus tué au milieu");
        let probe = InstanceLock::new(Some(dir.0.join("comptes.lock")), Duration::from_secs(5));
        assert!(probe.acquire().is_ok(), "verrou du processus tué rendu");
        // Relances : la migration va au bout (étape 2 au plus tôt au lancement
        // qui trouve le jeton déjà copié).
        for _ in 0..2 {
            let out = spawn("migrer").wait_with_output().unwrap_or_else(|e| panic!("{e}"));
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
            somewhere("relance");
        }
        assert_eq!(vault.get(&imported()), Ok(Some(Token::from(FAKE))));
        assert!(!dir.token().exists());

        // 3. Un processus tient le verrou : l'autre attend puis renonce ; tué, il le rend.
        let mut holder = spawn("tenir");
        let mut line = String::new();
        if let Some(out) = holder.stdout.take() {
            let mut r = io::BufReader::new(out);
            while !line.contains("verrou-tenu") {
                line.clear();
                if io::BufRead::read_line(&mut r, &mut line).unwrap_or(0) == 0 {
                    break;
                }
            }
        }
        assert!(line.contains("verrou-tenu"), "{line}");
        let short = InstanceLock::new(Some(dir.0.join("comptes.lock")), Duration::from_millis(100));
        assert!(short.acquire().is_err(), "verrou tenu par l'autre processus");
        let _ = holder.kill();
        let _ = holder.wait();
        assert!(probe.acquire().is_ok(), "verrou rendu à la mort du processus");
    }

    /// Pendant la vérification et les zéros de l'étape 2, le fichier ne peut
    /// être ni renommé ni remplacé (ouvert sans partage de suppression).
    #[cfg(windows)]
    #[test]
    fn file_cannot_be_renamed_during_check_and_zeros() {
        let dir = TempDir::new("renomme");
        dir.write(FAKE.as_bytes());
        let f = open_locked(&dir.token()).unwrap_or_else(|e| panic!("{e}"));
        let other = dir.0.join("ailleurs");
        assert!(fs::rename(dir.token(), &other).is_err(), "renommage refusé tant que le descripteur est ouvert");
        assert!(fs::remove_file(dir.token()).is_err(), "suppression refusée tant que le descripteur est ouvert");
        drop(f);
        assert_eq!(dir.content(), Some(FAKE.as_bytes().to_vec()));
    }

    /// Le chemin désigne un autre fichier quand on le rouvre pour supprimer
    /// (renommé puis remplacé entre la fermeture et la réouverture) : rien
    /// n'est supprimé, le fichier qui contient un jeton reste intact.
    #[test]
    fn replaced_path_is_not_removed_after_zeros() {
        let dir = TempDir::new("remplace");
        let first = dir.0.join("premier");
        fs::write(&first, FAKE.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
        dir.write(RENEWED.as_bytes());
        let f = open_locked(&first).unwrap_or_else(|e| panic!("{e}"));
        let len = checked(&f).unwrap_or_else(|e| panic!("{e}"));
        // Zéros sur `premier`, puis le chemin `token` (un autre fichier) est rouvert.
        let r = zero_then_remove(f, len, &dir.token());
        assert!(matches!(&r, Err(WipeFailure::Emptied(why)) if why.contains("autre fichier")), "{r:?}");
        assert_eq!(dir.content(), Some(RENEWED.as_bytes().to_vec()), "l'autre fichier n'est ni écrasé ni supprimé");
        assert_eq!(fs::read(&first).ok(), Some(vec![0u8; FAKE.len()]), "le fichier ouvert a bien été écrasé");
    }

    /// Même chemin, toujours vide de jeton : supprimé.
    #[test]
    fn same_zeroed_path_is_removed() {
        let dir = TempDir::new("memechemin");
        dir.write(FAKE.as_bytes());
        let f = open_locked(&dir.token()).unwrap_or_else(|e| panic!("{e}"));
        let len = checked(&f).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(zero_then_remove(f, len, &dir.token()), Ok(()));
        assert!(!dir.token().exists());
    }
}
