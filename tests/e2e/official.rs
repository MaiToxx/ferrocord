//! MP du compte système de Discord (lot « messages ») : lecture seule.
//! Face au faux serveur, aucune requête d'écriture ne part vers ce salon.

use crate::support::{ids::*, *};
use ferrocord::state::calls::CallCmd;
use ferrocord::state::social::{Emoji, SocialCmd};
use ferrocord::state::store::SendBlock;
use ferrocord::state::upload::{OutFile, Source};
use ferrocord::state::{Cmd, Event};
use serde_json::json;
use std::sync::Arc;

/// MP inventé avec le compte système.
const OFFICIAL_DM: u64 = 4000000000000000090;
const OFFICIAL_MSG: u64 = 5000000000000000901;

#[tokio::test]
async fn official_dm_receives_no_write() {
    let mut s = connected_with(USER_TOKEN, "ready_user_classic", |p| {
        let dm = json!({
            "id": OFFICIAL_DM.to_string(), "type": 1, "last_message_id": OFFICIAL_MSG.to_string(), "flags": 0,
            "recipients": [{ "id": "1000000000000000777", "username": "officiel_fictif", "global_name": "Officiel", "discriminator": "0", "avatar": null, "system": true }]
        });
        if let Some(list) = p["d"]["private_channels"].as_array_mut() {
            list.push(dm);
        }
    })
    .await;
    assert!(s.client.store.is_official_dm(OFFICIAL_DM));
    assert_eq!(s.client.store.send_block(OFFICIAL_DM), Some(SendBlock::Official));

    // La lecture reste permise : l'historique se charge.
    s.http.on("GET", &messages_path(OFFICIAL_DM), [Response::ok(json!([]))]);
    s.client.store.select_channel(OFFICIAL_DM);
    s.client.flush();
    assert_eq!(s.http.next_request().await.method, "GET");
    s.client.until("historique", |e| matches!(e, Event::History { .. })).await;

    // Les gestes de l'interface : refusés à la source.
    let thumbs = || Emoji::Unicode("👍".into());
    let file = OutFile { source: Source::Memory(Arc::from(&b"abc"[..])), name: "note.txt".into(), size: 3, description: String::new(), spoiler: false };
    assert!(!s.client.store.send_message(OFFICIAL_DM, "bonjour".into()));
    assert!(!s.client.store.toggle_reaction(OFFICIAL_DM, OFFICIAL_MSG, thumbs()));
    assert!(s.client.store.send_files(OFFICIAL_DM, String::new(), vec![file], None).is_err());
    s.client.store.start_call(OFFICIAL_DM);
    // Des commandes posées par un autre chemin : retirées à la sortie du magasin.
    s.client.store.push_command(Cmd::Send { channel_id: OFFICIAL_DM, content: "x".into(), nonce: 7 });
    s.client.store.push_command(Cmd::Typing { channel_id: OFFICIAL_DM });
    s.client.store.push_command(Cmd::Social(SocialCmd::React { channel_id: OFFICIAL_DM, message_id: OFFICIAL_MSG, emoji: thumbs(), add: true }));
    s.client.store.push_command(Cmd::Social(SocialCmd::Pin { channel_id: OFFICIAL_DM, message_id: OFFICIAL_MSG, pin: true }));
    s.client.store.push_command(Cmd::Calls(CallCmd::Ring { channel_id: OFFICIAL_DM, recipients: None }));
    s.client.flush();

    // Témoin : une requête ordinaire part après elles ; rien d'autre n'est arrivé entre-temps.
    s.http.on("GET", &messages_path(GENERAL), [Response::ok(fixture("rest_history"))]);
    s.client.store.select_channel(GENERAL);
    s.client.flush();
    let next = s.http.next_request().await;
    assert!(next.method == "GET" && next.path.starts_with(&messages_path(GENERAL)), "{} {}", next.method, next.path);
    s.client.until("historique", |e| matches!(e, Event::History { .. })).await;
    let official: Vec<String> = s.http.requests().iter().filter(|r| r.path.contains(&OFFICIAL_DM.to_string())).map(|r| format!("{} {}", r.method, r.path)).collect();
    assert_eq!(official.len(), 1, "seule la lecture de l'historique : {official:?}");
    assert!(official[0].starts_with("GET "), "{official:?}");
}
