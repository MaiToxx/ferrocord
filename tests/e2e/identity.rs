//! Identité annoncée (décision de l'utilisateur du 14 novembre 2026) : en mode
//! bureau, la passerelle, l'API et le CDN annoncent le même client de
//! bureau ; en mode Ferrocord, rien ne change par rapport à avant.

use crate::support::*;
use ferrocord::net::cdn::Cdn;
use ferrocord::net::identity::{self, Config, Fetched, Machine, Refresh, Versions, FERROCORD_UA};
use ferrocord::net::Endpoints;
use ferrocord::state::{Event, Status};
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::support::ids::GENERAL;

/// En-têtes que seul le mode bureau envoie.
const DESKTOP_ONLY: [&str; 12] = [
    "x-super-properties",
    "x-discord-locale",
    "x-discord-timezone",
    "accept-language",
    "referer",
    "origin",
    "sec-ch-ua",
    "sec-ch-ua-mobile",
    "sec-ch-ua-platform",
    "sec-fetch-dest",
    "sec-fetch-mode",
    "sec-fetch-site",
];

fn machine() -> Machine {
    Machine { os_version: "10.0.26100".into(), os_sdk_version: "26100".into(), os_arch: "x64", app_arch: "x64", system_locale: "fr-FR".into(), timezone: Some("Europe/Paris".into()) }
}

fn desktop() -> Config {
    Config::desktop(Versions::embedded(), machine())
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

fn temp_file(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ferrocord-e2e-identite-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("identite.json")
}

/// Hello → Identify → READY avec ces adresses ; rend aussi les en-têtes d'ouverture.
async fn connect(token: &str, ready: &str, ep: impl FnOnce(&FakeGateway, &FakeHttp) -> Endpoints) -> (Setup, Vec<(String, String)>) {
    let mut gw = FakeGateway::start().await;
    let http = FakeHttp::start().await;
    let mut client = Client::start(token, ep(&gw, &http));
    let mut conn = gw.accept().await;
    let opening = conn.headers.clone();
    let identify = conn.hello_identify(QUIET).await;
    conn.send(ready_with_resume(&gw, ready)).await;
    client.until("READY", |e| matches!(e, Event::Ready { .. })).await;
    client.until("statut Connecté", |e| matches!(e, Event::Status(Status::Connected))).await;
    (Setup { gw, http, client, conn, identify }, opening)
}

#[tokio::test]
async fn desktop_identity_is_the_same_on_the_gateway_the_api_and_the_cdn() {
    let (mut s, opening) = connect(USER_TOKEN, "ready_user_classic", |gw, http| Endpoints { identity: desktop(), ..endpoints(gw, http) }).await;

    // Passerelle : propriétés du client de bureau, et rien d'autre de nouveau dans l'Identify.
    let p = &s.identify["properties"];
    assert_eq!(s.identify["token"], USER_TOKEN);
    assert_eq!(s.identify["compress"], false);
    assert_eq!(s.identify["large_threshold"], 50);
    assert_eq!(s.identify.as_object().map(|o| o.len()), Some(4), "ni capabilities, ni presence, ni client_state : {}", s.identify);
    assert_eq!((&p["os"], &p["browser"], &p["release_channel"]), (&json!("Windows"), &json!("Discord Client"), &json!("stable")));
    assert_eq!((&p["os_version"], &p["os_sdk_version"], &p["system_locale"]), (&json!("10.0.26100"), &json!("26100"), &json!("fr-FR")));
    assert!(p.get("device").is_none(), "pas de champ de l'ancienne identité");
    let ua = p["browser_user_agent"].as_str().unwrap().to_string();
    assert!(ua.contains(&format!("discord/{}", p["client_version"].as_str().unwrap())) && ua.contains(&format!("Electron/{}", p["browser_version"].as_str().unwrap())), "{ua}");
    assert!(!s.identify.to_string().to_lowercase().contains("ferrocord"));
    assert!(!p.to_string().contains(USER_TOKEN), "le jeton n'est que dans `token`");

    // Ouverture du WebSocket : même User-Agent, origine du client.
    assert_eq!(header(&opening, "user-agent"), Some(ua.as_str()));
    assert_eq!(header(&opening, "origin"), Some("https://discord.com"));
    assert_eq!(header(&opening, "accept-language"), Some("fr-FR,fr;q=0.9"));

    // API, lecture : mêmes propriétés, encodées ; pas d'Origin sur un GET.
    s.http.on("GET", &messages_path(GENERAL), [Response::ok(json!([]))]);
    s.client.store.select_channel(GENERAL);
    s.client.flush();
    let get = s.http.next_request().await;
    assert_eq!(get.authorization.as_deref(), Some(USER_TOKEN));
    assert_eq!(header(&get.headers, "user-agent"), Some(ua.as_str()));
    let encoded = header(&get.headers, "x-super-properties").unwrap();
    let decoded = String::from_utf8(ferrocord::state::usersettings::b64::decode(encoded).unwrap()).unwrap();
    let sp: Value = serde_json::from_str(&decoded).unwrap();
    let mut gateway_props = p.clone();
    for only_gateway in ["is_fast_connect", "gateway_connect_reasons"] {
        assert!(gateway_props.as_object_mut().unwrap().remove(only_gateway).is_some(), "{only_gateway}");
    }
    assert_eq!(sp, gateway_props, "X-Super-Properties = propriétés de l'Identify");
    assert!(!decoded.contains(USER_TOKEN));
    assert_eq!(header(&get.headers, "x-discord-locale"), Some("fr"));
    assert_eq!(header(&get.headers, "x-discord-timezone"), Some("Europe/Paris"));
    assert_eq!(header(&get.headers, "referer"), Some("https://discord.com/channels/@me"));
    assert_eq!(header(&get.headers, "sec-ch-ua-platform"), Some("\"Windows\""));
    assert_eq!(header(&get.headers, "origin"), None);

    // API, écriture : mêmes en-têtes, avec l'origine.
    s.http.on("POST", &messages_path(GENERAL), [Response::ok(json!({ "id": "5000000000000009001", "channel_id": GENERAL.to_string(), "content": "bonjour", "author": { "id": "1000000000000000001", "username": "moi_fictif" } }))]);
    s.client.post(GENERAL, "bonjour");
    let post = loop {
        let r = s.http.next_request().await;
        if r.method == "POST" {
            break r;
        }
    };
    assert_eq!(header(&post.headers, "origin"), Some("https://discord.com"));
    assert_eq!(header(&post.headers, "x-super-properties"), Some(encoded));
    assert_eq!(header(&post.headers, "user-agent"), Some(ua.as_str()));

    // Aucun en-tête ne porte le nom de Ferrocord ; le jeton n'est que dans Authorization.
    for r in [&get, &post] {
        for (name, value) in &r.headers {
            assert!(!value.to_lowercase().contains("ferrocord"), "{name}: {value}");
            assert!(name == "authorization" || !value.contains(USER_TOKEN), "{name}");
        }
    }

    // CDN : même User-Agent, sans jeton ni propriétés.
    s.http.on("GET", "/cdn/avatars/1/a.png", [Response::bytes(200, vec![1, 2, 3], "image/png")]);
    let cdn = Cdn::from_endpoints(&Endpoints { identity: desktop(), ..endpoints(&s.gw, &s.http) });
    assert!(cdn.fetch(&format!("{}/avatars/1/a.png", s.http.cdn())).await.is_ok());
    let img = s.http.next_request().await;
    assert_eq!(header(&img.headers, "user-agent"), Some(ua.as_str()));
    assert!(img.authorization.is_none() && header(&img.headers, "x-super-properties").is_none());
}

#[tokio::test]
async fn ferrocord_identity_is_what_it_was() {
    let (mut s, opening) = connect(USER_TOKEN, "ready_user_classic", endpoints).await;
    assert_eq!(
        s.identify,
        json!({ "token": USER_TOKEN, "properties": { "os": std::env::consts::OS, "browser": "Ferrocord", "device": "Ferrocord" }, "compress": false, "large_threshold": 50 })
    );
    for absent in ["user-agent", "origin", "accept-language"] {
        assert_eq!(header(&opening, absent), None, "ouverture du WebSocket : pas de {absent}");
    }

    s.http.on("GET", &messages_path(GENERAL), [Response::ok(json!([]))]);
    s.http.on("POST", &messages_path(GENERAL), [Response::ok(json!({ "id": "5000000000000009001", "channel_id": GENERAL.to_string(), "content": "bonjour", "author": { "id": "1000000000000000001", "username": "moi_fictif" } }))]);
    s.client.store.select_channel(GENERAL);
    s.client.flush();
    let get = s.http.next_request().await;
    s.client.post(GENERAL, "bonjour");
    let post = loop {
        let r = s.http.next_request().await;
        if r.method == "POST" {
            break r;
        }
    };
    for r in [&get, &post] {
        assert_eq!(header(&r.headers, "user-agent"), Some(FERROCORD_UA));
        assert_eq!(FERROCORD_UA, concat!("Ferrocord/", env!("CARGO_PKG_VERSION")));
        for absent in DESKTOP_ONLY {
            assert_eq!(header(&r.headers, absent), None, "{} {} : pas de {absent}", r.method, r.path);
        }
    }

    s.http.on("GET", "/cdn/avatars/1/a.png", [Response::bytes(200, vec![1, 2, 3], "image/png")]);
    let cdn = Cdn::from_endpoints(&endpoints(&s.gw, &s.http));
    assert!(cdn.fetch(&format!("{}/avatars/1/a.png", s.http.cdn())).await.is_ok());
    assert_eq!(header(&s.http.next_request().await.headers, "user-agent"), Some(FERROCORD_UA));
}

/// Un bot ne se présente jamais comme le client de bureau, même si le réglage le demande.
#[tokio::test]
async fn a_bot_keeps_the_ferrocord_identity() {
    let (mut s, opening) = connect(BOT_TOKEN, "ready_bot", |gw, http| Endpoints { identity: desktop(), ..endpoints(gw, http) }).await;
    assert_eq!(s.identify["properties"], json!({ "os": std::env::consts::OS, "browser": "Ferrocord", "device": "Ferrocord" }));
    assert!(s.identify["intents"].is_u64());
    assert_eq!(header(&opening, "user-agent"), None);
    s.http.on("GET", &messages_path(GENERAL), [Response::ok(json!([]))]);
    s.client.send(ferrocord::state::Cmd::History { channel_id: GENERAL, before: None });
    let r = s.http.next_request().await;
    assert_eq!(header(&r.headers, "user-agent"), Some(FERROCORD_UA));
    assert_eq!(header(&r.headers, "x-super-properties"), None);
}

const MANIFEST: &str = r#"{"full":{"host_version":[1,0,9999],"package_sha256":"00","url":"https://exemple.invalid/full.distro"},"deltas":[],"modules":{},"required_modules":[],"metadata_version":123456,"required_update":false}"#;
const LOGIN_PAGE: &str = r#"<!DOCTYPE html><html><head><script nonce="x">window.GLOBAL_ENV = {"API_ENDPOINT":"//discord.com/api","RELEASE_CHANNEL":"stable","BUILD_NUMBER":"999999","VERSION_HASH":"00"}</script></head><body></body></html>"#;

fn refresh_of(http: &FakeHttp, cache: PathBuf) -> Refresh {
    Refresh::new(cache, format!("{}/manifests/latest", http.cdn()), format!("{}/login", http.cdn()))
}

#[tokio::test]
async fn versions_are_refreshed_once_a_day_without_the_token() {
    let mut http = FakeHttp::start().await;
    http.on("GET", "/cdn/manifests/latest", [Response::bytes(200, MANIFEST.as_bytes().to_vec(), "application/json")]);
    http.on("GET", "/cdn/login", [Response::bytes(200, LOGIN_PAGE.as_bytes().to_vec(), "text/html")]);
    let cache = temp_file("jour");

    identity::refresh(refresh_of(&http, cache.clone()), desktop()).await;
    let manifest = http.next_request().await;
    let login = http.next_request().await;
    assert!(manifest.path.starts_with("/cdn/manifests/latest?install_id="), "{}", manifest.path);
    assert!(manifest.path.ends_with("&channel=stable&platform=win&arch=x64"), "{}", manifest.path);
    assert_eq!(login.path, "/cdn/login");
    let ua = desktop().user_agent();
    for r in [&manifest, &login] {
        assert!(r.authorization.is_none(), "sans jeton");
        assert_eq!(header(&r.headers, "user-agent"), Some(ua.as_str()));
        assert_eq!(header(&r.headers, "x-super-properties"), None);
    }

    let got = Fetched::read(&cache);
    assert_eq!((got.client_version.as_deref(), got.client_build_number, got.native_build_number), (Some("1.0.9999"), Some(999_999), Some(123_456)));
    assert!(got.tried_at > 0 && manifest.path.contains(got.install_id.as_deref().unwrap()));
    // Le prochain lancement annonce ces numéros, partout.
    let v = Versions::resolve(&got, &Default::default());
    assert_eq!((v.client_version.as_str(), v.client_build_number, v.native_build_number), ("1.0.9999", 999_999, 123_456));
    assert!(Config::desktop(v, machine()).user_agent().contains("discord/1.0.9999 "));

    // Le même jour (autre lancement, ou même lancement) : rien ne repart.
    identity::refresh(refresh_of(&http, cache.clone()), desktop()).await;
    assert_eq!(http.requests().len(), 2);

    // Mode Ferrocord : jamais rien, même sans cache.
    identity::refresh(refresh_of(&http, temp_file("honnete")), Config::default()).await;
    assert_eq!(http.requests().len(), 2);
}

#[tokio::test]
async fn a_failed_refresh_keeps_what_was_known_and_waits_a_day() {
    let http = FakeHttp::start().await;
    http.on("GET", "/cdn/manifests/latest", [Response::error(500, "panne")]);
    http.on("GET", "/cdn/login", [Response::bytes(200, b"<html>maintenance</html>".to_vec(), "text/html")]);
    let cache = temp_file("panne");
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    // Dernier essai il y a plus d'un jour.
    std::fs::write(&cache, r#"{"tried_at":1000,"client_version":"1.0.9500","client_build_number":800000,"native_build_number":99999}"#).unwrap();

    identity::refresh(refresh_of(&http, cache.clone()), desktop()).await;
    assert_eq!(http.requests().len(), 2);
    let got = Fetched::read(&cache);
    assert_eq!((got.client_version.as_deref(), got.client_build_number, got.native_build_number), (Some("1.0.9500"), Some(800_000), Some(99_999)));
    assert!(got.tried_at > 1000, "l'essai raté compte pour aujourd'hui");
    identity::refresh(refresh_of(&http, cache), desktop()).await;
    assert_eq!(http.requests().len(), 2, "pas de nouvel essai avant demain");
}

/// La connexion elle-même lance le rafraîchissement (mode bureau), sans jeton.
#[tokio::test]
async fn connecting_in_desktop_mode_asks_for_the_versions() {
    let cache = temp_file("connexion");
    let c = cache.clone();
    let (mut s, _) = connect(USER_TOKEN, "ready_user_classic", move |gw, http| {
        http.on("GET", "/cdn/manifests/latest", [Response::bytes(200, MANIFEST.as_bytes().to_vec(), "application/json")]);
        http.on("GET", "/cdn/login", [Response::bytes(200, LOGIN_PAGE.as_bytes().to_vec(), "text/html")]);
        Endpoints { identity: desktop().with_refresh(refresh_of(http, c)), ..endpoints(gw, http) }
    })
    .await;
    for _ in 0..2 {
        let r = s.http.next_request().await;
        assert!(r.path.starts_with("/cdn/"), "{}", r.path);
        assert!(r.authorization.is_none());
    }
    within("cache écrit", async {
        while Fetched::read(&cache).client_build_number != Some(999_999) {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await;
    // La connexion en cours garde les numéros du lancement.
    assert_eq!(s.identify["properties"]["client_build_number"], Versions::embedded().client_build_number);
}
