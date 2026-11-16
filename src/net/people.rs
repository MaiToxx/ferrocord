//! Utilisateurs, membres et présences : décodage des formes documentées
//! (objet `user`, objet `member`, Presence Update, Activity) vers
//! `state::people`, et compléments demandés par la carte de profil.
//!
//! Chaque champ est lu à part : une forme inattendue ne fait perdre que ce
//! champ (ou la présence entière si son `status` est illisible), jamais le
//! paquet qui la contient (READY, GUILD_CREATE).

use super::raw::{de_lenient, de_loose_id, Item, RawChannel, RawGuild, RawGuildMember, RawMember, RawMessage, RawReady, RawUser};
use super::ratelimit::Route;
use super::rest::{api, ApiError, Ctx};
use super::Out;
use crate::state::model::ImageHash;
use crate::state::people::{clip, Activity, ActivityEmoji, ActivityImage, ActivityKind, ClientStatus, Rich, MemberInfo, People, PeopleHub, Presence, Status, UserInfo, MAX_ACTIVITIES};
use crate::state::{DmKnown, Event};
use serde::Deserialize;
use serde_json::json;
use std::{collections::HashMap, sync::Arc, time::Duration};

/// Attente maximale d'un complément de profil avant de partir : au-delà, la
/// carte reste sans lui.
const FETCH_MAX_WAIT: Duration = Duration::from_secs(10);
/// Attente maximale de la création d'un MP.
const OPEN_DM_MAX_WAIT: Duration = Duration::from_secs(15);

/// Presence Update (documenté) ; aussi les éléments de `presences` de
/// GUILD_CREATE et du READY.
#[derive(Deserialize)]
pub(super) struct RawPresence {
    #[serde(default, deserialize_with = "de_lenient")]
    user: Option<RawUser>,
    /// À la place de `user` dans `merged_presences` (READY_SUPPLEMENTAL).
    #[serde(default, deserialize_with = "de_loose_id")]
    user_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    status: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    client_status: Option<RawClientStatus>,
    #[serde(default, deserialize_with = "de_lenient")]
    activities: Option<Vec<Item<RawActivity>>>,
}

#[derive(Deserialize)]
struct RawClientStatus {
    #[serde(default, deserialize_with = "de_lenient")]
    desktop: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    mobile: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    web: Option<String>,
}

#[derive(Deserialize)]
struct RawActivity {
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(rename = "type", default, deserialize_with = "de_lenient")]
    kind: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    details: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    state: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    emoji: Option<RawEmoji>,
    #[serde(default, deserialize_with = "de_lenient")]
    timestamps: Option<RawTimestamps>,
    // Présence enrichie (lot C3).
    #[serde(default, deserialize_with = "de_loose_id")]
    application_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    session_id: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    sync_id: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    url: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    assets: Option<RawAssets>,
    #[serde(default, deserialize_with = "de_lenient")]
    buttons: Option<Vec<Item<String>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    party: Option<RawParty>,
}

#[derive(Deserialize)]
struct RawAssets {
    #[serde(default, deserialize_with = "de_lenient")]
    large_image: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    large_text: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    small_image: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    small_text: Option<String>,
}

#[derive(Deserialize)]
struct RawParty {
    #[serde(default, deserialize_with = "de_lenient")]
    id: Option<String>,
    /// `[présents, places]`.
    #[serde(default, deserialize_with = "de_lenient")]
    size: Option<Vec<u32>>,
}

#[derive(Deserialize)]
struct RawEmoji {
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    animated: Option<bool>,
}

#[derive(Deserialize)]
struct RawTimestamps {
    /// Nombre ou chaîne (docs.discord.food : « string », en ms Unix).
    #[serde(default, deserialize_with = "de_loose_id")]
    start: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    end: Option<u64>,
}

fn text(s: &Option<String>) -> Option<String> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(clip)
}

fn ms(v: Option<u64>) -> Option<i64> {
    v.and_then(|v| i64::try_from(v).ok()).filter(|ms| *ms > 0)
}

/// Adresse d'une image d'activité (docs.discord.food, « Presence » ›
/// Activity Asset Image) : image de l'application (`app-assets`) ou du
/// proxy média (`mp:`) ; les images d'autres sites (`spotify:`, `twitch:`,
/// `youtube:`) ne sont pas chargées : seul le CDN de Discord est contacté.
pub(super) fn asset_url(app_id: Option<u64>, key: &str) -> Option<String> {
    if let Some(path) = key.strip_prefix("mp:") {
        let ok = !path.is_empty()
            && path.len() <= 300
            && !path.starts_with('/')
            && !path.contains("..")
            && path.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~/%:".contains(&b));
        return ok.then(|| format!("https://media.discordapp.net/{path}"));
    }
    let digits = !key.is_empty() && key.len() <= 20 && key.bytes().all(|b| b.is_ascii_digit());
    (digits && app_id.is_some()).then(|| format!("https://cdn.discordapp.com/app-assets/{}/{key}.png", app_id.unwrap_or_default()))
}

impl RawActivity {
    /// Champs enrichis ; `None` s'il n'y en a aucun.
    fn rich(&self) -> Option<Box<Rich>> {
        let image = |key: &Option<String>, label: &Option<String>| {
            let url = key.as_deref().and_then(|k| asset_url(self.application_id, k.trim()));
            let text = text(label);
            (url.is_some() || text.is_some()).then_some(ActivityImage { url, text })
        };
        let a = self.assets.as_ref();
        let spotify = self.party.as_ref().and_then(|p| p.id.as_deref()).is_some_and(|id| id.starts_with("spotify:"))
            || a.and_then(|a| a.large_image.as_deref()).is_some_and(|k| k.starts_with("spotify:"));
        let word = |s: &Option<String>| s.as_deref().filter(|s| !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric())).map(str::to_string);
        let url = self.url.as_deref().filter(|u| (u.starts_with("https://") || u.starts_with("http://")) && u.len() <= 512 && !u.chars().any(char::is_control)).map(str::to_string);
        let r = Rich {
            app_id: self.application_id,
            end_ms: ms(self.timestamps.as_ref().and_then(|t| t.end)),
            large: a.and_then(|a| image(&a.large_image, &a.large_text)),
            small: a.and_then(|a| image(&a.small_image, &a.small_text)),
            buttons: self.buttons.iter().flatten().filter_map(|b| text(&b.0)).take(2).collect(),
            session_id: word(&self.session_id),
            sync_id: word(&self.sync_id),
            spotify,
            url,
            party: self.party.as_ref().and_then(|p| p.size.as_deref()).and_then(|s| match s {
                [now, max] if *now > 0 && now <= max => Some((*now, *max)),
                _ => None,
            }),
        };
        (r != Rich::default()).then(|| Box::new(r))
    }
}

impl RawActivity {
    fn to_activity(&self) -> Option<Activity> {
        let kind = ActivityKind::from_code(self.kind?)?;
        let emoji = self.emoji.as_ref().map(|e| ActivityEmoji { name: text(&e.name), id: e.id, animated: e.id.is_some() && e.animated == Some(true) }).filter(|e| e.name.is_some() || e.id.is_some());
        let a = Activity {
            kind,
            name: text(&self.name).unwrap_or_default(),
            details: text(&self.details),
            state: text(&self.state),
            emoji,
            start_ms: ms(self.timestamps.as_ref().and_then(|t| t.start)),
            rich: self.rich(),
        };
        // Une activité sans rien à afficher n'est pas gardée.
        (!a.name.is_empty() || a.state.is_some() || a.emoji.is_some()).then_some(a)
    }
}

impl RawPresence {
    /// (utilisateur, présence) ; `None` si l'identifiant ou le statut est illisible.
    fn to_presence(&self) -> Option<(u64, Presence)> {
        let id = self.user.as_ref().map(|u| u.id).or(self.user_id)?;
        let status = Status::parse(self.status.as_deref()?)?;
        let platform = |s: &Option<String>| s.as_deref().and_then(Status::parse);
        let client = self.client_status.as_ref().map(|c| ClientStatus { desktop: platform(&c.desktop), mobile: platform(&c.mobile), web: platform(&c.web) }).unwrap_or_default();
        let activities = self.activities.iter().flatten().filter_map(|a| a.0.as_ref()?.to_activity()).take(MAX_ACTIVITIES).collect();
        Some((id, Presence { status, client, activities }))
    }

    /// Écrit la présence (et l'utilisateur s'il est complet) ; vrai s'il faut
    /// redessiner (utilisateur affiché qui change).
    fn store(&self, p: &mut People) -> bool {
        let mut redraw = false;
        if let Some(u) = self.user.as_ref().and_then(user_info) {
            redraw |= p.upsert_user(u);
        }
        if let Some((id, presence)) = self.to_presence() {
            redraw |= p.set_presence(id, presence);
        }
        redraw
    }
}

/// Horodatage ISO 8601 de l'API (ms Unix).
fn iso_ms(s: &Option<String>) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s.as_deref()?).ok().map(|d| d.timestamp_millis())
}

fn hash(s: &Option<String>) -> Option<ImageHash> {
    s.as_deref().and_then(ImageHash::parse)
}

/// Objet `user` complet ; `None` sans nom d'utilisateur (objet partiel, comme
/// celui d'une présence qui ne porte que l'identifiant).
pub(super) fn user_info(u: &RawUser) -> Option<UserInfo> {
    if u.username().is_empty() {
        return None;
    }
    Some(UserInfo {
        id: u.id,
        username: clip(u.username()),
        global_name: u.global_name().filter(|n| !n.is_empty()).map(clip),
        discriminator: u.discriminator().map(str::to_string),
        avatar: u.avatar(),
        banner: u.banner,
        accent: u.accent_color,
        flags: u.public_flags,
        bot: u.bot.unwrap_or(false),
        system: u.system.unwrap_or(false),
        look: u.look(),
    })
}

fn member_info(m: &RawGuildMember) -> MemberInfo {
    MemberInfo {
        nick: m.nick().filter(|n| !n.is_empty()).map(clip),
        roles: m.role_ids(),
        joined_at: iso_ms(&m.joined_at),
        avatar: hash(&m.avatar),
        premium_since: iso_ms(&m.premium_since),
        timeout_until: m.timeout_ms(),
        banner: hash(&m.banner),
        decoration: m.avatar_decoration_data.as_ref().and_then(super::cosmetics::RawDecoration::asset),
    }
}

fn message_member(m: &RawMember) -> MemberInfo {
    MemberInfo {
        nick: m.nick.as_deref().filter(|n| !n.is_empty()).map(clip),
        roles: m.roles.as_ref().and_then(|r| r.iter().map(|id| id.parse().ok()).collect()),
        joined_at: iso_ms(&m.joined_at),
        avatar: hash(&m.avatar),
        premium_since: iso_ms(&m.premium_since),
        timeout_until: iso_ms(&m.communication_disabled_until),
        banner: hash(&m.banner),
        decoration: m.avatar_decoration_data.as_ref().and_then(super::cosmetics::RawDecoration::asset),
    }
}

/// Membre complet (avec `user`) d'un serveur ; vrai s'il faut redessiner.
pub(super) fn store_member(p: &mut People, guild_id: u64, m: &RawGuildMember) -> bool {
    let Some(user_id) = m.user_id() else { return false };
    let mut redraw = false;
    if let Some(u) = m.user().and_then(user_info) {
        redraw |= p.upsert_user(u);
    }
    redraw | p.upsert_member(guild_id, user_id, member_info(m))
}

fn store_presences(p: &mut People, list: &Option<Vec<Item<RawPresence>>>) {
    for pr in list.iter().flatten().filter_map(|x| x.0.as_ref()) {
        pr.store(p);
    }
}

fn store_dm_users(p: &mut People, c: &RawChannel) {
    for u in c.recipients.iter().filter_map(user_info) {
        p.upsert_user(u);
    }
    if let Some((user_id, channel_id)) = c.dm_peer() {
        p.note_dm(user_id, channel_id);
    }
}

/// READY : moi, les utilisateurs cités, les correspondants des MP, les
/// membres et présences des serveurs, et les présences du READY. Le READY
/// lui-même réveille l'interface.
pub(super) fn ready(r: &RawReady, hub: &PeopleHub) {
    let mut p = hub.lock();
    for u in std::iter::once(&r.user).chain(&r.users).filter_map(user_info) {
        p.upsert_user(u);
    }
    for c in &r.private_channels {
        store_dm_users(&mut p, c);
    }
    for g in &r.guilds {
        store_guild(&mut p, g);
    }
    store_presences(&mut p, &r.presences);
}

fn store_guild(p: &mut People, g: &RawGuild) {
    for m in g.members.iter().flatten().filter_map(|m| m.0.as_ref()) {
        store_member(p, g.id, m);
    }
    store_presences(p, &g.presences);
}

/// GUILD_CREATE (l'événement réveille l'interface).
pub(super) fn guild(g: &RawGuild, hub: &PeopleHub) {
    store_guild(&mut hub.lock(), g);
}

/// Auteur (objet `user` complet) et son membre (`member`, sans `user`).
fn store_message(p: &mut People, m: &RawMessage) {
    if let Some(u) = user_info(&m.author) {
        p.upsert_user(u);
    }
    if let (Some(guild_id), Some(member)) = (m.guild_id, &m.member) {
        p.upsert_member(guild_id, m.author.id, message_member(member));
    }
}

/// MESSAGE_CREATE (l'événement réveille l'interface).
pub(super) fn message(m: &RawMessage, hub: &PeopleHub) {
    store_message(&mut hub.lock(), m);
}

/// Page d'historique REST.
pub(super) fn messages(list: &[RawMessage], hub: &PeopleHub) {
    let mut p = hub.lock();
    for m in list {
        store_message(&mut p, m);
    }
}

/// GUILD_MEMBER_UPDATE de n'importe qui : réveille l'interface seulement si
/// l'utilisateur est affiché et que quelque chose a changé.
pub(super) fn member_update(m: &RawGuildMember, out: &Out) {
    let Some(guild_id) = m.guild_id else { return };
    if store_member(&mut out.people.lock(), guild_id, m) {
        out.poke();
    }
}

/// Présence d'un élément de GUILD_MEMBER_LIST_UPDATE (objet `presence` du
/// membre, dont `user` peut ne porter que l'identifiant, ou manquer).
pub(super) fn list_presence(raw: &str, user_id: u64) -> Option<Presence> {
    let mut p = serde_json::from_str::<RawPresence>(raw).ok()?;
    p.user = None;
    p.user_id = Some(user_id);
    p.to_presence().map(|(_, presence)| presence)
}

/// PRESENCE_UPDATE : rien ne part dans la file des événements ; réveil
/// seulement si un utilisateur affiché change.
pub(super) fn presence_update(d: &str, out: &Out) {
    let Ok(p) = serde_json::from_str::<RawPresence>(d) else { return };
    if p.store(&mut out.people.lock()) {
        out.poke();
    }
}

/// READY_SUPPLEMENTAL (non documenté) : présences des amis
/// (`merged_presences.friends`, un `user_id` au lieu de `user`), dans le même
/// magasin que les autres présences.
pub(super) fn supplemental(d: &str, out: &Out) {
    #[derive(Deserialize)]
    struct Merged {
        #[serde(default, deserialize_with = "de_lenient")]
        friends: Option<Vec<Item<RawPresence>>>,
    }
    #[derive(Deserialize)]
    struct Supplemental {
        #[serde(default, deserialize_with = "de_lenient")]
        merged_presences: Option<Merged>,
    }
    let Ok(s) = serde_json::from_str::<Supplemental>(d) else { return };
    let friends = s.merged_presences.and_then(|m| m.friends).unwrap_or_default();
    let mut redraw = false;
    {
        let mut p = out.people.lock();
        for f in friends.iter().filter_map(|f| f.0.as_ref()) {
            redraw |= f.store(&mut p);
        }
    }
    if redraw {
        out.poke();
    }
}

// ------------------------------------------------------------ REST

/// Erreur d'un complément : un jeton refusé ramène à l'écran de connexion
/// comme partout ; le reste est ignoré (la carte reste sans le complément).
pub(super) fn degrade(ctx: &Ctx, e: ApiError) {
    if let ApiError::Unauthorized = e {
        ctx.out.send(e.into_event(Event::Error));
    }
}

/// Get User (`GET /users/{user.id}`, documenté) : bannière, couleur d'accent, badges.
pub(super) async fn fetch_user(ctx: Arc<Ctx>, user_id: u64) {
    let url = format!("{}/users/{user_id}", ctx.api_root);
    match api(&ctx, Route::user_get(), reqwest::Method::GET, &url, None, FETCH_MAX_WAIT, std::future::pending()).await {
        Ok(body) => {
            let Some(u) = serde_json::from_str::<RawUser>(&body).ok().filter(|u| u.id == user_id).as_ref().and_then(user_info) else { return };
            if ctx.out.people.lock().upsert_user(u) {
                ctx.out.poke();
            }
        }
        Err(e) => degrade(&ctx, e),
    }
}

/// Get Guild Member (`GET /guilds/{guild.id}/members/{user.id}`, documenté) :
/// pseudo, rôles, arrivée, boost, avatar de serveur.
pub(super) async fn fetch_member(ctx: Arc<Ctx>, guild_id: u64, user_id: u64) {
    let url = format!("{}/guilds/{guild_id}/members/{user_id}", ctx.api_root);
    match api(&ctx, Route::guild_member_get(guild_id), reqwest::Method::GET, &url, None, FETCH_MAX_WAIT, std::future::pending()).await {
        Ok(body) => {
            let Ok(m) = serde_json::from_str::<RawGuildMember>(&body) else { return };
            if m.user_id() != Some(user_id) {
                return;
            }
            if store_member(&mut ctx.out.people.lock(), guild_id, &m) {
                ctx.out.poke();
            }
        }
        Err(e) => degrade(&ctx, e),
    }
}

/// Create DM (`POST /users/@me/channels`, documenté : rend le MP existant
/// s'il y en a un). Le MP est ajouté à la liste puis ouvert.
pub(super) async fn open_dm(ctx: Arc<Ctx>, user_id: u64) {
    let url = format!("{}/users/@me/channels", ctx.api_root);
    let body = json!({ "recipient_id": user_id.to_string() });
    match api(&ctx, Route::dm_create(), reqwest::Method::POST, &url, Some(body), OPEN_DM_MAX_WAIT, std::future::pending()).await {
        Ok(body) => {
            let c = match serde_json::from_str::<RawChannel>(&body) {
                Ok(c) => c,
                Err(e) => return ctx.out.send(Event::Error(crate::state::Problem::Api(e.to_string()))),
            };
            store_dm_users(&mut ctx.out.people.lock(), &c);
            let Some(dm) = c.into_dm(&HashMap::new()) else { return };
            let channel_id = dm.id;
            ctx.out.send(Event::DmUpsert { dm, known: DmKnown::ALL });
            ctx.out.send(Event::DmOpened { channel_id });
        }
        Err(e) => ctx.out.send(e.into_event(Event::Error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::people::Badge;

    fn fixture(name: &str) -> serde_json::Value {
        let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path} : {e}"))
    }

    fn presence(json: &str) -> Option<(u64, Presence)> {
        serde_json::from_str::<RawPresence>(json).ok().and_then(|p| p.to_presence())
    }

    #[test]
    fn presence_update_fixture() {
        let v = fixture("presence_update");
        let (id, p) = presence(&v["d"].to_string()).unwrap_or_else(|| panic!("présence attendue"));
        assert_eq!(id, 1000000000000000002);
        assert_eq!(p.status, Status::Online);
        assert_eq!(p.client, ClientStatus { desktop: Some(Status::Online), mobile: Some(Status::Idle), web: None });
        let kinds: Vec<ActivityKind> = p.activities.iter().map(|a| a.kind).collect();
        assert_eq!(kinds, vec![ActivityKind::Custom, ActivityKind::Playing, ActivityKind::Listening]);
        let custom = p.custom().unwrap_or_else(|| panic!("statut personnalisé attendu"));
        assert_eq!(custom.state.as_deref(), Some("En train de coder"));
        assert_eq!(custom.emoji.as_ref().and_then(|e| e.name.as_deref()), Some("🦀"));
        assert_eq!(p.activities[1].name, "Jeu Fictif");
        assert_eq!(p.activities[1].start_ms, Some(1_736_935_200_000));
        assert_eq!(p.activities[2].details.as_deref(), Some("Morceau fictif"));
        assert!(p.activities[2].rich.is_none(), "rien d'enrichi");
    }

    /// Présence enrichie (lot C3) : Spotify, jeu avec images et boutons,
    /// images d'autres sites refusées.
    #[test]
    fn rich_presence_fixture() {
        let v = fixture("presence_update_rich");
        let (_, p) = presence(&v["d"].to_string()).unwrap_or_else(|| panic!("présence attendue"));
        let [spotify, game, watch] = &p.activities[..] else { panic!("trois activités : {:?}", p.activities) };
        let s = spotify.rich().unwrap_or_else(|| panic!("Spotify enrichi"));
        assert!(s.spotify);
        assert_eq!((spotify.start_ms, s.end_ms), (Some(1_736_935_200_000), Some(1_736_935_410_000)), "horodatages en chaînes");
        assert_eq!(s.sync_id.as_deref(), Some("0FictifMorceau000000a"));
        assert_eq!(s.large, Some(ActivityImage { url: None, text: Some("Album fictif".into()) }), "i.scdn.co jamais contacté");
        let g = game.rich().unwrap_or_else(|| panic!("jeu enrichi"));
        assert_eq!(g.app_id, Some(6000000000000000001));
        assert_eq!(g.session_id.as_deref(), Some("0a1b2c3d4e5f"));
        assert_eq!(g.large.as_ref().and_then(|i| i.url.as_deref()), Some("https://cdn.discordapp.com/app-assets/6000000000000000001/6000000000000000030.png"));
        assert_eq!(g.small.as_ref().and_then(|i| i.url.as_deref()), Some("https://media.discordapp.net/external/fictif/rang.png"));
        assert_eq!(g.small.as_ref().and_then(|i| i.text.as_deref()), Some("Rang 12"));
        assert_eq!(g.buttons, ["Rejoindre", "Site du jeu"]);
        assert!(!g.spotify);
        assert!(watch.rich.is_none(), "images d'autres sites ou hostiles : rien");
        assert_eq!(asset_url(None, "123"), None, "image d'application sans application");
        assert_eq!(asset_url(Some(1), "mp:/x"), None);
        assert_eq!(asset_url(Some(1), "mp:a?b=c"), None);
    }

    #[test]
    fn presence_tolerates_odd_shapes() {
        // Statut illisible : présence ignorée, sans erreur.
        assert!(presence(r#"{ "user": { "id": "1" }, "status": "ailleurs" }"#).is_none());
        assert!(presence(r#"{ "user": { "id": "1" } }"#).is_none());
        assert!(presence(r#"{ "user": { "id": 1 }, "status": "online" }"#).is_none());
        // Champs secondaires illisibles : ignorés un par un.
        let (_, p) = presence(
            r#"{ "user": { "id": "1" }, "status": "dnd", "client_status": "x",
                 "activities": [ 5, { "type": 9, "name": "?" }, { "type": 0 }, { "type": 3, "name": "Film", "timestamps": "hier" },
                                 { "type": 4, "name": "Custom Status", "emoji": { "id": "7000000000000000001", "name": null } } ] }"#,
        )
        .unwrap_or_else(|| panic!("présence attendue"));
        assert_eq!(p.status, Status::Dnd);
        assert_eq!(p.client, ClientStatus::default());
        assert_eq!(p.activities.len(), 2);
        assert_eq!((p.activities[0].kind, p.activities[0].start_ms), (ActivityKind::Watching, None));
        assert_eq!(p.activities[1].emoji, Some(ActivityEmoji { name: None, id: Some(7000000000000000001), animated: false }));
        // Activités au-delà de la borne : ignorées ; textes rognés.
        let many = (0..20).map(|_| format!(r#"{{ "type": 0, "name": "{}" }}"#, "x".repeat(500))).collect::<Vec<_>>().join(",");
        let (_, p) = presence(&format!(r#"{{ "user": {{ "id": "1" }}, "status": "online", "activities": [{many}] }}"#)).unwrap_or_else(|| panic!("présence"));
        assert_eq!(p.activities.len(), MAX_ACTIVITIES);
        assert!(p.activities.iter().all(|a| a.name.chars().count() == crate::state::people::MAX_TEXT));
    }

    #[test]
    fn rest_user_fixture() {
        let body = fixture("rest_user").to_string();
        let u = serde_json::from_str::<RawUser>(&body).ok().as_ref().and_then(user_info).unwrap_or_else(|| panic!("utilisateur attendu"));
        assert_eq!((u.id, u.username.as_str(), u.display_name()), (1000000000000000002, "alice_fictive", "Alice"));
        assert!(matches!(u.banner, Some(Some(h)) if !h.animated));
        assert_eq!(u.accent, Some(Some(0x5b8def)));
        assert_eq!(u.flags.map(Badge::of), Some(vec![Badge::HypesquadBrilliance, Badge::ActiveDeveloper]));
        assert_eq!(u.discriminator, None);
        // Objet de message : ni bannière ni couleur (inconnues, pas absentes).
        let m = fixture("message_create_simple");
        let a = serde_json::from_value::<RawUser>(m["d"]["author"].clone()).ok().as_ref().and_then(user_info).unwrap_or_else(|| panic!("auteur"));
        assert_eq!((a.banner, a.accent, a.flags), (None, None, Some(0)));
        // Bannière nulle, couleur hors bornes : connues comme absentes.
        let u: RawUser = serde_json::from_str(r#"{ "id": "1", "username": "u", "banner": null, "accent_color": 99999999, "discriminator": "0042", "bot": true }"#)
            .unwrap_or_else(|e| panic!("{e}"));
        let u = user_info(&u).unwrap_or_else(|| panic!("utilisateur"));
        assert_eq!((u.banner, u.accent, u.handle(), u.bot), (Some(None), Some(None), "u#0042".to_string(), true));
    }

    #[test]
    fn rest_guild_member_fixture() {
        let body = fixture("rest_guild_member").to_string();
        let m: RawGuildMember = serde_json::from_str(&body).unwrap_or_else(|e| panic!("{e}"));
        let info = member_info(&m);
        assert_eq!(info.nick.as_deref(), Some("Ali"));
        assert_eq!(info.roles, Some(vec![2000000000000001001]));
        assert_eq!(info.joined_at, Some(1_709_294_400_000));
        assert_eq!(info.premium_since, Some(1_717_243_200_000));
        assert!(info.avatar.is_some() && info.timeout_until.is_none());
        let mut p = People::default();
        store_member(&mut p, 9, &m);
        assert_eq!(p.member(9, 1000000000000000002).and_then(|m| m.nick.clone()).as_deref(), Some("Ali"));
        assert_eq!(p.user(1000000000000000002).map(|u| u.username.as_str()), Some("alice_fictive"));
    }

    #[test]
    fn message_author_and_member_are_kept() {
        let m: RawMessage = serde_json::from_value(fixture("message_create_simple")["d"].clone()).unwrap_or_else(|e| panic!("{e}"));
        let hub = PeopleHub::default();
        message(&m, &hub);
        let p = hub.lock();
        assert_eq!(p.user(1000000000000000002).map(|u| u.display_name().to_string()), Some("Alice".into()));
        let member = p.member(2000000000000000001, 1000000000000000002).cloned().unwrap_or_else(|| panic!("membre attendu"));
        assert_eq!((member.roles, member.joined_at), (Some(Vec::new()), Some(1_709_294_400_000)));
    }

    #[test]
    fn dm_create_response_fixture() {
        let c: RawChannel = serde_json::from_value(fixture("rest_dm_create")).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(c.dm_peer(), Some((1000000000000000003, 4000000000000000003)));
        let mut p = People::default();
        store_dm_users(&mut p, &c);
        assert_eq!(p.dm_with(1000000000000000003), Some(4000000000000000003));
        assert_eq!(p.dm_peer(4000000000000000003), Some(1000000000000000003));
    }

    /// Un PRESENCE_UPDATE ne met rien dans la file des événements, et ne
    /// réveille l'interface que pour un utilisateur affiché qui change.
    #[test]
    fn presence_wakes_only_for_watched_users() {
        use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
        const ALICE: u64 = 1000000000000000002;
        let (tx, rx) = std::sync::mpsc::channel();
        let wakes = Arc::new(AtomicUsize::new(0));
        let w = wakes.clone();
        let out = Out { tx, wake: Arc::new(move || { w.fetch_add(1, SeqCst); }), people: PeopleHub::default(), watch: Default::default(), interest: Default::default() };
        let online = fixture("presence_update")["d"].to_string();
        let idle = online.replace(r#""status":"online""#, r#""status":"idle""#);
        assert_ne!(online, idle);
        presence_update(&online, &out);
        presence_update(&idle, &out);
        assert_eq!(wakes.load(SeqCst), 0, "Alice n'est pas affichée");
        {
            let mut p = out.people.lock();
            p.watch(ALICE);
            p.end_frame();
        }
        presence_update(&idle, &out);
        assert_eq!(wakes.load(SeqCst), 0, "rien n'a changé");
        presence_update(&online, &out);
        assert_eq!(wakes.load(SeqCst), 1);
        presence_update("{ illisible", &out);
        // Membre d'un utilisateur affiché qui change : réveil aussi.
        let mut m: serde_json::Value = fixture("rest_guild_member");
        m["guild_id"] = json!("2000000000000000001");
        let m: RawGuildMember = serde_json::from_value(m).unwrap_or_else(|e| panic!("{e}"));
        member_update(&m, &out);
        assert_eq!(wakes.load(SeqCst), 2);
        member_update(&m, &out);
        assert_eq!(wakes.load(SeqCst), 2);
        assert_eq!(rx.try_iter().count(), 0, "aucun événement");
    }

    #[test]
    fn guild_create_members_and_presences() {
        let mut v = fixture("guild_create_alpha");
        v["d"]["members"] = serde_json::Value::Array(vec![fixture("rest_guild_member"), json!(7)]);
        v["d"]["presences"] = json!([fixture("presence_update")["d"], { "user": { "id": "1000000000000000003" }, "status": "idle" }, null]);
        let g: RawGuild = serde_json::from_value(v["d"].clone()).unwrap_or_else(|e| panic!("{e}"));
        let hub = PeopleHub::default();
        guild(&g, &hub);
        let mut p = hub.lock();
        assert_eq!(p.status(1000000000000000002), Some(Status::Online));
        assert_eq!(p.status(1000000000000000003), Some(Status::Idle));
        assert!(p.member(2000000000000000001, 1000000000000000002).is_some());
    }
}
