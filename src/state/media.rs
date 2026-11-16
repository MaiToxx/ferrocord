//! Pièces jointes et embeds d'un message, tels qu'affichés : analysés une
//! fois à la réception (genre de fichier, spoiler, animation, horodatage),
//! jamais à chaque image. Aucune adresse n'est construite ici : `net::cdn`
//! ajoute les paramètres de taille au moment de la demande.

use super::image::ImageRef;

/// Ce qu'une pièce jointe contient, d'après son type MIME (ou, à défaut,
/// l'extension du nom) : décide de la vignette ou de la carte.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MediaKind {
    Image,
    Video,
    Audio,
    #[default]
    File,
}

impl MediaKind {
    /// Genre d'après le type MIME, sinon d'après l'extension.
    pub fn classify(content_type: Option<&str>, name: &str) -> (MediaKind, bool) {
        let mime = content_type.map(|c| c.split(';').next().unwrap_or("").trim().to_ascii_lowercase());
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        let gif = mime.as_deref() == Some("image/gif") || (mime.is_none() && ext == "gif");
        let kind = match mime.as_deref() {
            // Seuls les formats que le décodeur lit deviennent des vignettes.
            Some("image/png" | "image/jpeg" | "image/webp" | "image/gif") => MediaKind::Image,
            Some(m) if m.starts_with("video/") => MediaKind::Video,
            Some(m) if m.starts_with("audio/") => MediaKind::Audio,
            Some(_) => MediaKind::File,
            None => match ext.as_str() {
                "png" | "jpg" | "jpeg" | "webp" | "gif" => MediaKind::Image,
                "mp4" | "webm" | "mov" | "mkv" => MediaKind::Video,
                "mp3" | "ogg" | "oga" | "opus" | "wav" | "flac" | "m4a" => MediaKind::Audio,
                _ => MediaKind::File,
            },
        };
        (kind, gif)
    }
}

/// Indicateurs documentés d'une pièce jointe (`flags`).
pub const ATTACHMENT_IS_SPOILER: u64 = 1 << 3;
/// Pièce jointe que l'analyse de Discord a marquée « contenu sensible »
/// (`CONTAINS_EXPLICIT_MEDIA`, docs.discord.food, « Attachment Flags » ;
/// champ reçu, aucune requête). Lot FF, F8.
pub const ATTACHMENT_IS_EXPLICIT: u64 = 1 << 4;
/// Même valeur pour une pièce jointe et pour une image d'embed.
pub const MEDIA_IS_ANIMATED: u64 = 1 << 5;

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Attachment {
    /// Identifiant (snowflake) : unique, sert de clé aux images.
    pub id: u64,
    pub name: String,
    /// Fichier original (cdn.discordapp.com).
    pub url: String,
    /// Même fichier par le proxy média (media.discordapp.net), qui sait
    /// le redimensionner. Vide si absent.
    pub proxy_url: String,
    /// Octets.
    pub size: u64,
    pub content_type: Option<String>,
    /// Dimensions en pixels ; 0 si inconnues.
    pub width: u32,
    pub height: u32,
    /// Texte alternatif.
    pub description: Option<String>,
    pub kind: MediaKind,
    /// Masqué jusqu'au clic (nom `SPOILER_…` ou indicateur `IS_SPOILER`).
    pub spoiler: bool,
    /// Marquée « contenu sensible » par Discord : masquée selon le réglage du compte.
    pub explicit: bool,
    /// GIF ou image animée : pastille « GIF » sur la première image.
    pub animated: bool,
    /// Change quand l'adresse est rafraîchie (lot T13) : l'image en échec
    /// sous l'ancienne adresse n'est pas confondue avec la nouvelle.
    pub rev: u32,
    /// Message vocal (`duration_secs`, `waveform`) : lecteur au lieu de la carte.
    pub voice: Option<super::voice_msg::VoiceNote>,
}

impl Attachment {
    /// Pièce jointe réduite à un nom et une adresse (fichier quelconque).
    pub fn file(name: impl Into<String>, url: impl Into<String>) -> Self {
        Attachment { name: name.into(), url: url.into(), ..Attachment::default() }
    }

    /// Complète genre, spoiler et animation d'après les champs reçus.
    pub fn analysed(mut self, flags: u64) -> Self {
        let (kind, gif) = MediaKind::classify(self.content_type.as_deref(), &self.name);
        self.kind = kind;
        self.animated = gif || flags & MEDIA_IS_ANIMATED != 0;
        self.spoiler = self.name.starts_with("SPOILER_") || flags & ATTACHMENT_IS_SPOILER != 0;
        self.explicit = flags & ATTACHMENT_IS_EXPLICIT != 0;
        self
    }

    /// Clé de l'image de cette pièce jointe.
    pub fn image_ref(&self) -> ImageRef {
        ImageRef::Media { owner: self.id, slot: 0, rev: self.rev }
    }

    /// Nouvelle adresse signée (`refreshed`) ; le proxy média reprend sa signature.
    pub fn refresh(&mut self, refreshed: &str) {
        if !self.proxy_url.is_empty() {
            self.proxy_url = super::announce::resign(&self.proxy_url, refreshed).unwrap_or_default();
        }
        self.url = refreshed.to_string();
        self.rev = fnv32(refreshed) | 1;
    }

    /// Adresse à demander (redimensionnable si possible).
    pub fn fetch_url(&self) -> &str {
        if self.proxy_url.is_empty() { &self.url } else { &self.proxy_url }
    }

    /// Nom affiché : sans le préfixe `SPOILER_`.
    pub fn display_name(&self) -> &str {
        self.name.strip_prefix("SPOILER_").filter(|n| !n.is_empty()).unwrap_or(&self.name)
    }
}

/// Type d'embed (champ `type`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EmbedKind {
    #[default]
    Rich,
    Image,
    Video,
    Gifv,
    Article,
    Link,
    /// Type inconnu ou propre à Discord (`poll_result`…) : affiché comme `Rich`.
    Other,
    /// `safety_system_notification` : notification du compte système de
    /// Discord, dont les `fields` portent l'objet en paires nom/valeur
    /// ([`Embed::safety_notice`]).
    SafetyNotification,
}

impl EmbedKind {
    pub fn parse(s: Option<&str>) -> Self {
        match s {
            None | Some("rich") => EmbedKind::Rich,
            Some("image") => EmbedKind::Image,
            Some("video") => EmbedKind::Video,
            Some("gifv") => EmbedKind::Gifv,
            Some("article") => EmbedKind::Article,
            Some("link") => EmbedKind::Link,
            Some("safety_system_notification") => EmbedKind::SafetyNotification,
            Some(_) => EmbedKind::Other,
        }
    }
}

/// Image, vignette, vidéo ou icône d'un embed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct EmbedMedia {
    /// Adresse d'origine (site tiers) : seulement ouverte dans le navigateur.
    pub url: String,
    /// Copie servie par le proxy de Discord : la seule jamais téléchargée.
    /// `None` : l'image n'est pas chargée (on ne contacte pas le site tiers).
    pub proxy_url: Option<String>,
    pub width: u32,
    pub height: u32,
    pub animated: bool,
    /// Empreinte de l'adresse : une image remplacée à la même place
    /// (embed mis à jour) n'est pas confondue avec l'ancienne.
    pub rev: u32,
}

impl EmbedMedia {
    pub fn new(url: String, proxy_url: Option<String>, width: u32, height: u32, flags: u64) -> Self {
        let rev = fnv32(proxy_url.as_deref().unwrap_or(&url));
        let animated = flags & MEDIA_IS_ANIMATED != 0;
        EmbedMedia { url, proxy_url, width, height, animated, rev }
    }
}

/// Empreinte FNV-1a sur 32 bits : stable d'une exécution à l'autre.
fn fnv32(s: &str) -> u32 {
    s.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193))
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct EmbedAuthor {
    pub name: String,
    pub url: Option<String>,
    pub icon: Option<EmbedMedia>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct EmbedFooter {
    pub text: String,
    pub icon: Option<EmbedMedia>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct EmbedField {
    pub name: String,
    pub value: String,
    pub inline: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct EmbedProvider {
    pub name: String,
    pub url: Option<String>,
}

/// Embed (forme documentée de l'objet Embed).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Embed {
    pub kind: EmbedKind,
    pub title: Option<String>,
    pub url: Option<String>,
    pub description: Option<String>,
    /// 0xRRGGBB.
    pub color: Option<u32>,
    /// ms Unix.
    pub timestamp: Option<i64>,
    pub author: Option<EmbedAuthor>,
    pub provider: Option<EmbedProvider>,
    pub fields: Vec<EmbedField>,
    pub image: Option<EmbedMedia>,
    pub thumbnail: Option<EmbedMedia>,
    pub video: Option<EmbedMedia>,
    pub footer: Option<EmbedFooter>,
}

/// Champs connus d'une notification du compte système ([`Embed::safety_notice`]).
const SAFETY_FIELDS: [&str; 9] = ["client_version_message", "classification_id", "body", "icon_type", "header", "timestamp", "theme", "ctas", "learn_more_link"];

/// Ce qu'une notification du compte système montre.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SafetyNotice<'a> {
    pub header: &'a str,
    pub body: &'a str,
    /// Instant de l'incident (ms Unix).
    pub timestamp_ms: Option<i64>,
    /// Adresse de « En savoir plus », telle que reçue (à vérifier avant de l'ouvrir).
    pub learn_more: Option<&'a str>,
    /// `ctas` demande le bouton « En savoir plus ».
    pub learn_more_cta: bool,
}

impl<'a> SafetyNotice<'a> {
    /// Adresse du pied de carte : seulement si `ctas` la demande.
    pub fn link(&self) -> Option<&'a str> {
        self.learn_more.filter(|_| self.learn_more_cta)
    }
}

/// Rôle d'une image dans un embed (fait partie de sa clé).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbedSlot {
    Image = 0,
    Thumbnail = 1,
    Author = 2,
    Footer = 3,
}

impl Embed {
    /// Embed réduit à un titre et une description (tests, banc).
    pub fn text(title: Option<&str>, description: Option<&str>) -> Self {
        Embed { title: title.map(Into::into), description: description.map(Into::into), ..Embed::default() }
    }

    /// Rien à montrer (ni texte, ni image, ni champ).
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.description.is_none()
            && self.author.is_none()
            && self.provider.is_none()
            && self.fields.is_empty()
            && self.image.is_none()
            && self.thumbnail.is_none()
            && self.video.is_none()
            && self.footer.is_none()
    }

    /// Aperçu fait d'un média seul (image, GIF, vidéo d'un lien collé) :
    /// la vignette est montrée en grand, sans cadre de texte.
    pub fn is_bare_media(&self) -> bool {
        matches!(self.kind, EmbedKind::Image | EmbedKind::Gifv) || (self.kind == EmbedKind::Video && self.title.is_none() && self.description.is_none())
    }

    /// Notification du compte système (`type: "safety_system_notification"`),
    /// lue dans les `fields` de l'embed. Forme relevée sur docs.discord.food,
    /// « Safety Hub » › Safety System Notification : `client_version_message`,
    /// `classification_id`, `body`, `icon_type`, `header`, `timestamp` (secondes
    /// Unix, en texte comme toutes les valeurs), `theme`, `ctas` (liste séparée
    /// par des virgules), `learn_more_link` (facultatif).
    ///
    /// `None` (l'embed s'affiche alors comme un embed ordinaire, champs
    /// compris) : autre type, champ inconnu ou en double, ou ni titre ni texte.
    pub fn safety_notice(&self) -> Option<SafetyNotice<'_>> {
        if self.kind != EmbedKind::SafetyNotification {
            return None;
        }
        let mut n = SafetyNotice::default();
        let mut seen = 0u16;
        for f in &self.fields {
            let (bit, value) = (SAFETY_FIELDS.iter().position(|k| *k == f.name)?, f.value.trim());
            if seen & (1 << bit) != 0 {
                return None;
            }
            seen |= 1 << bit;
            match f.name.as_str() {
                "header" => n.header = value,
                "body" => n.body = value,
                // Secondes Unix à virgule ; illisible ou hors plage : pas de date.
                "timestamp" => n.timestamp_ms = value.parse::<f64>().ok().filter(|s| s.is_finite() && (0.0..=4_102_444_800.0).contains(s)).map(|s| (s * 1000.0) as i64),
                "ctas" => n.learn_more_cta = value.split(',').any(|c| c.trim() == "learn_more_link"),
                "learn_more_link" => n.learn_more = Some(value).filter(|v| !v.is_empty()),
                // Technique (message pour vieux clients, identifiant, icône, thème) : jamais montré.
                _ => {}
            }
        }
        (!n.header.is_empty() || !n.body.is_empty()).then_some(n)
    }

    /// Clé de l'image `slot` du `index`-ième embed du message `msg`.
    pub fn image_ref(msg: u64, index: usize, slot: EmbedSlot, media: &EmbedMedia) -> ImageRef {
        let index = u16::try_from(index).unwrap_or(u16::MAX / 4);
        ImageRef::Media { owner: msg, slot: index.saturating_mul(4).saturating_add(1 + slot as u16), rev: media.rev }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_by_mime_then_extension() {
        let c = |m: Option<&str>, n: &str| MediaKind::classify(m, n);
        assert_eq!(c(Some("image/png"), "x.bin"), (MediaKind::Image, false));
        assert_eq!(c(Some("IMAGE/GIF"), "x"), (MediaKind::Image, true));
        assert_eq!(c(Some("image/svg+xml"), "x.svg"), (MediaKind::File, false), "format non décodé : carte");
        assert_eq!(c(Some("video/mp4"), "x.png"), (MediaKind::Video, false));
        assert_eq!(c(Some("audio/ogg; codecs=opus"), "x"), (MediaKind::Audio, false));
        assert_eq!(c(Some("text/plain; charset=utf-8"), "a.png"), (MediaKind::File, false));
        assert_eq!(c(None, "photo.JPG"), (MediaKind::Image, false));
        assert_eq!(c(None, "a.gif"), (MediaKind::Image, true));
        assert_eq!(c(None, "clip.webm"), (MediaKind::Video, false));
        assert_eq!(c(None, "son.mp3"), (MediaKind::Audio, false));
        assert_eq!(c(None, "sans-extension"), (MediaKind::File, false));
    }

    #[test]
    fn spoiler_and_animation() {
        let a = Attachment { name: "SPOILER_chat.png".into(), content_type: Some("image/png".into()), ..Attachment::default() }.analysed(0);
        assert!(a.spoiler && !a.animated);
        assert_eq!(a.display_name(), "chat.png");
        assert!(!a.explicit);
        let c = Attachment { name: "y.png".into(), content_type: Some("image/png".into()), ..Attachment::default() }.analysed(ATTACHMENT_IS_EXPLICIT);
        assert!(c.explicit && !c.spoiler);
        let b = Attachment { name: "x.webp".into(), content_type: Some("image/webp".into()), ..Attachment::default() }.analysed(ATTACHMENT_IS_SPOILER | MEDIA_IS_ANIMATED);
        assert!(b.spoiler && b.animated);
        assert_eq!(Attachment::file("SPOILER_", "u").display_name(), "SPOILER_");
    }

    /// Notification synthétique du compte système, à la forme documentée.
    fn notice(fields: &[(&str, &str)]) -> Embed {
        Embed {
            kind: EmbedKind::parse(Some("safety_system_notification")),
            title: Some("Message fictif concernant le compte".into()),
            fields: fields.iter().map(|(n, v)| EmbedField { name: (*n).into(), value: (*v).into(), inline: false }).collect(),
            ..Embed::default()
        }
    }

    const NOTICE: [(&str, &str); 9] = [
        ("client_version_message", "Mets l'application à jour pour lire cette notification."),
        ("body", "Texte fictif de la notification."),
        ("icon_type", "default"),
        ("header", "Titre fictif"),
        ("timestamp", "1794767947.766727"),
        ("theme", "default"),
        ("ctas", "learn_more_link"),
        ("learn_more_link", "https://support.discord.com/hc/articles/1"),
        ("classification_id", "1500000000000000001"),
    ];

    #[test]
    fn safety_notice_is_read_from_the_fields() {
        let e = notice(&NOTICE);
        let n = e.safety_notice().unwrap_or_else(|| panic!("notification"));
        assert_eq!((n.header, n.body), ("Titre fictif", "Texte fictif de la notification."));
        assert_eq!(n.timestamp_ms, Some(1_794_767_947_766));
        assert_eq!(n.link(), Some("https://support.discord.com/hc/articles/1"));
        // Ordre quelconque, champs facultatifs absents, date illisible : toujours une carte.
        let e = notice(&[("header", "T"), ("timestamp", "bientôt")]);
        let n = e.safety_notice().unwrap_or_else(|| panic!("titre seul"));
        assert_eq!((n.header, n.body, n.timestamp_ms, n.link()), ("T", "", None, None));
        for bad in ["NaN", "inf", "-1", "1e300", ""] {
            assert_eq!(notice(&[("body", "b"), ("timestamp", bad)]).safety_notice().and_then(|n| n.timestamp_ms), None, "{bad}");
        }
        // Lien présent mais pas demandé par `ctas`, ou demandé sans lien : pas de pied.
        assert_eq!(notice(&[("body", "b"), ("learn_more_link", "https://discord.com/x")]).safety_notice().and_then(|n| n.link()), None);
        assert_eq!(notice(&[("body", "b"), ("ctas", "learn_more_link, policy_violation_detail")]).safety_notice().and_then(|n| n.link()), None);
        assert!(notice(&[("body", "b"), ("ctas", "policy_violation_detail,learn_more_link"), ("learn_more_link", "u")]).safety_notice().is_some_and(|n| n.link() == Some("u")));
    }

    /// Tout ce qui sort de la forme connue garde le rendu d'embed ordinaire :
    /// rien n'est caché, et jamais de carte vide.
    #[test]
    fn unknown_safety_shapes_fall_back_to_the_ordinary_embed() {
        // Autre type, même avec les bons champs.
        for kind in [None, Some("rich"), Some("safety_policy_notice"), Some("age_verification_system_notification"), Some("inconnu")] {
            let e = Embed { kind: EmbedKind::parse(kind), ..notice(&NOTICE) };
            assert_eq!(e.safety_notice(), None, "{kind:?}");
        }
        // Champ inconnu, champ en double, ni titre ni texte, aucun champ.
        let mut extra = NOTICE.to_vec();
        extra.push(("champ_futur", "x"));
        assert_eq!(notice(&extra).safety_notice(), None);
        assert_eq!(notice(&[("header", "a"), ("header", "b")]).safety_notice(), None);
        assert_eq!(notice(&[("header", "  "), ("body", ""), ("theme", "default")]).safety_notice(), None);
        assert_eq!(notice(&[]).safety_notice(), None);
        assert_eq!(notice(&[("Header", "casse différente")]).safety_notice(), None);
    }

    #[test]
    fn embed_image_refs_are_distinct() {
        let m = EmbedMedia::new("https://a.invalid/1.png".into(), None, 1, 1, 0);
        let n = EmbedMedia::new("https://a.invalid/2.png".into(), None, 1, 1, 0);
        let r = |i, s, m: &EmbedMedia| Embed::image_ref(7, i, s, m);
        assert_ne!(r(0, EmbedSlot::Image, &m), r(0, EmbedSlot::Thumbnail, &m));
        assert_ne!(r(0, EmbedSlot::Footer, &m), r(1, EmbedSlot::Image, &m));
        assert_ne!(r(0, EmbedSlot::Image, &m), r(0, EmbedSlot::Image, &n), "image remplacée : autre clé");
        assert_eq!(fnv32(""), 0x811c_9dc5);
    }
}
