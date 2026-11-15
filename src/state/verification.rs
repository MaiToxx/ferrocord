//! Vérification des membres et demandes d'adhésion (lot ND).
//!
//! Source : docs.discord.food, resources/guild : « Member Verification
//! Object », « Member Verification Form Field Structure », « Get Guild
//! Member Verification », « Get Current User Guild Join Request »,
//! « Create Guild Join Request », « List Guild Join Requests », « Action
//! Guild Join Request », « Guild Join Request Object ».
//!
//! Tout part d'un clic : « Règles et vérification » dans le menu du
//! serveur lit le formulaire et l'état de la demande du compte ;
//! « Envoyer » dépose les réponses saisies par l'utilisateur, une fois ;
//! un modérateur lit les demandes en attente et en accepte ou refuse une
//! par clic. Aucun envoi automatique, aucune action groupée.

use super::event::Cmd;
use super::quests::Refused;
use super::usersettings::Remote;
use super::{Problem, Store};

/// Longueurs maximales (source) : réponse courte, paragraphe, motif de refus.
pub const TEXT_MAX: usize = 150;
pub const PARAGRAPH_MAX: usize = 1000;
pub const REASON_MAX: usize = 160;
/// Demandes lues par liste.
pub const REQUESTS_PAGE: usize = 25;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Terms,
    Text,
    Paragraph,
    Choice,
    /// Type inconnu de Ferrocord : affiché, sans réponse possible ici.
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub kind: FieldKind,
    pub label: String,
    pub description: String,
    pub required: bool,
    /// Règles à accepter (`Terms`).
    pub rules: Vec<String>,
    /// Réponses possibles (`Choice`).
    pub choices: Vec<String>,
    pub placeholder: String,
    /// Le champ tel que Discord l'a envoyé (objet JSON) : il est renvoyé
    /// tel quel avec la réponse.
    pub raw: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Form {
    /// Date de dernière modification, renvoyée avec les réponses.
    pub version: Option<String>,
    pub description: String,
    pub fields: Vec<Field>,
}

/// État de la demande du compte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Aucune demande connue.
    Unknown,
    Started,
    Submitted,
    Approved,
    Rejected(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gate {
    /// `None` : le serveur n'a pas de vérification.
    pub form: Option<Form>,
    pub status: Status,
}

/// Réponse saisie pour un champ, dans l'ordre du formulaire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    Agreed(bool),
    Text(String),
    Choice(Option<usize>),
}

impl Answer {
    pub fn empty(kind: FieldKind) -> Answer {
        match kind {
            FieldKind::Terms => Answer::Agreed(false),
            FieldKind::Text | FieldKind::Paragraph | FieldKind::Other => Answer::Text(String::new()),
            FieldKind::Choice => Answer::Choice(None),
        }
    }

    /// Valeur de `response`, ou `None` si rien n'est répondu.
    fn response(&self, field: &Field) -> Option<serde_json::Value> {
        match (self, field.kind) {
            (Answer::Agreed(true), FieldKind::Terms) => Some(true.into()),
            (Answer::Text(s), FieldKind::Text | FieldKind::Paragraph) => {
                let max = if field.kind == FieldKind::Text { TEXT_MAX } else { PARAGRAPH_MAX };
                let s: String = s.trim().chars().take(max).collect();
                (!s.is_empty()).then(|| s.into())
            }
            (Answer::Choice(Some(i)), FieldKind::Choice) if *i < field.choices.len() => Some((*i as u64).into()),
            _ => None,
        }
    }
}

/// Une demande d'adhésion vue par un modérateur.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinRequest {
    pub id: u64,
    pub user_id: u64,
    /// Nom affiché, puis nom d'utilisateur.
    pub name: String,
    pub username: String,
    pub created_ms: Option<i64>,
    /// Question et réponse, en clair.
    pub answers: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerifyCmd {
    Load { guild_id: u64 },
    /// `body` : le corps JSON construit par [`Store::verification_submit`].
    Submit { guild_id: u64, body: String },
    Requests { guild_id: u64 },
    Action { guild_id: u64, request_id: u64, approve: bool, reason: String },
}

#[derive(Debug)]
pub enum VerifyEvent {
    Loaded { guild_id: u64, result: Result<Gate, Problem> },
    Submitted { guild_id: u64, result: Result<Status, Refused> },
    Requests { guild_id: u64, result: Result<Vec<JoinRequest>, Problem> },
    Actioned { guild_id: u64, request_id: u64, result: Result<(), Refused> },
}

#[derive(Default)]
pub struct Verification {
    /// Serveur dont la fenêtre est ouverte.
    pub guild: Option<u64>,
    pub gate: Remote<Gate>,
    pub requests: Remote<Vec<JoinRequest>>,
    /// Une écriture est en cours (envoi des réponses, ou décision sur cette demande).
    pub sending: bool,
    pub deciding: Option<u64>,
    pub refused: Option<Refused>,
}

impl Verification {
    pub fn clear(&mut self) {
        *self = Verification::default();
    }
}

/// Tous les champs obligatoires ont-ils une réponse ?
pub fn complete(form: &Form, answers: &[Answer]) -> bool {
    form.fields.len() == answers.len() && form.fields.iter().zip(answers).all(|(f, a)| !f.required || a.response(f).is_some())
}

/// Corps de « Create Guild Join Request » : tous les champs du formulaire,
/// tels que reçus, chacun avec sa réponse.
fn submit_body(form: &Form, answers: &[Answer]) -> Option<String> {
    let mut fields = Vec::with_capacity(form.fields.len());
    for (f, a) in form.fields.iter().zip(answers) {
        let mut raw: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&f.raw).ok()?;
        match a.response(f) {
            Some(r) => raw.insert("response".into(), r),
            None => raw.remove("response"),
        };
        fields.push(serde_json::Value::Object(raw));
    }
    let mut body = serde_json::Map::new();
    if let Some(v) = &form.version {
        body.insert("version".into(), v.clone().into());
    }
    body.insert("form_fields".into(), fields.into());
    Some(serde_json::Value::Object(body).to_string())
}

impl Store {
    /// « Règles et vérification » : formulaire et état de la demande, lus à
    /// chaque ouverture de la fenêtre.
    pub fn verification_open(&mut self, guild_id: u64) {
        self.verification = Verification { guild: Some(guild_id), gate: Remote::Loading, ..Verification::default() };
        self.push_command(Cmd::Verification(VerifyCmd::Load { guild_id }));
    }

    pub fn verification_close(&mut self) {
        self.verification.clear();
    }

    /// Peut-on encore envoyer des réponses ? (Pas de demande déposée ni acceptée.)
    pub fn verification_can_submit(&self) -> bool {
        matches!(&self.verification.gate, Remote::Ready(Gate { form: Some(_), status: Status::Unknown | Status::Started | Status::Rejected(_) }))
    }

    /// « Envoyer » : une fois, si tous les champs obligatoires sont remplis.
    pub fn verification_submit(&mut self, answers: &[Answer]) -> bool {
        let Some(guild_id) = self.verification.guild else { return false };
        if self.verification.sending || !self.verification_can_submit() {
            return false;
        }
        let Remote::Ready(Gate { form: Some(form), .. }) = &self.verification.gate else { return false };
        if !complete(form, answers) {
            return false;
        }
        let Some(body) = submit_body(form, answers) else { return false };
        self.verification.sending = true;
        self.verification.refused = None;
        self.push_command(Cmd::Verification(VerifyCmd::Submit { guild_id, body }));
        true
    }

    /// Puis-je voir et traiter les demandes d'adhésion ? (KICK_MEMBERS, source.)
    pub fn can_review_requests(&self, guild_id: u64) -> bool {
        self.guild_can(guild_id, super::perms::Permissions::KICK_MEMBERS)
    }

    /// « Voir les demandes » (ou « Actualiser ») : les demandes en attente.
    pub fn join_requests_load(&mut self) -> bool {
        let Some(guild_id) = self.verification.guild else { return false };
        if matches!(self.verification.requests, Remote::Loading) || !self.can_review_requests(guild_id) {
            return false;
        }
        self.verification.requests = Remote::Loading;
        self.push_command(Cmd::Verification(VerifyCmd::Requests { guild_id }));
        true
    }

    /// Accepter ou refuser une demande listée : une décision à la fois.
    pub fn join_request_decide(&mut self, request_id: u64, approve: bool, reason: &str) -> bool {
        let Some(guild_id) = self.verification.guild else { return false };
        let listed = matches!(&self.verification.requests, Remote::Ready(l) if l.iter().any(|r| r.id == request_id));
        if !listed || self.verification.deciding.is_some() || !self.can_review_requests(guild_id) {
            return false;
        }
        self.verification.deciding = Some(request_id);
        self.verification.refused = None;
        let reason: String = if approve { String::new() } else { reason.trim().chars().filter(|c| !c.is_control()).take(REASON_MAX).collect() };
        self.push_command(Cmd::Verification(VerifyCmd::Action { guild_id, request_id, approve, reason }));
        true
    }

    pub(super) fn apply_verification(&mut self, e: VerifyEvent) {
        let v = &mut self.verification;
        match e {
            // Réponse pour une fenêtre fermée ou rouverte ailleurs : ignorée.
            VerifyEvent::Loaded { guild_id, .. } | VerifyEvent::Submitted { guild_id, .. } | VerifyEvent::Requests { guild_id, .. } | VerifyEvent::Actioned { guild_id, .. } if v.guild != Some(guild_id) => {}
            VerifyEvent::Loaded { result, .. } => v.gate = result.map_or_else(Remote::Failed, Remote::Ready),
            VerifyEvent::Submitted { result, .. } => {
                v.sending = false;
                match result {
                    Ok(status) => {
                        if let Remote::Ready(g) = &mut v.gate {
                            // Sans état lisible dans la réponse, la demande est au moins déposée.
                            g.status = if status == Status::Unknown { Status::Submitted } else { status };
                        }
                    }
                    Err(r) => v.refused = Some(r),
                }
            }
            VerifyEvent::Requests { result, .. } => v.requests = result.map_or_else(Remote::Failed, Remote::Ready),
            VerifyEvent::Actioned { request_id, result, .. } => {
                v.deciding = None;
                match result {
                    Ok(()) => {
                        if let Remote::Ready(l) = &mut v.requests {
                            l.retain(|r| r.id != request_id);
                        }
                    }
                    Err(r) => v.refused = Some(r),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Event;

    fn field(kind: FieldKind, required: bool, raw: &str) -> Field {
        Field { kind, label: "Q".into(), description: String::new(), required, rules: vec!["Règle".into()], choices: vec!["A".into(), "B".into()], placeholder: String::new(), raw: raw.into() }
    }

    fn form() -> Form {
        Form {
            version: Some("2026-01-01T00:00:00+00:00".into()),
            description: String::new(),
            fields: vec![
                field(FieldKind::Terms, true, r#"{"field_type":"TERMS","label":"Q","required":true,"values":["Règle"],"automations":null,"inconnu":1}"#),
                field(FieldKind::Text, false, r#"{"field_type":"TEXT_INPUT","label":"Q","required":false}"#),
                field(FieldKind::Choice, true, r#"{"field_type":"MULTIPLE_CHOICE","label":"Q","required":true,"choices":["A","B"]}"#),
            ],
        }
    }

    #[test]
    fn answers_are_sent_once_with_every_field_echoed() {
        let mut s = Store::new();
        assert!(!s.verification_submit(&[]));
        s.verification_open(9);
        assert_eq!(s.take_commands(), vec![Cmd::Verification(VerifyCmd::Load { guild_id: 9 })]);
        s.apply(Event::Verification(VerifyEvent::Loaded { guild_id: 8, result: Err(Problem::Forbidden) }));
        assert_eq!(s.verification.gate, Remote::Loading, "réponse d'un autre serveur ignorée");
        s.apply(Event::Verification(VerifyEvent::Loaded { guild_id: 9, result: Ok(Gate { form: Some(form()), status: Status::Unknown }) }));
        let partial = [Answer::Agreed(true), Answer::Text(String::new()), Answer::Choice(None)];
        assert!(!s.verification_submit(&partial), "champ obligatoire sans réponse");
        assert!(!s.verification_submit(&[Answer::Agreed(true), Answer::Text("x".into()), Answer::Choice(Some(7))]), "choix hors liste");
        let full = [Answer::Agreed(true), Answer::Text("  ".into()), Answer::Choice(Some(1))];
        assert!(s.verification_submit(&full));
        assert!(!s.verification_submit(&full), "un envoi à la fois");
        let cmds = s.take_commands();
        let [Cmd::Verification(VerifyCmd::Submit { guild_id: 9, body })] = cmds.as_slice() else { panic!("envoi attendu : {cmds:?}") };
        let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
        assert_eq!(v["version"], "2026-01-01T00:00:00+00:00");
        assert_eq!(v["form_fields"][0], serde_json::json!({"field_type":"TERMS","label":"Q","required":true,"values":["Règle"],"automations":null,"inconnu":1,"response":true}));
        assert!(v["form_fields"][1].get("response").is_none());
        assert_eq!(v["form_fields"][2]["response"], 1);
        s.apply(Event::Verification(VerifyEvent::Submitted { guild_id: 9, result: Ok(Status::Unknown) }));
        assert!(matches!(&s.verification.gate, Remote::Ready(g) if g.status == Status::Submitted));
        assert!(!s.verification_submit(&full), "demande déposée : rien ne repart");
    }

    #[test]
    fn a_refusal_is_kept_and_nothing_is_retried() {
        let mut s = Store::new();
        s.verification_open(9);
        s.apply(Event::Verification(VerifyEvent::Loaded { guild_id: 9, result: Ok(Gate { form: Some(form()), status: Status::Rejected("non".into()) }) }));
        let full = [Answer::Agreed(true), Answer::Text("oui".into()), Answer::Choice(Some(0))];
        assert!(s.verification_submit(&full));
        s.take_commands();
        s.apply(Event::Verification(VerifyEvent::Submitted { guild_id: 9, result: Err(Refused::Captcha) }));
        assert_eq!((s.verification.sending, s.verification.refused.clone()), (false, Some(Refused::Captcha)));
        assert!(s.take_commands().is_empty());
        // Sans le droit d'expulser, aucune lecture ni décision sur les demandes.
        assert!(!s.join_requests_load() && !s.join_request_decide(1, true, ""));
        assert!(s.take_commands().is_empty());
    }
}
