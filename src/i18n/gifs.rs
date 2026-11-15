//! Textes du sélecteur de GIF, des favoris et des super réactions (lot G).
//! Module enfant de `table` (il en reprend les macros), tenu à part pour les fusions.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        TAB_GIFS = "GIF", "GIFs";
        SEARCH = "Rechercher sur Tenor", "Search Tenor";
        OPEN_GIFS = "Ouvrir le sélecteur de GIF", "Open GIF picker";
        OPEN_STICKERS = "Ouvrir le sélecteur d'autocollants", "Open sticker picker";
        FAVORITES = "Favoris", "Favorites";
        TRENDING = "GIF tendance", "Trending GIFs";
        BACK = "Retour", "Back";
        NONE_FOUND = "Aucun GIF ne correspond.", "No GIFs match.";
        NO_FAVORITES = "Aucun GIF favori pour l'instant. Survolez un GIF et cliquez sur l'étoile pour le garder ici.", "No favorite GIFs yet. Hover a GIF and click the star to keep it here.";
        USER_ONLY = "Les GIF ne sont pas proposés aux comptes de bot.", "GIFs are not available to bot accounts.";
        RETRY = "Réessayer", "Retry";
        ADD_FAVORITE = "Ajouter aux favoris", "Add to favorites";
        REMOVE_FAVORITE = "Retirer des favoris", "Remove from favorites";
        FAVORITES_UNKNOWN = "Favoris du compte pas encore reçus\u{a0}: réessayez dans un instant.", "Account favorites not received yet: try again in a moment.";
        FAVORITES_BUSY = "Un favori est déjà en cours d'enregistrement.", "A favorite is already being saved.";
        VIDEO_FAVORITE = "GIF vidéo (aperçu non pris en charge)", "Video GIF (no preview)";
        SEND_GIF = "Envoyer ce GIF", "Send this GIF";
        EMOJI_FAVORITES = "Favoris", "Favorites";
        FOLD = "Replier la section", "Collapse section";
        UNFOLD = "Déplier la section", "Expand section";
        SUPER = "Super réaction", "Super reaction";
        SUPER_TIP = "Envoyer l'emoji choisi en super réaction (Nitro)", "Send the picked emoji as a super reaction (Nitro)";
        BANNER_TIP = "Maj+clic\u{a0}: en choisir plusieurs · clic droit\u{a0}: favoris", "Shift+click: pick several · right-click: favorites";
        UNICODE_ORIGIN = "Emoji standard", "Standard emoji";
    }
}
