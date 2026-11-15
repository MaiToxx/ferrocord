//! Commandes intégrées de la zone de saisie (F10) et messages `@silent`
//! (F9), comme le client de bureau : `/shrug`, `/tableflip`, `/unflip`,
//! `/me`, `/spoiler` transforment le texte avant l'envoi ; `/nick` change
//! le pseudo sur le serveur ; `/tts` et `@silent` restent en tête du texte
//! jusqu'à la requête, où ils deviennent `tts: true` et le drapeau
//! `SUPPRESS_NOTIFICATIONS` de Create Message (tous deux documentés).
//! Garder ces deux préfixes dans le texte fait qu'un nouvel essai, une
//! réponse ou un envoi avec fichiers se comportent de la même façon.

/// Drapeau de message `SUPPRESS_NOTIFICATIONS` (documentation officielle,
/// « Message Flags », 1 << 12) : n'envoie ni notification poussée ni de bureau.
pub const SUPPRESS_NOTIFICATIONS: u64 = 1 << 12;

/// Texte envoyé par `/shrug` : barres obliques inverses doublées, pour que
/// la mise en forme n'en fasse pas de l'italique.
pub const SHRUG: &str = "¯\\\\_(ツ)\\_/¯";
pub const TABLEFLIP: &str = "(╯°□°)╯︵ ┻━┻";
pub const UNFLIP: &str = "┬─┬ノ( º _ ºノ)";

/// Noms des commandes intégrées, pour la liste « / ».
pub const LIST: [&str; 7] = ["shrug", "tableflip", "unflip", "me", "spoiler", "tts", "nick"];

/// Effet d'une commande intégrée.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Builtin {
    /// Le message part avec ce texte.
    Text(String),
    /// Changer mon pseudo sur le serveur (`None` : le retirer).
    Nick(Option<String>),
}

/// `/nom reste` : le nom et le reste (sans espaces autour).
fn split(text: &str) -> Option<(&str, &str)> {
    let body = text.strip_prefix('/')?;
    let (name, rest) = body.split_once(char::is_whitespace).unwrap_or((body, ""));
    Some((name, rest.trim()))
}

/// Commande intégrée en tête de `text`, s'il y en a une que l'interface
/// traite elle-même (`/tts` reste dans le texte, voir [`send_opts`]).
pub fn apply(text: &str) -> Option<Builtin> {
    let (name, rest) = split(text)?;
    let after = |art: &str| if rest.is_empty() { art.to_string() } else { format!("{rest} {art}") };
    Some(match name {
        "shrug" => Builtin::Text(after(SHRUG)),
        "tableflip" => Builtin::Text(after(TABLEFLIP)),
        "unflip" => Builtin::Text(after(UNFLIP)),
        "me" if !rest.is_empty() => Builtin::Text(format!("_{rest}_")),
        "spoiler" if !rest.is_empty() => Builtin::Text(format!("||{rest}||")),
        "nick" => Builtin::Nick((!rest.is_empty()).then(|| rest.to_string())),
        _ => return None,
    })
}

/// Ce que la requête d'envoi fait d'un texte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SendOpts<'a> {
    /// Texte réellement envoyé.
    pub text: &'a str,
    pub tts: bool,
    pub silent: bool,
}

/// Préfixes `@silent` et `/tts` d'un texte à envoyer.
pub fn send_opts(content: &str) -> SendOpts<'_> {
    let strip = |s: &'_ str, word: &str| -> Option<usize> {
        let rest = s.strip_prefix(word)?;
        (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| s.len() - rest.trim_start().len())
    };
    let mut text = content;
    let silent = strip(text, "@silent").map(|n| text = &text[n..]).is_some();
    // `/tts` seul n'est pas une commande : il part tel quel.
    let tts = strip(text, "/tts").filter(|n| *n < text.len()).map(|n| text = &text[n..]).is_some();
    SendOpts { text, tts, silent }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_commands_rewrite_the_message() {
        assert_eq!(apply("/shrug"), Some(Builtin::Text("¯\\\\_(ツ)\\_/¯".into())));
        assert_eq!(apply("/shrug tant pis"), Some(Builtin::Text("tant pis ¯\\\\_(ツ)\\_/¯".into())));
        assert_eq!(apply("/tableflip  non "), Some(Builtin::Text("non (╯°□°)╯︵ ┻━┻".into())));
        assert_eq!(apply("/unflip"), Some(Builtin::Text("┬─┬ノ( º _ ºノ)".into())));
        assert_eq!(apply("/me danse"), Some(Builtin::Text("_danse_".into())));
        assert_eq!(apply("/spoiler la fin"), Some(Builtin::Text("||la fin||".into())));
        assert_eq!(apply("/nick  Zoé "), Some(Builtin::Nick(Some("Zoé".into()))));
        assert_eq!(apply("/nick"), Some(Builtin::Nick(None)));
    }

    #[test]
    fn everything_else_is_plain_text() {
        for t in ["/me", "/spoiler ", "/tts bonjour", "/inconnue x", "pas /shrug", "/shrugs", "//shrug", ""] {
            assert_eq!(apply(t), None, "{t}");
        }
    }

    #[test]
    fn silent_and_tts_prefixes_become_send_options() {
        let o = |text, tts, silent| SendOpts { text, tts, silent };
        assert_eq!(send_opts("bonjour"), o("bonjour", false, false));
        assert_eq!(send_opts("@silent bonjour"), o("bonjour", false, true));
        assert_eq!(send_opts("@silent"), o("", false, true));
        assert_eq!(send_opts("@silently"), o("@silently", false, false));
        assert_eq!(send_opts("/tts  à voix haute"), o("à voix haute", true, false));
        assert_eq!(send_opts("@silent /tts x"), o("x", true, true));
        assert_eq!(send_opts("/tts"), o("/tts", false, false));
        assert_eq!(send_opts("un @silent au milieu"), o("un @silent au milieu", false, false));
    }
}
