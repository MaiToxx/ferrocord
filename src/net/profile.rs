//! Profil complet et note d'un autre utilisateur (comptes utilisateur
//! seulement, non documenté par Discord).
//!
//! Formes vérifiées le 13 novembre 2026 sur docs.discord.food (« Users » :
//! Get User Profile, Get User Note, Modify User Note ; « Reference » : CDN
//! Formatting) ; jeux d'essai synthétiques `tests/fixtures/rest_user_profile.json`
//! et `rest_user_note.json`. Chaque champ est lu à part : une forme
//! inattendue ne fait perdre que ce champ.
//!
//! Rien ne part sans geste : ouverture d'une carte (profil, note), sortie du
//! champ de note (écriture). Tout passe par `rest::api` (limiteur commun),
//! sans en-tête propre au client officiel. Compte bot : ni profil ni note,
//! seulement les compléments documentés (`net::people`).

use super::people::{degrade, fetch_member, fetch_user, store_member, user_info};
use super::ratelimit::Route;
use super::raw::{de_lenient, de_loose_id, Item, RawGuildMember, RawUser};
use super::rest::{api, ApiError, Ctx};
use crate::state::model::ImageHash;
use crate::state::people::PeopleHub;
use crate::state::profiles::{clip_to, Connection, MutualGuild, NoteWrite, Profile, ProfileBadge, ProfileCmd, ProfileState, ProfileText, MAX_BIO, MAX_LIST, MAX_NOTE, MAX_SHORT};
use serde::Deserialize;
use serde_json::json;
use std::{sync::Arc, time::Duration};

/// Attente maximale avant de partir : au-delà, la carte s'en passe.
const MAX_WAIT: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
struct RawMeta {
    #[serde(default, deserialize_with = "de_lenient")]
    bio: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    pronouns: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    banner: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    accent_color: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    theme_colors: Option<Vec<u32>>,
    /// Effet de profil, sous toutes ses formes (voir [`effect_of`]).
    #[serde(default)]
    profile_effect: Option<serde_json::Value>,
    /// Forme ancienne (et celle de l'écriture) : l'identifiant seul.
    #[serde(default, deserialize_with = "de_loose_id")]
    profile_effect_id: Option<u64>,
}

#[derive(Deserialize)]
struct RawBadge {
    #[serde(default, deserialize_with = "de_lenient")]
    id: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    description: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    icon: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    link: Option<String>,
}

#[derive(Deserialize)]
struct RawMutualGuild {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    nick: Option<String>,
}

#[derive(Deserialize)]
struct RawConnection {
    #[serde(rename = "type", default, deserialize_with = "de_lenient")]
    kind: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    id: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    verified: Option<bool>,
}

/// Effet de profil porté, d'après `profile_effect` : (identifiant de l'effet,
/// SKU de son produit s'il est donné). Formes lues :
/// - `{ "id", "expires_at" }` (docs.discord.food, « Profile Effect Structure ») ;
/// - `{ "id", "sku_id", "expires_at" }` (comme `avatar_decoration_data`, qui
///   porte son `sku_id` : forme attendue si Discord l'ajoute, non confirmée) ;
/// - `{ "sku_id" }` seul : l'identifiant est alors celui du SKU ;
/// - l'identifiant seul, en chaîne ou en nombre (forme ancienne).
///
/// Un effet expiré (`expires_at` passé, en secondes Unix ou en date ISO)
/// n'est plus porté. `now_ms` : l'instant présent.
fn effect_of(v: &serde_json::Value, now_ms: i64) -> Option<(u64, Option<u64>)> {
    let id_of = |v: &serde_json::Value| v.as_u64().or_else(|| v.as_str()?.parse().ok()).filter(|id| *id != 0);
    let Some(o) = v.as_object() else { return Some((id_of(v)?, None)) };
    let expires_ms = o.get("expires_at").and_then(|e| match e {
        // Secondes Unix d'après la source ; une valeur trop grande est déjà en millisecondes.
        serde_json::Value::Number(n) => n.as_i64().map(|s| if s > 100_000_000_000 { s } else { s.saturating_mul(1000) }),
        serde_json::Value::String(s) => chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp_millis()),
        _ => None,
    });
    if expires_ms.is_some_and(|at| at <= now_ms) {
        return None;
    }
    let sku = o.get("sku_id").and_then(id_of);
    Some((o.get("id").and_then(id_of).or(sku)?, sku))
}

/// Réponse de Get User Profile (seuls les champs affichés).
#[derive(Deserialize)]
struct RawProfile {
    #[serde(default, deserialize_with = "de_lenient")]
    user: Option<RawUser>,
    #[serde(default, deserialize_with = "de_lenient")]
    user_profile: Option<RawMeta>,
    #[serde(default, deserialize_with = "de_lenient")]
    guild_member_profile: Option<RawMeta>,
    #[serde(default, deserialize_with = "de_lenient")]
    guild_member: Option<RawGuildMember>,
    #[serde(default, deserialize_with = "de_lenient")]
    badges: Option<Vec<Item<RawBadge>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    guild_badges: Option<Vec<Item<RawBadge>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    mutual_guilds: Option<Vec<Item<RawMutualGuild>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    mutual_friends: Option<Vec<Item<RawUser>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    mutual_friends_count: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    connected_accounts: Option<Vec<Item<RawConnection>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    premium_type: Option<u8>,
    #[serde(default, deserialize_with = "de_lenient")]
    premium_since: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    premium_guild_since: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    legacy_username: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    widgets: Option<Vec<serde_json::Value>>,
}

fn text(s: &Option<String>, max: usize) -> Option<String> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(|s| clip_to(s, max))
}

fn iso_ms(s: &Option<String>) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s.as_deref()?).ok().map(|d| d.timestamp_millis())
}

fn color(c: u32) -> Option<u32> {
    (c <= 0xFF_FFFF).then_some(c)
}

impl RawMeta {
    fn to_text(&self) -> ProfileText {
        let theme = match self.theme_colors.as_deref() {
            Some([a, b]) => color(*a).zip(color(*b)).map(|(a, b)| [a, b]),
            _ => None,
        };
        let effect = match &self.profile_effect {
            Some(v) => effect_of(v, crate::state::store::now_ms()),
            None => self.profile_effect_id.filter(|id| *id != 0).map(|id| (id, None)),
        };
        ProfileText {
            bio: text(&self.bio, MAX_BIO),
            pronouns: text(&self.pronouns, MAX_SHORT),
            banner: self.banner.as_deref().and_then(ImageHash::parse),
            accent: self.accent_color.and_then(color),
            theme,
            effect: effect.map(|(id, _)| id),
            effect_sku: effect.and_then(|(_, sku)| sku),
        }
    }
}

impl RawBadge {
    fn to_badge(&self) -> Option<ProfileBadge> {
        // Un badge sans description reste affiché s'il a son icône.
        let icon = self.icon.as_deref().and_then(ImageHash::parse);
        let description = text(&self.description, MAX_SHORT);
        if description.is_none() && icon.is_none() {
            return None;
        }
        Some(ProfileBadge {
            id: text(&self.id, MAX_SHORT)?,
            description: description.unwrap_or_default(),
            icon,
            // Seuls les liens web sûrs sont gardés.
            link: self.link.as_deref().filter(|l| l.starts_with("https://") && l.len() <= 512).map(str::to_string),
        })
    }
}

impl RawConnection {
    fn to_connection(&self) -> Option<Connection> {
        Some(Connection {
            kind: text(&self.kind, MAX_SHORT)?,
            id: text(&self.id, MAX_SHORT).unwrap_or_default(),
            name: text(&self.name, MAX_SHORT)?,
            verified: self.verified.unwrap_or(false),
        })
    }
}

/// Longueur gardée de la forme des badges.
const MAX_SHAPE: usize = 4000;

/// Forme des listes `badges` et `guild_badges` de la réponse : noms des
/// champs et types des valeurs ; seule la valeur de `id` (référence du
/// badge, pas une donnée du compte) est gardée.
fn badge_shape(body: &str) -> String {
    fn shape(v: &serde_json::Value, key: &str, depth: u8) -> String {
        use serde_json::Value as V;
        match v {
            V::Null => "nul".into(),
            V::Bool(_) => "booléen".into(),
            V::Number(_) => "nombre".into(),
            V::String(s) if key == "id" && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') => format!("{s:?}"),
            V::String(s) => format!("texte({})", s.chars().count()),
            _ if depth >= 4 => "…".into(),
            V::Array(a) => format!("[{}]", a.iter().take(40).map(|x| shape(x, "", depth + 1)).collect::<Vec<_>>().join(", ")),
            V::Object(o) => format!("{{{}}}", o.iter().take(40).map(|(k, x)| format!("{}: {}", k.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(40).collect::<String>(), shape(x, k, depth + 1))).collect::<Vec<_>>().join(", ")),
        }
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else { return String::new() };
    let mut out = String::new();
    for key in ["badges", "guild_badges"] {
        if let Some(list) = v.get(key).filter(|l| l.as_array().is_some_and(|a| !a.is_empty())) {
            out.push_str(&format!("{key} = {} ", shape(list, "", 0)));
        }
    }
    out.chars().take(MAX_SHAPE).collect()
}

fn items<R, T>(list: &Option<Vec<Item<R>>>, f: impl Fn(&R) -> Option<T>) -> Vec<T> {
    list.iter().flatten().filter_map(|x| x.0.as_ref().and_then(&f)).take(MAX_LIST).collect()
}

/// Décode la réponse et l'écrit dans `hub` (profil, utilisateur, membre du
/// serveur, amis en commun). `false` : réponse illisible ou d'un autre utilisateur.
pub(super) fn store(hub: &PeopleHub, user_id: u64, guild_id: Option<u64>, body: &str) -> bool {
    let Ok(raw) = serde_json::from_str::<RawProfile>(body) else { return false };
    if raw.user.as_ref().is_some_and(|u| u.id != user_id) {
        return false;
    }
    let mut badges: Vec<ProfileBadge> = Vec::new();
    for b in items(&raw.badges, RawBadge::to_badge).into_iter().chain(items(&raw.guild_badges, RawBadge::to_badge)) {
        // Doublon du serveur : même identifiant et même icône (deux badges
        // distincts peuvent partager un identifiant).
        if badges.len() < MAX_LIST && !badges.iter().any(|x| x.id == b.id && x.icon == b.icon) {
            badges.push(b);
        }
    }
    let mutual_friends: Option<Vec<UserLine>> = raw.mutual_friends.as_ref().map(|_| items(&raw.mutual_friends, |u: &RawUser| Some((u.id, user_info(u)))));
    let widgets = super::widgets::read_widgets(raw.widgets.as_deref().unwrap_or_default());
    let profile = Profile {
        user: raw.user_profile.as_ref().map(RawMeta::to_text).unwrap_or_default(),
        guild: guild_id.and(raw.guild_member_profile.as_ref()).map(RawMeta::to_text),
        badges,
        badge_shape: badge_shape(body),
        premium_type: raw.premium_type,
        premium_since: iso_ms(&raw.premium_since),
        premium_guild_since: iso_ms(&raw.premium_guild_since),
        connections: items(&raw.connected_accounts, RawConnection::to_connection),
        mutual_guilds: raw.mutual_guilds.as_ref().map(|_| items(&raw.mutual_guilds, |g: &RawMutualGuild| Some(MutualGuild { id: g.id?, nick: text(&g.nick, MAX_SHORT) }))),
        mutual_friends: mutual_friends.as_ref().map(|l| l.iter().map(|(id, _)| *id).collect()),
        mutual_friends_count: raw.mutual_friends_count,
        legacy_username: text(&raw.legacy_username, MAX_SHORT),
        widgets: widgets.0,
        widgets_opaque: widgets.1,
    };
    let mut p = hub.lock();
    if let Some(u) = raw.user.as_ref().and_then(user_info) {
        p.upsert_user(u);
    }
    for u in mutual_friends.into_iter().flatten().filter_map(|(_, u)| u) {
        p.upsert_user(u);
    }
    if let (Some(g), Some(m)) = (guild_id, &raw.guild_member)
        && m.user_id() == Some(user_id)
    {
        store_member(&mut p, g, m);
    }
    p.profiles_mut().set(user_id, guild_id, ProfileState::Ready(Box::new(profile)));
    true
}

type UserLine = (u64, Option<crate::state::people::UserInfo>);

/// Get User Note : `note` (256 caractères au plus) ; `None` si illisible.
pub(super) fn note_of(body: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct RawNote {
        #[serde(default, deserialize_with = "de_lenient")]
        note: Option<String>,
    }
    let n: RawNote = serde_json::from_str(body).ok()?;
    Some(n.note.map(|s| clip_to(&s, MAX_NOTE)).unwrap_or_default())
}

/// USER_NOTE_UPDATE (`id` : la personne, `note` : le texte, vide si effacée) :
/// note écrite ici ou sur un autre appareil. Sans cela, une note lue une fois
/// restait celle de la première lecture jusqu'à la fin de la session.
pub(super) fn note_update(d: &str, out: &super::Out) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(d) else { return };
    let Some(user_id) = v["id"].as_str().and_then(|s| s.parse::<u64>().ok()).or_else(|| v["id"].as_u64()) else { return };
    let text = clip_to(v["note"].as_str().unwrap_or_default(), MAX_NOTE);
    out.people.lock().profiles_mut().note_loaded(user_id, Some(text));
    out.poke();
}

/// Adresse de Get User Profile : serveurs et amis en commun demandés, et le
/// profil du serveur affiché.
fn profile_url(root: &str, user_id: u64, guild_id: Option<u64>) -> String {
    let mut url = format!("{root}/users/{user_id}/profile?with_mutual_guilds=true&with_mutual_friends=true&with_mutual_friends_count=true");
    if let Some(g) = guild_id {
        url.push_str(&format!("&guild_id={g}"));
    }
    url
}

const fn route(template: &'static str) -> Route {
    Route { template, major: 0 }
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: ProfileCmd) {
    match cmd {
        ProfileCmd::Fetch { user_id, guild_id } => fetch(ctx, user_id, guild_id).await,
        ProfileCmd::Note { user_id } => read_note(&ctx, user_id).await,
        ProfileCmd::SetNote { user_id, note } => write_note(&ctx, user_id, note).await,
    }
}

async fn fallback(ctx: Arc<Ctx>, user_id: u64, guild_id: Option<u64>) {
    fetch_user(ctx.clone(), user_id).await;
    if let Some(g) = guild_id {
        fetch_member(ctx, g, user_id).await;
    }
}

async fn fetch(ctx: Arc<Ctx>, user_id: u64, guild_id: Option<u64>) {
    let set = |state| {
        ctx.out.people.lock().profiles_mut().set(user_id, guild_id, state);
        ctx.out.poke();
    };
    if ctx.is_bot() {
        set(ProfileState::Unavailable);
        return fallback(ctx.clone(), user_id, guild_id).await;
    }
    let url = profile_url(&ctx.api_root, user_id, guild_id);
    match api(&ctx, route("GET /users/{}/profile"), reqwest::Method::GET, &url, None, MAX_WAIT, std::future::pending()).await {
        Ok(body) => {
            if store(&ctx.out.people, user_id, guild_id, &body) {
                ctx.out.poke();
            } else {
                set(ProfileState::Unavailable);
            }
        }
        Err(ApiError::Captcha) => set(ProfileState::Captcha),
        Err(e @ ApiError::Unauthorized) => {
            set(ProfileState::Unavailable);
            degrade(&ctx, e);
        }
        // Profil privé, utilisateur introuvable, erreur : sans message, avec
        // ce que les points documentés donnent encore (403 sans serveur commun…).
        Err(_) => {
            set(ProfileState::Unavailable);
            fallback(ctx.clone(), user_id, guild_id).await;
        }
    }
}

async fn read_note(ctx: &Ctx, user_id: u64) {
    let loaded = |text| {
        ctx.out.people.lock().profiles_mut().note_loaded(user_id, text);
        ctx.out.poke();
    };
    if ctx.is_bot() {
        return loaded(None);
    }
    let url = format!("{}/users/@me/notes/{user_id}", ctx.api_root);
    match api(ctx, route("GET /users/@me/notes/{}"), reqwest::Method::GET, &url, None, MAX_WAIT, std::future::pending()).await {
        Ok(body) => loaded(note_of(&body)),
        // Pas de note sur cet utilisateur.
        Err(ApiError::NotFound(_)) => loaded(Some(String::new())),
        Err(e @ ApiError::Unauthorized) => {
            loaded(None);
            degrade(ctx, e);
        }
        Err(_) => loaded(None),
    }
}

async fn write_note(ctx: &Ctx, user_id: u64, note: String) {
    let url = format!("{}/users/@me/notes/{user_id}", ctx.api_root);
    let body = json!({ "note": note });
    let result = match api(ctx, route("PUT /users/@me/notes/{}"), reqwest::Method::PUT, &url, Some(body), MAX_WAIT, std::future::pending()).await {
        Ok(_) => NoteWrite::Done,
        Err(ApiError::Captcha) => NoteWrite::Captcha,
        Err(e @ ApiError::Unauthorized) => {
            degrade(ctx, e);
            NoteWrite::Failed
        }
        Err(_) => NoteWrite::Failed,
    };
    ctx.out.people.lock().profiles_mut().note_written(user_id, result);
    ctx.out.poke();
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALICE: u64 = 1000000000000000002;
    const ALPHA: u64 = 2000000000000000001;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"))
    }

    #[test]
    fn profile_fixture_decodes_field_by_field() {
        let hub = PeopleHub::default();
        assert!(store(&hub, ALICE, Some(ALPHA), &fixture("rest_user_profile")));
        let p = hub.lock();
        let prof = p.profiles().profile(ALICE, Some(ALPHA)).and_then(ProfileState::profile).cloned().unwrap_or_else(|| panic!("profil attendu"));
        assert_eq!(prof.user.pronouns.as_deref(), Some("elle"));
        assert_eq!(prof.user.theme, Some([5992431, 2829099]));
        assert_eq!(prof.user.accent, Some(6000111));
        assert!(prof.user.banner.is_some());
        // Profil de serveur : sa bio passe devant, ses pronoms vides sont absents.
        assert_eq!(prof.bio(), Some("Bio de serveur fictive."));
        assert_eq!(prof.pronouns(), Some("elle"));
        // Badges : illisibles écartés, lien non web retiré, doublon du serveur ignoré.
        let ids: Vec<&str> = prof.badges.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, vec!["premium", "hypesquad_house_2", "active_developer"]);
        assert!(prof.badges[0].icon.is_some() && prof.badges[0].link.is_some());
        // Forme pour le journal : champs et types, l'identifiant du badge, aucun texte.
        assert!(prof.badge_shape.starts_with(r#"badges = [{description: texte(29), icon: texte(32), id: "premium", link: texte(38)}"#), "{}", prof.badge_shape);
        assert!(!prof.badge_shape.contains("Abonné") && !prof.badge_shape.contains("exemple.invalid"));
        assert_eq!((prof.badges[1].link.as_deref(), prof.badges[2].icon), (None, None));
        assert_eq!(prof.premium_type, Some(2));
        assert_eq!(prof.premium_since, Some(1_706_781_600_000));
        assert_eq!(prof.premium_guild_since, Some(1_717_243_200_000));
        assert_eq!(prof.legacy_username.as_deref(), Some("alice#1234"));
        assert_eq!(prof.mutual_guilds, Some(vec![MutualGuild { id: ALPHA, nick: Some("Ali".into()) }, MutualGuild { id: 2000000000000000003, nick: None }]));
        assert_eq!(prof.mutual_friends, Some(vec![1000000000000000003, 1000000000000000004]));
        assert_eq!(prof.friends_count(), Some(2));
        let kinds: Vec<(&str, bool)> = prof.connections.iter().map(|c| (c.kind.as_str(), c.verified)).collect();
        assert_eq!(kinds, vec![("github", true), ("steam", false), ("battlenet", true)]);
        // Utilisateur, membre et amis en commun versés dans les tables.
        assert_eq!(p.user(1000000000000000003).map(|u| u.display_name()), Some("Bob"));
        assert_eq!(p.member(ALPHA, ALICE).and_then(|m| m.nick.as_deref()), Some("Ali"));
        assert_eq!(p.user(ALICE).and_then(|u| u.flags), Some(4194432));
    }

    /// Chaque forme connue ou attendue de `profile_effect` (jeu d'essai
    /// `profil/profile_effect_forms.json` : une métadonnée de profil par forme).
    #[test]
    fn every_shape_of_the_profile_effect_is_read() {
        let forms: serde_json::Value = serde_json::from_str(&fixture("profil/profile_effect_forms")).unwrap_or_else(|e| panic!("{e}"));
        let read = |name: &str| {
            let meta: RawMeta = serde_json::from_value(forms[name].clone()).unwrap_or_else(|e| panic!("{name} : {e}"));
            let t = meta.to_text();
            (t.effect, t.effect_sku)
        };
        const ID: u64 = 6000000000000000301;
        const SKU: u64 = 6000000000000000302;
        assert_eq!(read("documentee"), (Some(ID), None), "{{ id, expires_at }}");
        assert_eq!(read("avec_sku"), (Some(ID), Some(SKU)), "{{ id, sku_id, expires_at }}");
        assert_eq!(read("sku_seul"), (Some(SKU), Some(SKU)));
        assert_eq!(read("identifiant_nu"), (Some(ID), None));
        assert_eq!(read("identifiant_nombre"), (Some(301), None));
        assert_eq!(read("ancien_champ"), (Some(ID), None), "profile_effect_id");
        assert_eq!(read("expire_plus_tard"), (Some(ID), None));
        assert_eq!(read("expire"), (None, None), "expiré : plus porté");
        assert_eq!(read("expire_iso"), (None, None));
        assert_eq!(read("nul"), (None, None));
        assert_eq!(read("illisible"), (None, None));
        assert_eq!(read("absent"), (None, None));
        // Le champ nouveau l'emporte sur l'ancien.
        assert_eq!(read("les_deux"), (Some(ID), Some(SKU)));
    }

    #[test]
    fn odd_or_foreign_profiles_are_refused_or_partial() {
        let hub = PeopleHub::default();
        assert!(!store(&hub, ALICE, None, "pas du json"));
        assert!(!store(&hub, 7, None, &fixture("rest_user_profile")), "profil d'un autre");
        // Profil privé ou d'un utilisateur qui m'a bloqué : presque vide, sans échec.
        assert!(store(&hub, ALICE, None, r#"{ "user": { "id": "1000000000000000002", "username": "a" }, "badges": null, "mutual_guilds": "x", "connected_accounts": [], "premium_since": 5 }"#));
        let p = hub.lock();
        let prof = p.profiles().profile(ALICE, None).and_then(ProfileState::profile).cloned().unwrap_or_else(|| panic!("profil"));
        assert_eq!(prof, Profile::default());
        assert!(prof.guild.is_none());
    }

    #[test]
    fn note_and_request_shapes() {
        assert_eq!(note_of(&fixture("rest_user_note")).as_deref(), Some("Rencontrée au club fictif."));
        assert_eq!(note_of(r#"{ "note": null }"#).as_deref(), Some(""));
        assert_eq!(note_of(&format!(r#"{{ "note": "{}" }}"#, "x".repeat(400))).map(|n| n.len()), Some(MAX_NOTE));
        assert_eq!(note_of("5"), None);
        assert_eq!(
            profile_url("https://api", 5, Some(9)),
            "https://api/users/5/profile?with_mutual_guilds=true&with_mutual_friends=true&with_mutual_friends_count=true&guild_id=9"
        );
        assert!(!profile_url("https://api", 5, None).contains("guild_id"));
    }
}
