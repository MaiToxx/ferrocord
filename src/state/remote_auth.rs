//! Connexion par QR code (lot C2) : ce que la passerelle d'authentification
//! à distance de Discord apprend à l'interface. Le réseau est dans
//! `net::remote_auth` ; l'interface ne voit que ces valeurs.
//!
//! Le jeton obtenu arrive enveloppé ([`Token`]) et suit ensuite exactement
//! le chemin d'un jeton saisi.

use super::image::ImageHash;
use super::Problem;
use crate::account::Token;
use std::sync::mpsc::Receiver;
use std::time::Instant;

/// Adresse codée dans le QR : l'application mobile l'ouvre après lecture.
pub fn qr_url(fingerprint: &str) -> String {
    format!("https://discord.com/ra/{fingerprint}")
}

/// Compte qui a lu le QR sur son téléphone (charge `pending_ticket`
/// déchiffrée : `id:discriminant:avatar:nom`, docs.discord.food,
/// « Remote Authentication › Desktop › User Payload Structure »).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteUser {
    pub id: u64,
    pub discriminator: Option<String>,
    pub avatar: Option<ImageHash>,
    pub name: String,
}

/// Longueur maximale d'un nom affiché (Discord : 2 à 32 caractères).
const MAX_NAME: usize = 32;

impl RemoteUser {
    /// `None` si la charge n'a pas la forme attendue. Le nom (dernier champ)
    /// peut contenir `:` ; un avatar `0` ou illisible est simplement absent.
    pub fn parse(payload: &str) -> Option<Self> {
        let mut parts = payload.splitn(4, ':');
        let id = parts.next()?.parse::<u64>().ok().filter(|id| *id != 0)?;
        let disc = parts.next()?;
        let avatar = parts.next()?;
        let name: String = parts.next()?.chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
        if name.trim().is_empty() {
            return None;
        }
        let discriminator = (!disc.is_empty() && disc != "0" && disc != "0000" && disc.len() <= 4 && disc.bytes().all(|b| b.is_ascii_digit())).then(|| disc.to_string());
        Some(RemoteUser { id, discriminator, avatar: ImageHash::parse(avatar), name })
    }
}

/// Pourquoi la connexion par QR code s'est arrêtée.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteProblem {
    /// Discord exige un captcha : jamais contourné, la connexion est à faire
    /// dans l'application officielle.
    Captcha,
    /// Connexion impossible ou coupée ; détail technique (anglais, tel quel,
    /// jamais de secret).
    Network(String),
    /// Échange mal formé, clé ou empreinte qui ne correspondent pas : rien
    /// n'est affiché ni utilisé.
    Protocol,
    /// Fermeture par la passerelle avec ce code (4001, 4002…).
    Closed(u16),
    /// Refus de l'échange du ticket par l'API.
    Api(Problem),
}

/// Ce qui arrive du réseau, dans l'ordre.
#[derive(Debug)]
pub enum RemoteEvent {
    /// QR à afficher : empreinte de la clé publique, valable jusqu'à `expires`.
    Qr { fingerprint: String, expires: Instant },
    /// Le QR a été lu : confirmation attendue sur le téléphone.
    User(RemoteUser),
    /// Connexion acceptée sur le téléphone, ticket échangé. Dernier événement.
    Token(Token),
    /// Refusée sur le téléphone. Dernier événement.
    Cancelled,
    /// Session expirée sans réponse (code 4003). Dernier événement.
    Expired,
    /// Dernier événement.
    Failed(RemoteProblem),
}

impl RemoteEvent {
    /// Après lui, plus rien n'arrive.
    pub fn is_final(&self) -> bool {
        !matches!(self, RemoteEvent::Qr { .. } | RemoteEvent::User(_))
    }
}

/// Une tentative en cours. La lâcher arrête tout (connexion fermée, clé
/// privée effacée de la mémoire avec la tâche).
pub struct RemoteLogin {
    events: Receiver<RemoteEvent>,
    _guard: Box<dyn Send>,
}

impl RemoteLogin {
    /// `guard` arrête la tâche réseau quand il est lâché.
    pub fn new(events: Receiver<RemoteEvent>, guard: Box<dyn Send>) -> Self {
        Self { events, _guard: guard }
    }

    /// Événements arrivés depuis le dernier appel.
    pub fn poll(&self) -> Vec<RemoteEvent> {
        self.events.try_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_payload_follows_the_documented_shape() {
        let u = RemoteUser::parse("852892297661906993:0:05145cc5646fbcba277b6d5ea2030610:dolfies").unwrap();
        assert_eq!(u.id, 852892297661906993);
        assert_eq!(u.discriminator, None);
        assert_eq!(u.avatar.map(|h| h.to_string()).as_deref(), Some("05145cc5646fbcba277b6d5ea2030610"));
        assert_eq!(u.name, "dolfies");
        let old = RemoteUser::parse("42:1234:0:nom:avec:deux-points").unwrap();
        assert_eq!((old.discriminator.as_deref(), old.avatar, old.name.as_str()), (Some("1234"), None, "nom:avec:deux-points"));
        for bad in ["", "abc:0:0:nom", "0:0:0:nom", "42:0:0", "42:0:0:", "42:0:0: \u{7}"] {
            assert_eq!(RemoteUser::parse(bad), None, "{bad:?}");
        }
        assert_eq!(RemoteUser::parse(&format!("42:0:0:{}", "x".repeat(100))).unwrap().name.len(), MAX_NAME);
    }

    #[test]
    fn qr_points_to_the_remote_auth_page() {
        assert_eq!(qr_url("abc_-9"), "https://discord.com/ra/abc_-9");
    }

    #[test]
    fn only_progress_events_are_not_final() {
        assert!(!RemoteEvent::Qr { fingerprint: String::new(), expires: Instant::now() }.is_final());
        assert!(RemoteEvent::Expired.is_final());
        assert!(RemoteEvent::Failed(RemoteProblem::Protocol).is_final());
    }
}
