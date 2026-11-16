//! Identité annoncée à Discord : tout ce qui dit « quel client je suis »
//! sort d'ici (propriétés de l'Identify, `X-Super-Properties`, User-Agent,
//! en-têtes associés), pour que les mêmes valeurs servent partout.
//!
//! Deux modes :
//!
//! - **bureau** (par défaut sous Windows, décision de l'utilisateur du
//!   14 novembre 2026, risque accepté) : les propriétés du client de bureau
//!   officiel, pour que Discord classe la session comme « ordinateur » et non
//!   « navigateur » ;
//! - **Ferrocord** : l'identité honnête d'origine (`browser: "Ferrocord"`,
//!   User-Agent `Ferrocord/x.y.z`, aucun en-tête du client officiel). Toujours
//!   employée pour un jeton de bot et hors de Windows (Android compris).
//!
//! Sources (lues le 14 novembre 2026) :
//!
//! - docs.discord.food, « Reference » › Client Properties (structure, types de
//!   système et de navigateur, signature de lancement, exemple Windows),
//!   › Locales (`X-Discord-Locale`, puis `Accept-Language`) ;
//! - docs.discord.food, « Client Distribution » › Get Latest Distributed
//!   Application Manifest (`host_version`, `metadata_version`) ;
//! - discord.py-self (`discord/tracking.py`, `discord/http.py`,
//!   `discord/gateway.py`) : en-têtes REST et WebSocket du client, propriétés
//!   propres à la passerelle, numéro de build lu dans la page `/login`,
//!   indications `Sec-CH-UA` de Chromium ;
//! - norme Fetch (WHATWG) : `Origin` seulement hors GET et HEAD pour une
//!   requête de même origine.
//!
//! Ce que ces sources n'établissent pas n'est pas envoyé (voir
//! `docs/passerelle-utilisateur.md`, « Identité annoncée ») : `capabilities`,
//! `presence` et `client_state` de l'Identify, `client_app_state`,
//! `X-Debug-Options`, `X-Installation-ID`, `X-Context-Properties`.
//!
//! Le jeton n'entre dans aucune valeur de ce module : [`Identity::identify`]
//! le pose dans le champ `token` du paquet, nulle part ailleurs.

use crate::state::identity_prefs::{dotted_version, ForcedVersions};
use crate::state::usersettings::b64;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

/// User-Agent du mode Ferrocord.
pub const FERROCORD_UA: &str = concat!("Ferrocord/", env!("CARGO_PKG_VERSION"));
/// Date des numéros de version embarqués ([`Versions::embedded`]).
pub const EMBEDDED_ON: &str = "2026-11-14";

const ORIGIN: &str = "https://discord.com";
const REFERER: &str = "https://discord.com/channels/@me";
/// L'identifiant de battement d'analyse est refait toutes les 30 minutes
/// (docs.discord.food, `client_heartbeat_session_id`).
const BEAT_LIFE: Duration = Duration::from_secs(30 * 60);
/// Au plus un rafraîchissement des numéros de version par jour.
const REFRESH_EVERY_SECS: i64 = 24 * 3600;
const REFRESH_WAIT: Duration = Duration::from_secs(15);
/// Taille maximale lue d'une réponse de rafraîchissement.
const REFRESH_BODY: usize = 1 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Desktop,
    Ferrocord,
}

/// Numéros de version du client de bureau stable pour Windows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Versions {
    /// Version de l'hôte (`client_version`, et `discord/…` du User-Agent).
    pub client_version: String,
    /// Build du client web chargé par l'hôte.
    pub client_build_number: u32,
    /// Version des métadonnées natives (`metadata_version` du manifeste).
    pub native_build_number: u32,
    /// Version d'Electron (`browser_version`).
    pub electron: String,
    /// Version de Chromium de cet Electron.
    pub chrome: String,
}

impl Versions {
    /// Valeurs relevées le [`EMBEDDED_ON`] (manifeste des mises à jour et page
    /// `/login` de Discord ; Electron et Chromium : cordapi.dolfi.es, le
    /// service de discord.py-self). Seuls Electron et Chromium ne sont jamais
    /// rafraîchis seuls : ils se règlent dans `local.toml`.
    pub fn embedded() -> Self {
        Versions { client_version: "1.0.9268".into(), client_build_number: 673_764, native_build_number: 98_625, electron: "44.10.2".into(), chrome: "152.0.7977.169".into() }
    }

    /// Le plus récent des numéros embarqués et rafraîchis, puis ce que
    /// `local.toml` impose.
    pub fn resolve(fetched: &Fetched, prefs: &ForcedVersions) -> Self {
        let mut v = Versions::embedded();
        if let Some(c) = fetched.client_version.as_deref().filter(|c| host_version(c) && version_key(c) > version_key(&v.client_version)) {
            v.client_version = c.to_string();
        }
        v.client_build_number = v.client_build_number.max(fetched.client_build_number.filter(|n| BUILD_RANGE.contains(n)).unwrap_or(0));
        v.native_build_number = v.native_build_number.max(fetched.native_build_number.filter(|n| NATIVE_RANGE.contains(n)).unwrap_or(0));
        let text = |slot: &mut String, forced: &Option<String>| {
            if let Some(f) = forced.as_deref().filter(|f| dotted_version(f)) {
                *slot = f.to_string();
            }
        };
        text(&mut v.client_version, &prefs.client_version);
        text(&mut v.electron, &prefs.electron);
        text(&mut v.chrome, &prefs.chrome);
        v.client_build_number = prefs.client_build_number.unwrap_or(v.client_build_number);
        v.native_build_number = prefs.native_build_number.unwrap_or(v.native_build_number);
        v
    }

    fn chrome_major(&self) -> u32 {
        self.chrome.split('.').next().and_then(|m| m.parse().ok()).unwrap_or(0)
    }
}

/// Au-delà, les versions d'Electron et de Chromium embarquées sont tenues
/// pour périmées (Discord change d'Electron toutes les quelques semaines) :
/// seule une mise à jour de Ferrocord les renouvelle.
pub const EMBEDDED_STALE_DAYS: i64 = 56;
/// Au-delà, le numéro de build (relevé ou embarqué) est tenu pour périmé.
pub const BUILD_STALE_DAYS: i64 = 21;

/// Jour (depuis 1970) du relevé embarqué.
fn embedded_day() -> Option<i64> {
    chrono::NaiveDate::parse_from_str(EMBEDDED_ON, "%Y-%m-%d").ok().map(|d| d.and_time(chrono::NaiveTime::MIN).and_utc().timestamp() / 86_400)
}

/// Âge de ce qui est annoncé (Paramètres › Avancés).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Freshness {
    pub versions: Versions,
    /// Jours depuis le relevé embarqué ([`EMBEDDED_ON`]).
    pub embedded_days: i64,
    /// Jours depuis le dernier numéro de build connu (relevé réussi, sinon l'embarqué).
    pub build_days: i64,
    /// Une valeur de `local.toml` l'emporte : l'âge ne dit plus rien.
    pub forced: bool,
}

impl Freshness {
    /// `today` : jours depuis 1970 ; `fetched` : dernier rafraîchissement connu.
    pub fn at(today: i64, fetched: &Fetched, prefs: &ForcedVersions) -> Self {
        let embedded = embedded_day().unwrap_or(today);
        let build = embedded.max(fetched.build_at / 86_400).min(today);
        Freshness { versions: Versions::resolve(fetched, prefs), embedded_days: (today - embedded).max(0), build_days: (today - build).max(0), forced: *prefs != ForcedVersions::default() }
    }

    /// Electron et Chromium trop vieux : mettre Ferrocord à jour.
    pub fn embedded_stale(&self) -> bool {
        !self.forced && self.embedded_days > EMBEDDED_STALE_DAYS
    }

    /// Numéro de build trop vieux (rafraîchissement en échec depuis longtemps).
    pub fn build_stale(&self) -> bool {
        !self.forced && self.build_days > BUILD_STALE_DAYS
    }
}

/// Âge des valeurs annoncées pour ce lancement : lu une fois (fichiers du
/// cache et de `local.toml`), aucune requête.
pub fn freshness() -> &'static Freshness {
    static F: std::sync::OnceLock<Freshness> = std::sync::OnceLock::new();
    F.get_or_init(|| {
        let prefs = crate::machine::default_dir().map(|d| crate::machine::read_prefs(&d.join("local.toml"))).unwrap_or_default().identity;
        let fetched = crate::platform::cache_dir().map(|d| Fetched::read(&d.join("ferrocord").join("identite.json"))).unwrap_or_default();
        Freshness::at(chrono::Utc::now().timestamp() / 86_400, &fetched, &prefs.forced())
    })
}

const BUILD_RANGE: std::ops::Range<u32> = 100_000..100_000_000;
const NATIVE_RANGE: std::ops::Range<u32> = 1..10_000_000;

fn version_key(s: &str) -> Vec<u32> {
    s.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// `1.0.9268` : trois nombres.
fn host_version(s: &str) -> bool {
    dotted_version(s) && s.split('.').count() == 3
}

/// Ce que le dernier rafraîchissement a appris (fichier `identite.json` du
/// cache). Tout est revalidé à la lecture.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fetched {
    /// Dernier essai (secondes Unix), réussi ou non.
    pub tried_at: i64,
    /// Dernier relevé réussi du numéro de build (secondes Unix ; 0 : jamais).
    pub build_at: i64,
    pub client_version: Option<String>,
    pub client_build_number: Option<u32>,
    pub native_build_number: Option<u32>,
    /// Identifiant d'installation envoyé au serveur de mises à jour (UUID
    /// tiré au hasard une fois ; jamais lié au compte, jamais envoyé ailleurs).
    pub install_id: Option<String>,
}

impl Fetched {
    pub fn read(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    fn write(&self, path: &Path) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string(self) {
            let _ = std::fs::write(path, text);
        }
    }
}

/// Ce que le client de bureau lit de la machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Machine {
    /// `10.0.26100` (version du noyau de Windows).
    pub os_version: String,
    /// Numéro de build de Windows (`26100`).
    pub os_sdk_version: String,
    pub os_arch: &'static str,
    pub app_arch: &'static str,
    /// Langue du système (`fr-FR`).
    pub system_locale: String,
    /// Fuseau horaire IANA (`Europe/Paris`) ; inconnu : en-tête omis.
    pub timezone: Option<String>,
}

impl Machine {
    /// Valeurs réelles de cette machine ; `None` hors de Windows (le mode
    /// bureau n'y existe pas : on n'annonce pas un Windows qu'on n'a pas).
    pub fn detect() -> Option<Machine> {
        #[cfg(windows)]
        return windows_machine();
        #[cfg(not(windows))]
        None
    }
}

/// Étiquette de langue du système, réduite à ce qui peut entrer dans un en-tête.
#[cfg_attr(not(windows), allow(dead_code))]
fn tidy_locale(tag: &str) -> String {
    let tag = tag.split(['.', '@']).next().unwrap_or_default().replace('_', "-");
    if !tag.is_empty() && tag.len() <= 16 && tag.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') { tag } else { "en-US".into() }
}

#[cfg(windows)]
fn windows_machine() -> Option<Machine> {
    let key = windows_registry::LOCAL_MACHINE.open(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion").ok()?;
    let major = key.get_u32("CurrentMajorVersionNumber").ok()?;
    let minor = key.get_u32("CurrentMinorVersionNumber").ok()?;
    let build = key.get_string("CurrentBuildNumber").ok()?;
    if build.is_empty() || build.len() > 6 || !build.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let app_arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => return None,
    };
    // Un programme x64 émulé sur ARM voit l'architecture réelle ici.
    let os_arch = match std::env::var("PROCESSOR_ARCHITEW6432").or_else(|_| std::env::var("PROCESSOR_ARCHITECTURE")).as_deref() {
        Ok("ARM64") => "arm64",
        Ok("AMD64") => "x64",
        _ => app_arch,
    };
    let timezone = windows::Globalization::Calendar::new()
        .and_then(|c| c.GetTimeZone())
        .ok()
        .map(|z| z.to_string())
        .filter(|z| !z.is_empty() && z.len() <= 64 && z.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'-' | b'+')));
    Some(Machine {
        os_version: format!("{major}.{minor}.{build}"),
        os_sdk_version: build,
        os_arch,
        app_arch,
        system_locale: tidy_locale(&sys_locale::get_locale().unwrap_or_default()),
        timezone,
    })
}

/// Où rafraîchir les numéros de version, et où les garder.
#[derive(Clone, Debug)]
pub struct Refresh {
    /// Fichier `identite.json`.
    pub cache: PathBuf,
    /// Manifeste des mises à jour du client de bureau (sans paramètres).
    pub manifest_url: String,
    /// Page de connexion du client web (elle porte `BUILD_NUMBER`).
    pub login_url: String,
    /// Un seul rafraîchissement à la fois, et un seul par lancement.
    started: Arc<AtomicBool>,
}

impl Refresh {
    pub fn new(cache: PathBuf, manifest_url: String, login_url: String) -> Self {
        Refresh { cache, manifest_url, login_url, started: Arc::default() }
    }

    /// Adresses de Discord (non réglables).
    fn discord(cache: PathBuf) -> Self {
        Self::new(cache, "https://updates.discord.com/distributions/app/manifests/latest".into(), "https://discord.com/login".into())
    }
}

/// Identité choisie pour ce lancement : mode, numéros de version et machine
/// sont figés (comme un client officiel, dont la version ne change pas en
/// cours de route) ; seul le mode est relu à chaque connexion.
#[derive(Clone, Debug)]
pub struct Config {
    pub mode: Mode,
    pub versions: Versions,
    /// `None` : mode bureau impossible (pas sous Windows).
    pub machine: Option<Machine>,
    pub refresh: Option<Refresh>,
    /// `local.toml`, relu à chaque connexion pour le seul réglage du mode.
    prefs_file: Option<PathBuf>,
    /// Langue choisie dans Ferrocord (réglage explicite), en langue de
    /// Discord : c'est elle que `X-Discord-Locale` annonce, comme le client
    /// officiel annonce la langue de son interface. `None` : celle du système.
    ui_locale: Option<&'static str>,
}

impl Default for Config {
    /// Identité de Ferrocord : ce que tout appelant obtient sans rien demander
    /// (tests, aperçu). L'application passe par [`Config::for_app`].
    fn default() -> Self {
        Config { mode: Mode::Ferrocord, versions: Versions::embedded(), machine: None, refresh: None, prefs_file: None, ui_locale: None }
    }
}

impl Config {
    /// Identité de bureau avec ces valeurs (tests ; l'application : [`Config::for_app`]).
    pub fn desktop(versions: Versions, machine: Machine) -> Self {
        Config { mode: Mode::Desktop, versions, machine: Some(machine), refresh: None, prefs_file: None, ui_locale: None }
    }

    /// Lit le réglage (`local.toml`) et les numéros rafraîchis (cache).
    pub fn for_app() -> Self {
        let prefs_file = crate::machine::default_dir().map(|d| d.join("local.toml"));
        let prefs = prefs_file.as_deref().map(crate::machine::read_prefs).unwrap_or_default().identity;
        let cache = crate::platform::cache_dir().map(|d| d.join("ferrocord").join("identite.json"));
        let fetched = cache.as_deref().map(Fetched::read).unwrap_or_default();
        let machine = Machine::detect();
        Config {
            mode: if prefs.desktop && machine.is_some() { Mode::Desktop } else { Mode::Ferrocord },
            versions: Versions::resolve(&fetched, &prefs.forced()),
            machine,
            refresh: cache.map(Refresh::discord),
            prefs_file,
            ui_locale: None,
        }
    }

    /// Avec la langue choisie dans l'interface (tests ; l'application : [`Config::reread_mode`]).
    pub fn with_ui_language(mut self, lang: Option<crate::i18n::Lang>) -> Self {
        self.ui_locale = lang.map(ui_locale);
        self
    }

    /// Avec un rafraîchissement des numéros de version (tests : faux serveur local).
    pub fn with_refresh(mut self, refresh: Refresh) -> Self {
        self.refresh = Some(refresh);
        self
    }

    /// La même identité, avec le mode que dit `local.toml` maintenant.
    pub fn reread_mode(&self) -> Self {
        let mut c = self.clone();
        c.ui_locale = crate::i18n::chosen_language().map(ui_locale);
        if let Some(f) = &self.prefs_file {
            let desktop = crate::machine::read_prefs(f).identity.desktop;
            c.mode = if desktop && c.machine.is_some() { Mode::Desktop } else { Mode::Ferrocord };
        }
        c
    }

    fn desktop_machine(&self) -> Option<&Machine> {
        self.machine.as_ref().filter(|_| self.mode == Mode::Desktop)
    }

    pub fn is_desktop(&self) -> bool {
        self.desktop_machine().is_some()
    }

    /// User-Agent de toutes les requêtes HTTP (API, CDN).
    pub fn user_agent(&self) -> String {
        if self.is_desktop() { desktop_user_agent(&self.versions) } else { FERROCORD_UA.to_string() }
    }

    /// En-têtes ajoutés à l'ouverture d'un WebSocket (passerelle, vocal) ;
    /// mode Ferrocord : aucun, comme avant.
    pub fn ws_headers(&self) -> Vec<(&'static str, String)> {
        match self.desktop_machine() {
            Some(m) => vec![("User-Agent", desktop_user_agent(&self.versions)), ("Origin", ORIGIN.to_string()), ("Accept-Language", accept_language(&m.system_locale))],
            None => Vec::new(),
        }
    }
}

/// User-Agent d'Electron tel que le client de bureau l'annonce
/// (docs.discord.food, exemple Windows). Chromium fige la partie système.
fn desktop_user_agent(v: &Versions) -> String {
    format!(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) discord/{} Chrome/{} Electron/{} Safari/537.36",
        v.client_version, v.chrome, v.electron
    )
}

/// `fr-FR` → `fr-FR,fr;q=0.9` (convention de Chromium ; discord.py-self
/// envoie `en-US,en;q=0.9`).
fn accept_language(locale: &str) -> String {
    match locale.split_once('-') {
        Some((lang, _)) => format!("{locale},{lang};q=0.9"),
        None => locale.to_string(),
    }
}

/// Langue de Discord la plus proche de celle du système
/// (docs.discord.food, « Reference » › Locales).
fn discord_locale(system: &str) -> &'static str {
    const EXACT: [&str; 8] = ["en-GB", "en-US", "es-ES", "es-419", "pt-BR", "sv-SE", "zh-CN", "zh-TW"];
    const PLAIN: [&str; 25] = ["ar", "bg", "cs", "da", "de", "el", "fi", "fr", "hi", "hr", "hu", "id", "it", "ja", "ko", "lt", "nl", "no", "pl", "ro", "ru", "th", "tr", "uk", "vi"];
    if let Some(e) = EXACT.iter().find(|e| e.eq_ignore_ascii_case(system)) {
        return e;
    }
    let lang = system.split('-').next().unwrap_or_default().to_ascii_lowercase();
    match lang.as_str() {
        "es" => "es-419",
        "pt" => "pt-BR",
        "sv" => "sv-SE",
        "zh" => "zh-CN",
        "nb" | "nn" => "no",
        l => PLAIN.iter().find(|p| **p == l).copied().unwrap_or("en-US"),
    }
}

/// Langue de Discord correspondant à une langue de l'interface de Ferrocord.
fn ui_locale(lang: crate::i18n::Lang) -> &'static str {
    match lang {
        crate::i18n::Lang::Fr => "fr",
        crate::i18n::Lang::En => "en-US",
    }
}

/// `Sec-CH-UA` d'Electron : la marque « Chromium » et une marque leurre, que
/// Chromium dérive de sa version majeure (algorithme repris de
/// discord.py-self, `generate_chromium_brand_version_list`, sans marque de
/// navigateur comme le fait Electron).
fn sec_ch_ua(major: u32) -> String {
    const CHARS: [char; 11] = [' ', '(', ':', '-', '.', '/', ')', ';', '=', '?', '_'];
    const GREASED: [&str; 3] = ["8", "99", "24"];
    let seed = major as usize;
    let grease = format!("\"Not{}A{}Brand\";v=\"{}\"", CHARS[seed % 11], CHARS[(seed + 1) % 11], GREASED[seed % 3]);
    let chromium = format!("\"Chromium\";v=\"{major}\"");
    if seed.is_multiple_of(2) { format!("{grease}, {chromium}") } else { format!("{chromium}, {grease}") }
}

fn random_u128() -> u128 {
    let mut bytes = [0u8; 16];
    if aws_lc_rs::rand::fill(&mut bytes).is_err() {
        // Le générateur du système ne répond pas : celui de la bibliothèque
        // standard (clés de hachage aléatoires) suffit à un identifiant.
        use std::hash::{BuildHasher, Hasher};
        for half in bytes.chunks_mut(8) {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u8(1);
            half.copy_from_slice(&h.finish().to_be_bytes());
        }
    }
    u128::from_be_bytes(bytes)
}

fn uuid_text(n: u128) -> String {
    let h = format!("{n:032x}");
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

/// Bits de version (4) et de variante (RFC 4122) posés sur 128 bits aléatoires.
fn v4(n: u128) -> u128 {
    (n & !(0xf_u128 << 76) & !(0x3_u128 << 62)) | (0x4_u128 << 76) | (0x2_u128 << 62)
}

/// UUID version 4 (aléatoire).
fn uuid4() -> String {
    uuid_text(v4(random_u128()))
}

/// Bits de la signature de lancement qui dénoncent une modification du
/// client (BetterDiscord, Vencord…) : toujours à zéro
/// (docs.discord.food, « Launch Signature »).
const MOD_BITS: u128 = 0b00000000100000000001000000010000000010000001000000001000000000000010000010000001000000000100000000000001000000000000100000000000;

fn launch_signature() -> String {
    uuid_text(v4(random_u128()) & !MOD_BITS)
}

/// Propriétés du client de bureau, dans l'ordre où il les écrit (relevé du
/// 14 novembre 2026 ; structure : docs.discord.food, Client Properties).
#[derive(Serialize)]
struct Props<'a> {
    os: &'static str,
    browser: &'static str,
    release_channel: &'static str,
    client_version: &'a str,
    os_version: &'a str,
    os_arch: &'a str,
    app_arch: &'a str,
    system_locale: &'a str,
    has_client_mods: bool,
    client_launch_id: &'a str,
    browser_user_agent: &'a str,
    browser_version: &'a str,
    os_sdk_version: &'a str,
    client_build_number: u32,
    native_build_number: u32,
    client_event_source: Option<&'static str>,
    launch_signature: &'a str,
    client_heartbeat_session_id: &'a str,
    /// Passerelle seulement (jamais dans `X-Super-Properties`).
    #[serde(skip_serializing_if = "Option::is_none")]
    is_fast_connect: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gateway_connect_reasons: Option<&'static str>,
}

struct Beat {
    id: String,
    since: Instant,
    /// `X-Super-Properties` correspondant, encodé une fois.
    encoded: String,
}

struct Desktop {
    versions: Versions,
    machine: Machine,
    user_agent: String,
    launch_id: String,
    launch_signature: String,
    /// Valeur de `X-Discord-Locale`, figée pour la connexion.
    locale: &'static str,
    beat: Mutex<Beat>,
}

impl Desktop {
    fn props(&self, beat: &str, gateway: bool) -> String {
        let (v, m) = (&self.versions, &self.machine);
        let p = Props {
            os: "Windows",
            browser: "Discord Client",
            release_channel: "stable",
            client_version: &v.client_version,
            os_version: &m.os_version,
            os_arch: m.os_arch,
            app_arch: m.app_arch,
            system_locale: &m.system_locale,
            has_client_mods: false,
            client_launch_id: &self.launch_id,
            browser_user_agent: &self.user_agent,
            browser_version: &v.electron,
            os_sdk_version: &m.os_sdk_version,
            client_build_number: v.client_build_number,
            native_build_number: v.native_build_number,
            client_event_source: None,
            launch_signature: &self.launch_signature,
            client_heartbeat_session_id: beat,
            is_fast_connect: gateway.then_some(false),
            gateway_connect_reasons: gateway.then_some("AppSkeleton"),
        };
        // Une structure de chaînes et de nombres : la sérialisation n'échoue pas.
        serde_json::to_string(&p).unwrap_or_default()
    }

    /// Battement courant (refait au bout de 30 minutes) : (identifiant, `X-Super-Properties`).
    fn beat(&self) -> (String, String) {
        let mut b = self.beat.lock().unwrap_or_else(|e| e.into_inner());
        if b.since.elapsed() >= BEAT_LIFE || b.encoded.is_empty() {
            if !b.encoded.is_empty() {
                b.id = uuid4();
                b.since = Instant::now();
            }
            b.encoded = b64::encode(self.props(&b.id, false).as_bytes());
        }
        (b.id.clone(), b.encoded.clone())
    }
}

/// Identité d'une connexion : ses identifiants de lancement vivent autant qu'elle.
pub struct Identity {
    desktop: Option<Desktop>,
}

impl Identity {
    /// `bot` : un jeton de bot garde toujours l'identité de Ferrocord.
    pub fn new(cfg: &Config, bot: bool) -> Self {
        let desktop = cfg.desktop_machine().filter(|_| !bot).map(|m| Desktop {
            versions: cfg.versions.clone(),
            machine: m.clone(),
            user_agent: desktop_user_agent(&cfg.versions),
            launch_id: uuid4(),
            launch_signature: launch_signature(),
            locale: cfg.ui_locale.unwrap_or_else(|| discord_locale(&m.system_locale)),
            beat: Mutex::new(Beat { id: uuid4(), since: Instant::now(), encoded: String::new() }),
        });
        Identity { desktop }
    }

    pub fn ferrocord() -> Self {
        Identity { desktop: None }
    }

    pub fn is_desktop(&self) -> bool {
        self.desktop.is_some()
    }

    pub fn user_agent(&self) -> &str {
        self.desktop.as_ref().map_or(FERROCORD_UA, |d| &d.user_agent)
    }

    /// Paquet Identify du mode bureau ; `None` : mode Ferrocord, le paquet
    /// d'origine est construit par la passerelle, inchangé.
    pub fn identify(&self, token: &str) -> Option<String> {
        #[derive(Serialize)]
        struct D<'a> {
            token: &'a str,
            properties: &'a serde_json::value::RawValue,
            compress: bool,
            large_threshold: u8,
        }
        #[derive(Serialize)]
        struct Packet<'a> {
            op: u8,
            d: D<'a>,
        }
        let d = self.desktop.as_ref()?;
        let props = serde_json::value::RawValue::from_string(d.props(&d.beat().0, true)).ok()?;
        // Compression par paquet : incompatible avec celle du transport.
        serde_json::to_string(&Packet { op: 2, d: D { token, properties: &props, compress: false, large_threshold: 50 } }).ok()
    }

    /// Valeur de `X-Super-Properties` (mode bureau).
    pub fn super_properties(&self) -> Option<String> {
        self.desktop.as_ref().map(|d| d.beat().1)
    }

    /// En-têtes du client de bureau sur une requête de l'API ; mode
    /// Ferrocord : la requête est rendue telle quelle.
    pub fn rest(&self, req: reqwest::RequestBuilder, method: &reqwest::Method) -> reqwest::RequestBuilder {
        let Some(d) = &self.desktop else { return req };
        let locale = &d.machine.system_locale;
        let mut req = req
            .header("X-Super-Properties", d.beat().1)
            .header("X-Discord-Locale", d.locale)
            .header("Accept-Language", accept_language(locale))
            .header("Referer", REFERER)
            .header("Sec-CH-UA", sec_ch_ua(d.versions.chrome_major()))
            .header("Sec-CH-UA-Mobile", "?0")
            .header("Sec-CH-UA-Platform", "\"Windows\"")
            .header("Sec-Fetch-Dest", "empty")
            .header("Sec-Fetch-Mode", "cors")
            .header("Sec-Fetch-Site", "same-origin");
        if let Some(tz) = &d.machine.timezone {
            req = req.header("X-Discord-Timezone", tz);
        }
        if !matches!(*method, reqwest::Method::GET | reqwest::Method::HEAD) {
            req = req.header("Origin", ORIGIN);
        }
        req
    }
}

/// Demande d'ouverture d'un WebSocket avec ces en-têtes (aucun : la demande
/// ordinaire, comme avant).
pub fn ws_request(url: &str, headers: &[(&'static str, String)]) -> Result<tokio_tungstenite::tungstenite::handshake::client::Request, tokio_tungstenite::tungstenite::Error> {
    use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue};
    let mut req = url.into_client_request()?;
    for (name, value) in headers {
        if let Ok(v) = HeaderValue::from_str(value) {
            req.headers_mut().insert(*name, v);
        }
    }
    Ok(req)
}

// ============================================================ rafraîchissement

/// `host_version` et `metadata_version` du manifeste
/// (docs.discord.food, Get Latest Distributed Application Manifest).
fn manifest_versions(body: &str) -> (Option<String>, Option<u32>) {
    #[derive(Deserialize)]
    struct Full {
        host_version: Option<Vec<u32>>,
    }
    #[derive(Deserialize)]
    struct Manifest {
        full: Option<Full>,
        metadata_version: Option<u32>,
    }
    let Ok(m) = serde_json::from_str::<Manifest>(body) else { return (None, None) };
    let host = m.full.and_then(|f| f.host_version).filter(|h| h.len() == 3).map(|h| format!("{}.{}.{}", h[0], h[1], h[2])).filter(|h| host_version(h));
    (host, m.metadata_version.filter(|n| NATIVE_RANGE.contains(n)))
}

/// `"BUILD_NUMBER":"673764"` dans la page (discord.py-self,
/// `scrape_client_build_number`).
fn build_number(page: &str) -> Option<u32> {
    let rest = page.split_once("\"BUILD_NUMBER\":")?.1.trim_start().strip_prefix('"')?;
    let digits = rest.split_once('"')?.0;
    digits.parse().ok().filter(|n| BUILD_RANGE.contains(n))
}

async fn get(http: &reqwest::Client, limits: &Arc<super::ratelimit::RateLimits>, template: &'static str, url: &str) -> Option<String> {
    use super::ratelimit::{Headers, Outcome, Route};
    let slot = limits.acquire(Route { template, major: 0 }, REFRESH_WAIT, std::future::pending()).await.ok()?;
    let mut resp = http.get(url).timeout(REFRESH_WAIT).send().await.ok()?;
    let status = resp.status().as_u16();
    let headers = Headers::read(|n| resp.headers().get(n).and_then(|v| v.to_str().ok()));
    slot.complete(&Outcome { status, headers, ..Outcome::default() });
    if !(200..300).contains(&status) {
        return None;
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.ok()? {
        if body.len() + chunk.len() > REFRESH_BODY {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).ok()
}

/// Demande à Discord les numéros de version courants du client de bureau :
/// deux GET publics, sans jeton, au plus une fois par jour (l'heure du
/// dernier essai, réussi ou non, est gardée dans le cache), par le limiteur
/// de débit. Le résultat sert au prochain lancement ; en cas d'échec, les
/// valeurs déjà connues restent.
pub async fn refresh(r: Refresh, cfg: Config) {
    let Some(arch) = cfg.desktop_machine().map(|m| m.app_arch) else { return };
    if r.started.swap(true, Ordering::SeqCst) {
        return;
    }
    let now = chrono::Utc::now().timestamp();
    let mut known = Fetched::read(&r.cache);
    if (0..REFRESH_EVERY_SECS).contains(&(now - known.tried_at)) {
        return;
    }
    known.tried_at = now;
    let install_id = known.install_id.clone().filter(|i| i.len() == 36 && i.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')).unwrap_or_else(uuid4);
    known.install_id = Some(install_id.clone());
    // Écrit avant d'essayer : un échec ne relance pas d'essai avant demain.
    known.write(&r.cache);
    let Ok(http) = crate::platform::http_builder().user_agent(cfg.user_agent()).no_proxy().redirect(reqwest::redirect::Policy::none()).build() else { return };
    let limits = Arc::new(super::ratelimit::RateLimits::default());
    let manifest = format!("{}?install_id={install_id}&channel=stable&platform=win&arch={arch}", r.manifest_url);
    if let Some(body) = get(&http, &limits, "GET /distributions/app/manifests/latest", &manifest).await {
        let (host, native) = manifest_versions(&body);
        known.client_version = host.or(known.client_version);
        known.native_build_number = native.or(known.native_build_number);
    }
    if let Some(page) = get(&http, &limits, "GET /login", &r.login_url).await
        && let Some(n) = build_number(&page)
    {
        known.client_build_number = Some(n);
        known.build_at = now;
    }
    known.write(&r.cache);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const TOKEN: &str = "faux.jeton.utilisateur-DE-TEST";

    fn machine() -> Machine {
        Machine { os_version: "10.0.26100".into(), os_sdk_version: "26100".into(), os_arch: "x64", app_arch: "x64", system_locale: "fr-FR".into(), timezone: Some("Europe/Paris".into()) }
    }

    fn desktop() -> Identity {
        Identity::new(&Config::desktop(Versions::embedded(), machine()), false)
    }

    fn is_uuid4(s: &str) -> bool {
        let b = s.as_bytes();
        s.len() == 36 && [8, 13, 18, 23].iter().all(|i| b[*i] == b'-') && b[14] == b'4' && matches!(b[19], b'8' | b'9' | b'a' | b'b') && s.bytes().all(|c| c == b'-' || c.is_ascii_hexdigit())
    }

    #[test]
    fn desktop_identify_has_the_documented_fields_in_order() {
        let id = desktop();
        let text = id.identify(TOKEN).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["op"], 2);
        assert_eq!(v["d"]["token"], TOKEN);
        assert_eq!(v["d"]["compress"], false);
        assert_eq!(v["d"]["large_threshold"], 50);
        assert!(v["d"].get("intents").is_none() && v["d"].get("capabilities").is_none());
        let p = &v["d"]["properties"];
        let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) discord/1.0.9268 Chrome/152.0.7977.169 Electron/44.10.2 Safari/537.36";
        for (k, want) in [
            ("os", json!("Windows")),
            ("browser", json!("Discord Client")),
            ("release_channel", json!("stable")),
            ("client_version", json!("1.0.9268")),
            ("os_version", json!("10.0.26100")),
            ("os_arch", json!("x64")),
            ("app_arch", json!("x64")),
            ("system_locale", json!("fr-FR")),
            ("has_client_mods", json!(false)),
            ("browser_user_agent", json!(ua)),
            ("browser_version", json!("44.10.2")),
            ("os_sdk_version", json!("26100")),
            ("client_build_number", json!(673_764)),
            ("native_build_number", json!(98_625)),
            ("client_event_source", Value::Null),
            ("is_fast_connect", json!(false)),
            ("gateway_connect_reasons", json!("AppSkeleton")),
        ] {
            assert_eq!(p[k], want, "{k}");
        }
        assert_eq!(id.user_agent(), ua);
        for k in ["client_launch_id", "launch_signature", "client_heartbeat_session_id"] {
            assert!(is_uuid4(p[k].as_str().unwrap()), "{k} : {}", p[k]);
        }
        // L'ordre des clés est celui du client officiel (pas l'ordre alphabétique).
        let order = ["\"os\"", "\"browser\"", "\"release_channel\"", "\"client_version\"", "\"os_version\"", "\"os_arch\"", "\"app_arch\"", "\"system_locale\"", "\"has_client_mods\"", "\"client_launch_id\"", "\"browser_user_agent\"", "\"browser_version\"", "\"os_sdk_version\"", "\"client_build_number\"", "\"native_build_number\"", "\"client_event_source\"", "\"launch_signature\"", "\"client_heartbeat_session_id\"", "\"is_fast_connect\"", "\"gateway_connect_reasons\""];
        let at: Vec<usize> = order.iter().map(|k| text.find(&format!("{k}:")).unwrap_or_else(|| panic!("{k} absent"))).collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]), "ordre des propriétés : {text}");
    }

    #[test]
    fn super_properties_match_the_identify_and_never_hold_the_token() {
        let id = desktop();
        let identify: Value = serde_json::from_str(&id.identify(TOKEN).unwrap()).unwrap();
        let encoded = id.super_properties().unwrap();
        let decoded = String::from_utf8(b64::decode(&encoded).unwrap()).unwrap();
        let sp: Value = serde_json::from_str(&decoded).unwrap();
        let mut gw = identify["d"]["properties"].clone();
        // Seules différences : les deux champs propres à la passerelle.
        gw.as_object_mut().unwrap().remove("is_fast_connect");
        gw.as_object_mut().unwrap().remove("gateway_connect_reasons");
        assert_eq!(sp, gw);
        assert_eq!(sp["browser_user_agent"], id.user_agent());
        assert!(!decoded.contains(TOKEN) && !identify["d"]["properties"].to_string().contains(TOKEN));
        assert_eq!(id.super_properties().unwrap(), encoded, "stable tant que le battement ne change pas");
    }

    #[test]
    fn heartbeat_session_rotates_after_thirty_minutes() {
        let id = desktop();
        let d = id.desktop.as_ref().unwrap();
        let (first, encoded) = d.beat();
        d.beat.lock().unwrap().since = Instant::now().checked_sub(BEAT_LIFE).unwrap();
        let (second, encoded2) = d.beat();
        assert!(first != second && encoded != encoded2 && is_uuid4(&second));
        let sp: Value = serde_json::from_slice(&b64::decode(&encoded2).unwrap()).unwrap();
        assert_eq!(sp["client_heartbeat_session_id"], second);
        assert_eq!(sp["client_launch_id"], d.launch_id, "le lancement, lui, ne change pas");
    }

    #[test]
    fn launch_signature_never_flags_a_client_mod() {
        for _ in 0..64 {
            let s = launch_signature();
            assert!(is_uuid4(&s), "{s}");
            assert_eq!(u128::from_str_radix(&s.replace('-', ""), 16).unwrap() & MOD_BITS, 0);
        }
        assert_eq!(MOD_BITS.count_ones(), 12);
        assert!([108, 84, 11].iter().all(|bit| (MOD_BITS >> bit) & 1 == 1), "BetterDiscord, Vencord, Moonlight");
    }

    #[test]
    fn rest_headers_of_both_modes() {
        let http = reqwest::Client::new();
        let headers = |id: &Identity, m: reqwest::Method| {
            let req = id.rest(http.request(m.clone(), "http://127.0.0.1:9/x"), &m).build().unwrap();
            req.headers().iter().map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap().to_string())).collect::<std::collections::BTreeMap<_, _>>()
        };
        let id = desktop();
        let get = headers(&id, reqwest::Method::GET);
        assert_eq!(get["x-super-properties"], id.super_properties().unwrap());
        assert_eq!(get["x-discord-locale"], "fr");
        assert_eq!(get["x-discord-timezone"], "Europe/Paris");
        assert_eq!(get["accept-language"], "fr-FR,fr;q=0.9");
        assert_eq!(get["referer"], "https://discord.com/channels/@me");
        assert_eq!(get["sec-ch-ua"], "\"Not?A_Brand\";v=\"24\", \"Chromium\";v=\"152\"");
        assert_eq!((get["sec-fetch-site"].as_str(), get["sec-fetch-mode"].as_str(), get["sec-fetch-dest"].as_str()), ("same-origin", "cors", "empty"));
        assert!(!get.contains_key("origin"), "même origine : pas d'Origin sur un GET");
        assert_eq!(headers(&id, reqwest::Method::POST)["origin"], "https://discord.com");
        assert!(!get.values().any(|v| v.contains("Ferrocord") || v.contains("ferrocord")), "aucun en-tête au nom de Ferrocord");

        for honest in [Identity::ferrocord(), Identity::new(&Config::default(), false), Identity::new(&Config::desktop(Versions::embedded(), machine()), true)] {
            assert!(headers(&honest, reqwest::Method::POST).is_empty(), "mode Ferrocord (ou bot) : aucun en-tête ajouté");
            assert_eq!(honest.user_agent(), FERROCORD_UA);
            assert!(honest.identify(TOKEN).is_none() && honest.super_properties().is_none());
        }
        assert!(Config::default().ws_headers().is_empty());
        let ws = Config::desktop(Versions::embedded(), machine()).ws_headers();
        assert_eq!(ws[0], ("User-Agent", id.user_agent().to_string()));
        assert_eq!(ws[1], ("Origin", "https://discord.com".to_string()));
    }

    /// Lot « fiable » : `X-Discord-Locale` suit la langue choisie dans
    /// Ferrocord (comme le client officiel suit la sienne), `Accept-Language`
    /// et `system_locale` restent ceux du système ; sans choix, la langue du
    /// système sert aux trois. Figé pour la connexion.
    #[test]
    fn discord_locale_follows_the_chosen_language() {
        let http = reqwest::Client::new();
        let get = |cfg: &Config| {
            let req = Identity::new(cfg, false).rest(http.get("http://127.0.0.1:9/x"), &reqwest::Method::GET).build().unwrap();
            let h = |n: &str| req.headers().get(n).map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
            (h("x-discord-locale"), h("accept-language"))
        };
        let base = Config::desktop(Versions::embedded(), machine());
        assert_eq!(get(&base), ("fr".into(), "fr-FR,fr;q=0.9".into()));
        assert_eq!(get(&base.clone().with_ui_language(Some(crate::i18n::Lang::En))), ("en-US".into(), "fr-FR,fr;q=0.9".into()));
        assert_eq!(get(&base.clone().with_ui_language(Some(crate::i18n::Lang::Fr))), ("fr".into(), "fr-FR,fr;q=0.9".into()));
        // Les propriétés (système) ne changent pas avec la langue de l'interface.
        let en = Identity::new(&base.clone().with_ui_language(Some(crate::i18n::Lang::En)), false);
        let v: Value = serde_json::from_str(&en.identify(TOKEN).unwrap()).unwrap();
        assert_eq!(v["d"]["properties"]["system_locale"], "fr-FR");
        // Mode Ferrocord : toujours aucun en-tête.
        let honest = Identity::new(&Config::default().with_ui_language(Some(crate::i18n::Lang::En)), false);
        assert!(honest.rest(http.get("http://127.0.0.1:9/x"), &reqwest::Method::GET).build().unwrap().headers().is_empty());
    }

    #[test]
    fn desktop_mode_needs_a_windows_machine() {
        let c = Config { mode: Mode::Desktop, ..Config::default() };
        assert!(!c.is_desktop() && c.user_agent() == FERROCORD_UA && !Identity::new(&c, false).is_desktop());
    }

    #[test]
    fn locales_and_client_hints() {
        assert_eq!(discord_locale("fr-FR"), "fr");
        assert_eq!(discord_locale("en-gb"), "en-GB");
        assert_eq!(discord_locale("es-MX"), "es-419");
        assert_eq!(discord_locale("zh-TW"), "zh-TW");
        assert_eq!(discord_locale("xx-YY"), "en-US");
        assert_eq!(accept_language("en-US"), "en-US,en;q=0.9");
        assert_eq!(tidy_locale("fr_FR.UTF-8"), "fr-FR");
        assert_eq!(tidy_locale("fr\r\nX: 1"), "en-US");
        assert_eq!(sec_ch_ua(134), "\"Not:A-Brand\";v=\"24\", \"Chromium\";v=\"134\"");
        assert_eq!(sec_ch_ua(135), "\"Chromium\";v=\"135\", \"Not-A.Brand\";v=\"8\"");
    }

    #[test]
    fn versions_take_the_newest_then_what_the_file_forces() {
        let none = ForcedVersions::default();
        assert_eq!(Versions::resolve(&Fetched::default(), &none), Versions::embedded());
        let newer = Fetched { client_version: Some("1.0.9300".into()), client_build_number: Some(700_000), native_build_number: Some(99_000), ..Fetched::default() };
        let v = Versions::resolve(&newer, &none);
        assert_eq!((v.client_version.as_str(), v.client_build_number, v.native_build_number), ("1.0.9300", 700_000, 99_000));
        // Cache plus vieux que l'exécutable, ou abîmé : les valeurs embarquées restent.
        let stale = Fetched { client_version: Some("1.0.9".into()), client_build_number: Some(7), native_build_number: Some(0), ..Fetched::default() };
        assert_eq!(Versions::resolve(&stale, &none), Versions::embedded());
        let bad = Fetched { client_version: Some("9.9.9\r\nX: 1".into()), client_build_number: Some(u32::MAX), ..Fetched::default() };
        assert_eq!(Versions::resolve(&bad, &none), Versions::embedded());
        let forced = ForcedVersions { client_build_number: Some(123_456), electron: Some("45.0.0".into()), chrome: Some("154.0.1.2".into()), ..ForcedVersions::default() };
        let v = Versions::resolve(&newer, &forced);
        assert_eq!((v.client_build_number, v.electron.as_str(), v.chrome_major(), v.client_version.as_str()), (123_456, "45.0.0", 154, "1.0.9300"));
    }

    #[test]
    fn refresh_answers_are_read_defensively() {
        let body = r#"{"full":{"host_version":[1,0,9300],"package_sha256":"x","url":"y"},"deltas":[],"modules":{},"metadata_version":99000,"required_update":true}"#;
        assert_eq!(manifest_versions(body), (Some("1.0.9300".into()), Some(99_000)));
        assert_eq!(manifest_versions(r#"{"full":{"host_version":[1,0]},"metadata_version":null}"#), (None, None));
        assert_eq!(manifest_versions("<html>"), (None, None));
        assert_eq!(build_number(r#"window.GLOBAL_ENV = {"API_ENDPOINT":"//discord.com/api","BUILD_NUMBER":"700000","RELEASE_CHANNEL":"stable"}"#), Some(700_000));
        assert_eq!(build_number(r#""BUILD_NUMBER": "700001""#), Some(700_001));
        assert_eq!(build_number(r#""BUILD_NUMBER":"12""#), None);
        assert_eq!(build_number(r#""BUILD_NUMBER":700000"#), None);
        assert_eq!(build_number("rien"), None);
    }

    #[test]
    fn stale_values_are_noticed() {
        let none = ForcedVersions::default();
        let day0 = embedded_day().unwrap();
        // Le jour du relevé, et trois semaines après : rien à dire.
        let f = Freshness::at(day0, &Fetched::default(), &none);
        assert_eq!((f.embedded_days, f.build_days, f.embedded_stale(), f.build_stale()), (0, 0, false, false));
        assert!(!Freshness::at(day0 + BUILD_STALE_DAYS, &Fetched::default(), &none).build_stale());
        // Sans relevé réussi depuis plus de trois semaines : build périmé.
        let f = Freshness::at(day0 + 30, &Fetched::default(), &none);
        assert!(f.build_stale() && !f.embedded_stale());
        // Relevé réussi la veille : le build est frais, mais Electron vieillit quand même.
        let fresh = Fetched { build_at: (day0 + 89) * 86_400, client_build_number: Some(700_000), ..Fetched::default() };
        let f = Freshness::at(day0 + 90, &fresh, &none);
        assert_eq!((f.build_days, f.build_stale(), f.embedded_stale()), (1, false, true));
        assert_eq!(f.versions.client_build_number, 700_000);
        // Horloge en arrière ou relevé daté du futur : jamais d'âge négatif.
        let f = Freshness::at(day0 - 5, &Fetched { build_at: (day0 + 50) * 86_400, ..Fetched::default() }, &none);
        assert_eq!((f.embedded_days, f.build_days), (0, 0));
        // Valeurs imposées dans local.toml : pas d'avertissement.
        let forced = ForcedVersions { electron: Some("45.0.0".into()), ..ForcedVersions::default() };
        let f = Freshness::at(day0 + 400, &Fetched::default(), &forced);
        assert!(f.forced && !f.embedded_stale() && !f.build_stale());
    }

    /// Le relevé du jour, pour ne pas laisser dériver les valeurs embarquées sans le voir.
    #[test]
    fn embedded_versions_are_well_formed() {
        let v = Versions::embedded();
        assert!(host_version(&v.client_version) && dotted_version(&v.electron) && dotted_version(&v.chrome));
        assert!(BUILD_RANGE.contains(&v.client_build_number) && NATIVE_RANGE.contains(&v.native_build_number));
        assert_eq!(EMBEDDED_ON.len(), 10);
    }
}
