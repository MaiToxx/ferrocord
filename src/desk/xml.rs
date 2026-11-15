//! Contenu XML d'une notification Windows (schéma « ToastGeneric »,
//! learn.microsoft.com, « Toast content schema »). Le texte du message
//! vient d'autrui : il est échappé, jamais interprété.

use crate::state::notify::{Alert, Kind};

/// Échappe `s` pour un texte ou un attribut XML ; retire les caractères
/// de contrôle que XML 1.0 refuse (le document entier serait rejeté).
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(' '),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

/// Document d'une notification : auteur, texte, lieu ; avatar s'il y a un
/// fichier (`file:///…`). Toujours silencieuse : Ferrocord joue ses sons.
///
/// Un en-tête (`header`) range les notifications par serveur dans le centre
/// de notifications ; le lieu ne répète alors pas le nom du serveur. Un
/// appel entrant reste affiché plus longtemps (`duration="long"`).
pub fn toast(a: &Alert, avatar: Option<&str>) -> String {
    let mut x = String::from(if a.kind == Kind::Ring { "<toast duration=\"long\">" } else { "<toast>" });
    if let Some(h) = a.header.as_deref().filter(|h| !h.is_empty()) {
        let id = a.guild.map_or_else(|| "mp".to_string(), |g| format!("g{g}"));
        x.push_str(&format!("<header id=\"{id}\" title=\"{}\" arguments=\"{id}\"/>", escape(h)));
    }
    x.push_str("<visual><binding template=\"ToastGeneric\">");
    x.push_str(&format!("<text>{}</text>", escape(&a.title)));
    if !a.body.is_empty() {
        x.push_str(&format!("<text>{}</text>", escape(&a.body)));
    }
    // Sous un en-tête de serveur, « Serveur › #salon » se réduit à « #salon ».
    let place = a.place.as_deref().map(|p| a.header.as_deref().and_then(|h| p.strip_prefix(h)?.strip_prefix(" › ")).unwrap_or(p));
    if let Some(p) = place {
        x.push_str(&format!("<text placement=\"attribution\">{}</text>", escape(p)));
    }
    if let Some(src) = avatar {
        x.push_str(&format!("<image placement=\"appLogoOverride\" hint-crop=\"circle\" src=\"{}\"/>", escape(src)));
    }
    x.push_str("</binding></visual><audio silent=\"true\"/></toast>");
    x
}

/// Adresse `file:///` d'un chemin local (barres obliques, espaces encodés).
pub fn file_uri(path: &std::path::Path) -> String {
    let p = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file:///");
    for c in p.trim_start_matches('/').chars() {
        match c {
            ' ' => out.push_str("%20"),
            '#' => out.push_str("%23"),
            '%' => out.push_str("%25"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert(title: &str, body: &str, place: Option<&str>) -> Alert {
        Alert { guild: None, channel: 1, kind: Kind::Message, title: title.into(), place: place.map(Into::into), body: body.into(), avatar: None, header: None }
    }

    #[test]
    fn hostile_text_is_escaped() {
        let x = toast(&alert("<b>Ève</b>", "a & b \"c\" 'd' \u{1}</text><text>pirate", Some("S › #x")), None);
        assert_eq!(
            x,
            "<toast><visual><binding template=\"ToastGeneric\"><text>&lt;b&gt;Ève&lt;/b&gt;</text><text>a &amp; b &quot;c&quot; &apos;d&apos; &lt;/text&gt;&lt;text&gt;pirate</text><text placement=\"attribution\">S › #x</text></binding></visual><audio silent=\"true\"/></toast>"
        );
        assert_eq!(x.matches("<text").count(), 3);
    }

    #[test]
    fn header_groups_by_server_and_ring_lasts() {
        let mut a = alert("Bob", "salut", Some("A & B › #général"));
        a.guild = Some(9);
        a.header = Some("A & B".into());
        let x = toast(&a, None);
        assert!(x.starts_with("<toast><header id=\"g9\" title=\"A &amp; B\" arguments=\"g9\"/><visual>"), "{x}");
        assert!(x.contains("<text placement=\"attribution\">#général</text>"), "{x}");
        let mut ring = alert("Bob", "Appel entrant", None);
        ring.kind = Kind::Ring;
        ring.header = Some("Messages privés".into());
        let x = toast(&ring, None);
        assert!(x.starts_with("<toast duration=\"long\"><header id=\"mp\" title=\"Messages privés\""), "{x}");
    }

    #[test]
    fn avatar_and_empty_body() {
        let x = toast(&alert("A", "", None), Some("file:///C:/x%20y/a.png"));
        assert!(x.contains("<text>A</text><image placement=\"appLogoOverride\" hint-crop=\"circle\" src=\"file:///C:/x%20y/a.png\"/>"), "{x}");
        assert_eq!(file_uri(std::path::Path::new(r"C:\Users\A B\c#1.png")), "file:///C:/Users/A%20B/c%231.png");
    }
}
