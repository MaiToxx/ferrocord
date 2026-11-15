//! Textes du sélecteur d'emojis et des autocollants (lot T4). Module enfant
//! de `table` (il en reprend les macros), tenu à part pour les fusions.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        SEARCH = "Rechercher un emoji", "Find the perfect emoji";
        SEARCH_STICKER = "Rechercher un autocollant", "Find a sticker";
        TAB_EMOJIS = "Emojis", "Emoji";
        TAB_STICKERS = "Autocollants", "Stickers";
        RECENT = "Récents", "Frequently used";
        RESULTS = "Résultats", "Search results";
        NONE_FOUND = "Aucun emoji ne correspond.", "No emoji match.";
        NO_STICKERS = "Aucun autocollant ne correspond.", "No sticker match.";
        CAT_PEOPLE = "Personnes", "People";
        CAT_NATURE = "Nature", "Nature";
        CAT_FOOD = "Nourriture", "Food";
        CAT_ACTIVITY = "Activités", "Activities";
        CAT_TRAVEL = "Voyages et lieux", "Travel";
        CAT_OBJECTS = "Objets", "Objects";
        CAT_SYMBOLS = "Symboles", "Symbols";
        CAT_FLAGS = "Drapeaux", "Flags";
        TONE = "Teinte de peau", "Skin tone";
        OPEN_PICKER = "Emojis et autocollants", "Emoji and stickers";
        PICK_TIP = "Maj+clic\u{a0}: en choisir plusieurs", "Shift+click: pick several";
        ANIMATED_STICKER = "Autocollant animé (aperçu non pris en charge)", "Animated sticker (no preview)";
        ATTRIBUTION = "Emojis", "Emoji";
        ATTRIBUTION_TIP = "Images des emojis\u{a0}: Twemoji (jdecked/twemoji), © Twitter, Inc. et contributeurs, licence CC-BY 4.0. Téléchargées à la demande depuis cdn.jsdelivr.net et gardées dans le cache d'images. Noms des emojis\u{a0}: paquet discord-emoji (licence MIT).", "Emoji images: Twemoji (jdecked/twemoji), © Twitter, Inc. and contributors, licensed under CC-BY 4.0. Downloaded on demand from cdn.jsdelivr.net and kept in the image cache. Emoji names: discord-emoji package (MIT license).";
    }
}
