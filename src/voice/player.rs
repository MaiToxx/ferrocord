//! Lecture d'un fichier vidéo ou audio joint à un message (phase 10, lot M) :
//! téléchargement (CDN de Discord seulement, proxy des médias externes
//! compris pour les vidéos des embeds ; borné), puis `video::play` sur
//! son propre fil. Le son sort par le fil de lecture en place : celui de
//! l'appel s'il y en a un, sinon la sortie seule des sons courts, ouverte
//! tant qu'un fichier est chargé, en lecture ou en pause.
//!
//! Un seul fichier à la fois : en ouvrir un ferme le précédent.

use super::Voice;
use crate::i18n::table::messages::t as pt;
use crate::i18n::table::voice_msg::t as mt;
use crate::video::play::{self, AudioOut, Handle, OpenError, Phase};
use std::sync::{Arc, Mutex, PoisonError};

/// Fichier téléchargé, au plus (tenu en mémoire le temps de la lecture).
pub(super) const MAX_FILE: usize = play::MAX_BYTES;

/// Son du fichier en lecture, lu par le fil de lecture audio à chaque rappel.
static OUTPUT: Mutex<Option<Arc<AudioOut>>> = Mutex::new(None);

/// Fil de lecture audio : ajoute le son du fichier à `out`. Jamais d'attente
/// (verrou pris au vol ; il n'est tenu ailleurs que le temps d'un échange).
pub(super) fn mix(out: &mut [f32]) {
    if let Ok(g) = OUTPUT.try_lock()
        && let Some(a) = g.as_ref()
    {
        a.mix(out);
    }
}

fn set_output(a: Option<Arc<AudioOut>>) {
    *OUTPUT.lock().unwrap_or_else(PoisonError::into_inner) = a;
}

#[derive(Default)]
pub(super) struct Files {
    current: Option<Handle>,
    /// La sortie audio était voulue au dernier relevé.
    wanted: bool,
}

impl Files {
    /// Un fichier a besoin de la sortie audio (chargé, en lecture ou en pause).
    pub(super) fn wants_output(&self) -> bool {
        self.current.as_ref().is_some_and(|h| !h.is_closed() && !h.is_gif() && matches!(h.snapshot().phase, Phase::Loading | Phase::Playing | Phase::Paused))
    }
}

impl Voice {
    /// Télécharge et joue `url` ; l'état se lit dans la poignée rendue.
    /// `gif` : en boucle et sans son (la sortie audio n'est pas ouverte).
    pub(super) fn file_open(&mut self, url: &str, mime: Option<&str>, gif: bool) -> Handle {
        self.file_close();
        let lang = crate::i18n::language();
        let h = Handle::loading(self.wake.clone());
        if !super::clips::allowed(url) && !crate::net::cdn::served_by_discord(url) {
            h.fail(mt::BAD_ADDRESS.get(lang).into());
            return h;
        }
        if gif {
            h.set_gif();
        } else {
            set_output(Some(h.audio()));
        }
        self.files.current = Some(h.clone());
        let client = self.http();
        let (url, mime, task) = (url.to_string(), mime.map(str::to_string), h.clone());
        self.rt.spawn(async move {
            match super::clips::download(client, &url, MAX_FILE).await {
                Ok(_) if task.is_closed() => {}
                Ok(bytes) => task.start(move || {
                    play::open(bytes, mime.as_deref()).map_err(|e| {
                        let lang = crate::i18n::language();
                        match e {
                            OpenError::Unsupported => pt::PLAYER_UNSUPPORTED.get(lang).to_string(),
                            OpenError::NoDecoder => pt::PLAYER_NO_DECODER.get(lang).to_string(),
                        }
                    })
                }),
                Err(e) => task.fail(e),
            }
        });
        self.file_tick();
        h
    }

    pub(super) fn file_close(&mut self) {
        if let Some(h) = self.files.current.take() {
            h.close();
            set_output(None);
            self.file_tick();
        }
    }

    /// Une fois par passe : ouvre ou ferme la sortie seule selon l'état du fichier.
    pub(super) fn file_tick(&mut self) {
        let want = self.files.wants_output();
        if want != self.files.wanted {
            self.files.wanted = want;
            self.sync_output();
        }
    }
}
