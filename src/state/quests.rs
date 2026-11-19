//! Quêtes (lot ND) : la liste des quêtes proposées au compte, leur
//! acceptation, la récompense à réclamer, et la progression **que Discord
//! renvoie**.
//!
//! Progression (0.1.13, réglage « Déclarer ma progression des quêtes »,
//! coupé par défaut) : Ferrocord ne déclare que ce qui a lieu.
//! - quête « jouer sur ordinateur » : un battement (`/quests/{id}/heartbeat`)
//!   part toutes les [`BEAT_EVERY_MS`] tant que la détection des jeux voit
//!   tourner l'application de la quête, et un dernier (`terminal`) quand
//!   elle s'arrête ([`Store::quests_tick`]) ;
//! - quête « regarder une vidéo » : le temps réellement lu par le lecteur de
//!   Ferrocord est déclaré par pas de [`VIDEO_STEP_S`] secondes
//!   (`/quests/{id}/video-progress`, [`Store::quest_watched`]).
//!
//! Jamais : diffusion, console, activité intégrée, vidéo sur mobile, ni
//! aucune progression sans le jeu détecté ou la vidéo en lecture. Un refus
//! de Discord arrête la progression de cette quête pour la session.
//!
//! Points d'API non documentés par Discord ; formes vérifiées sur
//! docs.discord.food, page « Quests » (détail dans `net/quests.rs` et
//! `docs/passerelle-utilisateur.md`). Rien ne part sans un geste : ouverture
//! de la page, « Actualiser », « Accepter », « Réclamer », « Afficher le code ».

use super::event::Cmd;
use super::notice::Problem;
use super::store::Store;
use super::usersettings::Remote;
use std::collections::{HashMap, HashSet};

/// Quêtes gardées au plus.
pub const MAX_QUESTS: usize = 100;
/// Tâches et récompenses lues au plus par quête.
pub const MAX_PARTS: usize = 12;
/// Longueur maximale d'un texte reçu.
pub const MAX_TEXT: usize = 256;
/// La page rouverte après ce délai redemande la liste (ms).
pub const STALE_MS: i64 = 5 * 60_000;
/// Écart entre deux battements d'une quête « jouer » (Discord compte au plus
/// deux minutes par battement : docs.discord.food, « Send Quest Heartbeat »).
pub const BEAT_EVERY_MS: i64 = 60_000;
/// Pas de la progression d'une vidéo, en secondes lues (même page, « Send
/// Quest Video Progress » : « every 10-15 seconds a video plays »).
pub const VIDEO_STEP_S: u32 = 10;
/// Applications lues au plus par tâche.
pub const MAX_APPS: usize = 8;
/// Hôte des images des quêtes (docs.discord.food, « CDN Endpoints » :
/// `/assets/quests/{quest_id}/{asset_name}` et, pour le logotype et la
/// vignette du jeu, `/assets/quests/{quest_id}/{theme}/{asset_name}`).
const CDN: &str = "https://cdn.discordapp.com";
/// Adresses essayées au plus pour une image (voir [`asset_urls`]).
pub const MAX_ASSET_URLS: usize = 4;

/// Ce qu'une tâche demande (docs.discord.food, « Quest Task Type »).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskKind {
    StreamOnDesktop,
    PlayOnDesktop,
    PlayOnXbox,
    PlayOnPlaystation,
    WatchVideo,
    WatchVideoOnMobile,
    PlayActivity,
    Achievement,
    Other,
}

impl TaskKind {
    pub fn parse(s: &str) -> TaskKind {
        match s {
            "STREAM_ON_DESKTOP" => TaskKind::StreamOnDesktop,
            "PLAY_ON_DESKTOP" | "PLAY_ON_DESKTOP_V2" => TaskKind::PlayOnDesktop,
            "PLAY_ON_XBOX" => TaskKind::PlayOnXbox,
            "PLAY_ON_PLAYSTATION" => TaskKind::PlayOnPlaystation,
            "WATCH_VIDEO" => TaskKind::WatchVideo,
            "WATCH_VIDEO_ON_MOBILE" => TaskKind::WatchVideoOnMobile,
            "PLAY_ACTIVITY" => TaskKind::PlayActivity,
            "ACHIEVEMENT_IN_GAME" | "ACHIEVEMENT_IN_ACTIVITY" => TaskKind::Achievement,
            _ => TaskKind::Other,
        }
    }

    /// La cible est-elle une durée en secondes ?
    pub fn timed(self) -> bool {
        !matches!(self, TaskKind::Achievement | TaskKind::Other)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    pub kind: TaskKind,
    /// Nom de la tâche tel que reçu (clé de la progression).
    pub key: String,
    /// Cible (secondes pour une durée).
    pub target: u32,
    /// Consigne rédigée par l'éditeur (`messages.task_description`), s'il y en a une.
    pub text: String,
    /// Applications qui comptent pour une tâche « jouer » (`applications[].id`).
    pub apps: Vec<u64>,
    /// Vidéo d'une tâche « regarder » (`assets.video_low_res.url`, sinon
    /// `assets.video.url`) : toujours en `https://`.
    pub video: Option<String>,
}

/// Grande famille d'une tâche, pour le filtre de la page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskClass {
    Play,
    Video,
    Other,
}

impl TaskKind {
    pub fn class(self) -> TaskClass {
        match self {
            TaskKind::PlayOnDesktop | TaskKind::PlayOnXbox | TaskKind::PlayOnPlaystation | TaskKind::StreamOnDesktop | TaskKind::PlayActivity => TaskClass::Play,
            TaskKind::WatchVideo | TaskKind::WatchVideoOnMobile => TaskClass::Video,
            TaskKind::Achievement | TaskKind::Other => TaskClass::Other,
        }
    }
}

/// Type de récompense (docs.discord.food, « Quest Reward Type »).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RewardKind {
    /// Code à saisir chez l'éditeur.
    Code,
    InGame,
    /// Parure de profil.
    Collectible,
    Orbs,
    /// Jours de Nitro.
    Premium,
    Other,
}

impl RewardKind {
    pub fn from_code(c: u64) -> RewardKind {
        match c {
            1 => RewardKind::Code,
            2 => RewardKind::InGame,
            3 => RewardKind::Collectible,
            4 => RewardKind::Orbs,
            5 => RewardKind::Premium,
            _ => RewardKind::Other,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reward {
    pub kind: RewardKind,
    pub name: String,
    /// Nom précédé de son article (`messages.name_with_article`), s'il est donné.
    pub article: String,
    pub orbs: Option<u32>,
    /// Image fixe de la récompense telle que reçue (voir [`asset_urls`]) ;
    /// une vidéo n'est pas gardée.
    pub asset: Option<String>,
}

/// Image du jeu (logotype, vignette) : la version ordinaire et celles prévues
/// pour un fond sombre ou clair, telles que reçues.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Themed {
    pub base: Option<String>,
    pub dark: Option<String>,
    pub light: Option<String>,
}

impl Themed {
    /// Adresses à essayer dans l'ordre : la version du thème, la version
    /// ordinaire, puis la version ordinaire rangée dans le dossier du thème.
    pub fn urls(&self, quest_id: u64, light: bool) -> Vec<String> {
        let (own, folder) = if light { (&self.light, "light") } else { (&self.dark, "dark") };
        let mut out: Vec<String> = Vec::new();
        let mut push = |urls: Vec<String>| {
            for u in urls {
                if !out.contains(&u) && out.len() < MAX_ASSET_URLS {
                    out.push(u);
                }
            }
        };
        push(own.as_deref().map(|n| asset_urls(quest_id, n)).unwrap_or_default());
        push(self.base.as_deref().map(|n| asset_urls(quest_id, n)).unwrap_or_default());
        // Dossier de thème : seulement pour un simple nom de fichier.
        if let Some(n) = self.base.as_deref().filter(|n| !n.contains('/')) {
            push(asset_urls(quest_id, &format!("{folder}/{n}")).into_iter().take(1).collect());
        }
        out
    }
}

/// Où j'en suis d'une quête, d'après Discord. Heures en ms Unix.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub enrolled_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub claimed_at: Option<i64>,
    /// Progression par tâche (nom de la tâche, valeur).
    pub progress: Vec<(String, u32)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quest {
    pub id: u64,
    /// Application du jeu de la quête (`config.application.id`).
    pub app_id: Option<u64>,
    pub name: String,
    pub game: String,
    pub publisher: String,
    pub starts_ms: Option<i64>,
    pub expires_ms: Option<i64>,
    /// Image d'ambiance fixe telle que reçue (`hero`, sinon `quest_bar_hero` ;
    /// une vidéo n'est pas gardée).
    pub hero: Option<String>,
    /// Vignette et logotype du jeu.
    pub tile: Themed,
    pub logo: Themed,
    /// Lien prévu par la quête pour son second bouton (`cta_config.link`,
    /// sinon `application.link`) : toujours en `https://`.
    pub link: Option<String>,
    /// Couleurs du dégradé de la quête (0xRRGGBB).
    pub colors: Option<[u32; 2]>,
    pub tasks: Vec<Task>,
    /// Une seule des tâches suffit (`join_operator: "or"`).
    pub any_task: bool,
    pub rewards: Vec<Reward>,
    /// La récompense peut être réclamée ici (elle vaut pour toutes les plateformes) ;
    /// sinon il faut choisir une console, dans l'application officielle.
    pub claim_here: bool,
    pub status: Option<Status>,
}

/// Étape d'une quête pour l'utilisateur.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Proposée, pas encore acceptée.
    Open,
    /// Acceptée, en cours.
    Accepted,
    /// Terminée : la récompense attend d'être réclamée.
    Completed,
    Claimed,
    /// Échue sans avoir été terminée.
    Expired,
}

impl Quest {
    pub fn stage(&self, now_ms: i64) -> Stage {
        let s = self.status.as_ref();
        if s.is_some_and(|s| s.claimed_at.is_some()) {
            Stage::Claimed
        } else if s.is_some_and(|s| s.completed_at.is_some()) {
            Stage::Completed
        } else if self.expires_ms.is_some_and(|e| e <= now_ms) {
            Stage::Expired
        } else if s.is_some_and(|s| s.enrolled_at.is_some()) {
            Stage::Accepted
        } else {
            Stage::Open
        }
    }

    /// Progression (valeur, cible) de la tâche la plus avancée si une seule
    /// suffit, sinon de la moins avancée ; `None` sans tâche chiffrée.
    pub fn progress(&self) -> Option<(u32, u32)> {
        let done = |t: &Task| {
            let v = self.status.as_ref().and_then(|s| s.progress.iter().find(|(k, _)| *k == t.key)).map_or(0, |(_, v)| *v);
            (v.min(t.target), t.target)
        };
        let ratio = |(v, t): &(u32, u32)| u64::from(*v) * 1_000_000 / u64::from((*t).max(1));
        let all = self.tasks.iter().filter(|t| t.target > 0).map(done);
        // À égalité, la première tâche (`max_by_key` rendrait la dernière).
        if self.any_task { all.rev().max_by_key(ratio) } else { all.min_by_key(ratio) }
    }

    /// Valeur atteinte pour la tâche `key`, d'après Discord.
    pub fn task_value(&self, key: &str) -> u32 {
        self.status.as_ref().and_then(|s| s.progress.iter().find(|(k, _)| k == key)).map_or(0, |(_, v)| *v)
    }

    /// Applications à voir tourner pour avancer la tâche « jouer sur
    /// ordinateur », tant qu'elle n'est pas finie : celles de la tâche,
    /// sinon celle de la quête.
    pub fn play_apps(&self) -> Vec<u64> {
        let Some(t) = self.tasks.iter().find(|t| t.kind == TaskKind::PlayOnDesktop && self.task_value(&t.key) < t.target) else { return Vec::new() };
        if t.apps.is_empty() { self.app_id.into_iter().collect() } else { t.apps.clone() }
    }

    /// Tâche « regarder une vidéo » encore à finir et dont la vidéo est connue.
    pub fn video_task(&self) -> Option<&Task> {
        self.tasks.iter().find(|t| t.kind == TaskKind::WatchVideo && t.video.is_some() && t.target > 0 && self.task_value(&t.key) < t.target)
    }

    /// Première récompense (celle que la page met en avant).
    pub fn reward(&self) -> Option<&Reward> {
        self.rewards.first()
    }

    /// Tâche mise en avant par la carte : jouer sur ordinateur si la quête
    /// le propose, sinon la première.
    pub fn main_task(&self) -> Option<&Task> {
        self.tasks.iter().find(|t| t.kind == TaskKind::PlayOnDesktop).or(self.tasks.first())
    }

    /// Adresses à essayer pour l'image d'ambiance.
    pub fn hero_urls(&self) -> Vec<String> {
        self.hero.as_deref().map(|n| asset_urls(self.id, n)).unwrap_or_default()
    }

    /// Adresses à essayer pour le logotype du jeu, sur fond clair ou sombre.
    pub fn logo_urls(&self, light: bool) -> Vec<String> {
        self.logo.urls(self.id, light)
    }

    /// Adresses à essayer pour la vignette de la récompense : son image fixe,
    /// puis la vignette du jeu (récompense sans image, ou en vidéo seulement).
    pub fn reward_urls(&self, light: bool) -> Vec<String> {
        let mut out = self.reward().and_then(|r| r.asset.as_deref()).map(|n| asset_urls(self.id, n)).unwrap_or_default();
        for u in self.tile.urls(self.id, light) {
            if !out.contains(&u) && out.len() < MAX_ASSET_URLS {
                out.push(u);
            }
        }
        out
    }

    /// Le lien de la quête est-il une invitation à un serveur ?
    pub fn link_is_invite(&self) -> bool {
        self.link.as_deref().is_some_and(|l| ["https://discord.gg/", "https://discord.com/invite/"].iter().any(|p| l.starts_with(p)))
    }
}

/// `name` désigne-t-il une vidéo (`hero` et `asset` en sont parfois) ?
pub fn is_video(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [".mp4", ".webm", ".mov", ".m3u8", ".m4v"].iter().any(|ext| n.ends_with(ext))
}

/// Adresses à essayer, dans l'ordre, pour l'image `name` de la quête
/// `quest_id`. Ces valeurs existent sous plusieurs formes ; chacune mène au
/// CDN, jamais ailleurs :
/// - un nom de fichier (`hero.jpg`, `dark/logo.png`) : le dossier documenté
///   `/assets/quests/{quest_id}/`, puis `/quests/{quest_id}/` ;
/// - un chemin complet (`quests/{id}/{fichier}`, `assets/quests/{id}/{fichier}`) :
///   tel quel, puis sous l'autre racine ;
/// - une adresse entière du CDN (`https://cdn.discordapp.com/…`) : son chemin,
///   relu comme ci-dessus.
///
/// Seuls passent les chemins simples (lettres, chiffres, `_`, `-`, `.`,
/// dossiers par `/`) d'une image fixe : ni autre hôte, ni `..`, ni paramètres,
/// ni vidéo (`.mp4`, `.webm`). Liste vide : rien à demander.
pub fn asset_urls(quest_id: u64, name: &str) -> Vec<String> {
    let name = name.strip_prefix("https://cdn.discordapp.com/").unwrap_or(name);
    let ok = !name.is_empty()
        && name.len() <= 160
        && name.split('/').all(|seg| !seg.is_empty() && !seg.starts_with('.') && seg.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')))
        && [".png", ".jpg", ".jpeg", ".webp", ".gif"].iter().any(|ext| name.to_ascii_lowercase().ends_with(ext));
    if !ok {
        return Vec::new();
    }
    if let Some(rest) = name.strip_prefix("assets/quests/") {
        vec![format!("{CDN}/assets/quests/{rest}"), format!("{CDN}/quests/{rest}")]
    } else if let Some(rest) = name.strip_prefix("quests/") {
        vec![format!("{CDN}/quests/{rest}"), format!("{CDN}/assets/quests/{rest}")]
    } else {
        vec![format!("{CDN}/assets/quests/{quest_id}/{name}"), format!("{CDN}/quests/{quest_id}/{name}")]
    }
}

/// Première adresse de [`asset_urls`].
pub fn asset_url(quest_id: u64, name: &str) -> Option<String> {
    asset_urls(quest_id, name).into_iter().next()
}

/// Ordre de la page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    /// Ce qui attend un geste d'abord (à réclamer, puis acceptées), puis
    /// l'ordre dans lequel Discord propose les quêtes.
    #[default]
    Suggested,
    /// Les plus récentes d'abord.
    Newest,
    /// Celles qui finissent le plus tôt d'abord.
    EndingSoon,
}

/// Filtre de la page : dans un groupe (récompenses, tâches), rien de coché
/// vaut tout coché ; une quête passe si l'une de ses récompenses et l'une de
/// ses tâches sont cochées.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    pub orbs: bool,
    /// Parures de profil.
    pub cosmetic: bool,
    /// Objets en jeu et codes.
    pub in_game: bool,
    pub nitro: bool,
    pub play: bool,
    pub video: bool,
    pub other: bool,
}

impl Filter {
    pub fn is_empty(&self) -> bool {
        *self == Filter::default()
    }

    pub fn accepts(&self, q: &Quest) -> bool {
        let reward = |k: RewardKind| match k {
            RewardKind::Orbs => self.orbs,
            RewardKind::Collectible => self.cosmetic,
            RewardKind::Code | RewardKind::InGame => self.in_game,
            RewardKind::Premium => self.nitro,
            RewardKind::Other => false,
        };
        let task = |k: TaskKind| match k.class() {
            TaskClass::Play => self.play,
            TaskClass::Video => self.video,
            TaskClass::Other => self.other,
        };
        let any_reward = self.orbs || self.cosmetic || self.in_game || self.nitro;
        let any_task = self.play || self.video || self.other;
        (!any_reward || q.rewards.iter().any(|r| reward(r.kind))) && (!any_task || q.tasks.iter().any(|t| task(t.kind)))
    }
}

/// Quêtes d'un onglet, filtrées et triées : « disponibles » (proposées,
/// acceptées, terminées à réclamer) ou « réclamées ». Les quêtes échues sans
/// avoir été terminées et celles de `hidden` ne sont pas montrées.
pub fn arrange<'a>(list: &'a [Quest], claimed: bool, sort: Sort, filter: Filter, hidden: &HashSet<u64>, now_ms: i64) -> Vec<&'a Quest> {
    let mut out: Vec<(usize, &Quest)> = list
        .iter()
        .enumerate()
        .filter(|(_, q)| match q.stage(now_ms) {
            Stage::Claimed => claimed,
            Stage::Expired => false,
            _ => !claimed,
        })
        .filter(|(_, q)| !hidden.contains(&q.id) && filter.accepts(q))
        .collect();
    match sort {
        Sort::Suggested => out.sort_by_key(|(i, q)| {
            let rank = match q.stage(now_ms) {
                Stage::Completed => 0,
                Stage::Accepted => 1,
                _ => 2,
            };
            (rank, *i)
        }),
        Sort::Newest => out.sort_by_key(|(i, q)| (std::cmp::Reverse(q.starts_ms.unwrap_or(i64::MIN)), *i)),
        Sort::EndingSoon => out.sort_by_key(|(i, q)| (q.expires_ms.unwrap_or(i64::MAX), *i)),
    }
    out.into_iter().map(|(_, q)| q).collect()
}

/// Adresse de la quête dans l'application officielle (là où elle se termine).
pub fn official_url(quest_id: u64) -> String {
    format!("https://discord.com/quests/{quest_id}")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuestCmd {
    /// `GET /quests/@me`.
    List,
    /// `POST /quests/{id}/enroll`.
    Enroll { id: u64 },
    /// `POST /quests/{id}/claim-reward`.
    Claim { id: u64 },
    /// `GET /quests/{id}/reward-code`.
    Code { id: u64 },
    /// `POST /quests/{id}/heartbeat` : le jeu `application_id` tourne
    /// (`terminal` : il vient de s'arrêter).
    Heartbeat { id: u64, application_id: u64, terminal: bool },
    /// `POST /quests/{id}/video-progress` : secondes de vidéo réellement lues.
    VideoProgress { id: u64, seconds: u32 },
}

/// Pourquoi une écriture n'a pas abouti.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Discord demande un captcha : Ferrocord s'arrête là.
    Captcha,
    Other(Problem),
}

#[derive(Debug)]
pub enum QuestEvent {
    /// Quêtes, et l'heure (ms Unix) avant laquelle aucune ne peut être acceptée.
    List(Result<(Vec<Quest>, Option<i64>), Problem>),
    Enrolled { id: u64, result: Result<Status, Refused> },
    /// Récompense réclamée à cette heure (ms Unix).
    Claimed { id: u64, result: Result<Option<i64>, Refused> },
    Code { id: u64, result: Result<String, Refused> },
    /// Réponse à un battement ou à une progression de vidéo : mon nouvel
    /// état. `Err(None)` : Discord n'accepte la progression d'un jeu que du
    /// client de bureau (identité « application de bureau » des réglages).
    Progress { id: u64, result: Result<Status, Option<Refused>> },
    /// QUESTS_USER_STATUS_UPDATE : mon état dans une quête a changé.
    Status { id: u64, status: Status },
    /// QUESTS_USER_COMPLETION_UPDATE.
    BlockedUntil(Option<i64>),
    /// VIRTUAL_CURRENCY_BALANCE_UPDATE : solde d'Orbes.
    Orbs(u64),
}

#[derive(Default)]
pub struct Quests {
    pub list: Remote<Vec<Quest>>,
    /// Heure de la dernière liste reçue (ms Unix).
    loaded_ms: i64,
    /// Aucune quête ne peut être acceptée avant cette heure (ms Unix).
    pub blocked_until: Option<i64>,
    /// Écritures en cours (une par quête).
    busy: HashSet<u64>,
    /// Dernier refus par quête, montré sur sa carte.
    refused: HashMap<u64, Refused>,
    /// Codes de récompense demandés.
    codes: HashMap<u64, String>,
    /// Solde d'Orbes, s'il a été annoncé par la passerelle.
    pub orbs: Option<u64>,
    /// Quêtes masquées par l'utilisateur : choix local, pour la session ;
    /// rien n'est dit à Discord.
    pub hidden: HashSet<u64>,
    /// Quêtes « jouer » dont le jeu tourne : application et heure (ms Unix)
    /// du dernier battement.
    beats: HashMap<u64, (u64, i64)>,
    /// Vidéo en cours de lecture, par quête : valeur de Discord quand la
    /// lecture a commencé, secondes déclarées, dernier cumul reçu du lecteur.
    video_sent: HashMap<u64, (u32, u32, u32)>,
    /// Quêtes dont Discord a refusé la progression : plus rien n'est déclaré
    /// pour elles jusqu'à la fin de la session.
    progress_off: HashSet<u64>,
    /// Un battement a été refusé faute d'identité de bureau.
    pub needs_identity: bool,
}

impl Quests {
    pub fn clear(&mut self) {
        *self = Quests::default();
    }

    pub fn busy(&self, id: u64) -> bool {
        self.busy.contains(&id)
    }

    pub fn refused(&self, id: u64) -> Option<&Refused> {
        self.refused.get(&id)
    }

    pub fn code(&self, id: u64) -> Option<&str> {
        self.codes.get(&id).map(String::as_str)
    }

    /// « Masquer » : la quête quitte la page jusqu'à « Réafficher » ou la fin de la session.
    pub fn hide(&mut self, id: u64) {
        if self.hidden.len() < MAX_QUESTS {
            self.hidden.insert(id);
        }
    }

    pub fn unhide_all(&mut self) {
        self.hidden.clear();
    }

    fn quest_mut(&mut self, id: u64) -> Option<&mut Quest> {
        match &mut self.list {
            Remote::Ready(l) => l.iter_mut().find(|q| q.id == id),
            _ => None,
        }
    }

    pub fn quest(&self, id: u64) -> Option<&Quest> {
        self.list.ready()?.iter().find(|q| q.id == id)
    }

    /// Le jeu de cette quête tourne et sa progression est déclarée.
    pub fn beating(&self, id: u64) -> bool {
        self.beats.contains_key(&id)
    }

    /// Discord a refusé la progression de cette quête.
    pub fn progress_refused(&self, id: u64) -> bool {
        self.progress_off.contains(&id)
    }

    /// Nombre de quêtes terminées dont la récompense attend.
    pub fn claimable(&self) -> usize {
        self.list.ready().map_or(0, |l| l.iter().filter(|q| q.status.as_ref().is_some_and(|s| s.completed_at.is_some() && s.claimed_at.is_none())).count())
    }
}

impl Store {
    /// Page des quêtes ouverte : la liste est demandée si elle est inconnue,
    /// en échec, ou vieille de plus de [`STALE_MS`].
    pub fn quests_open(&mut self) {
        let stale = matches!(self.quests.list, Remote::Ready(_)) && self.now().saturating_sub(self.quests.loaded_ms) > STALE_MS;
        if stale || matches!(self.quests.list, Remote::Idle | Remote::Failed(_)) {
            self.quests_refresh();
        }
    }

    /// « Actualiser » : la liste est redemandée (sauf si elle l'est déjà).
    /// La liste affichée reste en place en attendant.
    pub fn quests_refresh(&mut self) {
        if self.quests.loaded_ms == i64::MIN {
            return;
        }
        if !matches!(self.quests.list, Remote::Ready(_)) {
            self.quests.list = Remote::Loading;
        }
        // Marque « demande en cours » : une seule à la fois.
        self.quests.loaded_ms = i64::MIN;
        self.push_command(Cmd::Quests(QuestCmd::List));
    }

    /// La liste est-elle en cours de (re)chargement ?
    pub fn quests_loading(&self) -> bool {
        self.quests.loaded_ms == i64::MIN
    }

    fn quest_write(&mut self, id: u64, cmd: QuestCmd) -> bool {
        if self.quests.quest(id).is_none() || !self.quests.busy.insert(id) {
            return false;
        }
        self.quests.refused.remove(&id);
        self.push_command(Cmd::Quests(cmd));
        true
    }

    /// « Accepter » : seulement une quête proposée, ouverte, hors blocage.
    pub fn quest_enroll(&mut self, id: u64) -> bool {
        let now = self.now();
        let open = self.quests.quest(id).is_some_and(|q| q.stage(now) == Stage::Open);
        open && self.quests.blocked_until.is_none_or(|t| t <= now) && self.quest_write(id, QuestCmd::Enroll { id })
    }

    /// « Réclamer » : seulement une quête que Discord dit terminée.
    pub fn quest_claim(&mut self, id: u64) -> bool {
        let now = self.now();
        self.quests.quest(id).is_some_and(|q| q.claim_here && q.stage(now) == Stage::Completed) && self.quest_write(id, QuestCmd::Claim { id })
    }

    /// « Afficher le code » d'une récompense réclamée.
    pub fn quest_code(&mut self, id: u64) -> bool {
        let now = self.now();
        let claimed = self.quests.quest(id).is_some_and(|q| q.stage(now) == Stage::Claimed && q.reward().is_some_and(|r| matches!(r.kind, RewardKind::Code | RewardKind::InGame)));
        claimed && self.quests.code(id).is_none() && self.quest_write(id, QuestCmd::Code { id })
    }

    /// Progression des quêtes « jouer sur ordinateur » : à appeler
    /// régulièrement. `on` : le réglage. Un battement part pour chaque quête
    /// acceptée dont une application tourne d'après la détection des jeux
    /// (`activity.running`), au plus toutes les [`BEAT_EVERY_MS`] ; un
    /// dernier, `terminal`, quand le jeu s'arrête. Rend le délai (ms) avant
    /// le prochain battement dû, s'il y en a un.
    pub fn quests_tick(&mut self, on: bool) -> Option<i64> {
        if self.quests.beats.is_empty() && (!on || self.activity.running.is_empty()) {
            return None;
        }
        let now = self.now();
        let running: Vec<u64> = self.activity.running.iter().filter_map(|g| g.app_id).collect();
        let accepted = |q: &Quest, off: &HashSet<u64>| q.stage(now) == Stage::Accepted && !off.contains(&q.id);
        let mut wanted: Vec<(u64, u64)> = Vec::new();
        if on && let Some(list) = self.quests.list.ready() {
            for q in list.iter().filter(|q| accepted(q, &self.quests.progress_off)) {
                if let Some(app) = q.play_apps().into_iter().find(|a| running.contains(a)) {
                    wanted.push((q.id, app));
                }
            }
        }
        let mut cmds = Vec::new();
        let stopped: Vec<(u64, u64)> = self.quests.beats.iter().filter(|(id, _)| !wanted.iter().any(|(w, _)| w == *id)).map(|(id, (app, _))| (*id, *app)).collect();
        for (id, application_id) in stopped {
            self.quests.beats.remove(&id);
            // Jeu arrêté (et non quête finie, refusée ou réglage coupé) : le dernier battement le dit.
            if on && self.quests.quest(id).is_some_and(|q| accepted(q, &self.quests.progress_off) && q.play_apps().contains(&application_id)) {
                cmds.push(QuestCmd::Heartbeat { id, application_id, terminal: true });
            }
        }
        let mut next: Option<i64> = None;
        for (id, application_id) in wanted {
            let last = self.quests.beats.get(&id).map(|(_, at)| *at);
            let at = match last {
                Some(at) if now.saturating_sub(at) < BEAT_EVERY_MS => at,
                _ => {
                    cmds.push(QuestCmd::Heartbeat { id, application_id, terminal: false });
                    now
                }
            };
            self.quests.beats.insert(id, (application_id, at));
            let wait = (BEAT_EVERY_MS - now.saturating_sub(at)).max(0);
            next = Some(next.map_or(wait, |n| n.min(wait)));
        }
        for c in cmds {
            self.push_command(Cmd::Quests(c));
        }
        next
    }

    /// Le lecteur a réellement lu `seconds` secondes de la vidéo de la quête
    /// `id` (cumul depuis l'ouverture, hors pauses et sauts) : la
    /// progression est déclarée par pas de [`VIDEO_STEP_S`], et à la cible.
    /// `on` : le réglage. Rien ne part pour une quête non acceptée.
    pub fn quest_watched(&mut self, id: u64, seconds: u32, on: bool) {
        let now = self.now();
        let Some(q) = self.quests.quest(id).filter(|q| on && q.stage(now) == Stage::Accepted && !self.quests.progress_off.contains(&id)) else { return };
        let Some(t) = q.video_task() else { return };
        let (value, target) = (q.task_value(&t.key), t.target);
        // Le temps lu s'ajoute à ce que Discord avait compté au début de cette
        // lecture ; un cumul qui repart de zéro (lecteur rouvert) repart de
        // la valeur du moment.
        let (base, sent) = match self.quests.video_sent.get(&id) {
            Some((base, sent, last)) if seconds >= *last => (*base, *sent),
            _ => (value, value),
        };
        let total = base.saturating_add(seconds).min(target);
        let due = total >= sent.saturating_add(VIDEO_STEP_S) || (total >= target && sent < target);
        self.quests.video_sent.insert(id, (base, if due { total } else { sent }, seconds));
        if due {
            self.push_command(Cmd::Quests(QuestCmd::VideoProgress { id, seconds: total }));
        }
    }

    pub(super) fn apply_quests(&mut self, e: QuestEvent) {
        let now = self.now();
        let q = &mut self.quests;
        match e {
            QuestEvent::List(Ok((mut list, blocked))) => {
                list.truncate(MAX_QUESTS);
                q.list = Remote::Ready(list);
                q.blocked_until = blocked;
                q.loaded_ms = now;
            }
            QuestEvent::List(Err(p)) => {
                // Un échec d'actualisation garde la liste affichée.
                if !matches!(q.list, Remote::Ready(_)) {
                    q.list = Remote::Failed(p);
                }
                q.loaded_ms = 0;
            }
            QuestEvent::Enrolled { id, result } => {
                q.busy.remove(&id);
                match result {
                    Ok(status) => {
                        if let Some(quest) = q.quest_mut(id) {
                            quest.status = Some(status);
                        }
                    }
                    Err(r) => {
                        q.refused.insert(id, r);
                    }
                }
            }
            QuestEvent::Claimed { id, result } => {
                q.busy.remove(&id);
                match result {
                    Ok(at) => {
                        if let Some(quest) = q.quest_mut(id) {
                            let s = quest.status.get_or_insert_with(Status::default);
                            s.claimed_at = at.or(Some(now));
                        }
                    }
                    Err(r) => {
                        q.refused.insert(id, r);
                    }
                }
            }
            QuestEvent::Code { id, result } => {
                q.busy.remove(&id);
                match result {
                    Ok(code) => {
                        q.codes.insert(id, code);
                    }
                    Err(r) => {
                        q.refused.insert(id, r);
                    }
                }
            }
            QuestEvent::Status { id, status } => {
                if let Some(quest) = q.quest_mut(id) {
                    quest.status = Some(status);
                }
            }
            QuestEvent::Progress { id, result } => match result {
                Ok(status) => {
                    if let Some(quest) = q.quest_mut(id) {
                        quest.status = Some(status);
                    }
                }
                // Refus : plus aucune déclaration pour cette quête, pas de nouvel essai.
                Err(r) => {
                    q.beats.remove(&id);
                    if q.progress_off.len() < MAX_QUESTS {
                        q.progress_off.insert(id);
                    }
                    match r {
                        Some(r) => {
                            q.refused.insert(id, r);
                        }
                        None => q.needs_identity = true,
                    }
                }
            },
            QuestEvent::BlockedUntil(t) => q.blocked_until = t,
            QuestEvent::Orbs(n) => q.orbs = Some(n),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Progression « jouer » : rien sans le réglage ni sans le jeu ; un
    /// battement à la détection, puis un par minute ; un dernier à l'arrêt
    /// du jeu ; un refus arrête tout pour cette quête.
    #[test]
    fn play_progress_beats_only_while_the_game_runs() {
        use crate::state::activity::RunningGame;
        let mut s = Store::new();
        let mut q = quest(1);
        q.status = Some(Status { enrolled_at: Some(1), ..Default::default() });
        let mut other = quest(2);
        other.tasks[0].apps = vec![99];
        s.apply(crate::state::Event::Quests(QuestEvent::List(Ok((vec![q, other], None)))));
        s.take_commands();
        let game = |app: u64| RunningGame { app_id: Some(app), name: "Jeu".into(), start_ms: 0 };
        let beats = |s: &mut Store| s.take_commands().into_iter().filter_map(|c| match c { Cmd::Quests(QuestCmd::Heartbeat { id, application_id, terminal }) => Some((id, application_id, terminal)), _ => None }).collect::<Vec<_>>();
        // Jeu absent, autre jeu, réglage coupé, quête non acceptée : rien.
        assert_eq!(s.quests_tick(true), None);
        s.activity.running = vec![game(5)];
        assert_eq!(s.quests_tick(true), None);
        s.activity.running = vec![game(77), game(99)];
        assert_eq!(s.quests_tick(false), None);
        assert!(beats(&mut s).is_empty());
        // Le jeu de la quête acceptée tourne : un battement, puis rien avant une minute.
        assert!(s.quests_tick(true).is_some_and(|w| w > BEAT_EVERY_MS - 1_000));
        assert_eq!(beats(&mut s), [(1, 77, false)]);
        assert!(s.quests.beating(1) && !s.quests.beating(2));
        s.clock_offset += 20_000;
        assert!(s.quests_tick(true).is_some_and(|w| (BEAT_EVERY_MS - 21_000..=BEAT_EVERY_MS - 20_000).contains(&w)));
        assert!(beats(&mut s).is_empty());
        s.clock_offset += 40_000;
        s.quests_tick(true);
        assert_eq!(beats(&mut s), [(1, 77, false)]);
        // Jeu arrêté : un dernier battement, puis plus rien.
        s.activity.running.clear();
        assert_eq!(s.quests_tick(true), None);
        assert_eq!(beats(&mut s), [(1, 77, true)]);
        assert_eq!(s.quests_tick(true), None);
        assert!(beats(&mut s).is_empty() && !s.quests.beating(1));
        // Refus de Discord : plus aucun battement pour cette quête.
        s.activity.running = vec![game(77)];
        s.quests_tick(true);
        assert_eq!(beats(&mut s).len(), 1);
        s.apply(crate::state::Event::Quests(QuestEvent::Progress { id: 1, result: Err(None) }));
        assert!(s.quests.progress_refused(1) && s.quests.needs_identity && !s.quests.beating(1));
        s.clock_offset += BEAT_EVERY_MS;
        assert_eq!(s.quests_tick(true), None);
        assert!(beats(&mut s).is_empty(), "ni battement ni battement final après un refus");
    }

    /// Quête finie par la réponse de Discord : le battement s'arrête sans « terminal ».
    #[test]
    fn play_progress_stops_when_the_task_is_done() {
        use crate::state::activity::RunningGame;
        let mut s = Store::new();
        let mut q = quest(1);
        q.status = Some(Status { enrolled_at: Some(1), ..Default::default() });
        s.apply(crate::state::Event::Quests(QuestEvent::List(Ok((vec![q], None)))));
        s.activity.running = vec![RunningGame { app_id: Some(77), name: "Jeu".into(), start_ms: 0 }];
        s.quests_tick(true);
        s.take_commands();
        let done = Status { enrolled_at: Some(1), completed_at: None, claimed_at: None, progress: vec![("PLAY_ON_DESKTOP".into(), 900)] };
        s.apply(crate::state::Event::Quests(QuestEvent::Progress { id: 1, result: Ok(done) }));
        s.clock_offset += BEAT_EVERY_MS;
        assert_eq!(s.quests_tick(true), None);
        assert!(s.take_commands().is_empty());
    }

    /// Vidéo : le temps lu est déclaré par pas de dix secondes, ajouté à ce
    /// que Discord a déjà compté, plafonné à la cible ; rien sans le réglage.
    #[test]
    fn video_progress_is_reported_in_steps_up_to_the_target() {
        let mut s = Store::new();
        let mut q = quest(1);
        q.status = Some(Status { enrolled_at: Some(1), completed_at: None, claimed_at: None, progress: vec![("WATCH_VIDEO".into(), 5)] });
        s.apply(crate::state::Event::Quests(QuestEvent::List(Ok((vec![q, quest(2)], None)))));
        s.take_commands();
        let sent = |s: &mut Store| s.take_commands().into_iter().filter_map(|c| match c { Cmd::Quests(QuestCmd::VideoProgress { id, seconds }) => Some((id, seconds)), _ => None }).collect::<Vec<_>>();
        s.quest_watched(1, 12, false);
        s.quest_watched(2, 12, true);
        s.quest_watched(1, 9, true);
        assert!(sent(&mut s).is_empty(), "réglage coupé, quête non acceptée, pas encore dix secondes");
        s.quest_watched(1, 10, true);
        s.quest_watched(1, 14, true);
        assert_eq!(sent(&mut s), [(1, 15)]);
        s.quest_watched(1, 21, true);
        s.quest_watched(1, 500, true);
        s.quest_watched(1, 600, true);
        assert_eq!(sent(&mut s), [(1, 26), (1, 30)], "la cible (30 s) n'est déclarée qu'une fois");
    }

    pub(crate) fn quest(id: u64) -> Quest {
        Quest {
            id,
            app_id: None,
            name: format!("Quête {id}"),
            game: "Jeu fictif".into(),
            publisher: "Studio fictif".into(),
            starts_ms: Some(0),
            expires_ms: Some(i64::MAX),
            hero: Some("hero.jpg".into()),
            tile: Themed::default(),
            logo: Themed::default(),
            link: None,
            colors: None,
            tasks: vec![Task { kind: TaskKind::PlayOnDesktop, key: "PLAY_ON_DESKTOP".into(), target: 900, text: String::new(), apps: vec![77], video: None }, Task { kind: TaskKind::WatchVideo, key: "WATCH_VIDEO".into(), target: 30, text: String::new(), apps: Vec::new(), video: Some("https://exemple.invalid/v.mp4".into()) }],
            any_task: true,
            rewards: vec![Reward { kind: RewardKind::Orbs, name: "700 Orbes".into(), article: String::new(), orbs: Some(700), asset: None }],
            claim_here: true,
            status: None,
        }
    }

    #[test]
    fn stages_and_progress() {
        let mut q = quest(1);
        assert_eq!((q.stage(5), q.progress()), (Stage::Open, Some((0, 900))));
        q.status = Some(Status { enrolled_at: Some(1), progress: vec![("PLAY_ON_DESKTOP".into(), 450), ("WATCH_VIDEO".into(), 10)], ..Default::default() });
        assert_eq!(q.stage(5), Stage::Accepted);
        assert_eq!(q.progress(), Some((450, 900)), "une tâche suffit : la plus avancée");
        q.any_task = false;
        assert_eq!(q.progress(), Some((10, 30)), "toutes les tâches : la moins avancée");
        q.status.as_mut().unwrap_or_else(|| panic!("état")).progress[1].1 = 9999;
        assert_eq!(q.progress(), Some((450, 900)), "valeur bornée à la cible");
        q.expires_ms = Some(4);
        assert_eq!(q.stage(5), Stage::Expired);
        q.status.as_mut().unwrap_or_else(|| panic!("état")).completed_at = Some(3);
        assert_eq!(q.stage(5), Stage::Completed, "terminée à temps : la récompense reste à réclamer");
        q.status.as_mut().unwrap_or_else(|| panic!("état")).claimed_at = Some(4);
        assert_eq!(q.stage(5), Stage::Claimed);
        q.tasks.clear();
        assert_eq!(q.progress(), None);
    }

    #[test]
    fn asset_names_cannot_leave_the_quest_folder() {
        assert_eq!(asset_url(7, "hero.jpg").as_deref(), Some("https://cdn.discordapp.com/assets/quests/7/hero.jpg"));
        assert_eq!(asset_url(7, "dark/Game_Tile-2.PNG").as_deref(), Some("https://cdn.discordapp.com/assets/quests/7/dark/Game_Tile-2.PNG"));
        for bad in ["", "hero.mp4", "../x.png", "a/../b.png", "/x.png", "x.png?y=1", "https://exemple.invalid/x.png", "a b.png", ".png", "a//b.png", "é.png"] {
            assert_eq!(asset_url(7, bad), None, "{bad}");
        }
    }

    /// Chaque forme connue d'une valeur d'image mène au CDN, la forme
    /// documentée d'abord, l'autre racine en repli.
    #[test]
    fn every_known_asset_form_resolves_to_the_cdn() {
        let cdn = "https://cdn.discordapp.com";
        // Nom de fichier (forme documentée).
        assert_eq!(asset_urls(7, "hero.jpg"), [format!("{cdn}/assets/quests/7/hero.jpg"), format!("{cdn}/quests/7/hero.jpg")]);
        // Chemin complet sous `quests/` : tel quel, jamais recollé derrière `/assets/quests/7/`.
        assert_eq!(asset_urls(7, "quests/7/1410358070831480904.png"), [format!("{cdn}/quests/7/1410358070831480904.png"), format!("{cdn}/assets/quests/7/1410358070831480904.png")]);
        assert_eq!(asset_urls(7, "assets/quests/7/hero.webp"), [format!("{cdn}/assets/quests/7/hero.webp"), format!("{cdn}/quests/7/hero.webp")]);
        // Adresse entière du CDN.
        assert_eq!(asset_urls(7, "https://cdn.discordapp.com/quests/7/a.png"), asset_urls(7, "quests/7/a.png"));
        // Vidéos, autres hôtes, paramètres : rien n'est demandé.
        for bad in ["hero.mp4", "quests/7/hero.webm", "https://cdn.discordapp.com/quests/7/a.png?size=64", "https://cdn.discordapp.com.exemple.invalid/a.png", "http://cdn.discordapp.com/a.png", "quests/../a.png"] {
            assert!(asset_urls(7, bad).is_empty(), "{bad}");
        }
        assert!(is_video("hero.MP4") && is_video("quests/7/a.webm") && !is_video("hero.jpg"));

        // Logotype : version du thème, version ordinaire, dossier du thème.
        let logo = Themed { base: Some("wordmark.png".into()), dark: Some("wordmark_dark.png".into()), light: None };
        assert_eq!(logo.urls(7, false), [format!("{cdn}/assets/quests/7/wordmark_dark.png"), format!("{cdn}/quests/7/wordmark_dark.png"), format!("{cdn}/assets/quests/7/wordmark.png"), format!("{cdn}/quests/7/wordmark.png")]);
        assert_eq!(logo.urls(7, true), [format!("{cdn}/assets/quests/7/wordmark.png"), format!("{cdn}/quests/7/wordmark.png"), format!("{cdn}/assets/quests/7/light/wordmark.png")]);
        assert!(Themed::default().urls(7, false).is_empty());

        // Vignette de la récompense : son image, sinon la vignette du jeu.
        let mut q = quest(7);
        q.tile.base = Some("quests/7/tile.png".into());
        assert_eq!(q.reward_urls(false)[0], format!("{cdn}/quests/7/tile.png"));
        q.rewards[0].asset = Some("orbes.png".into());
        assert_eq!(q.reward_urls(false), [format!("{cdn}/assets/quests/7/orbes.png"), format!("{cdn}/quests/7/orbes.png"), format!("{cdn}/quests/7/tile.png"), format!("{cdn}/assets/quests/7/tile.png")]);
        assert_eq!(q.hero_urls()[0], format!("{cdn}/assets/quests/7/hero.jpg"));
    }

    /// Tri et filtre s'appliquent à la liste reçue, sans rien demander.
    #[test]
    fn sort_and_filter_are_local() {
        let now = 1_000;
        let mk = |id: u64, starts: i64, expires: i64, reward: RewardKind, task: TaskKind| {
            let mut q = quest(id);
            (q.starts_ms, q.expires_ms) = (Some(starts), Some(expires));
            q.rewards[0].kind = reward;
            q.tasks = vec![Task { kind: task, key: "T".into(), target: 60, text: String::new(), apps: Vec::new(), video: None }];
            q
        };
        let mut list = vec![
            mk(1, 10, 9_000, RewardKind::Orbs, TaskKind::WatchVideo),
            mk(2, 30, 5_000, RewardKind::Collectible, TaskKind::PlayOnDesktop),
            mk(3, 20, 7_000, RewardKind::Code, TaskKind::Achievement),
            mk(4, 40, 8_000, RewardKind::Orbs, TaskKind::PlayOnXbox),
        ];
        list[2].status = Some(Status { enrolled_at: Some(1), ..Default::default() });
        list[3].status = Some(Status { enrolled_at: Some(1), completed_at: Some(2), ..Default::default() });
        let none = HashSet::new();
        let ids = |list: &[Quest], sort, filter, hidden: &HashSet<u64>| arrange(list, false, sort, filter, hidden, now).iter().map(|q| q.id).collect::<Vec<_>>();
        assert_eq!(ids(&list, Sort::Suggested, Filter::default(), &none), [4, 3, 1, 2], "à réclamer, acceptée, puis l'ordre de Discord");
        assert_eq!(ids(&list, Sort::Newest, Filter::default(), &none), [4, 2, 3, 1]);
        assert_eq!(ids(&list, Sort::EndingSoon, Filter::default(), &none), [2, 3, 4, 1]);
        assert_eq!(ids(&list, Sort::Newest, Filter { orbs: true, ..Default::default() }, &none), [4, 1]);
        assert_eq!(ids(&list, Sort::Newest, Filter { orbs: true, in_game: true, ..Default::default() }, &none), [4, 3, 1], "dans un groupe : l'un ou l'autre");
        assert_eq!(ids(&list, Sort::Newest, Filter { orbs: true, play: true, ..Default::default() }, &none), [4], "entre groupes : l'un et l'autre");
        assert_eq!(ids(&list, Sort::Newest, Filter { video: true, other: true, ..Default::default() }, &none), [3, 1]);
        assert_eq!(ids(&list, Sort::Suggested, Filter::default(), &HashSet::from([4, 1])), [3, 2], "quêtes masquées");
        // Réclamée : dans l'autre onglet ; échue sans être terminée : nulle part.
        list[3].status.as_mut().unwrap_or_else(|| panic!("état")).claimed_at = Some(3);
        list[0].expires_ms = Some(5);
        assert_eq!(ids(&list, Sort::Suggested, Filter::default(), &none), [3, 2]);
        assert_eq!(arrange(&list, true, Sort::Suggested, Filter::default(), &none, now).iter().map(|q| q.id).collect::<Vec<_>>(), [4]);
        let mut s = Store::new();
        s.quests.hide(4);
        assert!(s.quests.hidden.contains(&4));
        s.quests.unhide_all();
        assert!(s.quests.hidden.is_empty());
        assert!(s.take_commands().is_empty(), "masquer ne demande rien à Discord");
    }

    /// Chaque écriture suit un geste, une à la fois par quête, et seulement
    /// dans l'étape qui la permet.
    #[test]
    fn writes_are_gated_by_stage_and_one_at_a_time() {
        let mut s = Store::new();
        assert!(!s.quest_enroll(1), "quête inconnue");
        s.quests_open();
        s.quests_open();
        s.quests_refresh();
        assert_eq!(s.take_commands(), vec![Cmd::Quests(QuestCmd::List)], "une seule demande de liste");
        assert!(s.quests_loading());
        let mut done = quest(2);
        done.status = Some(Status { enrolled_at: Some(1), completed_at: Some(2), ..Default::default() });
        s.apply_quests(QuestEvent::List(Ok((vec![quest(1), done], None))));
        assert!(!s.quests_loading());
        assert_eq!(s.quests.claimable(), 1);
        s.quests_open();
        assert!(s.take_commands().is_empty(), "liste récente : pas de nouvelle demande");

        assert!(!s.quest_claim(1), "pas terminée : rien à réclamer");
        assert!(!s.quest_code(1));
        assert!(s.quest_enroll(1) && !s.quest_enroll(1));
        assert!(s.quests.busy(1));
        assert_eq!(s.take_commands(), vec![Cmd::Quests(QuestCmd::Enroll { id: 1 })]);
        s.apply_quests(QuestEvent::Enrolled { id: 1, result: Err(Refused::Captcha) });
        assert_eq!(s.quests.refused(1), Some(&Refused::Captcha));
        assert!(s.quest_enroll(1));
        assert_eq!(s.quests.refused(1), None, "nouvel essai : le refus précédent est effacé");
        s.apply_quests(QuestEvent::Enrolled { id: 1, result: Ok(Status { enrolled_at: Some(9), ..Default::default() }) });
        let now = s.now();
        assert_eq!(s.quests.quest(1).map(|q| q.stage(now)), Some(Stage::Accepted));
        assert!(!s.quest_enroll(1), "déjà acceptée");
        s.take_commands();

        assert!(!s.quest_enroll(2));
        assert!(s.quest_claim(2));
        assert_eq!(s.take_commands(), vec![Cmd::Quests(QuestCmd::Claim { id: 2 })]);
        s.apply_quests(QuestEvent::Claimed { id: 2, result: Ok(Some(77)) });
        assert_eq!(s.quests.quest(2).and_then(|q| q.status.as_ref()).and_then(|st| st.claimed_at), Some(77));
        assert_eq!(s.quests.claimable(), 0);
        assert!(!s.quest_code(2), "récompense en Orbes : pas de code");

        // La passerelle met à jour la progression et le solde ; un blocage empêche d'accepter.
        s.apply_quests(QuestEvent::Status { id: 1, status: Status { enrolled_at: Some(9), progress: vec![("PLAY_ON_DESKTOP".into(), 300)], ..Default::default() } });
        assert_eq!(s.quests.quest(1).and_then(Quest::progress), Some((300, 900)));
        s.apply_quests(QuestEvent::Orbs(1400));
        assert_eq!(s.quests.orbs, Some(1400));
        s.apply_quests(QuestEvent::List(Ok((vec![quest(3)], Some(i64::MAX)))));
        assert!(!s.quest_enroll(3), "acceptation bloquée par Discord");
        s.apply_quests(QuestEvent::BlockedUntil(None));
        assert!(s.quest_enroll(3));
        // Échec d'actualisation : la liste affichée reste.
        s.quests_refresh();
        s.apply_quests(QuestEvent::List(Err(Problem::Http(500))));
        assert!(s.quests.quest(3).is_some());
    }
}
