//! Fil des comptes : tous les appels au coffre et la migration de l'ancien
//! fichier se font ici, l'un après l'autre (règle 1.3.1). L'interface n'envoie
//! que des demandes et lit des réponses, sans jamais attendre.
//!
//! Les appels au coffre passent par un seul fil permanent, avec un délai
//! maximal ([`TimedVault`]) ; un délai dépassé rend le coffre « indéterminé »
//! pour la session (plus aucun retrait ni retour arrière) et garde le verrou
//! entre instances ([`InstanceLock`]) tant que l'appel abandonné n'est pas
//! fini. Une écriture qui échoue remet la valeur d'avant ; si cette remise
//! échoue, la réponse le dit avec le nom exact de l'entrée.

use super::{
    lock::InstanceLock,
    migrate::{self, Checked, FileMark, FileState, Finish, Survey, WipeCheck, WipeFailure},
    vault::{get_confirmed, TimedVault, VAULT_TIMEOUT},
    AccountKey, Migration, Token, Vault, VaultError,
};
use crate::i18n::{
    language,
    verbatim::{f as vf, t as vt},
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::Duration,
};

type Wake = Arc<dyn Fn() + Send + Sync>;

/// Attente maximale du fil des comptes à la fermeture : au-delà, il est
/// abandonné (le processus se termine sans lui).
pub const SHUTDOWN_WAIT: Duration = Duration::from_secs(3);

enum Request {
    Startup { legacy: Option<PathBuf>, open: Option<AccountKey>, probe_imported: bool, room: bool, declined: Option<FileMark> },
    /// Étape 1 de la migration, à la demande (« Importer » après un refus).
    Import { path: PathBuf, room: bool },
    /// Le coffre a-t-il une copie du jeton du fichier (sous l'une de ces clés) ?
    CheckWipe { path: PathBuf, keys: Vec<AccountKey> },
    /// Taille et date du fichier (import refusé).
    MarkLegacy(PathBuf),
    Load(AccountKey),
    /// Lesquelles de ces entrées existent dans le coffre (sans rendre leur jeton) ?
    Probe(Vec<AccountKey>),
    Store { seq: u64, key: AccountKey, token: Token, expect: Expect },
    /// Annule l'écriture `seq` (si elle a réussi) : remet la valeur d'avant.
    Undo(u64),
    /// L'écriture `seq` est acquise : on oublie de quoi l'annuler.
    Settle(u64),
    /// Relit coffre et fichier pour la question de retrait de `key`.
    Survey { key: AccountKey, path: Option<PathBuf>, keys: Vec<AccountKey> },
    /// Retrait confirmé sur l'état `expected` (`None` : sans question, tests seulement).
    Forget { key: AccountKey, path: Option<PathBuf>, keys: Vec<AccountKey>, expected: Option<Survey> },
    Finish { path: PathBuf, token: Token },
    /// Suppression confirmée sur l'état `expected`.
    WipeLegacy { path: PathBuf, keys: Vec<AccountKey>, expected: Survey },
    Shutdown,
}

/// Réglages du fil des comptes.
#[derive(Clone, Debug)]
pub struct LinkOptions {
    /// Verrou entre instances (aucun par défaut : tests, aperçu).
    pub lock: InstanceLock,
    /// Délai maximal d'un appel au coffre.
    pub vault_timeout: Duration,
}

impl Default for LinkOptions {
    fn default() -> Self {
        Self { lock: InstanceLock::none(), vault_timeout: VAULT_TIMEOUT }
    }
}

/// Résultat du démarrage.
#[derive(Debug)]
pub struct Started {
    pub migration: Migration,
    /// Le coffre a une entrée `importe` que la liste des comptes ne
    /// connaissait pas (migration faite, mais liste pas encore enregistrée).
    pub imported_found: bool,
    /// Jeton du compte à rouvrir, s'il y en avait un.
    pub opened: Option<(AccountKey, Result<Option<Token>, VaultError>)>,
}

/// Réponse du fil des comptes.
#[derive(Debug)]
pub enum Reply {
    Started(Started),
    Loaded { key: AccountKey, result: Result<Option<Token>, VaultError> },
    /// Réponse à [`AccountLink::probe`] : les entrées trouvées (un coffre
    /// illisible n'en trouve aucune).
    Probed(Vec<AccountKey>),
    /// Écriture `seq` vérifiée par relecture. En cas d'échec, la valeur
    /// d'avant est remise ; `put_back` dit, avec le nom de l'entrée, si
    /// cette remise a échoué ou n'a pas pu être tentée.
    /// `recover` : la valeur d'avant, gardée en mémoire quand elle n'a pas pu
    /// être remise (l'utilisateur peut la recopier).
    /// `left` : ce que contient l'entrée après un échec dont la valeur d'avant n'a pas pu être remise.
    Stored { seq: u64, key: AccountKey, result: Result<(), VaultError>, put_back: Option<String>, recover: Option<Token>, left: Option<Token> },
    /// Écriture `seq` annulée (valeur d'avant remise) ; l'erreur nomme l'entrée.
    Restored { seq: u64, key: AccountKey, result: Result<(), String> },
    /// `unknown` : le coffre n'a pas répondu à temps, le retrait a peut-être eu lieu.
    Forgotten { key: AccountKey, result: Result<(), String>, unknown: bool },
    /// Écriture `seq` non faite : l'entrée contient un autre jeton que celui
    /// attendu (`existing`, `None` : absente). Rien n'est écrit sans « oui ».
    Occupied { seq: u64, key: AccountKey, existing: Option<Token> },
    /// État relu pour la question de retrait de `key`.
    Surveyed { key: AccountKey, survey: Survey },
    /// Action confirmée non faite : l'état a changé depuis la question.
    Stale { act: Act, survey: Survey },
    /// Étape 2 de la migration (après READY).
    Finished(Finish),
    /// Étape 1 demandée par « Importer ».
    Imported(Migration),
    /// Réponse à [`AccountLink::check_wipe`].
    WipeChecked(WipeCheck, Survey),
    /// Réponse à [`AccountLink::mark_legacy`].
    LegacyMarked(Option<FileMark>),
    /// Ancien fichier de jeton écrasé puis supprimé (à la demande de l'utilisateur).
    LegacyWiped(Result<(), WipeFailure>),
}

/// Ce qu'une écriture accepte de trouver dans l'entrée avant d'écrire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expect {
    /// Entrée absente (ou déjà ce jeton) : un autre jeton n'est jamais remplacé.
    Vacant,
    /// Remplacement confirmé : l'entrée contient encore exactement ceci
    /// (`None` : absente), relu verrou tenu ; sinon rien n'est écrit.
    Holds(Option<Token>),
    /// « Recopier le jeton d'avant » : entrée absente, ou qui contient encore
    /// ce que l'écriture ratée y a laissé (`None` : inconnu, alors absente seulement).
    Ours(Option<Token>),
    /// Quoi qu'il y ait : jamais hors des tests (anciens comportements remis).
    Anything,
}

/// Action confirmée dont l'état a changé.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    Remove(AccountKey),
    Wipe,
}

/// Lien entre l'interface et le fil des comptes ; aucune méthode ne bloque.
pub struct AccountLink {
    tx: Sender<Request>,
    rx: Receiver<Reply>,
    wake: Arc<Mutex<Option<Wake>>>,
    thread: Option<JoinHandle<()>>,
    /// Reçoit un message quand le fil a fini (fermeture bornée).
    done: Receiver<()>,
    vault_name: Option<&'static str>,
    next_seq: u64,
}

impl AccountLink {
    /// Sans verrou entre instances (tests, aperçu).
    pub fn start(vault: Arc<dyn Vault>) -> Self {
        Self::start_with(vault, LinkOptions::default())
    }

    pub fn start_with(vault: Arc<dyn Vault>, options: LinkOptions) -> Self {
        let (tx, req_rx) = mpsc::channel();
        let (out, rx) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        let wake: Arc<Mutex<Option<Wake>>> = Arc::default();
        let vault = TimedVault::new(vault, options.vault_timeout);
        let vault_name = vault.name();
        let w = wake.clone();
        // Verrou gardé tant qu'un appel abandonné au coffre tourne encore.
        let lock = options.lock.pinned_while(vault.busy_probe());
        let thread = std::thread::Builder::new()
            .name("comptes".into())
            .spawn(move || {
                run(&vault, &lock, &req_rx, &out, &w);
                let _ = done_tx.send(());
            })
            .ok();
        Self { tx, rx, wake, thread, done, vault_name, next_seq: 0 }
    }

    pub fn set_wake(&self, f: impl Fn() + Send + Sync + 'static) {
        if let Ok(mut w) = self.wake.lock() {
            *w = Some(Arc::new(f));
        }
    }

    /// Nom du coffre (écran de connexion) ; `None` : les jetons ne sont pas retenus.
    pub fn vault_name(&self) -> Option<&'static str> {
        self.vault_name
    }

    /// Étape 1 de la migration de `legacy` (`room` : la liste peut recevoir
    /// le compte importé), recherche d'une entrée `importe` oubliée par la
    /// liste (`probe_imported`), puis lecture du jeton de `open`.
    pub fn startup(&self, legacy: Option<PathBuf>, open: Option<AccountKey>, probe_imported: bool, room: bool, declined: Option<FileMark>) {
        let _ = self.tx.send(Request::Startup { legacy, open, probe_imported, room, declined });
    }

    pub fn load(&self, key: AccountKey) {
        let _ = self.tx.send(Request::Load(key));
    }

    /// Cherche ces entrées dans le coffre (le Gestionnaire d'identifiants
    /// n'est pas parcouru : seules les clés données sont lues).
    pub fn probe(&self, keys: Vec<AccountKey>) {
        let _ = self.tx.send(Request::Probe(keys));
    }

    /// Écrit un jeton si l'entrée contient ce qu'`expect` accepte (sinon
    /// réponse [`Reply::Occupied`], rien d'écrit) ; rend le numéro de
    /// l'écriture (repris par la réponse).
    pub fn store(&mut self, key: AccountKey, token: Token, expect: Expect) -> u64 {
        self.next_seq += 1;
        let seq = self.next_seq;
        let _ = self.tx.send(Request::Store { seq, key, token, expect });
        seq
    }

    /// Annule l'écriture `seq` dès qu'elle est finie : remet la valeur
    /// d'avant (ou retire l'entrée), seulement si le coffre contient
    /// toujours ce qui a été écrit. Envoyé tout de suite, la demande part
    /// même si l'application se ferme avant la réponse.
    pub fn undo(&self, seq: u64) {
        let _ = self.tx.send(Request::Undo(seq));
    }

    /// L'écriture `seq` ne sera pas annulée.
    pub fn settle(&self, seq: u64) {
        let _ = self.tx.send(Request::Settle(seq));
    }

    /// Relit coffre et fichier pour la question de retrait de `key` (`keys` :
    /// les entrées où une copie peut rester).
    pub fn survey(&self, key: AccountKey, path: Option<PathBuf>, keys: Vec<AccountKey>) {
        let _ = self.tx.send(Request::Survey { key, path, keys });
    }

    /// Retrait confirmé sur l'état `expected` : fait seulement s'il n'a pas changé.
    pub fn forget(&self, key: AccountKey, path: Option<PathBuf>, keys: Vec<AccountKey>, expected: Option<Survey>) {
        let _ = self.tx.send(Request::Forget { key, path, keys, expected });
    }

    /// Étape 2 de la migration, après READY obtenu avec `token`.
    pub fn finish_migration(&self, path: PathBuf, token: Token) {
        let _ = self.tx.send(Request::Finish { path, token });
    }

    /// Écrase puis supprime l'ancien fichier de jeton (action de l'utilisateur, confirmée).
    /// Étape 1 de la migration à la demande (« Importer » après un refus).
    pub fn import(&self, path: PathBuf, room: bool) {
        let _ = self.tx.send(Request::Import { path, room });
    }

    /// Le coffre a-t-il une copie du jeton du fichier, sous l'une de ces clés ?
    pub fn check_wipe(&self, path: PathBuf, keys: Vec<AccountKey>) {
        let _ = self.tx.send(Request::CheckWipe { path, keys });
    }

    /// Taille et date du fichier, pour se souvenir d'un import refusé.
    pub fn mark_legacy(&self, path: PathBuf) {
        let _ = self.tx.send(Request::MarkLegacy(path));
    }

    /// Suppression confirmée sur l'état `expected` : faite seulement s'il n'a pas changé.
    pub fn wipe_legacy(&self, path: PathBuf, keys: Vec<AccountKey>, expected: Survey) {
        let _ = self.tx.send(Request::WipeLegacy { path, keys, expected });
    }

    pub fn poll(&mut self) -> Vec<Reply> {
        self.rx.try_iter().collect()
    }
}

impl Drop for AccountLink {
    /// Termine les demandes déjà envoyées (dont les annulations), au plus
    /// [`SHUTDOWN_WAIT`] : un coffre qui ne répond pas ne retient pas la
    /// fermeture (le fil est alors abandonné, sans rien couper en cours de
    /// fichier : l'ancien fichier n'est jamais supprimé avant que le coffre
    /// ait rendu son jeton).
    fn drop(&mut self) {
        let _ = self.tx.send(Request::Shutdown);
        if let Some(t) = self.thread.take()
            && self.done.recv_timeout(SHUTDOWN_WAIT).is_ok()
        {
            let _ = t.join();
        }
    }
}

/// Écriture réussie, gardée de quoi l'annuler : compte, jeton écrit, valeur d'avant.
type Written = (AccountKey, Token, Option<Token>);

fn run(vault: &TimedVault, lock: &InstanceLock, rx: &Receiver<Request>, out: &Sender<Reply>, wake: &Mutex<Option<Wake>>) {
    let send = |r: Reply| {
        if out.send(r).is_ok()
            && let Ok(w) = wake.lock()
            && let Some(w) = w.as_ref()
        {
            w();
        }
    };
    let mut written: HashMap<u64, Written> = HashMap::new();
    while let Ok(req) = rx.recv() {
        match req {
            Request::Startup { legacy, open, probe_imported, room, declined } => {
                let migration = legacy.as_deref().map_or(Migration::NoFile, |p| migrate::migrate(p, vault, lock, room, declined));
                let imported_found = !migration.stored() && probe_imported && matches!(vault.get(&AccountKey::imported()), Ok(Some(_)));
                let opened = open.map(|key| {
                    let result = vault.get(&key);
                    (key, result)
                });
                send(Reply::Started(Started { migration, imported_found, opened }));
            }
            Request::Load(key) => {
                let result = vault.get(&key);
                send(Reply::Loaded { key, result });
            }
            Request::Probe(keys) => {
                let found = keys.into_iter().filter(|k| matches!(vault.get(k), Ok(Some(_)))).collect();
                send(Reply::Probed(found));
            }
            Request::Store { seq, key, token, expect } => match store(vault, lock, &key, &token, &expect) {
                Err(existing) => send(Reply::Occupied { seq, key, existing }),
                Ok((result, previous, put_back, recover, left)) => {
                    if result.is_ok() {
                        written.insert(seq, (key.clone(), token, previous));
                    }
                    send(Reply::Stored { seq, key, result, put_back, recover, left });
                }
            },
            Request::Undo(seq) => {
                // Écriture échouée (déjà remise) : rien à annuler.
                if let Some((key, token, previous)) = written.remove(&seq) {
                    let result = restore(vault, lock, &key, &token, previous.as_ref()).map_err(|e| {
                        vf::entry_may_hold(language(), vault.entry_name(&key), e)
                    });
                    send(Reply::Restored { seq, key, result });
                }
            }
            Request::Settle(seq) => {
                written.remove(&seq);
            }
            Request::Survey { key, path, keys } => send(Reply::Surveyed { key, survey: migrate::survey(path.as_deref(), vault, &keys) }),
            Request::Forget { key, path, keys, expected } => match expected {
                Some(expected) => match migrate::forget_checked(path.as_deref(), vault, lock, &key, &keys, &expected) {
                    Checked::Done(result) => {
                        let unknown = result.is_err() && vault.indeterminate();
                        send(Reply::Forgotten { key, result, unknown });
                    }
                    Checked::Stale(survey) => send(Reply::Stale { act: Act::Remove(key), survey }),
                },
                None => {
                    let result = lock.acquire().and_then(|_held| vault.delete(&key).map_err(|e| e.to_string()));
                    let unknown = result.is_err() && vault.indeterminate();
                    send(Reply::Forgotten { key, result, unknown });
                }
            },
            Request::Finish { path, token } => send(Reply::Finished(migrate::finish(&path, vault, lock, &token))),
            Request::Import { path, room } => send(Reply::Imported(migrate::migrate(&path, vault, lock, room, None))),
            Request::CheckWipe { path, keys } => {
                let (check, survey) = migrate::check_wipe(&path, vault, &keys);
                send(Reply::WipeChecked(check, survey));
            }
            // Retenu seulement si le fichier contient un jeton (ou un contenu inconnu).
            Request::MarkLegacy(path) => {
                let mark = match migrate::survey(Some(&path), vault, &[]).file {
                    FileState::Token(_) | FileState::Unknown(_) => migrate::file_mark(&path),
                    FileState::Absent | FileState::Empty => None,
                };
                send(Reply::LegacyMarked(mark));
            }
            Request::WipeLegacy { path, keys, expected } => match migrate::wipe_checked(&path, vault, lock, &keys, &expected) {
                Checked::Done(result) => send(Reply::LegacyWiped(result)),
                Checked::Stale(survey) => send(Reply::Stale { act: Act::Wipe, survey }),
            },
            Request::Shutdown => return,
        }
    }
}

/// Ce que rend une écriture : résultat, valeur d'avant, message si la
/// valeur d'avant n'a pas pu être remise, et cette valeur gardée en
/// mémoire pour que l'utilisateur puisse la recopier.
type StoreOutcome = (Result<(), VaultError>, Option<Token>, Option<String>, Option<Token>, Option<Token>);

/// Écrit puis relit : une écriture n'est réussie que si le coffre rend le
/// même jeton (une absence est relue une deuxième fois). En cas d'échec, la
/// valeur d'avant est remise (verrou tenu : ce qui est dans le coffre à ce
/// moment-là vient de nous), sauf si le coffre n'a pas répondu à temps. Si
/// elle n'a pas pu être remise, elle n'est pas perdue pour autant : elle
///
/// Avant d'écrire, verrou tenu, l'entrée est relue : si elle contient un
/// autre jeton que ce qu'`expect` accepte, rien n'est écrit (`Err` : ce
/// qu'elle contient). Déjà ce jeton : rien à écrire. Coffre illisible :
/// rien n'est écrit, l'erreur est rendue.
/// revient à l'interface (`recover`), qui propose de la recopier.
fn store(vault: &dyn Vault, lock: &InstanceLock, key: &AccountKey, token: &Token, expect: &Expect) -> Result<StoreOutcome, Option<Token>> {
    let _held = match lock.acquire() {
        Ok(h) => h,
        Err(why) => return Ok((Err(VaultError::Failed(why)), None, None, None, None)),
    };
    // « Absent » mène à retirer l'entrée en cas d'échec : vérifié deux fois.
    let previous = match get_confirmed(vault, key) {
        Ok(p) => p,
        Err(e) => return Ok((Err(e), None, None, None, None)),
    };
    if previous.as_ref() == Some(token) {
        return Ok((Ok(()), previous, None, None, None));
    }
    let accepted = match expect {
        Expect::Vacant => previous.is_none(),
        Expect::Holds(t) => &previous == t,
        Expect::Ours(left) => previous.is_none() || (left.is_some() && &previous == left),
        Expect::Anything => true,
    };
    if !accepted {
        return Err(previous);
    }
    let result = vault.set(key, token).and_then(|()| match get_confirmed(vault, key) {
        Ok(Some(back)) if &back == token => Ok(()),
        Err(e) => Err(e),
        _ => Err(VaultError::Failed(vt::VAULT_DID_NOT_RETURN.get(language()).into())),
    });
    let (mut message, mut recover, mut left) = (None, None, None);
    if result.is_err() {
        let entry = vault.entry_name(key);
        if vault.indeterminate() {
            message = Some(vf::write_timeout(language(), &entry));
            recover = previous.clone();
        } else if let Err(e) = put_back(vault, key, previous.as_ref()) {
            message = Some(match previous {
                Some(_) => vf::put_back_failed(language(), &e, &entry),
                None => vf::new_entry_not_removed(language(), &entry, &e),
            });
            recover = previous.clone();
            // Ce que l'écriture ratée laisse dans l'entrée : « Recopier » ne réécrit que par-dessus ceci.
            left = vault.get(key).ok().flatten();
        }
    }
    Ok((result, previous, message, recover, left))
}

/// Remet `previous`, ou retire l'entrée s'il n'y avait rien ; vérifié par relecture.
fn put_back(vault: &dyn Vault, key: &AccountKey, previous: Option<&Token>) -> Result<(), VaultError> {
    match previous {
        Some(p) => vault.set(key, p)?,
        None => vault.delete(key)?,
    }
    match vault.get(key)? {
        now if now.as_ref() == previous => Ok(()),
        _ => Err(VaultError::Failed(vt::REREAD_MISMATCH.get(language()).into())),
    }
}

/// Annulation : seulement si le coffre contient toujours ce qu'on a écrit
/// (sinon, quelqu'un d'autre l'a changé depuis : on n'y touche pas).
fn restore(vault: &dyn Vault, lock: &InstanceLock, key: &AccountKey, written: &Token, previous: Option<&Token>) -> Result<(), VaultError> {
    let _held = lock.acquire().map_err(VaultError::Failed)?;
    match vault.get(key)? {
        Some(now) if &now == written => put_back(vault, key, previous),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::vault::{Faults, MemoryVault};
    use super::*;
    use std::{
        fs,
        time::{Duration, Instant},
    };

    fn next(link: &mut AccountLink) -> Reply {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(r) = link.poll().into_iter().next() {
                return r;
            }
            assert!(Instant::now() < end, "pas de réponse du fil des comptes");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn store_verifies_by_reading_back() {
        let v = Arc::new(MemoryVault::new());
        let mut link = AccountLink::start(v.clone());
        let k = AccountKey::of_user(7);
        link.store(k.clone(), Token::from("a"), Expect::Anything);
        assert!(matches!(next(&mut link), Reply::Stored { result: Ok(()), put_back: None, .. }));
        assert_eq!(v.peek(&k), Some(Token::from("a")));

        v.set_faults(Faults { corrupt_set: true, ..Faults::default() });
        link.store(AccountKey::of_user(8), Token::from("b"), Expect::Anything);
        assert!(matches!(next(&mut link), Reply::Stored { result: Err(_), put_back: None, .. }));
        assert_eq!(v.peek(&AccountKey::of_user(8)), None, "écriture douteuse retirée (rien avant)");

        v.set_faults(Faults::default());
        link.survey(k.clone(), None, vec![k.clone()]);
        let Reply::Surveyed { survey, .. } = next(&mut link) else { panic!("relecture attendue") };
        link.forget(k.clone(), None, vec![k.clone()], Some(survey));
        assert!(matches!(next(&mut link), Reply::Forgotten { result: Ok(()), .. }));
        link.load(k);
        assert!(matches!(next(&mut link), Reply::Loaded { result: Ok(None), .. }));
    }

    /// Écriture refusée à la relecture, puis retrait refusé : la réponse le
    /// dit, avec le nom exact de l'entrée (et sans le jeton).
    #[test]
    fn failed_put_back_names_the_entry() {
        let v = Arc::new(MemoryVault::new());
        v.set_faults(Faults { corrupt_set: true, fail_delete: true, ..Faults::default() });
        let mut link = AccountLink::start(v.clone());
        link.store(AccountKey::of_user(8), Token::from("secret-de-test"), Expect::Anything);
        match next(&mut link) {
            Reply::Stored { result: Err(_), put_back: Some(m), .. } => {
                assert!(m.contains("« compte 8 »") && m.contains("n'a pas pu être retirée"), "{m}");
                assert!(!m.contains("secret-de-test"), "{m}");
            }
            other => panic!("{other:?}"),
        }
        // Valeur d'avant impossible à remettre.
        let k = AccountKey::of_user(9);
        let v = Arc::new(MemoryVault::with([(k.clone(), Token::from("ancien"))]));
        v.set_faults(Faults { corrupt_set: true, ..Faults::default() });
        let mut link = AccountLink::start(v.clone());
        link.store(k, Token::from("nouveau"), Expect::Anything);
        assert!(matches!(next(&mut link), Reply::Stored { result: Err(_), put_back: Some(m), recover: Some(p), .. } if p == Token::from("ancien") && m.contains("Recopier le jeton d'avant") && m.contains("« compte 9 »")));
    }

    /// Une entrée qui contient un autre jeton n'est jamais remplacée sans
    /// confirmation : rien n'est écrit, la réponse dit ce qu'elle contient ;
    /// le remplacement confirmé n'a lieu que si elle contient encore ce qui
    /// a été annoncé.
    #[test]
    fn different_token_is_never_replaced_without_confirmation() {
        let k = AccountKey::of_user(5);
        let v = Arc::new(MemoryVault::with([(k.clone(), Token::from("enregistre"))]));
        let mut link = AccountLink::start(v.clone());
        let seq = link.store(k.clone(), Token::from("saisi"), Expect::Vacant);
        assert!(matches!(next(&mut link), Reply::Occupied { seq: s, existing: Some(t), .. } if s == seq && t == Token::from("enregistre")));
        assert_eq!(v.peek(&k), Some(Token::from("enregistre")), "rien n'est écrit");
        // Même jeton : rien à écrire, réussite.
        link.store(k.clone(), Token::from("enregistre"), Expect::Vacant);
        assert!(matches!(next(&mut link), Reply::Stored { result: Ok(()), .. }));
        // Confirmé, mais l'autre instance a changé l'entrée entre-temps : rien n'est écrit.
        v.set(&k, &Token::from("ailleurs")).unwrap_or_else(|e| panic!("{e}"));
        link.store(k.clone(), Token::from("saisi"), Expect::Holds(Some(Token::from("enregistre"))));
        assert!(matches!(next(&mut link), Reply::Occupied { existing: Some(t), .. } if t == Token::from("ailleurs")));
        assert_eq!(v.peek(&k), Some(Token::from("ailleurs")));
        // Confirmé sur l'état actuel : écrit.
        link.store(k.clone(), Token::from("saisi"), Expect::Holds(Some(Token::from("ailleurs"))));
        assert!(matches!(next(&mut link), Reply::Stored { result: Ok(()), .. }));
        assert_eq!(v.peek(&k), Some(Token::from("saisi")));
        // Coffre illisible : rien n'est écrit, l'erreur est rendue.
        v.set_faults(Faults { fail_get: true, ..Faults::default() });
        link.store(AccountKey::of_user(6), Token::from("x"), Expect::Vacant);
        assert!(matches!(next(&mut link), Reply::Stored { result: Err(_), .. }));
        v.set_faults(Faults::default());
        assert_eq!(v.peek(&AccountKey::of_user(6)), None);
        // « Recopier » : seulement par-dessus ce que l'échec a laissé, ou une entrée vide.
        link.store(k.clone(), Token::from("avant"), Expect::Ours(Some(Token::from("laisse"))));
        assert!(matches!(next(&mut link), Reply::Occupied { existing: Some(t), .. } if t == Token::from("saisi")));
        link.store(k.clone(), Token::from("avant"), Expect::Ours(None));
        assert!(matches!(next(&mut link), Reply::Occupied { .. }), "contenu laissé inconnu : entrée vide seulement");
        assert_eq!(v.peek(&k), Some(Token::from("saisi")));
        link.store(k.clone(), Token::from("avant"), Expect::Ours(Some(Token::from("saisi"))));
        assert!(matches!(next(&mut link), Reply::Stored { result: Ok(()), .. }));
        assert_eq!(v.peek(&k), Some(Token::from("avant")));
        link.store(AccountKey::of_user(7), Token::from("avant"), Expect::Ours(None));
        assert!(matches!(next(&mut link), Reply::Stored { result: Ok(()), .. }));
    }

    /// Un nouveau jeton refusé à la relecture ne détruit pas l'ancien : il est remis.
    #[test]
    fn failed_store_puts_previous_value_back() {
        /// Abîme seulement le jeton « nouveau » (la remise de l'ancien passe).
        struct DamagesNew(Arc<MemoryVault>);
        impl Vault for DamagesNew {
            fn get(&self, k: &AccountKey) -> Result<Option<Token>, VaultError> {
                self.0.get(k)
            }
            fn set(&self, k: &AccountKey, t: &Token) -> Result<(), VaultError> {
                let t = if t.expose() == "nouveau" { Token::from("abime") } else { t.clone() };
                self.0.set(k, &t)
            }
            fn delete(&self, k: &AccountKey) -> Result<(), VaultError> {
                self.0.delete(k)
            }
            fn name(&self) -> Option<&'static str> {
                None
            }
        }
        let k = AccountKey::of_user(7);
        let v = Arc::new(MemoryVault::with([(k.clone(), Token::from("ancien"))]));
        let mut link = AccountLink::start(Arc::new(DamagesNew(v.clone())));
        link.store(k.clone(), Token::from("nouveau"), Expect::Anything);
        let r = next(&mut link);
        assert!(matches!(&r, Reply::Stored { result: Err(_), put_back: None, .. }), "{r:?}");
        assert_eq!(v.peek(&k), Some(Token::from("ancien")), "valeur d'avant remise, pas effacée");
        drop(link);

        let mut link = AccountLink::start(v.clone());
        let seq = link.store(k.clone(), Token::from("nouveau"), Expect::Anything);
        assert!(matches!(next(&mut link), Reply::Stored { seq: s, result: Ok(()), .. } if s == seq));
        // Annulation : la valeur d'avant revient.
        link.undo(seq);
        assert!(matches!(next(&mut link), Reply::Restored { result: Ok(()), .. }));
        assert_eq!(v.peek(&k), Some(Token::from("ancien")));
        // Annulation d'une écriture remplacée entre-temps : rien n'est touché.
        let seq = link.store(k.clone(), Token::from("nouveau"), Expect::Anything);
        assert!(matches!(next(&mut link), Reply::Stored { result: Ok(()), .. }));
        assert_eq!(v.set(&k, &Token::from("autre")), Ok(()));
        link.undo(seq);
        assert!(matches!(next(&mut link), Reply::Restored { result: Ok(()), .. }));
        assert_eq!(v.peek(&k), Some(Token::from("autre")));
        // Écriture acquise : plus rien à annuler.
        let seq = link.store(k.clone(), Token::from("final"), Expect::Anything);
        assert!(matches!(next(&mut link), Reply::Stored { result: Ok(()), .. }));
        link.settle(seq);
        link.undo(seq);
        link.load(k.clone());
        assert!(matches!(next(&mut link), Reply::Loaded { result: Ok(Some(t)), .. } if t == Token::from("final")));
    }

    #[test]
    fn startup_probes_a_forgotten_imported_entry() {
        let v = Arc::new(MemoryVault::with([(AccountKey::imported(), Token::from("i")), (AccountKey::of_user(3), Token::from("t"))]));
        let mut link = AccountLink::start(v);
        link.startup(None, Some(AccountKey::of_user(3)), true, true, None);
        match next(&mut link) {
            Reply::Started(s) => {
                assert_eq!(s.migration, Migration::NoFile);
                assert!(s.imported_found);
                assert_eq!(s.opened, Some((AccountKey::of_user(3), Ok(Some(Token::from("t"))))));
            }
            other => panic!("{other:?}"),
        }
        link.startup(None, None, false, true, None);
        assert!(matches!(next(&mut link), Reply::Started(Started { imported_found: false, opened: None, .. })));
    }

    /// Recherche d'entrées : seulement les clés demandées, sans jeton dans la
    /// réponse ; coffre illisible : rien de trouvé.
    #[test]
    fn probe_finds_only_present_entries() {
        let v = Arc::new(MemoryVault::with([(AccountKey::of_user(3), Token::from("t"))]));
        let mut link = AccountLink::start(v.clone());
        link.probe(vec![AccountKey::of_user(3), AccountKey::of_user(4)]);
        assert!(matches!(next(&mut link), Reply::Probed(found) if found == vec![AccountKey::of_user(3)]));
        v.set_faults(Faults { fail_get: true, ..Faults::default() });
        link.probe(vec![AccountKey::of_user(3)]);
        assert!(matches!(next(&mut link), Reply::Probed(found) if found.is_empty()));
    }

    /// Les réponses passent par `Debug` dans les messages d'échec des tests :
    /// le jeton n'y apparaît pas.
    #[test]
    fn replies_never_show_the_token() {
        const SECRET: &str = "MTk4.Qp9Lz.FAUX-7vXc";
        let r = Reply::Loaded { key: AccountKey::of_user(1), result: Ok(Some(Token::from(SECRET))) };
        let st = Reply::Stored { seq: 1, key: AccountKey::of_user(1), result: Err(VaultError::Failed("x".into())), put_back: None, recover: Some(Token::from(SECRET)), left: None };
        let s = Started { migration: Migration::Failed { token: Token::from(SECRET), why: "x".into() }, imported_found: false, opened: None };
        for text in [format!("{r:?}"), format!("{st:?}"), format!("{s:#?}")] {
            assert!(!crate::account::token::leaks(&text, SECRET), "{text}");
        }
    }

    /// L'ancien fichier est supprimé à la demande, quel que soit son contenu.
    #[test]
    fn legacy_file_is_wiped_on_request() {
        let dir = std::env::temp_dir().join(format!("ferrocord-oubli-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
        let path = dir.join("token");
        fs::write(&path, "autre\n").unwrap_or_else(|e| panic!("{e}"));
        let mut link = AccountLink::start(Arc::new(MemoryVault::new()));
        let keys = vec![AccountKey::imported()];
        link.check_wipe(path.clone(), keys.clone());
        let Reply::WipeChecked(check, survey) = next(&mut link) else { panic!("relecture attendue") };
        assert_eq!(check, WipeCheck::OnlyCopy, "coffre vide : seule copie");
        link.wipe_legacy(path.clone(), keys.clone(), survey.clone());
        assert!(matches!(next(&mut link), Reply::LegacyWiped(Ok(()))));
        assert!(!path.exists());
        // Confirmation périmée (déjà supprimé ailleurs) : la question est reposée.
        link.wipe_legacy(path.clone(), keys, survey);
        match next(&mut link) {
            Reply::Stale { act: Act::Wipe, survey } => assert!(matches!(survey.file, FileState::Absent)),
            other => panic!("{other:?}"),
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// Retrait confirmé sur un état que l'autre instance a changé : rien
    /// n'est retiré, la question est reposée avec l'état relu.
    #[test]
    fn stale_forget_is_not_done() {
        let k = AccountKey::of_user(3);
        let v = Arc::new(MemoryVault::with([(k.clone(), Token::from("MTk4.Aa1Bb.TEST-1aaa"))]));
        let mut link = AccountLink::start(v.clone());
        link.survey(k.clone(), None, vec![k.clone()]);
        let Reply::Surveyed { survey, .. } = next(&mut link) else { panic!("relecture attendue") };
        // Autre instance : jeton renouvelé entre la question et la confirmation.
        let renewed = Token::from("MTk4.Cc2Dd.TEST-2bbb");
        v.set(&k, &renewed).unwrap_or_else(|e| panic!("{e}"));
        link.forget(k.clone(), None, vec![k.clone()], Some(survey));
        match next(&mut link) {
            Reply::Stale { act: Act::Remove(key), survey } => {
                assert_eq!(key, k);
                assert_eq!(survey.entry(&k), Ok(Some(&renewed)));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(v.peek(&k), Some(renewed), "rien n'est retiré");
    }

    /// Coffre dont la n-ième lecture (à partir de 0) est lente (ici la
    /// relecture : les deux premières vérifient l'absence).
    struct SlowGet {
        inner: Arc<MemoryVault>,
        slow: usize,
        pause: Duration,
        gets: std::sync::atomic::AtomicUsize,
    }

    impl Vault for SlowGet {
        fn get(&self, k: &AccountKey) -> Result<Option<Token>, VaultError> {
            if self.gets.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == self.slow {
                std::thread::sleep(self.pause);
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
            Some("lent")
        }
    }

    /// Relecture abandonnée après le délai : rien n'est retiré ni remis
    /// ensuite (l'appel abandonné peut encore aboutir), et plus aucun appel
    /// n'est fait au coffre pendant la session.
    #[test]
    fn no_rollback_after_a_timeout() {
        let mem = Arc::new(MemoryVault::new());
        let slow = Arc::new(SlowGet { inner: mem.clone(), slow: 2, pause: Duration::from_millis(400), gets: Default::default() });
        let mut link = AccountLink::start_with(slow, LinkOptions { vault_timeout: Duration::from_millis(60), ..LinkOptions::default() });
        let k = AccountKey::of_user(7);
        link.store(k.clone(), Token::from("saisi"), Expect::Anything);
        let r = next(&mut link);
        assert!(matches!(&r, Reply::Stored { result: Err(VaultError::Unavailable(_)), .. }), "{r:?}");
        std::thread::sleep(Duration::from_millis(700));
        assert_eq!(mem.peek(&k), Some(Token::from("saisi")), "l'entrée écrite n'est pas retirée après un délai dépassé");
        let calls = mem.calls();
        link.load(k);
        assert!(matches!(next(&mut link), Reply::Loaded { result: Err(_), .. }));
        assert_eq!(mem.calls(), calls, "plus aucun appel au coffre pendant cette session");
    }

    /// Fermer l'application n'attend pas un coffre qui ne répond plus.
    #[test]
    fn shutdown_is_bounded() {
        struct SlowSet;
        impl Vault for SlowSet {
            fn get(&self, _: &AccountKey) -> Result<Option<Token>, VaultError> {
                Ok(None)
            }
            fn set(&self, _: &AccountKey, _: &Token) -> Result<(), VaultError> {
                std::thread::sleep(Duration::from_secs(8));
                Ok(())
            }
            fn delete(&self, _: &AccountKey) -> Result<(), VaultError> {
                Ok(())
            }
            fn name(&self) -> Option<&'static str> {
                None
            }
        }
        let mut link = AccountLink::start_with(Arc::new(SlowSet), LinkOptions { vault_timeout: Duration::from_secs(30), ..LinkOptions::default() });
        link.store(AccountKey::of_user(1), Token::from("x"), Expect::Anything);
        std::thread::sleep(Duration::from_millis(100));
        let start = Instant::now();
        drop(link);
        assert!(start.elapsed() < Duration::from_secs(5), "arrêt en {:?}", start.elapsed());
    }

    /// Un coffre bloqué ne bloque pas le fil : la demande répond après le délai.
    #[test]
    fn stuck_vault_answers_after_the_timeout() {
        struct Stuck;
        impl Vault for Stuck {
            fn get(&self, _: &AccountKey) -> Result<Option<Token>, VaultError> {
                std::thread::sleep(Duration::from_secs(3));
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
        let mut link = AccountLink::start_with(Arc::new(Stuck), LinkOptions { vault_timeout: Duration::from_millis(50), ..LinkOptions::default() });
        let start = Instant::now();
        link.load(AccountKey::of_user(1));
        assert!(matches!(next(&mut link), Reply::Loaded { result: Err(VaultError::Unavailable(_)), .. }));
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
