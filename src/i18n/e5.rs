//! Textes des plugins du lot E5 : thèmes partageables, Mica et transparence
//! de Windows 11, rapport de plantage local. Module enfant de `table`, mêmes
//! macros. (Les noms et les descriptions des plugins sont dans `plugins.rs`.)

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        // Rapport de plantage local.
        CRASH_BANNER = "Ferrocord s'est arrêté brutalement la dernière fois. Un rapport a été écrit sur cet ordinateur\u{a0}; rien n'a été envoyé.", "Ferrocord stopped unexpectedly last time. A report was written on this computer; nothing was sent.";
        CRASH_BANNER_TIP = "Le rapport contient la date, la version, l'endroit du code et les derniers événements techniques. Ni jeton, ni message, ni nom de compte.", "The report holds the date, the version, the place in the code and the last technical events. No token, no message, no account name.";
        CRASH_OPEN = "Ouvrir le rapport", "Open report";
        CRASH_FOLDER = "Ouvrir le dossier", "Open folder";
        CRASH_IGNORE = "Ignorer", "Dismiss";

        // Thèmes partageables.
        SHARE_FILE_HEADER = "# Thème Ferrocord (.ferrotheme)\u{a0}: tout un thème dans un fichier, à partager.\n# Pour l'utiliser\u{a0}: Paramètres > Apparence > Thèmes partageables, ou déposez ce\n# fichier sur la fenêtre de Ferrocord.\n", "# Ferrocord theme (.ferrotheme): a whole theme in one file, to share.\n# To use it: Settings > Appearance > Shareable themes, or drop this file on\n# the Ferrocord window.\n";
        SEC_SHARE = "Thèmes partageables", "Shareable themes";
        SHARE_TIP = "Un fichier .ferrotheme contient tout un thème\u{a0}: couleurs, dégradé, rayons, densité, polices choisies et, si vous le voulez, l'image de fond. Pour en importer un, choisissez-le ici ou déposez-le sur la fenêtre\u{a0}: un aperçu s'affiche avant toute application.", "A .ferrotheme file holds a whole theme: colours, gradient, radii, density, chosen fonts and, if you wish, the background image. To import one, pick it here or drop it on the window: a preview is shown before anything is applied.";
        BUNDLED = "Thèmes fournis", "Included themes";
        SHARE_IMPORT = "Importer un fichier…", "Import a file…";
        SHARE_IMPORT_TITLE = "Importer un thème", "Import a theme";
        SHARE_EXPORT = "Exporter…", "Export…";
        SHARE_EXPORT_TITLE = "Exporter le thème", "Export the theme";
        SHARE_EXPORT_ROW = "Exporter le thème en cours", "Export the current theme";
        SHARE_EXPORT_ROW_TIP = "Le fichier porte ce nom et, si vous le donnez, le nom de son auteur.", "The file carries this name and, if you give one, its author's name.";
        SHARE_NAME = "Nom du thème", "Theme name";
        SHARE_AUTHOR = "Auteur (facultatif)", "Author (optional)";
        SHARE_WITH_IMAGE = "Inclure l'image de fond", "Include the background image";
        SHARE_WITH_IMAGE_TIP = "L'image est réduite et copiée dans le fichier. Son emplacement sur cet ordinateur n'y figure jamais.", "The image is scaled down and copied into the file. Its location on this computer is never written.";
        SHARE_APPLY = "Appliquer", "Apply";
        SHARE_HAS_IMAGE = "Image de fond incluse", "Background image included";
        SHARE_FONTS_NOTE = "Ces polices doivent être installées sur cet ordinateur\u{a0}; sinon celles du thème restent.", "These fonts must be installed on this computer; otherwise the theme's fonts stay.";
        SHARE_KEEPS = "Le thème sera ajouté à vos profils d'apparence. Votre zoom, la largeur des colonnes, la barre de titre et vos autres réglages ne changent pas.", "The theme will be added to your appearance profiles. Your zoom, column widths, title bar and other settings do not change.";
        SHARE_UNSAVED = "Thème appliqué, mais pas enregistré\u{a0}: le dossier des réglages ne peut pas être écrit.", "Theme applied but not saved: the settings folder cannot be written.";
        SHARE_NOT_TEXT = "Ce fichier n'est pas un texte lisible.", "This file is not readable text.";
        SHARE_NOT_A_THEME = "Ce fichier n'est pas un thème Ferrocord (.ferrotheme).", "This file is not a Ferrocord theme (.ferrotheme).";
        SHARE_UNREADABLE = "Le fichier n'a pas pu être lu.", "The file could not be read.";
        SHARE_IMAGE_UNREADABLE = "L'image de fond du thème n'est pas une image PNG, JPEG ou WebP lisible.", "The theme's background image is not a readable PNG, JPEG or WebP image.";
        SHARE_IMAGE_EXPORT_FAILED = "L'image de fond n'a pas pu être lue\u{a0}: le thème n'a pas été exporté.", "The background image could not be read: the theme was not exported.";
    }
}

/// Textes à paramètres.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        share_too_big(mb) = "Fichier trop gros (plus de {mb}\u{a0}Mo).", "File too large (over {mb} MB).";
        share_bad_format(version) = "Version de format inconnue (ce Ferrocord lit la version {version}).", "Unknown format version (this Ferrocord reads version {version}).";
        share_bad_name(max) = "Le thème n'a pas de nom valable (lettres et chiffres, {max} caractères au plus).", "The theme has no valid name (letters and digits, {max} characters at most).";
        share_bad_author(max) = "Le nom de l'auteur n'est pas valable ({max} caractères au plus).", "The author's name is not valid ({max} characters at most).";
        share_bad_font(max) = "Un nom de police est trop long (plus de {max} caractères).", "A font name is too long (over {max} characters).";
        share_bad_section(name) = "La section [{name}] est mal formée.", "The [{name}] section is malformed.";
        share_image_too_big(mb) = "L'image de fond du thème est trop grosse (plus de {mb}\u{a0}Mo).", "The theme's background image is too large (over {mb} MB).";
        share_import_title(name) = "Importer «\u{a0}{name}\u{a0}»\u{a0}?", "Import “{name}”?";
        share_by(author) = "par {author}", "by {author}";
        share_fonts(fonts) = "Polices\u{a0}: {fonts}", "Fonts: {fonts}";
        share_exported(path) = "Thème exporté\u{a0}: {path}", "Theme exported: {path}";
        share_export_failed(reason) = "Le thème n'a pas pu être exporté\u{a0}: {reason}", "The theme could not be exported: {reason}";
        share_refused(reason) = "Thème refusé\u{a0}: {reason}", "Theme refused: {reason}";
        share_added(name) = "Thème «\u{a0}{name}\u{a0}» appliqué et ajouté à vos profils d'apparence.", "Theme “{name}” applied and added to your appearance profiles.";
    }
}
