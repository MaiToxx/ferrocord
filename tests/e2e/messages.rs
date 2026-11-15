//! Messages en direct : réception, modification, suppression, non-lus, mentions.

use crate::support::{ids::*, *};
use ferrocord::state::Event;
use serde_json::{json, Value};

/// Ouvre #général et attend son historique (jeu `rest_history`, 3 messages).
async fn open_general(s: &mut Setup) {
    s.http.on("GET", &messages_path(GENERAL), [Response::ok(fixture("rest_history"))]);
    s.client.store.select_channel(GENERAL);
    s.client.flush();
    s.client.until("historique de #général", |e| matches!(e, Event::History { .. })).await;
}

fn msg_ids(s: &Setup, ch: u64) -> Vec<u64> {
    s.client.store.chans[&ch].msgs.iter().map(|m| m.id).collect()
}

fn last(s: &Setup, ch: u64) -> &ferrocord::state::model::Message {
    s.client.store.chans[&ch].msgs.back().unwrap_or_else(|| panic!("salon {ch} vide"))
}

/// Jeu de données avec le salon et/ou l'auteur remplacés.
fn variant(name: &str, channel: u64, author: Option<u64>) -> Value {
    let mut p = fixture(name);
    p["d"]["channel_id"] = json!(channel.to_string());
    if let Some(a) = author {
        p["d"]["author"]["id"] = json!(a.to_string());
    }
    p
}

#[tokio::test]
async fn live_message_create_update_delete() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    open_general(&mut s).await;
    let cs = &s.client.store.chans[&GENERAL];
    // Discord renvoie du plus récent au plus ancien ; le magasin garde l'ordre chronologique.
    assert_eq!(msg_ids(&s, GENERAL), vec![5000000000000000201, 5000000000000000202, 5000000000000000203]);
    // Message REST : ni guild_id ni member, le nom global est affiché.
    let authors: Vec<&str> = cs.msgs.iter().map(|m| m.author.as_str()).collect();
    assert_eq!(authors, vec!["bob_fictif", "Moi Fictif", "Alice"]);
    assert!(cs.msgs[2].edited && !cs.msgs[0].edited);
    assert!(!cs.more && cs.loaded && !cs.loading);

    s.conn.dispatch_fixture("message_create_simple", 10).await;
    s.client.until("MESSAGE_CREATE", |e| matches!(e, Event::MessageCreate(_))).await;
    let m = last(&s, GENERAL);
    assert_eq!((m.id, m.author.as_str(), m.content.as_str()), (5000000000000000300, "Alice", "Bonjour tout le monde"));
    assert_eq!(m.guild_id, Some(ALPHA));
    assert!(!m.edited);
    // Salon affiché : pas de non-lu.
    assert!(s.client.store.unread().is_empty());

    s.conn.dispatch_fixture("message_update_partial", 11).await;
    s.client.until("MESSAGE_UPDATE", |e| matches!(e, Event::MessageUpdate { .. })).await;
    let m = last(&s, GENERAL);
    assert_eq!(m.content, "Bonjour tout le monde (modifié)");
    assert!(m.edited);

    // Mise à jour sans `content` (aperçu de lien ajouté) : le texte ne change
    // pas, le message n'est pas marqué modifié, ses embeds sont remplacés.
    s.conn.dispatch_fixture("message_create_embed", 12).await;
    s.conn.dispatch_fixture("message_update_embeds_only", 13).await;
    s.client.until("embeds mis à jour", |e| matches!(e, Event::MessageMedia { embeds: Some(_), attachments: None, .. })).await;
    let m = last(&s, GENERAL);
    assert_eq!(m.embeds.len(), 1);
    assert_eq!(m.embeds[0].url.as_deref(), Some("https://exemple.invalid/article"));
    s.conn.dispatch_fixture("message_delete", 14).await;
    s.client
        .until("MESSAGE_DELETE", |e| {
            assert!(!matches!(e, Event::MessageUpdate { .. }), "mise à jour sans contenu transmise : {e:?}");
            matches!(e, Event::MessageDelete { .. })
        })
        .await;
    assert_eq!(
        msg_ids(&s, GENERAL),
        vec![5000000000000000201, 5000000000000000202, 5000000000000000203, 5000000000000000302]
    );
    assert!(!last(&s, GENERAL).edited);
}

#[tokio::test]
async fn message_variants_are_converted() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    open_general(&mut s).await;
    for (i, name) in ["message_create_attachment", "message_create_embed", "message_create_nick", "message_create_system"]
        .into_iter()
        .enumerate()
    {
        s.conn.dispatch_fixture(name, 10 + i as u64).await;
    }
    s.conn.send(variant("message_create_mention", GENERAL, None)).await;
    s.client.until("mention", |e| matches!(e, Event::MessageCreate(m) if m.id == 5000000000000000303)).await;

    let msgs = &s.client.store.chans[&GENERAL].msgs;
    let by_id = |id: u64| msgs.iter().find(|m| m.id == id).unwrap_or_else(|| panic!("message {id} absent"));

    let a = by_id(5000000000000000301);
    assert_eq!(a.author, "bob_fictif");
    assert_eq!(a.content, "");
    let files: Vec<&str> = a.attachments.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(files, vec!["photo-fictive.png", "notes.txt"]);
    assert!(a.attachments[0].url.starts_with("https://cdn.discordapp.com/attachments/"));

    let e = by_id(5000000000000000302);
    assert_eq!(e.content, "Regardez https://exemple.invalid/article");
    let titles: Vec<Option<&str>> = e.embeds.iter().map(|e| e.title.as_deref()).collect();
    assert_eq!(titles, vec![Some("Titre fictif"), Some("Seulement un titre"), None]);
    assert_eq!(e.embeds[0].description.as_deref(), Some("Description fictive"));
    assert_eq!(e.embeds[0].provider.as_ref().map(|p| p.name.as_str()), Some("Exemple"));
    assert_eq!(e.embeds[1].fields.len(), 1);
    // L'embed « image » : sa vignette, sans copie du proxy, n'est jamais téléchargée.
    assert!(e.embeds[2].is_bare_media() && e.embeds[2].thumbnail.as_ref().is_some_and(|t| t.proxy_url.is_none()));
    assert_eq!((a.attachments[0].width, a.attachments[0].height), (640, 480));
    assert_eq!(a.attachments[1].kind, ferrocord::state::model::MediaKind::File);

    assert_eq!(by_id(5000000000000000304).author, "Ève du serveur");

    let sys = by_id(5000000000000000305);
    // Phrase d'arrivée choisie selon l'identifiant, avec le nom (lot T13).
    let joined = ferrocord::state::system_msgs::text(ferrocord::i18n::Lang::Fr, ferrocord::state::SystemKind::Joined, sys, Default::default());
    assert_eq!((sys.author.as_str(), sys.content.as_str()), ("Carol", joined.as_str()));
    assert!(sys.content.contains("Carol"));

    // Source gardée telle quelle ; les balises sont mises en mots par la mise en forme.
    let m = by_id(5000000000000000303);
    assert_eq!(m.content, "Salut <@1000000000000000001>, as-tu vu <#3000000000000000011> ?");
    let doc = ferrocord::state::markdown::parse_message(&s.client.store, m, ferrocord::i18n::Lang::Fr, true);
    let shown: String = doc.blocks.iter().flat_map(|b| b.spans.iter().map(|s| s.text.as_str())).collect();
    assert_eq!(shown, "Salut @Moi Fictif, as-tu vu #général ?");
    assert_eq!(m.mentions, vec![(ME, "Moi Fictif".to_string())]);
}

#[tokio::test]
async fn unread_and_mentions() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    open_general(&mut s).await;

    // Mon propre message ailleurs : pas de non-lu.
    s.conn.send(variant("message_create_simple", ANNONCES, Some(ME))).await;
    // Message d'Alice dans #annonces : non-lu sans mention.
    let mut p = variant("message_create_simple", ANNONCES, None);
    p["d"]["id"] = json!("5000000000000000310");
    s.conn.send(p).await;
    s.client.until("message #annonces", |e| matches!(e, Event::MessageCreate(m) if m.id == 5000000000000000310)).await;
    assert!(s.client.store.unread().contains(&ANNONCES));
    assert!(!s.client.store.mentioned().contains(&ANNONCES));

    // Mention de moi dans #hors-sujet.
    s.conn.dispatch_fixture("message_create_mention", 20).await;
    s.client.until("mention", |e| matches!(e, Event::MessageCreate(m) if m.id == 5000000000000000303)).await;
    assert!(s.client.store.unread().contains(&HORS_SUJET));
    assert!(s.client.store.mentioned().contains(&HORS_SUJET));
    let alpha = s.client.store.guilds.iter().find(|g| g.id == ALPHA).cloned().unwrap_or_else(|| panic!("Alpha"));
    assert_eq!(s.client.store.guild_has_unread(&alpha), (true, true));

    // Message privé : compte comme une mention, et le MP remonte en tête.
    assert_eq!(s.client.store.dms[0].id, DM_GROUP);
    s.conn.dispatch_fixture("message_create_dm", 21).await;
    s.client.until("MP", |e| matches!(e, Event::MessageCreate(m) if m.channel_id == DM_ALICE)).await;
    assert!(s.client.store.mentioned().contains(&DM_ALICE));
    assert_eq!((s.client.store.dms[0].id, s.client.store.dms[0].last_message), (DM_ALICE, 5000000000000000306));

    // Ouvrir le salon efface ses marques.
    s.client.store.select_channel(HORS_SUJET);
    assert!(!s.client.store.unread().contains(&HORS_SUJET) && !s.client.store.mentioned().contains(&HORS_SUJET));
    assert_eq!(s.client.store.guild_has_unread(&alpha), (true, false));
    assert!(s.client.store.unread().iter().all(|c| [ANNONCES, DM_ALICE].contains(c)));
}
