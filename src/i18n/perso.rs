//! Textes de la personnalisation du client (lot PERSO).
//! Module enfant de `table`, mêmes macros.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        SEC_PREVIEW = "Aperçu", "Preview";
        SEC_THEMES = "Thèmes", "Themes";
        SEC_COLORS = "Couleurs du thème", "Theme colors";
        SEC_GRADIENT = "Dégradé", "Gradient";
        SEC_BACKDROP = "Image de fond", "Background image";
        SEC_AUTO = "Jour et nuit", "Day and night";
        SEC_FONTS = "Polices", "Fonts";
        SEC_DETAILS = "Détails", "Details";
        SEC_PROFILES = "Profils d'apparence", "Appearance profiles";

        SAVED_THEMES = "Mes thèmes", "My themes";
        NO_SAVED = "Aucun pour l'instant.", "None yet.";
        NAME_HINT = "Nom", "Name";
        SAVE_AS = "Enregistrer sous ce nom", "Save under this name";
        SAVE_THEME_TIP = "Garde le thème en cours (couleurs, tailles, formes) sous ce nom, dans le dossier «\u{a0}themes\u{a0}» à côté de theme.toml.", "Keeps the current theme (colors, sizes, shapes) under this name, in the \u{201c}themes\u{201d} folder next to theme.toml.";
        APPLY = "Appliquer", "Apply";
        DUPLICATE = "Dupliquer", "Duplicate";
        NO_LIBRARY = "Aperçu\u{a0}: rien n'est enregistré sur le disque.", "Preview: nothing is saved to disk.";
        BAD_NAME = "Donnez un nom fait de lettres, de chiffres ou d'espaces.", "Give a name made of letters, digits or spaces.";

        ACCENT = "Couleur d'accent", "Accent color";
        ACCENT_TIP = "Boutons, liens actifs, sélection. Choisissez une pastille ou saisissez un code hexadécimal.", "Buttons, active links, selection. Pick a swatch or type a hex code.";
        HEX_TIP = "Code hexadécimal, par exemple #5865f2", "Hex code, for example #5865f2";
        CONTRAST_OK = "Contraste suffisant partout (WCAG AA).", "Contrast is sufficient everywhere (WCAG AA).";
        CONTRAST_ACCENT = "le texte des boutons d'accent", "accent button text";
        CONTRAST_MENTION = "le texte des mentions", "mention text";

        GRADIENT_ON = "Dégradé sur les fonds", "Gradient on the backgrounds";
        GRADIENT_ON_TIP = "Pose un dégradé de deux ou trois couleurs sur les fonds de l'application. Local et gratuit\u{a0}: rien n'est envoyé à Discord.", "Lays a two or three color gradient over the app's backgrounds. Local and free: nothing is sent to Discord.";
        GRADIENT_READY = "Prêts à l'emploi", "Ready-made";
        GRADIENT_COLORS = "Couleurs", "Colors";
        GRADIENT_THIRD = "Troisième couleur", "Third color";
        GRADIENT_ANGLE = "Angle", "Angle";
        GRADIENT_ANGLE_TIP = "0°\u{a0}: de gauche à droite\u{a0}; 90°\u{a0}: de haut en bas.", "0°: left to right; 90°: top to bottom.";
        STRENGTH = "Intensité", "Strength";
        STRENGTH_TIP = "Part du dégradé sur les fonds. Trop fort, le texte se lit mal.", "Share of the gradient over the backgrounds. Too strong and text gets hard to read.";
        G_CREPUSCULE = "Crépuscule", "Dusk";
        G_LAGON = "Lagon", "Lagoon";
        G_AURORE = "Aurore", "Aurora";
        G_BRAISE = "Braise", "Ember";
        G_FORET = "Forêt", "Forest";
        G_NUIT = "Nuit", "Night";
        G_BONBON = "Bonbon", "Candy";
        G_ARDOISE = "Ardoise", "Slate";

        IMAGE = "Image", "Image";
        IMAGE_TIP = "Image de cet ordinateur (PNG, JPEG ou WebP), affichée derrière l'interface. Elle n'est envoyée nulle part.", "An image from this computer (PNG, JPEG or WebP), shown behind the interface. It is not sent anywhere.";
        IMAGE_CHOOSE = "Choisir une image…", "Choose an image…";
        IMAGE_REMOVE = "Retirer", "Remove";
        IMAGE_NONE = "Aucune image", "No image";
        IMAGE_LOADING = "Chargement de l'image…", "Loading the image…";
        IMAGE_FAILED = "Image de fond illisible\u{a0}: elle a été ignorée.", "The background image could not be read: it was ignored.";
        IMAGE_TITLE = "Image de fond", "Background image";
        SCOPE = "Zone", "Area";
        SCOPE_TIP = "Derrière la discussion seulement, ou derrière toute l'application.", "Behind the conversation only, or behind the whole app.";
        SCOPE_CHAT = "Discussion", "Conversation";
        SCOPE_APP = "Toute l'application", "Whole app";
        OPACITY = "Opacité", "Opacity";
        OPACITY_TIP = "Part de l'image sur les fonds.", "Share of the image over the backgrounds.";
        BLUR = "Flou", "Blur";
        BLUR_TIP = "Calculé une seule fois, au relâchement du curseur\u{a0}: aucun coût ensuite.", "Computed once, when the slider is released: no cost afterwards.";
        DIM = "Assombrissement", "Dimming";
        DIM_TIP = "Assombrit l'image pour garder le texte lisible.", "Darkens the image to keep text readable.";

        AUTO = "Changer de thème tout seul", "Switch themes automatically";
        AUTO_TIP = "À chaque bascule, le thème choisi remplace le thème en cours\u{a0}: enregistrez vos retouches sous un nom pour les retrouver.", "At each switch the chosen theme replaces the current one: save your tweaks under a name to get them back.";
        AUTO_OFF = "Non", "No";
        AUTO_SYSTEM = "Suivre le mode clair ou sombre de Windows", "Follow Windows light or dark mode";
        AUTO_CLOCK = "Selon l'heure", "By time of day";
        DAY_THEME = "Thème du jour", "Day theme";
        NIGHT_THEME = "Thème de la nuit", "Night theme";
        DAY_NIGHT_TIP = "Un préréglage ou l'un de vos thèmes enregistrés.", "A preset or one of your saved themes.";
        DAY_HOUR = "Le jour commence à", "Day starts at";
        NIGHT_HOUR = "La nuit commence à", "Night starts at";
        HOUR_TIP = "Heure de cet ordinateur.", "This computer's time.";
        SYSTEM_UNKNOWN = "Windows n'a pas dit quel mode est actif\u{a0}: le thème ne change pas.", "Windows did not report the active mode: the theme does not change.";

        UI_FONT = "Police de l'interface", "Interface font";
        UI_FONT_TIP = "Une police installée sur cet ordinateur\u{a0}; les caractères qui lui manquent viennent de la police du thème.", "A font installed on this computer; characters it lacks come from the theme's font.";
        CODE_FONT = "Police du code", "Code font";
        CODE_FONT_TIP = "Police à chasse fixe des blocs de code.", "Fixed-width font of code blocks.";
        FONT_THEME = "Celle du thème", "The theme's";
        FONT_CONSOLAS = "Consolas (par défaut)", "Consolas (default)";
        FONTS_LOADING = "Lecture des polices installées…", "Reading installed fonts…";
        FONT_MISSING = "Police introuvable sur cet ordinateur\u{a0}: celle du thème est utilisée.", "Font not found on this computer: the theme's font is used.";

        NAME_COLOR = "Couleur des noms", "Name color";
        NAME_COLOR_TIP = "Dans les messages\u{a0}: le nom prend la couleur du rôle, ou reste en couleur de texte avec un point de la couleur du rôle.", "In messages: the name takes the role color, or stays in text color with a dot in the role color.";
        NAME_ROLE = "Couleur du rôle", "Role color";
        NAME_DOT = "Point de couleur à côté du nom", "Colored dot next to the name";
        HIDE_RAIL = "Masquer la barre des serveurs", "Hide the server bar";
        HIDE_RAIL_TIP = "Gagne de la place\u{a0}; Ctrl+K change toujours de serveur, et ce réglage la fait revenir.", "Saves room; Ctrl+K still switches servers, and this setting brings it back.";
        ANIMATIONS = "Animations", "Animations";
        ANIMATIONS_TIP = "L'intensité des animations se règle dans Accessibilité.", "Animation intensity is set in Accessibility.";
        ANIMATIONS_OPEN = "Ouvrir Accessibilité", "Open Accessibility";

        PROFILES_TIP = "Un profil garde tout\u{a0}: le thème, le dégradé, l'image de fond, les polices et les détails. Un clic pour en changer.", "A profile keeps everything: theme, gradient, background image, fonts and details. One click to switch.";
        SAVE_PROFILE_TIP = "Garde tous les réglages d'apparence en cours sous ce nom.", "Keeps all current appearance settings under this name.";
        RESET_ALL = "Tout réinitialiser", "Reset everything";
        RESET_ALL_TIP = "Revient au thème par défaut, sans dégradé, sans image de fond et avec les polices du thème. Vos thèmes et profils enregistrés sont gardés.", "Back to the default theme, with no gradient, no background image and the theme's fonts. Your saved themes and profiles are kept.";

        PREVIEW_NAME_A = "Camille", "Camille";
        PREVIEW_NAME_B = "Noé", "Noah";
        PREVIEW_MENTION = "@vous", "@you";
    }
}

/// Textes à paramètres.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        saved_as(name) = "«\u{a0}{name}\u{a0}» enregistré.", "\u{201c}{name}\u{201d} saved.";
        entry_unreadable(name) = "«\u{a0}{name}\u{a0}» est illisible.", "\u{201c}{name}\u{201d} could not be read.";
        not_written(why) = "Enregistrement impossible\u{a0}: {why}", "Could not save: {why}";
        hour(h) = "{h}\u{a0}h", "{h}:00";
        degrees(n) = "{n}°", "{n}°";
        copy_of(name) = "{name} copie", "{name} copy";
    }
}
