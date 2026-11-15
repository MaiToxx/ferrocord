//! Pièces jointes et embeds tels que Discord les envoie (objets Attachment
//! et Embed de la documentation « Message Resource »). Chaque élément et
//! chaque champ facultatif est lu à part : un embed mal formé est ignoré,
//! un champ illisible est absent, et le message reste lisible.

use super::raw::Item;
use crate::state::model::{Attachment, Embed, EmbedAuthor, EmbedField, EmbedFooter, EmbedKind, EmbedMedia, EmbedProvider};
use serde::{de::DeserializeOwned, Deserialize, Deserializer};

/// Champ facultatif : `None` s'il est absent, `null` ou de forme inattendue.
fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Option<T>, D::Error> {
    Ok(Item::<T>::deserialize(d)?.0)
}

/// Liste dont chaque élément est lu à part ; les illisibles sont ignorés.
pub(super) fn de_items<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Vec<T>, D::Error> {
    let items: Option<Vec<Item<T>>> = lenient(d)?;
    Ok(items.unwrap_or_default().into_iter().filter_map(|i| i.0).collect())
}

/// Comme [`de_items`], mais `None` si la liste est absente, `null` ou n'est
/// pas une liste (MESSAGE_UPDATE partiel : rien ne change).
pub(super) fn de_opt_items<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Option<Vec<T>>, D::Error> {
    let items: Option<Vec<Item<T>>> = lenient(d)?;
    Ok(items.map(|v| v.into_iter().filter_map(|i| i.0).collect()))
}

/// Dimension en pixels : 0 si absente, négative ou démesurée.
fn dim(v: Option<i64>) -> u32 {
    v.and_then(|n| u32::try_from(n).ok()).unwrap_or(0)
}

#[derive(Deserialize)]
pub(super) struct RawAttachment {
    #[serde(default, deserialize_with = "lenient")]
    id: Option<String>,
    filename: String,
    url: String,
    #[serde(default, deserialize_with = "lenient")]
    proxy_url: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    size: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    content_type: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    width: Option<i64>,
    #[serde(default, deserialize_with = "lenient")]
    height: Option<i64>,
    #[serde(default, deserialize_with = "lenient")]
    description: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    flags: Option<u64>,
    /// Message vocal (docs.discord.food, « Attachment Object »).
    #[serde(default, deserialize_with = "lenient")]
    duration_secs: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    waveform: Option<String>,
}

impl RawAttachment {
    pub fn into_attachment(self) -> Attachment {
        let voice = crate::state::voice_msg::VoiceNote::parse(self.duration_secs, self.waveform.as_deref());
        let a = Attachment {
            id: self.id.and_then(|s| s.parse().ok()).unwrap_or(0),
            name: self.filename,
            url: self.url,
            proxy_url: self.proxy_url.unwrap_or_default(),
            size: self.size.unwrap_or(0),
            content_type: self.content_type,
            width: dim(self.width),
            height: dim(self.height),
            description: self.description.filter(|d| !d.trim().is_empty()),
            ..Attachment::default()
        }
        .analysed(self.flags.unwrap_or(0));
        // Les champs ne valent que pour un son (« Content-Type must begin with audio/ »).
        Attachment { voice: voice.filter(|_| a.kind == crate::state::model::MediaKind::Audio), ..a }
    }
}

/// Image, vignette ou vidéo d'un embed.
#[derive(Deserialize)]
struct RawMedia {
    #[serde(default, deserialize_with = "lenient")]
    url: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    proxy_url: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    width: Option<i64>,
    #[serde(default, deserialize_with = "lenient")]
    height: Option<i64>,
    #[serde(default, deserialize_with = "lenient")]
    flags: Option<u64>,
}

impl RawMedia {
    fn into_media(self) -> Option<EmbedMedia> {
        let proxy = self.proxy_url.filter(|u| !u.is_empty());
        let url = self.url.filter(|u| !u.is_empty()).or_else(|| proxy.clone())?;
        Some(EmbedMedia::new(url, proxy, dim(self.width), dim(self.height), self.flags.unwrap_or(0)))
    }
}

/// Icône d'auteur ou de pied.
fn icon(url: Option<String>, proxy: Option<String>) -> Option<EmbedMedia> {
    RawMedia { url, proxy_url: proxy, width: None, height: None, flags: None }.into_media()
}

#[derive(Deserialize)]
struct RawAuthor {
    name: String,
    #[serde(default, deserialize_with = "lenient")]
    url: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    icon_url: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    proxy_icon_url: Option<String>,
}

#[derive(Deserialize)]
struct RawFooter {
    text: String,
    #[serde(default, deserialize_with = "lenient")]
    icon_url: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    proxy_icon_url: Option<String>,
}

#[derive(Deserialize)]
struct RawField {
    name: String,
    value: String,
    #[serde(default, deserialize_with = "lenient")]
    inline: Option<bool>,
}

#[derive(Deserialize)]
struct RawProvider {
    #[serde(default, deserialize_with = "lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    url: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct RawEmbed {
    #[serde(default, rename = "type", deserialize_with = "lenient")]
    kind: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    title: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    description: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    url: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    timestamp: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    color: Option<u32>,
    #[serde(default, deserialize_with = "lenient")]
    footer: Option<RawFooter>,
    #[serde(default, deserialize_with = "lenient")]
    image: Option<RawMedia>,
    #[serde(default, deserialize_with = "lenient")]
    thumbnail: Option<RawMedia>,
    #[serde(default, deserialize_with = "lenient")]
    video: Option<RawMedia>,
    #[serde(default, deserialize_with = "lenient")]
    provider: Option<RawProvider>,
    #[serde(default, deserialize_with = "lenient")]
    author: Option<RawAuthor>,
    #[serde(default, deserialize_with = "de_items")]
    fields: Vec<RawField>,
}

fn nonempty(s: Option<String>) -> Option<String> {
    s.filter(|s| !s.trim().is_empty())
}

impl RawEmbed {
    /// `None` : rien à afficher.
    pub fn into_embed(self) -> Option<Embed> {
        let e = Embed {
            kind: EmbedKind::parse(self.kind.as_deref()),
            title: nonempty(self.title),
            url: nonempty(self.url),
            description: nonempty(self.description),
            color: self.color.map(|c| c & 0xFF_FFFF),
            timestamp: self.timestamp.and_then(|t| chrono::DateTime::parse_from_rfc3339(&t).ok()).map(|t| t.timestamp_millis()),
            author: self.author.filter(|a| !a.name.trim().is_empty()).map(|a| EmbedAuthor { name: a.name, url: nonempty(a.url), icon: icon(a.icon_url, a.proxy_icon_url) }),
            provider: self.provider.and_then(|p| Some(EmbedProvider { name: nonempty(p.name)?, url: nonempty(p.url) })),
            fields: self.fields.into_iter().map(|f| EmbedField { name: f.name, value: f.value, inline: f.inline.unwrap_or(false) }).collect(),
            image: self.image.and_then(RawMedia::into_media),
            thumbnail: self.thumbnail.and_then(RawMedia::into_media),
            video: self.video.and_then(RawMedia::into_media),
            footer: self.footer.filter(|f| !f.text.trim().is_empty()).map(|f| EmbedFooter { text: f.text, icon: icon(f.icon_url, f.proxy_icon_url) }),
        };
        (!e.is_empty()).then_some(e)
    }
}

pub(super) fn attachments(raw: Vec<RawAttachment>) -> Vec<Attachment> {
    raw.into_iter().map(RawAttachment::into_attachment).collect()
}

pub(super) fn embeds(raw: Vec<RawEmbed>) -> Vec<Embed> {
    raw.into_iter().filter_map(RawEmbed::into_embed).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::MediaKind;

    #[derive(Deserialize)]
    struct Msg {
        #[serde(default, deserialize_with = "de_items")]
        attachments: Vec<RawAttachment>,
        #[serde(default, deserialize_with = "de_items")]
        embeds: Vec<RawEmbed>,
    }

    fn read(json: &str) -> (Vec<Attachment>, Vec<Embed>) {
        let m: Msg = serde_json::from_str(json).unwrap_or_else(|e| panic!("{e}"));
        (attachments(m.attachments), embeds(m.embeds))
    }

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"))).unwrap_or_else(|e| panic!("{e}"));
        v["d"].to_string()
    }

    #[test]
    fn attachment_fixture() {
        let (a, _) = read(&fixture("message_create_attachment"));
        assert_eq!(a.len(), 2);
        assert_eq!((a[0].id, a[0].kind, a[0].width, a[0].height, a[0].size), (7000000000000000001, MediaKind::Image, 640, 480, 12345));
        assert!(a[0].proxy_url.starts_with("https://media.discordapp.net/attachments/"));
        assert_eq!(a[0].fetch_url(), a[0].proxy_url);
        assert_eq!((a[1].kind, a[1].width), (MediaKind::File, 0));
    }

    #[test]
    fn voice_message_fixture() {
        let (a, _) = read(&fixture("message_create_voice"));
        let v = a[0].voice.as_ref().unwrap_or_else(|| panic!("{:?}", a[0]));
        assert_eq!((a[0].kind, v.duration_ms, v.waveform.len()), (MediaKind::Audio, 5400, 54));
        assert_eq!(&v.waveform[..2], &[20, 51]);
        // Durée et onde sur autre chose qu'un son : ignorées.
        let (a, _) = read(r#"{"attachments":[{"filename":"a.png","url":"u","content_type":"image/png","duration_secs":2,"waveform":"AA=="}]}"#);
        assert_eq!(a[0].voice, None);
        // Champs illisibles : message vocal sans onde, rien ne casse.
        let (a, _) = read(r#"{"attachments":[{"filename":"v.ogg","url":"u","content_type":"audio/ogg","duration_secs":"x","waveform":5}]}"#);
        assert_eq!(a[0].voice, None);
    }

    #[test]
    fn media_fixture_full_shape() {
        let (a, e) = read(&fixture("message_create_media"));
        let kinds: Vec<MediaKind> = a.iter().map(|a| a.kind).collect();
        assert_eq!(kinds, vec![MediaKind::Image, MediaKind::Image, MediaKind::Image, MediaKind::Video, MediaKind::Audio, MediaKind::File]);
        assert!(a[1].spoiler && a[1].description.as_deref() == Some("Texte alternatif fictif"));
        assert!(a[2].animated && !a[2].spoiler);
        let rich = &e[0];
        assert_eq!(rich.kind, EmbedKind::Rich);
        assert_eq!(rich.color, Some(0x5865F2));
        assert_eq!(rich.author.as_ref().map(|a| (a.name.as_str(), a.icon.is_some())), Some(("Auteur fictif", true)));
        assert_eq!(rich.fields.len(), 3);
        assert!(rich.fields[0].inline && !rich.fields[2].inline);
        assert_eq!(rich.timestamp, Some(1_736_935_200_000));
        assert!(rich.image.as_ref().is_some_and(|i| i.proxy_url.is_some() && i.width == 400));
        assert_eq!(rich.footer.as_ref().map(|f| f.text.as_str()), Some("Pied fictif"));
        let video = &e[1];
        assert_eq!(video.kind, EmbedKind::Video);
        assert!(video.thumbnail.is_some() && video.video.is_some());
        assert_eq!(e[2].kind, EmbedKind::Gifv);
        assert!(e[2].is_bare_media());
    }

    /// Champs de forme inattendue : ignorés un à un, le reste est gardé.
    #[test]
    fn malformed_parts_are_skipped() {
        let (a, e) = read(
            r#"{
                "attachments": [
                    { "id": "1", "filename": "a.png", "url": "u", "width": "large", "height": -4, "size": "x", "flags": null },
                    { "filename": 3 },
                    "pas un objet"
                ],
                "embeds": [
                    { "title": "T", "color": "rouge", "timestamp": "hier", "fields": [ { "name": "n", "value": "v" }, { "name": 1 } ],
                      "image": { "proxy_url": "https://media.discordapp.net/x.png" }, "thumbnail": { "width": 3 },
                      "author": { "url": "sans nom" }, "provider": {}, "footer": { "text": "" }, "inconnu": [1, 2] },
                    { "type": "rich" },
                    { "type": "poll_result", "description": "D" },
                    42
                ]
            }"#,
        );
        assert_eq!(a.len(), 1);
        assert_eq!((a[0].width, a[0].height, a[0].size, a[0].kind), (0, 0, 0, MediaKind::Image));
        assert_eq!(e.len(), 2, "l'embed vide et l'élément illisible sont ignorés");
        let t = &e[0];
        assert_eq!((t.color, t.timestamp), (None, None));
        assert_eq!(t.fields.len(), 1);
        assert!(t.image.is_some() && t.thumbnail.is_none() && t.author.is_none() && t.provider.is_none() && t.footer.is_none());
        assert_eq!(e[1].kind, EmbedKind::Other);
    }
}
