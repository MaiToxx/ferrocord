//! Lot « fiable » : ce qu'un client bancal ferait et que Ferrocord ne doit
//! pas faire (reconnexions en boucle, reprises sans fin, martèlement d'une
//! ressource refusée, nouvelles tentatives sur une écriture), prouvé contre
//! le faux serveur local.

use crate::support::{ids::*, *};
use ferrocord::state::{Cmd, Event, Status};
use serde_json::json;
use std::time::{Duration, Instant};

/// Délai de base des tests (`support::endpoints`).
const BASE: Duration = Duration::from_millis(10);

// ====================================================================== passerelle

/// Session acceptée (READY, puis RESUMED) et coupée aussitôt, en boucle :
/// avant ce lot, chaque READY remettait le compteur d'échecs à zéro et le
/// client revenait toutes les `BASE` sans jamais ralentir.
#[tokio::test]
async fn a_session_cut_right_after_ready_backs_off() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let mut at = Instant::now();
    s.conn.close(4000).await;
    let mut gaps = Vec::new();
    for round in 0..4u64 {
        let mut conn = s.gw.accept().await;
        gaps.push(at.elapsed());
        let d = conn.hello_resume(QUIET).await;
        assert_eq!(d["session_id"], "session-fictive-0001", "la session est gardée : reprise, pas d'Identify");
        conn.dispatch("RESUMED", 2 + round, json!({})).await;
        s.client.until("RESUMED", |e| matches!(e, Event::Status(Status::Connected))).await;
        at = Instant::now();
        conn.close(4000).await;
    }
    // Gigue comprise (moitié au moins du délai doublé), le quatrième retour
    // attend au moins 8 BASE ; 4 BASE toléré (machine chargée : une session
    // qui aurait traîné plus de 30 BASE compte, à raison, comme stable).
    assert!(gaps[3] >= BASE * 4, "le client revient sans ralentir : {gaps:?}");
}

/// 4008 (débit dépassé) : la session est gardée, mais le client attend une
/// fenêtre entière (60 × BASE ; 60 s en production) avant de revenir.
#[tokio::test]
async fn close_4008_waits_a_full_window_then_resumes() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let at = Instant::now();
    s.conn.close(4008).await;
    let mut conn = s.gw.accept().await;
    let waited = at.elapsed();
    assert!(waited >= BASE * 60, "revenu trop tôt après 4008 : {waited:?}");
    assert_eq!(conn.hello_resume(QUIET).await["seq"], 1);
}

/// Reprise que le serveur ferme sans RESUMED ni op 9, trois fois : la session
/// est abandonnée et un Identify neuf part par la passerelle par défaut,
/// au lieu d'une reprise retentée sans fin.
#[tokio::test]
async fn a_resume_refused_three_times_gives_way_to_a_fresh_identify() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.conn.close(4000).await;
    for _ in 0..3 {
        let mut conn = s.gw.accept().await;
        assert!(conn.path.starts_with("/resume/"), "{}", conn.path);
        conn.hello_resume(QUIET).await;
        conn.close(4000).await;
    }
    let mut conn = s.gw.accept().await;
    assert!(conn.path.starts_with("/gw/"), "session neuve : passerelle par défaut, reçu {}", conn.path);
    let d = conn.hello_identify(QUIET).await;
    assert_eq!(d["token"], USER_TOKEN);
}

/// `resume_gateway_url` qui ne répond plus (ici : un port qui coupe avant la
/// poignée de main) : après trois essais, la reprise passe par la passerelle
/// par défaut, session gardée.
#[tokio::test]
async fn a_dead_resume_address_falls_back_to_the_default_gateway() {
    let dead = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap_or_else(|e| panic!("bind : {e}"));
    let port = dead.local_addr().map(|a| a.port()).unwrap_or_else(|e| panic!("adresse : {e}"));
    let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let h = hits.clone();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = dead.accept().await {
            h.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            drop(tcp);
        }
    });
    let mut s = connected_with(USER_TOKEN, "ready_user_classic", |p| p["d"]["resume_gateway_url"] = json!(format!("ws://127.0.0.1:{port}/resume"))).await;
    s.conn.close(4000).await;
    let mut conn = s.gw.accept().await;
    assert!(conn.path.starts_with("/gw/"), "{}", conn.path);
    let d = conn.hello_resume(QUIET).await;
    assert_eq!((d["session_id"].as_str(), d["seq"].as_u64()), (Some("session-fictive-0001"), Some(1)));
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 3, "trois essais sur l'adresse morte, pas un de plus");
}

/// Connexion zombie (heartbeat jamais accusé) : fermée avec un code qui
/// garde la session (ni 1000 ni 1001), puis reprise.
#[tokio::test]
async fn a_zombie_connection_is_closed_with_a_resumable_code() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.conn.reconnect().await;
    // Op 7 : le client ferme lui aussi sans renoncer à la session.
    assert_eq!(s.conn.closed_with().await, Some(4000));
    let mut conn = s.gw.accept().await;
    conn.auto_ack = false;
    conn.hello(1000).await;
    conn.recv_op(6).await;
    conn.recv_op(1).await;
    assert_eq!(conn.closed_with().await, Some(4000), "ni 1000 ni 1001 : la session doit rester reprenable");
    assert_eq!(s.gw.accept().await.hello_resume(QUIET).await["seq"], 1);
}

/// Session invalidée, non reprenable : le client ferme en 1000 (il renonce
/// à la session) et s'identifie de nouveau après une attente tirée au hasard
/// (de `invalid_session_wait` à cinq fois plus : 1 à 5 s en production).
#[tokio::test]
async fn invalid_session_waits_a_random_time_within_bounds() {
    let mut waits = Vec::new();
    for _ in 0..6 {
        let mut s = connected(USER_TOKEN, "ready_user_classic").await;
        let at = Instant::now();
        s.conn.invalid_session(false).await;
        assert_eq!(s.conn.closed_with().await, Some(1000), "session abandonnée : fermeture normale");
        waits.push(at.elapsed());
        s.gw.accept().await.hello_identify(QUIET).await;
    }
    assert!(waits.iter().all(|w| *w >= BASE), "attente minimale non tenue : {waits:?}");
    // Six tirages identiques au dixième de milliseconde près : pas de hasard.
    let rounded: std::collections::BTreeSet<u128> = waits.iter().map(|w| w.as_micros() / 100).collect();
    assert!(rounded.len() > 1, "attentes identiques : {waits:?}");
}

// ====================================================================== REST

fn history(channel_id: u64) -> Cmd {
    Cmd::History { channel_id, before: None }
}

fn gets(http: &FakeHttp, channel: u64) -> usize {
    http.requests().iter().filter(|r| r.method == "GET" && r.path.starts_with(&messages_path(channel))).count()
}

/// Lecture refusée (403, puis 404 sur un autre salon) : redemandée aussitôt,
/// elle reçoit la même réponse SANS nouvelle requête. Un client qui martèle
/// une ressource refusée se remarque, et s'approche du blocage d'adresse.
#[tokio::test]
async fn a_refused_read_is_not_asked_again_right_away() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &messages_path(GENERAL), [Response::error(403, "Missing Access")]);
    s.http.on("GET", &messages_path(HORS_SUJET), [Response::error(404, "Unknown Channel")]);
    for channel in [GENERAL, HORS_SUJET] {
        for round in 0..4 {
            s.client.send(history(channel));
            s.client.until("échec de l'historique", |e| matches!(e, Event::HistoryError { channel_id, .. } if *channel_id == channel)).await;
            assert_eq!(gets(&s.http, channel), 1, "essai {round} du salon {channel} : une seule requête doit être partie");
        }
    }
    let h = s.client.health();
    assert_eq!((h.client_errors, h.spared, h.retries), (2, 6, 0), "{h:?}");
    // Une écriture réussie vide la mémoire : elle a pu changer la réponse
    // (rôle donné, serveur rejoint…).
    s.http.on("POST", &messages_path(DISCUSSIONS), [Response::ok(fixture("rest_history")[0].clone())]);
    s.client.post(DISCUSSIONS, "bonjour");
    s.client.until("envoi confirmé", |e| matches!(e, Event::SendConfirmed { .. })).await;
    s.client.send(history(GENERAL));
    s.client.until("nouvel échec", |e| matches!(e, Event::HistoryError { channel_id: GENERAL, .. })).await;
    assert_eq!(gets(&s.http, GENERAL), 2, "après une écriture, la lecture repart vraiment");
}

/// Panne passagère (502) sur une lecture : nouvel essai, puis succès. La
/// même panne sur un envoi de message : UNE requête, jamais renvoyée seule.
#[tokio::test]
async fn a_read_is_retried_after_a_502_but_a_write_never_is() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &messages_path(GENERAL), [Response::raw(502, json!(null)), Response::ok(fixture("rest_history"))]);
    s.client.send(history(GENERAL));
    s.client.until("historique malgré le 502", |e| matches!(e, Event::History { channel_id: GENERAL, .. })).await;
    assert_eq!(gets(&s.http, GENERAL), 2);

    s.http.on("POST", &messages_path(GENERAL), [Response::raw(502, json!(null))]);
    let nonce = s.client.post(GENERAL, "une seule fois");
    s.client.until("échec de l'envoi", |e| matches!(e, Event::SendError { nonce: n, .. } if *n == nonce)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let posts: Vec<_> = s.http.requests().into_iter().filter(|r| r.method == "POST").collect();
    assert_eq!(posts.len(), 1, "une écriture ne repart jamais automatiquement");
    assert_eq!(posts[0].json()["nonce"], nonce.to_string(), "le nonce protège le réessai manuel");
    let h = s.client.health();
    assert_eq!((h.retries, h.server_errors, h.refused()), (1, 2, 0), "{h:?}");
}

/// Panne qui dure : trois requêtes en tout (une, plus deux nouveaux essais
/// espacés), puis échec annoncé. Jamais de boucle.
#[tokio::test]
async fn a_lasting_outage_stops_after_two_retries() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &messages_path(GENERAL), [Response::raw(503, json!(null))]);
    let at = Instant::now();
    s.client.send(history(GENERAL));
    s.client.until("échec annoncé", |e| matches!(e, Event::HistoryError { channel_id: GENERAL, .. })).await;
    assert_eq!(gets(&s.http, GENERAL), 3);
    // 0,25 à 0,5 s puis 0,5 à 1 s : les essais ne partent pas en rafale.
    assert!(at.elapsed() >= Duration::from_millis(750), "essais trop rapprochés : {:?}", at.elapsed());
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(gets(&s.http, GENERAL), 3, "plus rien ne part après abandon");
}

/// 401 : le jeton est mort. Aucune nouvelle tentative, retour à la connexion.
#[tokio::test]
async fn a_401_is_never_retried() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &messages_path(GENERAL), [Response::error(401, "401: Unauthorized")]);
    s.client.send(history(GENERAL));
    s.client.until("jeton refusé", |e| matches!(e, Event::Fatal { bad_token: true, .. })).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(gets(&s.http, GENERAL), 1);
}

/// Usage normal contre le faux serveur : démarrage, ouverture de quelques
/// salons, un message. Aucune réponse 4xx ni 429, aucun nouvel essai, aucune
/// requête vers une route que le faux serveur ne connaît pas.
#[tokio::test]
async fn an_ordinary_session_produces_no_refusal_at_all() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let limits = |r: Response| r.bucket("seau-fictif", 5, 4, 1.0);
    for channel in [GENERAL, HORS_SUJET, DISCUSSIONS] {
        s.http.on("GET", &messages_path(channel), [limits(Response::ok(fixture("rest_history")))]);
    }
    s.http.on("POST", &messages_path(GENERAL), [limits(Response::ok(fixture("rest_history")[0].clone()))]);
    for channel in [GENERAL, HORS_SUJET, DISCUSSIONS, GENERAL] {
        s.client.store.select_channel(channel);
        s.client.flush();
        if s.client.store.chans.get(&channel).is_some_and(|c| c.loading) {
            s.client.until("historique", |e| matches!(e, Event::History { channel_id, .. } if *channel_id == channel)).await;
        }
    }
    s.client.post(GENERAL, "bonjour");
    s.client.until("envoi confirmé", |e| matches!(e, Event::SendConfirmed { .. })).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    s.client.drain();
    let h = s.client.health();
    assert_eq!((h.refused(), h.server_errors, h.retries, h.spared), (0, 0, 0, 0), "{h:?}");
    assert_eq!((h.sessions, h.identifies), (1, 1), "une seule connexion, un seul Identify");
    let all = s.http.requests();
    let paths: Vec<String> = all.iter().map(|r| format!("{} {}", r.method, r.path.split('?').next().unwrap_or_default())).collect();
    assert_eq!(h.requests as usize, all.len(), "{paths:?}");
    // Le salon rouvert ne redemande rien : trois lectures, une écriture.
    assert_eq!(paths.iter().filter(|p| p.starts_with("GET ")).count(), 3, "{paths:?}");
    assert_eq!(paths.iter().filter(|p| p.starts_with("POST ")).count(), 1, "{paths:?}");
}

// ====================================================================== journal

/// Rien de ce qui peut finir dans un journal ne porte le jeton : ni les
/// événements (états, erreurs, arrêt), ni les erreurs REST, même quand le
/// serveur renvoie le jeton ou le texte du message dans sa réponse d'erreur.
/// Le texte d'un message refusé ne figure pas non plus dans l'erreur.
#[tokio::test]
async fn nothing_loggable_ever_holds_the_token() {
    const SECRET: &str = "texte-prive-du-message-7391";
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.client.record = Some(Vec::new());
    // Réponses d'erreur hostiles : elles répètent le jeton et le message.
    let echo = format!("refus pour {USER_TOKEN} : {SECRET}");
    s.http.on("GET", &messages_path(GENERAL), [Response::error(403, &echo)]);
    s.http.on("GET", &messages_path(HORS_SUJET), [Response::error(404, &echo)]);
    s.http.on("GET", &messages_path(DISCUSSIONS), (0..3).map(|_| Response::error(502, &echo)));
    s.http.on("POST", &messages_path(GENERAL), [Response::error(400, &echo)]);
    for channel in [GENERAL, HORS_SUJET, DISCUSSIONS] {
        s.client.send(history(channel));
    }
    let nonce = s.client.post(GENERAL, SECRET);
    s.client.until("envoi refusé", |e| matches!(e, Event::SendError { nonce: n, .. } if *n == nonce)).await;
    // Coupure puis reprise : les états de connexion passent aussi par le journal.
    s.conn.close(4008).await;
    let mut conn = s.gw.accept().await;
    conn.hello_resume(QUIET).await;
    // Enfin le jeton est refusé : arrêt définitif.
    s.http.on("GET", &messages_path(ANNONCES), [Response::error(401, &echo)]);
    s.client.send(history(ANNONCES));
    s.client.until("jeton refusé", |e| matches!(e, Event::Fatal { bad_token: true, .. })).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    s.client.drain();
    let seen = s.client.record.take().unwrap_or_default();
    assert!(seen.len() >= 4, "{seen:?}");
    let bare = USER_TOKEN.rsplit('.').next().unwrap_or(USER_TOKEN);
    for line in &seen {
        assert!(!line.contains(USER_TOKEN) && !line.contains(bare), "jeton dans un événement : {line}");
        // Ce que le journal de débogage écrit (états, erreurs, arrêt) :
        // jamais le texte du message.
        if line.starts_with("Status") || line.starts_with("Fatal") || line.starts_with("Error") {
            assert!(!line.contains(SECRET), "contenu dans le journal : {line}");
        }
    }
    let h = format!("{:?}", s.client.health());
    assert!(!h.contains(bare));
}

// ====================================================================== redirections, rafales

/// Une redirection de l'API vers un autre hôte n'est pas suivie : les
/// en-têtes d'identité (super-propriétés, langue, fuseau) ne partent jamais
/// chez un tiers. Le second serveur ne reçoit rien.
#[tokio::test]
async fn a_redirect_to_another_host_is_never_followed() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let other = FakeHttp::start().await;
    let elsewhere = format!("{}/channels/{GENERAL}/messages", other.api());
    s.http.on("GET", &messages_path(GENERAL), [Response::raw(302, json!(null)).header("Location", &elsewhere)]);
    s.client.send(history(GENERAL));
    s.client.until("échec de l'historique", |e| matches!(e, Event::HistoryError { channel_id: GENERAL, .. })).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(other.requests().is_empty(), "la redirection a été suivie hors de l'hôte demandé");
    assert_eq!(gets(&s.http, GENERAL), 1);
}

/// Suppression d'une sélection de messages (compte utilisateur : un par
/// un) : jamais dos à dos, une courte pause irrégulière sépare deux requêtes.
#[tokio::test]
async fn selected_messages_are_not_deleted_back_to_back() {
    use ferrocord::state::admin::moderation::ModCmd;
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let now = s.client.store.now();
    let id = |n: u64| ((u64::try_from(now - 1_420_070_400_000).unwrap_or(0) - 1000) << 22) + n;
    let ids = vec![id(1), id(2), id(3)];
    for m in &ids {
        s.http.on("DELETE", &format!("/api/v10/channels/{GENERAL}/messages/{m}"), [Response { body: String::new(), ..Response::raw(204, json!(null)) }]);
    }
    s.client.send(Cmd::Moderation(ModCmd::BulkDelete { channel_id: GENERAL, ids, now_ms: now }));
    s.http.next_request().await;
    let mut last = Instant::now();
    for _ in 0..2 {
        s.http.next_request().await;
        let gap = last.elapsed();
        assert!(gap >= Duration::from_millis(300), "suppressions trop rapprochées : {gap:?}");
        last = Instant::now();
    }
}
