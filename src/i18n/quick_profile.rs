//! Textes de la carte de mon compte (clic sur le panneau utilisateur) et de
//! l'édition rapide du profil. Module enfant de `table` (il en reprend les
//! macros), tenu à part pour que les ajouts des lots parallèles ne se mêlent pas.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        /// Infobulle du panneau utilisateur.
        ACCOUNT_TIP = "Mon compte (clic droit\u{a0}: actions rapides)", "My account (right-click: quick actions)";
        EDIT_PROFILE = "Modifier le profil", "Edit Profile";
        STATUS = "Statut", "Status";
        CUSTOM_STATUS_HINT = "Quoi de neuf\u{a0}?", "What's up?";
        SET_CUSTOM_STATUS = "Définir un statut personnalisé", "Set Custom Status";
        EDIT_CUSTOM_STATUS = "Modifier le statut personnalisé", "Edit Custom Status";
        CLEAR_CUSTOM_STATUS = "Retirer le statut personnalisé", "Clear custom status";
        STATUS_IDLE_HINT = "Vous apparaîtrez comme absent.", "You will appear as away.";
        STATUS_DND_HINT = "Vous ne recevrez aucune notification sur le bureau.", "You will not receive any desktop notifications.";
        STATUS_INVISIBLE_HINT = "Vous n'apparaîtrez pas en ligne, mais garderez l'accès à tout Discord.", "You will not appear online, but will have full access to all of Discord.";
        MANAGE_ACCOUNTS = "Gérer les comptes", "Manage Accounts";
        SWITCH_ACCOUNT = "Changer de compte", "Switch Accounts";
        ID_COPIED = "Identifiant copié.", "User ID copied.";
        TAB_MAIN = "Profil principal", "Main Profile";
        TAB_GUILD = "Profil de ce serveur", "This Server's Profile";
        PREVIEW = "Aperçu", "Preview";
        UNSAVED = "Modifications non enregistrées", "Unsaved changes";
        MORE_OPTIONS = "Plus d'options", "More options";
        MORE_OPTIONS_TIP = "Ouvre la page Profils des paramètres", "Opens the Profiles page of the settings";
        AVATAR = "Avatar", "Avatar";
        BANNER = "Bannière", "Banner";
        CHANGE = "Changer…", "Change…";
        CROP = "Recadrer", "Crop";
        CROP_DONE = "Terminer", "Done";
        KEEP = "Garder l'actuelle", "Keep current";
        IMAGE_NEW = "Nouvelle image, envoyée à l'enregistrement.", "New image, sent when you save.";
        IMAGE_REMOVED = "Retirée à l'enregistrement.", "Removed when you save.";
        BANNER_COLOR = "Couleur de bannière", "Banner color";
        THEME = "Thème du profil", "Profile theme";
        THEME_PRIMARY = "Principale", "Primary";
        THEME_ACCENT = "Secondaire", "Accent";
        NITRO_ONLY = "Réservé à Nitro", "Nitro only";
        NICK = "Pseudo sur ce serveur", "Server nickname";
        GUILD_LOADING = "Chargement du profil de ce serveur…", "Loading this server's profile…";
        SAVED = "Profil enregistré.", "Profile saved.";
        ERR_NAME_RATE = "Vous changez de nom trop souvent\u{a0}: réessayez plus tard.", "You are changing your name too often: try again later.";
        ERR_IMAGE_RATE = "Vous changez d'avatar ou de bannière trop souvent\u{a0}: réessayez plus tard.", "You are changing your avatar or banner too often: try again later.";
        ERR_IMAGE_LARGE = "Image trop lourde pour Discord\u{a0}: choisissez-en une plus légère.", "Image too large for Discord: pick a smaller one.";
        ERR_NITRO = "Discord réserve cette fonction aux abonnés Nitro.", "Discord keeps this feature for Nitro subscribers.";
        // Éditeur de profil (fenêtre « Modifier le profil ») : colonne de gauche.
        ED_PLATE = "Plaque nominative", "Nameplate";
        ED_AVATAR_DECO = "Avatar et décoration", "Avatar & Decoration";
        ED_NAME_STYLE = "Style du pseudonyme", "Display Name Style";
        ED_THEME_BANNER = "Thème et bannière", "Theme & Banner";
        ED_DECORATION = "Décoration d'avatar", "Avatar Decoration";
        ED_THEME_COLORS = "Couleurs du thème", "Theme colors";
        ED_COLLAPSE = "Replier la colonne", "Collapse sidebar";
        ED_EXPAND = "Déplier la colonne", "Expand sidebar";
        ED_PICK_PROFILE = "Choisir le profil à modifier", "Choose which profile to edit";
        ED_NITRO_MARK = "Nitro", "Nitro";
        ED_LOCKED = "Discord réserve ce réglage aux abonnés Nitro\u{a0}: Ferrocord ne vend rien.", "Discord keeps this setting for Nitro subscribers: Ferrocord sells nothing.";
        ED_NO_PLATE = "Ce compte ne possède aucune plaque nominative.", "This account owns no nameplates.";
        ED_NO_DECORATION = "Ce compte ne possède aucune décoration d'avatar.", "This account owns no avatar decorations.";
        ED_OWNED_TIP = "Les objets s'obtiennent dans l'application officielle\u{a0}: rien ne s'achète ici.", "Items are obtained in the official app: nothing is bought here.";
        ED_STYLE_EFFECT = "Effet", "Effect";
        ED_STYLE_FONT = "Police", "Font";
        ED_STYLE_COLORS = "Couleurs", "Colors";
        ED_STYLE_NONE = "Aucun style", "No style";
        ED_STYLE_FONT_NOTE = "Polices libres embarquées (licence OFL), les mêmes que dans Discord. Medieval et Vampyre n'ont pas de police libre\u{a0}: Ferrocord les montre dans la police la plus proche de cette machine, les autres clients verront celle choisie.", "Bundled free fonts (OFL licence), the same as in Discord. Medieval and Vampyre have no free font: Ferrocord shows them in the closest font on this machine, other clients will see the chosen one.";
        ED_STYLE_READ_ONLY = "Cet effet est affiché, mais Ferrocord ne sait pas l'enregistrer\u{a0}: choisissez un autre effet pour modifier le style.", "This effect is shown, but Ferrocord cannot save it: pick another effect to change the style.";
        ED_FX_SOLID = "Uni", "Solid";
        ED_FX_GRADIENT = "Dégradé", "Gradient";
        ED_FX_NEON = "Néon", "Neon";
        ED_FX_TOON = "Toon", "Toon";
        ED_FX_POP = "Pop", "Pop";
        ED_FX_GLOW = "Lueur", "Glow";
        ED_FX_PRISM = "Prisme", "Prism";
        ED_FX_GUMMY = "Gummy", "Gummy";
        ED_THEME_ON = "Thème personnalisé", "Custom theme";
        ED_BANNER_COLOR_ON = "Couleur unie", "Solid color";
        // Profil modifiable sur place.
        ED_ADD_PRONOUNS = "Ajouter des pronoms", "Add pronouns";
        ED_ADD_TAG = "Ajouter un tag", "Add a tag";
        ED_BIO = "Bio", "Bio";
        ED_BIO_HINT = "Parle un peu de toi", "Tell the world a bit about yourself";
        ED_SEND_MESSAGE = "Envoyer un message", "Message";
        ED_SEND_MESSAGE_TIP = "Aperçu\u{a0}: ce bouton est inactif dans l'éditeur.", "Preview: this button is inactive in the editor.";
        ED_STATUS_HINT = "Définir un statut", "Set a status";
        ED_STATUS_TIP = "Statut personnalisé (enregistré à part)", "Custom status (saved separately)";
        // Barre d'enregistrement.
        ED_UNSAVED = "Attention, des modifications n'ont pas été enregistrées", "Careful, you have unsaved changes";
        ED_RESET = "Réinitialiser", "Reset";
        ED_CAPTCHA = "Discord demande une vérification (captcha)\u{a0}: Ferrocord s'arrête là. Faites ce changement dans l'application officielle.", "Discord asks for a verification (captcha): Ferrocord stops here. Make this change in the official app.";
        // Colonne de droite : widgets de jeux.
        ED_TAB_BOARD = "Tableau", "Board";
        ED_TAB_WISHLIST = "Liste de souhaits", "Wishlist";
        ED_YOUR_WIDGETS = "Tes widgets", "Your Widgets";
        ED_ADD_WIDGET = "Ajouter un widget", "Add Widget";
        ED_W_CURRENT = "Jeux du moment", "Current Games";
        ED_W_FAVORITE = "Jeu favori", "Favorite Game";
        ED_W_PLAYED = "Jeux que j'aime", "Games I Like";
        ED_W_WANTED = "Jeux que je veux", "Want to Play";
        ED_PICK_ONE = "Choisis 1 jeu", "Pick 1 game";
        ED_COMMENT_HINT = "Dis à tout le monde pourquoi c'est ton jeu préféré", "Tell everyone why it's your favorite game";
        ED_ADD_GAME = "Ajouter un jeu", "Add a game";
        ED_REMOVE_GAME = "Retirer ce jeu", "Remove this game";
        ED_REMOVE_WIDGET = "Retirer le widget", "Remove widget";
        ED_NO_WIDGETS = "Aucun widget pour l'instant.", "No widgets yet.";
        ED_WISHLIST_EMPTY = "Ta liste de souhaits est vide.", "Your wishlist is empty.";
        ED_GAME_SEARCH = "Rechercher un jeu", "Search for a game";
        ED_SUGGESTED = "Suggestions de Discord", "Suggested by Discord";
        ED_SEARCH_NOTE = "La recherche porte sur les jeux que Discord sait détecter\u{a0}; cette liste est chargée quand l'activité de jeu est activée.", "Search covers the games Discord can detect; that list is loaded when game activity is turned on.";
        ED_NO_GAME = "Aucun jeu trouvé.", "No game found.";
        ED_GAME_UNKNOWN = "Jeu inconnu", "Unknown game";
        ED_WIDGETS_READONLY = "Un widget de ce profil n'est pas pris en charge\u{a0}: ils sont montrés sans pouvoir être modifiés, pour ne rien perdre.", "A widget on this profile is not supported: they are shown read-only, so nothing is lost.";
        ED_WIDGETS_LOADING = "Chargement du profil…", "Loading profile…";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        count(n, max) = "{n}/{max}", "{n}/{max}";
        not_saved(reason) = "Non enregistré\u{a0}: {reason}", "Not saved: {reason}";
        up_to(n) = "Ajoute jusqu'à {n} jeux", "Add up to {n} games";
    }
}
