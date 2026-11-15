//! Emojis et autocollants (lot T4) : autocollants reçus par la passerelle,
//! envoi d'un autocollant (Create Message, `sticker_ids`) face au faux serveur.

use crate::support::{ids::*, *};
use ferrocord::state::emoji::{StickerEvent, StickerFormat};
use ferrocord::state::Event;
use serde_json::json;

const STICKER_MSG: u64 = 1100000000000000001;
const WAVE: u64 = 7000000000000000701;

#[tokio::test]
async fn stickers_received_and_sent() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &messages_path(GENERAL), [Response::ok(fixture("rest_history"))]);
    s.client.store.select_channel(GENERAL);
    s.client.flush();
    assert_eq!(s.http.next_request().await.method, "GET");
    s.client.until("historique", |e| matches!(e, Event::History { .. })).await;

    // Autocollants du serveur, puis un message qui en porte.
    s.conn.dispatch_fixture("guild_stickers_update", 30).await;
    s.client.until("autocollants", |e| matches!(e, Event::Stickers(StickerEvent::Guild { .. }))).await;
    let offered: Vec<u64> = s.client.store.picker_stickers(GENERAL).iter().map(|x| x.id).collect();
    assert_eq!(offered, [WAVE], "seuls les disponibles");
    s.conn.dispatch_fixture("message_create_sticker", 31).await;
    s.client.until("message", |e| matches!(e, Event::MessageCreate(m) if m.id == STICKER_MSG)).await;
    let m = s.client.store.find_message(GENERAL, STICKER_MSG).cloned().unwrap_or_else(|| panic!("message absent"));
    assert_eq!(m.meta.stickers.iter().map(|x| x.format).collect::<Vec<_>>(), [StickerFormat::Apng, StickerFormat::Lottie, StickerFormat::Gif]);

    // Envoi : `sticker_ids`, sans texte, même nonce que les messages.
    s.http.on("POST", &messages_path(GENERAL), [Response::ok(fixture("message_create_simple")["d"].clone())]);
    assert!(s.client.store.send_sticker(GENERAL, WAVE, "Wave".into()));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.method, "POST");
    let b = r.json();
    assert_eq!(b["sticker_ids"], json!([WAVE.to_string()]));
    assert!(b.get("content").is_none(), "{b}");
    assert!(b["nonce"].is_string() && b["enforce_nonce"] == true);
    s.client.until("envoyé", |e| matches!(e, Event::SendConfirmed { .. })).await;
}