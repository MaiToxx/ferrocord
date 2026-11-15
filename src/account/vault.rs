//! Coffre à jetons : une entrée par compte. Sous Windows, le Gestionnaire
//! d'identifiants (identifiants génériques, persistance « ordinateur local » :
//! le jeton ne suit pas un profil itinérant). Ailleurs, aucun coffre : les
//! jetons ne sont pas retenus d'un lancement à l'autre (voir [`SystemVault`]).
//!
//! Les appels au coffre peuvent bloquer : ils ne sont faits que par le fil
//! des comptes (`account::link`), jamais par le fil d'interface.
//!
//! Aucune erreur ne transporte de donnée lue : les erreurs de la crate
//! `keyring-core` peuvent contenir le secret illisible (`BadEncoding`), elles
//! sont donc traduites en messages fixes, jamais affichées telles quelles.

use super::{AccountKey, Token};
use crate::i18n::{
    language,
    verbatim::{f as vf, t as vt},
    Text,
};
use std::{
    collections::HashMap,
    fmt,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};

/// Échec d'un appel au coffre (aucune donnée du coffre dedans).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VaultError {
    /// Coffre inaccessible ou absent sur ce système.
    Unavailable(String),
    /// Lecture, écriture ou suppression refusée.
    Failed(String),
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VaultError::Unavailable(why) => f.write_str(&vf::vault_unavailable(language(), why)),
            VaultError::Failed(why) => f.write_str(why),
        }
    }
}

/// Un coffre à jetons, indexé par compte.
pub trait Vault: Send + Sync {
    /// `Ok(None)` : aucune entrée pour ce compte.
    fn get(&self, key: &AccountKey) -> Result<Option<Token>, VaultError>;
    fn set(&self, key: &AccountKey, token: &Token) -> Result<(), VaultError>;
    /// Supprimer une entrée absente n'est pas une erreur.
    fn delete(&self, key: &AccountKey) -> Result<(), VaultError>;
    /// Nom du coffre pour l'écran de connexion ; `None` s'il n'y en a pas.
    fn name(&self) -> Option<&'static str>;
    /// Nom de l'entrée d'un compte, tel que l'utilisateur peut la retrouver
    /// (Gestionnaire d'identifiants : son « adresse »).
    fn entry_name(&self, key: &AccountKey) -> String {
        vf::entry_account(language(), key)
    }
    /// Un appel a été abandonné après son délai : on ne sait plus ce que
    /// contient le coffre, rien n'est retiré ni remis pendant cette session.
    fn indeterminate(&self) -> bool {
        false
    }
}

/// Lecture dont la réponse « absent » est confirmée par une deuxième
/// lecture : à utiliser quand l'absence mène à écrire ou à retirer.
pub fn get_confirmed(vault: &dyn Vault, key: &AccountKey) -> Result<Option<Token>, VaultError> {
    match vault.get(key)? {
        Some(t) => Ok(Some(t)),
        None => vault.get(key),
    }
}

/// Pannes simulées par [`MemoryVault`] (tests et aperçu).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Faults {
    /// Tout appel échoue comme un coffre inaccessible.
    pub unavailable: bool,
    /// Les écritures échouent.
    pub fail_set: bool,
    /// Les lectures échouent.
    pub fail_get: bool,
    /// Les suppressions échouent.
    pub fail_delete: bool,
    /// Une écriture garde un autre texte que celui donné (relecture différente).
    pub corrupt_set: bool,
}

/// Coffre en mémoire : tests et aperçu hors ligne. Rien n'est écrit nulle part.
#[derive(Default)]
pub struct MemoryVault {
    entries: Mutex<HashMap<AccountKey, Token>>,
    faults: Mutex<Faults>,
    /// Nombre d'appels reçus (tous genres confondus), pour les tests.
    calls: Mutex<usize>,
}

impl MemoryVault {
    pub fn new() -> Self {
        Self::default()
    }

    /// Coffre déjà rempli.
    pub fn with(entries: impl IntoIterator<Item = (AccountKey, Token)>) -> Self {
        let v = Self::new();
        if let Ok(mut e) = v.entries.lock() {
            e.extend(entries);
        }
        v
    }

    pub fn set_faults(&self, f: Faults) {
        if let Ok(mut faults) = self.faults.lock() {
            *faults = f;
        }
    }

    /// Contenu actuel (tests).
    pub fn peek(&self, key: &AccountKey) -> Option<Token> {
        self.entries.lock().ok().and_then(|e| e.get(key).cloned())
    }

    /// Tout le contenu (tests : coffre « sur le disque » avant une panne de courant).
    pub fn snapshot(&self) -> HashMap<AccountKey, Token> {
        self.entries.lock().map(|e| e.clone()).unwrap_or_default()
    }

    /// Remet un contenu pris par [`MemoryVault::snapshot`] (panne de courant simulée).
    pub fn restore(&self, entries: HashMap<AccountKey, Token>) {
        if let Ok(mut e) = self.entries.lock() {
            *e = entries;
        }
    }

    pub fn len(&self) -> usize {
        self.entries.lock().map(|e| e.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn calls(&self) -> usize {
        self.calls.lock().map(|c| *c).unwrap_or(0)
    }

    fn begin(&self) -> Result<Faults, VaultError> {
        if let Ok(mut c) = self.calls.lock() {
            *c += 1;
        }
        let f = self.faults.lock().map(|f| *f).unwrap_or_default();
        if f.unavailable {
            return Err(VaultError::Unavailable("coffre de test hors service".into()));
        }
        Ok(f)
    }
}

impl Vault for MemoryVault {
    fn get(&self, key: &AccountKey) -> Result<Option<Token>, VaultError> {
        if self.begin()?.fail_get {
            return Err(VaultError::Failed("lecture refusée (test)".into()));
        }
        Ok(self.peek(key))
    }

    fn set(&self, key: &AccountKey, token: &Token) -> Result<(), VaultError> {
        let f = self.begin()?;
        if f.fail_set {
            return Err(VaultError::Failed("écriture refusée (test)".into()));
        }
        let stored = if f.corrupt_set { Token::from(format!("{}-abime", token.expose())) } else { token.clone() };
        let mut e = self.entries.lock().map_err(|_| VaultError::Failed("verrou".into()))?;
        e.insert(key.clone(), stored);
        Ok(())
    }

    fn delete(&self, key: &AccountKey) -> Result<(), VaultError> {
        if self.begin()?.fail_delete {
            return Err(VaultError::Failed("suppression refusée (test)".into()));
        }
        let mut e = self.entries.lock().map_err(|_| VaultError::Failed("verrou".into()))?;
        e.remove(key);
        Ok(())
    }

    fn name(&self) -> Option<&'static str> {
        Some(vt::DEMO_VAULT.get(language()))
    }
}

/// Délai maximal d'un appel au coffre.
pub const VAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Travail confié au fil du coffre.
type Job = Box<dyn FnOnce(&dyn Vault) + Send>;

/// Coffre servi par un seul fil permanent (« coffre ») qui exécute tous
/// les appels, l'un après l'autre, dans l'ordre où ils sont faits : le
/// Gestionnaire d'identifiants ne garantit pas l'ordre d'appels venus de
/// fils différents (une lecture juste après une écriture peut répondre
/// « absent »).
///
/// Le délai maximal porte sur l'attente de la réponse. Dépassé, l'appel est
/// abandonné (il peut encore aboutir plus tard) et le coffre devient
/// « indéterminé » pour le reste de la session : tous les appels suivants
/// répondent « indisponible » sans être faits, rien n'est retiré ni remis.
/// [`TimedVault::busy`] dit si un appel abandonné tourne encore (le verrou
/// entre instances reste tenu pendant ce temps).
pub struct TimedVault {
    jobs: Option<mpsc::Sender<Job>>,
    limit: Duration,
    name: Option<&'static str>,
    /// Pour `entry_name` seulement (aucun appel au coffre).
    inner: Arc<dyn Vault>,
    stalled: Arc<AtomicBool>,
    abandoned: Arc<AtomicUsize>,
}

/// Où en est un appel : fini, ou abandonné par l'appelant.
#[derive(Default)]
struct Call {
    done: bool,
    abandoned: bool,
}

impl TimedVault {
    pub fn new(inner: Arc<dyn Vault>, limit: Duration) -> Self {
        let name = inner.name();
        let (tx, rx) = mpsc::channel::<Job>();
        let worker = inner.clone();
        // Le fil s'arrête quand le dernier émetteur disparaît ; il n'est
        // jamais attendu (un appel bloqué ne retient pas la fermeture).
        let jobs = std::thread::Builder::new()
            .name("coffre".into())
            .spawn(move || {
                for job in rx {
                    job(worker.as_ref());
                }
            })
            .ok()
            .map(|_| tx);
        Self { jobs, limit, name, inner, stalled: Arc::default(), abandoned: Arc::default() }
    }

    /// Un appel abandonné après son délai n'est pas encore terminé.
    pub fn busy(&self) -> bool {
        self.abandoned.load(Ordering::SeqCst) > 0
    }

    /// Fonction « un appel abandonné tourne encore », pour le verrou.
    pub fn busy_probe(&self) -> impl Fn() -> bool + Send + Sync + 'static {
        let a = self.abandoned.clone();
        move || a.load(Ordering::SeqCst) > 0
    }

    fn timed<T: Send + 'static>(&self, f: impl FnOnce(&dyn Vault) -> Result<T, VaultError> + Send + 'static) -> Result<T, VaultError> {
        if self.stalled.load(Ordering::SeqCst) {
            return Err(VaultError::Unavailable(INDETERMINATE.get(language()).into()));
        }
        let Some(jobs) = &self.jobs else { return Err(VaultError::Unavailable(vt::VAULT_THREAD_FAILED.get(language()).into())) };
        let (tx, rx) = mpsc::channel();
        let call = Arc::new(Mutex::new(Call::default()));
        let (c, abandoned) = (call.clone(), self.abandoned.clone());
        let job: Job = Box::new(move |v| {
            let r = f(v);
            let mut c = c.lock().unwrap_or_else(|e| e.into_inner());
            c.done = true;
            let _ = tx.send(r);
            if c.abandoned {
                abandoned.fetch_sub(1, Ordering::SeqCst);
            }
        });
        if jobs.send(job).is_err() {
            return Err(VaultError::Unavailable(vt::VAULT_THREAD_STOPPED.get(language()).into()));
        }
        match rx.recv_timeout(self.limit) {
            Ok(r) => r,
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(VaultError::Unavailable(vt::VAULT_THREAD_STOPPED.get(language()).into())),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let mut c = call.lock().unwrap_or_else(|e| e.into_inner());
                if c.done {
                    // Fini juste à temps : la réponse est là.
                    return rx.try_recv().unwrap_or_else(|_| Err(VaultError::Unavailable(vt::VAULT_REPLY_LOST.get(language()).into())));
                }
                c.abandoned = true;
                self.abandoned.fetch_add(1, Ordering::SeqCst);
                self.stalled.store(true, Ordering::SeqCst);
                Err(VaultError::Unavailable(vf::vault_no_answer(language(), self.limit.as_secs_f32().ceil())))
            }
        }
    }
}

/// Réponse de tout appel après un délai dépassé.
const INDETERMINATE: Text = vt::INDETERMINATE;

impl Vault for TimedVault {
    fn get(&self, key: &AccountKey) -> Result<Option<Token>, VaultError> {
        let key = key.clone();
        self.timed(move |v| v.get(&key))
    }

    fn set(&self, key: &AccountKey, token: &Token) -> Result<(), VaultError> {
        let (key, token) = (key.clone(), token.clone());
        self.timed(move |v| v.set(&key, &token))
    }

    fn delete(&self, key: &AccountKey) -> Result<(), VaultError> {
        let key = key.clone();
        self.timed(move |v| v.delete(&key))
    }

    fn name(&self) -> Option<&'static str> {
        self.name
    }

    fn entry_name(&self, key: &AccountKey) -> String {
        self.inner.entry_name(key)
    }

    fn indeterminate(&self) -> bool {
        self.stalled.load(Ordering::SeqCst)
    }
}

/// Coffre du système : le Gestionnaire d'identifiants sous Windows.
///
/// Repli hors Windows : aucun coffre (chaque appel répond « indisponible ») ;
/// un jeton saisi sert pour la session en cours sans être retenu, et un
/// ancien fichier `token` n'est pas migré (il reste en place et sert tel quel).
pub struct SystemVault {
    /// Préfixe du nom des entrées (`ferrocord:compte:` ; autre pour le test réel).
    prefix: String,
    #[cfg(windows)]
    store: Option<std::sync::Arc<windows_native_keyring_store::Store>>,
}

/// Service enregistré avec chaque entrée.
#[cfg(windows)]
const SERVICE: &str = "ferrocord";

impl SystemVault {
    pub fn new() -> Self {
        Self::with_prefix("ferrocord:compte:")
    }

    /// Entrées nommées `<prefix><compte>` (le test réel prend un préfixe à lui).
    pub fn with_prefix(prefix: &str) -> Self {
        Self {
            prefix: prefix.to_string(),
            #[cfg(windows)]
            store: windows_native_keyring_store::Store::new().ok(),
        }
    }

    /// Nom de l'entrée d'un compte dans le Gestionnaire d'identifiants.
    pub fn target(&self, key: &AccountKey) -> String {
        format!("{}{}", self.prefix, key.as_str())
    }

    #[cfg(windows)]
    fn entry(&self, key: &AccountKey) -> Result<keyring_core::Entry, VaultError> {
        use keyring_core::api::CredentialStoreApi;
        let store = self.store.as_ref().ok_or_else(|| VaultError::Unavailable(vt::CREDMAN_UNREACHABLE.get(language()).into()))?;
        let target = self.target(key);
        // « Local » : l'entrée reste sur cet ordinateur (pas de profil itinérant).
        let mods = HashMap::from([("target", target.as_str()), ("persistence", "Local")]);
        store.build(SERVICE, key.as_str(), Some(&mods)).map_err(describe)
    }
}

impl Default for SystemVault {
    fn default() -> Self {
        Self::new()
    }
}

/// Erreur du coffre Windows en français, sans aucune donnée lue.
#[cfg(windows)]
fn describe(e: keyring_core::Error) -> VaultError {
    use keyring_core::Error as E;
    match e {
        E::NoStorageAccess(err) => VaultError::Unavailable(vf::access_denied(language(), err)),
        E::PlatformFailure(err) => VaultError::Failed(vf::windows_error(language(), err)),
        E::NoDefaultStore => VaultError::Unavailable(vt::CREDMAN_UNREACHABLE.get(language()).into()),
        E::NoEntry => VaultError::Failed(vt::ENTRY_NOT_FOUND.get(language()).into()),
        E::BadEncoding(_) | E::BadDataFormat(..) | E::BadStoreFormat(_) => VaultError::Failed(vt::ENTRY_UNREADABLE.get(language()).into()),
        E::TooLong(..) => VaultError::Failed(vt::TOKEN_TOO_LONG.get(language()).into()),
        E::Invalid(..) => VaultError::Failed(vt::ENTRY_REJECTED.get(language()).into()),
        E::Ambiguous(_) => VaultError::Failed(vt::ENTRY_AMBIGUOUS.get(language()).into()),
        _ => VaultError::Failed(vt::OPERATION_REFUSED.get(language()).into()),
    }
}

#[cfg(windows)]
impl Vault for SystemVault {
    fn get(&self, key: &AccountKey) -> Result<Option<Token>, VaultError> {
        match self.entry(key)?.get_password() {
            Ok(s) => Ok(Some(Token::from(s))),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(e) => Err(describe(e)),
        }
    }

    fn set(&self, key: &AccountKey, token: &Token) -> Result<(), VaultError> {
        self.entry(key)?.set_password(token.expose()).map_err(describe)
    }

    fn delete(&self, key: &AccountKey) -> Result<(), VaultError> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(e) => Err(describe(e)),
        }
    }

    fn name(&self) -> Option<&'static str> {
        Some(vt::WINDOWS_VAULT.get(language()))
    }

    fn entry_name(&self, key: &AccountKey) -> String {
        self.target(key)
    }
}

/// Android : jeton chiffré (AES-GCM) par une clé du Keystore, qui ne sort
/// jamais du système ; le fichier chiffré est dans le stockage privé.
#[cfg(target_os = "android")]
impl Vault for SystemVault {
    fn get(&self, key: &AccountKey) -> Result<Option<Token>, VaultError> {
        crate::platform::android::vault::get(key.as_str()).map(|t| t.map(Token::from)).map_err(android_error)
    }
    fn set(&self, key: &AccountKey, token: &Token) -> Result<(), VaultError> {
        crate::platform::android::vault::set(key.as_str(), token.expose()).map_err(android_error)
    }
    fn delete(&self, key: &AccountKey) -> Result<(), VaultError> {
        crate::platform::android::vault::delete(key.as_str()).map_err(android_error)
    }
    fn name(&self) -> Option<&'static str> {
        Some(vt::ANDROID_VAULT.get(language()))
    }
}

#[cfg(target_os = "android")]
fn android_error(e: crate::platform::android::vault::Error) -> VaultError {
    use crate::platform::android::vault::Error as E;
    match e {
        E::Keystore => VaultError::Unavailable(vt::KEYSTORE_UNREACHABLE.get(language()).into()),
        E::File => VaultError::Failed(vt::KEYSTORE_FILE_FAILED.get(language()).into()),
        E::Unreadable => VaultError::Failed(vt::ENTRY_UNREADABLE.get(language()).into()),
    }
}

#[cfg(not(any(windows, target_os = "android")))]
impl Vault for SystemVault {
    fn get(&self, _: &AccountKey) -> Result<Option<Token>, VaultError> {
        Err(unavailable())
    }
    fn set(&self, _: &AccountKey, _: &Token) -> Result<(), VaultError> {
        Err(unavailable())
    }
    fn delete(&self, _: &AccountKey) -> Result<(), VaultError> {
        Err(unavailable())
    }
    fn name(&self) -> Option<&'static str> {
        None
    }
}

#[cfg(not(any(windows, target_os = "android")))]
fn unavailable() -> VaultError {
    VaultError::Unavailable(vt::NO_VAULT.get(language()).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &str) -> AccountKey {
        AccountKey::parse(s).unwrap_or_else(|| panic!("clé {s}"))
    }

    #[test]
    fn memory_vault_round_trip_and_faults() {
        let v = MemoryVault::new();
        let k = key("42");
        assert_eq!(v.get(&k), Ok(None));
        assert_eq!(v.set(&k, &Token::from("a")), Ok(()));
        assert_eq!(v.get(&k), Ok(Some(Token::from("a"))));
        assert_eq!(v.delete(&k), Ok(()));
        assert_eq!(v.delete(&k), Ok(()), "absente : pas une erreur");
        assert_eq!(v.get(&k), Ok(None));

        v.set_faults(Faults { corrupt_set: true, ..Faults::default() });
        assert_eq!(v.set(&k, &Token::from("a")), Ok(()));
        assert_ne!(v.get(&k), Ok(Some(Token::from("a"))));
        v.set_faults(Faults { unavailable: true, ..Faults::default() });
        assert!(matches!(v.get(&k), Err(VaultError::Unavailable(_))));
        v.set_faults(Faults { fail_set: true, fail_delete: true, ..Faults::default() });
        assert!(v.set(&k, &Token::from("b")).is_err());
        assert!(v.delete(&k).is_err());
    }

    /// Coffre qui ne répond plus.
    struct Stuck;

    impl Vault for Stuck {
        fn get(&self, _: &AccountKey) -> Result<Option<Token>, VaultError> {
            std::thread::sleep(Duration::from_secs(3));
            Ok(None)
        }
        fn set(&self, _: &AccountKey, _: &Token) -> Result<(), VaultError> {
            self.get(&key("1")).map(|_| ())
        }
        fn delete(&self, _: &AccountKey) -> Result<(), VaultError> {
            self.get(&key("1")).map(|_| ())
        }
        fn name(&self) -> Option<&'static str> {
            Some("bloqué")
        }
    }

    #[test]
    fn timed_vault_gives_up_on_a_stuck_vault() {
        let v = TimedVault::new(Arc::new(Stuck), Duration::from_millis(50));
        let start = std::time::Instant::now();
        assert!(matches!(v.get(&key("1")), Err(VaultError::Unavailable(m)) if m.contains("ne répond pas")));
        assert!(v.set(&key("1"), &Token::from("a")).is_err());
        assert!(start.elapsed() < Duration::from_secs(2), "pas d'attente au-delà du délai");
        assert!(v.indeterminate() && v.busy(), "coffre indéterminé, appel abandonné en cours");
        assert_eq!(v.name(), Some("bloqué"));
        // Un coffre qui répond passe à travers sans changement.
        let ok = TimedVault::new(Arc::new(MemoryVault::new()), Duration::from_secs(5));
        assert_eq!(ok.set(&key("2"), &Token::from("b")), Ok(()));
        assert_eq!(ok.get(&key("2")), Ok(Some(Token::from("b"))));
    }

    /// Coffre qui note sur quel fil chaque appel est fait.
    #[derive(Default)]
    struct Threads(Mutex<Vec<std::thread::ThreadId>>, MemoryVault);

    impl Threads {
        fn note(&self) {
            if let Ok(mut v) = self.0.lock() {
                v.push(std::thread::current().id());
            }
        }
    }

    impl Vault for Threads {
        fn get(&self, k: &AccountKey) -> Result<Option<Token>, VaultError> {
            self.note();
            self.1.get(k)
        }
        fn set(&self, k: &AccountKey, t: &Token) -> Result<(), VaultError> {
            self.note();
            self.1.set(k, t)
        }
        fn delete(&self, k: &AccountKey) -> Result<(), VaultError> {
            self.note();
            self.1.delete(k)
        }
        fn name(&self) -> Option<&'static str> {
            None
        }
    }

    /// Le coffre de Windows ne garantit pas l'ordre d'appels faits depuis
    /// des fils différents : tous passent par un seul fil, toujours le même.
    #[test]
    fn calls_run_in_order_on_one_permanent_thread() {
        let seen = Arc::new(Threads::default());
        let v = TimedVault::new(seen.clone(), Duration::from_secs(5));
        let k = key("1");
        for i in 0..5 {
            let t = Token::from(format!("jeton-{i}"));
            assert_eq!(v.set(&k, &t), Ok(()));
            assert_eq!(v.get(&k), Ok(Some(t)), "écriture puis lecture immédiate, tour {i}");
        }
        assert_eq!(v.delete(&k), Ok(()));
        assert_eq!(v.get(&k), Ok(None));
        let ids = seen.0.lock().map(|v| v.clone()).unwrap_or_default();
        assert_eq!(ids.len(), 12);
        assert!(ids.iter().all(|id| *id == ids[0]), "un seul fil pour tous les appels : {ids:?}");
        assert_ne!(ids[0], std::thread::current().id(), "pas le fil de l'appelant");
    }

    /// Le seul test qui touche au vrai Gestionnaire d'identifiants : une
    /// entrée au nom manifestement de test, supprimée à la fin (et vérifiée
    /// absente). Lancé seulement à la demande :
    /// `cargo test system_vault_round_trip -- --ignored`.
    #[test]
    #[ignore = "écrit dans le vrai Gestionnaire d'identifiants (une entrée de test, supprimée)"]
    fn system_vault_round_trip() {
        use std::hash::{BuildHasher, Hasher};
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        let prefix = format!("ferrocord-test-{:016x}:", h.finish());
        let sys = Arc::new(SystemVault::with_prefix(&prefix));
        let k = key("1");
        let target = sys.target(&k);

        /// Supprime l'entrée de test même si le test échoue (directement,
        /// sans passer par le fil du coffre, qui pourrait être bloqué).
        struct Cleanup(Arc<SystemVault>, AccountKey);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = self.0.delete(&self.1);
            }
        }
        let _cleanup = Cleanup(sys.clone(), k.clone());

        // Tout passe par le fil unique et le délai, comme dans l'application.
        let v = TimedVault::new(sys.clone(), VAULT_TIMEOUT);
        let first = Token::from("faux.jeton.DE-TEST-pour-le-coffre");
        let second = Token::from("autre.faux.jeton.DE-TEST-ecrase");
        assert_eq!(v.get(&k), Ok(None), "{target} : absente au départ");
        assert_eq!(v.set(&k, &first), Ok(()), "création");
        assert_eq!(v.get(&k), Ok(Some(first)), "lecture immédiate");
        assert_eq!(v.set(&k, &second), Ok(()), "écrasement d'une entrée existante");
        assert_eq!(v.get(&k), Ok(Some(second)), "lecture après écrasement");
        assert_eq!(v.delete(&k), Ok(()), "suppression");
        assert_eq!(v.get(&k), Ok(None), "absente après suppression");
        assert_eq!(v.delete(&k), Ok(()), "supprimer une entrée absente n'est pas une erreur");
        for i in 0..50 {
            let t = Token::from(format!("faux.jeton.DE-TEST-cycle-{i}"));
            assert_eq!(v.set(&k, &t), Ok(()), "cycle {i} : écriture");
            assert_eq!(v.get(&k), Ok(Some(t)), "cycle {i} : lecture immédiate (jamais « introuvable »)");
        }
        assert_eq!(v.delete(&k), Ok(()));
        assert_eq!(v.get(&k), Ok(None), "entrée de test nettoyée");
        assert!(!v.indeterminate(), "aucun délai dépassé");
        println!("entrée de test {target} : création, écrasement, suppression et 50 cycles écriture-lecture réussis, entrée supprimée");
    }
}
