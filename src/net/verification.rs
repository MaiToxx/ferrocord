//! Vérification des membres et demandes d'adhésion (lot ND).
//!
//! Source : docs.discord.food, resources/guild. Lecture du formulaire
//! (`GET /guilds/{id}/member-verification`) et de la demande du compte
//! (`GET /guilds/{id}/requests/@me`), dépôt des réponses
//! (`PUT /guilds/{id}/requests/@me`, corps `{ version?, form_fields }`),
//! liste des demandes en attente (`GET /guilds/{id}/requests`, réponse
//! `{ guild_join_requests, total? }`) et décision
//! (`PATCH /guilds/{id}/requests/id/{demande}`, corps
//! `{ action, rejection_reason? }`). Jeux d'essai synthétiques :
//! `tests/fixtures/nd/rest_member_verification.json`,
//! `tests/fixtures/nd/rest_join_requests.json`.

use super::ratelimit::Route;
use super::raw::RawUser;
use super::rest::{api, ApiError, Ctx};
use crate::state::quests::Refused;
use crate::state::verification::{Field, FieldKind, Form, Gate, JoinRequest, Status, VerifyCmd, VerifyEvent, REQUESTS_PAGE};
use crate::state::{Event, Problem};
use reqwest::Method;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

const MAX_WAIT: Duration = Duration::from_secs(10);
const MAX_FIELDS: usize = 10;

fn text(v: &Value, max: usize) -> String {
    v.as_str().map(str::trim).unwrap_or_default().chars().filter(|c| !c.is_control() || *c == '\n').take(max).collect()
}

fn texts(v: &Value, max_items: usize, max: usize) -> Vec<String> {
    v.as_array().into_iter().flatten().map(|s| text(s, max)).filter(|s| !s.is_empty()).take(max_items).collect()
}

fn field(v: &Value) -> Option<Field> {
    let obj = v.as_object()?;
    let kind = match v["field_type"].as_str()? {
        "TERMS" => FieldKind::Terms,
        "TEXT_INPUT" => FieldKind::Text,
        "PARAGRAPH" => FieldKind::Paragraph,
        "MULTIPLE_CHOICE" => FieldKind::Choice,
        _ => FieldKind::Other,
    };
    // Renvoyé tel quel avec la réponse : jamais une réponse venue du serveur.
    let mut raw = obj.clone();
    raw.remove("response");
    Some(Field {
        kind,
        label: text(&v["label"], 300),
        description: text(&v["description"], 300),
        required: v["required"].as_bool().unwrap_or(false),
        rules: texts(&v["values"], 16, 300),
        choices: texts(&v["choices"], 8, 150),
        placeholder: text(&v["placeholder"], 150),
        raw: Value::Object(raw).to_string(),
    })
}

/// « Member Verification Object ». `Some(None)` : pas de vérification ;
/// `None` : forme inattendue.
pub fn decode_member_verification(body: &str) -> Option<Option<Form>> {
    if body.trim().is_empty() {
        return Some(None);
    }
    let v: Value = serde_json::from_str(body).ok()?;
    if v.is_null() {
        return Some(None);
    }
    let list = v.as_object()?.get("form_fields")?.as_array()?;
    if list.len() > MAX_FIELDS {
        return None;
    }
    // Un champ illisible rendrait l'envoi incomplet : la forme entière est refusée.
    let fields: Vec<Field> = list.iter().map(field).collect::<Option<_>>()?;
    let version = v["version"].as_str().filter(|s| s.len() <= 64).map(str::to_owned);
    Some((!fields.is_empty()).then(|| Form { version, description: text(&v["description"], 300), fields }))
}

fn status(v: &Value) -> Status {
    match v["application_status"].as_str() {
        Some("STARTED") => Status::Started,
        Some("SUBMITTED") => Status::Submitted,
        Some("APPROVED") => Status::Approved,
        Some("REJECTED") => Status::Rejected(text(&v["rejection_reason"], 160)),
        _ => Status::Unknown,
    }
}

/// État d'une demande (« Guild Join Request Object », `application_status`).
pub fn decode_join_status(body: &str) -> Status {
    serde_json::from_str::<Value>(body).map(|v| status(&v)).unwrap_or(Status::Unknown)
}

fn id(v: &Value) -> Option<u64> {
    v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_u64())
}

fn answer(f: &Value) -> Option<(String, String)> {
    let label = text(&f["label"], 300);
    let reply = match &f["response"] {
        Value::String(s) => s.trim().chars().filter(|c| !c.is_control() || *c == '\n').take(1000).collect(),
        // Choix : la source montre les réponses possibles sous `choices`, son exemple sous `values`.
        Value::Number(n) => {
            let i = usize::try_from(n.as_u64()?).ok()?;
            let options = if f["choices"].is_array() { &f["choices"] } else { &f["values"] };
            text(options.get(i)?, 150)
        }
        // Règles acceptées : rien à lire.
        _ => return None,
    };
    (!label.is_empty() && !reply.is_empty()).then_some((label, reply))
}

/// Réponse de « List Guild Join Requests ». `None` : forme inattendue.
pub fn decode_join_requests(body: &str) -> Option<Vec<JoinRequest>> {
    let v: Value = serde_json::from_str(body).ok()?;
    let list = v.as_object()?.get("guild_join_requests")?.as_array()?;
    let mut out: Vec<JoinRequest> = Vec::new();
    for r in list {
        let (Some(rid), Some(user_id)) = (id(&r["id"]).or_else(|| id(&r["join_request_id"])), id(&r["user_id"])) else { continue };
        if status(r) != Status::Submitted || out.len() >= REQUESTS_PAGE || out.iter().any(|x| x.id == rid) {
            continue;
        }
        let user: Option<RawUser> = serde_json::from_value(r["user"].clone()).ok();
        let username: String = user.as_ref().map(|u| u.username().chars().filter(|c| !c.is_control()).take(64).collect()).unwrap_or_default();
        let name = user.as_ref().and_then(|u| u.global_name()).map(|n| n.chars().filter(|c| !c.is_control()).take(64).collect()).unwrap_or_else(|| username.clone());
        out.push(JoinRequest {
            id: rid,
            user_id,
            name,
            username,
            created_ms: super::quests::iso_ms(r["created_at"].as_str()),
            answers: r["form_responses"].as_array().into_iter().flatten().filter_map(answer).take(MAX_FIELDS).collect(),
        });
    }
    Some(out)
}

fn fatal(e: ApiError, or: impl FnOnce(Refused) -> Event) -> Event {
    match super::quests::refused(e) {
        Ok(r) => or(r),
        Err(fatal) => *fatal,
    }
}

fn problem(r: Refused) -> Problem {
    match r {
        // Captcha sur une lecture : on s'arrête là, comme pour un refus.
        Refused::Captcha => Problem::Forbidden,
        Refused::Other(p) => p,
    }
}

async fn load(ctx: &Ctx, guild_id: u64) -> Event {
    let done = |result| Event::Verification(VerifyEvent::Loaded { guild_id, result });
    let url = format!("{}/guilds/{guild_id}/member-verification", ctx.api_root);
    let form = match api(ctx, Route { template: "GET /guilds/{}/member-verification", major: guild_id }, Method::GET, &url, None, MAX_WAIT, std::future::pending()).await {
        Ok(body) => match decode_member_verification(&body) {
            Some(f) => f,
            None => return done(Err(Problem::Api(String::new()))),
        },
        // Pas de vérification sur ce serveur.
        Err(ApiError::NotFound(_)) => None,
        Err(e) => return fatal(e, |r| done(Err(problem(r)))),
    };
    let mut status = Status::Unknown;
    if form.is_some() {
        let url = format!("{}/guilds/{guild_id}/requests/@me", ctx.api_root);
        match api(ctx, Route { template: "GET /guilds/{}/requests/@me", major: guild_id }, Method::GET, &url, None, MAX_WAIT, std::future::pending()).await {
            Ok(body) => status = decode_join_status(&body),
            Err(ApiError::Unauthorized) => return fatal(ApiError::Unauthorized, |r| done(Err(problem(r)))),
            // Aucune demande, ou état illisible : inconnu.
            Err(_) => {}
        }
    }
    done(Ok(Gate { form, status }))
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: VerifyCmd) {
    let bot = ctx.is_bot();
    let root = ctx.api_root.as_str();
    let ev = match cmd {
        VerifyCmd::Load { guild_id } if bot => Event::Verification(VerifyEvent::Loaded { guild_id, result: Err(Problem::Forbidden) }),
        VerifyCmd::Load { guild_id } => load(&ctx, guild_id).await,
        VerifyCmd::Submit { guild_id, body } => {
            let done = |result| Event::Verification(VerifyEvent::Submitted { guild_id, result });
            match serde_json::from_str::<Value>(&body) {
                _ if bot => done(Err(Refused::Other(Problem::Forbidden))),
                Err(_) => done(Err(Refused::Other(Problem::Api(String::new())))),
                Ok(body) => {
                    let url = format!("{root}/guilds/{guild_id}/requests/@me");
                    match api(&ctx, Route { template: "PUT /guilds/{}/requests/@me", major: guild_id }, Method::PUT, &url, Some(body), MAX_WAIT, std::future::pending()).await {
                        Ok(b) => done(Ok(decode_join_status(&b))),
                        Err(e) => fatal(e, |r| done(Err(r))),
                    }
                }
            }
        }
        VerifyCmd::Requests { guild_id } => {
            let done = |result| Event::Verification(VerifyEvent::Requests { guild_id, result });
            if bot {
                done(Err(Problem::Forbidden))
            } else {
                let url = format!("{root}/guilds/{guild_id}/requests?status=SUBMITTED&limit={REQUESTS_PAGE}");
                match api(&ctx, Route { template: "GET /guilds/{}/requests", major: guild_id }, Method::GET, &url, None, MAX_WAIT, std::future::pending()).await.map(|b| decode_join_requests(&b)) {
                    Ok(Some(list)) => done(Ok(list)),
                    Ok(None) => done(Err(Problem::Api(String::new()))),
                    Err(e) => fatal(e, |r| done(Err(problem(r)))),
                }
            }
        }
        VerifyCmd::Action { guild_id, request_id, approve, reason } => {
            let done = |result| Event::Verification(VerifyEvent::Actioned { guild_id, request_id, result });
            if bot {
                done(Err(Refused::Other(Problem::Forbidden)))
            } else {
                let url = format!("{root}/guilds/{guild_id}/requests/id/{request_id}");
                match api(&ctx, Route { template: "PATCH /guilds/{}/requests/id/{}", major: guild_id }, Method::PATCH, &url, Some(action_body(approve, &reason)), MAX_WAIT, std::future::pending()).await {
                    Ok(_) => done(Ok(())),
                    Err(e) => fatal(e, |r| done(Err(r))),
                }
            }
        }
    };
    ctx.out.send(ev);
}

fn action_body(approve: bool, reason: &str) -> Value {
    match (approve, reason.is_empty()) {
        (true, _) => json!({ "action": "APPROVED" }),
        (false, true) => json!({ "action": "REJECTED" }),
        (false, false) => json!({ "action": "REJECTED", "rejection_reason": reason }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_fixture_keeps_every_field_and_drops_server_responses() {
        let form = decode_member_verification(include_str!("../../tests/fixtures/nd/rest_member_verification.json")).flatten().unwrap_or_else(|| panic!("formulaire attendu"));
        assert_eq!(form.version.as_deref(), Some("2026-03-02T10:00:00.000000+00:00"));
        assert_eq!(form.fields.iter().map(|f| (f.kind, f.required)).collect::<Vec<_>>(), vec![(FieldKind::Terms, true), (FieldKind::Text, false), (FieldKind::Paragraph, false), (FieldKind::Choice, true)]);
        assert_eq!(form.fields[0].rules, vec!["Rester courtois.", "Pas de publicité."]);
        assert_eq!(form.fields[3].choices, vec!["Un ami", "Une recherche"]);
        let raw: Value = serde_json::from_str(&form.fields[0].raw).unwrap_or_default();
        assert!(raw.get("response").is_none() && raw["automations"].is_null() && raw["field_type"] == "TERMS");
        assert_eq!(decode_member_verification(""), Some(None));
        assert_eq!(decode_member_verification("null"), Some(None));
        assert_eq!(decode_member_verification(r#"{"form_fields":[]}"#), Some(None));
        assert_eq!(decode_member_verification(r#"{"form_fields":[3]}"#), None);
        assert_eq!(decode_member_verification("[]"), None);
        assert_eq!(decode_member_verification(r#"{"form_fields":[{"field_type":"VERIFICATION","label":"x","required":true}]}"#).flatten().map(|f| f.fields[0].kind), Some(FieldKind::Other));
    }

    #[test]
    fn requests_fixture_lists_submitted_ones_with_readable_answers() {
        let list = decode_join_requests(include_str!("../../tests/fixtures/nd/rest_join_requests.json")).unwrap_or_else(|| panic!("liste attendue"));
        assert_eq!(list.iter().map(|r| (r.id, r.user_id, r.name.as_str(), r.username.as_str())).collect::<Vec<_>>(), vec![(1000000000000000951, 1000000000000000701, "Camille Fictive", "camille_fictive")]);
        assert_eq!(list[0].answers, vec![("Pourquoi nous rejoindre ?".to_owned(), "Pour parler de plantes imaginaires.".to_owned()), ("Comment nous as-tu trouvés ?".to_owned(), "Une recherche".to_owned())]);
        assert!(list[0].created_ms.is_some());
        assert_eq!(decode_join_requests("[]"), None);
        assert_eq!(decode_join_status(r#"{"application_status":"REJECTED","rejection_reason":"Incomplet"}"#), Status::Rejected("Incomplet".into()));
        assert_eq!(decode_join_status("x"), Status::Unknown);
        assert_eq!(action_body(true, "x"), json!({ "action": "APPROVED" }));
        assert_eq!(action_body(false, "Incomplet"), json!({ "action": "REJECTED", "rejection_reason": "Incomplet" }));
    }
}
