//! Passerelle : identification, READY, reprise, reconnexions, heartbeats.

use crate::support::{ids::*, *};
use ferrocord::state::store::Effect;
use ferrocord::state::{Event, Problem, Status};
use serde_json::json;
use std::time::{Duration, Instant};

fn guild_ids(c: &Client) -> Vec<u64> {
    c.store.guilds.iter().map(|g| g.id).collect()
}

fn channel_ids(c: &Client, guild: u64) -> Vec<u64> {
    c.store.guilds.iter().find(|g| g.id == guild).map(|g| g.channels.iter().map(|c| c.id).collect()).unwrap_or_default()
}

#[tokio::test]
async fn user_identify_then_ready_fills_store() {
    let s = connected(USER_TOKEN, "ready_user_classic").await;

    // Identify d'un compte utilisateur : jeton tel quel, propriétés, pas d'intents.
    let d = &s.identify;
    assert_eq!(d["token"], USER_TOKEN);
    assert_eq!(d["properties"]["os"], std::env::consts::OS);
    assert_eq!(d["properties"]["browser"], "Ferrocord");
    assert_eq!(d["properties"]["device"], "Ferrocord");
    assert_eq!(d["compress"], false);
    assert!(d.get("intents").is_none(), "pas d'intents pour un jeton utilisateur : {d}");
    assert_eq!(s.conn.path, "/gw/?v=10&encoding=json&compress=zlib-stream");

    let c = &s.client;
    let me = c.store.me.as_ref().unwrap_or_else(|| panic!("utilisateur attendu"));
    assert_eq!((me.id, me.name.as_str()), (ME, "Moi Fictif"));
    // Ordre de user_settings.guild_positions.
    assert_eq!(guild_ids(c), vec![GAMMA, ALPHA, BETA]);
    assert_eq!(c.store.guilds[1].name, "Serveur Alpha");
    // Sans catégorie d'abord, puis chaque catégorie suivie de ses salons ; le vocal est écarté.
    assert_eq!(channel_ids(c, ALPHA), vec![ANNONCES, DISCUSSIONS, GENERAL, HORS_SUJET, ARCHIVES, ANCIEN]);
    // MP triés du plus récent au plus ancien, noms tirés des destinataires.
    let dms: Vec<(u64, &str)> = c.store.dms.iter().map(|d| (d.id, d.name.as_str())).collect();
    assert_eq!(dms, vec![(DM_GROUP, "bob_fictif, Ève"), (DM_ALICE, "Alice")]);
    assert_eq!(c.store.status, Status::Connected);
}

#[tokio::test]
async fn user_ready_with_properties_and_recipient_ids() {
    let s = connected(USER_TOKEN, "ready_user_properties").await;
    let c = &s.client;
    let me = c.store.me.as_ref().unwrap_or_else(|| panic!("utilisateur attendu"));
    // global_name nul : le nom d'utilisateur est affiché.
    assert_eq!(me.name, "moi_fictif");
    // Sans user_settings : ordre reçu ; noms lus dans `properties`.
    assert_eq!(guild_ids(c), vec![BETA, ALPHA]);
    assert_eq!(c.store.guilds[0].name, "Serveur Bêta");
    assert_eq!(channel_ids(c, ALPHA), vec![DISCUSSIONS, GENERAL, HORS_SUJET]);
    let dms: Vec<(u64, &str)> = c.store.dms.iter().map(|d| (d.id, d.name.as_str())).collect();
    assert_eq!(
        dms,
        vec![(4000000000000000003, "bob_fictif, Ève"), (DM_ALICE, "Alice"), (DM_GROUP, "Groupe fictif")]
    );
}

#[tokio::test]
async fn bot_ready_then_guild_create_and_delete() {
    let mut s = connected(BOT_TOKEN, "ready_bot").await;
    assert_eq!(s.identify["token"], BOT_TOKEN);
    // GUILDS | GUILD_EXPRESSIONS | GUILD_MESSAGES | GUILD_MESSAGE_REACTIONS | GUILD_MESSAGE_TYPING
    // | DIRECT_MESSAGES | DIRECT_MESSAGE_REACTIONS | DIRECT_MESSAGE_TYPING | MESSAGE_CONTENT
    assert_eq!(s.identify["intents"], 1 | 8 | 512 | 1024 | 2048 | 4096 | 8192 | 16384 | 32768);
    // Les serveurs « unavailable » du READY ne sont pas affichés…
    assert!(s.client.store.guilds.is_empty());
    assert_eq!(s.client.store.me.as_ref().map(|u| u.name.as_str()), Some("robot_fictif"));

    // … ils arrivent ensuite par GUILD_CREATE, dans l'ordre d'arrivée.
    s.conn.dispatch_fixture("guild_create_alpha", 2).await;
    s.conn.dispatch_fixture("guild_create_beta", 3).await;
    s.client.until("GUILD_CREATE bêta", |e| matches!(e, Event::GuildCreate(g) if g.id == BETA)).await;
    assert_eq!(guild_ids(&s.client), vec![ALPHA, BETA]);
    assert_eq!(channel_ids(&s.client, ALPHA), vec![ANNONCES, DISCUSSIONS, GENERAL, HORS_SUJET]);
    assert_eq!(channel_ids(&s.client, BETA), vec![ACCUEIL]);

    // Panne (unavailable) : ignorée. Départ du serveur : retiré.
    s.conn.dispatch_fixture("guild_delete_unavailable", 4).await;
    s.conn.dispatch_fixture("guild_delete", 5).await;
    s.client.until("GUILD_DELETE", |e| matches!(e, Event::GuildDelete(_))).await;
    assert_eq!(guild_ids(&s.client), vec![ALPHA]);
}

#[tokio::test]
async fn channel_events_update_store() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.conn.dispatch_fixture("channel_create", 2).await;
    s.client.until("CHANNEL_CREATE", |e| matches!(e, Event::ChannelUpsert { .. })).await;
    assert_eq!(channel_ids(&s.client, ALPHA), vec![ANNONCES, DISCUSSIONS, GENERAL, HORS_SUJET, NOUVEAU, ARCHIVES, ANCIEN]);

    // Renommé et déplacé dans une autre catégorie : retrié.
    s.conn.dispatch_fixture("channel_update", 3).await;
    s.client.until("CHANNEL_UPDATE", |e| matches!(e, Event::ChannelUpsert { .. })).await;
    assert_eq!(channel_ids(&s.client, ALPHA), vec![ANNONCES, DISCUSSIONS, GENERAL, NOUVEAU, ARCHIVES, ANCIEN, HORS_SUJET]);
    assert_eq!(s.client.store.channel_name(HORS_SUJET), Some("discussion-libre"));

    // Suppression du salon affiché : il est retiré et désélectionné.
    s.client.store.select_channel(GENERAL);
    s.conn.dispatch_fixture("channel_delete", 4).await;
    s.client.until("CHANNEL_DELETE", |e| matches!(e, Event::ChannelDelete { .. })).await;
    assert!(!channel_ids(&s.client, ALPHA).contains(&GENERAL));
    assert_eq!(s.client.store.channel, None);

    // Nouveau MP : en tête de liste, nommé d'après ses destinataires.
    s.conn.dispatch_fixture("channel_create_dm", 5).await;
    s.client.until("CHANNEL_CREATE (MP)", |e| matches!(e, Event::DmUpsert { .. })).await;
    assert_eq!(s.client.store.dms[0].name, "Carol");
}

#[tokio::test]
async fn close_4004_is_fatal_bad_token() {
    let mut gw = FakeGateway::start().await;
    let http = FakeHttp::start().await;
    let mut client = Client::start(USER_TOKEN, endpoints(&gw, &http));
    let mut conn = gw.accept().await;
    conn.hello_identify(QUIET).await;
    conn.close(4004).await;
    client.until("Fatal", |e| matches!(e, Event::Fatal { .. })).await;
    assert_eq!(
        client.effects.last(),
        Some(&Effect::Fatal { problem: Problem::BadToken, bad_token: true })
    );
}

/// READY (s = 1) puis un dispatch (s = 2) : la reprise doit annoncer seq 2.
async fn ready_then_seq_2() -> Setup {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.conn.dispatch_fixture("channel_create", 2).await;
    s.client.until("CHANNEL_CREATE", |e| matches!(e, Event::ChannelUpsert { .. })).await;
    s
}

async fn expect_resume(gw: &mut FakeGateway, seq: u64) -> GwConn {
    let mut conn = gw.accept().await;
    assert_eq!(conn.path, "/resume/?v=10&encoding=json&compress=zlib-stream", "la reprise passe par resume_gateway_url");
    let d = conn.hello_resume(QUIET).await;
    assert_eq!(d["token"], USER_TOKEN);
    assert_eq!(d["session_id"], "session-fictive-0001");
    assert_eq!(d["seq"], seq);
    conn
}

#[tokio::test]
async fn op7_reconnects_and_resumes() {
    let mut s = ready_then_seq_2().await;
    s.conn.reconnect().await;
    let mut conn = expect_resume(&mut s.gw, 2).await;
    conn.dispatch("RESUMED", 3, json!({})).await;
    s.client.until("statut après RESUMED", |e| matches!(e, Event::Status(Status::Connected))).await;
    // La session continue : le magasin n'a pas été vidé.
    assert_eq!(s.client.store.guilds.len(), 3);
}

#[tokio::test]
async fn op9_not_resumable_identifies_again() {
    let mut s = ready_then_seq_2().await;
    s.conn.invalid_session(false).await;
    let mut conn = s.gw.accept().await;
    assert_eq!(conn.path, "/gw/?v=10&encoding=json&compress=zlib-stream", "nouvelle session : passerelle normale");
    let d = conn.hello_identify(QUIET).await;
    assert_eq!(d["token"], USER_TOKEN);
}

#[tokio::test]
async fn op9_resumable_resumes() {
    let mut s = ready_then_seq_2().await;
    s.conn.invalid_session(true).await;
    expect_resume(&mut s.gw, 2).await;
}

#[tokio::test]
async fn heartbeat_request_answered_with_last_seq() {
    let mut s = ready_then_seq_2().await;
    s.conn.dispatch_fixture("message_create_simple", 7).await;
    s.conn.request_heartbeat().await;
    let hb = s.conn.recv_op(1).await;
    assert_eq!(hb["d"], 7);
}

#[tokio::test]
async fn periodic_heartbeat_carries_seq() {
    let mut gw = FakeGateway::start().await;
    let http = FakeHttp::start().await;
    let _client = Client::start(USER_TOKEN, endpoints(&gw, &http));
    let mut conn = gw.accept().await;
    conn.hello_identify(1000).await;
    conn.send(ready_with_resume(&gw, "ready_user_classic")).await;
    // Un heartbeat parti avant que READY soit lu porte `null` : on l'accuse
    // et on attend le suivant.
    let mut seen = Vec::new();
    loop {
        let hb = conn.recv().await;
        assert_eq!(hb["op"], 1, "seuls des heartbeats sont attendus : {hb}");
        seen.push(hb["d"].clone());
        if hb["d"] == 1 {
            break;
        }
        assert!(hb["d"].is_null() && seen.len() < 3, "heartbeats inattendus : {seen:?}");
        conn.heartbeat_ack().await;
    }
}

#[tokio::test]
async fn unacked_heartbeat_reconnects_and_resumes() {
    let mut s = ready_then_seq_2().await;
    // Nouvelle session avec un heartbeat rapide, jamais accusé.
    s.conn.reconnect().await;
    let mut conn = s.gw.accept().await;
    conn.auto_ack = false;
    conn.hello(1000).await;
    assert_eq!(conn.recv_op(6).await["d"]["seq"], 2);
    let hb = conn.recv_op(1).await;
    assert_eq!(hb["d"], 2);
    // Pas d'accusé : le client abandonne cette connexion et reprend ailleurs.
    conn.expect_closed().await;
    expect_resume(&mut s.gw, 2).await;
}

#[tokio::test]
async fn abrupt_disconnect_reconnects_and_resumes() {
    let s = ready_then_seq_2().await;
    let Setup { mut gw, conn, client: _client, .. } = s;
    conn.drop_abruptly();
    expect_resume(&mut gw, 2).await;
}

// ------------------------------------------------------------ attente entre tentatives

/// Délai de base des tests (`support::endpoints`).
const BASE: Duration = Duration::from_millis(10);

/// Joue `rounds` connexions qui n'atteignent jamais READY, chacune terminée
/// par `end`, et renvoie l'écart entre la fin de chacune et la connexion
/// suivante. `end` rend l'heure prise juste AVANT l'action qui met fin à la
/// session : le client ne peut commencer à attendre qu'après, donc l'écart
/// mesuré est toujours au moins son attente, même sur une machine chargée.
async fn gaps_between_failed_sessions<F, Fut>(rounds: usize, mut end: F) -> Vec<Duration>
where
    F: FnMut(GwConn) -> Fut,
    Fut: std::future::Future<Output = Instant>,
{
    let mut gw = FakeGateway::start().await;
    let http = FakeHttp::start().await;
    let _client = Client::start(USER_TOKEN, endpoints(&gw, &http));
    let mut gaps = Vec::new();
    let mut ended_at: Option<Instant> = None;
    for _ in 0..=rounds {
        let conn = gw.accept().await;
        if let Some(t) = ended_at {
            gaps.push(t.elapsed());
        }
        ended_at = Some(end(conn).await);
    }
    gaps
}

/// La k-ième nouvelle tentative (k = 1, 2, …) attend au moins `BASE × 2^k`.
fn assert_growing(gaps: &[Duration]) {
    for (i, g) in gaps.iter().enumerate() {
        let min = BASE * (1u32 << (i + 1));
        assert!(*g >= min, "tentative {} trop rapide : {g:?} < {min:?} (écarts : {gaps:?})", i + 1);
    }
}

#[tokio::test]
async fn repeated_close_after_hello_backs_off() {
    let gaps = gaps_between_failed_sessions(4, |mut c| async move {
        c.hello_identify(QUIET).await;
        let at = Instant::now();
        c.close(4000).await;
        c.expect_closed().await;
        at
    })
    .await;
    assert_growing(&gaps);
}

#[tokio::test]
async fn repeated_invalid_session_backs_off() {
    let gaps = gaps_between_failed_sessions(4, |mut c| async move {
        c.hello_identify(QUIET).await;
        let at = Instant::now();
        c.invalid_session(false).await;
        c.expect_closed().await;
        at
    })
    .await;
    assert_growing(&gaps);
}

#[tokio::test]
async fn repeated_reconnect_request_before_ready_backs_off() {
    let gaps = gaps_between_failed_sessions(4, |mut c| async move {
        c.hello_identify(QUIET).await;
        let at = Instant::now();
        c.reconnect().await;
        c.expect_closed().await;
        at
    })
    .await;
    assert_growing(&gaps);
}

#[tokio::test]
async fn short_heartbeat_interval_is_a_session_error() {
    // Moins de 1000 ms (ici 0, puis 999) : anomalie de protocole. Le client
    // n'envoie pas son jeton et attend de plus en plus longtemps.
    let mut interval = [0u64, 999, 999].into_iter();
    let gaps = gaps_between_failed_sessions(2, |mut c| {
        let iv = interval.next().unwrap_or(999);
        async move {
            let at = Instant::now();
            c.hello(iv).await;
            c.expect_closed_silently().await;
            at
        }
    })
    .await;
    assert_growing(&gaps);
}

#[tokio::test]
async fn foreign_resume_url_is_ignored_under_strict_rule() {
    let mut gw = FakeGateway::start().await;
    let http = FakeHttp::start().await;
    let ep = ferrocord::net::Endpoints { resume_hosts: ferrocord::net::ResumeHosts::Discord, ..endpoints(&gw, &http) };
    let mut client = Client::start(USER_TOKEN, ep);
    let mut conn = gw.accept().await;
    conn.hello_identify(QUIET).await;
    // Adresse de reprise hors de discord.gg (locale, pour ne rien contacter d'autre).
    conn.send(ready_with_resume(&gw, "ready_user_classic")).await;
    client.until("READY", |e| matches!(e, Event::Ready { .. })).await;
    conn.reconnect().await;
    let mut again = gw.accept().await;
    assert_eq!(again.path, "/gw/?v=10&encoding=json&compress=zlib-stream", "la reprise doit passer par la passerelle par défaut");
    let d = again.hello_resume(QUIET).await;
    assert_eq!((d["session_id"].as_str(), d["seq"].as_u64()), (Some("session-fictive-0001"), Some(1)));
}
