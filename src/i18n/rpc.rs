//! Textes de la Rich Presence locale (lot R).
//! Module enfant de `table` (il en reprend les macros), tenu à part pour la fusion.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        SECTION = "Rich Presence des jeux et logiciels", "Rich Presence from games and apps";
        TOGGLE = "Accepter la Rich Presence des programmes de cet ordinateur", "Accept Rich Presence from programs on this computer";
        TOGGLE_TIP = "Les jeux et logiciels qui savent parler à Discord (tube local discord-ipc) peuvent afficher leur activité dans votre profil. Seule l'activité est acceptée\u{a0}: aucune autre commande n'agit sur votre compte.", "Games and apps that talk to Discord (local discord-ipc pipe) can show their activity on your profile. Only the activity is accepted: no other command can act on your account.";
        OFF = "Coupée\u{a0}: aucun programme ne peut se brancher.", "Off: no program can connect.";
        WAITING = "Ouverture du tube local…", "Opening the local pipe…";
        NO_PIPE = "Aucun tube libre (discord-ipc-0 à 9 pris)\u{a0}: nouvel essai dans une demi-minute.", "No free pipe (discord-ipc-0 to 9 taken): retrying in half a minute.";
        OFFICIAL_FIRST = "Les programmes s'adressent au premier tube\u{a0}: si le client officiel tourne aussi, c'est lui qui les reçoit.", "Programs use the first pipe: if the official client is also running, it receives them.";
        UNSUPPORTED = "Rich Presence locale indisponible sur ce système.", "Local Rich Presence is unavailable on this system.";
        SHARING_OFF = "«\u{a0}Afficher l'activité en cours\u{a0}» est coupé\u{a0}: les activités reçues ne sont montrées à personne.", "“Share your activity” is off: received activities are shown to nobody.";
        NONE = "Aucune application branchée.", "No app connected.";
        NO_ACTIVITY = "pas d'activité", "no activity";
        SHOWN = "montrée", "shown";
        IMAGES_TIP = "Une image désignée par son nom n'est transmise qu'après «\u{a0}Charger les images\u{a0}» (une requête à Discord, pour la liste des images de l'application). Les images données par une adresse ne sont pas transmises.", "An image given by name is only sent after “Load images” (one request to Discord, for the app's image list). Images given by address are not sent.";
        ASSETS_LOAD = "Charger les images", "Load images";
        ASSETS_LOAD_TIP = "Demande à Discord la liste des images de cette application, pour transmettre celles qu'elle désigne par leur nom.", "Asks Discord for this app's image list, to send the images it gives by name.";
        ASSETS_LOADING = "chargement des images…", "loading images…";
        ASSETS_FAILED = "images indisponibles", "images unavailable";
        ASSETS_UNKNOWN = "image inconnue de l'application", "image unknown to the app";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        listening(pipe) = "Prête sur le tube {pipe}.", "Ready on pipe {pipe}.";
        connected_since(time) = "branchée depuis {time}", "connected for {time}";
    }
}
