//! Libellés et explications des jetons du thème (panneau Apparence,
//! commentaires de `theme.toml`), rangés comme les groupes de `theme::tokens!`
//! : `tokens::colors::bg0`… La macro du thème va chercher ici le texte de
//! chaque jeton : un jeton sans texte ne compile pas.

use super::Lang;

/// Libellé court et explication d'un jeton, dans les deux langues.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tok(pub(crate) [(&'static str, &'static str); 2]);

impl Tok {
    /// Libellé court (une ligne du panneau).
    pub const fn label(self, lang: Lang) -> &'static str {
        self.0[lang.index()].0
    }

    /// Explication (infobulle du panneau, commentaire du fichier).
    pub const fn detail(self, lang: Lang) -> &'static str {
        self.0[lang.index()].1
    }
}

macro_rules! token_texts {
    ($( $group:ident { $( $field:ident = [$lfr:literal, $dfr:literal], [$len:literal, $den:literal]; )+ } )+) => {
        $(
            #[allow(non_upper_case_globals)]
            pub mod $group {
                use super::Tok;
                $( pub const $field: Tok = Tok([($lfr, $dfr), ($len, $den)]); )+
            }
        )+

        /// Tous les textes de jetons : (groupe, champ, texte).
        #[cfg(test)]
        pub(crate) const ALL: &[(&str, &str, Tok)] = &[$($( (stringify!($group), stringify!($field), $group::$field), )+)+];
    };
}

token_texts! {
    colors {
        bg0 = ["Fond du rail", "Fond 0\u{a0}: rail des serveurs, pied utilisateur, écran de connexion"],
              ["Rail background", "Background 0: server rail, user bar, login screen"];
        bg1 = ["Fond des salons", "Fond 1\u{a0}: colonne des salons, embeds"],
              ["Channels background", "Background 1: channel column, embeds"];
        bg2 = ["Fond principal", "Fond 2\u{a0}: discussion, panneau Apparence"],
              ["Main background", "Background 2: chat, Appearance panel"];
        bg3 = ["Fond de saisie", "Fond 3\u{a0}: zone de saisie, notifications"],
              ["Input background", "Background 3: message box, notifications"];
        border = ["Bordures", "Bordures des cartes et des embeds"],
                 ["Borders", "Borders of cards and embeds"];
        pill = ["Pastilles", "Fond des pastilles du rail"],
               ["Pills", "Background of the rail's server pills"];
        selected = ["Salon ouvert", "Fond du salon ou de la conversation ouverte"],
                   ["Open channel", "Background of the open channel or conversation"];
        row_hover = ["Survol", "Fond d'un salon ou d'un serveur survolé"],
                    ["Hover", "Background of a hovered channel or server"];
        text = ["Texte", "Texte des messages"],
               ["Text", "Message text"];
        text_strong = ["Texte fort", "Texte mis en avant\u{a0}: non-lus, sélection, marque «\u{a0}non lu\u{a0}»"],
                      ["Strong text", "Emphasized text: unread, selection, “unread” mark"];
        muted = ["Texte atténué", "Salons lus, catégories, embeds"],
                ["Muted text", "Read channels, categories, embeds"];
        faint = ["Texte discret", "Heures, mentions «\u{a0}(modifié)\u{a0}», symbole du salon"],
                ["Faint text", "Times, “(edited)” notes, channel symbol"];
        accent = ["Accent", "Serveur ouvert, bouton Connexion, logo"],
                 ["Accent", "Open server, Log in button, logo"];
        on_accent = ["Texte sur accent", "Texte posé sur la couleur d'accent"],
                    ["Text on accent", "Text drawn on the accent color"];
        danger = ["Alerte", "Erreurs et mentions"],
                 ["Alert", "Errors and mentions"];
        link = ["Liens", "Liens et pièces jointes"],
               ["Links", "Links and attachments"];
        hover = ["Voile de survol", "Voile ajouté sous un message survolé"],
                ["Hover overlay", "Overlay added under a hovered message"];
        widget_text = ["Libellés", "Texte des libellés ordinaires (nom d'utilisateur, notifications, panneau)"],
                      ["Labels", "Text of ordinary labels (username, notifications, panel)"];
        button_bg = ["Fond des boutons", "Fond des boutons et listes déroulantes"],
                    ["Button background", "Background of buttons and drop-down lists"];
        button_text = ["Texte des boutons", "Texte des boutons"],
                      ["Button text", "Button text"];
        control_bg = ["Fond des curseurs", "Fond des cases à cocher et des curseurs"],
                     ["Slider background", "Background of checkboxes and sliders"];
        separator = ["Séparateurs", "Séparateurs entre les zones"],
                    ["Separators", "Separators between areas"];
        scrim = ["Fond de la visionneuse", "Voile posé sur la fenêtre derrière une image agrandie"],
                ["Viewer backdrop", "Overlay drawn over the window behind an enlarged image"];
        shadow = ["Ombres", "Ombre portée des menus, des cartes et des fenêtres"],
                 ["Shadows", "Drop shadow of menus, cards and windows"];
        focus = ["Contour du clavier", "Contour de la ligne ou du bouton qui a le clavier (Tab, flèches, sélecteur rapide)"],
                ["Keyboard outline", "Outline of the row or button that has the keyboard (Tab, arrows, quick switcher)"];
        online = ["En ligne", "Pastille de statut\u{a0}: en ligne"],
                 ["Online", "Status dot: online"];
        idle = ["Inactif", "Pastille de statut\u{a0}: inactif"],
               ["Idle", "Status dot: idle"];
        dnd = ["Ne pas déranger", "Pastille de statut\u{a0}: ne pas déranger"],
              ["Do not disturb", "Status dot: do not disturb"];
        offline = ["Hors ligne", "Pastille de statut\u{a0}: hors ligne"],
                  ["Offline", "Status dot: offline"];
        authors = ["Auteurs", "Couleurs des noms d'auteurs (8 couleurs)"],
                  ["Authors", "Colors of author names (8 colors)"];
        panel_border = ["Filet des zones", "Filet entre le rail, la colonne des salons et la discussion, et sous la barre de titre"],
                       ["Zone divider", "Line between the rail, the channel column and the chat, and under the title bar"];
        menu_bg = ["Fond des menus", "Fond des menus, des listes déroulantes et des infobulles"],
                  ["Menu background", "Background of menus, drop-down lists and tooltips"];
        menu_hover = ["Menu\u{a0}: survol", "Fond d'une ligne de menu survolée"],
                     ["Menu: hover", "Background of a hovered menu row"];
        channel_idle = ["Salon au repos", "Nom et icône d'un salon ni ouvert, ni survolé, ni non lu"],
                       ["Idle channel", "Name and icon of a channel that is not open, hovered or unread"];
    }
    typography {
        body = ["Taille du texte", "Texte courant\u{a0}: messages, salons, libellés"],
               ["Text size", "Body text: messages, channels, labels"];
        small = ["Petit texte", "Heures, catégories, remarques"],
                ["Small text", "Times, categories, notes"];
        button = ["Boutons", "Texte des boutons"],
                 ["Buttons", "Button text"];
        title = ["Titres", "Nom du serveur et du salon ouvert"],
                ["Titles", "Name of the open server and channel"];
        icon = ["Symbole du salon", "Symbole devant le nom du salon ouvert"],
               ["Channel symbol", "Symbol before the open channel's name"];
        rail = ["Initiales", "Initiales des serveurs dans les pastilles"],
               ["Initials", "Server initials in the pills"];
        logo = ["Logo", "Logo de l'écran de connexion"],
               ["Logo", "Login screen logo"];
        line_height = ["Interligne", "Hauteur d'une ligne de message\u{a0}; 0\u{a0}: celle de la police"],
                      ["Line height", "Height of a line of message text; 0: the font's own"];
        family = ["Police", "Police de l'interface\u{a0}: Figtree, fournie avec Ferrocord, ou Segoe UI du système\u{a0}; le code reste en Consolas"],
                 ["Font", "Interface font: Figtree, shipped with Ferrocord, or the system's Segoe UI; code stays in Consolas"];
    }
    shapes {
        roundness = ["Arrondi des coins", "Multiplie tous les rayons ci-dessous (0 = coins carrés)"],
                    ["Corner roundness", "Multiplies every radius below (0 = square corners)"];
        card = ["Carte de connexion", "Rayon de la carte de connexion"],
               ["Login card", "Radius of the login card"];
        rail = ["Pastilles", "Rayon des pastilles des serveurs"],
               ["Pills", "Radius of the server pills"];
        rail_selected = ["Pastille ouverte", "Rayon de la pastille du serveur ouvert, et des lignes du rail en liste"],
                        ["Selected pill", "Radius of the open server's pill, and of the rail rows in list mode"];
        row = ["Lignes de salon", "Rayon du fond d'un salon ouvert ou survolé"],
              ["Channel rows", "Radius of an open or hovered channel's background"];
        control = ["Boutons et champs", "Rayon des boutons, des champs et des cases"],
                  ["Buttons and fields", "Radius of buttons, fields and checkboxes"];
        composer = ["Zone de saisie", "Rayon de la zone de saisie"],
                   ["Message box", "Radius of the message box"];
        embed = ["Embeds", "Rayon des embeds"],
                ["Embeds", "Radius of embeds"];
        embed_bar = ["Barre des embeds", "Largeur de la barre de couleur à gauche des embeds"],
                    ["Embed bar", "Width of the color bar left of embeds"];
        toast = ["Notifications", "Rayon des notifications"],
                ["Notifications", "Radius of notifications"];
        hover = ["Survol d'un message", "Rayon du voile sous un message survolé"],
                ["Message hover", "Radius of the overlay under a hovered message"];
        avatar = ["Avatars carrés", "Rayon des coins des avatars en forme de carré arrondi"],
                 ["Square avatars", "Corner radius of rounded-square avatars"];
        border = ["Bordures", "Épaisseur des bordures"],
                 ["Borders", "Border thickness"];
        unread_ring = ["Marque «\u{a0}non lu\u{a0}»", "Épaisseur de l'anneau du rail et de la marque des salons non lus"],
                      ["“Unread” mark", "Thickness of the rail ring and of the unread channel mark"];
        mention_dot = ["Pastille de mention", "Rayon de la pastille rouge des mentions"],
                      ["Mention badge", "Radius of the red mention badge"];
        status_dot = ["Pastille de statut", "Rayon de la pastille de statut sur l'avatar d'un membre"],
                     ["Status badge", "Radius of the status badge on a member's avatar"];
        focus = ["Épaisseur du contour", "Épaisseur du contour de l'élément qui a le clavier"],
                ["Outline width", "Width of the outline of the element that has the keyboard"];
    }
    spacing {
        item_x = ["Entre éléments (horiz.)", "Espace horizontal entre deux éléments d'une ligne"],
                 ["Between items (horiz.)", "Horizontal space between two items on a row"];
        item_y = ["Entre éléments (vert.)", "Espace vertical entre deux éléments empilés"],
                 ["Between items (vert.)", "Vertical space between two stacked items"];
        control_x = ["Boutons\u{a0}: côtés","Marge horizontale dans un bouton ou un champ"],
                    ["Buttons: sides", "Horizontal padding inside a button or a field"];
        control_y = ["Boutons\u{a0}: haut et bas","Marge verticale dans un bouton ou un champ"],
                    ["Buttons: top and bottom", "Vertical padding inside a button or a field"];
        rail_x = ["Rail\u{a0}: côtés", "Marge horizontale du rail des serveurs"],
                 ["Rail: sides", "Horizontal margin of the server rail"];
        rail_y = ["Rail\u{a0}: haut et bas", "Marge verticale du rail des serveurs"],
                 ["Rail: top and bottom", "Vertical margin of the server rail"];
        rail_gap = ["Rail\u{a0}: sous les MP", "Espace sous le bouton des messages privés"],
                   ["Rail: below DMs", "Space below the direct messages button"];
        channels_pad = ["Salons\u{a0}: marge", "Marge intérieure de la colonne des salons"],
                       ["Channels: margin", "Inner margin of the channel column"];
        category_gap = ["Salons\u{a0}: avant catégorie", "Espace au-dessus d'une catégorie"],
                       ["Channels: before category", "Space above a category"];
        channel_row = ["Salons\u{a0}: hauteur", "Hauteur d'une ligne de salon"],
                      ["Channels: height", "Height of a channel row"];
        channel_indent = ["Salons\u{a0}: retrait", "Retrait des salons rangés dans une catégorie"],
                         ["Channels: indent", "Indent of channels inside a category"];
        mention_inset = ["Salons\u{a0}: pastille", "Distance entre la pastille de mention et le bord droit"],
                        ["Channels: badge", "Distance between the mention badge and the right edge"];
        user_bar_x = ["Pied\u{a0}: côtés", "Marge horizontale du pied utilisateur"],
                     ["User bar: sides", "Horizontal margin of the user bar"];
        user_bar_y = ["Pied\u{a0}: haut et bas", "Marge verticale du pied utilisateur"],
                     ["User bar: top and bottom", "Vertical margin of the user bar"];
        guild_title_x = ["Titre serveur\u{a0}: côtés", "Marge horizontale du titre du serveur"],
                        ["Server title: sides", "Horizontal margin of the server title"];
        guild_title_y = ["Titre serveur\u{a0}: haut et bas", "Marge verticale du titre du serveur"],
                        ["Server title: top and bottom", "Vertical margin of the server title"];
        chat_head_x = ["En-tête\u{a0}: côtés", "Marge horizontale de l'en-tête du salon"],
                      ["Header: sides", "Horizontal margin of the channel header"];
        chat_head_y = ["En-tête\u{a0}: haut et bas", "Marge verticale de l'en-tête du salon"],
                      ["Header: top and bottom", "Vertical margin of the channel header"];
        composer_x = ["Saisie\u{a0}: côtés", "Marge horizontale autour de la zone de saisie"],
                     ["Message box: sides", "Horizontal margin around the message box"];
        composer_top = ["Saisie\u{a0}: au-dessus", "Marge au-dessus de la zone de saisie"],
                       ["Message box: above", "Margin above the message box"];
        composer_bottom = ["Saisie\u{a0}: en dessous", "Marge sous la zone de saisie"],
                          ["Message box: below", "Margin below the message box"];
        composer_inner_x = ["Saisie\u{a0}: retrait horiz.", "Retrait horizontal du texte dans la zone de saisie"],
                           ["Message box: horiz. inset", "Horizontal inset of the text in the message box"];
        composer_inner_y = ["Saisie\u{a0}: retrait vert.", "Retrait vertical du texte dans la zone de saisie"],
                           ["Message box: vert. inset", "Vertical inset of the text in the message box"];
        message_x = ["Messages\u{a0}: côtés", "Marge horizontale des messages"],
                    ["Messages: sides", "Horizontal margin of messages"];
        message_gap = ["Messages\u{a0}: entre groupes", "Espace au-dessus d'un nouveau groupe de messages"],
                      ["Messages: between groups", "Space above a new group of messages"];
        message_gap_grouped = ["Messages\u{a0}: dans un groupe", "Espace entre deux messages d'un même groupe"],
                              ["Messages: within a group", "Space between two messages of the same group"];
        avatar_gap = ["Messages\u{a0}: après l'avatar", "Espace entre l'avatar et le texte d'un message"],
                     ["Messages: after avatar", "Space between the avatar and a message's text"];
        chat_pad = ["Messages\u{a0}: haut et bas", "Espace en haut et en bas de l'historique"],
                   ["Messages: top and bottom", "Space at the top and bottom of the history"];
        embed_pad = ["Embeds\u{a0}: retrait", "Retrait intérieur des embeds"],
                    ["Embeds: inset", "Inner inset of embeds"];
        media_gap = ["Images\u{a0}: écart", "Espace entre les images d'une grille et entre les pièces jointes"],
                    ["Images: gap", "Space between the images of a grid and between attachments"];
        compact_indent = ["Une ligne\u{a0}: retrait", "Retrait des embeds et pièces jointes en affichage sur une ligne"],
                         ["Single line: indent", "Indent of embeds and attachments in single-line mode"];
        card_pad = ["Connexion\u{a0}: retrait", "Retrait intérieur de la carte de connexion"],
                   ["Login: inset", "Inner inset of the login card"];
        login_title_gap = ["Connexion\u{a0}: sous le titre", "Espace sous le titre de la carte de connexion"],
                          ["Login: below title", "Space below the login card's title"];
        login_field_gap = ["Connexion\u{a0}: sous le champ", "Espace sous le champ du jeton"],
                          ["Login: below field", "Space below the token field"];
        login_button_gap = ["Connexion\u{a0}: avant le bouton", "Espace au-dessus du bouton Connexion"],
                           ["Login: before button", "Space above the Log in button"];
        login_error_gap = ["Connexion\u{a0}: avant l'erreur", "Espace au-dessus d'un message d'erreur"],
                          ["Login: before error", "Space above an error message"];
        login_notice_gap = ["Connexion\u{a0}: avant l'avertissement", "Espace au-dessus de l'avertissement"],
                           ["Login: before warning", "Space above the warning"];
        toast_pad = ["Notifications\u{a0}: retrait", "Retrait intérieur des notifications"],
                    ["Notifications: inset", "Inner inset of notifications"];
        toast_offset = ["Notifications\u{a0}: position", "Distance entre une notification et le bas de la fenêtre"],
                       ["Notifications: position", "Distance between a notification and the bottom of the window"];
        member_row = ["Membres\u{a0}: hauteur", "Hauteur d'une ligne de la liste des membres"],
                     ["Members: height", "Height of a row in the member list"];
        member_header = ["Membres\u{a0}: titres", "Hauteur d'un titre de groupe de la liste des membres"],
                        ["Members: headings", "Height of a group heading in the member list"];
        header_height = ["En-têtes\u{a0}: hauteur", "Hauteur des en-têtes (serveur, salon, amis), marges comprises\u{a0}; 0\u{a0}: d'après le texte"],
                        ["Headers: height", "Height of headers (server, channel, friends), margins included; 0: from the text"];
        message_right = ["Messages\u{a0}: marge droite", "Marge à droite des messages"],
                        ["Messages: right margin", "Margin to the right of messages"];
        control_height = ["Contrôles\u{a0}: hauteur", "Hauteur minimale des boutons, des champs et des lignes de menu"],
                         ["Controls: height", "Minimum height of buttons, fields and menu rows"];
    }
    layout {
        zoom = ["Zoom", "Agrandit toute l'interface (Ctrl + molette ou Ctrl +/-)"],
               ["Zoom", "Enlarges the whole interface (Ctrl + mouse wheel or Ctrl +/-)"];
        density = ["Densité", "Espacements verticaux\u{a0}: aérée, normale ou compacte"],
                  ["Density", "Vertical spacing: spacious, normal or compact"];
        messages = ["Messages", "Groupés par auteur, ou une ligne par message (12:34 auteur\u{a0}: texte)"],
                   ["Messages", "Grouped by author, or one line per message (12:34 author: text)"];
        clock = ["Horloge", "Heures sur 24\u{a0}h ou 12\u{a0}h"],
                ["Clock", "Times in 24-hour or 12-hour format"];
        rail_style = ["Rail des serveurs", "Pastilles d'initiales, ou liste avec les noms"],
                     ["Server rail", "Initial pills, or a list with names"];
        avatars = ["Avatars", "Affichés (messages groupés, messages privés, pied utilisateur), ou masqués partout\u{a0}: aucun avatar n'est alors téléchargé. Les icônes de serveur restent"],
                  ["Avatars", "Shown (grouped messages, direct messages, user bar), or hidden everywhere: then no avatars are downloaded. Server icons remain"];
        avatar_shape = ["Forme des avatars", "Ronds, ou carrés aux coins arrondis"],
                       ["Avatar shape", "Round, or squares with rounded corners"];
        media_load = ["Charger les images", "Images jointes et aperçus de liens\u{a0}: chargés automatiquement, ou seulement au clic (leur place reste réservée)"],
                     ["Load images", "Attached images and link previews: loaded automatically, or only when clicked (their space stays reserved)"];
        media_width = ["Largeur des images", "Largeur maximale d'une image jointe dans la discussion"],
                      ["Image width", "Maximum width of an attached image in the chat"];
        media_height = ["Hauteur des images", "Hauteur maximale d'une image jointe dans la discussion"],
                       ["Image height", "Maximum height of an attached image in the chat"];
        embed_width = ["Largeur des embeds", "Largeur maximale d'un embed"],
                      ["Embed width", "Maximum width of an embed"];
        embed_thumb = ["Vignettes des embeds", "Côté de la vignette à droite d'un embed"],
                      ["Embed thumbnails", "Size of the thumbnail on the right of an embed"];
        avatar_size = ["Taille des avatars", "Côté des avatars à gauche des messages"],
                      ["Avatar size", "Size of the avatars left of messages"];
        small_avatar = ["Petits avatars", "Côté des avatars des messages privés et du pied utilisateur"],
                       ["Small avatars", "Size of the avatars in direct messages and the user bar"];
        rail_width = ["Largeur du rail", "Largeur du rail en pastilles"],
                     ["Rail width", "Width of the rail in pill mode"];
        rail_list_width = ["Largeur du rail (liste)", "Largeur du rail en liste"],
                          ["Rail width (list)", "Width of the rail in list mode"];
        rail_button = ["Taille des pastilles", "Côté des pastilles du rail"],
                      ["Pill size", "Size of the rail's pills"];
        rail_list_row = ["Lignes du rail (liste)", "Hauteur d'une ligne du rail en liste"],
                        ["Rail rows (list)", "Height of a rail row in list mode"];
        channels_width = ["Largeur des salons", "Largeur de la colonne des salons (aussi réglable à la souris)"],
                         ["Channel column width", "Width of the channel column (also adjustable with the mouse)"];
        members = ["Liste des membres", "À droite de la discussion, ou masquée (aussi par le bouton de l'en-tête)"],
                  ["Member list", "Right of the chat, or hidden (also with the header button)"];
        members_width = ["Largeur des membres", "Largeur de la liste des membres"],
                        ["Member list width", "Width of the member list"];
        card_width = ["Carte de connexion", "Largeur de la carte de connexion"],
                     ["Login card", "Width of the login card"];
        button_height = ["Bouton Connexion", "Hauteur du bouton Connexion"],
                        ["Log in button", "Height of the Log in button"];
        composer_rows = ["Saisie\u{a0}: lignes au plus", "Hauteur maximale de la zone de saisie, en lignes (au-delà, elle défile)"],
                        ["Message box: max lines", "Maximum height of the message box, in lines (it scrolls beyond)"];
        typing = ["Signaler que j'écris", "Prévient les autres quand vous écrivez («\u{a0}X écrit…\u{a0}»)\u{a0}; envoyé à Discord au plus toutes les 8\u{a0}s pendant la frappe"],
                 ["Send typing indicator", "Lets others see when you are typing (“X is typing…”); sent to Discord at most every 8 s while typing"];
        status_dots = ["Statut sur les messages", "Pastille de statut (en ligne, inactif…) sur les avatars des messages\u{a0}; toujours affichée dans les messages privés et sur les profils"],
                      ["Status on messages", "Status dot (online, idle…) on message avatars; always shown in direct messages and on profiles"];
        profile_width = ["Largeur des profils", "Largeur de la carte de profil"],
                        ["Profile width", "Width of the profile card"];
        profile_avatar = ["Avatar des profils", "Côté de l'avatar de la carte de profil"],
                         ["Profile avatar", "Size of the avatar on the profile card"];
        titlebar = ["Barre de titre", "Barre de titre dessinée par Ferrocord, ou celle de Windows"],
                   ["Title bar", "Title bar drawn by Ferrocord, or the one from Windows"];
        titlebar_height = ["Barre de titre\u{a0}: hauteur", "Hauteur de la barre de titre intégrée"],
                          ["Title bar: height", "Height of the built-in title bar"];
    }
    markdown {
        code_bg = ["Fond du code", "Fond du code en ligne, des blocs de code et des horodatages"],
                  ["Code background", "Background of inline code, code blocks and timestamps"];
        mention_bg = ["Fond des mentions", "Fond des mentions (@membre, #salon)\u{a0}; sa transparence sert aussi aux mentions de rôle"],
                     ["Mention background", "Background of mentions (@member, #channel); its transparency also applies to role mentions"];
        mention_text = ["Texte des mentions", "Texte des mentions (@membre, #salon, @everyone)"],
                       ["Mention text", "Text of mentions (@member, #channel, @everyone)"];
        mention_row = ["Message qui me mentionne", "Fond d'un message qui me mentionne (@moi, @everyone, un de mes rôles, réponse avec mention)\u{a0}; sa teinte, opaque, fait la barre de son bord gauche"],
                      ["Message mentioning me", "Background of a message that mentions me (@me, @everyone, one of my roles, reply with mention); its hue, opaque, draws the bar on its left edge"];
        spoiler = ["Spoiler masqué", "Pavé qui cache un spoiler tant qu'on n'a pas cliqué dessus"],
                  ["Hidden spoiler", "Block hiding a spoiler until it is clicked"];
        quote_bar = ["Barre des citations", "Barre verticale à gauche des citations"],
                    ["Quote bar", "Vertical bar left of quotes"];
        keyword = ["Code\u{a0}: mots-clés", "Coloration des blocs de code\u{a0}: mots-clés"],
                  ["Code: keywords", "Code block highlighting: keywords"];
        string = ["Code\u{a0}: chaînes", "Coloration des blocs de code\u{a0}: chaînes de caractères, lignes ajoutées d'un diff"],
                 ["Code: strings", "Code block highlighting: strings, added lines of a diff"];
        number = ["Code\u{a0}: nombres", "Coloration des blocs de code\u{a0}: nombres"],
                 ["Code: numbers", "Code block highlighting: numbers"];
        comment = ["Code\u{a0}: commentaires", "Coloration des blocs de code\u{a0}: commentaires"],
                  ["Code: comments", "Code block highlighting: comments"];
        h1 = ["Titre 1", "Taille des titres «\u{a0}#\u{a0}»"],
             ["Heading 1", "Size of “#” headings"];
        h2 = ["Titre 2", "Taille des titres «\u{a0}##\u{a0}»"],
             ["Heading 2", "Size of “##” headings"];
        h3 = ["Titre 3", "Taille des titres «\u{a0}###\u{a0}»"],
             ["Heading 3", "Size of “###” headings"];
        quote_width = ["Barre des citations\u{a0}: largeur", "Largeur de la barre à gauche des citations"],
                      ["Quote bar: width", "Width of the bar left of quotes"];
        quote_gap = ["Citations\u{a0}: retrait", "Écart entre la barre d'une citation et son texte"],
                    ["Quotes: inset", "Gap between a quote's bar and its text"];
        list_indent = ["Listes\u{a0}: retrait", "Retrait de chaque niveau de liste (puce comprise)"],
                      ["Lists: indent", "Indent of each list level (bullet included)"];
        code_pad = ["Code\u{a0}: marge", "Marge intérieure des blocs de code"],
                   ["Code: padding", "Inner padding of code blocks"];
        code_radius = ["Code\u{a0}: coins", "Rayon des coins des blocs de code"],
                      ["Code: corners", "Corner radius of code blocks"];
        emoji = ["Emojis\u{a0}: taille", "Côté des emojis dans le texte"],
                ["Emojis: size", "Side of emojis within text"];
        jumbo = ["Emojis seuls\u{a0}: taille", "Côté des emojis d'un message qui n'a qu'eux"],
                ["Emoji-only: size", "Side of emojis in a message made only of them"];
        picker_emoji = ["Sélecteur\u{a0}: emojis", "Côté des emojis du sélecteur"],
                       ["Picker: emojis", "Side of emojis in the picker"];
        sticker = ["Autocollants\u{a0}: taille", "Côté des autocollants"],
                  ["Stickers: size", "Side of stickers"];
    }
}
