//! Textes des plugins vocaux (lot E4) : suppression de bruit RNNoise,
//! égaliseur, enregistrement local d'un appel, incrustation « qui parle ».
//! Module enfant de `table` (mêmes macros), tenu à part pour les fusions.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        // Suppression de bruit RNNoise.
        RNN = "Suppression de bruit RNNoise", "RNNoise noise suppression";
        RNN_ABOUT = "Nettoie votre micro sur cet ordinateur, avant l'envoi, avec le réseau de neurones RNNoise\u{a0}: clavier, ventilateur et bruits de fond sont retirés bien mieux que par le filtre d'origine, qu'il remplace quand «\u{a0}Suppression du bruit\u{a0}» est activée. Intensité et essai «\u{a0}S'entendre\u{a0}» dans Paramètres > Voix et vidéo.", "Cleans your microphone on this computer, before sending, with the RNNoise neural network: keyboard, fan and background noise are removed far better than by the original filter, which it replaces while “Noise suppression” is on. Strength and the “Hear yourself” test are in Settings > Voice & Video.";
        RNN_STRENGTH = "Intensité de RNNoise", "RNNoise strength";
        RNN_STRENGTH_TIP = "À 100\u{a0}%, seul le son nettoyé est envoyé\u{a0}; plus bas, un peu du son d'origine est gardé, ce qui sonne plus naturel.", "At 100%, only the cleaned sound is sent; lower, some of the original sound is kept, which sounds more natural.";
        RNN_HEAR = "S'entendre", "Hear yourself";
        RNN_HEAR_STOP = "Arrêter l'essai", "Stop the test";
        RNN_NEEDS_NOISE = "RNNoise n'agit que si «\u{a0}Suppression du bruit\u{a0}» est activée ci-dessus.", "RNNoise only works while “Noise suppression” above is on.";

        // Égaliseur.
        EQ = "Égaliseur des voix", "Voice equalizer";
        EQ_ABOUT = "Règle les graves, les médiums et les aigus des voix que vous entendez, avec des préréglages et un limiteur qui empêche la saturation. Le volume de chaque personne (jusqu'à 200\u{a0}%, clic droit sur elle dans un salon vocal) est déjà retenu d'une session à l'autre.", "Adjusts the bass, mids and treble of the voices you hear, with presets and a limiter that prevents clipping. Each person's volume (up to 200%, right-click them in a voice channel) is already remembered between sessions.";
        EQ_PRESET = "Préréglage", "Preset";
        EQ_PRESET_TIP = "«\u{a0}Personnalisé\u{a0}» utilise les trois curseurs ci-dessous.", "“Custom” uses the three sliders below.";
        EQ_FLAT = "Plat (aucun effet)", "Flat (no effect)";
        EQ_CLEAR = "Voix claires", "Clear voices";
        EQ_BASS = "Graves renforcés", "Bass boost";
        EQ_SOFT = "Aigus adoucis", "Softer treble";
        EQ_RADIO = "Radio", "Radio";
        EQ_NIGHT = "Nuit (moins de graves)", "Night (less bass)";
        EQ_CUSTOM = "Personnalisé", "Custom";
        EQ_LOW = "Graves (dB)", "Bass (dB)";
        EQ_MID = "Médiums (dB)", "Mids (dB)";
        EQ_HIGH = "Aigus (dB)", "Treble (dB)";
        EQ_BAND_TIP = "Utilisé avec le préréglage «\u{a0}Personnalisé\u{a0}».", "Used with the “Custom” preset.";

        // Enregistrement d'un appel.
        REC = "Enregistrement local d'un appel", "Local call recording";
        REC_ABOUT = "Bouton d'enregistrement dans le panneau vocal\u{a0}: les voix reçues et votre micro sont écrits dans un fichier audio (Ogg/Opus) sur cet ordinateur. Discord ne prévient pas les autres participants, et la loi peut exiger leur accord\u{a0}: demandez-le. Jamais de démarrage automatique\u{a0}; un indicateur rouge reste affiché tant que ça enregistre.", "Record button in the voice panel: received voices and your microphone are written to an audio file (Ogg/Opus) on this computer. Discord does not notify the other participants, and the law may require their consent: ask for it. Never starts by itself; a red indicator stays visible while recording.";
        REC_FOLDER = "Dossier des enregistrements", "Recordings folder";
        REC_FOLDER_TIP = "Chemin complet du dossier\u{a0}; il se choisit aussi au démarrage d'un enregistrement.", "Full path of the folder; it can also be chosen when a recording starts.";
        REC_TRACKS = "Une piste par personne", "One track per person";
        REC_TRACKS_TIP = "En plus du fichier mixé, un fichier par participant, tous calés sur le même départ.", "On top of the mixed file, one file per participant, all aligned on the same start.";
        REC_WARN_TITLE = "Enregistrer un appel", "Recording a call";
        REC_WARN_1 = "Discord ne prévient pas les autres participants que vous les enregistrez.", "Discord does not tell the other participants that you are recording them.";
        REC_WARN_2 = "Selon le pays, enregistrer une conversation sans l'accord de chacun peut être illégal. Prévenez tout le monde et obtenez leur accord avant de commencer.", "Depending on the country, recording a conversation without everyone's consent may be illegal. Tell everyone and get their consent before you start.";
        REC_WARN_3 = "L'enregistrement ne démarre jamais seul, et s'arrête quand vous quittez le salon.", "Recording never starts by itself, and stops when you leave the channel.";
        REC_UNDERSTOOD = "J'ai compris", "I understand";
        REC_TURN_OFF = "Désactiver le plugin", "Disable the plugin";
        REC_START_TITLE = "Démarrer l'enregistrement\u{a0}?", "Start recording?";
        REC_START = "Démarrer l'enregistrement", "Start recording";
        REC_NO_FOLDER = "Aucun dossier choisi", "No folder chosen";
        REC_PICK = "Choisir le dossier…", "Choose folder…";
        REC_PICK_TITLE = "Dossier des enregistrements", "Recordings folder";
        REC_BUTTON = "Enregistrer l'appel", "Record the call";
        REC_STOP = "Arrêter l'enregistrement", "Stop recording";
        REC_LABEL = "ENR", "REC";
        REC_FAILED_OPEN = "Impossible de créer le fichier d'enregistrement dans ce dossier.", "Could not create the recording file in this folder.";
        REC_FAILED_WRITE = "L'enregistrement s'est arrêté\u{a0}: écriture impossible.", "Recording stopped: could not write.";
        REC_NOT_CONNECTED = "L'enregistrement démarre une fois la connexion vocale établie.", "Recording starts once the voice connection is established.";
        REC_TRACK_ME = "moi", "me";
        REC_TRACK_STREAM = "direct", "stream";

        // Incrustation « qui parle ».
        OVERLAY = "Incrustation «\u{a0}qui parle\u{a0}»", "“Who's talking” overlay";
        OVERLAY_ABOUT = "Pendant un appel, une petite fenêtre transparente, toujours au premier plan, liste les participants et entoure celui qui parle. Les clics la traversent et elle ne prend jamais le focus. C'est une fenêtre ordinaire, sans injection dans le jeu\u{a0}: elle ne s'affiche pas par-dessus un jeu en plein écran exclusif (choisissez «\u{a0}fenêtré sans bordure\u{a0}»).", "During a call, a small transparent window, always on top, lists the participants and rings whoever is talking. Clicks go through it and it never takes focus. It is an ordinary window, with no injection into the game: it does not show over a game in exclusive fullscreen (pick “borderless windowed”).";
        OVERLAY_MOVE = "Mode déplacer", "Move mode";
        OVERLAY_MOVE_TIP = "La fenêtre reçoit les clics\u{a0}: faites-la glisser, tirez son coin pour la redimensionner, puis «\u{a0}Terminé\u{a0}».", "The window takes clicks: drag it, pull its corner to resize it, then “Done”.";
        OVERLAY_OPACITY = "Opacité (%)", "Opacity (%)";
        OVERLAY_OPACITY_TIP = "Opacité des avatars, des noms et de leur fond.", "Opacity of the avatars, names and their backdrop.";
        OVERLAY_X = "Position horizontale", "Horizontal position";
        OVERLAY_Y = "Position verticale", "Vertical position";
        OVERLAY_POS_TIP = "En points depuis le coin haut gauche de l'écran principal\u{a0}; le mode déplacer la règle à la souris.", "In points from the top-left corner of the main screen; move mode sets it with the mouse.";
        OVERLAY_W = "Largeur", "Width";
        OVERLAY_H = "Hauteur", "Height";
        OVERLAY_SIZE_TIP = "En points\u{a0}; le mode déplacer la règle à la souris.", "In points; move mode sets it with the mouse.";
        OVERLAY_DONE = "Terminé", "Done";
        OVERLAY_HINT = "Glissez pour déplacer", "Drag to move";
        OVERLAY_TITLE = "Ferrocord\u{a0}: qui parle", "Ferrocord: who's talking";
    }
}

/// Textes à paramètres.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        rec_folder(path) = "Dossier\u{a0}: {path}", "Folder: {path}";
        rec_saved(path) = "Enregistrement terminé\u{a0}: {path}", "Recording saved: {path}";
        rec_time(time) = "ENR {time}", "REC {time}";
        rec_tip(time) = "Enregistrement en cours ({time}). Cliquez pour l'arrêter.", "Recording ({time}). Click to stop.";
        percent(n) = "{n}\u{a0}%", "{n}%";
    }
}
