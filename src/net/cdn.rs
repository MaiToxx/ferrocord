//! CDN des images (avatars, icônes) : adresses et téléchargement.
//!
//! Le CDN n'a jamais besoin du jeton : le client HTTP d'ici est distinct de
//! celui de l'API, sans en-tête `Authorization`, sans cookies, sans proxy,
//! sans suivre les redirections, et ne contacte que les domaines du CDN de
//! Discord (vérifiés avant chaque requête par [`CdnHosts::check`]).

use crate::state::model::ImageRef;
use std::time::Duration;

/// Taille d'image la plus petite et la plus grande servies par le CDN
/// (puissances de deux, documentation « Image Formatting »).
pub const MIN_SIZE: u32 = 16;
pub const MAX_SIZE: u32 = 4096;

/// Tailles acceptées en plus des puissances de deux (docs.discord.food,
/// « Reference » › CDN Formatting : « with some additional accepted values »).
const EXTRA_SIZES: [u32; 22] = [20, 22, 24, 28, 40, 44, 48, 56, 60, 80, 96, 100, 160, 240, 300, 320, 480, 600, 640, 1280, 1536, 3072];

/// Sur Android, les densités d'écran (2,625, 2,75, 3…) tombent loin des
/// puissances de deux : la taille intermédiaire évite de télécharger et de
/// décoder jusqu'à quatre fois trop de pixels. Ailleurs, puissances de deux
/// seules (adresses et cache disque inchangés).
const FINE_SIZES: bool = cfg!(target_os = "android");

/// Plus petite taille servie par le CDN qui couvre `px` pixels.
pub fn accepted_size(px: u32, fine: bool) -> u32 {
    let pow = px.clamp(MIN_SIZE, MAX_SIZE).next_power_of_two().min(MAX_SIZE);
    if !fine {
        return pow;
    }
    EXTRA_SIZES.iter().copied().filter(|s| *s >= px).fold(pow, u32::min)
}

/// Domaines d'images autorisés.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CdnHosts {
    /// `https://cdn.discordapp.com` et `https://media.discordapp.net`, sans
    /// identifiants, port explicite ni fragment.
    Discord,
    /// Tests seulement : `http://127.0.0.1` (tout port).
    Loopback,
}

impl CdnHosts {
    /// Adresse normalisée si elle est acceptée ; c'est cette forme-là qui est
    /// demandée ensuite (l'hôte vérifié est exactement celui contacté).
    pub fn check(self, url: &str) -> Option<String> {
        if !url.is_ascii() {
            return None;
        }
        let u = reqwest::Url::parse(url).ok()?;
        if !u.username().is_empty() || u.password().is_some() || u.fragment().is_some() || u.cannot_be_a_base() {
            return None;
        }
        let ok = match self {
            CdnHosts::Discord => {
                u.scheme() == "https"
                    && u.port().is_none()
                    && (u.domain().is_some_and(|h| h == "cdn.discordapp.com" || h == "media.discordapp.net" || is_external_proxy(h)) || is_twemoji(&u) || is_gif_media(&u))
            }
            CdnHosts::Loopback => u.scheme() == "http" && u.host_str() == Some("127.0.0.1"),
        };
        ok.then(|| u.as_str().to_string())
    }
}

/// Images des emojis Unicode : jeu Twemoji (graphismes sous licence CC-BY 4.0,
/// © Twitter et contributeurs de jdecked/twemoji), version figée, servi par
/// le CDN public jsDelivr et gardé dans le cache disque d'images. Seules les
/// images PNG 72 × 72 de cette version sont acceptées sur ce domaine.
pub const TWEMOJI_BASE: &str = "https://cdn.jsdelivr.net/gh/jdecked/twemoji@16.0.1/assets/72x72";

fn is_twemoji(u: &reqwest::Url) -> bool {
    u.domain() == Some("cdn.jsdelivr.net")
        && u.query().is_none()
        && u.path().strip_prefix("/gh/jdecked/twemoji@16.0.1/assets/72x72/").and_then(|f| f.strip_suffix(".png")).is_some_and(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'))
}

/// Médias du sélecteur de GIF (lot G) : les réponses de `/gifs/*` désignent
/// des fichiers de `media.tenor.com`, que le client officiel télécharge lui
/// aussi directement (aucun proxy de Discord ne les sert sans signature).
/// Aucun jeton n'y part ; seuls des fichiers image sans paramètres sont acceptés.
pub fn is_gif_media(u: &reqwest::Url) -> bool {
    u.domain() == Some("media.tenor.com") && u.query().is_none() && [".gif", ".webp", ".png"].iter().any(|ext| u.path().ends_with(ext))
}

/// Proxy des images externes des embeds (`images-ext-1.discordapp.net`,
/// `images-ext-2…`) : il sert une copie de l'image d'un site tiers, que
/// l'application ne contacte jamais elle-même.
fn is_external_proxy(host: &str) -> bool {
    host.strip_prefix("images-ext-")
        .and_then(|r| r.strip_suffix(".discordapp.net"))
        .is_some_and(|n| !n.is_empty() && n.len() <= 2 && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Fichier servi par Discord lui-même : son CDN, son proxy média ou son
/// proxy des médias externes (copie d'un fichier tiers). C'est la condition
/// pour lire dans l'application la vidéo d'un embed (lot VID).
pub fn served_by_discord(url: &str) -> bool {
    url.is_ascii()
        && reqwest::Url::parse(url).is_ok_and(|u| {
            u.scheme() == "https" && u.port().is_none() && u.username().is_empty() && u.password().is_none() && u.domain().is_some_and(|h| h == "cdn.discordapp.com" || h == "media.discordapp.net" || is_external_proxy(h))
        })
}

/// Affiche d'une vidéo jointe : sa première image en JPEG, que le proxy
/// média sert quand on lui demande ce format (`format=jpeg` ; forme relevée
/// dans `docs/passerelle-utilisateur.md`, lot VID). `None` hors du proxy média.
pub fn poster_url(proxy_url: &str) -> Option<String> {
    let mut u = reqwest::Url::parse(proxy_url).ok()?;
    if u.scheme() != "https" || u.domain() != Some("media.discordapp.net") {
        return None;
    }
    let kept: Vec<(String, String)> = u.query_pairs().filter(|(k, _)| k != "format").map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    u.query_pairs_mut().clear().extend_pairs(kept).append_pair("format", "jpeg");
    Some(u.into())
}

/// Adresse d'une image de pièce jointe ou d'embed redimensionnée par le
/// proxy média : `width` et `height` (pixels) remplacent ceux déjà présents,
/// les autres paramètres (signature `ex`, `is`, `hm`) sont gardés. Une
/// adresse illisible est rendue telle quelle (elle sera refusée au
/// téléchargement par [`CdnHosts::check`]).
pub fn media_url(url: &str, width: u32, height: u32) -> String {
    let Ok(mut u) = reqwest::Url::parse(url) else { return url.to_string() };
    // Hors du proxy média de Discord : pas de redimensionnement, adresse intacte.
    if is_gif_media(&u) {
        return url.to_string();
    }
    let kept: Vec<(String, String)> = u.query_pairs().filter(|(k, _)| k != "width" && k != "height").map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    u.query_pairs_mut().clear().extend_pairs(kept).append_pair("width", &width.max(1).to_string()).append_pair("height", &height.max(1).to_string());
    u.into()
}

/// Adresse d'une image fixe redimensionnée par le proxy média, sans perte
/// de qualité (lot « messages », partie D). [`media_url`] seule laisse le
/// proxy réencoder un JPEG en JPEG à sa qualité par défaut : une seconde
/// compression, visible sur les textes et les aplats. Ici on demande
/// `quality=lossless` (valeurs `lossless`, `high`, `low`, « Only supported
/// with WebP and JPG formats » : docs.discord.food, « CDN Formats »), en
/// WebP pour un JPEG ou un PNG : la forme des adresses du client officiel
/// (`…&format=webp&quality=lossless&width=…&height=…`). Seuls des
/// paramètres s'ajoutent à l'adresse reçue ; jamais sur une image peut-être
/// animée (GIF, extension inconnue), ni si un `format` est déjà demandé
/// (affiche d'une vidéo) ; un WebP garde son format (il peut être animé).
pub fn still_url(url: &str, width: u32, height: u32) -> String {
    let sized = media_url(url, width, height);
    let Ok(mut u) = reqwest::Url::parse(&sized) else { return sized };
    if !u.domain().is_some_and(|h| h == "media.discordapp.net" || is_external_proxy(h)) || u.query_pairs().any(|(k, _)| k == "format" || k == "quality") {
        return sized;
    }
    let ext = u.path().rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("jpg" | "jpeg" | "png") => u.query_pairs_mut().append_pair("format", "webp").append_pair("quality", "lossless"),
        Some("webp") => u.query_pairs_mut().append_pair("quality", "lossless"),
        _ => return sized,
    };
    u.into()
}

/// Taille demandée au proxy pour afficher `w` × `h` pixels : la même
/// proportion, le grand côté arrondi au palier supérieur. Redimensionner la
/// fenêtre ne redemande donc l'image qu'en changeant de palier ; la copie
/// reçue est ensuite ramenée une fois, ici, à la taille exacte.
pub fn stepped(w: u32, h: u32, max_side: u32) -> (u32, u32) {
    let (w, h) = (w.max(1), h.max(1));
    let long = w.max(h);
    let step = if long <= 256 { 32 } else if long <= 1024 { 64 } else { 128 };
    let target = long.div_ceil(step).saturating_mul(step).min(max_side.max(1));
    if target <= long {
        return (w, h);
    }
    let scale = |v: u32| ((u64::from(v) * u64::from(target)).div_ceil(u64::from(long)) as u32).clamp(1, target);
    (scale(w), scale(h))
}

/// Format demandé au CDN.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    /// WebP statique (première image d'une image animée).
    WebP,
    /// WebP animé (`?animated=true`), pour les lots suivants.
    AnimatedWebP,
    Gif,
}

impl Format {
    fn ext(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpg",
            Format::WebP | Format::AnimatedWebP => "webp",
            Format::Gif => "gif",
        }
    }
}

/// Taille à demander au CDN pour une image affichée sur `points` points à
/// `ppp` pixels par point : plus petite taille servie qui la couvre, entre 16
/// et 4096 (puissance de deux, ou taille intermédiaire sur Android).
pub fn cdn_size(points: f32, ppp: f32) -> u32 {
    let px = (points * ppp).ceil();
    let px = if px.is_finite() && px > 0.0 { px.min(MAX_SIZE as f32) as u32 } else { MIN_SIZE };
    accepted_size(px, FINE_SIZES)
}

/// Adresse d'une image sur le CDN `base` (sans `/` final). `size` est
/// ramené à une taille servie (voir [`accepted_size`]).
pub fn image_url(base: &str, r: &ImageRef, size: u32, format: Format) -> String {
    let base = base.trim_end_matches('/');
    let path = match r {
        ImageRef::Avatar { user_id, hash } => format!("avatars/{user_id}/{hash}"),
        ImageRef::GuildIcon { guild_id, hash } => format!("icons/{guild_id}/{hash}"),
        // Forme utilisée par l'app officielle, absente de la documentation publique.
        ImageRef::GroupIcon { channel_id, hash } => format!("channel-icons/{channel_id}/{hash}"),
        // Adresse reçue avec le message, passée à la demande (`media_url`) :
        // rien à construire ici, et une adresse vide est refusée.
        ImageRef::Media { .. } => return String::new(),
        ImageRef::Banner { user_id, hash } => format!("banners/{user_id}/{hash}"),
        ImageRef::MemberAvatar { guild_id, user_id, hash } => format!("guilds/{guild_id}/users/{user_id}/avatars/{hash}"),
        // docs.discord.food, « CDN Formatting » (Guild Member Banner, Profile Badge).
        ImageRef::MemberBanner { guild_id, user_id, hash } => format!("guilds/{guild_id}/users/{user_id}/banners/{hash}"),
        ImageRef::BadgeIcon { hash } => format!("badge-icons/{hash}"),
        // Hors du CDN de Discord : `Cdn::url` les construit.
        ImageRef::Twemoji(_) => return String::new(),
        ImageRef::CustomEmoji { id } => format!("emojis/{id}"),
        ImageRef::Sticker { id, .. } => format!("stickers/{id}"),
        // Documentation « Image Formatting » : Role Icon.
        ImageRef::RoleIcon { role_id, hash } => format!("role-icons/{role_id}/{hash}"),
        // Documentation « Image Formatting » : Guild Scheduled Event Cover.
        ImageRef::EventCover { event_id, hash } => format!("guild-events/{event_id}/{hash}"),
        // Lot P. Documentation « Image Formatting » : Avatar Decoration (PNG
        // seul), Guild Tag Badge. `passthrough` (docs.discord.food, « CDN
        // Formatting ») : `false` donne la première image d'une décoration animée.
        ImageRef::Decoration { asset } => {
            return format!("{base}/avatar-decoration-presets/{}.png?size={}&passthrough=false", asset.name(), accepted_size(size, FINE_SIZES));
        }
        // docs.discord.food, « CDN Formatting » : Nameplate, taille fixe
        // (`size` ignoré sous `/assets/`), noms `static.png` et `asset.webm`.
        ImageRef::Nameplate { asset } => return format!("{base}/assets/collectibles/{}static.png", asset.name()),
        ImageRef::TagBadge { guild_id, hash } => format!("guild-tag-badges/{guild_id}/{hash}"),
        // Documentation « Image Formatting » : Application Icon, Application Cover.
        ImageRef::AppIcon { app_id, hash } => format!("app-icons/{app_id}/{hash}"),
    };
    let size = accepted_size(size, FINE_SIZES);
    let animated = if format == Format::AnimatedWebP { "&animated=true" } else { "" };
    format!("{base}/{path}.{}?size={size}{animated}", format.ext())
}

/// Échec d'un téléchargement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FetchError {
    /// 404 (ou 403, 410) : l'image n'existe pas, inutile de redemander.
    NotFound,
    /// Adresse refusée, redirection, réponse trop grosse ou statut inattendu :
    /// redemander donnerait la même chose.
    Refused(String),
    /// Erreur réseau, délai dépassé ou 5xx : on pourra réessayer plus tard.
    Network(String),
    /// 429 : le CDN demande d'attendre ce délai (`Retry-After`, borné)
    /// avant toute nouvelle requête d'image.
    RateLimited(Duration),
}

impl FetchError {
    /// Rien ne sert de réessayer pendant la session.
    pub fn is_permanent(&self) -> bool {
        !matches!(self, FetchError::Network(_) | FetchError::RateLimited(_))
    }
}

/// Attente imposée par un 429 du CDN quand `Retry-After` manque ou est illisible.
pub const RETRY_AFTER_DEFAULT: Duration = Duration::from_secs(30);
/// Attente la plus longue acceptée (un serveur ne peut pas figer les images plus longtemps).
pub const RETRY_AFTER_MAX: Duration = Duration::from_secs(300);

/// Attente demandée par l'en-tête `Retry-After` (secondes, ou date HTTP
/// comparée à `now`), bornée à [`RETRY_AFTER_MAX`] ; [`RETRY_AFTER_DEFAULT`]
/// si l'en-tête manque ou est illisible.
pub fn retry_after(value: Option<&str>, now: std::time::SystemTime) -> Duration {
    let Some(v) = value.map(str::trim) else { return RETRY_AFTER_DEFAULT };
    let wait = if let Ok(secs) = v.parse::<f64>() {
        (secs.is_finite() && secs >= 0.0).then(|| Duration::from_secs_f64(secs.min(RETRY_AFTER_MAX.as_secs_f64())))
    } else {
        http_date(v).map(|at| at.duration_since(now).unwrap_or(Duration::ZERO))
    };
    wait.unwrap_or(RETRY_AFTER_DEFAULT).min(RETRY_AFTER_MAX)
}

/// Date HTTP au format IMF-fixdate (`Sun, 06 Nov 1994 08:49:37 GMT`), le
/// seul que les serveurs doivent envoyer.
fn http_date(s: &str) -> Option<std::time::SystemTime> {
    let mut it = s.split_ascii_whitespace();
    let (_day, d, m, y, hms, zone) = (it.next()?, it.next()?, it.next()?, it.next()?, it.next()?, it.next()?);
    if it.next().is_some() || zone != "GMT" || d.len() != 2 || y.len() != 4 {
        return None;
    }
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let month = MONTHS.iter().position(|x| *x == m)? as i64 + 1;
    let (day, year): (i64, i64) = (d.parse().ok()?, y.parse().ok()?);
    let mut t = hms.split(':').map(|p| if p.len() == 2 { p.parse::<i64>().ok() } else { None });
    let (h, mi, sec) = (t.next()??, t.next()??, t.next()??);
    if t.next().is_some() || !(1..=31).contains(&day) || h > 23 || mi > 59 || sec > 60 || year < 1970 {
        return None;
    }
    // Jours depuis 1970-01-01 (algorithme « days from civil » de H. Hinnant).
    let (yy, mm) = if month <= 2 { (year - 1, month + 9) } else { (year, month - 3) };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * mm + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = u64::try_from(days * 86_400 + h * 3600 + mi * 60 + sec).ok()?;
    std::time::UNIX_EPOCH.checked_add(Duration::from_secs(secs))
}

/// Limites d'un téléchargement.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Réponse refusée au-delà (sans être lue en entier).
    pub max_bytes: usize,
    /// Durée maximale d'une requête, réponse comprise.
    pub timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        // Un avatar de 4096 px en PNG reste bien en deçà de 10 Mo.
        Limits { max_bytes: 10 << 20, timeout: Duration::from_secs(20) }
    }
}

/// Client du CDN.
#[derive(Clone)]
pub struct Cdn {
    http: reqwest::Client,
    base: String,
    hosts: CdnHosts,
    limits: Limits,
}

impl Cdn {
    /// `base` et `hosts` viennent d'[`super::Endpoints`].
    pub fn new(base: String, hosts: CdnHosts, limits: Limits) -> Self {
        Self::with_user_agent(base, hosts, limits, super::identity::FERROCORD_UA)
    }

    /// Comme [`Cdn::new`], avec le User-Agent de l'identité annoncée (`net::identity`).
    pub fn with_user_agent(base: String, hosts: CdnHosts, limits: Limits, user_agent: &str) -> Self {
        let http = crate::platform::http_builder()
            .user_agent(user_agent)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .https_only(hosts == CdnHosts::Discord)
            .timeout(limits.timeout)
            .connect_timeout(limits.timeout)
            .build()
            // Échoue seulement si la pile TLS ne s'initialise pas : rien à voir avec les données reçues.
            .expect("client HTTP du CDN");
        Cdn { http, base, hosts, limits }
    }

    pub fn from_endpoints(ep: &super::Endpoints) -> Self {
        Self::with_user_agent(ep.cdn.clone(), ep.cdn_hosts, Limits::default(), &ep.identity.user_agent())
    }

    /// Adresse d'une image statique (WebP) à la taille `size`.
    pub fn url(&self, r: &ImageRef, size: u32) -> String {
        let loopback = self.hosts == CdnHosts::Loopback;
        match r {
            ImageRef::Twemoji(u) if loopback => format!("{}/twemoji/{}.png", self.base.trim_end_matches('/'), u.file()),
            ImageRef::Twemoji(u) => format!("{TWEMOJI_BASE}/{}.png", u.file()),
            // Documentation « Image Formatting » : PNG et APNG en `.png`, GIF
            // servis par `media.discordapp.net`.
            ImageRef::Sticker { gif: false, .. } => image_url(&self.base, r, size, Format::Png),
            ImageRef::Sticker { gif: true, .. } if loopback => image_url(&self.base, r, size, Format::Gif),
            ImageRef::Sticker { gif: true, .. } => image_url("https://media.discordapp.net", r, size, Format::Gif),
            ImageRef::BadgeIcon { .. } | ImageRef::TagBadge { .. } | ImageRef::Decoration { .. } | ImageRef::Nameplate { .. } => image_url(&self.base, r, size, Format::Png),
            _ => image_url(&self.base, r, size, Format::WebP),
        }
    }

    /// Adresse de la version animée de `r`, s'il en a une : GIF pour les
    /// empreintes `a_` (avatars, icônes, bannières) et pour les emojis
    /// personnalisés (l'appelant sait s'ils sont animés : le modèle le
    /// dit, pas la référence) ; l'adresse habituelle pour un autocollant,
    /// déjà en APNG ou en GIF ; le WebM d'une plaque.
    pub fn anim_url(&self, r: &ImageRef, size: u32) -> Option<String> {
        match r {
            // Bannières : WebP animé, que la documentation officielle (« Image
            // Formatting », Animated Images) recommande plutôt que le GIF.
            ImageRef::Banner { hash, .. } | ImageRef::MemberBanner { hash, .. } if hash.animated => Some(image_url(&self.base, r, size, Format::AnimatedWebP)),
            ImageRef::Avatar { hash, .. } | ImageRef::GuildIcon { hash, .. } | ImageRef::MemberAvatar { hash, .. } if hash.animated => Some(image_url(&self.base, r, size, Format::Gif)),
            // Décoration animée : l'APNG d'origine (`passthrough=true`).
            ImageRef::Decoration { asset } if asset.animated() => {
                Some(format!("{}/avatar-decoration-presets/{}.png?size={}&passthrough=true", self.base.trim_end_matches('/'), asset.name(), accepted_size(size, FINE_SIZES)))
            }
            ImageRef::CustomEmoji { .. } => Some(image_url(&self.base, r, size, Format::Gif)),
            ImageRef::Sticker { .. } => Some(self.url(r, size)),
            // Plaque animée : un WebM à taille fixe, à côté de l'image fixe
            // (docs.discord.food, « CDN Formatting », Nameplate : `asset.webm`).
            // Lu par `images::webm` ; jouée au survol de la ligne seulement.
            ImageRef::Nameplate { asset } => Some(format!("{}/assets/collectibles/{}asset.webm", self.base.trim_end_matches('/'), asset.name())),
            _ => None,
        }
    }

    /// Télécharge `url` (qui doit passer [`CdnHosts::check`]). Aucune
    /// donnée d'identification n'est envoyée.
    pub async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        let Some(url) = self.hosts.check(url) else { return Err(FetchError::Refused("adresse d'image refusée".into())) };
        let mut resp = self.http.get(url).send().await.map_err(|e| FetchError::Network(e.without_url().to_string()))?;
        let status = resp.status().as_u16();
        match status {
            200 => {}
            403 | 404 | 410 => return Err(FetchError::NotFound),
            429 => {
                let value = resp.headers().get(reqwest::header::RETRY_AFTER).and_then(|v| v.to_str().ok());
                return Err(FetchError::RateLimited(retry_after(value, std::time::SystemTime::now())));
            }
            408 | 500..=599 =>return Err(FetchError::Network(format!("HTTP {status}"))),
            _ => return Err(FetchError::Refused(format!("HTTP {status}"))),
        }
        let max = self.limits.max_bytes;
        if resp.content_length().is_some_and(|n| n > max as u64) {
            return Err(FetchError::Refused("image trop grosse".into()));
        }
        let mut body = Vec::with_capacity(resp.content_length().map_or(0, |n| n as usize).min(max));
        while let Some(chunk) = resp.chunk().await.map_err(|e| FetchError::Network(e.without_url().to_string()))? {
            if body.len() + chunk.len() > max {
                return Err(FetchError::Refused("image trop grosse".into()));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::CdnHosts::{self, *};
    use super::*;
    use crate::state::model::ImageHash;

    fn ok(rule: CdnHosts, url: &str) -> bool {
        rule.check(url).is_some()
    }

    #[test]
    fn discord_rule_accepts_cdn_hosts() {
        for url in [
            "https://cdn.discordapp.com/avatars/1/0123456789abcdef0123456789abcdef.webp?size=64",
            "https://media.discordapp.net/stickers/1.gif",
            "https://CDN.DiscordApp.com/icons/1/x.png",
            "https://cdn.discordapp.com:443/x.png",
        ] {
            assert!(ok(Discord, url), "{url} devrait être acceptée");
        }
        assert_eq!(Discord.check("https://CDN.DiscordApp.com:443/a.png").as_deref(), Some("https://cdn.discordapp.com/a.png"));
        // Hôte encodé : décodé par l'analyseur, et c'est la forme décodée (vérifiée) qui est demandée.
        assert_eq!(Discord.check("https://cdn%2ediscordapp.com/a.png").as_deref(), Some("https://cdn.discordapp.com/a.png"));
        assert_eq!(Discord.check("https://cdn.discordapp.com%2eevil.tld/a.png"), None);
    }

    #[test]
    fn discord_rule_rejects_lookalikes() {
        for url in [
            "",
            "pas une adresse",
            "http://cdn.discordapp.com/a.png",
            "ftp://cdn.discordapp.com/a.png",
            "wss://cdn.discordapp.com/a.png",
            "https://cdn.discordapp.com.evil.tld/a.png",
            "https://evil.tld/cdn.discordapp.com/a.png",
            "https://evil.tld?.cdn.discordapp.com",
            "https://evil.tld#.cdn.discordapp.com",
            r"https://evil.tld\.cdn.discordapp.com",
            "https://cdn.discordapp.com@evil.tld/a.png",
            "https://user@cdn.discordapp.com/a.png",
            "https://user:mdp@cdn.discordapp.com/a.png",
            "https://evilcdn.discordapp.com/a.png",
            "https://x.cdn.discordapp.com/a.png",
            "https://discordapp.com/a.png",
            "https://discord.com/api/v10/users/@me",
            "https://cdn.discordapp.com./a.png",
            "https://cdn.discordapp.com:8443/a.png",
            "https://cdn.discordapp.com/a.png#frag",
            "https://media.discordapp.net.evil.tld/a.png",
            "https://162.159.130.233/a.png",
            "https://127.0.0.1/a.png",
            "https://cdn.discordаpp.com/a.png",
            "data:image/png;base64,AAAA",
            "file:///C:/Windows/win.ini",
            "javascript:alert(1)",
        ] {
            assert!(!ok(Discord, url), "{url} devrait être refusée");
        }
    }

    /// Proxy des images externes : `images-ext-<chiffres>` seulement.
    #[test]
    fn external_proxy_hosts() {
        for url in ["https://images-ext-1.discordapp.net/external/x/a.png", "https://images-ext-2.discordapp.net/external/y?width=4"] {
            assert!(ok(Discord, url), "{url}");
        }
        for url in [
            "http://images-ext-1.discordapp.net/a.png",
            "https://images-ext-.discordapp.net/a.png",
            "https://images-ext-a.discordapp.net/a.png",
            "https://images-ext-123.discordapp.net/a.png",
            "https://images-ext-1.discordapp.net.evil.tld/a.png",
            "https://x.images-ext-1.discordapp.net/a.png",
            "https://images-ext-1.discordapp.com/a.png",
            "https://exemple.invalid/vignette.png",
        ] {
            assert!(!ok(Discord, url), "{url}");
        }
    }

    /// Emojis et autocollants : adresses documentées, Twemoji limité à sa version figée.
    #[test]
    fn emoji_and_sticker_urls() {
        let cdn = Cdn::new("https://cdn.discordapp.com".into(), Discord, Limits::default());
        let fire = crate::state::emoji::by_name("fire").unwrap_or_else(|| panic!("fire"));
        let tw = cdn.url(&ImageRef::Twemoji(fire), 32);
        assert_eq!(tw, format!("{TWEMOJI_BASE}/1f525.png"));
        assert!(ok(Discord, &tw));
        assert_eq!(cdn.url(&ImageRef::CustomEmoji { id: 42 }, 40), "https://cdn.discordapp.com/emojis/42.webp?size=64");
        assert_eq!(cdn.url(&ImageRef::Sticker { id: 7, gif: false }, 160), "https://cdn.discordapp.com/stickers/7.png?size=256");
        assert_eq!(cdn.url(&ImageRef::Sticker { id: 7, gif: true }, 160), "https://media.discordapp.net/stickers/7.gif?size=256");
        for bad in [
            "https://cdn.jsdelivr.net/gh/jdecked/twemoji@16.0.1/assets/72x72/../../x.png",
            "https://cdn.jsdelivr.net/gh/autre/depot@1/assets/72x72/1f525.png",
            "https://cdn.jsdelivr.net/gh/jdecked/twemoji@16.0.1/assets/72x72/1f525.png?x=1",
            "https://cdn.jsdelivr.net/gh/jdecked/twemoji@16.0.1/assets/svg/1f525.svg",
            "http://cdn.jsdelivr.net/gh/jdecked/twemoji@16.0.1/assets/72x72/1f525.png",
            "https://cdn.jsdelivr.net.evil.tld/gh/jdecked/twemoji@16.0.1/assets/72x72/1f525.png",
        ] {
            assert!(!ok(Discord, bad), "{bad}");
        }
        let local = Cdn::new("http://127.0.0.1:9".into(), Loopback, Limits::default());
        assert_eq!(local.url(&ImageRef::Twemoji(fire), 32), "http://127.0.0.1:9/twemoji/1f525.png");
    }

    #[test]
    fn media_url_sets_size_and_keeps_signature() {
        let u = "https://media.discordapp.net/attachments/1/2/a.png?ex=0a&is=0b&hm=0c";
        assert_eq!(media_url(u, 320, 180), "https://media.discordapp.net/attachments/1/2/a.png?ex=0a&is=0b&hm=0c&width=320&height=180");
        let again = media_url(&media_url(u, 320, 180), 64, 0);
        assert_eq!(again, "https://media.discordapp.net/attachments/1/2/a.png?ex=0a&is=0b&hm=0c&width=64&height=1", "remplacés, jamais doublés");
        assert_eq!(media_url("https://images-ext-1.discordapp.net/external/x/a.png", 8, 8), "https://images-ext-1.discordapp.net/external/x/a.png?width=8&height=8");
        assert_eq!(media_url("pas une adresse", 8, 8), "pas une adresse");
        assert!(ok(Discord, &media_url(u, 1, 1)));
    }

    /// Partie D : sans `quality=lossless`, le proxy recompresse (JPEG → JPEG).
    #[test]
    fn still_url_asks_for_a_lossless_copy() {
        let q = "ex=0a&is=0b&hm=0c";
        let m = |name: &str| format!("https://media.discordapp.net/attachments/1/2/{name}?{q}");
        assert_eq!(still_url(&m("photo.JPG"), 320, 180), format!("{}&width=320&height=180&format=webp&quality=lossless", m("photo.JPG")));
        assert_eq!(still_url(&m("a.png"), 8, 8), format!("{}&width=8&height=8&format=webp&quality=lossless", m("a.png")));
        assert_eq!(still_url(&m("a.webp"), 8, 8), format!("{}&width=8&height=8&quality=lossless", m("a.webp")), "peut être animé : format gardé");
        // Peut-être animée, ou format déjà choisi : rien de plus que la taille.
        for other in [m("a.gif"), m("sans-extension"), format!("{}&format=jpeg", m("film.mp4")), format!("{}&quality=high", m("b.jpg"))] {
            assert_eq!(still_url(&other, 8, 8), media_url(&other, 8, 8), "{other}");
        }
        assert_eq!(still_url("https://images-ext-1.discordapp.net/external/x/https/site.example/a.jpeg", 8, 8), "https://images-ext-1.discordapp.net/external/x/https/site.example/a.jpeg?width=8&height=8&format=webp&quality=lossless");
        // Hors du proxy média : jamais.
        let cdn = "https://cdn.discordapp.com/attachments/1/2/a.png";
        assert_eq!(still_url(cdn, 8, 8), media_url(cdn, 8, 8));
        let g = "https://media.tenor.com/synthetique01AAAAM/chat.png";
        assert_eq!(still_url(g, 8, 8), g);
        assert_eq!(still_url("pas une adresse", 8, 8), "pas une adresse");
        // Appliquée deux fois : rien n'est doublé.
        let twice = still_url(&still_url(&m("a.png"), 320, 180), 64, 64);
        assert_eq!(twice, format!("{}&format=webp&quality=lossless&width=64&height=64", m("a.png")));
        assert!(ok(Discord, &still_url(&m("a.png"), 1, 1)));
    }

    /// Paliers : même proportion, jamais plus petit que demandé, borné.
    #[test]
    fn stepped_rounds_the_long_side_up() {
        assert_eq!(stepped(688, 430, 4096), (704, 440));
        assert_eq!(stepped(704, 440, 4096), (704, 440), "déjà sur un palier");
        assert_eq!((stepped(700, 438, 4096).0, stepped(690, 431, 4096)), (704, (704, 440)), "même palier");
        assert_eq!(stepped(100, 100, 4096), (128, 128));
        assert_eq!(stepped(1300, 20, 4096), (1408, 22));
        assert_eq!(stepped(4000, 3000, 4096), (4096, 3072));
        assert_eq!(stepped(4096, 4096, 4096), (4096, 4096));
        assert_eq!(stepped(5000, 10, 4096), (5000, 10), "au-dessus de la borne : inchangé");
        assert_eq!(stepped(0, 0, 4096), (32, 32));
        for (w, h) in [(1, 1), (33, 7), (257, 255), (1025, 3), (999, 1000)] {
            let (sw, sh) = stepped(w, h, 4096);
            assert!(sw >= w && sh >= h && sw.max(sh) - w.max(h) < 128, "{w}×{h} → {sw}×{sh}");
        }
    }

    #[test]
    fn gif_media_host_is_narrow() {
        let g = "https://media.tenor.com/synthetique01AAAAM/chat.gif";
        assert!(ok(Discord, g));
        assert_eq!(media_url(g, 64, 64), g, "jamais de paramètres ajoutés");
        for bad in [
            "http://media.tenor.com/a/b.gif",
            "https://media.tenor.com/a/b.gif?x=1",
            "https://media.tenor.com/a/b.mp4",
            "https://media.tenor.com:8443/a/b.gif",
            "https://tenor.com/view/x.gif",
            "https://media.tenor.com.evil.tld/a/b.gif",
            "https://u@media.tenor.com/a/b.gif",
        ] {
            assert!(!ok(Discord, bad), "{bad} devrait être refusée");
        }
    }

    #[test]
    fn loopback_rule_is_loopback_http_only() {
        assert!(ok(Loopback, "http://127.0.0.1:1234/avatars/1/x.webp?size=64"));
        for url in ["http://localhost:1234/x", "http://127.0.0.2:1/x", "https://cdn.discordapp.com/x", "http://u@127.0.0.1:1/x", "http://127.0.0.1.evil.tld/x"] {
            assert!(!ok(Loopback, url), "{url} devrait être refusée");
        }
    }

    #[test]
    fn cdn_size_is_next_power_of_two() {
        assert_eq!(cdn_size(40.0, 1.0), 64);
        assert_eq!(cdn_size(40.0, 1.5), 64);
        assert_eq!(cdn_size(40.0, 2.0), 128);
        assert_eq!(cdn_size(32.0, 1.0), 32);
        assert_eq!(cdn_size(32.0, 1.25), 64);
        assert_eq!(cdn_size(1.0, 1.0), 16);
        assert_eq!(cdn_size(0.0, 1.0), 16);
        assert_eq!(cdn_size(-5.0, 1.0), 16);
        assert_eq!(cdn_size(f32::NAN, 1.0), 16);
        assert_eq!(cdn_size(f32::INFINITY, 1.0), 16);
        assert_eq!(cdn_size(5000.0, 2.0), 4096);
        assert_eq!(cdn_size(2049.0, 1.0), 4096);
    }

    #[test]
    fn fine_sizes_follow_the_screen_density() {
        // Rail de 48 points à 2,7 : 130 px, servi en 160 au lieu de 256.
        assert_eq!(accepted_size(105, true), 128);
        assert_eq!(accepted_size(97, true), 100);
        assert_eq!(accepted_size(81, true), 96);
        assert_eq!(accepted_size(130, true), 160);
        assert_eq!(accepted_size(260, true), 300);
        assert_eq!(accepted_size(2100, true), 3072);
        assert_eq!(accepted_size(1, true), 16);
        assert_eq!(accepted_size(9000, true), 4096);
        assert_eq!(accepted_size(130, false), 256, "bureau : puissances de deux seules");
    }

    #[test]
    fn animated_urls() {
        let cdn = Cdn::from_endpoints(&crate::net::Endpoints::default());
        let a = ImageHash::parse("a_0123456789abcdef0123456789abcdef").unwrap_or_else(|| panic!("empreinte"));
        let s = ImageHash::synthetic(1);
        assert_eq!(cdn.anim_url(&ImageRef::Avatar { user_id: 3, hash: a }, 64).as_deref(), Some("https://cdn.discordapp.com/avatars/3/a_0123456789abcdef0123456789abcdef.gif?size=64"));
        assert_eq!(cdn.anim_url(&ImageRef::Avatar { user_id: 3, hash: s }, 64), None, "pas de version animée");
        assert_eq!(cdn.anim_url(&ImageRef::CustomEmoji { id: 9 }, 48).as_deref(), Some("https://cdn.discordapp.com/emojis/9.gif?size=64"));
        let st = ImageRef::Sticker { id: 5, gif: false };
        assert_eq!(cdn.anim_url(&st, 160), Some(cdn.url(&st, 160)));
        assert_eq!(cdn.anim_url(&ImageRef::BadgeIcon { hash: s }, 16), None);
    }

    #[test]
    fn cosmetic_urls() {
        use crate::state::cosmetics::Asset;
        let cdn = Cdn::from_endpoints(&crate::net::Endpoints::default());
        let a = ImageHash::parse("a_0123456789abcdef0123456789abcdef").unwrap_or_else(|| panic!("empreinte"));
        let h = ImageHash::parse("0123456789abcdef0123456789abcdef").unwrap_or_else(|| panic!("empreinte"));
        // Bannière animée : WebP animé ; fixe : pas de version animée.
        assert_eq!(
            cdn.anim_url(&ImageRef::Banner { user_id: 3, hash: a }, 300).as_deref(),
            Some("https://cdn.discordapp.com/banners/3/a_0123456789abcdef0123456789abcdef.webp?size=512&animated=true")
        );
        assert_eq!(cdn.anim_url(&ImageRef::Banner { user_id: 3, hash: h }, 300), None);
        assert!(cdn.anim_url(&ImageRef::MemberBanner { guild_id: 1, user_id: 3, hash: a }, 300).is_some_and(|u| u.ends_with("&animated=true")));
        let still = Asset::decoration("dec0123").unwrap_or_else(|| panic!("nom"));
        let moving = Asset::decoration("a_dec0123").unwrap_or_else(|| panic!("nom"));
        assert_eq!(cdn.url(&ImageRef::Decoration { asset: moving }, 96), "https://cdn.discordapp.com/avatar-decoration-presets/a_dec0123.png?size=128&passthrough=false");
        assert_eq!(cdn.anim_url(&ImageRef::Decoration { asset: moving }, 96).as_deref(), Some("https://cdn.discordapp.com/avatar-decoration-presets/a_dec0123.png?size=128&passthrough=true"));
        assert_eq!(cdn.anim_url(&ImageRef::Decoration { asset: still }, 96), None);
        let plate = Asset::path("nameplates/nameplates/fictive/").unwrap_or_else(|| panic!("chemin"));
        assert_eq!(cdn.url(&ImageRef::Nameplate { asset: plate }, 512), "https://cdn.discordapp.com/assets/collectibles/nameplates/nameplates/fictive/static.png");
        assert_eq!(
            cdn.anim_url(&ImageRef::Nameplate { asset: plate }, 512).as_deref(),
            Some("https://cdn.discordapp.com/assets/collectibles/nameplates/nameplates/fictive/asset.webm"),
            "plaque animée : le WebM, sans paramètre de taille"
        );
        assert!(cdn.anim_url(&ImageRef::Nameplate { asset: plate }, 512).is_some_and(|u| ok(Discord, &u)));
        assert_eq!(cdn.url(&ImageRef::TagBadge { guild_id: 7, hash: h }, 16), "https://cdn.discordapp.com/guild-tag-badges/7/0123456789abcdef0123456789abcdef.png?size=16");
        // Toute adresse construite passe la validation ; un nom hostile n'entre pas.
        for r in [ImageRef::Decoration { asset: moving }, ImageRef::Nameplate { asset: plate }, ImageRef::TagBadge { guild_id: 7, hash: h }] {
            assert!(ok(Discord, &cdn.url(&r, 64)));
        }
        for bad in ["../x", "a?b=1", "a/b", "A", "", "a.b"] {
            assert!(Asset::decoration(bad).is_none(), "{bad}");
        }
        for bad in ["nameplates/../x/", "//evil.invalid/", "a", "a/?x/", "/a/"] {
            assert!(Asset::path(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn image_urls() {
        let h = ImageHash::parse("0123456789abcdef0123456789abcdef").unwrap_or_else(|| panic!("empreinte"));
        let a = ImageHash::parse("a_0123456789abcdef0123456789abcdef").unwrap_or_else(|| panic!("empreinte"));
        let base = "https://cdn.discordapp.com";
        assert_eq!(crate::net::Endpoints::default().cdn, base, "valeur de production");
        assert_eq!(crate::net::Endpoints::default().cdn_hosts, Discord);
        assert_eq!(
            image_url(base, &ImageRef::Avatar { user_id: 80351110224678912, hash: h }, 64, Format::WebP),
            "https://cdn.discordapp.com/avatars/80351110224678912/0123456789abcdef0123456789abcdef.webp?size=64"
        );
        assert_eq!(
            image_url("https://cdn.discordapp.com/", &ImageRef::GuildIcon { guild_id: 1, hash: h }, 100, Format::Png),
            "https://cdn.discordapp.com/icons/1/0123456789abcdef0123456789abcdef.png?size=128"
        );
        assert_eq!(image_url(base, &ImageRef::BadgeIcon { hash: h }, 32, Format::Png), "https://cdn.discordapp.com/badge-icons/0123456789abcdef0123456789abcdef.png?size=32");
        assert_eq!(
            image_url(base, &ImageRef::MemberBanner { guild_id: 1, user_id: 2, hash: h }, 512, Format::WebP),
            "https://cdn.discordapp.com/guilds/1/users/2/banners/0123456789abcdef0123456789abcdef.webp?size=512"
        );
        assert_eq!(image_url(base, &ImageRef::GroupIcon { channel_id: 2, hash: h }, 1, Format::Jpeg), "https://cdn.discordapp.com/channel-icons/2/0123456789abcdef0123456789abcdef.jpg?size=16");
        // Empreinte animée : image statique (première image) sauf demande contraire.
        assert_eq!(
            image_url(base, &ImageRef::Avatar { user_id: 3, hash: a }, 99999, Format::WebP),
            "https://cdn.discordapp.com/avatars/3/a_0123456789abcdef0123456789abcdef.webp?size=4096"
        );
        assert_eq!(
            image_url(base, &ImageRef::Avatar { user_id: 3, hash: a }, 64, Format::AnimatedWebP),
            "https://cdn.discordapp.com/avatars/3/a_0123456789abcdef0123456789abcdef.webp?size=64&animated=true"
        );
        assert_eq!(image_url(base, &ImageRef::Avatar { user_id: 3, hash: a }, 64, Format::Gif), "https://cdn.discordapp.com/avatars/3/a_0123456789abcdef0123456789abcdef.gif?size=64");
        // Toute adresse construite passe la validation.
        for r in [ImageRef::Avatar { user_id: u64::MAX, hash: a }, ImageRef::GuildIcon { guild_id: 0, hash: h }, ImageRef::GroupIcon { channel_id: 9, hash: h }] {
            assert!(ok(Discord, &image_url(base, &r, 64, Format::WebP)));
        }
    }

    #[test]
    fn fetch_errors_permanence() {
        assert!(FetchError::NotFound.is_permanent());
        assert!(FetchError::Refused("x".into()).is_permanent());
        assert!(!FetchError::Network("x".into()).is_permanent());
        assert!(!FetchError::RateLimited(Duration::from_secs(1)).is_permanent());
    }

    #[test]
    fn retry_after_seconds_date_or_default() {
        // 1994-11-06 08:49:37 UTC = 784111777.
        let now = std::time::UNIX_EPOCH + Duration::from_secs(784_111_777);
        let ra = |v: Option<&str>| retry_after(v, now);
        assert_eq!(ra(Some("7")), Duration::from_secs(7));
        assert_eq!(ra(Some(" 0 ")), Duration::ZERO);
        assert_eq!(ra(Some("1.5")), Duration::from_millis(1500));
        assert_eq!(ra(Some("Sun, 06 Nov 1994 08:50:17 GMT")), Duration::from_secs(40));
        assert_eq!(ra(Some("Sun, 06 Nov 1994 08:49:00 GMT")), Duration::ZERO, "date passée");
        assert_eq!(ra(Some("Thu, 29 Feb 2024 00:00:00 GMT")), RETRY_AFTER_MAX, "bornée");
        assert_eq!(ra(Some("999999999")), RETRY_AFTER_MAX);
        for bad in [None, Some(""), Some("-3"), Some("NaN"), Some("inf"), Some("bientôt"), Some("Sun, 06 Nov 1994 08:49:37 CET"), Some("Sun, 6 Nov 1994 08:49:37 GMT"), Some("Sun, 06 Foo 1994 08:49:37 GMT"), Some("Sun, 06 Nov 1994 25:00:00 GMT")] {
            assert_eq!(ra(bad), RETRY_AFTER_DEFAULT, "{bad:?}");
        }
    }
}
