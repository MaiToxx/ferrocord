//! Images : empreintes reçues, CDN local, en-têtes envoyés, limites, cache disque.

use crate::support::{ids::*, *};
use eframe::egui;
use ferrocord::images::{Config, DiskConfig, Image, Images, Stats};
use ferrocord::net::cdn::{Cdn, CdnHosts, FetchError, Limits};
use ferrocord::state::model::{ImageHash, ImageRef};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

/// Garde-fou des attentes d'état (jamais une synchronisation) : large, pour
/// qu'une machine très chargée ne fasse pas échouer un test correct.
const GUARD: Duration = Duration::from_secs(60);

/// Empreintes des jeux de données.
const ALICE_AVATAR: &str = "a11ce0000000000000000000000000a1";
const ALPHA_ICON: &str = "a1fa0000000000000000000000000a1f";
const MY_AVATAR: &str = "0e010000000000000000000000000e01";

fn hash(s: &str) -> ImageHash {
    ImageHash::parse(s).unwrap_or_else(|| panic!("empreinte {s}"))
}

/// PNG synthétique.
fn png(side: u32, seed: u8) -> Vec<u8> {
    let img = image::RgbaImage::from_fn(side, side, |x, y| image::Rgba([seed, (x * 2) as u8, (y * 2) as u8, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img).write_to(&mut out, image::ImageFormat::Png).unwrap_or_else(|e| panic!("{e}"));
    out.into_inner()
}

/// Chemin servi par le faux CDN pour un avatar WebP de taille `size`.
fn avatar_path(user: u64, h: &str, size: u32) -> (String, String) {
    (format!("/cdn/avatars/{user}/{h}.webp"), format!("/cdn/avatars/{user}/{h}.webp?size={size}"))
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ferrocord-e2e-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn start_images(http: &FakeHttp, disk: Option<DiskConfig>) -> Images {
    let cdn = Cdn::new(http.cdn(), CdnHosts::Loopback, Limits::default());
    Images::start(egui::Context::default(), &tokio::runtime::Handle::current(), Arc::new(cdn), disk, Config::default(), Arc::new(|| {}))
}

/// Images successives affichant `refs`, jusqu'à ce que toutes soient prêtes ou en échec.
async fn settle(images: &Images, refs: &[(ImageRef, f32)]) -> Vec<Image> {
    let deadline = Instant::now() + GUARD;
    loop {
        images.begin_frame();
        let got: Vec<Image> = refs.iter().map(|(r, s)| images.get(*r, *s)).collect();
        images.end_frame();
        if got.iter().all(|i| *i != Image::Pending) {
            return got;
        }
        assert!(Instant::now() < deadline, "délai dépassé : {got:?}");
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

#[tokio::test]
async fn hashes_reach_the_store() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let me = s.client.store.me.clone().unwrap_or_else(|| panic!("moi"));
    assert_eq!(me.avatar.image, Some(ImageRef::Avatar { user_id: ME, hash: hash(MY_AVATAR) }));
    let alpha = s.client.store.guilds.iter().find(|g| g.id == ALPHA).and_then(|g| g.icon);
    assert_eq!(alpha, Some(ImageRef::GuildIcon { guild_id: ALPHA, hash: hash(ALPHA_ICON) }));
    assert_eq!(s.client.store.guilds.iter().find(|g| g.id == BETA).and_then(|g| g.icon), None);
    let dm = s.client.store.dms.iter().find(|d| d.id == DM_ALICE).map(|d| d.avatar.image);
    assert_eq!(dm, Some(Some(ImageRef::Avatar { user_id: ALICE, hash: hash(ALICE_AVATAR) })));

    // Historique REST et message en direct : avatar de l'auteur.
    s.http.on("GET", &messages_path(GENERAL), [Response::ok(fixture("rest_history"))]);
    s.client.store.select_channel(GENERAL);
    s.client.flush();
    s.client.until("historique", |e| matches!(e, ferrocord::state::Event::History { .. })).await;
    s.conn.dispatch_fixture("message_create_simple", 2).await;
    s.client.until("message", |e| matches!(e, ferrocord::state::Event::MessageCreate(_))).await;
    let msgs = &s.client.store.chans[&GENERAL].msgs;
    let alice: Vec<_> = msgs.iter().filter(|m| m.author_id == ALICE).map(|m| m.author_avatar.image).collect();
    assert_eq!(alice.len(), 2);
    assert!(alice.iter().all(|a| *a == Some(ImageRef::Avatar { user_id: ALICE, hash: hash(ALICE_AVATAR) })));
    // Bob n'a pas d'avatar : avatar par défaut, sans image.
    assert!(msgs.iter().filter(|m| m.author_id == 1000000000000000003).all(|m| m.author_avatar.image.is_none()));

    // GUILD_UPDATE : icône retirée.
    let mut upd = fixture("guild_update");
    upd["d"]["icon"] = serde_json::Value::Null;
    upd["s"] = 3.into();
    s.conn.send(upd).await;
    s.client.until("GUILD_UPDATE", |e| matches!(e, ferrocord::state::Event::GuildUpdate { .. })).await;
    assert_eq!(s.client.store.guilds.iter().find(|g| g.id == ALPHA).and_then(|g| g.icon), None);
}

/// Le jeton n'est jamais envoyé au CDN : ni `Authorization`, ni cookie, ni
/// aucun en-tête qui le contienne. Les requêtes de l'API, elles, le portent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cdn_requests_carry_no_token() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &messages_path(GENERAL), [Response::ok(fixture("rest_history"))]);
    s.client.store.select_channel(GENERAL);
    s.client.flush();
    s.client.until("historique", |e| matches!(e, ferrocord::state::Event::History { .. })).await;

    let (route, full) = avatar_path(ALICE, ALICE_AVATAR, 64);
    s.http.on("GET", &route, [Response::bytes(200, png(64, 1), "image/webp").header("set-cookie", "suivi=1; Path=/")]);
    let (route2, _) = avatar_path(ME, MY_AVATAR, 64);
    s.http.on("GET", &route2, [Response::bytes(200, png(64, 2), "image/webp")]);
    let images = start_images(&s.http, None);
    let alice = ImageRef::Avatar { user_id: ALICE, hash: hash(ALICE_AVATAR) };
    let me = ImageRef::Avatar { user_id: ME, hash: hash(MY_AVATAR) };
    // Deux images l'une après l'autre : la seconde passerait un cookie reçu avec la première.
    assert!(matches!(settle(&images, &[(alice, 40.0)]).await[..], [Image::Ready(_)]));
    assert!(matches!(settle(&images, &[(me, 40.0)]).await[..], [Image::Ready(_)]));

    let reqs = s.http.requests();
    let api: Vec<_> = reqs.iter().filter(|r| r.path.starts_with("/api/")).collect();
    assert!(!api.is_empty() && api.iter().all(|r| r.authorization.as_deref() == Some(USER_TOKEN)), "témoin : l'API porte le jeton");
    let cdn: Vec<_> = reqs.iter().filter(|r| r.path.starts_with("/cdn/")).collect();
    assert_eq!(cdn.len(), 2, "{cdn:#?}");
    assert_eq!(cdn[0].path, full);
    for r in cdn {
        assert_eq!(r.method, "GET");
        assert_eq!(r.authorization, None);
        for (name, value) in &r.headers {
            assert!(!matches!(name.as_str(), "authorization" | "cookie" | "proxy-authorization"), "en-tête {name} envoyé au CDN");
            assert!(!value.contains(USER_TOKEN) && !value.contains("faux.jeton"), "jeton dans {name}");
        }
        let names: Vec<&str> = r.headers.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.iter().all(|n| ["host", "user-agent", "accept"].contains(n)), "en-têtes inattendus : {names:?}");
    }
}

/// Image jointe : demandée au proxy à la taille affichée (paramètres de
/// signature gardés, `width` et `height` ajoutés), sans jeton ; une image
/// servie bien plus grande que demandé est refusée avant décodage.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attachments_are_requested_resized() {
    let http = FakeHttp::start().await;
    http.on("GET", "/cdn/attachments/1/2/a.png", [Response::bytes(200, png(160, 4), "image/png")]);
    http.on("GET", "/cdn/attachments/1/3/b.png", [Response::bytes(200, png(2048, 5), "image/png")]);
    let images = start_images(&http, None);
    let refs = [(ImageRef::Media { owner: 2, slot: 0, rev: 0 }, "/cdn/attachments/1/2/a.png"), (ImageRef::Media { owner: 3, slot: 0, rev: 0 }, "/cdn/attachments/1/3/b.png")];
    let urls: Vec<String> = refs.iter().map(|(_, p)| format!("{}{}?ex=0a&hm=0b", http.cdn(), p.trim_start_matches("/cdn"))).collect();
    let size = egui::vec2(160.0, 90.0);
    let deadline = Instant::now() + GUARD;
    let got = loop {
        images.begin_frame();
        let got: Vec<Image> = refs.iter().zip(&urls).map(|((r, _), u)| images.get_media(*r, u, size)).collect();
        images.end_frame();
        if got.iter().all(|i| *i != Image::Pending) {
            break got;
        }
        assert!(Instant::now() < deadline, "délai dépassé : {got:?}");
        tokio::time::sleep(Duration::from_millis(2)).await;
    };
    assert!(matches!(got[0], Image::Ready(_)), "{got:?}");
    assert_eq!(got[1], Image::Failed, "2048 px servis pour 160 demandés : refusée");
    let reqs = http.requests();
    let mut paths: Vec<&str> = reqs.iter().map(|r| r.path.as_str()).collect();
    paths.sort_unstable();
    assert_eq!(paths, vec!["/cdn/attachments/1/2/a.png?ex=0a&hm=0b&width=160&height=90", "/cdn/attachments/1/3/b.png?ex=0a&hm=0b&width=160&height=90"]);
    assert!(reqs.iter().all(|r| r.authorization.is_none()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn not_found_is_requested_once() {
    let mut http = FakeHttp::start().await;
    let images = start_images(&http, None);
    let alice = ImageRef::Avatar { user_id: ALICE, hash: hash(ALICE_AVATAR) };
    // Aucune route : 404.
    assert_eq!(settle(&images, &[(alice, 40.0)]).await, vec![Image::Failed]);
    for _ in 0..20 {
        settle(&images, &[(alice, 40.0), (alice, 41.0)]).await;
    }
    let _ = http.next_request().await;
    assert_eq!(http.requests().len(), 1);
}

#[tokio::test]
async fn fetch_limits_and_refusals() {
    let http = FakeHttp::start().await;
    let limits = Limits { max_bytes: 100_000, timeout: Duration::from_millis(500) };
    let cdn = Cdn::new(http.cdn(), CdnHosts::Loopback, limits);
    let url = |p: &str| format!("{}{p}", http.cdn());
    http.on("GET", "/cdn/ok.png", [Response::bytes(200, png(16, 3), "image/png")]);
    http.on("GET", "/cdn/gros.png", [Response::bytes(200, vec![0; 200_000], "image/png")]);
    http.on("GET", "/cdn/flot.png", [Response::streamed(50 << 20)]);
    http.on("GET", "/cdn/redirige.png", [Response::bytes(302, Vec::new(), "text/plain").header("location", "/api/v10/users/@me")]);
    http.on("GET", "/cdn/lent.png", [Response::bytes(200, png(16, 3), "image/png").delayed(Duration::from_secs(60))]);
    http.on("GET", "/cdn/panne.png", [Response::bytes(503, Vec::new(), "text/plain")]);

    assert!(cdn.fetch(&url("/ok.png")).await.is_ok_and(|b| !b.is_empty()));
    // Longueur annoncée trop grande : refus sans lire le corps.
    assert!(matches!(cdn.fetch(&url("/gros.png")).await, Err(FetchError::Refused(_))));
    // Sans longueur annoncée : lecture arrêtée au plafond (50 Mo envoyés,
    // refus dès que 100 ko sont dépassés ; aucune durée n'est vérifiée).
    assert!(matches!(cdn.fetch(&url("/flot.png")).await, Err(FetchError::Refused(_))));
    // Redirection : jamais suivie (elle pourrait mener ailleurs).
    assert!(matches!(cdn.fetch(&url("/redirige.png")).await, Err(FetchError::Refused(_))));
    assert!(http.requests().iter().all(|r| !r.path.starts_with("/api/")), "redirection suivie");
    // Délai maximal : 500 ms pour une réponse qui en prendrait 60 (marge
    // telle qu'aucune charge de la machine ne peut inverser le résultat).
    assert!(matches!(cdn.fetch(&url("/lent.png")).await, Err(FetchError::Network(_))));
    assert!(matches!(cdn.fetch(&url("/panne.png")).await, Err(FetchError::Network(_))));
    assert_eq!(cdn.fetch(&url("/absent.png")).await, Err(FetchError::NotFound));
    // Adresse hors du CDN autorisé : refusée sans requête.
    let before = http.requests().len();
    for bad in ["http://localhost:1/x.png", "https://cdn.discordapp.com/x.png", "http://127.0.0.2/x.png", "file:///etc/passwd"] {
        assert!(matches!(cdn.fetch(bad).await, Err(FetchError::Refused(_))), "{bad}");
    }
    assert_eq!(http.requests().len(), before);
    // Règle de production : rien d'autre que le CDN de Discord en https.
    let prod = Cdn::new(http.cdn(), CdnHosts::Discord, Limits::default());
    assert!(matches!(prod.fetch(&url("/ok.png")).await, Err(FetchError::Refused(_))));
    assert_eq!(http.requests().len(), before);
}

/// Images hostiles servies par le CDN : échec propre, sans panique, et
/// l'adresse n'est pas redemandée.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hostile_images_fail_cleanly() {
    let http = FakeHttp::start().await;
    let images = start_images(&http, None);
    let good = png(64, 9);
    let mut bomb = good[..16].to_vec();
    bomb.extend_from_slice(&100_000u32.to_be_bytes());
    bomb.extend_from_slice(&100_000u32.to_be_bytes());
    bomb.extend_from_slice(&good[24..]);
    let bodies: Vec<(u64, Vec<u8>, &str)> = vec![
        (1, bomb, "image/png"),
        (2, good[..good.len() / 2].to_vec(), "image/png"),
        (3, b"<!doctype html><p>pas une image</p>".to_vec(), "image/webp"),
        (4, Vec::new(), "image/webp"),
        (5, good.clone(), "image/webp"),
    ];
    let mut refs = Vec::new();
    for (user, body, ctype) in bodies {
        let h = ImageHash::synthetic(user);
        http.on("GET", &format!("/cdn/avatars/{user}/{h}.webp"), [Response::bytes(200, body, ctype)]);
        refs.push((ImageRef::Avatar { user_id: user, hash: h }, 40.0));
    }
    let got = settle(&images, &refs).await;
    assert_eq!(&got[..4], &[Image::Failed; 4]);
    assert!(matches!(got[4], Image::Ready(_)), "image saine malgré les autres");
    let n = http.requests().len();
    for _ in 0..10 {
        settle(&images, &refs).await;
    }
    assert_eq!(http.requests().len(), n, "pas de nouvel essai");
}

/// Relancé avec le même dossier : l'image vient du disque, sans requête.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disk_cache_survives_restart() {
    let dir = temp_dir("disque");
    let alice = ImageRef::Avatar { user_id: ALICE, hash: hash(ALICE_AVATAR) };
    let http = FakeHttp::start().await;
    let (route, _) = avatar_path(ALICE, ALICE_AVATAR, 64);
    http.on("GET", &route, [Response::bytes(200, png(64, 4), "image/webp")]);
    {
        let images = start_images(&http, Some(DiskConfig::new(dir.clone())));
        assert!(matches!(settle(&images, &[(alice, 40.0)]).await[..], [Image::Ready(_)]));
        assert_eq!(http.requests().len(), 1);
    }
    let files: Vec<String> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(!files[0].contains("avatars") && !files[0].contains(ALICE_AVATAR), "nom dérivé d'une empreinte : {}", files[0]);

    // Nouvelle instance (redémarrage), même dossier : aucune requête.
    let images = start_images(&http, Some(DiskConfig::new(dir.clone())));
    assert!(matches!(settle(&images, &[(alice, 40.0)]).await[..], [Image::Ready(_)]));
    assert_eq!(http.requests().len(), 1, "servie par le disque");
    assert_eq!(images.stats(), Stats { bytes: 40 * 40 * 4, textures: 1, pending: 0, failed: 0 });

    // Fichier abîmé sur le disque : toléré, l'image est retéléchargée.
    let path = dir.join(&files[0]);
    std::fs::write(&path, b"FCI1\x05\0\0\0abc").unwrap_or_else(|e| panic!("{e}"));
    let images = start_images(&http, Some(DiskConfig::new(dir.clone())));
    assert!(matches!(settle(&images, &[(alice, 40.0)]).await[..], [Image::Ready(_)]));
    assert_eq!(http.requests().len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 429 du CDN : tout l'ordonnanceur s'arrête pendant `Retry-After` (aucune
/// autre requête d'image ne part, même pour des images nouvelles), puis
/// reprend une requête à la fois avant de remonter.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rate_limit_pauses_every_image() {
    let http = FakeHttp::start().await;
    let images = start_images(&http, None);
    let users: Vec<u64> = (1..=6).collect();
    for &u in &users {
        let h = ImageHash::synthetic(u);
        let route = format!("/cdn/avatars/{u}/{h}.webp");
        let ok = Response::bytes(200, png(64, u as u8), "image/webp");
        if u == 1 {
            http.on("GET", &route, [Response::bytes(429, Vec::new(), "text/plain").header("retry-after", "1"), ok]);
        } else {
            http.on("GET", &route, [ok]);
        }
    }
    let refs: Vec<(ImageRef, f32)> = users.iter().map(|&u| (ImageRef::Avatar { user_id: u, hash: ImageHash::synthetic(u) }, 40.0)).collect();
    // La première seule, jusqu'à la pause.
    let deadline = Instant::now() + GUARD;
    while !images.worker_stats().paused {
        assert!(Instant::now() < deadline, "pas de pause");
        images.begin_frame();
        let _ = images.get(refs[0].0, refs[0].1);
        images.end_frame();
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    // Pendant la pause, toutes affichées : elles attendent.
    let got = settle(&images, &refs).await;
    assert!(got.iter().all(|i| matches!(i, Image::Ready(_))), "{got:?}");

    let reqs = http.requests();
    let cdn: Vec<_> = reqs.iter().filter(|r| r.path.starts_with("/cdn/")).collect();
    assert_eq!(cdn.len(), 7, "une 429, puis chaque image une fois : {cdn:#?}");
    let limited = cdn[0].received_at;
    for r in &cdn[1..] {
        assert!(r.received_at.duration_since(limited) >= Duration::from_secs(1), "requête pendant la pause : {} après {:?}", r.path, r.received_at.duration_since(limited));
    }
    // Reprise prudente : les deux premières requêtes l'une après l'autre.
    assert_eq!((cdn[1].in_flight, cdn[2].in_flight), (0, 0), "{cdn:#?}");
    assert!(!images.worker_stats().paused);
}
