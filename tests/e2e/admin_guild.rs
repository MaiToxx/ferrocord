//! Paramètres du serveur (lot A8 + A9) : lecture et enregistrement de la vue
//! d'ensemble, GUILD_UPDATE, webhooks (création, suppression avec raison),
//! intégrations, refus A2F, face au faux serveur. Rien ne quitte 127.0.0.1.

use crate::support::{ids::*, *};
use ferrocord::state::admin::guild::{AdminEvent, AdminNotice, GuildEdit, Load};
use ferrocord::state::admin::webhooks::HookScope;
use ferrocord::state::{Event, Problem};
use serde_json::{json, Value};

fn admin(e: &Event) -> Option<&AdminEvent> {
    match e {
        Event::GuildAdmin(a) => Some(a),
        _ => None,
    }
}

fn no_content() -> Response {
    Response { body: String::new(), ..Response::raw(204, Value::Null) }
}

#[tokio::test]
async fn overview_is_read_saved_and_follows_guild_update() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let path = format!("/api/v10/guilds/{ALPHA}");
    s.http.on("GET", &path, [Response::ok(fixture("rest_guild_get"))]);
    s.client.store.open_guild_admin(ALPHA);
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", format!("{path}?with_counts=true").as_str()));
    assert!(!r.headers.iter().any(|(n, _)| n == "x-super-properties"), "aucun en-tête du client officiel");
    s.client.until("serveur lu", |e| matches!(admin(e), Some(AdminEvent::Loaded { .. }))).await;
    let p = s.client.store.guild_admin.profile.as_ref().and_then(Load::ready).cloned().unwrap_or_default();
    assert_eq!((p.premium_tier, p.afk_timeout), (2, 300));
    // Le niveau de boost relève la limite d'envoi dans ce serveur.
    assert_eq!(s.client.store.upload_limit_in(GENERAL), 50 << 20);

    let mut saved = fixture("rest_guild_get");
    saved["name"] = "Alpha 2".into();
    saved["afk_timeout"] = 60.into();
    s.http.on("PATCH", &path, [Response::ok(saved)]);
    assert!(s.client.store.save_guild(ALPHA, GuildEdit { name: Some("Alpha 2".into()), afk_timeout: Some(60), icon: Some(None), ..Default::default() }));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.json()), ("PATCH", json!({ "name": "Alpha 2", "afk_timeout": 60, "icon": null })));
    s.client.until("enregistré", |e| matches!(admin(e), Some(AdminEvent::Loaded { saved: true, .. }))).await;
    assert_eq!(s.client.store.guild_admin.take_notice(), Some(AdminNotice::Saved));
    assert!(s.client.store.guilds.iter().any(|g| g.id == ALPHA && g.name == "Alpha 2"), "le rail suit");

    // GUILD_UPDATE (changement fait ailleurs) : la page suit.
    s.conn.dispatch("GUILD_UPDATE", 40, json!({ "id": ALPHA.to_string(), "name": "Alpha 3", "premium_tier": 3, "description": null })).await;
    s.client.until("mise à jour", |e| matches!(admin(e), Some(AdminEvent::Updated { .. }))).await;
    let p = s.client.store.guild_admin.profile.as_ref().and_then(Load::ready).cloned().unwrap_or_default();
    assert_eq!((p.name.as_str(), p.premium_tier, p.description), ("Alpha 3", 3, None));
    assert_eq!(s.client.store.upload_limit_in(GENERAL), 100 << 20);
}

#[tokio::test]
async fn two_factor_refusal_is_explained() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let path = format!("/api/v10/guilds/{ALPHA}");
    s.http.on("GET", &path, [Response::ok(fixture("rest_guild_get"))]);
    s.http.on("PATCH", &path, [Response::raw(403, json!({ "message": "Two factor is required for this operation", "code": 60003 }))]);
    s.client.store.open_guild_admin(ALPHA);
    s.client.flush();
    s.client.until("serveur lu", |e| matches!(admin(e), Some(AdminEvent::Loaded { .. }))).await;
    assert!(s.client.store.save_guild(ALPHA, GuildEdit { afk_timeout: Some(900), ..Default::default() }));
    s.client.flush();
    s.client.until("refus", |e| matches!(admin(e), Some(AdminEvent::Failed { .. }))).await;
    let n = s.client.store.guild_admin.take_notice();
    assert!(matches!(n, Some(AdminNotice::Failed { failure: ferrocord::state::admin::guild::Failure::Problem(Problem::MfaRequired), .. })), "{n:?}");
    assert!(!s.client.store.guild_admin.saving);
}

#[tokio::test]
async fn webhooks_are_listed_created_and_deleted_with_a_reason() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &format!("/api/v10/guilds/{ALPHA}"), [Response::ok(fixture("rest_guild_get"))]);
    s.client.store.open_guild_admin(ALPHA);
    s.client.flush();
    s.client.until("serveur lu", |e| matches!(admin(e), Some(AdminEvent::Loaded { .. }))).await;

    s.http.on("GET", &format!("/api/v10/guilds/{ALPHA}/webhooks"), [Response::ok(fixture("rest_guild_webhooks"))]);
    assert!(s.client.store.load_webhooks(HookScope::Guild(ALPHA)));
    s.client.flush();
    s.client.until("webhooks", |e| matches!(admin(e), Some(AdminEvent::Webhooks { .. }))).await;
    assert_eq!(s.client.store.channel_webhooks(ALPHA, GENERAL).map(|v| v.len()), Some(1));

    let mut created = fixture("rest_guild_webhooks")[0].clone();
    created["id"] = "6000000000000000003".into();
    created["name"] = "Alertes".into();
    s.http.on("POST", &format!("/api/v10/channels/{GENERAL}/webhooks"), [Response::ok(created)]);
    assert!(s.client.store.create_webhook(ALPHA, GENERAL, "Alertes", None));
    s.client.flush();
    s.client.until("créé", |e| matches!(admin(e), Some(AdminEvent::WebhookSaved { created: true, .. }))).await;
    let r = s.http.requests().into_iter().find(|r| r.method == "POST").unwrap_or_else(|| panic!("création"));
    assert_eq!(r.json(), json!({ "name": "Alertes" }));
    assert_eq!(s.client.store.channel_webhooks(ALPHA, GENERAL).map(|v| v.len()), Some(2));

    s.http.on("DELETE", "/api/v10/webhooks/6000000000000000003", [no_content()]);
    assert!(s.client.store.delete_webhook(ALPHA, 6000000000000000003, Some("Trop de bruit".into())));
    s.client.flush();
    s.client.until("supprimé", |e| matches!(admin(e), Some(AdminEvent::WebhookDeleted { .. }))).await;
    let r = s.http.requests().into_iter().find(|r| r.method == "DELETE").unwrap_or_else(|| panic!("suppression"));
    let reason = r.headers.iter().find(|(n, _)| n == "x-audit-log-reason").map(|(_, v)| v.as_str());
    assert_eq!(reason, Some("Trop%20de%20bruit"));
    assert_eq!(s.client.store.channel_webhooks(ALPHA, GENERAL).map(|v| v.len()), Some(1));

    // WEBHOOKS_UPDATE : la liste est à relire.
    s.conn.dispatch("WEBHOOKS_UPDATE", 41, json!({ "guild_id": ALPHA.to_string(), "channel_id": GENERAL.to_string() })).await;
    s.client.until("périmée", |e| matches!(admin(e), Some(AdminEvent::WebhooksChanged { .. }))).await;
    assert!(s.client.store.webhooks_wanted(HookScope::Guild(ALPHA)));
}

#[tokio::test]
async fn integrations_are_listed_and_removed() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &format!("/api/v10/guilds/{ALPHA}"), [Response::ok(fixture("rest_guild_get"))]);
    s.http.on("GET", &format!("/api/v10/guilds/{ALPHA}/integrations"), [Response::ok(fixture("rest_guild_integrations"))]);
    s.client.store.open_guild_admin(ALPHA);
    assert!(s.client.store.load_integrations(ALPHA));
    s.client.flush();
    s.client.until("intégrations", |e| matches!(admin(e), Some(AdminEvent::Integrations { .. }))).await;
    let path = format!("/api/v10/guilds/{ALPHA}/integrations/7000000000000000001");
    s.http.on("DELETE", &path, [no_content()]);
    assert!(s.client.store.remove_integration(ALPHA, 7000000000000000001, None));
    s.client.flush();
    s.client.until("retirée", |e| matches!(admin(e), Some(AdminEvent::IntegrationRemoved { .. }))).await;
    let r = s.http.requests().into_iter().find(|r| r.method == "DELETE").unwrap_or_else(|| panic!("retrait"));
    assert!(!r.headers.iter().any(|(n, _)| n == "x-audit-log-reason"), "sans raison, pas d'en-tête");
    assert_eq!(s.client.store.guild_admin.integrations.as_ref().and_then(Load::ready).map(Vec::len), Some(1));
}

/// Lot V (groupe 2) : widget, modèle de serveur et élagage, face au faux serveur.
#[tokio::test]
async fn widget_template_and_prune_go_through_documented_routes() {
    use ferrocord::state::admin::extras::{ExtraEvent, Widget};
    let extra = |e: &Event| match admin(e) {
        Some(AdminEvent::Extra(x)) => Some(format!("{x:?}")),
        _ => None,
    };
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &format!("/api/v10/guilds/{ALPHA}"), [Response::ok(fixture("rest_guild_get"))]);
    s.client.store.open_guild_admin(ALPHA);
    s.client.flush();
    s.client.until("serveur lu", |e| matches!(admin(e), Some(AdminEvent::Loaded { .. }))).await;

    let widget = format!("/api/v10/guilds/{ALPHA}/widget");
    s.http.on("GET", &widget, [Response::ok(fixture("rest_guild_widget"))]);
    s.http.on("PATCH", &widget, [Response::ok(json!({ "enabled": false, "channel_id": null }))]);
    assert!(s.client.store.load_widget(ALPHA));
    s.client.flush();
    s.client.until("widget lu", |e| extra(e).is_some_and(|x| x.starts_with("Widget"))).await;
    assert_eq!(s.client.store.guild_admin.extras.widget, Some(Load::Ready(Widget { enabled: true, channel_id: Some(GENERAL) })));
    assert!(s.client.store.save_widget(ALPHA, Widget::default()));
    s.client.flush();
    s.client.until("widget enregistré", |e| matches!(admin(e), Some(AdminEvent::Extra(ExtraEvent::Widget { saved: true, .. })))).await;
    let r = s.http.requests().into_iter().find(|r| r.method == "PATCH").unwrap_or_else(|| panic!("widget"));
    assert_eq!(r.json(), json!({ "enabled": false, "channel_id": null }));
    assert!(!r.headers.iter().any(|(n, _)| n == "x-super-properties"), "aucun en-tête du client officiel");

    let templates = format!("/api/v10/guilds/{ALPHA}/templates");
    s.http.on("GET", &templates, [Response::ok(fixture("rest_guild_templates"))]);
    assert!(s.client.store.load_templates(ALPHA));
    s.client.flush();
    s.client.until("modèles lus", |e| extra(e).is_some_and(|x| x.starts_with("Templates"))).await;
    let mut synced = fixture("rest_guild_templates")[0].clone();
    synced["is_dirty"] = false.into();
    s.http.on("PUT", &format!("{templates}/hgM48av5Q69A"), [Response::ok(synced)]);
    assert!(s.client.store.sync_template(ALPHA, "hgM48av5Q69A"));
    s.client.flush();
    s.client.until("modèle synchronisé", |e| extra(e).is_some_and(|x| x.starts_with("TemplateSaved"))).await;
    assert_eq!(s.client.store.guild_admin.extras.templates.as_ref().and_then(Load::ready).map(|l| l[0].dirty), Some(false));

    let prune = format!("/api/v10/guilds/{ALPHA}/prune");
    s.http.on("GET", &prune, [Response::ok(json!({ "pruned": 12 }))]);
    s.http.on("POST", &prune, [Response::ok(json!({ "pruned": 12 }))]);
    assert!(s.client.store.load_prune_count(ALPHA, 30));
    s.client.flush();
    s.client.until("compte", |e| extra(e).is_some_and(|x| x.starts_with("PruneCount"))).await;
    assert_eq!(s.client.store.guild_admin.extras.prune, Some((30, Load::Ready(12))));
    assert!(s.client.store.prune_members(ALPHA, 30, "Ménage"));
    s.client.flush();
    s.client.until("élagué", |e| extra(e).is_some_and(|x| x.starts_with("Pruned"))).await;
    let r = s.http.requests().into_iter().find(|r| r.method == "POST").unwrap_or_else(|| panic!("élagage"));
    assert_eq!(r.json(), json!({ "days": 30, "compute_prune_count": true }));
    let reason = r.headers.iter().find(|(n, _)| n == "x-audit-log-reason").map(|(_, v)| v.as_str());
    assert_eq!(reason, Some("M%C3%A9nage"));
}
