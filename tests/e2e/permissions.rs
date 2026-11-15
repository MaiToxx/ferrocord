//! Permissions : salons masqués, lecture seule, historique interdit, changements
//! en direct, et repli prudent quand les informations manquent.

use crate::support::{ids::*, *};
use ferrocord::state::{Effect, Event, Notice, Permissions as P, Problem, SendBlock, Verdict};
use serde_json::{json, Value};

const MEMBRE: u64 = 2000000000000001001;

fn visible(c: &Client, guild: u64) -> Vec<u64> {
    let g = c.store.guilds.iter().find(|g| g.id == guild).unwrap_or_else(|| panic!("serveur {guild} absent"));
    g.channels.iter().map(|c| c.id).filter(|id| !c.store.is_hidden(*id)).collect()
}

fn variant(name: &str, channel: u64) -> Value {
    let mut p = fixture(name);
    p["d"]["channel_id"] = json!(channel.to_string());
    p
}

/// Envoi témoin dans #général : sa requête prouve que tout ce qui la
/// précédait dans la file a déjà été traité (les commandes partent dans l'ordre).
async fn witness_send(s: &mut Setup) {
    s.http.on("POST", &messages_path(GENERAL), [Response::ok(fixture("message_create_simple")["d"].clone())]);
    assert!(s.client.store.send_message(GENERAL, "témoin".into()));
    s.client.flush();
    loop {
        let r = s.http.next_request().await;
        if r.method == "POST" {
            assert_eq!(r.path, messages_path(GENERAL));
            break;
        }
    }
}

fn paths(s: &Setup) -> Vec<String> {
    s.http.requests().iter().map(|r| format!("{} {}", r.method, r.path.split('?').next().unwrap_or_default())).collect()
}

#[tokio::test]
async fn ready_with_roles_hides_inaccessible_channels() {
    // Format classique (membres dans `guilds[].members`) et format des
    // capacités modernes (`properties` + `merged_members`) : même résultat.
    for ready in ["ready_user_roles", "ready_user_merged"] {
        let s = connected(USER_TOKEN, ready).await;
        let c = &s.client;
        // #modération réservé aux modérateurs ; « Équipe » et son salon aussi :
        // la catégorie disparaît avec lui. Le vocal n'est jamais affiché.
        assert_eq!(visible(c, ALPHA), vec![ANNONCES, DISCUSSIONS, GENERAL, HORS_SUJET, ARCHIVES, ANCIEN], "{ready}");
        for id in [MODERATION, EQUIPE, COULISSES] {
            assert!(c.store.is_hidden(id), "{ready} : {id} devrait être masqué");
            assert_eq!(c.store.can(id, P::VIEW_CHANNEL), Verdict::Refuse);
        }
        // #annonces : lecture seule (et donc ni pièces jointes ni @everyone).
        assert_eq!(c.store.can(ANNONCES, P::VIEW_CHANNEL), Verdict::Permis);
        assert_eq!(c.store.can(ANNONCES, P::SEND_MESSAGES), Verdict::Refuse);
        assert_eq!(c.store.can(ANNONCES, P::ATTACH_FILES), Verdict::Refuse);
        assert_eq!(c.store.send_block(ANNONCES), Some(SendBlock::NoPermission));
        assert_eq!(c.store.can(GENERAL, P::SEND_MESSAGES), Verdict::Permis);
        assert_eq!(c.store.can(GENERAL, P::MANAGE_MESSAGES), Verdict::Refuse);
        assert_eq!(c.store.can(ANCIEN, P::READ_MESSAGE_HISTORY), Verdict::Refuse);
        // Mon membre a été lu (rôles et pseudo).
        let alpha = c.store.guilds.iter().find(|g| g.id == ALPHA).and_then(|g| g.me.clone()).unwrap_or_else(|| panic!("{ready} : membre"));
        assert_eq!((alpha.roles, alpha.nick.as_deref(), alpha.timeout_until), (vec![MEMBRE], Some("Moi du serveur"), None));
        // Bêta : j'en suis propriétaire, le refus fait à @everyone ne me concerne pas.
        assert_eq!(visible(c, BETA), vec![ACCUEIL]);
        assert_eq!(c.store.can(ACCUEIL, P::MANAGE_CHANNELS), Verdict::Permis);
        // Gamma : mon membre est absent du paquet. @everyone n'a aucune
        // permission, mais on ne masque rien sur une supposition.
        assert_eq!(visible(c, GAMMA), vec![SALON_GAMMA]);
        assert_eq!(c.store.can(SALON_GAMMA, P::VIEW_CHANNEL), Verdict::Inconnu);
        assert_eq!(c.store.send_block(SALON_GAMMA), None);
        // Les MP ne sont pas concernés.
        assert_eq!(c.store.can(DM_ALICE, P::SEND_MESSAGES), Verdict::Inconnu);
    }
}

#[tokio::test]
async fn no_history_request_without_read_message_history() {
    let mut s = connected(USER_TOKEN, "ready_user_roles").await;
    s.client.store.select_guild(ALPHA);
    s.client.store.select_channel(ANCIEN);
    s.client.flush();
    let cs = &s.client.store.chans[&ANCIEN];
    assert!(cs.history_denied && cs.loaded && !cs.loading && !cs.more);
    // Chargement de l'historique ancien non plus.
    assert!(!s.client.store.load_older(ANCIEN));
    assert!(s.client.store.take_commands().is_empty());

    // Témoin : l'historique de #général est demandé, celui d'#ancien jamais.
    s.http.on("GET", &messages_path(GENERAL), [Response::ok(fixture("rest_history"))]);
    s.client.store.select_channel(GENERAL);
    s.client.flush();
    s.client.until("historique de #général", |e| matches!(e, Event::History { .. })).await;
    witness_send(&mut s).await;
    assert!(!paths(&s).iter().any(|p| p.contains(&ANCIEN.to_string())), "{:?}", paths(&s));

    // Les messages arrivés en direct s'affichent quand même.
    s.client.store.select_channel(ANCIEN);
    s.conn.send(variant("message_create_simple", ANCIEN)).await;
    s.client.until("message dans #ancien", |e| matches!(e, Event::MessageCreate(m) if m.channel_id == ANCIEN)).await;
    assert_eq!(s.client.store.chans[&ANCIEN].msgs.len(), 1);
}

#[tokio::test]
async fn no_send_without_send_messages() {
    let mut s = connected(USER_TOKEN, "ready_user_roles").await;
    assert!(!s.client.store.send_message(ANNONCES, "refusé".into()));
    s.client.flush();
    witness_send(&mut s).await;
    let posts: Vec<String> = paths(&s).into_iter().filter(|p| p.starts_with("POST")).collect();
    assert_eq!(posts, vec![format!("POST {}", messages_path(GENERAL))]);
}

#[tokio::test]
async fn live_changes_update_visibility_and_composer() {
    let mut s = connected(USER_TOKEN, "ready_user_roles").await;
    s.http.on("GET", &messages_path(HORS_SUJET), [Response::ok(json!([]))]);
    s.client.store.select_guild(ALPHA);
    s.client.store.select_channel(HORS_SUJET);
    s.client.flush();
    s.client.until("historique de #hors-sujet", |e| matches!(e, Event::History { .. })).await;

    // Mention dans un salon masqué : ni non-lu ni mention sur le serveur.
    let mut seq = 10;
    let mut next = || {
        seq += 1;
        seq
    };
    let mut p = variant("message_create_mention", MODERATION);
    p["s"] = json!(next());
    s.conn.send(p).await;
    s.client.until("mention dans #modération", |e| matches!(e, Event::MessageCreate(m) if m.channel_id == MODERATION)).await;
    assert_eq!(s.client.store.guild_has_unread(&alpha(&s.client)), (false, false));

    // Je deviens modérateur : #modération et « Équipe » apparaissent, avec leurs marques.
    s.conn.dispatch_fixture("guild_member_update_roles", next()).await;
    s.client.until("GUILD_MEMBER_UPDATE", |e| matches!(e, Event::MemberUpdate { .. })).await;
    assert_eq!(visible(&s.client, ALPHA), vec![ANNONCES, DISCUSSIONS, GENERAL, HORS_SUJET, MODERATION, ARCHIVES, ANCIEN, EQUIPE, COULISSES]);
    assert_eq!(s.client.store.guild_has_unread(&alpha(&s.client)), (true, true));
    assert_eq!(s.client.store.can(GENERAL, P::MANAGE_MESSAGES), Verdict::Permis);

    // Le rôle Modérateur est supprimé : retour à l'état initial.
    s.conn.dispatch_fixture("guild_role_delete", next()).await;
    s.client.until("GUILD_ROLE_DELETE", |e| matches!(e, Event::RoleDelete { .. })).await;
    assert_eq!(visible(&s.client, ALPHA), vec![ANNONCES, DISCUSSIONS, GENERAL, HORS_SUJET, ARCHIVES, ANCIEN]);

    // Un nouveau rôle ne change rien tant que je ne l'ai pas.
    s.conn.dispatch_fixture("guild_role_create", next()).await;
    s.client.until("GUILD_ROLE_CREATE", |e| matches!(e, Event::RoleUpsert { .. })).await;
    assert_eq!(s.client.store.send_block(GENERAL), None);

    // @everyone perd SEND_MESSAGES : la saisie de #général se ferme.
    s.conn.dispatch_fixture("guild_role_update", next()).await;
    s.client.until("GUILD_ROLE_UPDATE", |e| matches!(e, Event::RoleUpsert { .. })).await;
    assert_eq!(s.client.store.send_block(GENERAL), Some(SendBlock::NoPermission));
    assert_eq!(s.client.store.can(GENERAL, P::VIEW_CHANNEL), Verdict::Permis);

    // Dérogation qui me retire #hors-sujet, ouvert : on le quitte, avec une notification.
    s.conn.dispatch_fixture("channel_update_overwrites", next()).await;
    s.client.until("CHANNEL_UPDATE", |e| matches!(e, Event::ChannelUpsert { .. })).await;
    assert!(s.client.store.is_hidden(HORS_SUJET));
    assert_eq!((s.client.store.guild, s.client.store.channel), (Some(ALPHA), None));
    assert_eq!(s.client.effects.last(), Some(&Effect::Toast(Notice::LostAccess)));

    // Exclusion temporaire : lecture seule partout, raison donnée.
    s.conn.dispatch_fixture("guild_member_update_timeout", next()).await;
    s.client.until("exclusion", |e| matches!(e, Event::MemberUpdate { .. })).await;
    let until = chrono::DateTime::parse_from_rfc3339("2099-01-01T00:00:00+00:00").map(|d| d.timestamp_millis()).unwrap_or_default();
    assert_eq!(s.client.store.send_block(ANNONCES), Some(SendBlock::Timeout { until_ms: until }));
    assert_eq!(s.client.store.can(GENERAL, P::READ_MESSAGE_HISTORY), Verdict::Permis);
    assert_eq!(s.client.store.can(GENERAL, P::ADD_REACTIONS), Verdict::Refuse);
    assert_eq!(s.client.store.next_timeout_end(), Some(until));

    // Je deviens propriétaire : tout est visible et permis, exclusion comprise.
    s.conn.dispatch_fixture("guild_update", next()).await;
    s.client.until("GUILD_UPDATE", |e| matches!(e, Event::GuildUpdate { .. })).await;
    assert_eq!(visible(&s.client, ALPHA), vec![ANNONCES, DISCUSSIONS, GENERAL, HORS_SUJET, MODERATION, ARCHIVES, ANCIEN, EQUIPE, COULISSES]);
    assert_eq!(s.client.store.send_block(GENERAL), None);
    assert_eq!(s.client.store.next_timeout_end(), None);
}

#[tokio::test]
async fn unknown_permissions_keep_previous_behaviour() {
    // READY sans mon membre : @everyone d'Alpha n'a que VIEW_CHANNEL, ce qui
    // fermerait toutes les saisies si on le prenait pour argent comptant.
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    for g in [ALPHA, BETA, GAMMA] {
        let all: Vec<u64> = s.client.store.guilds.iter().find(|x| x.id == g).map(|x| x.channels.iter().map(|c| c.id).collect()).unwrap_or_default();
        assert_eq!(visible(&s.client, g), all);
    }
    assert_eq!(s.client.store.can(GENERAL, P::SEND_MESSAGES), Verdict::Inconnu);
    assert_eq!(s.client.store.send_block(GENERAL), None);
    // L'historique est demandé, et un refus de Discord s'affiche comme avant.
    s.http.on("GET", &messages_path(HORS_SUJET), [Response::error(403, "Missing Access")]);
    s.client.store.select_channel(HORS_SUJET);
    s.client.flush();
    s.client.until("erreur d'historique", |e| matches!(e, Event::HistoryError { .. })).await;
    assert_eq!(s.client.store.chans[&HORS_SUJET].error, Some(Problem::Forbidden));
    assert!(!s.client.store.chans[&HORS_SUJET].history_denied);
    // L'envoi part.
    witness_send(&mut s).await;

    // Un GUILD_MEMBER_UPDATE illisible pour moi rend Alpha inconnu à nouveau.
    let mut s = connected(USER_TOKEN, "ready_user_roles").await;
    assert!(s.client.store.is_hidden(MODERATION));
    let mut p = fixture("guild_member_update_roles");
    p["d"]["communication_disabled_until"] = json!("pas une date");
    s.conn.send(p).await;
    s.client.until("membre illisible", |e| matches!(e, Event::MemberUpdate { member: None, .. })).await;
    assert!(!s.client.store.is_hidden(MODERATION));
    assert_eq!(s.client.store.can(ANNONCES, P::SEND_MESSAGES), Verdict::Inconnu);
}

fn alpha(c: &Client) -> ferrocord::state::model::Guild {
    c.store.guilds.iter().find(|g| g.id == ALPHA).cloned().unwrap_or_else(|| panic!("Alpha"))
}

/// Date RFC 3339 à `ms` (ms Unix), comme dans `communication_disabled_until`.
fn rfc3339(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.to_rfc3339()).unwrap_or_default()
}

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[tokio::test]
async fn damaged_ready_connects_and_hides_nothing_wrongly() {
    let alpha_all = [ANNONCES, DISCUSSIONS, GENERAL, HORS_SUJET, MODERATION, ARCHIVES, ANCIEN, EQUIPE, COULISSES];
    // (dégât, salons d'Alpha attendus visibles)
    type Damage = (&'static str, fn(&mut Value));
    let cases: [Damage; 5] = [
        // Rôle que je porte, permissions de type inattendu : Alpha inconnu.
        ("permissions en objet", |p| p["d"]["guilds"][0]["roles"][1]["permissions"] = json!({ "x": 1 })),
        // Rôle null dans la liste : rôles du serveur inconnus.
        ("rôle null", |p| p["d"]["guilds"][0]["roles"].as_array_mut().into_iter().for_each(|a| a.push(Value::Null))),
        // Rôle sans identifiant.
        ("rôle sans id", |p| {
            if let Some(r) = p["d"]["guilds"][0]["roles"][2].as_object_mut() {
                r.remove("id");
            }
        }),
        // Mon membre : rôles d'un type inattendu.
        ("rôles en chaîne", |p| p["d"]["guilds"][0]["members"][0]["roles"] = json!("2000000000000001001")),
        // Liste des membres illisible.
        ("membres en nombre", |p| p["d"]["guilds"][0]["members"] = json!(7)),
    ];
    for (what, edit) in cases {
        let s = connected_with(USER_TOKEN, "ready_user_roles", edit).await;
        assert_eq!(visible(&s.client, ALPHA), alpha_all, "{what}");
        assert_eq!(s.client.store.can(MODERATION, P::VIEW_CHANNEL), Verdict::Inconnu, "{what}");
        assert_eq!(s.client.store.send_block(ANNONCES), None, "{what}");
        // Les autres serveurs ne sont pas touchés.
        assert_eq!(s.client.store.can(ACCUEIL, P::MANAGE_CHANNELS), Verdict::Permis, "{what}");
    }

    // Dérogation abîmée (null, type inconnu) : seul ce salon devient inconnu.
    for bad in [Value::Null, json!({ "id": "2000000000000000001", "type": "rôle", "allow": "0", "deny": "2048" })] {
        let s = connected_with(USER_TOKEN, "ready_user_roles", |p| p["d"]["guilds"][0]["channels"][0]["permission_overwrites"] = json!([bad])).await;
        assert_eq!(s.client.store.can(ANNONCES, P::SEND_MESSAGES), Verdict::Inconnu);
        assert_eq!(s.client.store.send_block(ANNONCES), None);
        assert!(s.client.store.is_hidden(MODERATION));
    }

    // `merged_members` illisible : READY reçu, mon membre inconnu partout.
    let s = connected_with(USER_TOKEN, "ready_user_merged", |p| p["d"]["merged_members"] = json!({ "a": 1 })).await;
    assert_eq!(visible(&s.client, ALPHA), alpha_all);
}

#[tokio::test]
async fn forbidden_send_right_after_timeout_shows_reason() {
    // Notre horloge croit l'exclusion finie depuis une minute ; Discord refuse
    // encore : la réponse fait foi et la raison est donnée.
    let mut s = connected(USER_TOKEN, "ready_user_roles").await;
    let until = now() - 60_000;
    let mut p = fixture("guild_member_update_timeout");
    p["d"]["communication_disabled_until"] = json!(rfc3339(until));
    s.conn.send(p).await;
    s.client.until("exclusion", |e| matches!(e, Event::MemberUpdate { .. })).await;
    assert_eq!(s.client.store.send_block(GENERAL), None);
    s.http.on("POST", &messages_path(GENERAL), [Response::error(403, "Missing Permissions")]);
    assert!(s.client.store.send_message(GENERAL, "x".into()));
    s.client.flush();
    s.client.until("refus d'envoi", |e| matches!(e, Event::SendError { .. })).await;
    let expected = Notice::NotSent(Problem::Timeout { until_ms: until });
    assert_eq!(s.client.effects.last(), Some(&Effect::Toast(expected)));
}

#[tokio::test]
async fn server_date_header_corrects_late_clock() {
    // Exclusion qui finit dans 30 min selon notre horloge, mais Discord a
    // une heure d'avance : la première réponse REST datée la lève.
    let mut s = connected(USER_TOKEN, "ready_user_roles").await;
    let mut p = fixture("guild_member_update_timeout");
    p["d"]["communication_disabled_until"] = json!(rfc3339(now() + 30 * 60_000));
    s.conn.send(p).await;
    s.client.until("exclusion", |e| matches!(e, Event::MemberUpdate { .. })).await;
    assert!(matches!(s.client.store.send_block(GENERAL), Some(SendBlock::Timeout { .. })));
    let ahead = chrono::DateTime::from_timestamp_millis(now() + 3_600_000).map(|d| d.format("%a, %d %b %Y %H:%M:%S GMT").to_string()).unwrap_or_default();
    s.http.on("GET", &messages_path(HORS_SUJET), [Response::ok(json!([])).dated(&ahead)]);
    s.client.store.select_guild(ALPHA);
    s.client.store.select_channel(HORS_SUJET);
    s.client.flush();
    s.client.until("heure de Discord", |e| matches!(e, Event::ServerTime(_))).await;
    assert_eq!(s.client.store.send_block(GENERAL), None);
    assert!(s.client.store.now() > now() + 3_000_000);
}
