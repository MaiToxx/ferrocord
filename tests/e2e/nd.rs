//! Lot ND face au faux serveur : effets de profil et quêtes (routes non
//! documentées, formes de docs.discord.food). Données synthétiques.

use crate::support::{ids::*, *};
use ferrocord::state::effects::EffectEvent;
use ferrocord::state::profiles::ProfileState;
use ferrocord::state::quests::{QuestEvent, Refused, Stage};
use ferrocord::state::usersettings::{AccountEvent, ProfileEdit, Remote};
use ferrocord::state::Event;
use serde_json::json;

const EFFECT: u64 = 6000000000000000001;
const QUEST: u64 = 8000000000000000001;
const CONSOLE_QUEST: u64 = 8000000000000000003;
const QUESTS: &str = "/api/v10/quests/@me";

const CATALOG: &str = "/api/v10/collectibles-categories";
/// Effet dont l'identifiant n'est pas le SKU de son produit (cas réel).
const AURORA: u64 = 6000000000000000301;
const AURORA_SKU: u64 = 6000000000000000302;
const EMBER: u64 = 6000000000000000311;

fn plain(r: &Request) {
    assert_eq!(r.authorization.as_deref(), Some(USER_TOKEN));
}

/// Attend le profil d'Alice dont `user_profile.profile_effect` vaut `effect`,
/// puis demande son effet comme le fait une carte ouverte.
async fn open_profile_with_effect(s: &mut Setup, effect: serde_json::Value) {
    let mut profile = fixture("rest_user_profile");
    profile["user_profile"]["profile_effect"] = effect;
    s.http.on("GET", &format!("/api/v10/users/{ALICE}/profile"), [Response::ok(profile)]);
    s.client.store.want_profile(ALICE, None);
    s.client.flush();
    s.http.next_request().await;
    within("profil", async {
        loop {
            s.client.drain();
            if s.client.store.people.lock().profiles().profile(ALICE, None).and_then(ProfileState::profile).is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await;
    let (id, sku) = s.client.store.people.lock().profiles().profile(ALICE, None).and_then(ProfileState::profile).and_then(|p| p.effect_ref()).unwrap_or_else(|| panic!("effet du profil absent"));
    s.client.store.want_effect(id, sku);
    s.client.flush();
}

/// Forme documentée (`{ id, expires_at }`) : l'identifiant de l'effet n'est
/// pas un SKU, le produit de ce nom rend 404 ; le catalogue relie l'effet à
/// son produit, demandé ensuite. Trois requêtes, une fois.
#[tokio::test]
async fn effect_id_is_resolved_through_the_catalog() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let by_id = format!("/api/v10/collectibles-products/{AURORA}");
    let by_sku = format!("/api/v10/collectibles-products/{AURORA_SKU}");
    s.http.on("GET", &by_id, [Response::error(404, "Unknown SKU")]);
    s.http.on("GET", CATALOG, [Response::ok(fixture("profil/rest_collectibles_categories_effects"))]);
    s.http.on("GET", &by_sku, [Response::ok(fixture("profil/rest_collectibles_product_effect"))]);
    open_profile_with_effect(&mut s, json!({ "id": AURORA.to_string(), "expires_at": null })).await;
    assert_eq!(s.http.next_request().await.path, by_id);
    s.client.until("pas ce produit", |e| matches!(e, Event::Effects(EffectEvent::Config { result: Ok(None), .. }))).await;
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str(), r.body.is_empty()), ("GET", CATALOG, true));
    plain(&r);
    s.client.until("catalogue", |e| matches!(e, Event::Effects(EffectEvent::Catalog(Ok(_))))).await;
    s.client.flush();
    assert_eq!(s.http.next_request().await.path, by_sku);
    s.client.until("effet", |e| matches!(e, Event::Effects(EffectEvent::Config { result: Ok(Some(_)), .. }))).await;
    let cfg = s.client.store.want_effect(AURORA, None).unwrap_or_else(|| panic!("configuration absente"));
    assert_eq!((cfg.id, cfg.sku_id, cfg.title.as_str(), cfg.layers.len()), (AURORA, AURORA_SKU, "Aurore fictive", 2));
    // Un autre effet du catalogue déjà reçu : son produit est demandé tout de suite par son SKU
    // (404 par défaut : effet absent), sans relire le catalogue.
    s.client.store.want_effect(EMBER, None);
    s.client.flush();
    assert_eq!(s.http.next_request().await.path, "/api/v10/collectibles-products/6000000000000000312");
    s.client.until("pas ce produit", |e| matches!(e, Event::Effects(EffectEvent::Config { result: Ok(None), .. }))).await;
    assert!(s.client.store.want_effect(EMBER, None).is_none());
    assert!(s.client.store.take_commands().is_empty(), "rien de plus n'est demandé");
    assert_eq!(s.http.requests().iter().filter(|r| r.path == CATALOG).count(), 1, "catalogue demandé une seule fois");
}

/// Forme avec `sku_id` : une seule requête, celle du produit.
#[tokio::test]
async fn effect_sku_from_the_profile_needs_one_request() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let by_sku = format!("/api/v10/collectibles-products/{AURORA_SKU}");
    s.http.on("GET", &by_sku, [Response::ok(fixture("profil/rest_collectibles_product_effect"))]);
    let before = s.http.requests().len();
    open_profile_with_effect(&mut s, json!({ "id": AURORA.to_string(), "sku_id": AURORA_SKU.to_string(), "expires_at": null })).await;
    assert_eq!(s.http.next_request().await.path, by_sku);
    s.client.until("effet", |e| matches!(e, Event::Effects(EffectEvent::Config { result: Ok(Some(_)), .. }))).await;
    assert!(s.client.store.want_effect(AURORA, Some(AURORA_SKU)).is_some());
    assert!(s.client.store.take_commands().is_empty());
    assert_eq!(s.http.requests().len() - before, 2, "le profil, puis le produit");
}

/// Un profil qui porte un effet : sa configuration est demandée une fois ;
/// l'édition liste les effets possédés ; le choix part dans `PATCH /users/@me/profile`.
#[tokio::test]
async fn profile_effect_is_fetched_once_listed_and_chosen() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let mut profile = fixture("rest_user_profile");
    profile["user_profile"]["profile_effect"] = json!({ "id": EFFECT.to_string(), "expires_at": null });
    s.http.on("GET", &format!("/api/v10/users/{ALICE}/profile"), [Response::ok(profile)]);
    s.client.store.want_profile(ALICE, None);
    s.client.flush();
    s.http.next_request().await;
    within("profil", async {
        loop {
            s.client.drain();
            if s.client.store.people.lock().profiles().profile(ALICE, None).and_then(ProfileState::profile).is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await;
    let effect = s.client.store.people.lock().profiles().profile(ALICE, None).and_then(ProfileState::profile).and_then(|p| p.effect());
    assert_eq!(effect, Some(EFFECT));

    let product = format!("/api/v10/collectibles-products/{EFFECT}");
    s.http.on("GET", &product, [Response::ok(fixture("nd/rest_collectibles_product"))]);
    assert!(s.client.store.want_effect(EFFECT, None).is_none());
    assert!(s.client.store.want_effect(EFFECT, None).is_none());
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", product.as_str()));
    plain(&r);
    s.client.until("effet", |e| matches!(e, Event::Effects(EffectEvent::Config { .. }))).await;
    let cfg = s.client.store.want_effect(EFFECT, None).unwrap_or_else(|| panic!("configuration absente"));
    assert_eq!((cfg.title.as_str(), cfg.layers.len()), ("Pluie fictive", 2));
    assert_eq!(s.http.requests().iter().filter(|r| r.path == product).count(), 1, "une seule demande");

    // Produit inconnu : le catalogue est consulté (une fois) ; absent de là aussi, il n'est jamais redemandé.
    s.http.on("GET", "/api/v10/collectibles-products/6000000000000000099", [Response::error(404, "Unknown SKU")]);
    s.http.on("GET", CATALOG, [Response::ok(fixture("profil/rest_collectibles_categories_effects"))]);
    s.client.store.want_effect(6000000000000000099, None);
    s.client.flush();
    s.http.next_request().await;
    s.client.until("effet inconnu", |e| matches!(e, Event::Effects(EffectEvent::Config { result: Ok(None), .. }))).await;
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", CATALOG));
    plain(&r);
    s.client.until("catalogue", |e| matches!(e, Event::Effects(EffectEvent::Catalog(_)))).await;
    s.client.store.want_effect(6000000000000000099, None);
    assert!(s.client.store.take_commands().is_empty());

    // Effets possédés : seuls les effets de profil lisibles restent.
    s.http.on("GET", "/api/v10/users/@me/collectibles-purchases", [Response::ok(fixture("nd/rest_collectibles_purchases"))]);
    s.client.store.want_owned_effects();
    s.client.store.want_owned_effects();
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", "/api/v10/users/@me/collectibles-purchases"));
    s.client.until("effets possédés", |e| matches!(e, Event::Effects(EffectEvent::Owned(_)))).await;
    let Remote::Ready(owned) = &s.client.store.effects.owned else { panic!("effets possédés absents") };
    assert_eq!(owned, &[EFFECT, 6000000000000000002]);

    // Choix, puis retrait : `profile_effect_id` seul dans le corps.
    s.http.on("GET", "/api/v10/users/@me", [Response::ok(fixture("rest_me"))]);
    s.client.store.account.load_profile();
    s.client.flush();
    s.http.next_request().await;
    s.client.until("compte", |e| matches!(e, Event::Account(AccountEvent::Profile(_)))).await;
    s.http.on("PATCH", "/api/v10/users/@me/profile", [Response::ok(json!({})), Response::ok(json!({}))]);
    for (choice, body) in [(Some(EFFECT), json!({ "profile_effect_id": EFFECT.to_string() })), (None, json!({ "profile_effect_id": null }))] {
        let edit = ProfileEdit { effect: Some(choice), ..Default::default() };
        s.client.store.account.save_profile(edit).unwrap_or_else(|e| panic!("{e:?}"));
        s.client.flush();
        let r = s.http.next_request().await;
        assert_eq!((r.method.as_str(), r.path.as_str(), r.json()), ("PATCH", "/api/v10/users/@me/profile", body));
        s.client.until("effet enregistré", |e| matches!(e, Event::Account(AccountEvent::ProfileWritten { result: Ok(()), .. }))).await;
    }
}

/// Un compte bot ne demande rien : ni effet, ni achats, ni quêtes.
#[tokio::test]
async fn bot_account_sends_no_undocumented_request() {
    let mut s = connected(BOT_TOKEN, "ready_bot").await;
    let before = s.http.requests().len();
    s.client.store.want_effect(EFFECT, None);
    s.client.store.want_owned_effects();
    s.client.store.quests_open();
    s.client.flush();
    s.client.until("quêtes refusées", |e| matches!(e, Event::Quests(QuestEvent::List(Err(_))))).await;
    assert_eq!(s.http.requests().len(), before);
}

/// Liste, accepter, progression annoncée par la passerelle, réclamer, code.
/// Aucune requête de progression n'existe.
#[tokio::test]
async fn quests_are_listed_accepted_and_claimed_without_any_progress_request() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", QUESTS, [Response::ok(fixture("nd/rest_quests"))]);
    s.client.store.quests_open();
    s.client.store.quests_open();
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", QUESTS));
    plain(&r);
    s.client.until("quêtes", |e| matches!(e, Event::Quests(QuestEvent::List(_)))).await;
    let now = s.client.store.now();
    assert_eq!(s.client.store.quests.quest(QUEST).map(|q| q.stage(now)), Some(Stage::Open));

    // Accepter : `location` seul.
    let enroll = format!("/api/v10/quests/{QUEST}/enroll");
    s.http.on("POST", &enroll, [Response::ok(fixture("nd/rest_quest_status"))]);
    assert!(s.client.store.quest_enroll(QUEST));
    assert!(!s.client.store.quest_enroll(QUEST), "une écriture à la fois");
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str(), r.json()), ("POST", enroll.as_str(), json!({ "location": 11 })));
    s.client.until("acceptée", |e| matches!(e, Event::Quests(QuestEvent::Enrolled { result: Ok(_), .. }))).await;
    assert_eq!(s.client.store.quests.quest(QUEST).map(|q| (q.stage(now), q.progress())), Some((Stage::Accepted, Some((300, 900)))));
    assert!(!s.client.store.quest_claim(QUEST), "pas terminée : rien à réclamer");

    // La progression vient de Discord (jouée dans le client officiel), ainsi que le solde.
    let mut status = fixture("nd/rest_quest_status");
    status["completed_at"] = json!("2026-11-15T12:15:00+00:00");
    status["progress"]["PLAY_ON_DESKTOP"]["value"] = json!(900);
    s.conn.dispatch("QUESTS_USER_STATUS_UPDATE", 40, json!({ "user_status": status })).await;
    s.conn.dispatch("VIRTUAL_CURRENCY_BALANCE_UPDATE", 41, json!({ "balance": 1400 })).await;
    s.client.until("solde", |e| matches!(e, Event::Quests(QuestEvent::Orbs(1400)))).await;
    assert_eq!(s.client.store.quests.quest(QUEST).map(|q| q.stage(now)), Some(Stage::Completed));
    assert_eq!((s.client.store.quests.orbs, s.client.store.quests.claimable()), (Some(1400), 2));

    // Réclamer.
    let claim = format!("/api/v10/quests/{QUEST}/claim-reward");
    s.http.on("POST", &claim, [Response::ok(json!({ "claimed_at": "2026-11-15T12:16:00+00:00", "entitlement_expiration_metadata": {}, "entitlements": [], "errors": [] }))]);
    assert!(s.client.store.quest_claim(QUEST));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str(), r.json()), ("POST", claim.as_str(), json!({ "location": 11, "platform": 0 })));
    s.client.until("réclamée", |e| matches!(e, Event::Quests(QuestEvent::Claimed { result: Ok(_), .. }))).await;
    assert_eq!(s.client.store.quests.quest(QUEST).map(|q| q.stage(now)), Some(Stage::Claimed));

    // Quête de console : jamais réclamée ici ; une fois réclamée ailleurs, son code se lit.
    assert!(!s.client.store.quest_claim(CONSOLE_QUEST));
    let mut theirs = fixture("nd/rest_quests")["quests"][2]["user_status"].clone();
    theirs["claimed_at"] = json!("2026-10-04T10:00:00+00:00");
    s.conn.dispatch("QUESTS_USER_STATUS_UPDATE", 42, json!({ "user_status": theirs })).await;
    s.conn.dispatch("QUESTS_USER_COMPLETION_UPDATE", 43, json!({ "quest_enrollment_blocked_until": null })).await;
    s.client.until("blocage", |e| matches!(e, Event::Quests(QuestEvent::BlockedUntil(None)))).await;
    let code = format!("/api/v10/quests/{CONSOLE_QUEST}/reward-code");
    s.http.on("GET", &code, [Response::ok(json!({ "quest_id": CONSOLE_QUEST.to_string(), "code": "FICTIF-0000-CODE", "platform": 0, "user_id": "1", "claimed_at": "2026-10-04T10:00:00+00:00", "tier": null }))]);
    assert!(s.client.store.quest_code(CONSOLE_QUEST));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", code.as_str()));
    s.client.until("code", |e| matches!(e, Event::Quests(QuestEvent::Code { result: Ok(_), .. }))).await;
    assert_eq!(s.client.store.quests.code(CONSOLE_QUEST), Some("FICTIF-0000-CODE"));

    // Jamais de progression déclarée par Ferrocord.
    for r in s.http.requests() {
        assert!(!["heartbeat", "video-progress", "console"].iter().any(|bad| r.path.contains(bad)), "{} {}", r.method, r.path);
        assert!(!r.headers.iter().any(|(n, _)| n.starts_with("x-captcha")), "aucune réponse à un captcha");
    }
}

/// Captcha demandé : l'action s'arrête, sans nouvel essai.
#[tokio::test]
async fn quest_captcha_stops_the_action() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", QUESTS, [Response::ok(fixture("nd/rest_quests"))]);
    s.client.store.quests_open();
    s.client.flush();
    s.http.next_request().await;
    s.client.until("quêtes", |e| matches!(e, Event::Quests(QuestEvent::List(_)))).await;
    let enroll = format!("/api/v10/quests/{QUEST}/enroll");
    s.http.on("POST", &enroll, [Response::raw(400, json!({ "captcha_key": ["captcha-required"], "captcha_sitekey": "fictif", "captcha_service": "hcaptcha" }))]);
    assert!(s.client.store.quest_enroll(QUEST));
    s.client.flush();
    s.http.next_request().await;
    s.client.until("captcha", |e| matches!(e, Event::Quests(QuestEvent::Enrolled { result: Err(Refused::Captcha), .. }))).await;
    assert_eq!(s.client.store.quests.refused(QUEST), Some(&Refused::Captcha));
    assert!(!s.client.store.quests.busy(QUEST));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(s.http.requests().iter().filter(|r| r.path == enroll).count(), 1, "aucun nouvel essai");
}

/// Tag de serveur : lu dans `profile` du serveur, choisi par `PUT /users/@me/clan`.
#[tokio::test]
async fn guild_tag_is_read_and_chosen_by_one_request() {
    use ferrocord::state::tags::TagEvent;
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    assert!(s.client.store.tag_choices().is_empty());
    assert!(!s.client.store.set_guild_tag(Some(ALPHA)), "serveur sans tag : aucune requête");
    let mut g = fixture("guild_create_alpha");
    g["d"]["profile"] = json!({ "tag": "ALFA", "badge": null });
    s.conn.dispatch("GUILD_CREATE", 40, g["d"].clone()).await;
    s.client.until("serveur", |e| matches!(e, Event::GuildCreate(_))).await;
    assert_eq!(s.client.store.tag_choices().iter().map(|t| (t.guild_id, t.text().to_owned())).collect::<Vec<_>>(), vec![(ALPHA, "ALFA".to_owned())]);

    let path = "/api/v10/users/@me/clan";
    s.http.on("PUT", path, [Response::raw(400, json!({ "captcha_key": ["captcha-required"], "captcha_sitekey": "fictif" })), Response::ok(fixture("rest_me")), Response::ok(fixture("rest_me"))]);
    assert!(s.client.store.set_guild_tag(Some(ALPHA)));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str(), r.json()), ("PUT", path, json!({ "identity_guild_id": ALPHA.to_string(), "identity_enabled": true })));
    plain(&r);
    s.client.until("captcha", |e| matches!(e, Event::Tags(TagEvent::Set { result: Err(Refused::Captcha), .. }))).await;
    assert_eq!((s.client.store.own_tag(), s.client.store.tags.refused()), (None, Some(&Refused::Captcha)));

    // Nouveau clic : accepté ; puis retrait.
    assert!(s.client.store.set_guild_tag(Some(ALPHA)));
    s.client.flush();
    s.http.next_request().await;
    s.client.until("tag", |e| matches!(e, Event::Tags(TagEvent::Set { result: Ok(()), .. }))).await;
    assert_eq!(s.client.store.own_tag().map(|t| t.guild_id), Some(ALPHA));
    assert!(s.client.store.set_guild_tag(None));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.json(), json!({ "identity_guild_id": null, "identity_enabled": false }));
    s.client.until("retrait", |e| matches!(e, Event::Tags(TagEvent::Set { guild: None, result: Ok(()) }))).await;
    assert_eq!(s.client.store.own_tag(), None);
    assert_eq!(s.http.requests().iter().filter(|r| r.path == path).count(), 3);
}

/// Boutique en consultation : une lecture, aucune requête d'achat.
#[tokio::test]
async fn shop_is_read_once_and_nothing_is_ever_bought() {
    use ferrocord::state::shop::ShopEvent;
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let path = "/api/v10/collectibles-categories";
    s.http.on("GET", path, [Response::ok(fixture("nd/rest_collectibles_categories"))]);
    s.client.store.shop_open();
    s.client.store.shop_open();
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", path));
    plain(&r);
    s.client.until("boutique", |e| matches!(e, Event::Shop(ShopEvent::Loaded(_)))).await;
    let Remote::Ready(cats) = &s.client.store.shop.list else { panic!("boutique absente") };
    assert_eq!(cats.iter().map(|c| c.products.len()).collect::<Vec<_>>(), [3, 1]);
    s.client.store.shop_open();
    assert!(s.client.store.take_commands().is_empty(), "liste connue : rien n'est redemandé");
    for r in s.http.requests() {
        assert!(r.method == "GET" || !["purchase", "billing", "payment", "store", "collectibles"].iter().any(|bad| r.path.contains(bad)), "{} {}", r.method, r.path);
    }
}

/// Inventaire : deux lectures, aucune écriture.
#[tokio::test]
async fn inventory_is_two_reads_and_no_write() {
    use ferrocord::state::inventory::InventoryEvent;
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", "/api/v10/users/@me/entitlements", [Response::ok(fixture("nd/rest_entitlements"))]);
    s.http.on("GET", "/api/v10/users/@me/entitlements/gifts", [Response::ok(json!([]))]);
    let before = s.http.requests().len();
    s.client.store.inventory_open();
    s.client.store.inventory_open();
    s.client.flush();
    let (a, b) = (s.http.next_request().await, s.http.next_request().await);
    assert_eq!((a.method.as_str(), a.path.as_str()), ("GET", "/api/v10/users/@me/entitlements?with_sku=true&exclude_ended=true"));
    assert_eq!((b.method.as_str(), b.path.as_str()), ("GET", "/api/v10/users/@me/entitlements/gifts"));
    plain(&a);
    s.client.until("inventaire", |e| matches!(e, Event::Inventory(InventoryEvent::Loaded(Ok(_))))).await;
    let Remote::Ready(owned) = &s.client.store.inventory.list else { panic!("inventaire absent") };
    assert_eq!((owned.mine.len(), owned.giftable.len()), (3, 0));
    assert_eq!(s.http.requests().len(), before + 2);
}

/// Décoration et plaque : lues dans les achats, portées par `PATCH /users/@me`.
#[tokio::test]
async fn owned_decoration_and_nameplate_are_worn_by_one_request() {
    use ferrocord::state::tags::{Slot, TagEvent};
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    assert!(!s.client.store.wear(Slot::Decoration, Some(6000000000000000010)), "rien de possédé : aucune requête");
    s.http.on("GET", "/api/v10/users/@me/collectibles-purchases", [Response::ok(fixture("nd/rest_collectibles_purchases"))]);
    s.client.store.want_owned_effects();
    s.client.flush();
    s.http.next_request().await;
    s.client.until("achats", |e| matches!(e, Event::Effects(EffectEvent::Owned(_)))).await;
    let looks: Vec<(u64, Slot)> = s.client.store.effects.looks.iter().map(|o| (o.sku_id, Slot::of(&o.look))).collect();
    assert_eq!(looks, vec![(6000000000000000010, Slot::Decoration), (6000000000000000020, Slot::Nameplate)]);
    assert!(!s.client.store.wear(Slot::Nameplate, Some(6000000000000000010)), "une décoration n'est pas une plaque");

    s.http.on("PATCH", "/api/v10/users/@me", [Response::ok(fixture("rest_me")), Response::ok(fixture("rest_me")), Response::ok(fixture("rest_me"))]);
    for (slot, sku, body) in [
        (Slot::Decoration, Some(6000000000000000010), json!({ "avatar_decoration_sku_id": "6000000000000000010" })),
        (Slot::Nameplate, Some(6000000000000000020), json!({ "nameplate_sku_id": "6000000000000000020" })),
        (Slot::Decoration, None, json!({ "avatar_decoration_sku_id": null })),
    ] {
        assert!(s.client.store.wear(slot, sku));
        s.client.flush();
        let r = s.http.next_request().await;
        assert_eq!((r.method.as_str(), r.path.as_str(), r.json()), ("PATCH", "/api/v10/users/@me", body));
        s.client.until("parure", |e| matches!(e, Event::Tags(TagEvent::Worn { result: Ok(()), .. }))).await;
        assert_eq!(s.client.store.worn(slot), sku);
    }
    assert_eq!(s.client.store.worn(Slot::Nameplate), Some(6000000000000000020));
}

/// Suggestions : une lecture, « Ignorer » = un DELETE, « Ajouter » = la demande habituelle.
#[tokio::test]
async fn friend_suggestions_are_read_once_dismissed_and_added_by_a_click() {
    use ferrocord::state::suggestions::SuggestionEvent;
    const A: u64 = 1000000000000000701;
    const B: u64 = 1000000000000000702;
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", "/api/v10/friend-suggestions", [Response::ok(fixture("nd/rest_friend_suggestions"))]);
    s.http.on("DELETE", &format!("/api/v10/friend-suggestions/{A}"), [Response::raw(204, serde_json::Value::Null)]);
    s.http.on("PUT", &format!("/api/v10/users/@me/relationships/{B}"), [Response::raw(204, serde_json::Value::Null)]);
    let before = s.http.requests().len();
    s.client.store.suggestions_open();
    s.client.store.suggestions_open();
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str(), r.body.is_empty()), ("GET", "/api/v10/friend-suggestions", true));
    plain(&r);
    s.client.until("suggestions", |e| matches!(e, Event::Suggestions(SuggestionEvent::List(Ok(_))))).await;
    let Remote::Ready(list) = &s.client.store.suggestions.list else { panic!("suggestions absentes") };
    assert_eq!(list.iter().map(|x| x.user_id).collect::<Vec<_>>(), vec![A, B]);
    assert!(s.client.store.suggestion_dismiss(A));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path, r.body.is_empty()), ("DELETE", format!("/api/v10/friend-suggestions/{A}"), true));
    assert!(s.client.store.suggestion_add(B));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path), ("PUT", format!("/api/v10/users/@me/relationships/{B}")));
    assert_eq!(s.client.store.suggestions.list, Remote::Ready(Vec::new()));
    assert_eq!(s.http.requests().len(), before + 3);
}

/// Découverte : catégories et liste à l'ouverture, une recherche par geste, que des lectures.
#[tokio::test]
async fn discovery_is_read_on_open_and_searched_by_a_gesture() {
    use ferrocord::state::discovery::DiscoveryEvent;
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", "/api/v10/discovery/categories", [Response::ok(fixture("nd/rest_discovery_categories"))]);
    s.http.on("GET", "/api/v10/discoverable-guilds", [Response::ok(fixture("nd/rest_discoverable_guilds"))]);
    s.http.on("GET", "/api/v10/discoverable-guilds/search", [Response::ok(fixture("nd/rest_discoverable_guilds")["guilds"].clone())]);
    let before = s.http.requests().len();
    s.client.store.discovery_open("fr");
    s.client.store.discovery_open("fr");
    s.client.flush();
    let (a, b) = (s.http.next_request().await, s.http.next_request().await);
    let mut paths = [a.path.clone(), b.path.clone()];
    paths.sort();
    assert_eq!(paths, ["/api/v10/discoverable-guilds?limit=24".to_string(), "/api/v10/discovery/categories?locale=fr&primary_only=true".to_string()]);
    assert_eq!((a.method.as_str(), b.method.as_str()), ("GET", "GET"));
    plain(&a);
    s.client.until("serveurs", |e| matches!(e, Event::Discovery(DiscoveryEvent::Results { result: Ok(_), .. }))).await;
    assert!(s.client.store.discovery_search("jardin fictif", Some(1)));
    assert!(!s.client.store.discovery_search("jardin fictif", Some(1)));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", "/api/v10/discoverable-guilds/search?query=jardin+fictif&limit=24&category_id=1"));
    s.client.until("recherche", |e| matches!(e, Event::Discovery(DiscoveryEvent::Results { seq: 2, result: Ok(_) }))).await;
    let Remote::Ready(list) = &s.client.store.discovery.results else { panic!("serveurs absents") };
    assert_eq!(list.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), vec!["Jardin Fictif", "Atelier Synthétique"]);
    assert_eq!(s.http.requests().len(), before + 3);
}

/// Résumés : une lecture à l'ouverture du panneau, une autre seulement sur « Actualiser ».
#[tokio::test]
async fn channel_summaries_are_one_read_per_gesture() {
    use ferrocord::state::summaries::SummaryEvent;
    const CHANNEL: u64 = 3000000000000000011;
    let path = format!("/api/v10/channels/{CHANNEL}/summaries");
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &path, [Response::ok(fixture("nd/rest_channel_summaries")), Response::error(403, "Missing Access")]);
    let before = s.http.requests().len();
    s.client.store.summaries_open(CHANNEL);
    s.client.store.summaries_open(CHANNEL);
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str(), r.body.is_empty()), ("GET", path.as_str(), true));
    plain(&r);
    s.client.until("résumés", |e| matches!(e, Event::Summaries(SummaryEvent::Loaded { result: Ok(_), .. }))).await;
    let Some(Remote::Ready(list)) = s.client.store.summaries.get(CHANNEL) else { panic!("résumés absents") };
    assert_eq!(list.iter().map(|x| x.topic.as_str()).collect::<Vec<_>>(), vec!["Choix du terreau", "Arrosage des semis fictifs"]);
    s.client.store.summaries_refresh(CHANNEL);
    s.client.flush();
    s.client.until("refus", |e| matches!(e, Event::Summaries(SummaryEvent::Loaded { result: Err(_), .. }))).await;
    s.client.store.summaries_open(CHANNEL);
    s.client.flush();
    assert_eq!(s.http.requests().len(), before + 2);
}

/// Vérification : deux lectures à l'ouverture, un envoi par clic avec tous les champs ; captcha : arrêt.
#[tokio::test]
async fn member_verification_is_read_then_answered_by_one_request() {
    use ferrocord::state::verification::{Answer, Gate, Status, VerifyEvent};
    let base = format!("/api/v10/guilds/{ALPHA}");
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &format!("{base}/member-verification"), [Response::ok(fixture("nd/rest_member_verification"))]);
    s.http.on("GET", &format!("{base}/requests/@me"), [Response::error(404, "Unknown Guild Join Request")]);
    s.http.on("PUT", &format!("{base}/requests/@me"), [Response::raw(400, json!({ "captcha_key": ["captcha-required"], "captcha_sitekey": "x" })), Response::ok(json!({ "application_status": "SUBMITTED" }))]);
    let before = s.http.requests().len();
    s.client.store.verification_open(ALPHA);
    s.client.flush();
    let (a, b) = (s.http.next_request().await, s.http.next_request().await);
    assert_eq!((a.method.as_str(), a.path, b.method.as_str(), b.path), ("GET", format!("{base}/member-verification"), "GET", format!("{base}/requests/@me")));
    s.client.until("formulaire", |e| matches!(e, Event::Verification(VerifyEvent::Loaded { result: Ok(_), .. }))).await;
    assert!(matches!(&s.client.store.verification.gate, Remote::Ready(Gate { form: Some(f), status: Status::Unknown }) if f.fields.len() == 4));
    let answers = [Answer::Agreed(true), Answer::Text("Pour essayer".into()), Answer::Text(String::new()), Answer::Choice(Some(1))];
    assert!(s.client.store.verification_submit(&answers));
    s.client.flush();
    let r = s.http.next_request().await;
    plain(&r);
    let body: serde_json::Value = serde_json::from_str(&r.body).unwrap();
    assert_eq!(body["version"], "2026-03-02T10:00:00.000000+00:00");
    let fields = body["form_fields"].as_array().unwrap();
    assert_eq!(fields.iter().map(|f| f["response"].clone()).collect::<Vec<_>>(), vec![json!(true), json!("Pour essayer"), json!(null), json!(1)]);
    assert_eq!((fields[0]["field_type"].as_str(), fields[3]["choices"].clone()), (Some("TERMS"), json!(["Un ami", "Une recherche"])));
    s.client.until("captcha", |e| matches!(e, Event::Verification(VerifyEvent::Submitted { result: Err(Refused::Captcha), .. }))).await;
    assert_eq!(s.http.requests().len(), before + 3, "captcha : rien n'est retenté");
    // Un nouveau clic renvoie les mêmes réponses, une fois.
    assert!(s.client.store.verification_submit(&answers));
    s.client.flush();
    s.client.until("envoi", |e| matches!(e, Event::Verification(VerifyEvent::Submitted { result: Ok(Status::Submitted), .. }))).await;
    assert!(!s.client.store.verification_submit(&answers));
    assert_eq!(s.http.requests().len(), before + 4);
}

/// Demandes d'adhésion : une lecture au clic, une décision par clic.
#[tokio::test]
async fn join_requests_are_listed_and_decided_one_click_each() {
    use ferrocord::state::verification::VerifyEvent;
    const REQUEST: u64 = 1000000000000000951;
    let base = format!("/api/v10/guilds/{BETA}");
    let mut s = connected(USER_TOKEN, "ready_user_roles").await;
    assert!(s.client.store.can_review_requests(BETA), "le jeu d'essai donne le droit d'expulser");
    s.http.on("GET", &format!("{base}/member-verification"), [Response::error(404, "Unknown")]);
    s.http.on("GET", &format!("{base}/requests"), [Response::ok(fixture("nd/rest_join_requests"))]);
    s.http.on("PATCH", &format!("{base}/requests/id/{REQUEST}"), [Response::ok(json!({ "application_status": "REJECTED" }))]);
    s.client.store.verification_open(BETA);
    s.client.flush();
    s.http.next_request().await;
    s.client.until("pas de formulaire", |e| matches!(e, Event::Verification(VerifyEvent::Loaded { result: Ok(g), .. }) if g.form.is_none())).await;
    let before = s.http.requests().len();
    assert!(s.client.store.join_requests_load());
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path), ("GET", format!("{base}/requests?status=SUBMITTED&limit=25")));
    s.client.until("demandes", |e| matches!(e, Event::Verification(VerifyEvent::Requests { result: Ok(_), .. }))).await;
    assert!(!s.client.store.join_request_decide(5, true, ""), "demande inconnue : rien ne part");
    assert!(s.client.store.join_request_decide(REQUEST, false, " Incomplet "));
    assert!(!s.client.store.join_request_decide(REQUEST, true, ""), "une décision à la fois");
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path, serde_json::from_str::<serde_json::Value>(&r.body).unwrap()), ("PATCH", format!("{base}/requests/id/{REQUEST}"), json!({ "action": "REJECTED", "rejection_reason": "Incomplet" })));
    s.client.until("décision", |e| matches!(e, Event::Verification(VerifyEvent::Actioned { result: Ok(()), .. }))).await;
    assert_eq!(s.client.store.verification.requests, Remote::Ready(Vec::new()));
    assert_eq!(s.http.requests().len(), before + 2);
}

/// Répertoire d'applications : catégories et liste à l'ouverture, une recherche par geste, que des lectures.
#[tokio::test]
async fn app_directory_is_read_on_open_and_searched_by_a_gesture() {
    use ferrocord::state::appdir::AppDirEvent;
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", "/api/v10/application-directory-static/categories", [Response::ok(fixture("nd/rest_app_directory_categories"))]);
    s.http.on("GET", "/api/v10/application-directory-static/search", [Response::ok(fixture("nd/rest_app_directory_search")), Response::ok(fixture("nd/rest_app_directory_search"))]);
    let before = s.http.requests().len();
    s.client.store.appdir_open("fr");
    s.client.store.appdir_open("fr");
    s.client.flush();
    let (a, b) = (s.http.next_request().await, s.http.next_request().await);
    let mut paths = [a.path.clone(), b.path.clone()];
    paths.sort();
    assert_eq!(paths, ["/api/v10/application-directory-static/categories?locale=fr".to_string(), "/api/v10/application-directory-static/search?page=1&page_size=24&locale=fr".to_string()]);
    assert_eq!((a.method.as_str(), b.method.as_str()), ("GET", "GET"));
    plain(&a);
    s.client.until("applications", |e| matches!(e, Event::AppDir(AppDirEvent::Results { result: Ok(_), .. }))).await;
    assert!(s.client.store.appdir_search("arrosoir", Some(4), "fr"));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", "/api/v10/application-directory-static/search?query=arrosoir&page=1&page_size=24&category_id=4&locale=fr"));
    s.client.until("recherche", |e| matches!(e, Event::AppDir(AppDirEvent::Results { seq: 2, result: Ok(_) }))).await;
    let Remote::Ready(list) = &s.client.store.appdir.results else { panic!("applications absentes") };
    assert_eq!(list.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), vec!["Arrosoir Fictif", "Semoir Synthétique"]);
    assert_eq!(s.http.requests().len(), before + 3);
}

/// Centre de sécurité : rien sans clic, une lecture par clic, aucune écriture.
#[tokio::test]
async fn safety_hub_is_one_read_per_click() {
    use ferrocord::state::safety::{SafetyEvent, Standing};
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", "/api/v10/safety-hub/@me", [Response::ok(fixture("nd/rest_safety_hub"))]);
    let before = s.http.requests().len();
    assert!(s.client.store.safety_hub_load());
    assert!(!s.client.store.safety_hub_load());
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str(), r.body.is_empty()), ("GET", "/api/v10/safety-hub/@me", true));
    plain(&r);
    s.client.until("état du compte", |e| matches!(e, Event::SafetyHub(SafetyEvent::Loaded(Ok(_))))).await;
    let Remote::Ready(hub) = &s.client.store.safety_hub else { panic!("état absent") };
    assert_eq!((hub.standing, hub.violations.len()), (Standing::Limited, 2));
    assert_eq!(s.http.requests().len(), before + 1);
}

/// Messages enregistrés : une écriture par clic (avec ou sans échéance), une lecture par ouverture.
#[tokio::test]
async fn saved_messages_are_written_and_listed_one_request_per_gesture() {
    use ferrocord::state::saved::{SavedEvent, SavedNotice, HOUR_MS};
    const MESSAGE: u64 = 1000000000000000981;
    let one = format!("/api/v10/users/@me/saved-messages/{GENERAL}/{MESSAGE}");
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("PUT", &one, [Response::ok(json!({})), Response::raw(400, json!({ "captcha_key": ["captcha-required"], "captcha_sitekey": "fictif" }))]);
    s.http.on("GET", "/api/v10/users/@me/saved-messages", [Response::ok(fixture("nd/rest_saved_messages"))]);
    s.http.on("DELETE", &one, [Response::raw(204, serde_json::Value::Null)]);
    let before = s.http.requests().len();
    assert!(s.client.store.save_message(GENERAL, MESSAGE, Some(HOUR_MS)));
    s.client.flush();
    let r = s.http.next_request().await;
    plain(&r);
    assert_eq!((r.method.as_str(), r.path.as_str()), ("PUT", one.as_str()));
    let due = serde_json::from_str::<serde_json::Value>(&r.body).unwrap()["due_at"].as_str().map(str::to_owned).unwrap();
    assert!(due.ends_with('Z') && due.len() == 20, "date ISO à la seconde : {due}");
    s.client.until("rappel", |e| matches!(e, Event::Saved(SavedEvent::Saved { reminder: true, result: Ok(()) }))).await;
    assert_eq!(s.client.store.saved.take_notice(), Some(SavedNotice::Reminder));
    // Sans échéance : `due_at` nul ; captcha : arrêt.
    assert!(s.client.store.save_message(GENERAL, MESSAGE, None));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(serde_json::from_str::<serde_json::Value>(&r.body).unwrap(), json!({ "due_at": null }));
    s.client.until("captcha", |e| matches!(e, Event::Saved(SavedEvent::Saved { result: Err(Refused::Captcha), .. }))).await;
    assert_eq!(s.http.requests().len(), before + 2, "rien n'est retenté ni relu");
    s.client.store.saved_open();
    s.client.store.saved_open();
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", "/api/v10/users/@me/saved-messages"));
    s.client.until("liste", |e| matches!(e, Event::Saved(SavedEvent::List(Ok(_))))).await;
    assert!(s.client.store.unsave_message(GENERAL, MESSAGE));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("DELETE", one.as_str()));
    s.client.until("retiré", |e| matches!(e, Event::Saved(SavedEvent::Removed { result: Ok(()), .. }))).await;
    let Remote::Ready(list) = &s.client.store.saved.list else { panic!("liste absente") };
    assert_eq!(list.len(), 2);
    assert_eq!(s.http.requests().len(), before + 4);
}

/// Style du nom : `PATCH /users/@me`, deux couleurs au plus ; retiré par trois nuls.
#[tokio::test]
async fn name_style_is_written_by_one_request() {
    use ferrocord::state::tags::{StyleEdit, TagEvent};
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let path = "/api/v10/users/@me";
    s.http.on("PATCH", path, [Response::raw(400, json!({ "captcha_key": ["captcha-required"], "captcha_sitekey": "fictif" })), Response::ok(fixture("rest_me")), Response::ok(fixture("rest_me"))]);
    let style = StyleEdit { font_id: 11, effect_id: 2, colors: vec![0xff0000, 0x0000ff, 0x00ff00] };
    assert!(!s.client.store.set_name_style(None), "aucun style porté : rien à retirer");
    assert!(s.client.store.set_name_style(Some(style.clone())));
    assert!(!s.client.store.set_name_style(Some(style.clone())), "une écriture à la fois");
    s.client.flush();
    let r = s.http.next_request().await;
    let body = json!({ "display_name_font_id": 11, "display_name_effect_id": 2, "display_name_colors": [0xff0000, 0x0000ff] });
    assert_eq!((r.method.as_str(), r.path.as_str(), r.json()), ("PATCH", path, body));
    plain(&r);
    // Captcha : l'action s'arrête là, le style n'est pas porté.
    s.client.until("captcha", |e| matches!(e, Event::Tags(TagEvent::Styled { result: Err(Refused::Captcha), .. }))).await;
    assert_eq!((s.client.store.own_name_style(), s.client.store.tags.refused()), (None, Some(&Refused::Captcha)));
    assert!(s.client.store.set_name_style(Some(style)));
    s.client.flush();
    s.http.next_request().await;
    s.client.until("style", |e| matches!(e, Event::Tags(TagEvent::Styled { result: Ok(()), .. }))).await;
    assert_eq!(s.client.store.own_name_style().map(|n| n.colors().to_vec()), Some(vec![0xff0000, 0x0000ff]));
    assert!(s.client.store.set_name_style(None));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.json(), json!({ "display_name_font_id": null, "display_name_effect_id": null, "display_name_colors": null }));
    s.client.until("retrait", |e| matches!(e, Event::Tags(TagEvent::Styled { style: None, result: Ok(()) }))).await;
    assert_eq!(s.client.store.own_name_style(), None);
    assert_eq!(s.http.requests().iter().filter(|r| r.path == path).count(), 3);
}

/// Widgets de jeux : lus dans le profil, nommés par `GET /games`, remplacés
/// par `PUT /users/@me/widgets` ; suggestions demandées une fois.
#[tokio::test]
async fn profile_widgets_are_read_named_and_replaced() {
    use ferrocord::state::widgets::{GameWidget, WidgetEvent, WidgetGame, WidgetKind};
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let mut profile = fixture("rest_user_profile");
    profile["widgets"] = fixture("nd/rest_widgets");
    s.http.on("GET", &format!("/api/v10/users/{ALICE}/profile"), [Response::ok(profile)]);
    s.client.store.want_profile(ALICE, None);
    s.client.flush();
    s.http.next_request().await;
    within("profil", async {
        loop {
            s.client.drain();
            if s.client.store.people.lock().profiles().profile(ALICE, None).and_then(ProfileState::profile).is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await;
    let read = s.client.store.people.lock().profiles().profile(ALICE, None).and_then(ProfileState::profile).cloned().unwrap_or_default();
    let kinds: Vec<WidgetKind> = read.widgets.iter().map(|w| w.kind).collect();
    assert_eq!(kinds, vec![WidgetKind::Favorite, WidgetKind::Current, WidgetKind::Played, WidgetKind::Application]);
    assert!(read.widgets_opaque, "le type inconnu du jeu d'essai interdit la réécriture");

    // Noms des jeux : une requête pour tous, une seule fois.
    let games = "/api/v10/games?game_ids=7000000000000000001&game_ids=7000000000000000002";
    s.http.on("GET", "/api/v10/games", [Response::ok(fixture("nd/rest_games"))]);
    let ids = [7000000000000000001, 7000000000000000002];
    s.client.store.want_games(&ids);
    s.client.store.want_games(&ids);
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", games));
    plain(&r);
    s.client.until("jeux", |e| matches!(e, Event::Widgets(WidgetEvent::Games { result: Ok(_), .. }))).await;
    assert_eq!(s.client.store.widgets.game(ids[0]).map(|g| (g.name.as_str(), g.cover.is_some())), Some(("Course Fictive 6", true)));
    assert_eq!(s.client.store.widgets.game(ids[1]).map(|g| g.name.as_str()), Some("Guerre Fictive"));

    let suggested = "/api/v10/users/@me/widgets/suggested-games";
    s.http.on("GET", suggested, [Response::ok(fixture("nd/rest_widgets_suggested"))]);
    s.client.store.want_suggested_games();
    s.client.store.want_suggested_games();
    s.client.flush();
    s.http.next_request().await;
    s.client.until("suggestions", |e| matches!(e, Event::Widgets(WidgetEvent::Suggested(Ok(_))))).await;
    assert!(matches!(&s.client.store.widgets.suggested, Remote::Ready(x) if x.games.len() == 2 && x.wishlist == vec![7000000000000000005]));

    // Écriture : un captcha arrête tout ; le second essai passe, la réponse fait foi.
    let path = "/api/v10/users/@me/widgets";
    let mut kept = fixture("nd/rest_widgets");
    kept.as_array_mut().unwrap_or_else(|| panic!("liste")).truncate(1);
    s.http.on("PUT", path, [Response::raw(400, json!({ "captcha_key": ["captcha-required"], "captcha_sitekey": "fictif" })), Response::ok(kept)]);
    let mut wanted = GameWidget::new(WidgetKind::Wanted);
    wanted.games.push(WidgetGame::new(ids[1]));
    let list = vec![read.widgets[0].clone(), wanted];
    assert!(s.client.store.save_widgets(list.clone()));
    assert!(!s.client.store.save_widgets(list.clone()), "une écriture à la fois");
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("PUT", path));
    plain(&r);
    assert_eq!(
        r.json(),
        json!({ "widgets": [
            { "id": "7100000000000000001", "data": { "type": "favorite_games", "games": [{ "game_id": "7000000000000000001", "comment": "Le meilleur jeu fictif.", "tags": ["expert", "open_to_teach"] }] } },
            { "data": { "type": "want_to_play_games", "games": [{ "game_id": "7000000000000000002", "comment": null, "tags": [] }] } },
        ] })
    );
    s.client.until("captcha", |e| matches!(e, Event::Widgets(WidgetEvent::Saved(Err(Refused::Captcha))))).await;
    assert_eq!(s.client.store.widgets.take_refusal(), Some(Refused::Captcha));
    assert!(s.client.store.save_widgets(list));
    s.client.flush();
    s.http.next_request().await;
    s.client.until("widgets", |e| matches!(e, Event::Widgets(WidgetEvent::Saved(Ok(w))) if w.len() == 1)).await;
    assert!(!s.client.store.widgets.saving());
    assert_eq!(s.http.requests().iter().filter(|r| r.path == path).count(), 2, "aucun nouvel essai de lui-même");
}
