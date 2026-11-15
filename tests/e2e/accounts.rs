//! Comptes de bout en bout : la vraie application (sans fenêtre), la vraie
//! session réseau face au faux serveur, un coffre en mémoire et des réglages
//! dans un dossier temporaire. Aucun jeton réel, aucun fichier de
//! l'utilisateur, aucun appel au vrai Gestionnaire d'identifiants.

use crate::support::{ids::*, *};
use eframe::egui;
use ferrocord::account::{AccountKey, AccountLink, MemoryVault, Token, TokenError, Vault, VaultError};
use ferrocord::images::Images;
use ferrocord::session::Session;
use ferrocord::settings::{self, Account, Settings, SettingsLink};
use ferrocord::state::Store;
use ferrocord::theme::{ThemeLink, WatchConfig};
use ferrocord::ui::{App, Boot};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const TOKEN_A: &str = "MTAx.QzR4-A.wq8Vtz-FAUX";
const TOKEN_B: &str = "MTAx.Yp3K-B.zz5Lmn-FAUX";
const TOKEN_ENV: &str = "MTAx.Hd2W-E.pk7Jqs-FAUX";
/// Deuxième identité (le compte B).
const OTHER: u64 = 1000000000000000077;
/// Message reçu en direct par le compte A seulement.
const A_ONLY: u64 = 5000000000000000300;

/// Dossier temporaire supprimé à la fin.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("ferrocord-comptes-{}-{}-{name}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{dir:?} : {e}"));
        Self(dir)
    }
    fn settings(&self) -> PathBuf {
        self.0.join("settings.toml")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Faux serveurs sur leur propre runtime : l'application, elle, a le sien
/// (celui de `Session`) et tourne sur le fil du test, comme une fenêtre.
struct Bench {
    rt: tokio::runtime::Runtime,
    gw: FakeGateway,
    http: FakeHttp,
}

impl Bench {
    fn new() -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap_or_else(|e| panic!("{e}"));
        let (gw, http) = rt.block_on(async { (FakeGateway::start().await, FakeHttp::start().await) });
        // Datée comme toute réponse de Discord : l'étape 2 attend cette heure (B1).
        http.on("GET", &messages_path(GENERAL), [Response::ok(fixture("rest_history")).dated(&http_date(now_secs()))]);
        Self { rt, gw, http }
    }

    fn app(&self, vault: Arc<dyn Vault>, settings: Option<&Path>, env_token: Option<Token>, legacy: Option<PathBuf>) -> App {
        self.app_env(vault, settings, env_token.map(Ok), legacy)
    }

    fn app_env(&self, vault: Arc<dyn Vault>, settings: Option<&Path>, env_token: Option<Result<Token, TokenError>>, legacy: Option<PathBuf>) -> App {
        let ctx = egui::Context::default();
        let cfg = WatchConfig { poll: Duration::from_millis(50), debounce: Duration::from_millis(50) };
        let boot = Boot {
            backend: Box::new(Session::with_endpoints(endpoints(&self.gw, &self.http))),
            store: Store::new(),
            theme: ThemeLink::open(None, false, WatchConfig::default()),
            settings: SettingsLink::open(settings.map(Path::to_path_buf), true, cfg),
            accounts: AccountLink::start(vault),
            env_token,
            legacy,
            images: Images::disabled(ctx.clone()),
        };
        App::with_context(ctx, boot)
    }

    /// Prochaine connexion, en faisant tourner l'application (le jeton peut
    /// d'abord devoir être lu dans le coffre) : Hello, Identify (dont le
    /// jeton est renvoyé), puis READY de `user`.
    fn session(&mut self, app: &mut App, user: Option<(u64, &str)>) -> (GwConn, String) {
        let end = Instant::now() + WAIT;
        let mut conn = loop {
            app.step();
            let gw = &mut self.gw;
            if let Ok(c) = self.rt.block_on(async { tokio::time::timeout(Duration::from_millis(10), gw.accept()).await }) {
                break c;
            }
            assert!(Instant::now() < end, "délai dépassé : connexion de l'application");
        };
        let identify = self.rt.block_on(conn.hello_identify(QUIET));
        let token = identify["token"].as_str().unwrap_or_default().to_string();
        if let Some((id, name)) = user {
            let mut p = ready_with_resume(&self.gw, "ready_user_classic");
            p["d"]["user"]["id"] = json!(id.to_string());
            p["d"]["user"]["global_name"] = json!(name);
            self.rt.block_on(conn.send(p));
        }
        (conn, token)
    }

    /// Prochaine requête HTTP : chemin et en-tête Authorization.
    fn request(&mut self) -> (String, Option<String>) {
        let r = self.rt.block_on(self.http.next_request());
        (r.path, r.authorization)
    }
}

/// Fait tourner l'application jusqu'à ce que `pred` soit vrai.
fn until(app: &mut App, what: &str, pred: impl Fn(&App) -> bool) {
    let end = Instant::now() + WAIT;
    loop {
        app.step();
        if pred(app) {
            return;
        }
        assert!(Instant::now() < end, "délai dépassé : {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn me(app: &App) -> Option<u64> {
    app.store().me.as_ref().map(|u| u.id)
}

fn msg_ids(app: &App, ch: u64) -> Vec<u64> {
    app.store().chans.get(&ch).map(|c| c.msgs.iter().map(|m| m.id).collect()).unwrap_or_default()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Un morceau de 6 caractères du jeton suffit à le trahir.
fn leaks(text: &str, token: &str) -> bool {
    let chars: Vec<char> = token.chars().collect();
    chars.windows(6).any(|w| text.contains(&w.iter().collect::<String>()))
}

/// × puis attente de la question (coffre et fichier relus) ; rend la question.
fn ask_remove(app: &mut App, key: &AccountKey) -> String {
    app.ask_remove_account(key.clone());
    until(app, "question de retrait prête", |a| a.remove_account_question(key).is_some());
    app.remove_account_question(key).unwrap_or_default()
}

/// × puis « Oui » sur la question relue ; rend la question.
fn confirm_remove(app: &mut App, key: &AccountKey) -> String {
    let q = ask_remove(app, key);
    assert!(app.remove_account(key.clone()), "retrait demandé : {q}");
    q
}

/// « Se déconnecter » puis « Oui » sur la question relue ; rend la question.
fn confirm_logout(app: &mut App) -> String {
    app.ask_logout();
    until(app, "question de déconnexion prête", |a| a.logout_confirmation().is_some());
    let q = app.logout_confirmation().unwrap_or_default();
    app.logout();
    q
}

/// « Supprimer le fichier de jeton en clair », question relue ; rend la question.
fn ask_wipe(app: &mut App) -> String {
    app.ask_delete_legacy_file();
    until(app, "question de suppression prête", |a| a.delete_legacy_question().is_some());
    app.delete_legacy_question().unwrap_or_default()
}

/// Heure murale actuelle, en secondes depuis 1970.
fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// L'application lit l'heure `secs` (secondes depuis 1970).
fn clock_at(app: &mut App, secs: u64) {
    app.set_clock(Arc::new(move || UNIX_EPOCH + Duration::from_secs(secs)));
}

const DAY: u64 = 24 * 3600;

/// En-tête `Date` HTTP de l'instant `secs`.
fn http_date(secs: u64) -> String {
    let at = chrono::DateTime::<chrono::Utc>::from_timestamp(i64::try_from(secs).unwrap_or(0), 0).unwrap_or_default();
    at.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

/// Une fois la session prête, ouvre #général : l'historique demandé est la
/// première réponse de Discord, dont l'heure (en-tête `Date`) décide de
/// l'étape 2 (B1).
fn discord_answers(app: &mut App) {
    until(app, "session prête", |a| me(a) == Some(ME));
    app.open(Some(ALPHA), GENERAL);
}

#[test]
fn two_accounts_switch_without_mixing() {
    let mut b = Bench::new();
    let dir = TempDir::new("deux");
    let vault = Arc::new(MemoryVault::new());
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, None);

    // Compte A, saisi et retenu : enregistré seulement après READY.
    app.add_account(Token::from(TOKEN_A), true);
    let (mut conn_a, token) = b.session(&mut app, Some((ME, "Moi Fictif")));
    assert_eq!(token, TOKEN_A);
    until(&mut app, "compte A enregistré", |a| a.current_account() == Some(&AccountKey::of_user(ME)));
    assert_eq!(vault.peek(&AccountKey::of_user(ME)), Some(Token::from(TOKEN_A)));

    // A ouvre #général (historique demandé avec le jeton A) et reçoit un message en direct.
    app.open(Some(ALPHA), GENERAL);
    assert_eq!(b.request(), (messages_path(GENERAL) + "?limit=50", Some(TOKEN_A.to_string())));
    until(&mut app, "historique de A", |a| msg_ids(a, GENERAL).len() == 3);
    b.rt.block_on(conn_a.dispatch_fixture("message_create_simple", 2));
    until(&mut app, "message en direct de A", |a| msg_ids(a, GENERAL).contains(&A_ONLY));

    // Compte B : la session de A est fermée et le magasin vidé avant tout.
    app.add_account(Token::from(TOKEN_B), true);
    assert!(app.store().me.is_none() && app.store().chans.is_empty() && app.store().guilds.is_empty());
    b.rt.block_on(conn_a.expect_closed());
    let (conn_b, token) = b.session(&mut app, Some((OTHER, "Autre Fictif")));
    assert_eq!(token, TOKEN_B);
    until(&mut app, "compte B enregistré", |a| a.current_account() == Some(&AccountKey::of_user(OTHER)));
    assert_eq!(me(&app), Some(OTHER));
    assert!(msg_ids(&app, GENERAL).is_empty(), "rien de A dans B");
    app.open(Some(ALPHA), GENERAL);
    assert_eq!(b.request().1.as_deref(), Some(TOKEN_B), "historique de B demandé avec le jeton B");
    until(&mut app, "historique de B", |a| msg_ids(a, GENERAL).len() == 3);
    assert!(!msg_ids(&app, GENERAL).contains(&A_ONLY), "le message de A n'apparaît pas dans B");

    // Retour à A, par le coffre : jeton A, et réouverture de #général.
    app.switch_account(AccountKey::of_user(ME));
    let mut conn_b = conn_b;
    b.rt.block_on(conn_b.expect_closed());
    let (_conn_a2, token) = b.session(&mut app, Some((ME, "Moi Fictif")));
    assert_eq!(token, TOKEN_A);
    until(&mut app, "A rouvert", |a| me(a) == Some(ME));
    assert_eq!(app.store().channel, Some(GENERAL), "dernier salon de A rouvert");
    assert_eq!(b.request().1.as_deref(), Some(TOKEN_A));
    until(&mut app, "historique de A (bis)", |a| msg_ids(a, GENERAL).len() == 3);
    assert!(!msg_ids(&app, GENERAL).contains(&A_ONLY), "session neuve : seul l'historique du serveur");

    // Toutes les requêtes HTTP ont porté le jeton de leur session, et rien d'autre.
    let auths: Vec<Option<String>> = b.http.requests().into_iter().map(|r| r.authorization).collect();
    assert_eq!(auths, vec![Some(TOKEN_A.into()), Some(TOKEN_B.into()), Some(TOKEN_A.into())]);

    // Réglages : deux comptes, identité connue, aucun jeton, A en dernier.
    let s = app.settings().clone();
    let ids: Vec<Option<u64>> = s.accounts.iter().map(|a| a.user_id).collect();
    assert_eq!(ids, vec![Some(ME), Some(OTHER)]);
    assert_eq!(s.last_account, Some(AccountKey::of_user(ME)));
    drop(app);
    let text = read(&dir.settings());
    assert!(text.contains("Autre Fictif") && text.contains("Moi Fictif"), "{text}");
    for t in [TOKEN_A, TOKEN_B] {
        assert!(!leaks(&text, t), "jeton dans settings.toml : {text}");
    }
    assert_eq!(settings::parse(&text).map(|s| s.accounts.len()), Ok(2));
}

/// Un jeton saisi puis refusé n'a rien laissé : ni coffre, ni liste.
#[test]
fn refused_new_token_leaves_nothing() {
    let mut b = Bench::new();
    let vault = Arc::new(MemoryVault::new());
    let mut app = b.app(vault.clone(), None, None, None);
    app.add_account(Token::from(TOKEN_A), true);
    let (mut conn, _) = b.session(&mut app, None);
    b.rt.block_on(conn.close(4004));
    until(&mut app, "refus affiché", |a| a.login_error().is_some());
    assert!(vault.is_empty());
    assert!(app.settings().accounts.is_empty());
    assert!(!leaks(app.login_error().unwrap_or_default(), TOKEN_A));
}

fn stored(vault: &MemoryVault, accounts: &[(u64, &str, &str)]) -> Settings {
    let mut s = Settings::default();
    for (id, name, token) in accounts {
        let key = AccountKey::of_user(*id);
        let _ = vault.set(&key, &Token::from(*token));
        s.accounts.push(Account { user_id: Some(*id), name: Some((*name).into()), ..Account::new(key) });
    }
    s
}

/// `DISCORD_TOKEN` refusé : la session éphémère se ferme, et rien
/// d'enregistré n'est touché (coffre, liste, fichier).
#[test]
fn refused_env_token_erases_nothing() {
    let mut b = Bench::new();
    let dir = TempDir::new("env");
    let vault = Arc::new(MemoryVault::new());
    let mut s = stored(&vault, &[(ME, "Moi Fictif", TOKEN_A)]);
    s.last_account = Some(AccountKey::of_user(ME));
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let before = read(&dir.settings());

    let mut app = b.app(vault.clone(), Some(&dir.settings()), Some(Token::from(TOKEN_ENV)), None);
    let (mut conn, token) = b.session(&mut app, None);
    assert_eq!(token, TOKEN_ENV, "DISCORD_TOKEN passe avant le dernier compte");
    b.rt.block_on(conn.close(4004));
    until(&mut app, "refus affiché", |a| a.login_error().is_some_and(|e| e.contains("DISCORD_TOKEN")));
    for _ in 0..20 {
        app.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(vault.peek(&AccountKey::of_user(ME)), Some(Token::from(TOKEN_A)));
    assert_eq!(app.settings().accounts.len(), 1);
    drop(app);
    assert_eq!(read(&dir.settings()), before, "settings.toml intact");
    assert!(!leaks(&read(&dir.settings()), TOKEN_ENV));
}

/// Jeton d'un compte enregistré refusé : rien n'est effacé (l'utilisateur
/// décide), la raison est affichée, et le compte n'est plus rouvert tout seul.
#[test]
fn refused_stored_token_erases_nothing() {
    let mut b = Bench::new();
    let vault = Arc::new(MemoryVault::new());
    let dir = TempDir::new("refus");
    let mut s = stored(&vault, &[(ME, "Moi Fictif", TOKEN_A), (OTHER, "Autre Fictif", TOKEN_B)]);
    s.last_account = Some(AccountKey::of_user(ME));
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));

    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, None);
    let (mut conn, token) = b.session(&mut app, None);
    assert_eq!(token, TOKEN_A, "dernier compte rouvert");
    b.rt.block_on(conn.close(4004));
    until(&mut app, "refus affiché", |a| a.login_error().is_some());
    idle(&mut app, 100);
    let err = app.login_error().unwrap_or_default().to_string();
    assert!(err.contains("Moi Fictif") && err.contains("Rien n'est effacé"), "{err}");
    assert_eq!(vault.peek(&AccountKey::of_user(ME)), Some(Token::from(TOKEN_A)));
    assert_eq!(vault.peek(&AccountKey::of_user(OTHER)), Some(Token::from(TOKEN_B)));
    assert_eq!(app.settings().accounts.len(), 2);
    assert_eq!(app.settings().last_account, None, "pas rouvert tout seul au prochain lancement");
}

/// Migration lancement par lancement. 1er lancement : le jeton du fichier est
/// copié dans le coffre et relu, la session s'ouvre, READY arrive, et le
/// fichier est TOUJOURS là à la fermeture (l'écriture du coffre pourrait
/// n'être qu'en mémoire). 2e lancement : le coffre a déjà ce jeton ; le
/// fichier reste jusqu'à READY, puis il est supprimé — seulement 24 h au
/// moins après la copie (heure retenue dans `settings.toml`). 3e : plus rien.
#[test]
fn legacy_file_is_deleted_on_the_second_launch_after_ready() {
    let mut b = Bench::new();
    let dir = TempDir::new("migration");
    let legacy = dir.0.join("token");
    std::fs::write(&legacy, format!("{TOKEN_A}\r\n")).unwrap_or_else(|e| panic!("{e}"));
    let vault = Arc::new(MemoryVault::new());
    let t1 = now_secs();
    // 1er lancement.
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    clock_at(&mut app, t1);
    let (_conn, token) = b.session(&mut app, Some((ME, "Moi Fictif")));
    assert_eq!(token, TOKEN_A);
    until(&mut app, "identité du compte importé", |a| a.settings().account(&AccountKey::imported()).is_some_and(|x| x.user_id == Some(ME)));
    idle(&mut app, 300);
    assert_eq!(vault.peek(&AccountKey::imported()), Some(Token::from(TOKEN_A)), "copié dans le coffre");
    assert_eq!(app.step_two(), (false, false), "pas d'étape 2 dans la session qui a écrit l'entrée");
    assert_eq!(app.settings().vault_copy_since, Some(t1), "instant de la copie retenu");
    let notice = app.legacy_notice().unwrap_or_default().to_string();
    assert!(notice.contains("sera supprimé à un prochain lancement, au plus tôt le"), "{notice}");
    drop(app);
    assert_eq!(read(&legacy), format!("{TOKEN_A}\r\n"), "1er lancement : fichier présent à la fermeture");
    let text = read(&dir.settings());
    assert!(text.contains("cle = \"importe\"") && text.contains(&format!("copie_depuis = \"{t1}\"")) && !leaks(&text, TOKEN_A), "{text}");
    // 2e lancement, 25 h plus tard : rouvert, fichier gardé jusqu'à READY.
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    clock_at(&mut app, t1 + DAY + 3600);
    b.http.on("GET", &messages_path(GENERAL), [Response::ok(fixture("rest_history")).dated(&http_date(t1 + DAY + 3600))]);
    let (mut conn, token) = b.session(&mut app, None);
    assert_eq!(token, TOKEN_A);
    idle(&mut app, 150);
    assert!(legacy.exists(), "2e lancement : fichier gardé tant que READY n'est pas arrivé");
    assert_eq!(app.step_two(), (true, false), "étape 2 programmée");
    send_ready(&b, &mut conn, ME, "Moi Fictif");
    discord_answers(&mut app);
    until(&mut app, "2e lancement : fichier supprimé après READY", |_| !legacy.exists());
    until(&mut app, "avertissement retiré", |a| a.legacy_notice().is_none());
    assert_eq!(vault.peek(&AccountKey::imported()), Some(Token::from(TOKEN_A)));
    assert_eq!(app.current_account(), Some(&AccountKey::imported()));
    until(&mut app, "instant de la copie oublié", |a| a.settings().vault_copy_since.is_none());
    drop(app);
    // 3e lancement : plus de fichier, plus d'avertissement.
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    let (_conn, token) = b.session(&mut app, Some((ME, "Moi Fictif")));
    assert_eq!(token, TOKEN_A);
    idle(&mut app, 100);
    assert!(app.legacy_notice().is_none() && !legacy.exists());
}

/// Coffre en panne au premier lancement : l'ancien fichier reste intact et
/// la session s'ouvre quand même avec son jeton.
#[test]
fn legacy_file_kept_when_vault_fails() {
    let mut b = Bench::new();
    let dir = TempDir::new("panne");
    let legacy = dir.0.join("token");
    std::fs::write(&legacy, TOKEN_A).unwrap_or_else(|e| panic!("{e}"));
    let vault = Arc::new(MemoryVault::new());
    vault.set_faults(ferrocord::account::Faults { unavailable: true, ..Default::default() });
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    let (_conn, token) = b.session(&mut app, Some((ME, "Moi Fictif")));
    assert_eq!(token, TOKEN_A);
    until(&mut app, "session ouverte", |a| me(a) == Some(ME));
    assert_eq!(read(&legacy), TOKEN_A, "fichier intact");
    assert!(app.settings().accounts.is_empty());
}

/// `DISCORD_TOKEN` est lu dans l'environnement, espaces retirés. Le jeton
/// factice n'est posé que dans un processus enfant (ce test relancé).
#[test]
fn env_token_is_read_from_environment() {
    const CHILD: &str = "FERROCORD_TEST_ENFANT";
    if std::env::var_os(CHILD).is_some() {
        assert_eq!(ferrocord::session::env_token(), Some(Ok(Token::from(TOKEN_ENV))));
        return;
    }
    let exe = std::env::current_exe().unwrap_or_else(|e| panic!("{e}"));
    let out = std::process::Command::new(exe)
        .args(["--exact", "accounts::env_token_is_read_from_environment", "--test-threads=1"])
        .env(CHILD, "1")
        .env("DISCORD_TOKEN", format!("  {TOKEN_ENV}\n"))
        .output()
        .unwrap_or_else(|e| panic!("{e}"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success() && stdout.contains("1 passed"), "{stdout}");
}

/// Au démarrage, le dernier compte est rouvert sur son dernier salon, et
/// seul l'historique de ce salon est demandé (pas celui du premier salon).
#[test]
fn last_channel_is_restored_at_startup() {
    let mut b = Bench::new();
    let dir = TempDir::new("restauration");
    let vault = Arc::new(MemoryVault::new());
    let mut s = stored(&vault, &[(ME, "Moi Fictif", TOKEN_A)]);
    s.last_account = Some(AccountKey::of_user(ME));
    s.accounts[0].place = Some(settings::Place { guild: Some(ALPHA), channel: Some(GENERAL) });
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));

    let mut app = b.app(vault, Some(&dir.settings()), None, None);
    let (_conn, token) = b.session(&mut app, Some((ME, "Moi Fictif")));
    assert_eq!(token, TOKEN_A);
    until(&mut app, "dernier salon rouvert", |a| a.store().channel == Some(GENERAL) && msg_ids(a, GENERAL).len() == 3);
    assert_eq!(app.store().guild, Some(ALPHA));
    let paths: Vec<String> = b.http.requests().into_iter().map(|r| r.path).collect();
    assert_eq!(paths, vec![messages_path(GENERAL) + "?limit=50"]);
}

/// Coffre dont les appels attendent un feu vert : rend observable ce qui se
/// passe pendant une écriture (ou un coffre qui ne répond pas).
struct Gate {
    inner: Arc<MemoryVault>,
    open: Mutex<bool>,
    changed: Condvar,
    /// Les lectures attendent aussi (coffre bloqué dès le démarrage).
    block_get: bool,
}

impl Gate {
    fn new(inner: Arc<MemoryVault>, block_get: bool) -> Arc<Self> {
        Arc::new(Self { inner, open: Mutex::new(false), changed: Condvar::new(), block_get })
    }
    fn release(&self) {
        *self.open.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.changed.notify_all();
    }
    fn wait(&self) {
        let mut open = self.open.lock().unwrap_or_else(|e| e.into_inner());
        while !*open {
            open = self.changed.wait(open).unwrap_or_else(|e| e.into_inner());
        }
    }
}

impl Vault for Gate {
    fn get(&self, k: &AccountKey) -> Result<Option<Token>, VaultError> {
        if self.block_get {
            self.wait();
        }
        self.inner.get(k)
    }
    fn set(&self, k: &AccountKey, t: &Token) -> Result<(), VaultError> {
        self.wait();
        self.inner.set(k, t)
    }
    fn delete(&self, k: &AccountKey) -> Result<(), VaultError> {
        self.inner.delete(k)
    }
    fn name(&self) -> Option<&'static str> {
        Some("coffre de test")
    }
}

/// Fait tourner l'application un moment (réponses tardives comprises).
fn idle(app: &mut App, ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        app.step();
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Jeton saisi et retenu : la liste est écrite avant le coffre. Pendant
/// l'écriture, le compte est déjà dans `settings.toml` ; une fois écrit, la
/// session devient celle du compte enregistré.
#[test]
fn typed_account_is_listed_before_the_vault_write() {
    let mut b = Bench::new();
    let dir = TempDir::new("ordre");
    let mem = Arc::new(MemoryVault::new());
    let gate = Gate::new(mem.clone(), false);
    let mut app = b.app(gate.clone(), Some(&dir.settings()), None, None);
    app.add_account(Token::from(TOKEN_A), true);
    let (_conn, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
    until(&mut app, "compte dans la liste", |a| a.settings().account(&AccountKey::of_user(ME)).is_some());
    let end = Instant::now() + WAIT;
    while !read(&dir.settings()).contains("Moi Fictif") {
        app.step();
        assert!(Instant::now() < end, "liste écrite sur le disque pendant que le coffre attend");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(mem.peek(&AccountKey::of_user(ME)), None, "coffre pas encore écrit");
    gate.release();
    until(&mut app, "compte enregistré", |a| a.current_account() == Some(&AccountKey::of_user(ME)));
    assert_eq!(mem.peek(&AccountKey::of_user(ME)), Some(Token::from(TOKEN_A)));
}

/// « Fermer la session » pendant l'écriture d'un jeton saisi : comme le dit
/// l'infobulle, rien n'est retenu (ni coffre, ni liste).
#[test]
fn closing_during_the_vault_write_keeps_nothing() {
    let mut b = Bench::new();
    let mem = Arc::new(MemoryVault::new());
    let gate = Gate::new(mem.clone(), false);
    let mut app = b.app(gate.clone(), None, None, None);
    app.add_account(Token::from(TOKEN_A), true);
    let (mut conn, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
    until(&mut app, "écriture commencée", |a| a.settings().account(&AccountKey::of_user(ME)).is_some());
    app.logout();
    b.rt.block_on(conn.expect_closed());
    gate.release();
    until(&mut app, "liste vidée", |a| a.settings().accounts.is_empty());
    let end = Instant::now() + WAIT;
    while mem.peek(&AccountKey::of_user(ME)).is_some() {
        app.step();
        assert!(Instant::now() < end, "écriture annulée dans le coffre");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(mem.is_empty() && app.current_account().is_none());
}

/// Liste pleine : le nouveau compte n'est pas retenu, et l'utilisateur le sait.
#[test]
fn full_account_list_is_never_exceeded_silently() {
    let mut b = Bench::new();
    let dir = TempDir::new("pleine");
    let vault = Arc::new(MemoryVault::new());
    let others: Vec<(u64, String, String)> = (1..=settings::MAX_ACCOUNTS as u64).map(|i| (i, format!("Compte {i}"), format!("faux.{i}"))).collect();
    let list: Vec<(u64, &str, &str)> = others.iter().map(|(i, n, t)| (*i, n.as_str(), t.as_str())).collect();
    let s = stored(&vault, &list);
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, None);
    app.add_account(Token::from(TOKEN_A), true);
    let (_conn, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
    until(&mut app, "limite signalée", |a| a.notice().is_some_and(|n| n.contains("16 comptes au plus")));
    idle(&mut app, 50);
    assert_eq!(vault.peek(&AccountKey::of_user(ME)), None);
    assert_eq!(app.settings().accounts.len(), settings::MAX_ACCOUNTS);
    assert_eq!(me(&app), Some(ME), "la session reste ouverte");
}

/// Jeton repris de l'ancien fichier à ce lancement, puis refusé par
/// Discord : rien n'est effacé, ni l'entrée du coffre, ni le fichier.
#[test]
fn refused_freshly_migrated_token_is_kept() {
    let mut b = Bench::new();
    let dir = TempDir::new("migre-refuse");
    let legacy = dir.0.join("token");
    std::fs::write(&legacy, format!("\u{feff}{TOKEN_A}\r\n")).unwrap_or_else(|e| panic!("{e}"));
    let vault = Arc::new(MemoryVault::new());
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    let (mut conn, token) = b.session(&mut app, None);
    assert_eq!(token, TOKEN_A, "marque d'ordre des octets retirée");
    b.rt.block_on(conn.close(4004));
    until(&mut app, "refus affiché", |a| a.login_error().is_some_and(|e| e.contains("Rien n'est effacé")));
    idle(&mut app, 50);
    assert_eq!(vault.peek(&AccountKey::imported()), Some(Token::from(TOKEN_A)), "jeton gardé dans le coffre");
    assert!(app.settings().account(&AccountKey::imported()).is_some(), "compte gardé dans la liste");
    assert_eq!(read(&legacy), format!("\u{feff}{TOKEN_A}\r\n"), "fichier gardé : l'étape 2 attend un READY");
}

/// Le coffre a déjà un jeton plus récent : le fichier n'est pas migré
/// par-dessus ; il reste signalé de façon durable, et « Supprimer le
/// fichier » l'efface sans toucher au coffre.
#[test]
fn newer_vault_token_wins_and_old_file_can_be_deleted() {
    let mut b = Bench::new();
    let dir = TempDir::new("plus-recent");
    let legacy = dir.0.join("token");
    std::fs::write(&legacy, TOKEN_B).unwrap_or_else(|e| panic!("{e}"));
    let vault = Arc::new(MemoryVault::with([(AccountKey::imported(), Token::from(TOKEN_A))]));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    let (_conn, token) = b.session(&mut app, Some((ME, "Moi Fictif")));
    assert_eq!(token, TOKEN_A, "le jeton du coffre sert");
    until(&mut app, "avertissement durable", |a| a.legacy_notice().is_some());
    assert!(!leaks(app.legacy_notice().unwrap_or_default(), TOKEN_B));
    idle(&mut app, 4500);
    assert!(app.legacy_notice().is_some(), "toujours affiché après le délai d'une notification");
    assert_eq!(read(&legacy), TOKEN_B, "fichier pas touché sans demande");
    // Le coffre a un autre jeton : la question le dit, le fichier est la seule copie du sien.
    app.ask_delete_legacy_file();
    until(&mut app, "question prête", |a| a.delete_legacy_question().is_some());
    let q = app.delete_legacy_question().unwrap_or_default();
    assert!(q.contains("ATTENTION : ce fichier est la seule copie de ce jeton"), "{q}");
    app.delete_legacy_file();
    until(&mut app, "fichier supprimé", |a| a.legacy_notice().is_none());
    assert!(!legacy.exists());
    assert_eq!(vault.peek(&AccountKey::imported()), Some(Token::from(TOKEN_A)), "coffre intact");
    assert_eq!(me(&app), Some(ME), "la session continue");
}

/// Étape 2 pendant qu'un autre programme tient le fichier ouvert en
/// écriture (et pourrait y écrire entre la vérification et l'effacement) :
/// l'ouverture exclusive échoue, rien n'est touché, c'est signalé ; un
/// nouvel essai a lieu au lancement suivant. Se déconnecter du compte
/// importé retire son entrée du coffre, jamais le fichier ; la question de
/// suppression dit alors que le fichier est la seule copie.
#[cfg(windows)]
#[test]
fn step_two_never_touches_a_file_open_for_writing() {
    use std::io::Write;
    use std::os::windows::fs::OpenOptionsExt;
    let mut b = Bench::new();
    let dir = TempDir::new("garde");
    let legacy = dir.0.join("token");
    std::fs::write(&legacy, TOKEN_A).unwrap_or_else(|e| panic!("{e}"));
    let vault = Arc::new(MemoryVault::new());
    // 1er lancement : copie dans le coffre.
    let t1 = now_secs();
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    clock_at(&mut app, t1);
    let (_conn, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
    until(&mut app, "identité", |a| a.settings().account(&AccountKey::imported()).is_some_and(|x| x.user_id == Some(ME)));
    idle(&mut app, 100);
    drop(app);
    // Un autre programme ouvre le fichier en écriture (partage complet : il ne gêne personne).
    let mut writer = std::fs::OpenOptions::new().read(true).append(true).share_mode(7).open(&legacy).unwrap_or_else(|e| panic!("{e}"));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    clock_at(&mut app, t1 + DAY + 3600);
    b.http.on("GET", &messages_path(GENERAL), [Response::ok(fixture("rest_history")).dated(&http_date(t1 + DAY + 3600))]);
    let (_conn, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
    discord_answers(&mut app);
    until(&mut app, "fichier gardé signalé", |a| a.legacy_notice().is_some_and(|n| n.contains("autre programme")));
    assert_eq!(read(&legacy), TOKEN_A, "rien n'est touché");
    // L'autre programme écrit après coup : sa donnée est intacte.
    writer.write_all(b"-suite").unwrap_or_else(|e| panic!("{e}"));
    drop(writer);
    assert_eq!(read(&legacy), format!("{TOKEN_A}-suite"));
    confirm_logout(&mut app);
    until(&mut app, "compte retiré", |a| a.settings().accounts.is_empty());
    idle(&mut app, 100);
    assert!(legacy.exists(), "fichier jamais supprimé par une déconnexion");
    assert_eq!(vault.peek(&AccountKey::imported()), None);
    assert!(app.legacy_notice().is_some_and(|n| n.contains("existe encore")), "{:?}", app.legacy_notice());
    // Seule la suppression demandée, confirmée après lecture du coffre, l'efface.
    app.ask_delete_legacy_file();
    until(&mut app, "question prête", |a| a.delete_legacy_question().is_some());
    let q = app.delete_legacy_question().unwrap_or_default();
    assert!(q.contains("ATTENTION") && !leaks(&q, TOKEN_A), "{q}");
    app.delete_legacy_file();
    until(&mut app, "fichier supprimé à la demande", |a| a.legacy_notice().is_none());
    assert!(!legacy.exists());
}

/// Coffre qui ne répond pas au démarrage : « Annuler » rend la main tout de
/// suite, et la réponse tardive n'ouvre rien.
#[test]
fn stuck_vault_can_be_cancelled() {
    let mut b = Bench::new();
    let dir = TempDir::new("bloque");
    let mem = Arc::new(MemoryVault::new());
    let mut s = stored(&mem, &[(ME, "Moi Fictif", TOKEN_A)]);
    s.last_account = Some(AccountKey::of_user(ME));
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let gate = Gate::new(mem, true);
    let mut app = b.app(gate.clone(), Some(&dir.settings()), None, None);
    idle(&mut app, 50);
    assert!(app.is_busy(), "ouverture des comptes en cours");
    app.cancel();
    app.step();
    assert!(!app.is_busy(), "retour immédiat à l'écran de connexion");
    gate.release();
    idle(&mut app, 100);
    let gw = &mut b.gw;
    let late = b.rt.block_on(async { tokio::time::timeout(Duration::from_millis(100), gw.accept()).await });
    assert!(late.is_err(), "aucune connexion ouverte après l'annulation");
    assert!(app.current_account().is_none() && !app.is_busy());
}

/// Envoie READY de `id` sur une connexion déjà identifiée.
fn send_ready(b: &Bench, conn: &mut GwConn, id: u64, name: &str) {
    let mut p = ready_with_resume(&b.gw, "ready_user_classic");
    p["d"]["user"]["id"] = json!(id.to_string());
    p["d"]["user"]["global_name"] = json!(name);
    b.rt.block_on(conn.send(p));
}

/// Fichier N, coffre T (plus récent), Discord refuse T : rien n'est
/// effacé, ni l'entrée du coffre, ni le fichier (chaîne de perte a), et
/// « Essayer le jeton du fichier » est proposé ; accepté, ce jeton est
/// enregistré sous son compte, le fichier n'étant toujours pas touché.
#[test]
fn refused_newer_vault_token_erases_nothing() {
    let mut b = Bench::new();
    let dir = TempDir::new("refus-recent");
    let legacy = dir.0.join("token");
    std::fs::write(&legacy, TOKEN_B).unwrap_or_else(|e| panic!("{e}"));
    let vault = Arc::new(MemoryVault::with([(AccountKey::imported(), Token::from(TOKEN_A))]));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    let (mut conn, token) = b.session(&mut app, None);
    assert_eq!(token, TOKEN_A);
    b.rt.block_on(conn.close(4004));
    until(&mut app, "refus affiché", |a| a.login_error().is_some());
    idle(&mut app, 150);
    assert_eq!(vault.peek(&AccountKey::imported()), Some(Token::from(TOKEN_A)), "entrée du coffre gardée");
    assert_eq!(read(&legacy), TOKEN_B, "fichier gardé");
    let err = app.login_error().unwrap_or_default().to_string();
    assert!(err.contains("Essayer le jeton du fichier") && !leaks(&err, TOKEN_B), "{err}");
    assert!(app.offers_file_token());
    // Essai du jeton du fichier : accepté, il est enregistré pour son compte.
    app.try_file_token();
    let (_conn, token) = b.session(&mut app, Some((OTHER, "Autre Fictif")));
    assert_eq!(token, TOKEN_B);
    until(&mut app, "jeton du fichier enregistré", |a| a.current_account() == Some(&AccountKey::of_user(OTHER)));
    assert_eq!(vault.peek(&AccountKey::of_user(OTHER)), Some(Token::from(TOKEN_B)));
    assert_eq!(vault.peek(&AccountKey::imported()), Some(Token::from(TOKEN_A)));
    assert_eq!(read(&legacy), TOKEN_B, "toujours pas touché");
}

/// Se déconnecter du compte importé retire son entrée du coffre, jamais
/// l'ancien fichier (qui reste signalé).
#[test]
fn logout_of_imported_account_keeps_the_file() {
    let mut b = Bench::new();
    let dir = TempDir::new("deconnexion-fichier");
    let legacy = dir.0.join("token");
    std::fs::write(&legacy, TOKEN_B).unwrap_or_else(|e| panic!("{e}"));
    let vault = Arc::new(MemoryVault::with([(AccountKey::imported(), Token::from(TOKEN_A))]));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    let (_conn, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
    until(&mut app, "session prête", |a| me(a) == Some(ME));
    // Le fichier contient un autre jeton : l'entrée est le seul exemplaire du sien.
    let q = confirm_logout(&mut app);
    assert!(q.contains("Se déconnecter") && q.contains("seul exemplaire enregistré") && q.contains("récupérer le jeton depuis Discord"), "{q}");
    until(&mut app, "compte retiré", |a| a.settings().accounts.is_empty());
    idle(&mut app, 150);
    assert_eq!(read(&legacy), TOKEN_B, "fichier jamais effacé par une déconnexion");
    assert_eq!(vault.peek(&AccountKey::imported()), None, "entrée du coffre retirée");
    assert!(app.legacy_notice().is_some_and(|n| n.contains("existe encore")), "{:?}", app.legacy_notice());
}

/// Le même compte enregistré deux fois (compte importé, puis saisi avec un
/// autre jeton) : rien n'est retiré tout seul. Le doublon est signalé ;
/// l'utilisateur en retire un, confirmé, et seule son entrée part.
#[test]
fn twin_is_signalled_never_erased() {
    let mut b = Bench::new();
    let dir = TempDir::new("doubles");
    let legacy = dir.0.join("token");
    std::fs::write(&legacy, TOKEN_A).unwrap_or_else(|e| panic!("{e}"));
    let vault = Arc::new(MemoryVault::with([(AccountKey::imported(), Token::from(TOKEN_A))]));
    let mut s = stored(&vault, &[(ME, "Moi Fictif", TOKEN_B)]);
    s.accounts.push(Account { user_id: Some(ME), name: Some("Moi Fictif".into()), ..Account::new(AccountKey::imported()) });
    s.last_account = Some(AccountKey::of_user(ME));
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    let (_conn, token) = b.session(&mut app, Some((ME, "Moi Fictif")));
    assert_eq!(token, TOKEN_B, "dernier compte rouvert");
    until(&mut app, "session prête", |a| me(a) == Some(ME));
    idle(&mut app, 300);
    assert_eq!(vault.peek(&AccountKey::imported()), Some(Token::from(TOKEN_A)), "doublon jamais retiré tout seul");
    assert_eq!(vault.peek(&AccountKey::of_user(ME)), Some(Token::from(TOKEN_B)));
    assert_eq!(app.settings().accounts.len(), 2);
    assert_eq!(read(&legacy), TOKEN_A, "fichier intact");
    assert_eq!(app.duplicate_account(), Some(AccountKey::imported()), "doublon signalé");
    // La question de suppression du fichier dit vrai : son jeton est aussi dans le coffre.
    app.ask_delete_legacy_file();
    until(&mut app, "question prête", |a| a.delete_legacy_question().is_some());
    let q = app.delete_legacy_question().unwrap_or_default();
    assert!(q.contains("aussi dans le coffre") && !q.contains("ATTENTION"), "{q}");
    app.cancel_confirmation();
    // Retrait confirmé de l'un des deux : seule son entrée part, le fichier reste.
    let q = confirm_remove(&mut app, &AccountKey::imported());
    assert!(q.contains("le fichier en clair, qui n'est pas touché, contient encore ce jeton"), "{q}");
    until(&mut app, "doublon retiré", |a| a.settings().account(&AccountKey::imported()).is_none());
    idle(&mut app, 100);
    assert_eq!(vault.peek(&AccountKey::imported()), None);
    assert_eq!(vault.peek(&AccountKey::of_user(ME)), Some(Token::from(TOKEN_B)), "l'autre ligne garde le compte");
    assert_eq!(read(&legacy), TOKEN_A, "fichier intact");
    assert!(app.duplicate_account().is_none());
}

/// Coffre dont les lectures attendent un feu vert une fois armé (étape 2 retenue).
struct HoldReads {
    inner: Arc<MemoryVault>,
    armed: std::sync::atomic::AtomicBool,
    gate: Mutex<bool>,
    changed: Condvar,
}

impl HoldReads {
    fn new(inner: Arc<MemoryVault>) -> Arc<Self> {
        Arc::new(Self { inner, armed: Default::default(), gate: Mutex::new(false), changed: Condvar::new() })
    }
    fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }
    fn release(&self) {
        *self.gate.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.changed.notify_all();
    }
}

impl Vault for HoldReads {
    fn get(&self, k: &AccountKey) -> Result<Option<Token>, VaultError> {
        if self.armed.load(Ordering::SeqCst) {
            let mut open = self.gate.lock().unwrap_or_else(|e| e.into_inner());
            while !*open {
                open = self.changed.wait(open).unwrap_or_else(|e| e.into_inner());
            }
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
        Some("coffre retenu")
    }
}

/// Fichier et coffre au même jeton, comme après un 1er lancement.
fn migrated(dir: &TempDir) -> (PathBuf, Arc<MemoryVault>) {
    let legacy = dir.0.join("token");
    std::fs::write(&legacy, TOKEN_A).unwrap_or_else(|e| panic!("{e}"));
    let vault = Arc::new(MemoryVault::with([(AccountKey::imported(), Token::from(TOKEN_A))]));
    let mut s = Settings::default();
    s.accounts.push(Account::new(AccountKey::imported()));
    s.last_account = Some(AccountKey::imported());
    // Copie faite il y a deux jours : l'étape 2 peut avoir lieu.
    s.vault_copy_since = Some(now_secs() - 2 * DAY);
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    (legacy, vault)
}

/// B1 : horloge de la machine en avance au lancement qui suit la copie.
/// Localement, la copie a deux jours ; pour Discord (en-tête `Date`), 18 h
/// seulement : l'étape 2 reste programmée, le fichier n'est pas touché.
#[test]
fn step_two_waits_for_discord_time() {
    let mut b = Bench::new();
    let dir = TempDir::new("horloge-avance");
    let (legacy, vault) = migrated(&dir);
    let since = now_secs() - 2 * DAY;
    b.http.on("GET", &messages_path(GENERAL), [Response::ok(fixture("rest_history")).dated(&http_date(since + 18 * 3600))]);
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    let (_c, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
    discord_answers(&mut app);
    let history = messages_path(GENERAL);
    until(&mut app, "historique reçu", |a| !msg_ids(a, GENERAL).is_empty());
    assert!(b.http.requests().iter().any(|r| r.path.starts_with(&history)));
    idle(&mut app, 300);
    assert_eq!(app.step_two(), (true, false), "programmée, en attente de l'heure de Discord");
    assert_eq!(read(&legacy), TOKEN_A, "fichier intact");
}

/// Retrait du compte importé pendant que l'étape 2 tourne : refusé, avec
/// l'explication ; l'étape 2 va au bout, le compte reste.
#[test]
fn removal_during_step_two_is_refused() {
    let mut b = Bench::new();
    let dir = TempDir::new("retrait-etape2");
    let (legacy, mem) = migrated(&dir);
    let vault = HoldReads::new(mem.clone());
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    let (mut conn, _) = b.session(&mut app, None);
    idle(&mut app, 100);
    // Question lue avant READY, « Oui » pendant l'étape 2.
    let q = ask_remove(&mut app, &AccountKey::imported());
    assert!(q.contains("contient encore ce jeton"), "{q}");
    vault.arm();
    send_ready(&b, &mut conn, ME, "Moi Fictif");
    discord_answers(&mut app);
    until(&mut app, "étape 2 en cours", |a| a.step_two().1);
    assert!(!app.remove_account(AccountKey::imported()), "retrait refusé pendant l'étape 2");
    assert!(app.notice().is_some_and(|n| n.contains("en train d'être supprimé")), "{:?}", app.notice());
    vault.release();
    until(&mut app, "étape 2 finie", |a| !a.step_two().1);
    assert!(!legacy.exists());
    assert_eq!(mem.peek(&AccountKey::imported()), Some(Token::from(TOKEN_A)), "le compte garde son jeton");
    assert!(app.settings().account(&AccountKey::imported()).is_some());
}

/// Retrait du compte importé pendant que l'étape 2 est programmée (avant
/// READY) : elle est annulée, le fichier reste, comme le dit la question.
/// Le refus est retenu (taille et date du fichier, pas son contenu) : au
/// lancement suivant, le fichier n'est pas importé de nouveau, seulement
/// signalé avec « Importer » et « Supprimer le fichier ».
#[test]
fn removal_while_step_two_is_scheduled_keeps_the_file_and_is_remembered() {
    let mut b = Bench::new();
    let dir = TempDir::new("retrait-programme");
    let (legacy, vault) = migrated(&dir);
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    let (_conn, token) = b.session(&mut app, None);
    assert_eq!(token, TOKEN_A);
    idle(&mut app, 100);
    assert_eq!(app.step_two(), (true, false), "étape 2 programmée");
    let q = confirm_remove(&mut app, &AccountKey::imported());
    assert!(q.contains("le fichier en clair, qui n'est pas touché, contient encore ce jeton"), "{q}");
    until(&mut app, "compte retiré", |a| a.settings().accounts.is_empty());
    assert_eq!(app.step_two(), (false, false), "étape 2 annulée");
    until(&mut app, "refus retenu", |a| a.settings().declined_import.is_some());
    idle(&mut app, 100);
    assert_eq!(read(&legacy), TOKEN_A, "le fichier reste");
    assert_eq!(vault.peek(&AccountKey::imported()), None);
    assert!(app.offers_import());
    drop(app);
    let text = read(&dir.settings());
    assert!(text.contains("[ancien_fichier_refuse]") && !leaks(&text, TOKEN_A), "{text}");
    // Lancement suivant : pas réimporté, seulement signalé.
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    until(&mut app, "avertissement", |a| a.legacy_notice().is_some());
    idle(&mut app, 150);
    assert_eq!(vault.peek(&AccountKey::imported()), None, "pas réimporté");
    assert!(app.settings().accounts.is_empty() && app.current_account().is_none());
    assert!(app.offers_import() && app.legacy_notice().is_some_and(|n| n.contains("existe encore")));
    // « Importer » : copié dans le coffre, session ouverte, fichier gardé (lancement suivant).
    app.import_legacy_file();
    let (_conn, token) = b.session(&mut app, Some((ME, "Moi Fictif")));
    assert_eq!(token, TOKEN_A);
    until(&mut app, "importé", |a| a.current_account() == Some(&AccountKey::imported()));
    idle(&mut app, 100);
    assert_eq!(vault.peek(&AccountKey::imported()), Some(Token::from(TOKEN_A)));
    assert!(legacy.exists() && app.settings().declined_import.is_none());
}

/// Discord refuse la session pendant l'écriture d'un jeton validé : rien
/// n'est annulé (pas d'`Undo`), l'écriture va au bout.
#[test]
fn refusal_during_a_write_undoes_nothing() {
    let mut b = Bench::new();
    let mem = Arc::new(MemoryVault::new());
    let gate = Gate::new(mem.clone(), false);
    let mut app = b.app(gate.clone(), None, None, None);
    app.add_account(Token::from(TOKEN_A), true);
    let (mut conn, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
    until(&mut app, "écriture commencée", |a| a.settings().account(&AccountKey::of_user(ME)).is_some());
    b.rt.block_on(conn.close(4004));
    until(&mut app, "refus affiché", |a| a.login_error().is_some());
    gate.release();
    idle(&mut app, 300);
    assert_eq!(mem.peek(&AccountKey::of_user(ME)), Some(Token::from(TOKEN_A)), "écriture gardée");
    assert!(app.settings().account(&AccountKey::of_user(ME)).is_some(), "compte gardé dans la liste");
}

/// Coffre dont la lecture d'une clé échoue (délai dépassé).
struct FailKey(Arc<MemoryVault>, AccountKey);

impl Vault for FailKey {
    fn get(&self, k: &AccountKey) -> Result<Option<Token>, VaultError> {
        if k == &self.1 {
            return Err(VaultError::Unavailable("délai dépassé (test)".into()));
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
        Some("coffre lent")
    }
}

/// Lecture du dernier compte impossible alors que la migration vient
/// d'écrire : le jeton du fichier reste proposé pour la session.
#[test]
fn unreadable_last_account_still_offers_the_file_token() {
    let mut b = Bench::new();
    let dir = TempDir::new("dernier-illisible");
    let legacy = dir.0.join("token");
    std::fs::write(&legacy, TOKEN_A).unwrap_or_else(|e| panic!("{e}"));
    let mem = Arc::new(MemoryVault::new());
    let mut s = stored(&mem, &[(OTHER, "Autre Fictif", TOKEN_B)]);
    s.last_account = Some(AccountKey::of_user(OTHER));
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let vault = Arc::new(FailKey(mem.clone(), AccountKey::of_user(OTHER)));
    let mut app = b.app(vault, Some(&dir.settings()), None, Some(legacy.clone()));
    until(&mut app, "erreur affichée", |a| a.login_error().is_some());
    assert_eq!(mem.peek(&AccountKey::imported()), Some(Token::from(TOKEN_A)), "migration écrite");
    app.try_file_token();
    let (_conn, token) = b.session(&mut app, Some((ME, "Moi Fictif")));
    assert_eq!(token, TOKEN_A, "jeton du fichier proposé pour la session");
    until(&mut app, "session prête", |a| me(a) == Some(ME));
    assert_eq!(read(&legacy), TOKEN_A);
}

/// `DISCORD_TOKEN` abîmé : signalé, et le compte enregistré se rouvre quand même.
#[test]
fn damaged_env_token_does_not_block_the_saved_account() {
    let mut b = Bench::new();
    let dir = TempDir::new("env-abime");
    let vault = Arc::new(MemoryVault::new());
    let mut s = stored(&vault, &[(ME, "Moi Fictif", TOKEN_A)]);
    s.last_account = Some(AccountKey::of_user(ME));
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let mut app = b.app_env(vault, Some(&dir.settings()), Some(Err(TokenError::Invalid("le jeton contient des espaces"))), None);
    let (_conn, token) = b.session(&mut app, Some((ME, "Moi Fictif")));
    assert_eq!(token, TOKEN_A, "compte enregistré rouvert");
    until(&mut app, "session prête", |a| a.current_account() == Some(&AccountKey::of_user(ME)));
    assert!(app.env_warning().is_some_and(|w| w.contains("DISCORD_TOKEN")));
}

/// Même chose pour le compte importé (rouvert sans liste, d'après la migration).
#[test]
fn damaged_env_token_does_not_block_the_imported_account() {
    let mut b = Bench::new();
    let dir = TempDir::new("env-abime-importe");
    let (legacy, vault) = migrated(&dir);
    let mut app = b.app_env(vault, None, Some(Err(TokenError::Invalid("espaces"))), Some(legacy));
    let (_conn, token) = b.session(&mut app, Some((ME, "Moi Fictif")));
    assert_eq!(token, TOKEN_A, "compte importé rouvert");
    until(&mut app, "session prête", |a| a.current_account() == Some(&AccountKey::imported()));
    assert!(app.env_warning().is_some() && app.login_error().is_none());
}

/// Coffre qui abîme les écritures tant qu'on le lui demande.
struct Corrupting(Arc<MemoryVault>, std::sync::atomic::AtomicBool);

impl Vault for Corrupting {
    fn get(&self, k: &AccountKey) -> Result<Option<Token>, VaultError> {
        self.0.get(k)
    }
    fn set(&self, k: &AccountKey, t: &Token) -> Result<(), VaultError> {
        if self.1.load(Ordering::SeqCst) {
            return self.0.set(k, &Token::from(format!("{}-abime", t.expose())));
        }
        self.0.set(k, t)
    }
    fn delete(&self, k: &AccountKey) -> Result<(), VaultError> {
        self.0.delete(k)
    }
    fn name(&self) -> Option<&'static str> {
        Some("coffre abîmé")
    }
}

/// Écriture ratée et jeton d'avant impossible à remettre : il est gardé en
/// mémoire et « Recopier le jeton d'avant » le réécrit.
#[test]
fn previous_token_can_be_recopied() {
    let mut b = Bench::new();
    let dir = TempDir::new("recopie");
    let mem = Arc::new(MemoryVault::new());
    let s = stored(&mem, &[(ME, "Moi Fictif", TOKEN_A)]);
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let vault = Arc::new(Corrupting(mem.clone(), std::sync::atomic::AtomicBool::new(true)));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, None);
    until(&mut app, "écran de connexion", |a| !a.is_busy());
    app.add_account(Token::from(TOKEN_B), true);
    let (_conn, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
    // Compte déjà enregistré avec un autre jeton : remplacé seulement sur « Oui ».
    until(&mut app, "question de remplacement", |a| a.replace_token_question().is_some());
    app.answer_replace_token(true);
    until(&mut app, "recopie proposée", |a| a.offers_recopy());
    let n = app.notice().unwrap_or_default().to_string();
    assert!(n.contains("Recopier le jeton d'avant") && !leaks(&n, TOKEN_A) && !leaks(&n, TOKEN_B), "{n}");
    assert_ne!(mem.peek(&AccountKey::of_user(ME)), Some(Token::from(TOKEN_A)), "entrée abîmée");
    vault.1.store(false, Ordering::SeqCst);
    app.recopy_previous_token();
    until(&mut app, "jeton d'avant recopié", |a| !a.offers_recopy());
    assert_eq!(mem.peek(&AccountKey::of_user(ME)), Some(Token::from(TOKEN_A)));
}

/// Coffre qui abîme toute écriture et refuse les suppressions.
struct BrokenUndo(Arc<MemoryVault>);

impl Vault for BrokenUndo {
    fn get(&self, k: &AccountKey) -> Result<Option<Token>, VaultError> {
        self.0.get(k)
    }
    fn set(&self, k: &AccountKey, t: &Token) -> Result<(), VaultError> {
        self.0.set(k, &Token::from(format!("{}-abime", t.expose())))
    }
    fn delete(&self, _: &AccountKey) -> Result<(), VaultError> {
        Err(VaultError::Failed("suppression refusée (test)".into()))
    }
    fn name(&self) -> Option<&'static str> {
        Some("coffre abîmé")
    }
}

/// Écriture refusée à la relecture, puis retrait impossible : l'utilisateur
/// sait quelle entrée est peut-être abîmée, et le compte reste dans la liste
/// (l'opération n'est pas présentée comme proprement annulée).
#[test]
fn failed_put_back_is_reported() {
    let mut b = Bench::new();
    let mem = Arc::new(MemoryVault::new());
    let mut app = b.app(Arc::new(BrokenUndo(mem.clone())), None, None, None);
    app.add_account(Token::from(TOKEN_A), true);
    let (_conn, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
    until(&mut app, "échec signalé", |a| a.notice().is_some_and(|n| n.contains("enregistré")));
    let n = app.notice().unwrap_or_default().to_string();
    assert!(n.contains("entrée") && n.contains("peut-être"), "{n}");
    assert!(!leaks(&n, TOKEN_A), "{n}");
    assert!(app.settings().account(&AccountKey::of_user(ME)).is_some(), "compte gardé dans la liste");
}

/// Deux enregistrements successifs du même compte (le premier annulé) : la
/// réponse au premier ne retire pas le compte pendant que le second écrit.
#[test]
fn second_save_of_the_same_account_is_kept() {
    let mut b = Bench::new();
    let mem = Arc::new(MemoryVault::new());
    let gate = Gate::new(mem.clone(), false);
    let mut app = b.app(gate.clone(), None, None, None);
    app.add_account(Token::from(TOKEN_A), true);
    let (mut conn, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
    until(&mut app, "première écriture commencée", |a| a.settings().account(&AccountKey::of_user(ME)).is_some());
    app.logout();
    b.rt.block_on(conn.expect_closed());
    app.add_account(Token::from(TOKEN_A), true);
    let (_conn2, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
    until(&mut app, "deuxième session prête", |a| me(a) == Some(ME));
    idle(&mut app, 50);
    gate.release();
    until(&mut app, "compte enregistré", |a| a.current_account() == Some(&AccountKey::of_user(ME)));
    idle(&mut app, 200);
    assert!(app.settings().account(&AccountKey::of_user(ME)).is_some(), "compte dans la liste");
    assert_eq!(mem.peek(&AccountKey::of_user(ME)), Some(Token::from(TOKEN_A)), "jeton dans le coffre");
}

/// Enregistrement annulé, puis l'application fermée avant la réponse :
/// l'annulation part quand même (le coffre ne garde pas le jeton).
#[test]
fn cancelled_save_is_undone_even_if_the_app_closes() {
    let mut b = Bench::new();
    let mem = Arc::new(MemoryVault::new());
    let gate = Gate::new(mem.clone(), false);
    let mut app = b.app(gate.clone(), None, None, None);
    app.add_account(Token::from(TOKEN_A), true);
    let (mut conn, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
    until(&mut app, "écriture commencée", |a| a.settings().account(&AccountKey::of_user(ME)).is_some());
    app.logout();
    b.rt.block_on(conn.expect_closed());
    gate.release();
    drop(app);
    assert_eq!(mem.peek(&AccountKey::of_user(ME)), None, "écriture annulée malgré la fermeture");
}

/// Liste pleine : le jeton de l'ancien fichier n'est pas écrit dans le
/// coffre (il y serait une entrée invisible) ; le fichier sert pour la session.
#[test]
fn full_list_blocks_the_migration_write() {
    let mut b = Bench::new();
    let dir = TempDir::new("pleine-migration");
    let legacy = dir.0.join("token");
    std::fs::write(&legacy, TOKEN_A).unwrap_or_else(|e| panic!("{e}"));
    let vault = Arc::new(MemoryVault::new());
    let others: Vec<(u64, String, String)> = (1..=settings::MAX_ACCOUNTS as u64).map(|i| (i, format!("Compte {i}"), format!("faux.{i}"))).collect();
    let list: Vec<(u64, &str, &str)> = others.iter().map(|(i, n, t)| (*i, n.as_str(), t.as_str())).collect();
    let s = stored(&vault, &list);
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    let (_conn, token) = b.session(&mut app, Some((ME, "Moi Fictif")));
    assert_eq!(token, TOKEN_A, "le fichier sert pour la session");
    until(&mut app, "session prête", |a| me(a) == Some(ME));
    idle(&mut app, 150);
    assert_eq!(vault.peek(&AccountKey::imported()), None, "rien d'écrit hors de la liste");
    assert_eq!(app.settings().accounts.len(), settings::MAX_ACCOUNTS);
    assert_eq!(read(&legacy), TOKEN_A);
}

/// Un message de A resté non envoyé : au changement de compte, la liste des
/// messages non envoyés est vidée, et il ne peut pas être réessayé avec le
/// jeton de B (aucun envoi ne part avec ce jeton).
#[test]
fn unsent_messages_of_a_are_never_retried_as_b() {
    let mut b = Bench::new();
    let vault = Arc::new(MemoryVault::new());
    let mut app = b.app(vault, None, None, None);
    app.add_account(Token::from(TOKEN_A), false);
    let (_conn_a, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
    until(&mut app, "A prêt", |a| me(a) == Some(ME));
    app.open(Some(ALPHA), GENERAL);
    let _ = b.request();
    until(&mut app, "historique de A", |a| msg_ids(a, GENERAL).len() == 3);
    b.http.on("POST", &messages_path(GENERAL), [Response::error(500, "Erreur interne fictive")]);
    assert!(app.send(GENERAL, "brouillon de A"));
    until(&mut app, "message de A non envoyé", |a| a.store().unsent.contains_key(&GENERAL));
    let nonce = app.store().unsent[&GENERAL][0].nonce;

    app.add_account(Token::from(TOKEN_B), false);
    assert!(app.store().unsent.is_empty(), "liste des messages non envoyés vidée au changement de compte");
    let (_conn_b, token) = b.session(&mut app, Some((OTHER, "Autre Fictif")));
    assert_eq!(token, TOKEN_B);
    until(&mut app, "B prêt", |a| me(a) == Some(OTHER));
    app.open(Some(ALPHA), GENERAL);
    until(&mut app, "historique de B", |a| msg_ids(a, GENERAL).len() == 3);
    assert!(!app.retry_unsent(GENERAL, nonce), "le message de A n'existe pas dans la session de B");
    idle(&mut app, 150);
    let posts: Vec<Option<String>> = b.http.requests().into_iter().filter(|r| r.method == "POST").map(|r| r.authorization).collect();
    assert_eq!(posts, vec![Some(TOKEN_A.to_string())], "un seul envoi, avec le jeton de A");
    assert!(app.store().unsent.is_empty());
}

/// Un événement envoyé sur la connexion de A juste avant le passage à B
/// (pas encore lu par l'interface) n'est jamais appliqué à B.
#[test]
fn late_event_of_previous_account_is_dropped() {
    let mut b = Bench::new();
    let vault = Arc::new(MemoryVault::new());
    let mut app = b.app(vault.clone(), None, None, None);
    app.add_account(Token::from(TOKEN_A), false);
    let (mut conn_a, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
    until(&mut app, "A prêt", |a| me(a) == Some(ME));
    app.open(Some(ALPHA), GENERAL);
    let _ = b.request();
    until(&mut app, "historique de A", |a| msg_ids(a, GENERAL).len() == 3);
    // Le message part sur la connexion de A, l'interface ne tourne pas.
    b.rt.block_on(conn_a.dispatch_fixture("message_create_simple", 2));
    std::thread::sleep(Duration::from_millis(100));
    app.add_account(Token::from(TOKEN_B), false);
    let (_conn_b, token) = b.session(&mut app, Some((OTHER, "Autre Fictif")));
    assert_eq!(token, TOKEN_B);
    until(&mut app, "B prêt", |a| me(a) == Some(OTHER));
    app.open(Some(ALPHA), GENERAL);
    let _ = b.request();
    until(&mut app, "historique de B", |a| msg_ids(a, GENERAL).len() == 3);
    idle(&mut app, 50);
    assert!(!msg_ids(&app, GENERAL).contains(&A_ONLY), "le message de A n'arrive pas chez B");
}

/// Date affichée pour l'instant `secs` (heure locale), comme l'avertissement.
fn shown_date(secs: u64) -> String {
    use chrono::TimeZone;
    chrono::Local.timestamp_opt(secs as i64, 0).single().map(|d| d.format("%d/%m/%Y à %H:%M").to_string()).unwrap_or_default()
}

/// D1 — étape 2 arrêtée entre les zéros et la suppression (un autre
/// programme tient le fichier ouvert en lecture sans partager la
/// suppression) : résultat distinct « vidé, pas supprimé ». Le fichier vidé
/// n'est jamais présenté comme une copie du jeton (question de retrait :
/// seul exemplaire) ; au lancement suivant, il est supprimé comme vestige.
#[cfg(windows)]
#[test]
fn emptied_file_is_never_presented_as_a_copy() {
    use std::os::windows::fs::OpenOptionsExt;
    let mut b = Bench::new();
    let dir = TempDir::new("vide");
    let (legacy, vault) = migrated(&dir);
    // Lecteur qui partage lecture et écriture, pas la suppression.
    let reader = std::fs::OpenOptions::new().read(true).share_mode(0x1 | 0x2).open(&legacy).unwrap_or_else(|e| panic!("{e}"));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    let (_conn, token) = b.session(&mut app, Some((ME, "Moi Fictif")));
    assert_eq!(token, TOKEN_A);
    discord_answers(&mut app);
    until(&mut app, "étape 2 finie", |a| a.legacy_notice().is_some_and(|n| n.contains("vidé")));
    let n = app.legacy_notice().unwrap_or_default().to_string();
    assert!(n.contains("ne contient plus le jeton") && !n.contains("sera supprimé") && !leaks(&n, TOKEN_A), "{n}");
    assert_eq!(std::fs::read(&legacy).unwrap_or_default(), vec![0u8; TOKEN_A.len()], "zéros sur le disque");
    assert_eq!(vault.peek(&AccountKey::imported()), Some(Token::from(TOKEN_A)));
    assert!(!app.offers_import());
    let q = ask_remove(&mut app, &AccountKey::imported());
    assert!(q.contains("seul exemplaire enregistré") && !q.contains("contient encore"), "{q}");
    app.cancel_confirmation();
    // « Supprimer le fichier » sur le vestige : la question dit qu'il est vide ;
    // suppression encore refusée après les zéros : signalé comme vidé.
    let q = ask_wipe(&mut app);
    assert!(q.contains("aucun jeton") && !q.contains("ATTENTION"), "{q}");
    app.delete_legacy_file();
    until(&mut app, "vestige toujours là", |a| a.legacy_notice().is_some_and(|n| n.contains("fichier vidé")));
    drop(reader);
    drop(app);
    // Lancement suivant : vestige sans jeton, supprimé ; rien n'est signalé.
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    let (_conn, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
    until(&mut app, "vestige supprimé", |_| !legacy.exists());
    idle(&mut app, 100);
    assert!(app.legacy_notice().is_none(), "{:?}", app.legacy_notice());
    assert_eq!(vault.peek(&AccountKey::imported()), Some(Token::from(TOKEN_A)));
}

/// D2 et D5 — contenu illisible : « contenu inconnu, peut-être votre seul
/// exemplaire », jamais « vide » ; la question de retrait dit que la copie
/// restante est inconnue. Puis suppression confirmée sur un état périmé
/// (l'autre instance a retiré l'entrée du coffre) : rien n'est supprimé, la
/// question est reposée (« seule copie »).
#[test]
fn unknown_content_and_stale_wipe_are_asked_again() {
    let mut b = Bench::new();
    let dir = TempDir::new("inconnu");
    let (legacy, vault) = migrated(&dir);
    std::fs::write(&legacy, [0xff, 0xfe, 0x41, 0x00]).unwrap_or_else(|e| panic!("{e}"));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    let (_conn, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
    until(&mut app, "session prête", |a| me(a) == Some(ME));
    idle(&mut app, 100);
    let q = ask_wipe(&mut app);
    assert!(q.contains("contenu inconnu") && q.contains("peut-être votre seul exemplaire") && !q.contains("vide"), "{q}");
    app.cancel_confirmation();
    let q = ask_remove(&mut app, &AccountKey::imported());
    assert!(q.contains("Copie restante inconnue") && q.contains("fichier en clair n'a pas pu être lu"), "{q}");
    app.cancel_confirmation();
    // Fichier au même jeton que le coffre : « aussi dans le coffre ».
    std::fs::write(&legacy, TOKEN_A).unwrap_or_else(|e| panic!("{e}"));
    let q = ask_wipe(&mut app);
    assert!(q.contains("aussi dans le coffre"), "{q}");
    // L'autre instance retire le compte importé entre la question et « Oui ».
    vault.delete(&AccountKey::imported()).unwrap_or_else(|e| panic!("{e}"));
    app.delete_legacy_file();
    until(&mut app, "question reposée", |a| a.delete_legacy_question().is_some_and(|q| q.contains("seule copie")));
    assert!(app.notice().is_some_and(|n| n.contains("n'a pas été supprimé")), "{:?}", app.notice());
    assert_eq!(read(&legacy), TOKEN_A, "rien n'est touché");
    app.delete_legacy_file();
    until(&mut app, "fichier supprimé", |_| !legacy.exists());
}

/// D3 — deux instances : la première copie le jeton dans le coffre ; la
/// deuxième, lancée une minute après, voit « même jeton » mais ne supprime
/// rien (moins de 24 h) et annonce la date ; 25 h après, l'étape 2 a lieu.
/// Instant absent ou dans le futur : retenu maintenant, 24 h d'attente.
#[test]
fn step_two_waits_a_day_after_the_copy() {
    let mut b = Bench::new();
    let dir = TempDir::new("delai");
    let legacy = dir.0.join("token");
    std::fs::write(&legacy, TOKEN_A).unwrap_or_else(|e| panic!("{e}"));
    let vault = Arc::new(MemoryVault::new());
    let t1 = now_secs();
    let mut first = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    clock_at(&mut first, t1);
    let (_c1, _) = b.session(&mut first, Some((ME, "Moi Fictif")));
    until(&mut first, "instant retenu sur le disque", |_| read(&dir.settings()).contains(&format!("copie_depuis = \"{t1}\"")));
    // Deuxième instance, une minute plus tard (la première tourne encore).
    let mut second = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    clock_at(&mut second, t1 + 60);
    let (_c2, token) = b.session(&mut second, Some((ME, "Moi Fictif")));
    assert_eq!(token, TOKEN_A);
    until(&mut second, "session prête", |a| me(a) == Some(ME));
    idle(&mut second, 200);
    assert_eq!(second.step_two(), (false, false), "pas d'étape 2 moins de 24 h après la copie");
    assert!(legacy.exists());
    let n = second.legacy_notice().unwrap_or_default().to_string();
    assert!(n.contains(&format!("au plus tôt le {}", shown_date(t1 + DAY))), "{n}");
    drop(second);
    drop(first);
    // 25 h plus tard : étape 2 après READY.
    let mut later = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    clock_at(&mut later, t1 + DAY + 3600);
    // Discord aussi est 25 h plus tard (l'heure de la machine seule ne suffit pas, B1).
    b.http.on("GET", &messages_path(GENERAL), [Response::ok(fixture("rest_history")).dated(&http_date(t1 + DAY + 3600))]);
    let (_c3, _) = b.session(&mut later, Some((ME, "Moi Fictif")));
    discord_answers(&mut later);
    until(&mut later, "fichier supprimé après 24 h", |_| !legacy.exists());
    drop(later);
    // Instant absent (settings.toml perdu) puis dans le futur : retenu maintenant, rien supprimé.
    for (name, since) in [("absent", None), ("futur", Some(t1 + 100 * DAY))] {
        let (legacy, vault) = migrated(&dir);
        let mut s = settings::parse(&read(&dir.settings())).unwrap_or_else(|e| panic!("{e:?}"));
        s.vault_copy_since = since;
        std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
        let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
        clock_at(&mut app, t1);
        let (_c, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
        until(&mut app, "session prête", |a| me(a) == Some(ME));
        idle(&mut app, 200);
        assert_eq!(app.step_two(), (false, false), "{name}");
        assert!(legacy.exists(), "{name}");
        assert_eq!(app.settings().vault_copy_since, Some(t1), "{name} : instant retenu maintenant");
        let n = app.legacy_notice().unwrap_or_default().to_string();
        assert!(n.contains(&format!("au plus tôt le {}", shown_date(t1 + DAY))), "{name} : {n}");
    }
}

/// D4 et D5 — confirmations périmées, deux instances. L'autre instance fait
/// l'étape 2 entre la question (« le fichier contient encore ce jeton ») et
/// « Oui » : rien n'est retiré, la question est reposée (« seul
/// exemplaire »). Puis l'autre instance retire elle-même l'entrée : la
/// question reposée le dit (« n'a plus de jeton »).
#[test]
fn stale_removal_is_asked_again() {
    let mut b = Bench::new();
    let dir = TempDir::new("perime");
    let (legacy, vault) = migrated(&dir);
    let mut s = settings::parse(&read(&dir.settings())).unwrap_or_else(|e| panic!("{e:?}"));
    s.vault_copy_since = Some(now_secs());
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    let (_conn, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
    until(&mut app, "session prête", |a| me(a) == Some(ME));
    idle(&mut app, 100);
    let q = ask_remove(&mut app, &AccountKey::imported());
    assert!(q.contains("le fichier en clair, qui n'est pas touché, contient encore ce jeton"), "{q}");
    // L'autre instance : étape 2 (vrai code), le fichier disparaît.
    let other = ferrocord::account::InstanceLock::new(Some(dir.0.join("comptes.lock")), Duration::from_secs(5));
    assert_eq!(ferrocord::account::migrate::finish(&legacy, &*vault, &other, &Token::from(TOKEN_A)), ferrocord::account::Finish::Wiped);
    assert!(app.remove_account(AccountKey::imported()));
    until(&mut app, "question reposée", |a| a.remove_account_question(&AccountKey::imported()).is_some_and(|q| q.contains("seul exemplaire")));
    assert!(app.notice().is_some_and(|n| n.contains("n'a pas été retiré")), "{:?}", app.notice());
    assert_eq!(vault.peek(&AccountKey::imported()), Some(Token::from(TOKEN_A)), "rien n'est retiré");
    assert!(app.settings().account(&AccountKey::imported()).is_some());
    // Une deuxième instance (vraie application) retire elle-même le compte.
    let mut s2 = Settings::default();
    s2.accounts.push(Account::new(AccountKey::imported()));
    let dir2 = TempDir::new("perime-autre");
    std::fs::write(dir2.settings(), settings::export(&s2)).unwrap_or_else(|e| panic!("{e}"));
    let mut second = b.app(vault.clone(), Some(&dir2.settings()), None, None);
    idle(&mut second, 100);
    let q2 = confirm_remove(&mut second, &AccountKey::imported());
    assert!(q2.contains("seul exemplaire"), "{q2}");
    until(&mut second, "retiré là-bas", |a| a.settings().accounts.is_empty());
    assert_eq!(vault.peek(&AccountKey::imported()), None);
    // Ici, « Oui » sur « seul exemplaire » : l'état a encore changé, question reposée.
    assert!(app.remove_account(AccountKey::imported()));
    until(&mut app, "question reposée", |a| a.remove_account_question(&AccountKey::imported()).is_some_and(|q| q.contains("n'a plus de jeton")));
    assert!(app.remove_account(AccountKey::imported()));
    until(&mut app, "retiré de la liste", |a| a.settings().accounts.is_empty());
}

/// D5 — coffre illisible au moment de la question : la copie restante est
/// dite inconnue (jamais « une copie reste »).
#[test]
fn removal_question_says_when_the_copy_is_unknown() {
    let b = Bench::new();
    let dir = TempDir::new("coffre-illisible");
    let vault = Arc::new(MemoryVault::new());
    let s = stored(&vault, &[(ME, "Moi Fictif", TOKEN_A)]);
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, None);
    idle(&mut app, 100);
    vault.set_faults(ferrocord::account::Faults { fail_get: true, ..Default::default() });
    let q = ask_remove(&mut app, &AccountKey::of_user(ME));
    assert!(q.contains("Copie restante inconnue") && q.contains("le coffre n'a pas pu être lu"), "{q}");
    vault.set_faults(Default::default());
    // Coffre relisible : la question change, l'état annoncé n'est plus le même.
    assert!(app.remove_account(AccountKey::of_user(ME)));
    until(&mut app, "question reposée", |a| a.remove_account_question(&AccountKey::of_user(ME)).is_some_and(|q| q.contains("seul exemplaire enregistré")));
    assert_eq!(vault.peek(&AccountKey::of_user(ME)), Some(Token::from(TOKEN_A)), "rien n'est retiré sur une question périmée");
}

/// Saisit `token` (« Se souvenir de moi ») ; Discord répond READY de ME.
fn type_token(b: &mut Bench, app: &mut App, token: &str) -> GwConn {
    app.add_account(Token::from(token), true);
    let (conn, sent) = b.session(app, Some((ME, "Moi Fictif")));
    assert_eq!(sent, token);
    until(app, "session prête", |a| me(a) == Some(ME));
    conn
}

const REPLACE_Q: &str = "Ce compte est déjà enregistré. Remplacer le jeton enregistré par celui que vous venez de saisir ? L'ancien sera effacé du coffre.";

/// BLOQUANT — jeton saisi d'un compte déjà enregistré avec un autre jeton
/// (compte saisi, pas importé) : rien n'est écrit ; la session continue ; la
/// question est posée. Sans réponse ou sur « Non », le coffre garde l'ancien ;
/// sur « Oui », il est remplacé, seulement si l'entrée n'a pas changé
/// entre-temps (sinon la question est reposée). Même jeton : aucune question.
#[test]
fn different_token_of_a_saved_account_is_replaced_only_on_yes() {
    let mut b = Bench::new();
    let dir = TempDir::new("remplacer");
    let vault = Arc::new(MemoryVault::new());
    let s = stored(&vault, &[(ME, "Moi Fictif", TOKEN_A)]);
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let key = AccountKey::of_user(ME);
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, None);
    // Sans réponse (session fermée) : rien n'est écrit.
    let _c = type_token(&mut b, &mut app, TOKEN_B);
    until(&mut app, "question posée", |a| a.replace_token_question().is_some());
    assert_eq!(app.replace_token_question().as_deref(), Some(REPLACE_Q));
    assert_eq!(vault.peek(&key), Some(Token::from(TOKEN_A)), "rien n'est écrit avant la réponse");
    assert_eq!(me(&app), Some(ME), "la session continue avec le jeton saisi");
    app.logout();
    idle(&mut app, 100);
    assert_eq!(app.replace_token_question(), None);
    assert_eq!(vault.peek(&key), Some(Token::from(TOKEN_A)), "sans réponse : l'ancien reste");
    // « Non ».
    let _c = type_token(&mut b, &mut app, TOKEN_B);
    until(&mut app, "question posée", |a| a.replace_token_question().is_some());
    app.answer_replace_token(false);
    idle(&mut app, 100);
    assert_eq!(app.replace_token_question(), None);
    assert_eq!(vault.peek(&key), Some(Token::from(TOKEN_A)), "« Non » : l'ancien reste");
    // « Oui » après que l'autre instance a changé l'entrée : question reposée, rien d'écrit.
    let _c = type_token(&mut b, &mut app, TOKEN_B);
    until(&mut app, "question posée", |a| a.replace_token_question().is_some());
    vault.set(&key, &Token::from(TOKEN_ENV)).unwrap_or_else(|e| panic!("{e}"));
    app.answer_replace_token(true);
    until(&mut app, "question reposée", |a| a.replace_token_question().is_some());
    assert!(app.notice().is_some_and(|n| n.contains("a changé entre-temps")), "{:?}", app.notice());
    assert_eq!(vault.peek(&key), Some(Token::from(TOKEN_ENV)), "état changé : rien n'est remplacé");
    // « Oui » sur la question reposée : remplacé.
    app.answer_replace_token(true);
    until(&mut app, "jeton remplacé", |a| a.current_account() == Some(&AccountKey::of_user(ME)));
    assert_eq!(vault.peek(&key), Some(Token::from(TOKEN_B)), "« Oui » : remplacé");
    assert_eq!(app.replace_token_question(), None);
    // Même jeton que le coffre : rien à écrire, aucune question.
    let calls = vault.calls();
    let _c = type_token(&mut b, &mut app, TOKEN_B);
    until(&mut app, "compte rouvert", |a| a.current_account() == Some(&AccountKey::of_user(ME)));
    idle(&mut app, 100);
    assert_eq!(app.replace_token_question(), None, "même jeton : aucune question");
    assert_eq!(vault.peek(&key), Some(Token::from(TOKEN_B)));
    assert!(vault.calls() > calls, "le coffre a été relu");
}

/// BLOQUANT, compte importé — même règle ; « Oui » écrit dans l'entrée du
/// compte importé, et (2c) cette écriture est une copie neuve : l'instant de
/// la copie repart de maintenant.
#[test]
fn different_token_of_the_imported_account_is_replaced_only_on_yes() {
    let mut b = Bench::new();
    let dir = TempDir::new("remplacer-importe");
    let (legacy, vault) = migrated(&dir);
    let t = now_secs();
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    clock_at(&mut app, t);
    let (_c, token) = b.session(&mut app, Some((ME, "Moi Fictif")));
    assert_eq!(token, TOKEN_A);
    until(&mut app, "session prête", |a| me(a) == Some(ME));
    idle(&mut app, 100);
    let _c = type_token(&mut b, &mut app, TOKEN_B);
    until(&mut app, "question posée", |a| a.replace_token_question().is_some());
    assert_eq!(app.replace_token_question().as_deref(), Some(REPLACE_Q));
    assert_eq!(vault.peek(&AccountKey::imported()), Some(Token::from(TOKEN_A)));
    app.answer_replace_token(false);
    idle(&mut app, 100);
    assert_eq!(vault.peek(&AccountKey::imported()), Some(Token::from(TOKEN_A)), "« Non » : l'ancien reste");
    let _c = type_token(&mut b, &mut app, TOKEN_B);
    until(&mut app, "question posée", |a| a.replace_token_question().is_some());
    let before = app.settings().vault_copy_since;
    app.answer_replace_token(true);
    until(&mut app, "jeton remplacé", |_| vault.peek(&AccountKey::imported()) == Some(Token::from(TOKEN_B)));
    until(&mut app, "copie neuve retenue", |a| a.settings().vault_copy_since == Some(t));
    assert_ne!(before, Some(t), "l'instant d'avant était plus ancien");
}

/// 2a — « Supprimer le fichier » moins de 24 h après la copie : la question
/// le dit (coupure de courant) et donne la date ; possible après « Oui ».
#[test]
fn early_manual_wipe_warns_about_power_loss() {
    let mut b = Bench::new();
    let dir = TempDir::new("tot");
    let (legacy, vault) = migrated(&dir);
    let t = now_secs();
    let since = t - 3600;
    let mut s = settings::parse(&read(&dir.settings())).unwrap_or_else(|e| panic!("{e:?}"));
    s.vault_copy_since = Some(since);
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    clock_at(&mut app, t);
    let (_c, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
    until(&mut app, "session prête", |a| me(a) == Some(ME));
    idle(&mut app, 100);
    let q = ask_wipe(&mut app);
    let warn = format!(
        "la copie dans le coffre date de moins de 24 h ; en cas de coupure de courant elle pourrait ne pas être conservée. Il est plus sûr d'attendre le {}.",
        shown_date(since + DAY)
    );
    assert!(q.contains("aussi dans le coffre") && q.to_lowercase().contains(&warn.to_lowercase()), "{q}");
    app.delete_legacy_file();
    until(&mut app, "fichier supprimé après confirmation", |_| !legacy.exists());
    assert_eq!(vault.peek(&AccountKey::imported()), Some(Token::from(TOKEN_A)));
}

/// Un lancement à l'heure `secs` avec READY ; rend l'application (session prête, calme).
fn launch_at(b: &mut Bench, dir: &TempDir, vault: &Arc<MemoryVault>, legacy: &Path, secs: u64) -> (App, GwConn) {
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.to_path_buf()));
    clock_at(&mut app, secs);
    let (c, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
    until(&mut app, "session prête", |a| me(a) == Some(ME));
    idle(&mut app, 200);
    (app, c)
}

/// 2b — horloge juste, puis en retard (instant de la copie dans le futur de
/// moins de 48 h : gardé), puis corrigée 5 h après la copie : aucune étape 2.
/// Puis horloge en avance au moment de la copie, corrigée ensuite : aucune
/// étape 2 non plus.
#[test]
fn wrong_clock_never_brings_step_two_forward() {
    let mut b = Bench::new();
    let t = now_secs();
    for (name, clocks) in [("retard", [t, t - 20 * 3600, t + 5 * 3600]), ("avance", [t + 30 * 3600, t + 3600, t + 10 * 3600])] {
        let dir = TempDir::new(name);
        let legacy = dir.0.join("token");
        std::fs::write(&legacy, TOKEN_A).unwrap_or_else(|e| panic!("{e}"));
        let vault = Arc::new(MemoryVault::new());
        // 1er lancement : copie dans le coffre, instant retenu sur le disque.
        let (app, _c) = launch_at(&mut b, &dir, &vault, &legacy, clocks[0]);
        until_disk(&dir, &format!("copie_depuis = \"{}\"", clocks[0]));
        drop(app);
        for (i, secs) in clocks.into_iter().enumerate().skip(1) {
            let (app, _c) = launch_at(&mut b, &dir, &vault, &legacy, secs);
            assert_eq!(app.step_two(), (false, false), "{name}, lancement {} : pas d'étape 2", i + 1);
            assert!(legacy.exists(), "{name}, lancement {}", i + 1);
            assert_eq!(app.settings().vault_copy_since, Some(clocks[0]), "{name}, lancement {} : instant gardé", i + 1);
            drop(app);
        }
    }
}

/// Attend que `settings.toml` contienne `what`.
fn until_disk(dir: &TempDir, what: &str) {
    let end = Instant::now() + WAIT;
    while !read(&dir.settings()).contains(what) {
        assert!(Instant::now() < end, "délai dépassé : {what} sur le disque");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 2d — `settings.toml` relu (réécrit au dehors avec un instant plus ancien) :
/// l'instant de la copie ne recule pas ; le plus récent est réécrit.
#[test]
fn reloaded_settings_never_move_the_copy_instant_back() {
    let b = Bench::new();
    let dir = TempDir::new("recul");
    let (legacy, vault) = migrated(&dir);
    let since = now_secs() - 3600;
    let mut s = settings::parse(&read(&dir.settings())).unwrap_or_else(|e| panic!("{e:?}"));
    s.vault_copy_since = Some(since);
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    idle(&mut app, 200);
    assert_eq!(app.settings().vault_copy_since, Some(since));
    // Autre programme (ou fichier restauré) : instant reculé de trois jours.
    s.vault_copy_since = Some(since - 3 * DAY);
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let end = Instant::now() + WAIT;
    while !read(&dir.settings()).contains(&format!("copie_depuis = \"{since}\"")) {
        app.step();
        assert_eq!(app.settings().vault_copy_since, Some(since), "l'instant ne recule jamais");
        assert!(Instant::now() < end, "délai dépassé : instant récent réécrit");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(app.settings().vault_copy_since, Some(since));
}

/// 2c — toute écriture dans l'entrée du compte importé (ici un jeton saisi
/// pour ce compte, entrée vide) est une copie neuve : l'instant de la copie
/// repart de maintenant, et l'étape 2 attend de nouveau 24 h.
#[test]
fn any_write_of_the_imported_entry_restarts_the_delay() {
    let mut b = Bench::new();
    let dir = TempDir::new("copie-neuve");
    let vault = Arc::new(MemoryVault::new());
    let t = now_secs();
    let mut s = Settings::default();
    s.accounts.push(Account { user_id: Some(ME), name: Some("Moi Fictif".into()), ..Account::new(AccountKey::imported()) });
    s.vault_copy_since = Some(t - 2 * DAY);
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, None);
    clock_at(&mut app, t);
    let _c = type_token(&mut b, &mut app, TOKEN_B);
    until(&mut app, "jeton écrit dans l'entrée du compte importé", |_| vault.peek(&AccountKey::imported()) == Some(Token::from(TOKEN_B)));
    idle(&mut app, 100);
    assert_eq!(app.settings().vault_copy_since, Some(t), "instant de la copie remis à maintenant");
    until_disk(&dir, &format!("copie_depuis = \"{t}\""));
}

const FILE_Q: &str = "Discord a refusé le jeton enregistré et accepté celui du fichier en clair. Remplacer le jeton enregistré par celui du fichier ? L'ancien sera effacé du coffre.";

/// Compte importé connu (identité ME), coffre V = TOKEN_A, fichier F = TOKEN_B ;
/// Discord refuse V. Rend l'application sur l'écran de connexion.
fn refused_vault_token(b: &mut Bench, dir: &TempDir) -> (App, Arc<MemoryVault>, PathBuf) {
    let legacy = dir.0.join("token");
    std::fs::write(&legacy, TOKEN_B).unwrap_or_else(|e| panic!("{e}"));
    let vault = Arc::new(MemoryVault::with([(AccountKey::imported(), Token::from(TOKEN_A))]));
    let mut s = Settings::default();
    s.accounts.push(Account { user_id: Some(ME), name: Some("Moi Fictif".into()), ..Account::new(AccountKey::imported()) });
    s.last_account = Some(AccountKey::imported());
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    let (mut conn, token) = b.session(&mut app, None);
    assert_eq!(token, TOKEN_A);
    b.rt.block_on(conn.close(4004));
    until(&mut app, "refus affiché", |a| a.login_error().is_some() && a.offers_file_token());
    (app, vault, legacy)
}

/// « Essayer le jeton du fichier » puis READY avec F.
fn try_file(b: &mut Bench, app: &mut App) -> GwConn {
    app.try_file_token();
    let (conn, token) = b.session(app, Some((ME, "Moi Fictif")));
    assert_eq!(token, TOKEN_B);
    until(app, "session prête", |a| me(a) == Some(ME));
    conn
}

/// BLOQUANT 1 — jeton du coffre refusé, jeton du fichier accepté : rien
/// n'est écrit, la question le dit exactement. Sans réponse ou « Non » : le
/// coffre garde V. « Oui » sur un état changé par l'autre instance : rien,
/// question reposée (sans plus prétendre que Discord a refusé ce jeton-là) ;
/// « Oui » ensuite : remplacé.
#[test]
fn file_token_after_refusal_replaces_only_on_yes() {
    let mut b = Bench::new();
    let dir = TempDir::new("essai-fichier");
    let (mut app, vault, legacy) = refused_vault_token(&mut b, &dir);
    let imported = AccountKey::imported();
    // Sans réponse (session fermée).
    let _c = try_file(&mut b, &mut app);
    until(&mut app, "question posée", |a| a.replace_token_question().is_some());
    assert_eq!(app.replace_token_question().as_deref(), Some(FILE_Q));
    assert_eq!(vault.peek(&imported), Some(Token::from(TOKEN_A)), "rien n'est écrit avant la réponse");
    app.logout();
    idle(&mut app, 100);
    assert_eq!(app.replace_token_question(), None);
    assert_eq!(vault.peek(&imported), Some(Token::from(TOKEN_A)), "sans réponse : V reste");
    // « Non ».
    assert!(app.offers_file_token());
    let _c = try_file(&mut b, &mut app);
    until(&mut app, "question posée", |a| a.replace_token_question().is_some());
    app.answer_replace_token(false);
    idle(&mut app, 100);
    assert_eq!(vault.peek(&imported), Some(Token::from(TOKEN_A)), "« Non » : V reste");
    // « Oui » après un changement par l'autre instance : reposée.
    let _c = try_file(&mut b, &mut app);
    until(&mut app, "question posée", |a| a.replace_token_question().as_deref() == Some(FILE_Q));
    vault.set(&imported, &Token::from(TOKEN_ENV)).unwrap_or_else(|e| panic!("{e}"));
    app.answer_replace_token(true);
    until(&mut app, "question reposée", |a| a.replace_token_question().is_some());
    let q = app.replace_token_question().unwrap_or_default();
    assert!(q.contains("du fichier de jeton en clair") && !q.contains("Discord a refusé"), "{q}");
    assert_eq!(vault.peek(&imported), Some(Token::from(TOKEN_ENV)), "état changé : rien n'est remplacé");
    app.answer_replace_token(true);
    until(&mut app, "remplacé", |_| vault.peek(&imported) == Some(Token::from(TOKEN_B)));
    assert_eq!(read(&legacy), TOKEN_B, "fichier pas touché");
}

/// BLOQUANT 2 — la question dit d'où vient le jeton : saisi, ou fichier en
/// clair. `DISCORD_TOKEN` n'est jamais enregistré : jamais de question.
#[test]
fn replace_question_names_where_the_token_comes_from() {
    let mut b = Bench::new();
    // Fichier en clair.
    let dir = TempDir::new("provenance-fichier");
    let (mut app, _vault, _legacy) = refused_vault_token(&mut b, &dir);
    let _c = try_file(&mut b, &mut app);
    until(&mut app, "question posée", |a| a.replace_token_question().is_some());
    let q = app.replace_token_question().unwrap_or_default();
    assert!(q.contains("du fichier") && !q.contains("venez de saisir"), "{q}");
    drop(app);
    // Saisi.
    let dir = TempDir::new("provenance-saisi");
    let vault = Arc::new(MemoryVault::new());
    let s = stored(&vault, &[(ME, "Moi Fictif", TOKEN_A)]);
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, None);
    let _c = type_token(&mut b, &mut app, TOKEN_B);
    until(&mut app, "question posée", |a| a.replace_token_question().is_some());
    let q = app.replace_token_question().unwrap_or_default();
    assert!(q.contains("venez de saisir") && !q.contains("fichier"), "{q}");
    drop(app);
    // DISCORD_TOKEN du même compte, autre jeton : rien n'est écrit, aucune question.
    let dir = TempDir::new("provenance-env");
    let vault = Arc::new(MemoryVault::new());
    let s = stored(&vault, &[(ME, "Moi Fictif", TOKEN_A)]);
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), Some(Token::from(TOKEN_ENV)), None);
    let (_c, token) = b.session(&mut app, Some((ME, "Moi Fictif")));
    assert_eq!(token, TOKEN_ENV);
    until(&mut app, "session prête", |a| me(a) == Some(ME));
    idle(&mut app, 200);
    assert_eq!(app.replace_token_question(), None, "DISCORD_TOKEN : jamais de question");
    assert_eq!(vault.peek(&AccountKey::of_user(ME)), Some(Token::from(TOKEN_A)));
}

/// 4 — retirer le compte importé efface l'instant de sa copie dans
/// `settings.toml` (une copie future repart de zéro).
#[test]
fn removing_the_imported_account_forgets_the_copy_instant() {
    let mut b = Bench::new();
    let dir = TempDir::new("oubli-instant");
    let (legacy, vault) = migrated(&dir);
    let mut s = settings::parse(&read(&dir.settings())).unwrap_or_else(|e| panic!("{e:?}"));
    s.vault_copy_since = Some(now_secs() - 3600);
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    assert!(read(&dir.settings()).contains("copie_depuis"));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    let (_c, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
    until(&mut app, "session prête", |a| me(a) == Some(ME));
    idle(&mut app, 100);
    confirm_remove(&mut app, &AccountKey::imported());
    until(&mut app, "compte retiré", |a| a.settings().account(&AccountKey::imported()).is_none());
    assert_eq!(app.settings().vault_copy_since, None);
    let end = Instant::now() + WAIT;
    while read(&dir.settings()).contains("copie_depuis") {
        app.step();
        assert!(Instant::now() < end, "délai dépassé : instant effacé sur le disque");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(legacy.exists(), "le fichier n'est pas touché");
}

/// 6 — l'avertissement « copie de moins de 24 h » vaut pour toute entrée
/// qui contient le jeton du fichier (pas seulement le compte importé), et
/// il est donné quand on ne sait pas quand cette copie a été écrite.
#[test]
fn recent_copy_warning_covers_every_entry() {
    let mut b = Bench::new();
    for (name, written) in [("inconnu", None), ("recent", Some(now_secs() - 3600))] {
        let dir = TempDir::new(name);
        let legacy = dir.0.join("token");
        std::fs::write(&legacy, TOKEN_A).unwrap_or_else(|e| panic!("{e}"));
        let vault = Arc::new(MemoryVault::new());
        let mut s = stored(&vault, &[(ME, "Moi Fictif", TOKEN_A)]);
        s.last_account = Some(AccountKey::of_user(ME));
        // Import refusé auparavant : rien n'est écrit sous le compte importé.
        s.declined_import = ferrocord::account::migrate::file_mark(&legacy);
        if let Some(at) = written {
            s.written.push((AccountKey::of_user(ME), at));
        }
        std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
        let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
        let (_c, _) = b.session(&mut app, Some((ME, "Moi Fictif")));
        until(&mut app, "session prête", |a| me(a) == Some(ME));
        idle(&mut app, 100);
        let q = ask_wipe(&mut app);
        assert!(q.contains("aussi dans le coffre"), "{name} : {q}");
        match written {
            None => assert!(q.contains("On ne sait pas quand la copie dans le coffre a été écrite"), "{name} : {q}"),
            Some(at) => assert!(q.contains(&format!("Il est plus sûr d'attendre le {}.", shown_date(at + DAY))), "{name} : {q}"),
        }
        assert_eq!(vault.peek(&AccountKey::imported()), None);
    }
}

/// 7 — « Recopier le jeton d'avant » quand l'autre instance a changé
/// l'entrée depuis l'échec : rien n'est écrit, la question est posée ;
/// « Oui » : recopié.
#[test]
fn recopy_never_overwrites_a_changed_entry() {
    let mut b = Bench::new();
    let dir = TempDir::new("recopie-changee");
    let mem = Arc::new(MemoryVault::new());
    let s = stored(&mem, &[(ME, "Moi Fictif", TOKEN_A)]);
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let vault = Arc::new(Corrupting(mem.clone(), std::sync::atomic::AtomicBool::new(true)));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, None);
    until(&mut app, "écran de connexion", |a| !a.is_busy());
    let _c = type_token(&mut b, &mut app, TOKEN_B);
    until(&mut app, "question de remplacement", |a| a.replace_token_question().is_some());
    app.answer_replace_token(true);
    until(&mut app, "recopie proposée", |a| a.offers_recopy());
    vault.1.store(false, Ordering::SeqCst);
    // L'autre instance écrit un autre jeton dans l'entrée.
    mem.set(&AccountKey::of_user(ME), &Token::from(TOKEN_ENV)).unwrap_or_else(|e| panic!("{e}"));
    app.recopy_previous_token();
    until(&mut app, "question posée", |a| a.replace_token_question().is_some());
    let q = app.replace_token_question().unwrap_or_default();
    assert!(q.contains("a changé depuis l'échec de l'écriture") && q.contains("jeton d'avant"), "{q}");
    assert_eq!(mem.peek(&AccountKey::of_user(ME)), Some(Token::from(TOKEN_ENV)), "rien n'est recopié sans accord");
    app.answer_replace_token(true);
    until(&mut app, "jeton d'avant recopié", |a| !a.offers_recopy());
    assert_eq!(mem.peek(&AccountKey::of_user(ME)), Some(Token::from(TOKEN_A)));
}

/// 8b — compte ajouté pour l'occasion (absent de la liste, entrée déjà
/// présente dans le coffre) : après « Oui », il est dans la liste.
#[test]
fn account_added_for_the_occasion_stays_listed_after_yes() {
    let mut b = Bench::new();
    let dir = TempDir::new("ajoute");
    let vault = Arc::new(MemoryVault::with([(AccountKey::of_user(ME), Token::from(TOKEN_A))]));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, None);
    let _c = type_token(&mut b, &mut app, TOKEN_B);
    until(&mut app, "question posée", |a| a.replace_token_question().is_some());
    app.answer_replace_token(true);
    until(&mut app, "remplacé", |_| vault.peek(&AccountKey::of_user(ME)) == Some(Token::from(TOKEN_B)));
    idle(&mut app, 100);
    assert!(app.settings().account(&AccountKey::of_user(ME)).is_some(), "le compte est dans la liste");
    assert_eq!(app.current_account(), Some(&AccountKey::of_user(ME)));
}

/// Liste de `settings.toml` lue sur le disque contient ce compte.
fn listed_on_disk(dir: &TempDir, key: &AccountKey) -> bool {
    settings::parse(&read(&dir.settings())).is_ok_and(|s| s.account(key).is_some())
}

/// Ce qu'écrit une autre instance ouverte en même temps, dont la liste en
/// mémoire ne connaît pas le compte ajouté ici : un seul compte, OTHER.
fn other_window_list() -> String {
    let mut s = Settings::default();
    s.accounts.push(Account { user_id: Some(OTHER), name: Some("Autre Fictif".into()), ..Account::new(AccountKey::of_user(OTHER)) });
    settings::export(&s)
}

/// A1 — deux fenêtres : l'autre réécrit `settings.toml` avec sa liste
/// périmée (sans le compte qu'on vient d'ajouter), pendant l'écriture du
/// coffre puis après. Le compte n'est jamais perdu : son entrée du coffre
/// le remet dans la liste, en mémoire et sur le disque.
#[test]
fn account_added_in_another_window_is_never_lost() {
    let mut b = Bench::new();
    let dir = TempDir::new("deux-fenetres");
    let mem = Arc::new(MemoryVault::new());
    let gate = Gate::new(mem.clone(), false);
    let mut app = b.app(gate.clone(), Some(&dir.settings()), None, None);
    let key = AccountKey::of_user(ME);
    let _c = type_token(&mut b, &mut app, TOKEN_A);
    until_disk(&dir, "Moi Fictif");
    // Pendant l'écriture du coffre (retenue) : liste réécrite par l'autre fenêtre.
    std::fs::write(dir.settings(), other_window_list()).unwrap_or_else(|e| panic!("{e}"));
    until(&mut app, "liste de l'autre fenêtre relue", |a| a.settings().account(&AccountKey::of_user(OTHER)).is_some());
    gate.release();
    until(&mut app, "compte enregistré", |a| a.current_account() == Some(&key));
    assert_eq!(mem.peek(&key), Some(Token::from(TOKEN_A)));
    until(&mut app, "compte remis dans la liste", |a| a.settings().account(&key).is_some());
    let end = Instant::now() + WAIT;
    while !(listed_on_disk(&dir, &key) && listed_on_disk(&dir, &AccountKey::of_user(OTHER))) {
        app.step();
        assert!(Instant::now() < end, "délai dépassé : les deux comptes sur le disque");
        std::thread::sleep(Duration::from_millis(5));
    }
    // Après l'écriture : l'autre fenêtre réécrit encore sa liste périmée.
    idle(&mut app, 100);
    std::fs::write(dir.settings(), other_window_list()).unwrap_or_else(|e| panic!("{e}"));
    // Seule l'application peut remettre le compte sur le disque : après relecture et recherche dans le coffre.
    let end = Instant::now() + WAIT;
    while !listed_on_disk(&dir, &key) {
        app.step();
        assert!(Instant::now() < end, "délai dépassé : compte retrouvé dans le coffre et réécrit sur le disque");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(app.settings().account(&key).is_some_and(|x| x.user_id == Some(ME) && x.name.as_deref() == Some("Moi Fictif")));
}

/// A1 — au démarrage, une entrée du coffre dont l'écriture est retenue
/// (`[ecritures]`) mais absente de la liste (liste réécrite par une autre
/// instance) est cherchée dans le coffre et remise dans la liste ; une
/// entrée retenue mais absente du coffre ne l'est pas.
#[test]
fn startup_finds_a_written_entry_missing_from_the_list() {
    let b = Bench::new();
    let dir = TempDir::new("demarrage-ecritures");
    let vault = Arc::new(MemoryVault::with([(AccountKey::of_user(ME), Token::from(TOKEN_A))]));
    let mut s = Settings::default();
    s.written.push((AccountKey::of_user(ME), now_secs()));
    s.written.push((AccountKey::of_user(OTHER), now_secs()));
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, None);
    until(&mut app, "compte remis dans la liste", |a| a.settings().account(&AccountKey::of_user(ME)).is_some());
    idle(&mut app, 150);
    assert!(app.settings().account(&AccountKey::of_user(OTHER)).is_none(), "absent du coffre : pas listé");
    assert_eq!(app.settings().account(&AccountKey::of_user(ME)).and_then(|a| a.user_id), Some(ME));
}

/// Coffre dont les suppressions attendent un feu vert.
struct HoldDelete {
    inner: Arc<MemoryVault>,
    open: Mutex<bool>,
    changed: Condvar,
}

impl HoldDelete {
    fn release(&self) {
        *self.open.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.changed.notify_all();
    }
}

impl Vault for HoldDelete {
    fn get(&self, k: &AccountKey) -> Result<Option<Token>, VaultError> {
        self.inner.get(k)
    }
    fn set(&self, k: &AccountKey, t: &Token) -> Result<(), VaultError> {
        self.inner.set(k, t)
    }
    fn delete(&self, k: &AccountKey) -> Result<(), VaultError> {
        let mut open = self.open.lock().unwrap_or_else(|e| e.into_inner());
        while !*open {
            open = self.changed.wait(open).unwrap_or_else(|e| e.into_inner());
        }
        self.inner.delete(k)
    }
    fn name(&self) -> Option<&'static str> {
        Some("coffre retenu")
    }
}

/// A2 — retirer un compte puis l'ajouter aussitôt de nouveau (avant la fin
/// du retrait) : le retrait se fait, puis l'écriture ; le compte reste
/// dans la liste, qui ne perd jamais une entrée du coffre.
#[test]
fn removed_then_re_added_account_stays_listed() {
    let mut b = Bench::new();
    let dir = TempDir::new("retrait-ajout");
    let mem = Arc::new(MemoryVault::new());
    let s = stored(&mem, &[(ME, "Moi Fictif", TOKEN_A)]);
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let vault = Arc::new(HoldDelete { inner: mem.clone(), open: Mutex::new(false), changed: Condvar::new() });
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, None);
    let key = AccountKey::of_user(ME);
    until(&mut app, "écran de connexion", |a| !a.is_busy());
    confirm_remove(&mut app, &key);
    // Retrait retenu dans le coffre ; le compte est saisi de nouveau.
    let _c = type_token(&mut b, &mut app, TOKEN_A);
    idle(&mut app, 50);
    vault.release();
    until(&mut app, "compte enregistré de nouveau", |a| a.current_account() == Some(&key));
    idle(&mut app, 200);
    assert_eq!(mem.peek(&key), Some(Token::from(TOKEN_A)), "jeton dans le coffre");
    assert!(app.settings().account(&key).is_some(), "compte dans la liste");
    let end = Instant::now() + WAIT;
    while !listed_on_disk(&dir, &key) {
        app.step();
        assert!(Instant::now() < end, "délai dépassé : compte sur le disque");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A3 — remplacement confirmé (« Oui »), puis changement de compte pendant
/// l'écriture : l'écriture confirmée va au bout (jamais défaite en silence).
#[test]
fn confirmed_replacement_survives_a_switch_of_account() {
    let mut b = Bench::new();
    let dir = TempDir::new("remplacement-changement");
    let mem = Arc::new(MemoryVault::new());
    let s = stored(&mem, &[(ME, "Moi Fictif", TOKEN_A), (OTHER, "Autre Fictif", TOKEN_B)]);
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let gate = Gate::new(mem.clone(), false);
    let mut app = b.app(gate.clone(), Some(&dir.settings()), None, None);
    until(&mut app, "écran de connexion", |a| !a.is_busy());
    let key = AccountKey::of_user(ME);
    let _c = type_token(&mut b, &mut app, TOKEN_ENV);
    until(&mut app, "question posée", |a| a.replace_token_question().is_some());
    app.answer_replace_token(true);
    idle(&mut app, 50);
    assert_eq!(mem.peek(&key), Some(Token::from(TOKEN_A)), "écriture retenue");
    app.switch_account(AccountKey::of_user(OTHER));
    idle(&mut app, 50);
    gate.release();
    let end = Instant::now() + WAIT;
    while mem.peek(&key) != Some(Token::from(TOKEN_ENV)) {
        app.step();
        assert!(Instant::now() < end, "délai dépassé : remplacement confirmé écrit");
        std::thread::sleep(Duration::from_millis(5));
    }
    idle(&mut app, 200);
    assert_eq!(mem.peek(&key), Some(Token::from(TOKEN_ENV)), "jamais défait après coup");
    assert!(app.settings().account(&key).is_some());
}

/// B2 — étape 2 programmée, puis une écriture dans l'entrée du compte
/// importé (remplacement confirmé) : copie neuve, l'étape 2 est
/// déprogrammée et l'avertissement donne la nouvelle date.
#[test]
fn rewrite_of_the_imported_entry_unschedules_step_two() {
    let mut b = Bench::new();
    let dir = TempDir::new("reecriture-importe");
    let (legacy, vault) = migrated(&dir);
    let mut s = settings::parse(&read(&dir.settings())).unwrap_or_else(|e| panic!("{e:?}"));
    s.accounts[0].user_id = Some(ME);
    s.accounts[0].name = Some("Moi Fictif".into());
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let t = now_secs();
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, Some(legacy.clone()));
    clock_at(&mut app, t);
    let (_c, token) = b.session(&mut app, None);
    assert_eq!(token, TOKEN_A);
    idle(&mut app, 100);
    assert_eq!(app.step_two(), (true, false), "étape 2 programmée");
    let _c = type_token(&mut b, &mut app, TOKEN_B);
    until(&mut app, "question posée", |a| a.replace_token_question().is_some());
    app.answer_replace_token(true);
    until(&mut app, "entrée réécrite", |_| vault.peek(&AccountKey::imported()) == Some(Token::from(TOKEN_B)));
    until(&mut app, "copie neuve retenue", |a| a.settings().vault_copy_since == Some(t));
    assert_eq!(app.step_two(), (false, false), "étape 2 déprogrammée");
    let notice = app.legacy_notice().unwrap_or_default().to_string();
    assert!(notice.contains(&format!("au plus tôt le {}", shown_date(t + DAY))), "{notice}");
    assert!(legacy.exists());
}

/// Retrait d'un doublon dont le jeton n'a pas d'autre copie : la question
/// dit que le compte reste enregistré sous l'autre ligne (pas qu'il faudra
/// récupérer un jeton depuis Discord).
#[test]
fn twin_removal_says_the_account_stays() {
    let b = Bench::new();
    let dir = TempDir::new("doublon-texte");
    let vault = Arc::new(MemoryVault::with([(AccountKey::imported(), Token::from(TOKEN_A))]));
    let mut s = stored(&vault, &[(ME, "Moi Fictif", TOKEN_B)]);
    s.accounts.push(Account { user_id: Some(ME), name: Some("Moi Fictif".into()), ..Account::new(AccountKey::imported()) });
    std::fs::write(dir.settings(), settings::export(&s)).unwrap_or_else(|e| panic!("{e}"));
    let mut app = b.app(vault.clone(), Some(&dir.settings()), None, None);
    until(&mut app, "écran de connexion", |a| !a.is_busy());
    let q = ask_remove(&mut app, &AccountKey::imported());
    assert!(q.contains("seul exemplaire enregistré de ce jeton-là") && q.contains("Le compte reste enregistré sous « Moi Fictif »"), "{q}");
    assert!(!q.contains("récupérer le jeton depuis Discord"), "{q}");
    // Plus de jeton dans l'autre ligne : il faudrait bien le récupérer.
    vault.delete(&AccountKey::of_user(ME)).unwrap_or_else(|e| panic!("{e}"));
    app.cancel_confirmation();
    let q = ask_remove(&mut app, &AccountKey::imported());
    assert!(q.contains("récupérer le jeton depuis Discord"), "{q}");
}
