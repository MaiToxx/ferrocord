//! Quêtes (lot ND) : liste, acceptation, récompense, code.
//!
//! Points d'API réservés aux comptes utilisateur, non documentés par
//! Discord. Formes vérifiées le 15 novembre 2026 sur docs.discord.food, page
//! « Quests » :
//! - « List Current User Quests » : `GET /quests/@me` ; réponse `{ quests,
//!   excluded_quests, quest_enrollment_blocked_until }` ;
//! - « Accept Quest » : `POST /quests/{quest.id}/enroll`, corps `{ location }`
//!   (type de contenu ; 11 : `QUEST_HOME_DESKTOP`) ; réponse : l'état de
//!   l'utilisateur ;
//! - « Claim Quest Reward » : `POST /quests/{quest.id}/claim-reward`, corps
//!   `{ location, platform }` (0 : `CROSS_PLATFORM`) ; réponse `{ claimed_at,
//!   entitlement_expiration_metadata, entitlements, errors }` ;
//! - « Get Quest Reward Code » : `GET /quests/{quest.id}/reward-code` ;
//!   réponse `{ quest_id, code, platform, user_id, claimed_at }` ;
//! - objets « Quest », « Quest Config » (`starts_at`, `expires_at`, `assets`,
//!   `colors`, `messages`, `task_config_v2 { join_operator, tasks }`,
//!   `rewards_config { rewards, platforms }`), « Quest User Status »
//!   (`enrolled_at`, `completed_at`, `claimed_at`, `progress { TÂCHE: { value
//!   } }`) ;
//! - images (« Quest Assets Structure », « Quest Reward Structure », « Quest
//!   CTA Config », et page « Reference », « CDN Endpoints ») : `assets { hero,
//!   hero_video, quest_bar_hero, quest_bar_hero_video, game_tile,
//!   game_tile_light, game_tile_dark, logotype, logotype_light, logotype_dark
//!   }`, `rewards[].asset` et `asset_video`, `messages.name_with_article`,
//!   `tasks[].messages.task_description`, `cta_config.link`,
//!   `application.link` (déprécié). Les images ne sont pas des requêtes
//!   d'API : elles viennent du CDN, sans jeton, par le chargeur d'images
//!   (`state::quests::asset_urls`), pour les cartes visibles seulement.
//!
//! Page « Gateway Events » : « Quests User Status Update » (`user_status`),
//! « Quests User Completion Update » (`quest_enrollment_blocked_until`),
//! « Virtual Currency Balance Update » (`balance`).
//!
//! **Jamais appelés, et absents du code** : `/quests/{id}/heartbeat`,
//! `/quests/{id}/video-progress`, `/quests/{id}/console/start` : Ferrocord
//! ne déclare aucune progression. Les champs scellés des publicités
//! (`metadata_sealed`, `traffic_metadata_sealed`) ne sont ni lus ni envoyés.
//!
//! Chaque requête suit un clic et passe par `api` (limiteur commun). Un
//! captcha demandé arrête l'action (`Refused::Captcha`). Un compte bot
//! n'appelle rien. Jeux d'essai synthétiques : `tests/fixtures/nd/rest_quests.json`,
//! `rest_quests_assets.json` (les formes connues des valeurs d'images),
//! `rest_quest_status.json`.

use super::ratelimit::Route;
use super::raw::{de_lenient, de_loose_id, Item};
use super::rest::{api, ApiError, Ctx};
use super::Out;
use crate::state::quests::{is_video, Quest, QuestCmd, QuestEvent, Refused, Reward, RewardKind, Status, Task, Themed, TaskKind, MAX_PARTS, MAX_QUESTS, MAX_TEXT};
use crate::state::{Event, Problem};
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::{sync::Arc, time::Duration};

const MAX_WAIT: Duration = Duration::from_secs(10);
/// Type de contenu « page des quêtes, bureau » (`QUEST_HOME_DESKTOP`).
const LOCATION: u8 = 11;
/// Plateforme « toutes » (`CROSS_PLATFORM`).
const PLATFORM: u8 = 0;

#[derive(Deserialize)]
struct RawProgress {
    #[serde(default, deserialize_with = "de_lenient")]
    value: Option<u64>,
}

#[derive(Deserialize)]
struct RawStatus {
    #[serde(default, deserialize_with = "de_loose_id")]
    quest_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    enrolled_at: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    completed_at: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    claimed_at: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    progress: Option<BTreeMap<String, Item<RawProgress>>>,
}

#[derive(Deserialize)]
struct RawTask {
    #[serde(rename = "type", default, deserialize_with = "de_lenient")]
    kind: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    target: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    messages: Option<RawTaskMessages>,
}

#[derive(Deserialize)]
struct RawTaskMessages {
    #[serde(default, deserialize_with = "de_lenient")]
    task_description: Option<String>,
}

/// `cta_config` et `application` : seul leur lien est lu.
#[derive(Deserialize)]
struct RawLink {
    #[serde(default, deserialize_with = "de_lenient")]
    link: Option<String>,
}

#[derive(Deserialize)]
struct RawTasks {
    #[serde(default, deserialize_with = "de_lenient")]
    join_operator: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    tasks: Option<BTreeMap<String, Item<RawTask>>>,
}

#[derive(Deserialize)]
struct RawRewardMessages {
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    name_with_article: Option<String>,
}

#[derive(Deserialize)]
struct RawReward {
    #[serde(rename = "type", default, deserialize_with = "de_lenient")]
    kind: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    asset: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    messages: Option<RawRewardMessages>,
    #[serde(default, deserialize_with = "de_lenient")]
    orb_quantity: Option<u64>,
}

#[derive(Deserialize)]
struct RawRewards {
    #[serde(default, deserialize_with = "de_lenient")]
    rewards: Option<Vec<Item<RawReward>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    platforms: Option<Vec<u64>>,
}

#[derive(Deserialize)]
struct RawAssets {
    #[serde(default, deserialize_with = "de_lenient")]
    hero: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    quest_bar_hero: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    game_tile: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    game_tile_light: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    game_tile_dark: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    logotype: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    logotype_light: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    logotype_dark: Option<String>,
}

#[derive(Deserialize)]
struct RawColors {
    #[serde(default, deserialize_with = "de_lenient")]
    primary: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    secondary: Option<String>,
}

#[derive(Deserialize)]
struct RawMessages {
    #[serde(default, deserialize_with = "de_lenient")]
    quest_name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    game_title: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    game_publisher: Option<String>,
}

#[derive(Deserialize)]
struct RawConfig {
    #[serde(default, deserialize_with = "de_lenient")]
    starts_at: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    expires_at: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    assets: Option<RawAssets>,
    #[serde(default, deserialize_with = "de_lenient")]
    colors: Option<RawColors>,
    #[serde(default, deserialize_with = "de_lenient")]
    messages: Option<RawMessages>,
    #[serde(default, deserialize_with = "de_lenient")]
    task_config_v2: Option<RawTasks>,
    #[serde(default, deserialize_with = "de_lenient")]
    rewards_config: Option<RawRewards>,
    #[serde(default, deserialize_with = "de_lenient")]
    cta_config: Option<RawLink>,
    #[serde(default, deserialize_with = "de_lenient")]
    application: Option<RawLink>,
}

#[derive(Deserialize)]
struct RawQuest {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    config: Option<RawConfig>,
    #[serde(default, deserialize_with = "de_lenient")]
    user_status: Option<RawStatus>,
    /// Quête d'essai réservée aux testeurs de Discord : jamais montrée.
    #[serde(default, deserialize_with = "de_lenient")]
    preview: Option<bool>,
}

#[derive(Deserialize)]
struct RawList {
    #[serde(default, deserialize_with = "de_lenient")]
    quests: Option<Vec<Item<RawQuest>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    quest_enrollment_blocked_until: Option<String>,
}

pub(super) fn iso_ms(s: Option<&str>) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s?).ok().map(|d| d.timestamp_millis())
}

fn text(s: Option<&str>) -> String {
    s.map(str::trim).unwrap_or_default().chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect()
}

/// Couleur `#RRGGBB`.
fn hex(s: Option<&str>) -> Option<u32> {
    let h = s?.strip_prefix('#')?;
    (h.len() == 6).then(|| u32::from_str_radix(h, 16).ok()).flatten()
}

/// Lien du second bouton d'une quête : `https://` seulement, sans espace ni
/// caractère de contrôle, de longueur raisonnable. Il n'est ouvert qu'au clic,
/// par `App::open_link` (confirmation hors des domaines de confiance).
fn https_link(s: Option<&str>) -> Option<String> {
    let s = s?.trim();
    (s.len() > "https://".len() && s.len() <= 512 && s.starts_with("https://") && !s.chars().any(|c| c.is_control() || c.is_whitespace())).then(|| s.to_string())
}

/// Nom de tâche : majuscules, chiffres et `_` seulement (clé de progression).
fn task_key(s: &str) -> Option<String> {
    (!s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')).then(|| s.to_string())
}

impl RawStatus {
    fn status(&self) -> Status {
        let progress = self.progress.iter().flatten().filter_map(|(k, p)| Some((task_key(k)?, p.0.as_ref()?.value?.min(u64::from(u32::MAX)) as u32))).take(MAX_PARTS).collect();
        Status { enrolled_at: iso_ms(self.enrolled_at.as_deref()), completed_at: iso_ms(self.completed_at.as_deref()), claimed_at: iso_ms(self.claimed_at.as_deref()), progress }
    }
}

impl RawQuest {
    fn quest(&self) -> Option<Quest> {
        if self.preview == Some(true) {
            return None;
        }
        let id = self.id?;
        let c = self.config.as_ref()?;
        let m = c.messages.as_ref();
        let name = text(m.and_then(|m| m.quest_name.as_deref()));
        let game = text(m.and_then(|m| m.game_title.as_deref()));
        if name.is_empty() && game.is_empty() {
            return None;
        }
        // Valeur d'image fixe : une vidéo (`hero` et `asset` en sont parfois) n'est pas gardée.
        let asset = |s: Option<&str>| s.map(|s| text(Some(s))).filter(|s| !s.is_empty() && !is_video(s));
        let tc = c.task_config_v2.as_ref();
        let tasks = tc
            .and_then(|t| t.tasks.as_ref())
            .into_iter()
            .flatten()
            .filter_map(|(k, t)| {
                let t = t.0.as_ref()?;
                let key = task_key(t.kind.as_deref().unwrap_or(k))?;
                Some(Task { kind: TaskKind::parse(&key), key, target: t.target.unwrap_or(0).min(u64::from(u32::MAX)) as u32, text: text(t.messages.as_ref().and_then(|m| m.task_description.as_deref())) })
            })
            .take(MAX_PARTS)
            .collect();
        let rc = c.rewards_config.as_ref();
        let rewards = rc
            .and_then(|r| r.rewards.as_ref())
            .into_iter()
            .flatten()
            .filter_map(|r| {
                let r = r.0.as_ref()?;
                Some(Reward {
                    kind: RewardKind::from_code(r.kind?),
                    name: text(r.messages.as_ref().and_then(|m| m.name.as_deref())),
                    article: text(r.messages.as_ref().and_then(|m| m.name_with_article.as_deref())),
                    orbs: r.orb_quantity.map(|n| n.min(u64::from(u32::MAX)) as u32),
                    asset: asset(r.asset.as_deref()),
                })
            })
            .take(MAX_PARTS)
            .collect();
        let a = c.assets.as_ref();
        let themed = |base: Option<&str>, dark: Option<&str>, light: Option<&str>| Themed { base: asset(base), dark: asset(dark), light: asset(light) };
        let colors = c.colors.as_ref().and_then(|c| Some([hex(c.primary.as_deref())?, hex(c.secondary.as_deref())?]));
        Some(Quest {
            id,
            name,
            game,
            publisher: text(m.and_then(|m| m.game_publisher.as_deref())),
            starts_ms: iso_ms(c.starts_at.as_deref()),
            expires_ms: iso_ms(c.expires_at.as_deref()),
            hero: a.and_then(|a| asset(a.hero.as_deref()).or(asset(a.quest_bar_hero.as_deref()))),
            tile: a.map(|a| themed(a.game_tile.as_deref(), a.game_tile_dark.as_deref(), a.game_tile_light.as_deref())).unwrap_or_default(),
            logo: a.map(|a| themed(a.logotype.as_deref(), a.logotype_dark.as_deref(), a.logotype_light.as_deref())).unwrap_or_default(),
            link: https_link(c.cta_config.as_ref().and_then(|l| l.link.as_deref())).or_else(|| https_link(c.application.as_ref().and_then(|l| l.link.as_deref()))),
            colors,
            tasks,
            any_task: tc.and_then(|t| t.join_operator.as_deref()) != Some("and"),
            rewards,
            // Réclamable ici si la récompense vaut pour toutes les plateformes (ou n'en précise aucune).
            claim_here: rc.and_then(|r| r.platforms.as_ref()).is_none_or(|p| p.is_empty() || p.contains(&u64::from(PLATFORM))),
            status: self.user_status.as_ref().map(RawStatus::status),
        })
    }
}

/// Réponse de `GET /quests/@me` : quêtes lisibles, et l'heure (ms Unix) avant
/// laquelle aucune ne peut être acceptée. `None` : réponse illisible.
pub fn decode_quests(body: &str) -> Option<(Vec<Quest>, Option<i64>)> {
    if !body.trim_start().starts_with('{') {
        return None;
    }
    let l: RawList = serde_json::from_str(body).ok()?;
    let quests = l.quests.iter().flatten().filter_map(|q| q.0.as_ref().and_then(RawQuest::quest)).take(MAX_QUESTS).collect();
    Some((quests, iso_ms(l.quest_enrollment_blocked_until.as_deref())))
}

/// État de l'utilisateur dans une quête (réponse de « Accept Quest »,
/// événement de passerelle) et la quête concernée, si elle est dite.
pub fn decode_quest_status(body: &str) -> Option<(Option<u64>, Status)> {
    let s: RawStatus = serde_json::from_str(body).ok()?;
    Some((s.quest_id, s.status()))
}

fn claimed_at(body: &str) -> Option<i64> {
    #[derive(Deserialize)]
    struct Raw {
        #[serde(default, deserialize_with = "de_lenient")]
        claimed_at: Option<String>,
    }
    iso_ms(serde_json::from_str::<Raw>(body).ok()?.claimed_at.as_deref())
}

fn code_of(body: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Raw {
        #[serde(default, deserialize_with = "de_lenient")]
        code: Option<String>,
    }
    let code = text(serde_json::from_str::<Raw>(body).ok()?.code.as_deref());
    (!code.is_empty()).then_some(code)
}

/// Événements de passerelle des quêtes et du solde d'Orbes.
pub(super) fn dispatch(kind: &str, d: &str, out: &Out) {
    #[derive(Deserialize)]
    struct StatusUpdate<'a> {
        #[serde(borrow)]
        user_status: &'a serde_json::value::RawValue,
    }
    #[derive(Deserialize)]
    struct Completion {
        #[serde(default, deserialize_with = "de_lenient")]
        quest_enrollment_blocked_until: Option<String>,
    }
    #[derive(Deserialize)]
    struct Balance {
        #[serde(default, deserialize_with = "de_lenient")]
        balance: Option<u64>,
    }
    let ev = match kind {
        "QUESTS_USER_STATUS_UPDATE" => serde_json::from_str::<StatusUpdate>(d).ok().and_then(|u| decode_quest_status(u.user_status.get())).and_then(|(id, status)| Some(QuestEvent::Status { id: id?, status })),
        "QUESTS_USER_COMPLETION_UPDATE" => serde_json::from_str::<Completion>(d).ok().map(|c| QuestEvent::BlockedUntil(iso_ms(c.quest_enrollment_blocked_until.as_deref()))),
        "VIRTUAL_CURRENCY_BALANCE_UPDATE" => serde_json::from_str::<Balance>(d).ok().and_then(|b| b.balance).map(QuestEvent::Orbs),
        _ => None,
    };
    if let Some(e) = ev {
        out.send(Event::Quests(e));
    }
}

const fn route(template: &'static str) -> Route {
    Route { template, major: 0 }
}

pub(super) fn refused(e: ApiError) -> Result<Refused, Box<Event>> {
    match e {
        ApiError::Captcha => Ok(Refused::Captcha),
        ApiError::Unauthorized => Err(Box::new(Event::Fatal { problem: Problem::BadToken, bad_token: true })),
        ApiError::Forbidden => Ok(Refused::Other(Problem::Forbidden)),
        ApiError::Unknown(p) | ApiError::Other(p) | ApiError::NotFound(p) => Ok(Refused::Other(p)),
        ApiError::Blocked(until) => Ok(Refused::Other(Problem::Blocked { retry_ms: super::wall_clock_ms(until) })),
        ApiError::Cancelled => Ok(Refused::Other(Problem::Api(String::new()))),
    }
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: QuestCmd) {
    let q = Event::Quests;
    let root = ctx.api_root.as_str();
    if ctx.is_bot() {
        let no = || Refused::Other(Problem::Forbidden);
        return ctx.out.send(q(match cmd {
            QuestCmd::List => QuestEvent::List(Err(Problem::Forbidden)),
            QuestCmd::Enroll { id } => QuestEvent::Enrolled { id, result: Err(no()) },
            QuestCmd::Claim { id } => QuestEvent::Claimed { id, result: Err(no()) },
            QuestCmd::Code { id } => QuestEvent::Code { id, result: Err(no()) },
        }));
    }
    let call = |r: Route, m: Method, url: String, body: Option<serde_json::Value>| {
        let ctx = ctx.clone();
        async move { api(&ctx, r, m, &url, body, MAX_WAIT, std::future::pending()).await }
    };
    // Écriture refusée : l'événement de la quête, ou la fin de session si le jeton est mort.
    let write = |res: Result<String, ApiError>, done: &dyn Fn(Result<String, Refused>) -> QuestEvent| match res {
        Ok(b) => q(done(Ok(b))),
        Err(e) => match refused(e) {
            Ok(r) => q(done(Err(r))),
            Err(fatal) => *fatal,
        },
    };
    let unreadable = || Refused::Other(Problem::Api(String::new()));
    let ev = match cmd {
        QuestCmd::List => match call(route("GET /quests/@me"), Method::GET, format!("{root}/quests/@me"), None).await.map(|b| decode_quests(&b)) {
            Ok(Some(list)) => q(QuestEvent::List(Ok(list))),
            Ok(None) => q(QuestEvent::List(Err(Problem::Api(String::new())))),
            Err(e) => e.into_event(|p| q(QuestEvent::List(Err(p)))),
        },
        QuestCmd::Enroll { id } => {
            let res = call(route("POST /quests/{}/enroll"), Method::POST, format!("{root}/quests/{id}/enroll"), Some(json!({ "location": LOCATION }))).await;
            write(res, &|r| QuestEvent::Enrolled { id, result: r.and_then(|b| decode_quest_status(&b).map(|(_, s)| s).ok_or_else(unreadable)) })
        }
        QuestCmd::Claim { id } => {
            let res = call(route("POST /quests/{}/claim-reward"), Method::POST, format!("{root}/quests/{id}/claim-reward"), Some(json!({ "location": LOCATION, "platform": PLATFORM }))).await;
            write(res, &|r| QuestEvent::Claimed { id, result: r.map(|b| claimed_at(&b)) })
        }
        QuestCmd::Code { id } => {
            let res = call(route("GET /quests/{}/reward-code"), Method::GET, format!("{root}/quests/{id}/reward-code"), None).await;
            write(res, &|r| QuestEvent::Code { id, result: r.and_then(|b| code_of(&b).ok_or_else(unreadable)) })
        }
    };
    ctx.out.send(ev);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::quests::Stage;

    #[test]
    fn quests_fixture_decodes_and_bad_entries_are_dropped() {
        let (list, blocked) = decode_quests(include_str!("../../tests/fixtures/nd/rest_quests.json")).unwrap_or_else(|| panic!("liste attendue"));
        assert_eq!(list.iter().map(|q| q.id).collect::<Vec<_>>(), [8_000_000_000_000_000_001, 8_000_000_000_000_000_002, 8_000_000_000_000_000_003], "quête d'essai, sans nom et illisible écartées");
        assert_eq!(blocked, None);
        let q = &list[0];
        assert_eq!((q.name.as_str(), q.game.as_str(), q.publisher.as_str()), ("Chasse aux données fictives", "Jeu Synthétique 3000", "Studio Fictif"));
        assert_eq!(q.colors, Some([0xE944D4, 0x5318A7]));
        assert_eq!((q.hero.as_deref(), q.tile.base.as_deref(), q.logo.base.as_deref()), (Some("hero.jpg"), Some("gametile.jpg"), Some("wordmark.png")));
        assert_eq!(q.link.as_deref(), Some("https://exemple.invalid/jeu"));
        assert_eq!(q.reward().map(|r| r.article.as_str()), Some("un Casque fictif"));
        assert!(q.any_task && q.claim_here && q.status.is_none());
        let kinds: Vec<(TaskKind, u32)> = q.tasks.iter().map(|t| (t.kind, t.target)).collect();
        assert_eq!(kinds, [(TaskKind::PlayOnDesktop, 900), (TaskKind::PlayOnPlaystation, 900), (TaskKind::PlayOnXbox, 900)]);
        assert_eq!(q.reward().map(|r| (r.kind, r.name.as_str(), r.asset.as_deref())), Some((RewardKind::Code, "Casque fictif", Some("recompense.png"))));
        assert_eq!(q.stage(1_800_000_000_000), Stage::Open);
        // Acceptée, à moitié faite : la progression est celle que Discord renvoie.
        let q = &list[1];
        assert_eq!((q.stage(1_800_000_000_000), q.progress()), (Stage::Accepted, Some((15, 30))));
        assert_eq!(q.reward().map(|r| (r.kind, r.orbs)), Some((RewardKind::Orbs, Some(700))));
        assert!(!q.any_task);
        // Terminée, récompense réservée à une console : pas réclamable ici.
        let q = &list[2];
        assert_eq!(q.stage(1_800_000_000_000), Stage::Completed);
        assert!(!q.claim_here);
        assert!(decode_quests("[]").is_none());
        assert_eq!(decode_quests("{}"), Some((Vec::new(), None)));
        assert_eq!(decode_quests(r#"{ "quests": "x", "quest_enrollment_blocked_until": "2030-01-01T00:00:00+00:00" }"#), Some((Vec::new(), Some(1_893_456_000_000))));
    }

    /// Les formes connues des valeurs d'images : chemin complet, adresse
    /// entière, nom de fichier ; une vidéo cède la place à l'image fixe prévue.
    #[test]
    fn asset_forms_fixture_yields_cdn_addresses() {
        let (list, _) = decode_quests(include_str!("../../tests/fixtures/nd/rest_quests_assets.json")).unwrap_or_else(|| panic!("liste attendue"));
        assert_eq!(list.len(), 4);
        let cdn = "https://cdn.discordapp.com";
        // Chemins complets ; `hero` est une vidéo : l'image fixe `quest_bar_hero` la remplace.
        let q = &list[0];
        assert_eq!(q.hero_urls(), [format!("{cdn}/quests/{}/1410000000000000002.jpg", q.id), format!("{cdn}/assets/quests/{}/1410000000000000002.jpg", q.id)]);
        assert_eq!(q.logo_urls(false)[0], format!("{cdn}/quests/{}/1410000000000000009.png", q.id), "logotype pour fond sombre");
        assert_eq!(q.logo_urls(true)[0], format!("{cdn}/quests/{}/1410000000000000008.png", q.id), "logotype pour fond clair");
        assert_eq!(q.logo_urls(false)[2], format!("{cdn}/quests/{}/1410000000000000007.png", q.id), "puis le logotype ordinaire");
        // Récompense en vidéo : la vignette du jeu (version du thème) la remplace.
        assert_eq!(q.reward().and_then(|r| r.asset.as_deref()), None);
        assert_eq!(q.reward_urls(false)[0], format!("{cdn}/quests/{}/1410000000000000006.png", q.id));
        assert!(q.link_is_invite());
        assert_eq!(q.reward().map(|r| r.article.as_str()), Some("une Décoration d'avatar fictive"));
        assert!(q.main_task().is_some_and(|t| t.kind == TaskKind::Achievement && t.text.starts_with("Trouve les 7 cartes")));
        // Adresses entières du CDN.
        let q = &list[1];
        assert_eq!(q.hero_urls()[0], format!("{cdn}/quests/{}/1420000000000000001.webp", q.id));
        assert_eq!(q.logo_urls(false)[0], format!("{cdn}/quests/{}/1420000000000000007.png", q.id));
        assert_eq!(q.reward_urls(false)[0], format!("{cdn}/quests/{}/1420000000000000005.png", q.id));
        assert_eq!(q.link.as_deref(), Some("https://exemple.invalid/appel/jouer"), "`cta_config.link` avant `application.link`");
        assert!(!q.link_is_invite());
        assert_eq!(q.main_task().map(|t| t.kind), Some(TaskKind::PlayOnDesktop));
        // Vidéos seulement : pas d'image d'ambiance. Liens qui ne sont pas en https : écartés.
        let q = &list[2];
        assert!(q.hero.is_none() && q.hero_urls().is_empty());
        assert_eq!(q.link, None);
        assert_eq!(q.logo_urls(true)[0], format!("{cdn}/assets/quests/{}/wordmark_clair.png", q.id));
        assert_eq!(q.logo_urls(false), [format!("{cdn}/assets/quests/{}/wordmark.png", q.id), format!("{cdn}/quests/{}/wordmark.png", q.id), format!("{cdn}/assets/quests/{}/dark/wordmark.png", q.id)]);
        assert_eq!(q.reward_urls(false)[0], format!("{cdn}/assets/quests/{}/pack.png", q.id));
        // Noms de fichiers (forme documentée) ; quête réclamée.
        let q = &list[3];
        assert_eq!(q.hero_urls()[0], format!("{cdn}/assets/quests/{}/hero.png", q.id));
        assert_eq!(q.stage(1_800_000_000_000), Stage::Claimed);
        assert_eq!(https_link(Some(" https://exemple.invalid/a b ")), None);
        assert_eq!(https_link(Some("https://")), None);
    }

    #[test]
    fn status_fixture_and_gateway_events() {
        let (id, s) = decode_quest_status(include_str!("../../tests/fixtures/nd/rest_quest_status.json")).unwrap_or_else(|| panic!("état attendu"));
        assert_eq!(id, Some(8_000_000_000_000_000_001));
        assert!(s.enrolled_at.is_some() && s.completed_at.is_none() && s.claimed_at.is_none());
        assert_eq!(s.progress, vec![("PLAY_ON_DESKTOP".to_string(), 300)], "clé piégée écartée");

        let (tx, rx) = std::sync::mpsc::channel();
        let out = Out { tx, wake: Arc::new(|| {}), people: Default::default(), watch: Default::default(), interest: Default::default() };
        dispatch("QUESTS_USER_STATUS_UPDATE", &format!(r#"{{ "user_status": {} }}"#, include_str!("../../tests/fixtures/nd/rest_quest_status.json")), &out);
        dispatch("QUESTS_USER_STATUS_UPDATE", r#"{ "user_status": { "enrolled_at": null } }"#, &out);
        dispatch("QUESTS_USER_COMPLETION_UPDATE", r#"{ "quest_enrollment_blocked_until": null }"#, &out);
        dispatch("VIRTUAL_CURRENCY_BALANCE_UPDATE", r#"{ "balance": 1400 }"#, &out);
        dispatch("VIRTUAL_CURRENCY_BALANCE_UPDATE", r#"{ "balance": "beaucoup" }"#, &out);
        dispatch("MESSAGE_CREATE", "{}", &out);
        drop(out);
        let events: Vec<Event> = rx.try_iter().collect();
        assert!(
            matches!(&events[..], [Event::Quests(QuestEvent::Status { id: 8_000_000_000_000_000_001, .. }), Event::Quests(QuestEvent::BlockedUntil(None)), Event::Quests(QuestEvent::Orbs(1400))]),
            "{events:?}"
        );
        assert_eq!(claimed_at(r#"{ "claimed_at": "2030-01-01T00:00:00+00:00", "entitlements": [], "errors": [] }"#), Some(1_893_456_000_000));
        assert_eq!(code_of(r#"{ "quest_id": "1", "code": " FICTIF-0000 ", "platform": 0 }"#).as_deref(), Some("FICTIF-0000"));
        assert_eq!(code_of(r#"{ "code": null }"#), None);
    }
}
