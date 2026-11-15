//! Réglages du compte : réglages protobuf (READY, écriture, refus, mise à
//! jour de la passerelle), notifications par serveur (muet et non-lus),
//! profil, appareils, applications. Faux serveur local seulement.

use crate::support::{ids::*, *};
use ferrocord::state::usersettings::notif::{Change, Level, Mute};
use ferrocord::state::usersettings::proto::Msg;
use ferrocord::state::usersettings::schema::{top, Key, SValue};
use ferrocord::state::usersettings::{b64, AccountEvent, Remote};
use ferrocord::state::{Effect, Event, Notice, Problem};
use serde_json::{json, Value};

const PROTO_PATH: &str = "/api/v10/users/@me/settings-proto/1";

fn account(e: &Event) -> Option<&AccountEvent> {
    match e {
        Event::Account(a) => Some(a),
        _ => None,
    }
}

/// READY utilisateur avec les réglages synchronisés du jeu `rest_settings_proto`
/// et #général (serveur Alpha) rendu muet sans fin.
async fn with_settings() -> Setup {
    connected_with(USER_TOKEN, "ready_user_classic", |p| {
        p["d"]["user_settings_proto"] = fixture("rest_settings_proto")["settings"].clone();
        p["d"]["user_guild_settings"] = json!({
            "version": 1,
            "partial": false,
            "entries": [{
                "guild_id": ALPHA.to_string(),
                "muted": false,
                "message_notifications": 1,
                "channel_overrides": [{ "channel_id": GENERAL.to_string(), "muted": true, "mute_config": { "end_time": null, "selected_time_window": -1 }, "message_notifications": 3 }]
            }]
        });
    })
    .await
}

/// Corps protobuf d'un PATCH des réglages (base64 dans `settings`).
fn sent_proto(body: &str) -> (Msg, Value) {
    let v: Value = serde_json::from_str(body).unwrap_or_else(|e| panic!("corps : {e}"));
    let bytes = b64::decode(v["settings"].as_str().unwrap_or_default()).unwrap_or_else(|| panic!("base64 : {body}"));
    (Msg::decode(&bytes).unwrap_or_else(|e| panic!("protobuf : {e:?}")), v)
}

#[tokio::test]
async fn synced_setting_is_written_alone_and_unknowns_survive() {
    let mut s = with_settings().await;
    let acc = &s.client.store.account;
    assert!(acc.proto_ready());
    assert_eq!(acc.bool(Key::InlineEmbeds), Some(true));
    assert_eq!(acc.bool(Key::MessageCompact), Some(true));
    assert_eq!(acc.value(Key::Status), Some(&SValue::Str("idle".into())));
    assert_eq!(acc.snapshot().custom_status.as_ref().map(|c| c.text.as_str()), Some("En pause"));
    // Aucune requête sans geste.
    assert!(s.http.requests().is_empty());

    s.http.on("PATCH", PROTO_PATH, [Response::ok(json!({ "settings": "MgJSAA==" }))]);
    s.client.store.account.write(Key::InlineEmbeds, SValue::Bool(false)).unwrap_or_else(|e| panic!("{e:?}"));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("PATCH", PROTO_PATH));
    assert_eq!(r.authorization.as_deref(), Some(USER_TOKEN));
    let (sent, v) = sent_proto(&r.body);
    assert_eq!(v["required_data_version"], 4);
    assert_eq!(sent.top_numbers(), vec![top::TEXT_AND_IMAGES], "seul le champ de premier niveau modifié part");
    let text = sent.message(top::TEXT_AND_IMAGES).ok().flatten().unwrap_or_default();
    assert_eq!(text.bytes(500), Some(&b"inconnu"[..]), "réglage inconnu renvoyé tel quel");
    assert!(text.message(17).ok().flatten().is_some(), "réglage voisin (mode compact) renvoyé");
    s.client.until("écriture confirmée", |e| matches!(account(e), Some(AccountEvent::ProtoWritten { .. }))).await;
    let acc = &s.client.store.account;
    assert_eq!(acc.bool(Key::InlineEmbeds), Some(false));
    assert!(!acc.proto_busy());
    assert_eq!(acc.value(Key::Status), Some(&SValue::Str("idle".into())), "autres sections intactes");
}

#[tokio::test]
async fn refused_write_is_rolled_back_and_gateway_update_applies() {
    let mut s = with_settings().await;
    s.http.on("PATCH", PROTO_PATH, [Response::error(400, "Invalid Form Body")]);
    s.client.store.account.write(Key::GifAutoPlay, SValue::Bool(false)).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(s.client.store.account.bool(Key::GifAutoPlay), Some(false), "optimiste");
    s.client.flush();
    s.client.until("refus", |e| matches!(account(e), Some(AccountEvent::ProtoWritten { .. }))).await;
    assert_eq!(s.client.store.account.bool(Key::GifAutoPlay), Some(true), "valeur rétablie");
    assert!(s.client.effects.contains(&Effect::Toast(Notice::SettingNotSaved(Problem::Api("Invalid Form Body".into())))), "{:?}", s.client.effects);

    // Changé sur un autre appareil : mise à jour partielle de la passerelle.
    s.conn.dispatch_fixture("user_settings_proto_update", 5).await;
    s.client.until("USER_SETTINGS_PROTO_UPDATE", |e| matches!(account(e), Some(AccountEvent::Proto { partial: true, .. }))).await;
    let acc = &s.client.store.account;
    assert_eq!(acc.bool(Key::InlineEmbeds), Some(false));
    assert_eq!(acc.value(Key::Status), Some(&SValue::Str("idle".into())), "partielle : le reste est gardé");
}

#[tokio::test]
async fn muted_channel_stays_read_until_unmuted() {
    let mut s = with_settings().await;
    // #général est muet : un nouveau message n'en fait pas un non-lu.
    s.conn.dispatch_fixture("message_create_simple", 10).await;
    s.client.until("MESSAGE_CREATE", |e| matches!(e, Event::MessageCreate(_))).await;
    assert!(!s.client.store.unread().contains(&GENERAL));

    let path = format!("/api/v10/users/@me/guilds/{ALPHA}/settings");
    let confirmed = json!({ "guild_id": ALPHA.to_string(), "muted": false, "message_notifications": 1, "channel_overrides": [{ "channel_id": GENERAL.to_string(), "muted": false, "message_notifications": 3 }] });
    s.http.on("PATCH", &path, [Response::ok(confirmed)]);
    s.client.store.account.change_notif(Some(ALPHA), Some(GENERAL), Change::Mute(Mute::default()), 0).unwrap_or_else(|e| panic!("{e:?}"));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("PATCH", path.as_str()));
    assert_eq!(r.json(), json!({ "channel_overrides": { GENERAL.to_string(): { "muted": false, "mute_config": null } } }));
    s.client.until("réglage du serveur confirmé", |e| matches!(account(e), Some(AccountEvent::GuildWritten { .. }))).await;
    assert_eq!(s.client.store.account.notifs.get(Some(ALPHA)).channel(GENERAL).level, Level::Inherit);

    let mut again = fixture("message_create_simple");
    again["d"]["id"] = json!("5000000000000000301");
    s.conn.send(json!({ "op": 0, "t": "MESSAGE_CREATE", "s": 11, "d": again["d"] })).await;
    s.client.until("MESSAGE_CREATE", |e| matches!(e, Event::MessageCreate(_))).await;
    assert!(s.client.store.unread().contains(&GENERAL), "réactivé : non-lu de nouveau");
}

#[tokio::test]
async fn profile_devices_and_apps_are_read_on_demand() {
    let mut s = with_settings().await;
    s.http.on("GET", "/api/v10/users/@me", [Response::ok(fixture("rest_me"))]);
    s.http.on("GET", "/api/v10/auth/sessions", [Response::ok(fixture("rest_auth_sessions"))]);
    s.http.on("GET", "/api/v10/oauth2/tokens", [Response::ok(fixture("rest_oauth2_tokens"))]);
    s.http.on("PATCH", "/api/v10/users/@me/profile", [Response::ok(json!({ "bio": "Nouvelle bio" }))]);
    s.http.on("POST", "/api/v10/auth/sessions/logout", [Response::raw(401, json!({ "message": "Vérification requise", "code": 60003 }))]);

    s.client.store.account.load_profile();
    s.client.flush();
    assert_eq!(s.http.next_request().await.path, "/api/v10/users/@me");
    s.client.until("profil", |e| matches!(account(e), Some(AccountEvent::Profile(_)))).await;
    let p = s.client.store.account.profile.ready().cloned().unwrap_or_else(|| panic!("{:?}", s.client.store.account.profile));
    assert_eq!(p.global_name.as_deref(), Some("Utilisateur Test"));

    let edit = ferrocord::state::usersettings::ProfileEdit { bio: Some("Nouvelle bio".into()), ..Default::default() };
    s.client.store.account.save_profile(edit).unwrap_or_else(|e| panic!("{e:?}"));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str(), r.json()), ("PATCH", "/api/v10/users/@me/profile", json!({ "bio": "Nouvelle bio" })));
    s.client.until("profil enregistré", |e| matches!(account(e), Some(AccountEvent::ProfileWritten { .. }))).await;
    assert_eq!(s.client.store.account.profile.ready().map(|p| p.bio.as_str()), Some("Nouvelle bio"));

    s.client.store.account.load_sessions();
    s.client.store.account.load_apps();
    s.client.flush();
    s.client.until("sessions", |e| matches!(account(e), Some(AccountEvent::Sessions(_)))).await;
    s.client.until("applications", |e| matches!(account(e), Some(AccountEvent::Apps(_)))).await;
    assert_eq!(s.client.store.account.apps.ready().map(Vec::len), Some(2));

    // Déconnexion d'un appareil qui exige une vérification : refusée, la
    // session reste ouverte (et le jeton n'est pas tenu pour mort).
    // Code 60003 : même message que l'A2F exigée par un serveur (phase 3).
    s.client.store.account.logout_session("hachage-synthetique-2").unwrap_or_else(|e| panic!("{e:?}"));
    s.client.flush();
    s.client.until("déconnexion refusée", |e| matches!(account(e), Some(AccountEvent::SessionLoggedOut { .. }))).await;
    assert!(matches!(&s.client.store.account.sessions, Remote::Ready(l) if l.len() == 2));
    assert!(s.client.effects.iter().all(|e| !matches!(e, Effect::Fatal { .. })), "{:?}", s.client.effects);
    assert!(s.client.effects.contains(&Effect::Toast(Notice::SettingNotSaved(Problem::MfaRequired))));
}

/// Données et confidentialité, Avancés, Activité : consentements (lus puis
/// changés un à la fois), mode développeur et avatar envoyé puis retiré.
#[tokio::test]
async fn consents_developer_mode_and_avatar() {
    use ferrocord::state::crop::{data_uri, ImageKind};
    use ferrocord::state::usersettings::extra::{ConsentKind, ExtraEvent, ImageOutcome, ImageTarget};
    let mut s = with_settings().await;
    let extra = |e: &Event| matches!(account(e), Some(AccountEvent::Extra(_)));
    s.http.on("GET", "/api/v10/users/@me/consent", [Response::ok(fixture("rest_consent"))]);
    s.http.on("POST", "/api/v10/users/@me/consent", [Response::ok(json!({ "personalization": { "consented": false } }))]);
    s.client.store.account.extra.load_consents();
    s.client.flush();
    assert_eq!(s.http.next_request().await.path, "/api/v10/users/@me/consent");
    s.client.until("consentements", extra).await;
    let c = s.client.store.account.extra.consents.ready().copied().unwrap_or_else(|| panic!("consentements"));
    assert_eq!((c.personalization, c.usage_statistics), (Some(true), Some(false)));
    s.client.store.account.extra.set_consent(ConsentKind::Personalization, false).unwrap_or_else(|e| panic!("{e:?}"));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.json()), ("POST", json!({ "grant": [], "revoke": ["personalization"] })));
    s.client.until("consentement écrit", extra).await;
    assert_eq!(s.client.store.account.extra.consents.ready().and_then(|c| c.personalization), Some(false));

    // Mode développeur : un seul champ de `appearance`.
    assert_eq!(s.client.store.account.bool(Key::DeveloperMode), Some(false));
    s.http.on("PATCH", PROTO_PATH, [Response::ok(json!({ "settings": "" }))]);
    s.client.store.account.write(Key::DeveloperMode, SValue::Bool(true)).unwrap_or_else(|e| panic!("{e:?}"));
    s.client.flush();
    let (sent, _) = sent_proto(&s.http.next_request().await.body);
    assert_eq!(sent.top_numbers(), vec![top::APPEARANCE]);
    assert_eq!(sent.message(top::APPEARANCE).ok().flatten().and_then(|a| a.varint(2)), Some(1));
    s.client.until("écriture confirmée", |e| matches!(account(e), Some(AccountEvent::ProtoWritten { .. }))).await;

    // Avatar du compte, puis bannière d'un serveur retirée (refusée : Nitro).
    let me = ImageTarget { kind: ImageKind::Avatar, guild: None };
    let data = data_uri(&[0x89, b'P', b'N', b'G']);
    s.http.on("PATCH", "/api/v10/users/@me", [Response::ok(fixture("rest_me"))]);
    s.client.store.account.extra.set_image(me, Some(data.clone())).unwrap_or_else(|e| panic!("{e:?}"));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str(), r.json()), ("PATCH", "/api/v10/users/@me", json!({ "avatar": data })));
    s.client.until("avatar envoyé", |e| matches!(account(e), Some(AccountEvent::Extra(ExtraEvent::ImageWritten { .. })))).await;
    assert_eq!(s.client.store.account.extra.image_outcome, Some((me, ImageOutcome::Sent)));

    let banner = ImageTarget { kind: ImageKind::Banner, guild: Some(ALPHA) };
    let path = format!("/api/v10/guilds/{ALPHA}/members/@me");
    s.http.on("PATCH", &path, [Response::error(400, "Nitro requis")]);
    s.client.store.account.extra.set_image(banner, None).unwrap_or_else(|e| panic!("{e:?}"));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.path.as_str(), r.json()), (path.as_str(), json!({ "banner": null })));
    s.client.until("bannière refusée", |e| matches!(account(e), Some(AccountEvent::Extra(ExtraEvent::ImageWritten { .. })))).await;
    assert_eq!(s.client.store.account.extra.image_outcome, Some((banner, ImageOutcome::Failed)));
    assert!(s.client.effects.contains(&Effect::Toast(Notice::SettingNotSaved(Problem::Api("Nitro requis".into())))), "{:?}", s.client.effects);
}

/// Liste des salons (phase 10, lot S) : replier une catégorie puis toutes, et
/// masquer les salons muets ; chaque geste est une écriture, annulée si refusée.
#[tokio::test]
async fn categories_fold_and_muted_channels_hide() {
    const DISCUSSIONS: u64 = 3000000000000000010;
    const ARCHIVES: u64 = 3000000000000000020;
    let mut s = with_settings().await;
    let path = format!("/api/v10/users/@me/guilds/{ALPHA}/settings");
    let entry = |collapsed: &[u64], hide: bool| {
        let mut overrides = vec![json!({ "channel_id": GENERAL.to_string(), "muted": true, "mute_config": { "end_time": null, "selected_time_window": -1 }, "message_notifications": 3 })];
        overrides.extend(collapsed.iter().map(|c| json!({ "channel_id": c.to_string(), "muted": false, "message_notifications": 3, "collapsed": true })));
        json!({ "guild_id": ALPHA.to_string(), "muted": false, "message_notifications": 1, "hide_muted_channels": hide, "channel_overrides": overrides })
    };
    let written = |e: &Event| matches!(account(e), Some(AccountEvent::GuildWritten { .. }));

    s.http.on("PATCH", &path, [Response::ok(entry(&[DISCUSSIONS], false)), Response::ok(entry(&[DISCUSSIONS, ARCHIVES], false)), Response::ok(entry(&[DISCUSSIONS, ARCHIVES], true)), Response::error(403, "Missing Access")]);
    s.client.store.toggle_category(ALPHA, DISCUSSIONS);
    assert!(s.client.store.list_rules(ALPHA).collapsed(DISCUSSIONS), "affiché sans attendre");
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("PATCH", path.as_str()));
    assert_eq!(r.json(), json!({ "channel_overrides": { DISCUSSIONS.to_string(): { "collapsed": true } } }));
    s.client.until("repli confirmé", written).await;

    // « Tout replier » : seulement les catégories encore dépliées, en une requête.
    s.client.store.collapse_all_categories(ALPHA);
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.json(), json!({ "channel_overrides": { ARCHIVES.to_string(): { "collapsed": true } } }));
    s.client.until("tout replié", written).await;
    let rules = s.client.store.list_rules(ALPHA);
    assert!(rules.collapsed(DISCUSSIONS) && rules.collapsed(ARCHIVES));

    // #général est muet : masqué, sauf s'il est ouvert ou mentionné (règle testée dans `state::chanlist`).
    s.client.store.set_hide_muted_channels(ALPHA, true).unwrap_or_else(|e| panic!("{e:?}"));
    s.client.flush();
    assert_eq!(s.http.next_request().await.json(), json!({ "hide_muted_channels": true }));
    s.client.until("muets masqués", written).await;
    assert!(s.client.store.hides_muted_channels(ALPHA));

    // Refus : l'affichage revient en arrière.
    s.client.store.toggle_category(ALPHA, DISCUSSIONS);
    assert!(!s.client.store.list_rules(ALPHA).collapsed(DISCUSSIONS));
    s.client.flush();
    assert_eq!(s.http.next_request().await.json(), json!({ "channel_overrides": { DISCUSSIONS.to_string(): { "collapsed": false } } }));
    s.client.until("refus", written).await;
    assert!(s.client.store.list_rules(ALPHA).collapsed(DISCUSSIONS), "annulé");
}
