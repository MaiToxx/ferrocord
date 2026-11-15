//! Textes de la page « Voix et vidéo » (lot V5). Module enfant de `table`,
//! mêmes macros.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        PAGE_VOICE = "Voix et vidéo", "Voice & Video";
        LOCAL_TIP = "Réglages de cet ordinateur (fichier local.toml)\u{a0}: Discord ne les synchronise pas.", "Settings of this computer (local.toml file): Discord doesn't sync them.";
        SEC_VOICE = "Paramètres vocaux", "Voice settings";
        INPUT_DEVICE = "Périphérique d'entrée", "Input device";
        INPUT_DEVICE_TIP = "Micro utilisé dans les salons vocaux et les appels. «\u{a0}Par défaut\u{a0}» suit celui de Windows, même s'il change pendant un appel.", "Microphone used in voice channels and calls. “Default” follows the Windows one, even if it changes during a call.";
        OUTPUT_DEVICE = "Périphérique de sortie", "Output device";
        OUTPUT_DEVICE_TIP = "Casque ou haut-parleurs. «\u{a0}Par défaut\u{a0}» suit celui de Windows, même s'il change pendant un appel.", "Headset or speakers. “Default” follows the Windows one, even if it changes during a call.";
        DEFAULT_DEVICE = "Par défaut (Windows)", "Default (Windows)";
        LOADING_DEVICES = "Recherche des périphériques…", "Looking for devices…";
        REFRESH = "Actualiser la liste", "Refresh list";
        INPUT_VOLUME = "Volume d'entrée", "Input volume";
        INPUT_VOLUME_TIP = "Amplifie ou atténue le micro avant tout traitement.", "Boosts or lowers the microphone before any processing.";
        OUTPUT_VOLUME = "Volume de sortie", "Output volume";
        OUTPUT_VOLUME_TIP = "Volume de tout ce que vous entendez dans les appels.", "Volume of everything you hear in calls.";
        SEC_TEST = "Test du micro", "Mic test";
        TEST_TIP = "Parlez\u{a0}: vous vous entendez, avec le volume, la sensibilité et le traitement choisis. Le test s'arrête quand vous quittez cette page.", "Talk: you hear yourself, with the chosen volume, sensitivity and processing. The test stops when you leave this page.";
        TEST_START = "Vérifier", "Let's check";
        TEST_STOP = "Arrêter le test", "Stop testing";
        METER_IDLE = "Lancez le test du micro pour voir son niveau.", "Start the mic test to see its level.";
        SEC_MODE = "Mode de saisie", "Input mode";
        MODE = "Mode", "Mode";
        MODE_TIP = "Détection de la voix\u{a0}: le micro s'ouvre quand vous parlez. Appui pour parler\u{a0}: seulement touche enfoncée.", "Voice activity: the mic opens when you talk. Push to talk: only while the key is held.";
        MODE_VOICE = "Détection de la voix", "Voice activity";
        MODE_PTT = "Appui pour parler", "Push to talk";
        AUTO_THRESHOLD = "Déterminer automatiquement la sensibilité", "Automatically determine input sensitivity";
        AUTO_THRESHOLD_TIP = "Le seuil suit le bruit de fond de la pièce.", "The threshold follows the room's background noise.";
        THRESHOLD = "Sensibilité", "Input sensitivity";
        THRESHOLD_TIP = "Niveau au-dessus duquel le micro s'ouvre. La barre montre le niveau du micro pendant le test.", "Level above which the mic opens. The bar shows the mic level during the test.";
        PTT_KEY = "Raccourci", "Shortcut";
        PTT_KEY_TIP = "Touche à maintenir enfoncée pour parler.", "Key to hold down to talk.";
        PTT_NONE = "Aucune touche", "No key";
        PTT_SET = "Choisir la touche", "Record key";
        PTT_WAITING = "Appuyez sur une touche… (Échap\u{a0}: annuler)", "Press a key… (Esc: cancel)";
        PTT_CLEAR = "Retirer", "Remove";
        PTT_RELEASE = "Délai de relâchement", "Release delay";
        PTT_RELEASE_TIP = "Le micro reste ouvert ce temps après le relâchement de la touche, pour ne pas couper la fin des mots.", "The mic stays open this long after the key is released, so word endings aren't cut off.";
        SEC_PROCESSING = "Traitement de la voix", "Voice processing";
        NOISE = "Suppression du bruit", "Noise suppression";
        NOISE_TIP = "Retire les bruits de fond (ventilateur, clavier, rue) en gardant la voix.", "Removes background noise (fan, keyboard, street) while keeping your voice.";
        ECHO = "Annulation de l'écho", "Echo cancellation";
        ECHO_TIP = "Empêche les autres de s'entendre quand votre micro reprend vos haut-parleurs.", "Stops others from hearing themselves when your mic picks up your speakers.";
        AGC = "Contrôle automatique du gain", "Automatic gain control";
        AGC_TIP = "Ramène votre voix à un niveau régulier, ni trop faible ni saturé.", "Keeps your voice at a steady level, neither too quiet nor clipping.";
        SEC_VIDEO = "Vidéo", "Video";
        CAMERA = "Caméra", "Camera";
        CAMERA_TIP = "Caméra allumée par le bouton de la caméra d'un appel. «\u{a0}Par défaut\u{a0}»\u{a0}: la première que Windows propose\u{a0}; elle remplace aussi une caméra débranchée.", "Camera turned on by the camera button of a call. “Default”: the first one Windows offers; it also replaces an unplugged camera.";
        VIDEO_TEST = "Tester la vidéo", "Test video";
        VIDEO_TEST_STOP = "Arrêter la vidéo", "Stop video";
        VIDEO_TEST_TIP = "La caméra ne s'allume que pendant le test\u{a0}: rien n'est envoyé, et elle s'éteint quand vous quittez cette page.", "The camera is only on during the test: nothing is sent, and it turns off when you leave this page.";
        VIDEO_IDLE = "Lancez le test pour voir l'image de votre caméra.", "Start the test to see your camera's picture.";
        VIDEO_STARTING = "Allumage de la caméra…", "Turning the camera on…";
        CAMERA_PREVIEW = "Toujours afficher un aperçu avant d'allumer la caméra", "Always preview video before turning on the camera";
        CAMERA_PREVIEW_TIP = "Le bouton de la caméra ouvre d'abord l'aperçu\u{a0}: l'image ne part qu'après «\u{a0}Allumer la caméra\u{a0}».", "The camera button opens the preview first: video is only sent after “Turn on camera”.";
        SEC_SHARE = "Partage d'écran", "Screen share";
        SHARE_QUALITY = "Qualité par défaut", "Default quality";
        SHARE_QUALITY_TIP = "Définition et images par seconde proposées d'abord dans la fenêtre de partage\u{a0}; elles restent modifiables à chaque partage.", "Resolution and frame rate offered first in the share window; you can still change them for each share.";
        SHARE_SOUND = "Partager le son par défaut", "Share sound by default";
        SHARE_SOUND_TIP = "«\u{a0}Partager le son\u{a0}» est coché d'avance dans la fenêtre de partage (écrans et fenêtres).", "“Share sound” is ticked in advance in the share window (screens and windows).";
        ERR_NO_INPUT = "Aucun micro disponible.", "No microphone available.";
        ERR_MIC_PRIVACY = "Windows bloque l'accès au micro. Ouvrez Paramètres > Confidentialité et sécurité > Microphone, activez «\u{a0}Accès au microphone\u{a0}» et «\u{a0}Autoriser les applications de bureau à accéder à votre microphone\u{a0}», puis rejoignez de nouveau.", "Windows is blocking microphone access. Open Settings > Privacy & security > Microphone, turn on “Microphone access” and “Let desktop apps access your microphone”, then join again.";
        ERR_NO_OUTPUT = "Aucune sortie audio disponible.", "No audio output available.";
    }
}

/// Textes à paramètres.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        default_named(name) = "Par défaut (Windows)\u{a0}: {name}", "Default (Windows): {name}";
        unplugged(name) = "{name} (débranché)", "{name} (unplugged)";
        db(n) = "{n}\u{a0}dB", "{n} dB";
        ms(n) = "{n}\u{a0}ms", "{n} ms";
        lost_input(name) = "Micro débranché ({name})\u{a0}: celui par défaut de Windows le remplace.", "Microphone unplugged ({name}): the Windows default replaces it.";
        lost_output(name) = "Sortie débranchée ({name})\u{a0}: celle par défaut de Windows la remplace.", "Output unplugged ({name}): the Windows default replaces it.";
        unsupported(format) = "Format audio non pris en charge\u{a0}: {format}", "Unsupported audio format: {format}";
        audio_error(reason) = "Erreur audio\u{a0}: {reason}", "Audio error: {reason}";
    }
}
