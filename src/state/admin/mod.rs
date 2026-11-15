//! Administration des serveurs (phase 3) : un fichier par domaine. Le réseau
//! correspondant est dans `net::admin`, les pages dans `ui::guild_settings`.

pub mod audit;
pub mod automod;
pub mod channels;
// Fonctions du serveur, écran de bienvenue, intégration (lot A7).
pub mod community;
// Événements programmés (lot A7).
pub mod events;
// Emojis, autocollants et sons du serveur (lot A6) ; fichiers envoyés.
pub mod expr_files;
pub mod expressions;
// Widget, modèle de serveur, élagage (groupe 2, lot V).
pub mod extras;
pub mod guild;
pub mod integrations;
pub mod moderation;
pub mod roles;
pub mod webhooks;

/// Ce que les fonctions d'un serveur (`features`) ouvrent : icônes de rôle
/// (`ROLE_ICONS`, niveau 2) et emplacements d'expressions supplémentaires.
/// Une seule structure, décodée une seule fois par [`Boost::from_features`],
/// pour les rôles (lot A3) et les expressions (lot A6) ; le niveau de boost
/// lui-même est `Guild::premium_tier` (seule source).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Boost {
    pub role_icons: bool,
    pub more_emoji: bool,
    pub more_stickers: bool,
    pub more_sounds: bool,
}

impl Boost {
    pub fn from_features<'a>(features: impl IntoIterator<Item = &'a str>) -> Boost {
        let mut b = Boost::default();
        for f in features {
            match f {
                "ROLE_ICONS" => b.role_icons = true,
                "MORE_EMOJI" => b.more_emoji = true,
                "MORE_STICKERS" => b.more_stickers = true,
                "MORE_SOUNDBOARD" => b.more_sounds = true,
                _ => {}
            }
        }
        b
    }
}

#[cfg(test)]
mod boost_tests {
    use super::Boost;

    #[test]
    fn one_decoder_for_roles_and_expressions() {
        let b = Boost::from_features(["ROLE_ICONS", "MORE_STICKERS", "INCONNUE"]);
        assert_eq!(b, Boost { role_icons: true, more_stickers: true, ..Boost::default() });
        assert_eq!(Boost::from_features([]), Boost::default());
    }
}
