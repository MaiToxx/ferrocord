//! Le jeton Discord, enveloppé : ni `Debug` ni `Display` n'en montrent quoi
//! que ce soit (pas même sa longueur). Seul [`Token::expose`] donne le texte,
//! et il n'est appelé qu'aux endroits où le jeton doit vraiment partir : la
//! connexion réseau et le coffre.

use crate::i18n::{language, verbatim::t as vt};
use std::fmt;

#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

/// Texte refusé comme jeton (le message ne contient rien du texte).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenError {
    /// Rien d'autre que des espaces.
    Empty,
    /// Caractère qu'un jeton ne contient jamais.
    Invalid(&'static str),
}

impl fmt::Display for TokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenError::Empty => f.write_str(vt::TOKEN_EMPTY.get(language())),
            TokenError::Invalid(why) => f.write_str(why),
        }
    }
}

/// Préfixe d'un jeton de bot (seul espace admis dans un jeton).
const BOT: &str = "Bot ";

impl Token {
    /// Jeton saisi ou lu sur disque : marque d'ordre des octets (U+FEFF) en
    /// tête, espaces et retours à la ligne autour retirés. Un jeton Discord
    /// n'est fait que de caractères ASCII visibles (`Bot ` en tête pour un
    /// bot) : tout autre caractère (contrôle, espace interne, caractère
    /// invisible) le fait refuser, pour ne jamais enregistrer un jeton abîmé.
    pub fn parse(text: &str) -> Result<Token, TokenError> {
        let t = text.trim_start_matches('\u{feff}').trim();
        if t.is_empty() {
            return Err(TokenError::Empty);
        }
        let body = t.strip_prefix(BOT).unwrap_or(t);
        if body.is_empty() {
            return Err(TokenError::Empty);
        }
        if let Some(c) = body.chars().find(|c| !c.is_ascii_graphic()) {
            return Err(TokenError::Invalid(if c.is_control() {
                vt::TOKEN_CONTROL.get(language())
            } else if c.is_whitespace() {
                vt::TOKEN_SPACES.get(language())
            } else {
                vt::TOKEN_ODD_CHAR.get(language())
            }));
        }
        Ok(Token(t.to_string()))
    }

    /// Le texte du jeton, pour l'envoyer à Discord ou au coffre.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

/// Tel quel (tests et faux serveur) ; une saisie passe par [`Token::parse`].
impl From<&str> for Token {
    fn from(s: &str) -> Self {
        Token(s.to_string())
    }
}

impl From<String> for Token {
    fn from(s: String) -> Self {
        Token(s)
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(<masqué>)")
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<jeton masqué>")
    }
}

impl Drop for Token {
    /// Remet à zéro les octets de cette copie avant de la rendre. C'est une
    /// précaution, pas une garantie : d'autres copies du jeton ont pu exister
    /// ailleurs en mémoire (réallocations, champ de saisie, pile réseau,
    /// coffre du système) et ne sont pas effacées par ceci.
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

/// Remet à zéro le texte puis le vide (même précaution, même limite que
/// pour [`Token`]).
pub fn wipe(s: &mut String) {
    let mut bytes = std::mem::take(s).into_bytes();
    bytes.fill(0);
    std::hint::black_box(&bytes);
}

/// Un morceau du jeton (4 caractères suffisent) apparaît-il dans `text` ?
#[cfg(test)]
pub(crate) fn leaks(text: &str, secret: &str) -> bool {
    let chars: Vec<char> = secret.chars().collect();
    chars.windows(4).any(|w| text.contains(&w.iter().collect::<String>()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "MTIzNDU2Nzg5.faux-DE-TEST.abcdefghijkl";

    #[test]
    fn debug_and_display_show_nothing() {
        let t = Token::from(SECRET);
        for text in [format!("{t:?}"), format!("{t}"), format!("{:?}", Some(t.clone())), format!("{:#?}", vec![t.clone()])] {
            assert!(!leaks(&text, SECRET), "{text}");
            assert!(!text.contains(&SECRET.len().to_string()), "pas même la longueur : {text}");
        }
        assert_eq!(t.expose(), SECRET);
    }

    #[test]
    fn parse_trims_and_refuses_blank() {
        assert_eq!(Token::parse("  abc\r\n").map(|t| t.expose().to_string()), Ok("abc".into()));
        assert_eq!(Token::parse(" \n\t "), Err(TokenError::Empty));
        assert_eq!(Token::parse(""), Err(TokenError::Empty));
        assert_eq!(Token::parse("\u{feff}\r\n"), Err(TokenError::Empty));
    }

    /// Fichier écrit avec une marque d'ordre des octets (Bloc-notes) : elle
    /// est retirée, le jeton n'est pas abîmé.
    #[test]
    fn parse_strips_byte_order_mark() {
        assert_eq!(Token::parse("\u{feff}MTk4.Gx7Qz.TEST-9qWr\r\n").map(|t| t.expose().to_string()), Ok("MTk4.Gx7Qz.TEST-9qWr".into()));
    }

    #[test]
    fn parse_refuses_damaged_tokens_with_a_clear_message() {
        for (text, what) in [
            ("MTk4.Gx7Qz\u{7}.TEST", "contrôle"),
            ("MTk4.Gx7Qz\0", "contrôle"),
            ("MTk4.Gx 7Qz.TEST", "espaces"),
            ("MTk4.Gx7Qz\t.TEST", "contrôle"),
            ("MTk4\u{200b}.Gx7Qz.TEST", "invisible"),
            ("\u{feff}MTk4.\u{feff}Gx7Qz", "invisible"),
            ("Bot  MTk4.Gx7Qz", "espaces"),
        ] {
            match Token::parse(text) {
                Err(TokenError::Invalid(why)) => {
                    assert!(why.contains(what), "{text:?} : {why}");
                    assert!(!leaks(why, "MTk4.Gx7Qz.TEST"), "{why}");
                }
                other => panic!("{text:?} : refus attendu, reçu {other:?}"),
            }
        }
        // Un jeton de bot garde son préfixe (seul espace admis).
        assert_eq!(Token::parse(" Bot MTk4.Gx7Qz\n").map(|t| t.expose().to_string()), Ok("Bot MTk4.Gx7Qz".into()));
    }

    #[test]
    fn wipe_empties_the_string() {
        let mut s = String::from(SECRET);
        wipe(&mut s);
        assert!(s.is_empty());
    }
}
