//! Table d'états de la migration et des comptes, parcourue par programme.
//!
//! Chaque parcours fait trois lancements de la vraie application (sans
//! fenêtre) avec son vrai fil des comptes, son vrai fil du coffre et son
//! verrou ; seuls Discord (connexion factice : READY avec l'identité du
//! jeton, ou « jeton refusé »), le coffre (en mémoire) et l'heure murale
//! sont simulés. Les fichiers sont dans un dossier temporaire.
//!
//! Dimensions :
//! - fichier : absent, valide, vide, zéros, abîmé, avec marque d'ordre des octets ;
//! - coffre : absent, même jeton, autre jeton, indisponible, lent (délai
//!   dépassé), écriture refusée, relecture différente, suppression refusée ;
//! - verrou entre instances : libre, tenu par une autre instance (1er lancement) ;
//! - Discord : accepte, refuse, ou refuse seulement le jeton du coffre (le
//!   fichier en a un autre ; compte importé déjà connu, ou en double) ;
//! - comptes : compte importé seul ; un autre compte, dernier utilisé ;
//!   le même compte enregistré deux fois (importé + saisi, autre jeton) ;
//! - horloge : 2e lancement 25 h après le 1er, ou 1 h après ; instant de la
//!   copie effacé de `settings.toml`, ou dans le futur ; horloge juste puis
//!   en retard puis corrigée, ou en avance puis corrigée (panne possible
//!   aussi au 3e lancement) ;
//! - suppression du fichier : normale, refusée après les zéros (à chaque
//!   fois), ou processus arrêté entre les zéros et la suppression ;
//! - un événement à l'appel n° k au coffre du lancement 1 ou 2 :
//!   - arrêt du processus (le coffre ne répond plus) ;
//!   - panne de courant : le coffre perd toute écriture faite depuis moins
//!     de 24 h (heure de la table) et le lancement s'arrête là ;
//!   - action de l'utilisateur, demandée pendant que cet appel est retenu,
//!     puis confirmée sur la question affichée : retrait du compte importé,
//!     déconnexion, suppression du fichier, saisie d'un jeton renouvelé du
//!     même compte suivie de chacune des trois réponses (aucune, non, oui) à
//!     la question de remplacement ;
//!   - après le refus du jeton du coffre, « Essayer le jeton du fichier »,
//!     suivi de chacune des trois réponses à la question de remplacement ;
//!   - la même question, mais l'autre instance agit entre la question et la
//!     confirmation (étape 2 faite, ou compte importé retiré) ;
//!   - l'autre instance agit pendant que cet appel est retenu (étape 2, ou
//!     retrait du compte importé, verrou pris).
//!
//! Invariant vérifié avant chaque appel au coffre, après chaque image et à
//! la fin du 3e lancement : chaque jeton présent au départ (fichier, coffre)
//! et chaque jeton saisi puis enregistré est lisible dans le fichier ou dans
//! le coffre — sauf ce que l'utilisateur a accepté de perdre, d'après la
//! question qu'il a confirmée : retirer un compte (ou s'en déconnecter) ne
//! libère les jetons de son entrée que si la question disait « seul
//! exemplaire » (ou copie inconnue, ou plus de jeton) ; supprimer le fichier
//! ne libère son jeton que si la question disait « seule copie » (ou contenu
//! inconnu) ; une question qui annonçait une copie restante ne libère rien.
//! Un jeton enregistré n'est remplacé que sur un « oui » à la question de
//! remplacement (vérifié à chaque image), dont le texte disait vrai
//! (provenance du jeton, refus annoncé du jeton que l'entrée contient) ; ce « oui » libère seulement ce
//! qui n'était que dans cette entrée. Un fichier vidé n'est
//! jamais présenté comme une copie du jeton.

use super::{
    accounts::{Confirm, STEP_TWO_DELAY},
    App, Backend, Boot, Phase,
};
use crate::account::{migrate, AccountKey, AccountLink, Faults, InstanceLock, LinkOptions, MemoryVault, Token, Vault, VaultError};
use crate::settings::{Account, Settings, SettingsLink, SettingsUpdate};
use crate::state::{model::User, Cmd, Event, Store};
use crate::theme::{ThemeLink, WatchConfig};
use eframe::egui;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, UNIX_EPOCH},
};

const FILE_TOKEN: &str = "MTk4.Fic7Qz.TEST-fichier";
const VAULT_TOKEN: &str = "MTk4.Cof3Wd.TEST-coffre";
/// Jeton du même compte, saisi un autre jour (doublon).
const TWIN_TOKEN: &str = "MTk4.Dbl5Rt.TEST-doublon";
const OTHER_TOKEN: &str = "MTk5.Aut2Kp.TEST-autre";
const RENEWED_TOKEN: &str = "MTk4.Neu8Yh.TEST-renouvele";
const ME: u64 = 1000000000000000042;
const OTHER: u64 = 1000000000000000077;

/// Heure murale du 1er lancement (secondes depuis 1970).
const T1: u64 = 1_800_000_000;
const HOUR: u64 = 3600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileState {
    Absent,
    Valid,
    Empty,
    Zeros,
    Damaged,
    Bom,
}

const FILES: [FileState; 6] = [FileState::Absent, FileState::Valid, FileState::Empty, FileState::Zeros, FileState::Damaged, FileState::Bom];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VaultState {
    Absent,
    Same,
    Other,
    Unavailable,
    Slow,
    WriteRefused,
    ReadBackDifferent,
    DeleteRefused,
}

const VAULTS: [VaultState; 8] = [
    VaultState::Absent,
    VaultState::Same,
    VaultState::Other,
    VaultState::Unavailable,
    VaultState::Slow,
    VaultState::WriteRefused,
    VaultState::ReadBackDifferent,
    VaultState::DeleteRefused,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Accounts {
    /// Seul le compte importé (ou rien).
    Alone,
    /// Un autre compte, dernier utilisé : le compte importé n'est pas rouvert.
    OtherLast,
    /// Le même compte enregistré deux fois : importé et saisi (autre jeton).
    Twin,
    /// Compte importé déjà connu (identité vue lors d'une session précédente).
    Known,
}

/// Heure murale des lancements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Clock {
    /// 2e lancement 25 h après le 1er, 3e 25 h après le 2e.
    Later,
    /// 2e lancement 1 h après le 1er.
    Soon,
    /// Instant de la copie effacé de `settings.toml` avant le 2e lancement.
    Missing,
    /// Instant de la copie dans le futur (100 h après le 2e lancement).
    Future,
    /// Horloge juste, puis en retard, puis corrigée : 1er lancement à T,
    /// 2e à T + 1 h affiché T − 20 h, 3e à T + 5 h (affiché juste).
    Late,
    /// Horloge en avance, puis corrigée : 1er lancement à T affiché
    /// T + 30 h, 2e à T + 1 h, 3e à T + 10 h (affichés justes).
    Ahead,
}

/// Sort de la suppression du fichier (étape 2 ou bouton).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fate {
    Normal,
    /// Zéros écrits, suppression refusée, à chaque fois.
    RemoveFails,
    /// Processus arrêté entre les zéros et la suppression (2e lancement).
    CrashAfterZeros,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Stop,
    Power,
    Remove,
    Logout,
    DeleteFile,
    /// Saisie d'un jeton renouvelé du même compte, puis cette réponse à la
    /// question de remplacement (si elle est posée).
    TypeRenewed(Answer),
    /// Après le refus du jeton du coffre : « Essayer le jeton du fichier »,
    /// puis cette réponse à la question de remplacement.
    TryFile(Answer),
    /// Question de retrait, puis l'autre instance fait l'étape 2, puis « Oui ».
    RemoveRaced,
    /// Question de suppression, puis l'autre instance retire le compte importé, puis « Oui ».
    WipeRaced,
    /// L'autre instance fait l'étape 2 pendant l'appel retenu.
    OtherStepTwo,
    /// L'autre instance retire le compte importé pendant l'appel retenu.
    OtherRemove,
}

/// Réponse à la question « Remplacer le jeton enregistré ? ».
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    /// Pas de réponse (la session se ferme avec le lancement).
    None,
    No,
    Yes,
}

/// Actions des cas où Discord refuse le jeton du coffre (et lui seul).
const TRY_FILE: [Action; 3] = [Action::TryFile(Answer::None), Action::TryFile(Answer::No), Action::TryFile(Answer::Yes)];

const USER_ACTIONS: [Action; 10] = [
    Action::Remove,
    Action::Logout,
    Action::DeleteFile,
    Action::TypeRenewed(Answer::None),
    Action::TypeRenewed(Answer::No),
    Action::TypeRenewed(Answer::Yes),
    Action::RemoveRaced,
    Action::WipeRaced,
    Action::OtherStepTwo,
    Action::OtherRemove,
];

/// Ce que fait l'autre instance (même coffre, même fichier, même verrou).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Other {
    /// Étape 2 (vrai code) avec le jeton du fichier.
    StepTwo,
    /// Retrait du compte importé, confirmé là-bas.
    Remove,
}

/// Un événement à l'appel n° `call` (compté depuis 0) du lancement `launch`.
#[derive(Clone, Copy, Debug)]
struct Happens {
    launch: usize,
    call: usize,
    action: Action,
}

/// Délai des appels au coffre dans la table : long, pour qu'un appel
/// ordinaire ne le dépasse jamais, même sous forte charge (sinon le coffre
/// devient « indéterminé » et le parcours n'est plus celui prévu).
const TIMEOUT: Duration = Duration::from_secs(30);
/// Cas « coffre lent » seulement : chaque appel dort `SLOW`, toujours plus
/// que ce délai-là ; la charge ne peut que l'allonger, jamais l'éviter.
const SLOW_TIMEOUT: Duration = Duration::from_millis(150);
const SLOW: Duration = Duration::from_millis(400);
/// Attente du verrou entre instances quand personne ne le tient durablement
/// (l'autre instance ne le prend que le temps d'une action).
const LOCK_WAIT: Duration = Duration::from_secs(20);
/// Attente du verrou au 1er lancement quand une autre instance le tient
/// pendant tout ce lancement (cas `lock_held` : il doit être refusé).
const LOCK_HELD_WAIT: Duration = Duration::from_millis(20);
/// Attente maximale de l'action sur l'appel retenu, et d'un lancement.
const GATE_WAIT: Duration = Duration::from_secs(20);
const LAUNCH_WAIT: Duration = Duration::from_secs(90);

#[derive(Clone, Copy, Debug)]
struct Case {
    file: FileState,
    vault: VaultState,
    lock_held: bool,
    accepts: bool,
    accounts: Accounts,
    /// Discord refuse le jeton du coffre (et accepte les autres).
    refuses_vault: bool,
    clock: Clock,
    fate: Fate,
}

impl Case {
    fn new(file: FileState, vault: VaultState, lock_held: bool, accepts: bool, accounts: Accounts) -> Self {
        Case { file, vault, lock_held, accepts, accounts, refuses_vault: false, clock: Clock::Later, fate: Fate::Normal }
    }

    fn file_bytes(&self) -> Option<Vec<u8>> {
        match self.file {
            FileState::Absent => None,
            FileState::Valid => Some(FILE_TOKEN.as_bytes().to_vec()),
            FileState::Empty => Some(Vec::new()),
            FileState::Zeros => Some(vec![0; 24]),
            FileState::Damaged => Some("MTk4.Fic\u{200b}7Qz.TEST\n".as_bytes().to_vec()),
            FileState::Bom => Some(format!("\u{feff}{FILE_TOKEN}\r\n").into_bytes()),
        }
    }

    fn file_has_token(&self) -> bool {
        matches!(self.file, FileState::Valid | FileState::Bom)
    }

    /// Contenu du coffre au départ.
    fn vault_entries(&self) -> Vec<(AccountKey, Token)> {
        let mut v = Vec::new();
        let imported = match self.vault {
            VaultState::Same if self.file_has_token() => Some(FILE_TOKEN),
            VaultState::Same | VaultState::Other => Some(VAULT_TOKEN),
            _ => None,
        };
        if let Some(t) = imported {
            v.push((AccountKey::imported(), Token::from(t)));
        }
        match self.accounts {
            Accounts::Alone => {}
            Accounts::OtherLast => v.push((AccountKey::of_user(OTHER), Token::from(OTHER_TOKEN))),
            Accounts::Twin => v.push((AccountKey::of_user(ME), Token::from(TWIN_TOKEN))),
            Accounts::Known => {}
        }
        v
    }

    /// Liste des comptes au départ.
    fn settings(&self) -> Settings {
        let mut s = Settings::default();
        match self.accounts {
            Accounts::Alone => {}
            Accounts::OtherLast => {
                s.accounts.push(Account { user_id: Some(OTHER), name: Some("Autre".into()), ..Account::new(AccountKey::of_user(OTHER)) });
                s.last_account = Some(AccountKey::of_user(OTHER));
            }
            Accounts::Known => {
                s.accounts.push(Account { user_id: Some(ME), name: Some("Moi".into()), ..Account::new(AccountKey::imported()) });
                s.last_account = Some(AccountKey::imported());
            }
            Accounts::Twin => {
                s.accounts.push(Account { user_id: Some(ME), name: Some("Moi".into()), ..Account::new(AccountKey::imported()) });
                s.accounts.push(Account { user_id: Some(ME), name: Some("Moi".into()), ..Account::new(AccountKey::of_user(ME)) });
                s.last_account = Some(AccountKey::imported());
            }
        }
        s
    }

    fn faults(&self) -> Faults {
        match self.vault {
            VaultState::Unavailable => Faults { unavailable: true, ..Faults::default() },
            VaultState::WriteRefused => Faults { fail_set: true, ..Faults::default() },
            VaultState::ReadBackDifferent => Faults { corrupt_set: true, ..Faults::default() },
            VaultState::DeleteRefused => Faults { corrupt_set: true, fail_delete: true, ..Faults::default() },
            _ => Faults::default(),
        }
    }

    /// Jetons qui doivent rester lisibles quelque part.
    fn must_survive(&self) -> Vec<Token> {
        let mut v = Vec::new();
        if self.file_has_token() {
            v.push(Token::from(FILE_TOKEN));
        }
        for (_, t) in self.vault_entries() {
            if !v.contains(&t) {
                v.push(t);
            }
        }
        v
    }

    /// Heure murale affichée au lancement `l` (1 à 3).
    fn time(&self, l: usize) -> u64 {
        match (self.clock, l) {
            (Clock::Late, 2) => T1 - 20 * HOUR,
            (Clock::Ahead, 1) => T1 + 30 * HOUR,
            _ => self.real(l),
        }
    }

    /// Heure réelle du lancement `l` (ce que le coffre garde après une panne en dépend).
    fn real(&self, l: usize) -> u64 {
        match (self.clock, l) {
            (_, 1) => T1,
            (Clock::Late | Clock::Ahead | Clock::Soon, 2) => T1 + HOUR,
            (Clock::Late, _) => T1 + 5 * HOUR,
            (Clock::Ahead, _) => T1 + 10 * HOUR,
            (_, 2) => T1 + 25 * HOUR,
            _ => self.real(2) + 25 * HOUR,
        }
    }

    fn name(&self, h: Option<Happens>) -> String {
        let ev = h.map_or("aucun".to_string(), |h| format!("{:?} à l'appel {} du lancement {}", h.action, h.call, h.launch));
        format!(
            "fichier {:?}, coffre {:?}, verrou {}, Discord {}, comptes {:?}, horloge {:?}, suppression {:?}, événement {ev}",
            self.file,
            self.vault,
            if self.lock_held { "tenu" } else { "libre" },
            if self.refuses_vault { "refuse le jeton du coffre" } else if self.accepts { "accepte" } else { "refuse" },
            self.accounts,
            self.clock,
            self.fate
        )
    }
}

fn in_file(file: &Path, t: &Token) -> bool {
    fs::read(file).ok().and_then(|b| String::from_utf8(b).ok()).and_then(|s| Token::parse(&s).ok()).as_ref() == Some(t)
}

/// Le fichier existe, n'est pas vide et ne contient que des zéros.
fn zeroed(file: &Path) -> bool {
    fs::read(file).is_ok_and(|b| !b.is_empty() && b.iter().all(|x| *x == 0))
}

/// Le jeton est lisible dans le fichier ou dans une entrée du coffre (sauf `except`).
fn readable_except(file: &Path, mem: &MemoryVault, t: &Token, except: Option<&AccountKey>) -> bool {
    in_file(file, t) || mem.snapshot().iter().any(|(k, v)| v == t && Some(k) != except)
}

fn readable(file: &Path, mem: &MemoryVault, t: &Token) -> bool {
    readable_except(file, mem, t, None)
}

/// Ce qui est partagé entre les lancements d'un parcours.
struct World {
    mem: Arc<MemoryVault>,
    file: PathBuf,
    dir: PathBuf,
    must: Mutex<Vec<Token>>,
    violations: Mutex<Vec<String>>,
    /// Contenu du coffre à la fin de chaque lancement, avec son heure (le
    /// départ compte comme écrit depuis longtemps).
    ends: Mutex<Vec<(u64, HashMap<AccountKey, Token>)>>,
}

impl World {
    fn must(&self) -> Vec<Token> {
        self.must.lock().map(|m| m.clone()).unwrap_or_default()
    }

    fn set_must(&self, v: Vec<Token>) {
        if let Ok(mut m) = self.must.lock() {
            *m = v;
        }
    }

    /// Liste tenue du début à la fin du contrôle : une libération (avec
    /// l'action qu'elle accompagne, autre fil) a lieu tout entière avant ou
    /// après, jamais entre la copie de la liste et la lecture des jetons.
    fn check(&self, when: &str) {
        let Ok(must) = self.must.lock() else { return };
        for t in must.iter() {
            if !readable(&self.file, &self.mem, t)
                && let Ok(mut v) = self.violations.lock()
            {
                v.push(format!("jeton illisible {when}"));
            }
        }
    }

    /// L'utilisateur accepte de perdre ce qui n'est que dans l'entrée `key`.
    fn release_entry(&self, key: &AccountKey) {
        if let Ok(mut m) = self.must.lock() {
            m.retain(|t| readable_except(&self.file, &self.mem, t, Some(key)));
        }
    }

    /// Libère ce qui n'est que dans l'entrée `key` et la retire, sans qu'un
    /// contrôle puisse voir l'état intermédiaire.
    fn release_and_delete(&self, key: &AccountKey) {
        if let Ok(mut m) = self.must.lock() {
            m.retain(|t| readable_except(&self.file, &self.mem, t, Some(key)));
            let _ = self.mem.delete(key);
        }
    }

    /// L'utilisateur accepte de perdre ce qui n'est que dans le fichier.
    fn release_file(&self) {
        if let Ok(mut m) = self.must.lock() {
            m.retain(|t| self.mem.snapshot().values().any(|v| v == t));
        }
    }

    /// Ce que le coffre garde après une panne à l'heure `now` : les
    /// écritures d'au moins 24 h (fin du dernier lancement assez ancien).
    fn durable_at(&self, now: u64) -> HashMap<AccountKey, Token> {
        let ends = self.ends.lock().map(|e| e.clone()).unwrap_or_default();
        ends.iter().rev().find(|(t, _)| now >= t.saturating_add(STEP_TWO_DELAY)).map(|(_, s)| s.clone()).unwrap_or_default()
    }

    /// L'autre instance agit (verrou pris, vrai code pour l'étape 2).
    fn other_acts(&self, what: Other) {
        let lock = InstanceLock::new(Some(self.dir.join("comptes.lock")), LOCK_WAIT);
        match what {
            Other::StepTwo => {
                let _ = migrate::finish(&self.file, &*self.mem, &lock, &Token::from(FILE_TOKEN));
            }
            Other::Remove => {
                if let Ok(_held) = lock.acquire() {
                    // Confirmé là-bas : ce qui n'était que dans cette entrée est libéré.
                    self.release_and_delete(&AccountKey::imported());
                }
            }
        }
    }
}

/// Coffre de la table, pour un lancement : en mémoire, avec ses pannes, un
/// appel lent au besoin, l'appel où survient l'événement, et la
/// vérification de l'invariant avant chaque appel.
struct TableVault {
    world: Arc<World>,
    slow: bool,
    /// Arrêt ou panne à cet appel.
    stop_at: Option<(usize, bool)>,
    /// Appel retenu pour une action de l'utilisateur ou de l'autre instance.
    gate: Option<usize>,
    reached: AtomicBool,
    released: AtomicBool,
    dead: AtomicBool,
    calls: AtomicUsize,
    /// Ce que le coffre garde en cas de panne pendant ce lancement.
    durable: HashMap<AccountKey, Token>,
}

impl TableVault {
    fn new(world: &Arc<World>, slow: bool, ev: Option<Happens>, launch: usize, now: u64) -> Self {
        let ev = ev.filter(|h| h.launch == launch);
        Self {
            world: world.clone(),
            slow,
            stop_at: ev.and_then(|h| match h.action {
                Action::Stop => Some((h.call, false)),
                Action::Power => Some((h.call, true)),
                _ => None,
            }),
            gate: ev.filter(|h| !matches!(h.action, Action::Stop | Action::Power)).map(|h| h.call),
            reached: AtomicBool::new(false),
            released: AtomicBool::new(false),
            dead: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
            durable: world.durable_at(now),
        }
    }

    /// Le processus s'arrête ; en cas de panne de courant, le coffre perd
    /// tout ce qui a été écrit depuis moins de 24 h.
    fn die(&self, power: bool) {
        if !self.dead.swap(true, Ordering::SeqCst) && power {
            self.world.mem.restore(self.durable.clone());
            self.world.check("après la panne de courant");
        }
    }

    fn enter(&self, what: &str) -> Result<(), VaultError> {
        self.world.check(&format!("avant l'appel « {what} »"));
        let k = self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some((at, power)) = self.stop_at
            && k == at
        {
            self.die(power);
        }
        if self.dead.load(Ordering::SeqCst) {
            return Err(VaultError::Unavailable("processus arrêté (table)".into()));
        }
        if self.gate == Some(k) {
            self.reached.store(true, Ordering::SeqCst);
            let end = Instant::now() + GATE_WAIT;
            while !self.released.load(Ordering::SeqCst) && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        if self.slow {
            std::thread::sleep(SLOW);
        }
        // Une panne pendant un appel lent : il ne laisse rien.
        if self.dead.load(Ordering::SeqCst) {
            return Err(VaultError::Unavailable("processus arrêté (table)".into()));
        }
        Ok(())
    }
}

impl Vault for TableVault {
    fn get(&self, k: &AccountKey) -> Result<Option<Token>, VaultError> {
        self.enter("lecture")?;
        self.world.mem.get(k)
    }
    fn set(&self, k: &AccountKey, t: &Token) -> Result<(), VaultError> {
        self.enter("écriture")?;
        self.world.mem.set(k, t)
    }
    fn delete(&self, k: &AccountKey) -> Result<(), VaultError> {
        self.enter("suppression")?;
        self.world.mem.delete(k)
    }
    fn name(&self) -> Option<&'static str> {
        Some("coffre de la table")
    }
}

/// Discord factice : à la connexion, READY avec l'identité du jeton
/// (accepte) ou « jeton refusé ».
struct FakeDiscord {
    accepts: bool,
    /// Jeton refusé même quand `accepts`.
    refused: Option<&'static str>,
    pending: Vec<Event>,
    /// Heure réelle du lancement : celle que Discord donne (en-tête `Date`, B1).
    server_ms: i64,
    connected: bool,
}

impl Backend for FakeDiscord {
    fn set_wake(&mut self, _: crate::net::Wake) {}
    fn connect(&mut self, token: Token) {
        self.connected = true;
        let (id, name) = if token.expose() == OTHER_TOKEN { (OTHER, "Autre") } else { (ME, "Moi") };
        self.pending = vec![if self.accepts && self.refused != Some(token.expose()) {
            Event::Ready { me: User::new(id, name.into()), guilds: Vec::new(), dms: Vec::new() }
        } else {
            Event::Fatal { problem: crate::state::Problem::BadToken, bad_token: true }
        }];
        // Une réponse HTTP après READY : l'heure réelle, pas celle de la machine.
        if matches!(self.pending.first(), Some(Event::Ready { .. })) {
            self.pending.push(Event::ServerTime(self.server_ms));
        }
    }
    fn disconnect(&mut self) {
        self.connected = false;
        self.pending.clear();
    }
    fn is_connected(&self) -> bool {
        self.connected
    }
    fn poll(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.pending)
    }
    fn send(&mut self, _: Vec<Cmd>) {}
}

/// Plus rien en cours : ni démarrage, ni lecture, ni écriture, ni étape 2.
fn settled(app: &App) -> bool {
    let idle = !matches!(app.phase, Phase::Starting | Phase::Unlocking(_))
        && !app.finishing
        && !app.wiping_legacy
        && !app.checking_wipe
        && !app.importing
        && app.surveying.is_none()
        && app.storing.is_empty()
        && app.forgetting.is_empty();
    idle && (app.phase == Phase::Login || app.current.as_ref().is_some_and(|c| c.ready))
}

/// Où en est l'action pendant un lancement.
enum Doing {
    Waiting,
    /// Question posée : « Oui » dès qu'elle est prête (après l'action de
    /// l'autre instance, s'il y en a une). `before` : liste des jetons
    /// à garder avant la dernière réponse (remise si l'action n'est pas
    /// faite parce que l'état a changé). `again` : l'action à refaire si la
    /// question disparaît sans réponse (session ouverte entre-temps :
    /// l'utilisateur clique de nouveau).
    Answering { before: Option<Vec<Token>>, stale_seen: usize, race: Option<Other>, answers: usize, again: Option<Action> },
    /// Jeton renouvelé saisi : on attend la question de remplacement (qui
    /// reçoit `answer`) ou l'écriture. `seen` : coffre à l'image d'avant
    /// (une entrée qui passe d'un autre jeton au renouvelé sans « oui » est
    /// une faute). `before` : jetons à garder avant le « oui » (remis si le
    /// renouvelé n'est finalement pas enregistré).
    Typed { token: Token, answer: Answer, seen: HashMap<AccountKey, Token>, yes: bool, before: Option<Vec<Token>>, answers: usize },
    /// « Essayer le jeton du fichier » dès qu'il est proposé (après le refus).
    TryFile(Answer),
    /// L'autre instance agit (fil à part, verrou pris).
    OtherActing(std::thread::JoinHandle<()>),
    Done,
}

/// Fait l'action (appel au coffre retenu).
fn act(app: &mut App, world: &Arc<World>, action: Action) -> Doing {
    let answering = |race| Doing::Answering { before: None, stale_seen: app.stale_count, race, answers: 0, again: Some(action) };
    match action {
        Action::Remove | Action::RemoveRaced => {
            let race = (action == Action::RemoveRaced).then_some(Other::StepTwo);
            let d = answering(race);
            app.ask_remove_account(AccountKey::imported());
            d
        }
        Action::Logout => {
            let d = answering(None);
            app.ask_logout();
            d
        }
        Action::DeleteFile | Action::WipeRaced => {
            // Le bouton n'est visible qu'avec l'avertissement ou la ligne du fichier.
            if app.legacy_file.is_none() && app.legacy_token.is_none() {
                return Doing::Done;
            }
            let race = (action == Action::WipeRaced).then_some(Other::Remove);
            let d = answering(race);
            app.ask_delete_legacy_file();
            d
        }
        Action::TypeRenewed(answer) => {
            // Rien n'est libéré ici : le jeton enregistré n'est remplacé que sur un « oui ».
            app.add_account(Token::from(RENEWED_TOKEN), true);
            Doing::Typed { token: Token::from(RENEWED_TOKEN), answer, seen: world.mem.snapshot(), yes: false, before: None, answers: 0 }
        }
        Action::TryFile(answer) => Doing::TryFile(answer),
        Action::OtherStepTwo | Action::OtherRemove => {
            let what = if action == Action::OtherStepTwo { Other::StepTwo } else { Other::Remove };
            let w = world.clone();
            Doing::OtherActing(std::thread::spawn(move || w.other_acts(what)))
        }
        Action::Stop | Action::Power => Doing::Done,
    }
}

/// Question de retrait qui accepte de perdre le jeton de l'entrée : elle
/// n'annonce aucune copie restante.
fn remove_consents(q: &str) -> bool {
    q.contains("seul exemplaire") || q.contains("n'a plus de jeton")
}

/// Question de suppression qui accepte de perdre le jeton du fichier.
fn wipe_consents(q: &str) -> bool {
    q.contains("seule copie") || q.contains("seul exemplaire")
}

/// Question posée et prête.
fn question(app: &App) -> Option<(Confirm, String)> {
    let c = app.confirm.clone()?;
    let q = match &c {
        Confirm::Remove(k) => app.remove_account_question(k),
        Confirm::Logout => app.logout_confirmation(),
        Confirm::WipeLegacy => app.delete_legacy_question(),
        Confirm::Replace => app.replace_token_question(),
    }?;
    Some((c, q))
}

/// « Oui » à la question `q`, en libérant ce qu'elle accepte de perdre.
/// `false` : l'action est refusée tout de suite (rien n'est fait).
fn answer(app: &mut App, world: &World, c: Confirm, q: &str, problems: &mut Vec<String>) -> bool {
    match c {
        Confirm::Remove(k) => {
            if remove_consents(q) {
                world.release_entry(&k);
            }
            let running = app.finishing;
            if !app.remove_account(k) {
                if !running {
                    problems.push("retrait refusé sans étape 2 en cours".into());
                }
                return false;
            }
            true
        }
        Confirm::Logout => {
            if remove_consents(q)
                && let Some(k) = app.current_account().cloned()
            {
                world.release_entry(&k);
            }
            app.logout();
            true
        }
        Confirm::WipeLegacy => {
            if wipe_consents(q) {
                world.release_file();
            }
            app.delete_legacy_file();
            true
        }
        // Pas posée pendant ces actions-là ; « Non » ne libère rien.
        Confirm::Replace => {
            app.answer_replace_token(false);
            true
        }
    }
}

/// Un lancement : l'application tourne jusqu'à ce que tout soit fini, en
/// vérifiant l'invariant après chaque image ; rend la liste des comptes.
fn launch(case: &Case, l: usize, world: &Arc<World>, vault: Arc<TableVault>, ev: Option<Happens>, settings: Settings, problems: &mut Vec<String>) -> Settings {
    let ctx = egui::Context::default();
    let wait = if case.lock_held && l == 1 { LOCK_HELD_WAIT } else { LOCK_WAIT };
    let lock = InstanceLock::new(Some(world.dir.join("comptes.lock")), wait);
    let timeout = if case.vault == VaultState::Slow { SLOW_TIMEOUT } else { TIMEOUT };
    let boot = Boot {
        backend: Box::new(FakeDiscord { accepts: case.accepts, refused: case.refuses_vault.then_some(VAULT_TOKEN), pending: Vec::new(), server_ms: i64::try_from(case.real(l)).unwrap_or(0) * 1000, connected: false }),
        store: Store::new(),
        theme: ThemeLink::open(None, false, WatchConfig::default()),
        settings: (SettingsLink::open(None, false, WatchConfig::default()).0, Some(SettingsUpdate::Loaded(settings))),
        accounts: AccountLink::start_with(vault.clone(), LinkOptions { lock, vault_timeout: timeout }),
        env_token: None,
        legacy: Some(world.file.clone()),
        images: crate::images::Images::disabled(ctx.clone()),
    };
    let mut app = App::with_context(ctx, boot);
    let now = case.time(l);
    app.set_clock(Arc::new(move || UNIX_EPOCH + Duration::from_secs(now)));
    let end = Instant::now() + LAUNCH_WAIT;
    let mut calm = 0;
    let mut doing = if vault.gate.is_some() { Doing::Waiting } else { Doing::Done };
    let action = ev.filter(|h| h.launch == l).map(|h| h.action);
    let crash = case.fate == Fate::CrashAfterZeros && l == 2;
    let others_act = ev.is_some_and(|h| matches!(h.action, Action::RemoveRaced | Action::WipeRaced | Action::OtherStepTwo | Action::OtherRemove));
    loop {
        app.step();
        world.check(&format!("après une image ({:?})", app.phase));
        // Arrêt entre les zéros et la suppression.
        if crash && zeroed(&world.file) {
            break;
        }
        if matches!(doing, Doing::Waiting)
            && vault.reached.load(Ordering::SeqCst)
            && let Some(a) = action
        {
            doing = act(&mut app, world, a);
            vault.released.store(true, Ordering::SeqCst);
        }
        if let Doing::Answering { before, stale_seen, race, answers, again } = &mut doing {
            // Action confirmée non faite (état changé) : rien n'a été perdu.
            if app.stale_count > *stale_seen {
                *stale_seen = app.stale_count;
                if let Some(b) = before.take() {
                    world.set_must(b);
                }
            }
            if let Some((c, q)) = question(&app) {
                // L'autre instance agit entre la question (déjà lue) et la confirmation.
                if let Some(r) = race.take() {
                    world.other_acts(r);
                }
                if *answers >= 4 {
                    problems.push(format!("question reposée sans fin : {q}"));
                    app.cancel_confirmation();
                    doing = Doing::Done;
                } else {
                    *answers += 1;
                    let b = world.must();
                    if answer(&mut app, world, c, &q, problems) {
                        *before = Some(b);
                    } else {
                        world.set_must(b);
                    }
                }
            } else if settled(&app)
                && *answers == 0
                && let Some(a) = again.take()
            {
                // Question effacée sans réponse (session ouverte) : nouveau clic.
                let r = race.take();
                doing = act(&mut app, world, a);
                if let Doing::Answering { again, race, .. } = &mut doing {
                    *again = None;
                    *race = r;
                }
            } else if settled(&app) {
                // Plus rien à répondre (fait, ou question devenue sans objet).
                if app.confirm.is_some() {
                    app.cancel_confirmation();
                }
                doing = Doing::Done;
            }
        }
        if matches!(&doing, Doing::OtherActing(h) if h.is_finished())
            && let Doing::OtherActing(h) = std::mem::replace(&mut doing, Doing::Done)
        {
            let _ = h.join();
        }
        if let Doing::TryFile(answer) = doing {
            if app.offers_file_token() {
                app.try_file_token();
                doing = Doing::Typed { token: Token::from(FILE_TOKEN), answer, seen: world.mem.snapshot(), yes: false, before: None, answers: 0 };
            } else if settled(&app) {
                doing = Doing::Done;
            }
        }
        if let Doing::Typed { token, answer, seen, yes, before, answers } = &mut doing {
            let renewed = token.clone();
            // Invariant : un jeton enregistré n'est remplacé que sur un « oui ».
            let now = world.mem.snapshot();
            if !*yes && now.iter().any(|(k, v)| v == &renewed && seen.get(k).is_some_and(|o| o != &renewed)) {
                problems.push(format!("jeton enregistré remplacé sans « oui » ({answer:?})"));
            }
            *seen = now;
            let asked = app.confirm == Some(Confirm::Replace) && app.replace_token_question().is_some();
            if asked && *answer != Answer::None {
                if *answers >= 4 {
                    problems.push("question de remplacement reposée sans fin".into());
                    app.answer_replace_token(false);
                } else if *answer == Answer::Yes {
                    *answers += 1;
                    let q = app.replace_token_question().unwrap_or_default();
                    let existing = app.replacing.as_ref().and_then(|r| r.existing.clone());
                    // Le texte dit vrai : provenance, et refus annoncé du jeton que l'entrée contient.
                    let from_file = renewed == Token::from(FILE_TOKEN);
                    let says_file = q.contains("du fichier");
                    let says_typed = q.contains("venez de saisir");
                    if says_file != from_file || says_typed == from_file {
                        problems.push(format!("question de remplacement fausse sur la provenance : {q}"));
                    }
                    if q.contains("Discord a refusé le jeton enregistré") && !(case.refuses_vault && existing == Some(Token::from(VAULT_TOKEN))) {
                        problems.push(format!("question de remplacement fausse sur le refus : {q}"));
                    }
                    // Consenti : ce qui n'est que dans l'entrée de ce compte.
                    if let Some(k) = app.replacing.as_ref().map(|r| r.account.key.clone()) {
                        if before.is_none() {
                            *before = Some(world.must());
                        }
                        world.release_entry(&k);
                    }
                    *yes = true;
                    app.answer_replace_token(true);
                } else {
                    app.answer_replace_token(false);
                }
            } else if settled(&app) && (asked || app.replacing.is_none()) {
                if readable(&world.file, &world.mem, &renewed) {
                    if let Ok(mut m) = world.must.lock() {
                        m.push(renewed);
                    }
                } else if let Some(b) = before.take() {
                    // « Oui » mais rien d'écrit : rien n'a été remplacé, tout doit être là.
                    world.set_must(b);
                }
                doing = Doing::Done;
            }
        }
        // Fichier vidé par ce processus (que des zéros) : jamais présenté comme
        // une copie du jeton. (Vidé par l'autre instance, il n'est su qu'à la
        // relecture : les questions relisent, et une confirmation périmée est reposée.)
        if settled(&app) && zeroed(&world.file) && !others_act && app.legacy_file_known() {
            problems.push("fichier vidé présenté comme une copie du jeton".into());
        }
        // Panne de courant : le lancement s'arrête net.
        if vault.dead.load(Ordering::SeqCst) && vault.stop_at.is_some_and(|(_, power)| power) {
            break;
        }
        let quiet = matches!(doing, Doing::Done | Doing::Waiting);
        calm = if settled(&app) && quiet { calm + 1 } else { 0 };
        // Fini, ou appel jamais atteint (moins d'appels que prévu) : rien à faire.
        if calm >= 5 {
            break;
        }
        if Instant::now() > end {
            problems.push(format!("ne se termine pas ({:?})", app.phase));
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    if let Doing::OtherActing(h) = doing {
        let _ = h.join();
    }
    // Panne de courant après le dernier appel du lancement.
    if let Some((_, true)) = vault.stop_at {
        vault.die(true);
    }
    let s = app.prefs.now.clone();
    drop(app);
    s
}

/// Retire `path` des suppressions refusées.
fn allow_remove(path: &Path) {
    if let Ok(mut v) = migrate::REMOVE_FAILS.lock() {
        v.retain(|p| p != path);
    }
}

fn refuse_remove(path: &Path) {
    if let Ok(mut v) = migrate::REMOVE_FAILS.lock() {
        v.push(path.to_path_buf());
    }
}

/// Un parcours : trois lancements, l'événement éventuel pendant le 1er ou le
/// 2e. Rend les problèmes et le nombre d'appels au coffre de chaque lancement.
fn walk(case: &Case, ev: Option<Happens>, n: usize) -> (Vec<String>, [usize; 3]) {
    // Numéro unique dans le processus : les deux tests de la table tournent en même temps.
    static WALKS: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("ferrocord-table-{}-{n}-{}", std::process::id(), WALKS.fetch_add(1, Ordering::Relaxed)));
    let _ = fs::remove_dir_all(&dir);
    if let Err(e) = fs::create_dir_all(&dir) {
        return (vec![format!("dossier temporaire : {e}")], [0, 0, 0]);
    }
    let file = dir.join("token");
    if let Some(b) = case.file_bytes() {
        let _ = fs::write(&file, b);
    }
    let mem = Arc::new(MemoryVault::with(case.vault_entries()));
    mem.set_faults(case.faults());
    let world = Arc::new(World {
        mem: mem.clone(),
        file: file.clone(),
        dir: dir.clone(),
        must: Mutex::new(case.must_survive()),
        violations: Mutex::new(Vec::new()),
        ends: Mutex::new(vec![(0, mem.snapshot())]),
    });
    let slow = case.vault == VaultState::Slow;
    let mut problems = Vec::new();
    let mut settings = case.settings();
    let mut calls = [0, 0, 0];
    if case.fate == Fate::RemoveFails {
        refuse_remove(&file);
    }
    for l in 1..=3 {
        if l == 2 {
            match case.clock {
                Clock::Missing => settings.vault_copy_since = None,
                Clock::Future => settings.vault_copy_since = Some(case.time(2) + 100 * HOUR),
                Clock::Later | Clock::Soon | Clock::Late | Clock::Ahead => {}
            }
        }
        if case.fate == Fate::CrashAfterZeros {
            if l == 2 { refuse_remove(&file) } else { allow_remove(&file) }
        }
        // Verrou tenu par une autre instance au premier lancement seulement.
        let other = (case.lock_held && l == 1).then(|| InstanceLock::new(Some(dir.join("comptes.lock")), Duration::from_secs(5)).acquire());
        let vault = Arc::new(TableVault::new(&world, slow, ev, l, case.real(l)));
        settings = launch(case, l, &world, vault.clone(), ev, settings, &mut problems);
        calls[l - 1] = vault.calls.load(Ordering::SeqCst);
        drop(other);
        if let Ok(mut e) = world.ends.lock() {
            e.push((case.real(l), mem.snapshot()));
        }
    }
    allow_remove(&file);
    for t in world.must() {
        if !readable(&file, &mem, &t) {
            problems.push("jeton perdu à la fin".into());
        }
    }
    if case.fate == Fate::CrashAfterZeros && zeroed(&file) {
        problems.push("fichier vidé (arrêt entre les zéros et la suppression) pas supprimé au lancement suivant".into());
    }
    problems.extend(world.violations.lock().map(|v| v.clone()).unwrap_or_default());
    problems.dedup();
    let _ = fs::remove_dir_all(&dir);
    (problems, calls)
}

/// Toutes les combinaisons de départ.
fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for file in FILES {
        for vault in VAULTS {
            for lock_held in [false, true] {
                for accepts in [true, false] {
                    cases.push(Case::new(file, vault, lock_held, accepts, Accounts::Alone));
                }
            }
        }
    }
    for accounts in [Accounts::OtherLast, Accounts::Twin] {
        for file in [FileState::Absent, FileState::Valid] {
            for vault in [VaultState::Absent, VaultState::Same, VaultState::Other, VaultState::Unavailable, VaultState::Slow, VaultState::ReadBackDifferent] {
                for accepts in [true, false] {
                    cases.push(Case::new(file, vault, false, accepts, accounts));
                }
            }
        }
    }
    for clock in [Clock::Soon, Clock::Missing, Clock::Future, Clock::Late, Clock::Ahead] {
        for vault in [VaultState::Absent, VaultState::Same] {
            for accepts in [true, false] {
                cases.push(Case { clock, ..Case::new(FileState::Valid, vault, false, accepts, Accounts::Alone) });
            }
        }
    }
    // Discord refuse le jeton du coffre, accepte celui du fichier (« Essayer le jeton du fichier »).
    for accounts in [Accounts::Known, Accounts::Twin] {
        cases.push(Case { refuses_vault: true, ..Case::new(FileState::Valid, VaultState::Other, false, true, accounts) });
    }
    for fate in [Fate::RemoveFails, Fate::CrashAfterZeros] {
        for vault in [VaultState::Absent, VaultState::Same] {
            cases.push(Case { fate, ..Case::new(FileState::Valid, vault, false, true, Accounts::Alone) });
        }
    }
    cases
}

/// Événements d'un cas, d'après le nombre d'appels d'un parcours sans événement.
fn events(case: &Case, calls: [usize; 3]) -> Vec<Happens> {
    let mut v = Vec::new();
    for call in 0..calls[0] {
        v.push(Happens { launch: 1, call, action: Action::Stop });
    }
    if case.lock_held {
        return v;
    }
    // Horloge fausse : panne aussi au 3e lancement (l'étape 2 ne doit pas y avoir lieu trop tôt).
    let power = if matches!(case.clock, Clock::Late | Clock::Ahead) { 3 } else { 2 };
    for launch in 1..=power {
        // Jusqu'à `calls` compris : panne après le dernier appel.
        for call in 0..=calls[launch - 1] {
            v.push(Happens { launch, call, action: Action::Power });
        }
    }
    for call in 0..calls[1] {
        v.push(Happens { launch: 2, call, action: Action::Stop });
    }
    let acts = case.file == FileState::Valid && matches!(case.vault, VaultState::Absent | VaultState::Same | VaultState::Other);
    if acts {
        let extra: &[Action] = if case.refuses_vault { &TRY_FILE } else { &[] };
        for &action in USER_ACTIONS.iter().chain(extra) {
            for launch in [1, 2] {
                for call in 0..calls[launch - 1] {
                    v.push(Happens { launch, call, action });
                }
            }
        }
    }
    v
}

/// Parcourt `cases` (tous leurs événements) ; rend (parcours, problèmes).
fn run_table(cases: &[Case], keep: impl Fn(&Happens) -> bool + Sync, old: Option<super::accounts::Old>) -> (usize, Vec<String>) {
    let next = AtomicUsize::new(0);
    let walks = AtomicUsize::new(0);
    let failures: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(4, 8);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                super::accounts::OLD.with(|c| c.set(old));
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(case) = cases.get(i) else { return };
                    let id = i * 10_000;
                    let (problems, calls) = walk(case, None, id);
                    walks.fetch_add(1, Ordering::SeqCst);
                    let mut found: Vec<String> = problems.into_iter().map(|p| format!("{} : {p}", case.name(None))).collect();
                    for (j, h) in events(case, calls).into_iter().filter(|h| keep(h)).enumerate() {
                        let (problems, _) = walk(case, Some(h), id + j + 1);
                        walks.fetch_add(1, Ordering::SeqCst);
                        found.extend(problems.into_iter().map(|p| format!("{} : {p}", case.name(Some(h)))));
                    }
                    if !found.is_empty()
                        && let Ok(mut f) = failures.lock()
                    {
                        f.extend(found);
                    }
                }
            });
        }
    });
    (walks.load(Ordering::SeqCst), failures.into_inner().unwrap_or_default())
}

#[test]
fn every_state_keeps_the_token_somewhere() {
    let cases = cases();
    assert_eq!(cases.len(), 6 * 8 * 2 * 2 + 2 * 2 * 6 * 2 + 5 * 2 * 2 + 2 + 2 * 2);
    let (total, failures) = run_table(&cases, |_| true, None);
    println!("table d'états : {} combinaisons de départ, {total} parcours de trois lancements (événements compris)", cases.len());
    assert!(failures.is_empty(), "{} problème(s) sur {total} parcours :\n{}", failures.len(), failures.join("\n"));
}

/// Cas où un contrôle a vu une fois un faux « jeton illisible » sous forte
/// charge (l'autre instance libérait puis retirait l'entrée entre la copie
/// de la liste et sa lecture) : rejoué plusieurs fois, jamais de problème.
#[test]
fn other_remove_during_emptied_step_two_is_replayed() {
    let case = Case { fate: Fate::RemoveFails, ..Case::new(FileState::Valid, VaultState::Absent, false, true, Accounts::Alone) };
    let h = Happens { launch: 2, call: 2, action: Action::OtherRemove };
    for n in 0..20 {
        let (problems, _) = walk(&case, Some(h), 900_000 + n);
        assert!(problems.is_empty(), "{} (essai {n}) : {problems:?}", case.name(Some(h)));
    }
}

/// Les anciens comportements remis un à un : la table les prend en défaut.
/// (`Old::RemoveDuringStepTwo` n'y figure plus : depuis que la question de
/// retrait relit coffre et fichier et que la confirmation est vérifiée sous
/// verrou, ce retrait-là ne perd plus rien ; le refus pendant l'étape 2 est
/// vérifié par les tests de bout en bout.)
#[test]
fn table_catches_the_old_behaviours() {
    use super::accounts::Old;
    let valid = |accounts, clock| {
        let mut v = Vec::new();
        for vault in [VaultState::Absent, VaultState::Same] {
            v.push(Case { clock, ..Case::new(FileState::Valid, vault, false, true, accounts) });
        }
        v
    };
    for (old, cases, keep) in [
        (Old::StepTwoAfterWrite, valid(Accounts::Alone, Clock::Later), (|h: &Happens| h.action == Action::Power) as fn(&Happens) -> bool),
        (Old::TwinCleanup, valid(Accounts::Twin, Clock::Later), |_: &Happens| false),
        (Old::NoDelay, valid(Accounts::Alone, Clock::Soon), |h: &Happens| h.action == Action::Power),
        (Old::Unchecked, valid(Accounts::Alone, Clock::Later), |h: &Happens| h.action == Action::RemoveRaced),
        (Old::SilentOverwrite, valid(Accounts::Alone, Clock::Later), |h: &Happens| h.action == Action::TypeRenewed(Answer::None)),
        (Old::FileOverwrite, vec![Case { refuses_vault: true, ..Case::new(FileState::Valid, VaultState::Other, false, true, Accounts::Known) }], |h: &Happens| h.action == Action::TryFile(Answer::None)),
    ] {
        let (total, failures) = run_table(&cases, keep, Some(old));
        println!("ancien comportement {old:?} : {} problème(s) sur {total} parcours ; premier : {:?}", failures.len(), failures.first());
        assert!(!failures.is_empty(), "{old:?} n'est pas pris en défaut ({total} parcours)");
    }
}

