//! API REST : historique, envoi, limites de débit, erreurs.

use crate::support::{ids::*, *};
use ferrocord::state::{Cmd, Effect, Event, Notice, Problem, UnsentState};
use serde_json::{json, Value};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

/// Page d'historique pleine (50 messages), du plus récent au plus ancien,
/// d'identifiants `base + 49` … `base`.
fn full_page(base: u64) -> Value {
    let template = fixture("rest_history")[0].clone();
    let page: Vec<Value> = (0..50u64)
        .rev()
        .map(|i| {
            let mut m = template.clone();
            m["id"] = json!((base + i).to_string());
            m
        })
        .collect();
    json!(page)
}

/// Textes gardés pour l'utilisateur dans un salon (messages non partis).
fn unsent(s: &Setup, channel: u64) -> Vec<(String, UnsentState)> {
    s.client.store.unsent.get(&channel).map(|l| l.iter().map(|u| (u.content.clone(), u.state.clone())).collect()).unwrap_or_default()
}

#[tokio::test]
async fn history_first_page_then_before() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let base = 5000000000000001000;
    s.http.on("GET", &messages_path(GENERAL), [Response::ok(full_page(base)), Response::ok(fixture("rest_history"))]);

    s.client.store.select_channel(GENERAL);
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.method, "GET");
    assert_eq!(r.path, format!("/api/v10/channels/{GENERAL}/messages?limit=50"));
    // Jeton utilisateur : en-tête Authorization exactement égal au jeton, sans préfixe.
    assert_eq!(r.authorization.as_deref(), Some(USER_TOKEN));
    assert_eq!(r.body, "");
    s.client.until("première page", |e| matches!(e, Event::History { before: None, .. })).await;
    let cs = &s.client.store.chans[&GENERAL];
    assert_eq!(cs.msgs.len(), 50);
    assert_eq!((cs.msgs[0].id, cs.msgs[49].id), (base, base + 49));
    assert!(cs.more, "page pleine : il en reste");

    assert!(s.client.store.load_older(GENERAL));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.path, format!("/api/v10/channels/{GENERAL}/messages?limit=50&before={base}"));
    assert_eq!(r.authorization.as_deref(), Some(USER_TOKEN));
    s.client.until("page précédente", |e| matches!(e, Event::History { before: Some(_), .. })).await;
    let cs = &s.client.store.chans[&GENERAL];
    assert_eq!(cs.msgs.len(), 53);
    let head: Vec<u64> = cs.msgs.iter().take(4).map(|m| m.id).collect();
    assert_eq!(head, vec![5000000000000000201, 5000000000000000202, 5000000000000000203, base]);
    assert!(!cs.more && !cs.loading);
}

/// Messages d'identifiants `lo..hi`, du plus récent au plus ancien : l'ordre
/// documenté par Discord pour Get Channel Messages (« from newest to oldest »),
/// avec `before` comme avec `after`.
fn page_desc(lo: u64, hi: u64) -> Value {
    let template = fixture("rest_history")[0].clone();
    json!((lo..hi)
        .rev()
        .map(|id| {
            let mut m = template.clone();
            m["id"] = json!(id.to_string());
            m
        })
        .collect::<Vec<Value>>())
}

/// Fenêtre remontée loin (les plus récents évincés), puis redescente page
/// par page avec `after` : URL `?limit=50&after=…`, pages rendues du plus
/// récent au plus ancien, enchaînées jusqu'au présent sans trou ni doublon,
/// puis le direct reprend à la suite.
#[tokio::test]
async fn history_after_walks_back_to_present() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    // Salon de 700 messages : base … base + 699.
    let base = 5000000000000010000;
    let total = 700;
    let older: Vec<Response> = (1..=11u64).map(|j| Response::ok(page_desc(base + 650 - 50 * j, base + 700 - 50 * j))).collect();
    let mut responses = vec![Response::ok(page_desc(base + 650, base + 700))];
    responses.extend(older);
    // Redescente : deux pages pleines, puis une page vide (le présent est atteint).
    responses.extend([Response::ok(page_desc(base + 600, base + 650)), Response::ok(page_desc(base + 650, base + 700)), Response::ok(json!([]))]);
    s.http.on("GET", &messages_path(GENERAL), responses);

    s.client.store.select_channel(GENERAL);
    s.client.flush();
    s.client.until("dernière page", |e| matches!(e, Event::History { before: None, .. })).await;
    for _ in 0..11 {
        let front = s.client.store.chans[&GENERAL].msgs.front().map_or(0, |m| m.id);
        s.client.store.set_viewing(GENERAL, Some((front, front + 3)));
        assert!(s.client.store.load_older(GENERAL));
        s.client.flush();
        s.client.until("page plus ancienne", |e| matches!(e, Event::History { before: Some(_), .. })).await;
    }
    let cs = &s.client.store.chans[&GENERAL];
    assert!(cs.newer, "les plus récents ont été évincés");
    assert_eq!(cs.msgs.len(), 500);
    assert_eq!((cs.msgs.front().map(|m| m.id), cs.msgs.back().map(|m| m.id)), (Some(base + 100), Some(base + 599)));

    let mut pages = 0;
    while s.client.store.chans[&GENERAL].newer {
        pages += 1;
        assert!(pages <= 3);
        let back = s.client.store.chans[&GENERAL].msgs.back().map_or(0, |m| m.id);
        s.client.store.set_viewing(GENERAL, Some((back - 3, back)));
        assert!(s.client.store.load_newer(GENERAL));
        s.client.flush();
        s.client.until("page plus récente", |e| matches!(e, Event::HistoryAfter { after, .. } if *after == back)).await;
    }
    assert_eq!(pages, 3);
    let paths: Vec<String> = s.http.requests().iter().map(|r| r.path.clone()).collect();
    let afters: Vec<&String> = paths.iter().filter(|p| p.contains("after=")).collect();
    assert_eq!(
        afters,
        vec![
            &format!("/api/v10/channels/{GENERAL}/messages?limit=50&after={}", base + 599),
            &format!("/api/v10/channels/{GENERAL}/messages?limit=50&after={}", base + 649),
            &format!("/api/v10/channels/{GENERAL}/messages?limit=50&after={}", base + 699),
        ]
    );
    // Sans trou ni doublon jusqu'au dernier message du salon.
    let cs = &s.client.store.chans[&GENERAL];
    let ids: Vec<u64> = cs.msgs.iter().map(|m| m.id).collect();
    assert!(ids.windows(2).all(|w| w[1] == w[0] + 1), "trou ou doublon");
    assert_eq!(ids.last(), Some(&(base + total - 1)));
    assert!(ids.len() <= 500 && cs.more);

    // Le direct reprend à la suite.
    let mut p = fixture("message_create_simple");
    p["d"]["id"] = json!((base + total).to_string());
    p["s"] = json!(10);
    s.conn.send(p).await;
    s.client.until("message en direct", |e| matches!(e, Event::MessageCreate(m) if m.id == base + total)).await;
    let cs = &s.client.store.chans[&GENERAL];
    assert_eq!(cs.msgs.back().map(|m| m.id), Some(base + total));
    assert!(!cs.newer && cs.missed() == 0);
}

#[tokio::test]
async fn history_403_sets_channel_error() {
    let mut s = connected(BOT_TOKEN, "ready_bot").await;
    s.http.on("GET", &messages_path(HORS_SUJET), [Response::error(403, "Missing Access")]);
    s.client.store.select_channel(HORS_SUJET);
    s.client.flush();
    s.client.until("erreur d'historique", |e| matches!(e, Event::HistoryError { .. })).await;
    let cs = &s.client.store.chans[&HORS_SUJET];
    assert_eq!(cs.error, Some(Problem::Forbidden));
    assert!(!cs.loading && !cs.loaded);
    // Jeton de bot : le préfixe « Bot » fait partie de l'en-tête.
    assert_eq!(s.http.requests()[0].authorization.as_deref(), Some(BOT_TOKEN));
}

#[tokio::test]
async fn send_message_body_and_order() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let created = fixture("message_create_simple")["d"].clone();
    // Première réponse lente : si les envois étaient parallèles, le second
    // arriverait pendant que le premier est encore en cours.
    s.http.on(
        "POST",
        &messages_path(GENERAL),
        [Response::ok(created.clone()).delayed(Duration::from_millis(150)), Response::ok(created)],
    );
    let n1 = s.client.post(GENERAL, "premier");
    let n2 = s.client.post(GENERAL, "second");

    let r1 = s.http.next_request().await;
    let r2 = s.http.next_request().await;
    for (r, text, nonce) in [(&r1, "premier", n1), (&r2, "second", n2)] {
        assert_eq!(r.method, "POST");
        assert_eq!(r.path, messages_path(GENERAL));
        assert_eq!(r.authorization.as_deref(), Some(USER_TOKEN));
        let b = r.json();
        assert_eq!(b["content"], text);
        assert_eq!(b["tts"], false);
        // Nonce texte (le nombre dépasse 2^53 en snowflake), vérifié par Discord.
        assert_eq!(b["nonce"], nonce.to_string());
        assert_eq!(b["enforce_nonce"], true);
        assert_eq!(b.as_object().map(|o| o.len()), Some(4), "champs inattendus : {b}");
    }
    assert_eq!(r2.in_flight, 0, "le second envoi doit attendre la réponse au premier");
    s.client.until("confirmation", |e| matches!(e, Event::SendConfirmed { nonce, .. } if *nonce == n2)).await;
}

#[tokio::test]
async fn rate_limit_waits_then_retries() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on(
        "POST",
        &messages_path(GENERAL),
        [Response::rate_limited(0.05), Response::ok(json!({})), Response::error(500, "Erreur interne fictive")],
    );
    s.client.post(GENERAL, "patience");
    // Témoin dans le même salon : les envois d'un salon partent dans
    // l'ordre, son erreur prouve donc que le premier est terminé.
    s.client.post(GENERAL, "témoin");

    let r1 = s.http.next_request().await;
    let r2 = s.http.next_request().await;
    assert!(r1.path == messages_path(GENERAL) && r2.path == r1.path);
    assert!(r2.received_at - r1.received_at >= Duration::from_millis(50), "retry_after non respecté");
    assert_eq!(r1.body, r2.body, "même message (et même nonce) au nouvel essai");

    s.client.until("erreur du témoin", |e| matches!(e, Event::SendError { .. })).await;
    assert_eq!(s.client.effects, vec![Effect::Toast(Notice::NotSent(Problem::Api("Erreur interne fictive".into())))]);
    let reqs = s.http.requests();
    assert_eq!(reqs.len(), 3);
    assert_eq!(reqs[2].json()["content"], "témoin");
    // Le texte refusé reste à l'utilisateur.
    assert_eq!(unsent(&s, GENERAL), vec![("témoin".into(), UnsentState::Failed(Problem::Api("Erreur interne fictive".into())))]);
}

#[tokio::test]
async fn rate_limit_gives_up_after_three_attempts() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let retry = Duration::from_millis(1200);
    s.http.on("POST", &messages_path(GENERAL), [Response::rate_limited(retry.as_secs_f64())]);
    s.client.post(GENERAL, "insistant");
    s.client.until("abandon", |e| matches!(e, Event::SendError { .. })).await;
    let failed_at = tokio::time::Instant::now();
    assert_eq!(
        s.client.effects,
        vec![Effect::Toast(Notice::NotSent(Problem::TooManyRequests))]
    );
    let reqs = s.http.requests();
    assert_eq!(reqs.len(), 3);
    for w in reqs.windows(2) {
        assert!(w[1].received_at - w[0].received_at >= retry, "retry_after non respecté entre deux essais");
    }
    // Après le dernier refus, l'échec est annoncé sans attendre retry_after.
    let tail = failed_at - reqs[2].received_at;
    assert!(tail < retry, "attente inutile après le dernier essai : {tail:?}");
    assert_eq!(unsent(&s, GENERAL).len(), 1, "le texte doit rester à l'utilisateur");
}

#[tokio::test]
async fn bad_retry_after_values_do_not_break_sending() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let limited = |v: Value| Response::raw(429, json!({ "message": "You are being rate limited.", "retry_after": v, "global": false }));
    let ok = Response::ok(json!({}));
    let witness = Response::error(500, "Erreur interne fictive");
    s.http.on("POST", &messages_path(GENERAL), [limited(json!(-5.0)), ok.clone(), witness.clone()]);
    s.http.on("POST", &messages_path(HORS_SUJET), [limited(json!("bientôt")), ok, witness]);
    for (channel_id, text) in [(GENERAL, "négatif"), (HORS_SUJET, "non numérique")] {
        s.client.post(channel_id, text);
        // Témoin : son erreur prouve que l'envoi précédent du salon est terminé sans erreur.
        s.client.post(channel_id, "témoin");
    }
    let errors = AtomicUsize::new(0);
    s.client
        .until("erreurs des deux témoins", |e| matches!(e, Event::SendError { .. }) && errors.fetch_add(1, Ordering::SeqCst) == 1)
        .await;
    let toast = || Effect::Toast(Notice::NotSent(Problem::Api("Erreur interne fictive".into())));
    assert_eq!(s.client.effects, vec![toast(), toast()]);

    let reqs = s.http.requests();
    let of = |p: String| reqs.iter().filter(|r| r.path == p).cloned().collect::<Vec<_>>();
    let (g, h) = (of(messages_path(GENERAL)), of(messages_path(HORS_SUJET)));
    assert_eq!((g.len(), h.len()), (3, 3));
    // Négatif : nouvel essai sans attendre la seconde par défaut ; non numérique : 1 s.
    assert!(g[1].received_at - g[0].received_at < Duration::from_millis(900));
    assert!(h[1].received_at - h[0].received_at >= Duration::from_secs(1));
    // Un salon ralenti ne retarde pas l'autre.
    assert!(g[2].received_at < h[1].received_at, "les salons devraient être indépendants");
}

// ------------------------------------------------------------ envois incertains

#[tokio::test]
async fn lost_response_is_unconfirmed_until_the_gateway_echoes_the_nonce() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    // La requête arrive, la réponse est coupée : le message est peut-être parti.
    s.http.on("POST", &messages_path(GENERAL), [Response::ok(json!({ "id": "1" })).cut()]);
    let nonce = s.client.post(GENERAL, "peut-être");
    s.client.until("envoi non confirmé", |e| matches!(e, Event::SendUnconfirmed { .. })).await;
    assert_eq!(s.client.effects, vec![Effect::Toast(Notice::Unconfirmed)]);
    assert_eq!(unsent(&s, GENERAL), vec![("peut-être".into(), UnsentState::Unconfirmed)]);
    assert_eq!(s.http.requests().len(), 1, "aucun nouvel essai automatique");

    // Discord diffuse le message avec ce nonce : il est bien parti.
    let mut m = fixture("message_create_simple");
    m["s"] = json!(2);
    m["d"]["author"]["id"] = json!(ME.to_string());
    m["d"]["content"] = json!("peut-être");
    m["d"]["nonce"] = json!(nonce.to_string());
    s.conn.send(m).await;
    s.client.until("confirmation par la passerelle", |e| matches!(e, Event::SendConfirmed { nonce: n, .. } if *n == nonce)).await;
    assert!(unsent(&s, GENERAL).is_empty());
}

#[tokio::test]
async fn message_of_someone_else_with_same_nonce_confirms_nothing() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("POST", &messages_path(GENERAL), [Response::ok(json!({})).cut()]);
    let nonce = s.client.post(GENERAL, "à moi");
    s.client.until("envoi non confirmé", |e| matches!(e, Event::SendUnconfirmed { .. })).await;
    let mut m = fixture("message_create_simple");
    m["s"] = json!(2);
    m["d"]["nonce"] = json!(nonce.to_string());
    s.conn.send(m).await;
    s.client
        .until("message d'Alice", |e| {
            assert!(!matches!(e, Event::SendConfirmed { .. }), "le message d'un autre ne confirme rien");
            matches!(e, Event::MessageCreate(_))
        })
        .await;
    assert_eq!(unsent(&s, GENERAL).len(), 1);
}

// ------------------------------------------------------------ compartiments

#[tokio::test]
async fn exhausted_bucket_waits_before_sending_without_429() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let ok = || Response::ok(json!({}));
    // Le compartiment annonce qu'il est vide pendant 800 ms après le premier envoi.
    s.http.on("POST", &messages_path(GENERAL), [ok().bucket("b-post", 5, 0, 0.8), ok().bucket("b-post", 5, 4, 5.0)]);
    s.http.on("POST", &messages_path(HORS_SUJET), [ok().bucket("b-post", 5, 4, 5.0)]);
    s.client.post(GENERAL, "un");
    s.client.post(GENERAL, "deux");
    s.client.post(HORS_SUJET, "ailleurs");
    let mut reqs = Vec::new();
    for _ in 0..3 {
        reqs.push(s.http.next_request().await);
    }
    let first = reqs.iter().find(|r| r.json()["content"] == "un").cloned().unwrap_or_else(|| panic!("premier envoi"));
    let second = reqs.iter().find(|r| r.json()["content"] == "deux").cloned().unwrap_or_else(|| panic!("second envoi"));
    let other = reqs.iter().find(|r| r.json()["content"] == "ailleurs").cloned().unwrap_or_else(|| panic!("autre salon"));
    assert!(second.received_at - first.received_at >= Duration::from_millis(800), "parti avant la recharge");
    // Autre salon : autre compartiment (paramètre majeur), aucune attente.
    assert!(other.received_at < second.received_at);
    assert!(s.http.requests().iter().all(|r| r.method == "POST"));
}

#[tokio::test]
async fn global_limit_pauses_all_requests() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let date = chrono::Utc::now().to_rfc2822();
    s.http.on("POST", &messages_path(GENERAL), [Response::global_limited(0.4).dated(&date), Response::ok(json!({}))]);
    s.http.on("GET", &messages_path(HORS_SUJET), [Response::ok(fixture("rest_history"))]);
    s.client.post(GENERAL, "global");
    let limited = s.http.next_request().await;
    // L'heure du serveur est publiée une fois le 429 lu par le limiteur.
    s.client.until("lecture du 429", |e| matches!(e, Event::ServerTime(_))).await;
    // Une requête d'un tout autre compartiment attend elle aussi la fin de la pause globale.
    s.client.store.select_channel(HORS_SUJET);
    s.client.flush();
    let mut next = Vec::new();
    for _ in 0..2 {
        next.push(s.http.next_request().await);
    }
    for r in &next {
        assert!(r.received_at - limited.received_at >= Duration::from_millis(400), "{} parti pendant la pause globale", r.path);
    }
    s.client.until("historique", |e| matches!(e, Event::History { .. })).await;
}

#[tokio::test]
async fn firewall_429_pauses_everything_without_retry() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    // 429 sans corps de l'API ni en-tête de limite : blocage du pare-feu.
    s.http.on("POST", &messages_path(GENERAL), [Response::firewall(120), Response::ok(json!({}))]);
    s.http.on("GET", &messages_path(HORS_SUJET), [Response::ok(fixture("rest_history"))]);
    s.client.post(GENERAL, "bloqué");
    let text = s.client.until("refus", |e| matches!(e, Event::SendError { .. })).await;
    assert!(text.contains("Blocked { retry_ms"), "{text}");
    // Pendant le blocage, plus rien ne part : ni envoi…
    s.client.post(GENERAL, "suivant");
    s.client.until("second refus", |e| matches!(e, Event::SendError { .. })).await;
    // … ni historique ; le salon montre l'heure du nouvel essai.
    s.client.store.select_channel(HORS_SUJET);
    s.client.flush();
    s.client.until("historique refusé", |e| matches!(e, Event::HistoryError { .. })).await;
    let error = s.client.store.chans[&HORS_SUJET].error.clone();
    assert!(matches!(error, Some(Problem::Blocked { retry_ms }) if retry_ms > chrono::Utc::now().timestamp_millis() - 1000), "{error:?}");
    assert_eq!(s.http.requests().len(), 1, "aucun nouvel essai automatique");
    let texts: Vec<String> = unsent(&s, GENERAL).into_iter().map(|(t, _)| t).collect();
    assert_eq!(texts, vec!["bloqué".to_string(), "suivant".to_string()]);
}

/// Les pages vers le présent (`after`) passent par le même limiteur que les
/// autres et sont abandonnées de la même façon quand le salon est quitté :
/// seul le chargement du bas se termine, sans erreur.
#[tokio::test]
async fn page_after_waits_in_the_limiter_and_is_cancelled_on_leave() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let base = 5000000000000010000;
    let mut responses: Vec<Response> = vec![Response::ok(page_desc(base + 650, base + 700))];
    responses.extend((1..=10u64).map(|j| Response::ok(page_desc(base + 650 - 50 * j, base + 700 - 50 * j))));
    // Dernière page vers le passé : le compartiment est vide pour 8 s ensuite.
    responses.push(Response::ok(page_desc(base + 100, base + 150)).bucket("b-get", 5, 0, 8.0));
    s.http.on("GET", &messages_path(GENERAL), responses);
    s.http.on("GET", &messages_path(HORS_SUJET), [Response::ok(fixture("rest_history")).bucket("b-get", 5, 4, 8.0)]);
    s.client.store.select_channel(GENERAL);
    s.client.flush();
    s.client.until("dernière page", |e| matches!(e, Event::History { before: None, .. })).await;
    for _ in 0..11 {
        let front = s.client.store.chans[&GENERAL].msgs.front().map_or(0, |m| m.id);
        s.client.store.set_viewing(GENERAL, Some((front, front + 3)));
        assert!(s.client.store.load_older(GENERAL));
        s.client.flush();
        s.client.until("page plus ancienne", |e| matches!(e, Event::History { before: Some(_), .. })).await;
    }
    assert!(s.client.store.chans[&GENERAL].newer);
    let back = s.client.store.chans[&GENERAL].msgs.back().map_or(0, |m| m.id);
    s.client.store.set_viewing(GENERAL, Some((back - 3, back)));
    // La page suivante attendrait la recharge du compartiment…
    assert!(s.client.store.load_newer(GENERAL));
    s.client.flush();
    // … mais l'utilisateur part ailleurs : elle est abandonnée tout de suite.
    let t = std::time::Instant::now();
    s.client.store.select_channel(HORS_SUJET);
    s.client.flush();
    s.client.until("abandon", |e| matches!(e, Event::HistoryCancelled { channel_id, before: None, after: Some(a) } if *channel_id == GENERAL && *a == back)).await;
    assert!(t.elapsed() < Duration::from_secs(2));
    let paths: Vec<String> = s.http.requests().iter().map(|r| r.path.clone()).collect();
    assert!(paths.iter().all(|p| !p.contains("after=")), "jamais demandée à Discord : {paths:?}");
    let cs = &s.client.store.chans[&GENERAL];
    assert!(!cs.loading_newer() && !cs.loading_older() && !cs.loading);
    assert_eq!((cs.error.as_ref(), cs.error_newer.as_ref()), (None, None));
}

#[tokio::test]
async fn history_of_a_channel_left_while_waiting_is_cancelled() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let base = 5000000000000001000;
    // Après la première page, le compartiment de #général est vide pour 8 s.
    s.http.on("GET", &messages_path(GENERAL), [Response::ok(full_page(base)).bucket("b-get", 5, 0, 8.0)]);
    s.http.on("GET", &messages_path(HORS_SUJET), [Response::ok(fixture("rest_history")).bucket("b-get", 5, 4, 8.0)]);
    s.client.store.select_channel(GENERAL);
    s.client.flush();
    s.client.until("première page", |e| matches!(e, Event::History { .. })).await;
    // Page précédente : elle attendrait la recharge…
    assert!(s.client.store.load_older(GENERAL));
    s.client.flush();
    // … mais l'utilisateur part ailleurs : elle est abandonnée tout de suite.
    let t = std::time::Instant::now();
    s.client.store.select_channel(HORS_SUJET);
    s.client.flush();
    s.client.until("abandon", |e| matches!(e, Event::HistoryCancelled { channel_id, before: Some(_), after: None } if *channel_id == GENERAL)).await;
    assert!(t.elapsed() < Duration::from_secs(2));
    s.client.until("historique de l'autre salon", |e| matches!(e, Event::History { channel_id, .. } if *channel_id == HORS_SUJET)).await;
    // L'historique abandonné n'a jamais été demandé à Discord.
    let gets: Vec<String> = s.http.requests().iter().map(|r| r.path.clone()).collect();
    assert_eq!(gets.len(), 2, "{gets:?}");
    // Revenir sur #général garde la première page, sans erreur affichée.
    s.client.store.select_channel(GENERAL);
    s.client.flush();
    let cs = &s.client.store.chans[&GENERAL];
    // Le chargement vers le passé (celui annulé) est terminé, sans erreur.
    assert!(!cs.loading && !cs.loading_older());
    assert_eq!(cs.error, None, "« Chargement annulé » ne doit pas rester affiché");
    assert_eq!(cs.msgs.len(), 50);
}

#[tokio::test]
async fn wait_beyond_the_cap_fails_at_once() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    // Recharge dans 10 minutes : plus que le plafond d'un envoi.
    s.http.on("POST", &messages_path(GENERAL), [Response::ok(json!({})).bucket("b-post", 1, 0, 600.0)]);
    s.client.post(GENERAL, "un");
    s.client.post(GENERAL, "deux");
    let t = std::time::Instant::now();
    s.client.until("refus", |e| matches!(e, Event::SendError { .. })).await;
    assert!(t.elapsed() < Duration::from_secs(2));
    assert_eq!(s.client.effects, vec![Effect::Toast(Notice::NotSent(Problem::TooManyRequests))]);
    assert_eq!(s.http.requests().len(), 1, "le second envoi ne doit pas partir");
    assert_eq!(unsent(&s, GENERAL), vec![("deux".into(), UnsentState::Failed(Problem::TooManyRequests))]);
}

#[tokio::test]
async fn missing_headers_mean_one_request_at_a_time() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    // Sans en-têtes : le client ne sait rien du compartiment et n'envoie
    // pas deux requêtes de la même route en même temps.
    s.http.on("GET", &messages_path(GENERAL), [Response::ok(full_page(5000000000000001000)).delayed(Duration::from_millis(150)), Response::ok(fixture("rest_history"))]);
    s.client.send(Cmd::History { channel_id: GENERAL, before: None });
    s.client.send(Cmd::History { channel_id: GENERAL, before: Some(5000000000000001000) });
    let r1 = s.http.next_request().await;
    let r2 = s.http.next_request().await;
    assert_eq!(r1.in_flight, 0);
    assert_eq!(r2.in_flight, 0, "la seconde est partie avant la réponse à la première");
    assert!(r2.received_at - r1.received_at >= Duration::from_millis(150));
}

/// Un 401 signifie que le jeton est mort : retour à l'écran de connexion,
/// comme pour le code de fermeture 4004.
fn bad_token() -> Effect {
    Effect::Fatal { problem: Problem::BadToken, bad_token: true }
}

#[tokio::test]
async fn unauthorized_history_is_fatal() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &messages_path(GENERAL), [Response::error(401, "401: Unauthorized")]);
    s.client.store.select_channel(GENERAL);
    s.client.flush();
    s.client
        .until("Fatal", |e| {
            assert!(!matches!(e, Event::HistoryError { .. }), "401 traité comme simple erreur : {e:?}");
            matches!(e, Event::Fatal { .. })
        })
        .await;
    assert_eq!(s.client.effects, vec![bad_token()]);
}

#[tokio::test]
async fn unauthorized_send_is_fatal() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("POST", &messages_path(GENERAL), [Response::error(401, "401: Unauthorized")]);
    s.client.post(GENERAL, "refusé");
    s.client
        .until("Fatal", |e| {
            assert!(!matches!(e, Event::Error(_) | Event::SendError { .. }), "401 traité comme simple erreur : {e:?}");
            matches!(e, Event::Fatal { .. })
        })
        .await;
    assert_eq!(s.client.effects, vec![bad_token()]);
}
