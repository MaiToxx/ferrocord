//! Textes des pages Accessibilité, Mode streamer, Avancés, Données et
//! confidentialité, Confidentialité de l'activité, À propos, et de l'envoi
//! d'avatar (vague 3). Module enfant de `table`, mêmes macros.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        PAGE_ACCESSIBILITY = "Accessibilité", "Accessibility";
        PAGE_STREAMER = "Mode streamer", "Streamer mode";
        SEC_STREAMING = "Pendant une diffusion", "While streaming";
        PAGE_ADVANCED = "Avancés", "Advanced";
        PAGE_DATA = "Données et confidentialité", "Data & privacy";
        PAGE_ACTIVITY = "Confidentialité de l'activité", "Activity privacy";
        PAGE_ABOUT = "À propos", "About";

        LOCAL_TIP = "Réglages de cet ordinateur (fichier local.toml)\u{a0}: Discord ne les synchronise pas.", "Settings of this computer (local.toml file): Discord doesn't sync them.";
        SEC_MOTION = "Animations", "Motion";
        SEC_COLORS = "Couleurs", "Colors";
        SEC_TEXT = "Texte et liens", "Text and links";
        MOTION = "Réduire les animations", "Reduce motion";
        MOTION_TIP = "Coupe les animations de l'interface (menus, défilement doux). «\u{a0}Comme Windows\u{a0}» suit le réglage Effets d'animation de Windows, lu au lancement.", "Turns off interface animations (menus, smooth scrolling). “Same as Windows” follows the Windows Animation effects setting, read at startup.";
        MOTION_SYSTEM = "Comme Windows", "Same as Windows";
        MOTION_ON = "Oui", "On";
        MOTION_OFF = "Non", "Off";
        INTENSITY = "Intensité des animations", "Animation intensity";
        INTENSITY_TIP = "«\u{a0}Discrète\u{a0}» ne garde que les fondus\u{a0}; «\u{a0}Aucune\u{a0}» affiche chaque changement tout de suite. «\u{a0}Réduire les animations\u{a0}» l'emporte.", "“Subtle” keeps fades only; “None” shows every change at once. “Reduce motion” takes precedence.";
        INTENSITY_NONE = "Aucune", "None";
        INTENSITY_SUBTLE = "Discrète", "Subtle";
        INTENSITY_FULL = "Complète", "Full";
        WINDOWS_REDUCED = "Windows\u{a0}: animations coupées", "Windows: animations off";
        WINDOWS_ANIMATED = "Windows\u{a0}: animations actives", "Windows: animations on";
        WINDOWS_UNKNOWN = "Réglage de Windows inconnu (animations gardées)", "Windows setting unknown (animations kept)";
        CONTRAST = "Contraste renforcé", "Increased contrast";
        CONTRAST_TIP = "Applique les couleurs du préréglage Contraste élevé (tailles et zoom gardés). Décoché\u{a0}: retour aux couleurs du préréglage d'avant.", "Applies the High contrast preset's colors (sizes and zoom kept). Unchecked: back to the previous preset's colors.";
        SATURATION = "Saturation", "Saturation";
        SATURATION_TIP = "Atténue les couleurs du thème (100\u{a0}%\u{a0}: couleurs d'origine).", "Tones down the theme colors (100%: original colors).";
        SEC_SCROLL = "Défilement", "Scrolling";
        SCROLL_SPEED = "Vitesse de défilement", "Scroll speed";
        SCROLL_SPEED_TIP = "Distance parcourue par un cran de molette (100\u{a0}%\u{a0}: comme Discord). Suit aussi le nombre de lignes par cran réglé dans Windows, lu au lancement.", "Distance travelled by one wheel notch (100%: same as Discord). Also follows the lines-per-notch setting of Windows, read at startup.";
        SIZE_ZOOM = "Taille du texte et zoom", "Text size and zoom";
        SIZE_ZOOM_TIP = "Réglés dans la page Apparence (Ctrl + et Ctrl - pour le zoom).", "Set in the Appearance page (Ctrl + and Ctrl - for zoom).";
        OPEN_APPEARANCE = "Ouvrir Apparence", "Open Appearance";
        UNDERLINE = "Souligner les liens", "Underline links";
        UNDERLINE_TIP = "Les liens des messages sont soulignés en plus d'être colorés.", "Links in messages are underlined as well as colored.";
        ROLE_COLORS = "Couleur des rôles dans les noms", "Role colors in names";
        ROLE_COLORS_TIP = "Liste des membres et mentions de rôle. Décoché\u{a0}: dans la couleur du texte.", "Member list and role mentions. Unchecked: in the text color.";
        INSTANT_TIPS = "Texte des boutons sans attendre", "Show button text immediately";
        INSTANT_TIPS_TIP = "Le texte d'un bouton à icône (infobulle) s'affiche dès le survol, sans délai.", "An icon button's text (tooltip) shows as soon as it's hovered, without delay.";

        STREAMER_ON = "Activer le mode streamer", "Enable streamer mode";
        STREAMER_ON_TIP = "Masque ce qui ne doit pas apparaître à l'écran pendant une diffusion.", "Hides what shouldn't appear on screen while streaming.";
        STREAMER_AUTO = "Activation automatique", "Automatic activation";
        STREAMER_AUTO_TIP = "Ferrocord ne détecte pas OBS ni les autres logiciels de diffusion (il faudrait lister les processus de l'ordinateur)\u{a0}: activez le mode à la main.", "Ferrocord doesn't detect OBS or other streaming software (that would mean listing the computer's processes): turn the mode on by hand.";
        STREAMER_AUTO_VALUE = "À la main seulement", "Manual only";
        HIDE_PERSONAL = "Masquer les informations personnelles", "Hide personal information";
        HIDE_PERSONAL_TIP = "Adresse e-mail et téléphone (Mon compte), notes, identifiants («\u{a0}Copier l'identifiant\u{a0}»).", "Email and phone (My account), notes, IDs (“Copy ID”).";
        HIDE_INVITES = "Masquer les liens d'invitation", "Hide invite links";
        HIDE_INVITES_TIP = "Les liens discord.gg et discord.com/invite des messages sont masqués et ne s'ouvrent pas.", "discord.gg and discord.com/invite links in messages are hidden and don't open.";
        MUTE_SOUNDS = "Couper les sons", "Disable sounds";
        MUTE_SOUNDS_TIP = "Sons de message et de mention.", "Message and mention sounds.";
        MUTE_TOASTS = "Couper les notifications", "Disable notifications";
        MUTE_TOASTS_TIP = "Notifications de bureau de Windows.", "Windows desktop notifications.";
        HIDDEN_STREAMER = "Masqué (mode streamer)", "Hidden (streamer mode)";

        DEV_MODE = "Mode développeur", "Developer mode";
        DEV_MODE_TIP = "Ajoute «\u{a0}Copier l'identifiant\u{a0}» aux menus des serveurs, des salons, des messages et des utilisateurs. Synchronisé avec Discord.", "Adds “Copy ID” to server, channel, message and user menus. Synced with Discord.";
        HW_ACCEL = "Accélération matérielle", "Hardware acceleration";
        HW_ACCEL_TIP = "Ferrocord dessine toujours avec la carte graphique (OpenGL)\u{a0}: il n'y a rien à régler.", "Ferrocord always draws with the graphics card (OpenGL): there's nothing to set.";
        HW_ACCEL_VALUE = "Toujours active (OpenGL)", "Always on (OpenGL)";
        SEC_DEBUG = "Débogage de Ferrocord", "Ferrocord debugging";
        SEC_IDENTITY = "Identité annoncée à Discord", "Identity announced to Discord";
        DESKTOP_IDENTITY = "Se présenter à Discord comme l'application de bureau", "Present to Discord as the desktop app";
        DESKTOP_IDENTITY_TIP = "Ferrocord annonce les mêmes propriétés que l'application officielle pour Windows\u{a0}: Discord vous montre connecté depuis un ordinateur, et non depuis un navigateur. Risque\u{a0}: Discord n'autorise pas les clients tiers, et se présenter comme son application peut aggraver une sanction, jusqu'à la fermeture du compte. Désactivé\u{a0}: Ferrocord s'annonce sous son propre nom. Appliqué à la prochaine connexion\u{a0}; redémarrez Ferrocord pour que les images et le vocal suivent.", "Ferrocord announces the same properties as the official Windows app: Discord shows you as connected from a computer rather than a browser. Risk: Discord does not allow third-party clients, and presenting as its own app may worsen a sanction, up to account termination. Off: Ferrocord announces itself under its own name. Applied at the next connection; restart Ferrocord for images and voice to follow.";
        DESKTOP_IDENTITY_UNAVAILABLE = "Disponible sous Windows seulement\u{a0}: ailleurs, Ferrocord s'annonce sous son propre nom.", "Available on Windows only: elsewhere, Ferrocord announces itself under its own name.";
        DEBUG_LOG = "Journal local", "Local log";
        DEBUG_LOG_TIP = "Écrit dans ferrocord.log les messages affichés en bas de la fenêtre et les connexions (jamais le jeton ni le contenu des messages). Au-delà d'1\u{a0}Mo, l'ancien journal est gardé à côté.", "Writes the messages shown at the bottom of the window and connections to ferrocord.log (never the token nor message contents). Past 1 MB, the old log is kept next to it.";
        FOLDER = "Dossier des réglages", "Settings folder";
        FOLDER_TIP = "local.toml, theme.toml, notify.toml et le journal.", "local.toml, theme.toml, notify.toml and the log.";
        OPEN_FOLDER = "Ouvrir le dossier", "Open folder";
        LOG_READY = "Connecté (READY reçu)", "Connected (READY received)";

        SEC_DATA_USE = "Utilisation de vos données", "How Discord uses your data";
        ACCESS_DETECT = "Détection des outils d'accessibilité", "Accessibility tool detection";
        ACCESS_DETECT_TIP = "Permet à Discord de savoir si vous utilisez un lecteur d'écran, pour améliorer l'accessibilité. Synchronisé avec Discord.", "Lets Discord know whether you use a screen reader, to improve accessibility. Synced with Discord.";
        USAGE_STATS = "Statistiques d'utilisation", "Usage statistics";
        USAGE_STATS_TIP = "Permet à Discord d'utiliser des données sur votre usage pour améliorer ses services.", "Lets Discord use data about how you use it to improve its services.";
        PERSONALIZATION = "Personnalisation", "Personalization";
        PERSONALIZATION_TIP = "Permet à Discord de personnaliser ce qu'il vous montre à partir de votre usage.", "Lets Discord personalize what it shows you based on your usage.";
        REQUEST_DATA = "Demander toutes mes données", "Request all of my data";
        REQUEST_DATA_TIP = "La demande se fait dans Discord (l'archive arrive par e-mail).", "The request is made in Discord (the archive arrives by email).";

        JOIN_FRIENDS = "Les amis peuvent rejoindre votre partie", "Friends can join your game";
        JOIN_FRIENDS_TIP = "Pour les jeux qui le permettent. Synchronisé avec Discord.", "For games that support it. Synced with Discord.";
        JOIN_VOICE = "Les membres du salon vocal peuvent rejoindre votre partie", "Voice channel members can join your game";
        JOIN_VOICE_TIP = "Ceux qui sont dans le même salon vocal que vous.", "Those in the same voice channel as you.";
        GUILDS_DEFAULT = "Partager l'activité dans les serveurs", "Share activity in servers";
        GUILDS_DEFAULT_TIP = "Pour les serveurs que vous rejoignez\u{a0}; chaque serveur se règle à part dans Discord.", "For servers you join; each server is set separately in Discord.";
        GD_ALL = "Dans tous les serveurs", "In all servers";
        GD_LARGE = "Sauf dans les grands serveurs", "Except in large servers";
        GD_NONE = "Dans aucun serveur", "In no server";

        COMMIT = "Commit", "Commit";
        COMMIT_TIP = "Indiqué si la variable FERROCORD_COMMIT était définie à la compilation.", "Shown if the FERROCORD_COMMIT variable was set at build time.";
        COMMIT_UNKNOWN = "Non indiqué", "Not recorded";
        REPO = "Dépôt", "Repository";
        REPO_TIP = "Le code source de Ferrocord n'est pas encore publié.", "Ferrocord's source code isn't published yet.";
        REPO_NONE = "Pas encore publié", "Not published yet";
        SEC_LICENSES = "Licences des dépendances", "Dependency licenses";
        LICENSES_TIP = "Bibliothèques Rust utilisées directement, et leur licence. Environ 260 bibliothèques en tout, presque toutes sous MIT ou Apache-2.0\u{a0}; exceptions ci-dessous.", "Rust libraries used directly, and their license. About 260 libraries in all, nearly all under MIT or Apache-2.0; exceptions below.";
        LICENSE_OWN_FONTS = "Polices embarquées, sous SIL Open Font License 1.1\u{a0}: Figtree (interface) et les polices des styles de nom, Bangers, BioRhyme, Cherry Bomb One, Chicle, Compagnon, Kalam, MuseoModerno, New Rocker, Orbitron, Pixelify Sans, Playpen Sans et Zilla Slab. Auteurs et texte de la licence\u{a0}: liste ci-dessus, filtre «\u{a0}police\u{a0}».", "Bundled fonts, under the SIL Open Font License 1.1: Figtree (interface) and the display name style fonts, Bangers, BioRhyme, Cherry Bomb One, Chicle, Compagnon, Kalam, MuseoModerno, New Rocker, Orbitron, Pixelify Sans, Playpen Sans and Zilla Slab. Authors and licence text: list above, filter \u{201c}police\u{201d}.";
        LICENSE_FONTS = "Polices de secours d'egui (Ubuntu, Noto)\u{a0}: OFL-1.1 et licence Ubuntu Font.", "egui fallback fonts (Ubuntu, Noto): OFL-1.1 and Ubuntu Font licence.";
        LICENSE_ROOTS = "Certificats racine (webpki-roots)\u{a0}: CDLA-Permissive-2.0.", "Root certificates (webpki-roots): CDLA-Permissive-2.0.";
        LICENSE_UNICODE = "Données Unicode (icu, zerovec, tinystr…)\u{a0}: Unicode-3.0.", "Unicode data (icu, zerovec, tinystr…): Unicode-3.0.";
        LICENSE_OTHER = "Aussi\u{a0}: ring (Apache-2.0 et ISC), clipboard-win et error-code (BSL-1.0), option-ext (MPL-2.0), subtle (BSD-3-Clause), zlib-rs et foldhash (Zlib).", "Also: ring (Apache-2.0 and ISC), clipboard-win and error-code (BSL-1.0), option-ext (MPL-2.0), subtle (BSD-3-Clause), zlib-rs and foldhash (Zlib).";
        SEC_ATTRIBUTIONS = "Attributions", "Attributions";

        AVATAR = "Avatar", "Avatar";
        BANNER = "Bannière", "Banner";
        IMAGE_TARGET = "Pour", "For";
        IMAGE_TARGET_TIP = "L'avatar et la bannière d'un serveur ne valent que dans ce serveur.", "A server's avatar and banner only apply in that server.";
        TARGET_ACCOUNT = "Le compte (partout)", "The account (everywhere)";
        CHOOSE_IMAGE = "Choisir une image…", "Choose an image…";
        PICK_TITLE = "Choisir une image", "Choose an image";
        REMOVE_IMAGE = "Retirer", "Remove";
        SEND_IMAGE = "Envoyer", "Upload";
        CANCEL_IMAGE = "Abandonner", "Discard";
        ZOOM = "Agrandissement", "Zoom";
        CROP_TIP = "Faites glisser l'image pour choisir la partie gardée.", "Drag the image to choose the part that's kept.";
        IMAGE_NITRO = "La bannière et les images par serveur demandent Nitro (Discord refuse sinon). Une image animée devient fixe.", "The banner and per-server images need Nitro (Discord refuses otherwise). An animated image becomes still.";
        IMAGE_PREPARING = "Préparation…", "Preparing…";
        IMAGE_SENT = "Envoyée", "Uploaded";
        IMAGE_REMOVED = "Retirée", "Removed";
        IMAGE_FAILED = "Non enregistrée", "Not saved";
        COPY_SERVER_ID = "Copier l'identifiant du serveur", "Copy Server ID";
        COPY_CHANNEL_ID = "Copier l'identifiant du salon", "Copy Channel ID";
    }
}

/// Textes à paramètres.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        image_unreadable(name) = "«\u{a0}{name}\u{a0}» n'est pas une image lisible (10\u{a0}Mo au plus)", "“{name}” isn't a readable image (10 MB at most)";
        local_not_saved(reason) = "Réglages de cet ordinateur non enregistrés\u{a0}: {reason}", "Settings of this computer not saved: {reason}";
        percent(n) = "{n}\u{a0}%", "{n}%";
    }
}
