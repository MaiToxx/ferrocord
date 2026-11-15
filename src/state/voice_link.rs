//! Lien entre l'interface et l'orchestrateur du vocal (lot V7, `crate::voice`).
//! L'interface ne fait ni réseau ni audio : à chaque passe, elle décrit ce
//! qu'elle veut ([`VoiceWant`], lu dans `Store::voice`) et relève ce qui
//! s'est passé ([`VoiceLink::poll`], appliqué au magasin).

use super::audio_prefs::AudioPrefs;
use super::voice::{Secret, VoiceCmd, VoiceConnState, VoiceEvent};
use super::Store;

/// Ce qu'il faut pour ouvrir la connexion vocale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallTarget<'a> {
    pub guild_id: Option<u64>,
    pub channel_id: u64,
    pub user_id: u64,
    pub endpoint: &'a str,
    pub session_id: &'a str,
    pub token: &'a Secret,
}

/// Que doit faire la connexion vocale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallWant<'a> {
    /// Hors de tout salon, ou échec affiché : rien ne tourne.
    Stop,
    /// Op 4 envoyée, Discord n'a pas encore répondu : rien ne tourne, et
    /// faute de réponse au bout de 15 s, l'échec est annoncé.
    Wait,
    /// Session et serveur connus : ouvrir (ou rouvrir si autre chose).
    Open(CallTarget<'a>),
    /// Connexion établie : la garder.
    Keep,
}

/// Ce que l'interface veut, à cette passe.
#[derive(Clone, Copy, Debug)]
pub struct VoiceWant<'a> {
    pub call: CallWant<'a>,
    /// Micro coupé (choisi, sourdine, ou imposé par le serveur).
    pub muted: bool,
    /// Rien n'est joué (sourdine choisie ou imposée).
    pub deafened: bool,
    pub audio: &'a AudioPrefs,
}

impl Store {
    /// Ce que l'orchestrateur doit faire, d'après l'état vocal.
    pub fn voice_want<'a>(&'a self, audio: &'a AudioPrefs) -> VoiceWant<'a> {
        let v = &self.voice;
        let me = self.me.as_ref().map(|u| u.id);
        let call = match (&v.conn, v.me, me) {
            (VoiceConnState::Connecting { endpoint, token, session_id }, Some(my), Some(user_id)) => {
                CallWant::Open(CallTarget { guild_id: my.guild_id, channel_id: my.channel_id, user_id, endpoint, session_id, token })
            }
            (VoiceConnState::Connected { .. }, Some(_), Some(_)) => CallWant::Keep,
            (VoiceConnState::Joining, Some(_), _) => CallWant::Wait,
            _ => CallWant::Stop,
        };
        let mine = me.and_then(|m| v.state(m));
        let deafened = v.self_deaf || mine.is_some_and(|s| s.deaf);
        let muted = v.self_mute || deafened || !me.is_some_and(|m| v.can_speak(m));
        VoiceWant { call, muted, deafened, audio }
    }
}

/// L'orchestrateur du vocal vu de l'interface.
pub trait VoiceLink {
    /// Une fois par passe : ce que l'interface veut (bon marché si rien ne change).
    fn sync(&mut self, want: &VoiceWant<'_>);
    /// Commandes vocales qui concernent l'audio (volume et muet par
    /// personne, appui-pour-parler) ; les autres sont ignorées.
    fn command(&mut self, c: &VoiceCmd);
    /// Demande les codes de vérification (réponse par [`VoiceEvent::Codes`]).
    fn request_codes(&mut self);
    /// Ce qui s'est passé depuis le dernier appel.
    fn poll(&mut self) -> Vec<VoiceEvent>;
    /// Une fois par passe, avant [`VoiceLink::sync`] (l'appel ouvert prend
    /// alors la file d'images et la caméra voulues) : caméra, partage,
    /// directs regardés, qualités voulues (lot M5).
    fn sync_video(&mut self, _want: &super::video_link::VideoWant<'_>) {}
    /// Ce que la vidéo a donné depuis le dernier appel.
    fn poll_video(&mut self) -> Vec<super::video_link::VideoUp> {
        Vec::new()
    }
    /// Joue un son court (tableau de sons, message vocal), dans l'appel ou,
    /// hors appel, par la sortie audio seule (phase 6).
    fn play_clip(&mut self, _p: super::voice_msg::ClipPlay) {}
    fn stop_clip(&mut self, _k: super::voice_msg::ClipKey) {}
    /// Enregistrement d'un message vocal.
    fn record(&mut self, _c: super::voice_msg::RecordCmd) {}
    /// Une fois par passe : touche de l'appui-pour-parler, pour le crochet
    /// clavier global (actif seulement pendant un appel en appui-pour-parler).
    fn set_ptt_key(&mut self, _k: Option<super::voice_msg::PttKey<'_>>) {}
    /// Sons et enregistrements depuis le dernier appel.
    fn poll_clips(&mut self) -> Vec<super::voice_msg::ClipUp> {
        Vec::new()
    }
    /// Télécharge et joue un fichier vidéo ou audio joint à un message (lot
    /// M) ; un seul à la fois. `gif` : vidéo d'un GIF (Tenor…), lue en boucle
    /// et sans son (lot VID). `None` : pas de lecteur (aperçu, tests).
    fn open_media(&mut self, _url: &str, _mime: Option<&str>, _gif: bool) -> Option<crate::video::play::Handle> {
        None
    }
    /// Arrête le fichier en lecture.
    fn close_media(&mut self) {}
    /// Enregistrement local de l'appel (plugin, lot E4) : démarrer, noms des
    /// participants, arrêter. Jamais appelé sans un clic de l'utilisateur.
    fn call_record(&mut self, _c: super::call_rec::CallRecCmd) {}
    /// Issue des enregistrements depuis le dernier appel.
    fn poll_call_record(&mut self) -> Vec<super::call_rec::CallRecUp> {
        Vec::new()
    }
}

/// Sans vocal (aperçu, tests de l'interface).
#[derive(Default)]
pub struct NoVoice;

impl VoiceLink for NoVoice {
    fn sync(&mut self, _want: &VoiceWant<'_>) {}
    fn command(&mut self, _c: &VoiceCmd) {}
    fn request_codes(&mut self) {}
    fn poll(&mut self) -> Vec<VoiceEvent> {
        Vec::new()
    }
}
