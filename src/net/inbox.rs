//! Mentions récentes de la boîte de réception (lot navigation). Points
//! d'API réservés aux comptes utilisateur, non documentés par Discord ;
//! forme vérifiée sur docs.discord.food, page « User » :
//!
//! - « List Recent Mentions » : `GET /users/@me/mentions`, paramètres
//!   `before` (snowflake), `limit` (1 à 100, 25 par défaut), `guild_id`,
//!   `roles` et `everyone` (booléens, vrais par défaut) ; réponse : une liste
//!   d'objets message (mentions des 7 derniers jours) ;
//! - « Delete Recent Mention » : `DELETE /users/@me/mentions/{message.id}`,
//!   réponse 204.
//!
//! Chaque requête suit un geste de l'utilisateur et passe par `api` (limiteur
//! commun) ; aucun en-tête propre au client officiel. Un compte bot n'appelle
//! rien (la liste reste vide, avec un message).

use super::people;
use super::ratelimit::Route;
use super::raw::RawMessage;
use super::rest::{api, ApiError, Ctx};
use crate::state::inbox::{Filter, InboxCmd, InboxEvent, PAGE};
use crate::state::model::Message;
use crate::state::{Event, Problem};
use reqwest::Method;
use std::{sync::Arc, time::Duration};

/// Attente maximale d'une demande avant de partir : au-delà, l'échec est annoncé.
const MAX_WAIT: Duration = Duration::from_secs(10);

/// Chemin et paramètres d'une page.
fn list_path(before: Option<u64>, f: Filter) -> String {
    let mut p = format!("/users/@me/mentions?limit={PAGE}&roles={}&everyone={}", f.roles, f.everyone);
    if let Some(g) = f.guild {
        p.push_str(&format!("&guild_id={g}"));
    }
    if let Some(b) = before {
        p.push_str(&format!("&before={b}"));
    }
    p
}

/// Page reçue, dans l'ordre de Discord ; `None` si elle est illisible.
pub(super) fn decode(body: &str) -> Option<Vec<RawMessage>> {
    serde_json::from_str(body).ok()
}

/// Page de mentions décodée (aperçu hors ligne, tests).
pub fn decode_messages(body: &str) -> Option<Vec<Message>> {
    Some(decode(body)?.into_iter().map(RawMessage::into_message).collect())
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: InboxCmd) {
    match cmd {
        InboxCmd::Load { round, before, filter } => {
            if ctx.is_bot() {
                ctx.out.send(Event::Inbox(InboxEvent::LoadFailed { round, problem: Problem::Forbidden }));
                return;
            }
            let url = format!("{}{}", ctx.api_root, list_path(before, filter));
            let route = Route { template: "GET /users/@me/mentions", major: 0 };
            let res = api(&ctx, route, Method::GET, &url, None, MAX_WAIT, std::future::pending()).await;
            let ev = match res.map(|b| decode(&b)) {
                Ok(Some(raw)) => {
                    people::messages(&raw, &ctx.out.people);
                    InboxEvent::Page { round, before, messages: raw.into_iter().map(RawMessage::into_message).collect() }
                }
                Ok(None) => InboxEvent::LoadFailed { round, problem: Problem::Api(String::new()) },
                Err(e) => match e.into_event(|problem| Event::Inbox(InboxEvent::LoadFailed { round, problem })) {
                    Event::Inbox(ev) => ev,
                    other => return ctx.out.send(other),
                },
            };
            ctx.out.send(Event::Inbox(ev));
        }
        InboxCmd::Delete { message_id } => {
            let url = format!("{}/users/@me/mentions/{message_id}", ctx.api_root);
            let route = Route { template: "DELETE /users/@me/mentions/{}", major: 0 };
            let ev = match api(&ctx, route, Method::DELETE, &url, None, MAX_WAIT, std::future::pending()).await {
                // Déjà retirée ailleurs : la liste suit.
                Ok(_) | Err(ApiError::NotFound(_)) => Event::Inbox(InboxEvent::Deleted { message_id }),
                Err(e) => e.into_event(|problem| Event::Inbox(InboxEvent::DeleteFailed { message_id, problem })),
            };
            ctx.out.send(ev);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_carries_the_documented_parameters() {
        assert_eq!(list_path(None, Filter::default()), "/users/@me/mentions?limit=25&roles=true&everyone=true");
        let f = Filter { roles: false, everyone: false, guild: Some(9) };
        assert_eq!(list_path(Some(77), f), "/users/@me/mentions?limit=25&roles=false&everyone=false&guild_id=9&before=77");
    }

    #[test]
    fn fixture_page_decodes_and_unknown_fields_are_ignored() {
        let body = include_str!("../../tests/fixtures/rest_recent_mentions.json");
        let msgs = decode_messages(body).unwrap_or_default();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].channel_id, 3000000000000000012);
        assert!(msgs[0].content.contains("<@1000000000000000001>"));
        assert_eq!(msgs[1].guild_id, None, "message privé");
        assert!(decode_messages("{\"pas\": \"une liste\"}").is_none());
    }
}
