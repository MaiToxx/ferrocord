//! Lecture à voix haute d'un message (lot R du groupe 3) : la synthèse
//! vocale de Windows (`Windows.Media.SpeechSynthesis`, voix et langue du
//! système), jouée par un `MediaPlayer`. Rien n'est embarqué et rien ne
//! quitte la machine. La synthèse se fait dans un fil à part ; une nouvelle
//! lecture arrête la précédente. Hors de Windows : sans effet.

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
    use std::sync::Mutex;
    use windows::core::HSTRING;
    use windows::Media::Core::MediaSource;
    use windows::Media::Playback::{MediaPlaybackState, MediaPlayer};
    use windows::Media::SpeechSynthesis::SpeechSynthesizer;

    /// Lecteur de la lecture en cours.
    static PLAYER: Mutex<Option<MediaPlayer>> = Mutex::new(None);
    /// Numéro de la dernière lecture demandée (0 : aucune) ; une synthèse
    /// dépassée par une autre demande, ou arrêtée, n'est pas jouée.
    static TURN: AtomicU64 = AtomicU64::new(0);
    /// Lecture en cours de synthèse (pas encore de lecteur).
    static PREPARING: AtomicU64 = AtomicU64::new(0);

    pub fn speak(text: String, rate: f64) {
        stop();
        let turn = TURN.fetch_add(1, Relaxed) + 1;
        PREPARING.store(turn, Relaxed);
        let spawned = std::thread::Builder::new().name("voix".into()).spawn(move || {
            // Voix absente ou synthèse refusée : rien n'est lu, rien n'est dit.
            let _ = run(&text, turn, rate);
            let _ = PREPARING.compare_exchange(turn, 0, Relaxed, Relaxed);
        });
        if spawned.is_err() {
            PREPARING.store(0, Relaxed);
        }
    }

    fn run(text: &str, turn: u64, rate: f64) -> windows::core::Result<()> {
        let synth = SpeechSynthesizer::new()?;
        // Débit réglable (Windows 10 1709 et suivants) ; refusé : débit normal.
        if (rate - 1.0).abs() > f64::EPSILON {
            let _ = synth.Options().and_then(|o| o.SetSpeakingRate(rate));
        }
        let stream = synth.SynthesizeTextToStreamAsync(&HSTRING::from(text))?.join()?;
        let source = MediaSource::CreateFromStream(&stream, &stream.ContentType()?)?;
        let player = MediaPlayer::new()?;
        player.SetSource(&source)?;
        let Ok(mut slot) = PLAYER.lock() else { return Ok(()) };
        // Arrêtée ou remplacée pendant la synthèse : rien n'est joué.
        if TURN.load(Relaxed) != turn || PREPARING.load(Relaxed) != turn {
            return Ok(());
        }
        player.Play()?;
        *slot = Some(player);
        Ok(())
    }

    pub fn stop() {
        PREPARING.store(0, Relaxed);
        if let Ok(mut slot) = PLAYER.lock()
            && let Some(player) = slot.take()
        {
            let _ = player.Pause();
            let _ = player.Close();
        }
    }

    pub fn speaking() -> bool {
        if PREPARING.load(Relaxed) != 0 {
            return true;
        }
        let Ok(slot) = PLAYER.lock() else { return false };
        let state = slot.as_ref().and_then(|p| p.PlaybackSession().ok()).and_then(|s| s.PlaybackState().ok());
        matches!(state, Some(MediaPlaybackState::Playing | MediaPlaybackState::Opening | MediaPlaybackState::Buffering))
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn speak(_: String, _: f64) {}
    pub fn stop() {}
    pub fn speaking() -> bool {
        false
    }
}

/// Texte lu au plus (caractères) : un message de Discord en compte 4 000 au maximum.
pub const MAX_SPOKEN: usize = 4000;

/// La machine sait-elle lire à voix haute ?
pub const AVAILABLE: bool = cfg!(windows);

/// Lit `text` à voix haute ; la lecture en cours est arrêtée.
pub fn speak(text: &str) {
    let text: String = text.chars().filter(|c| !c.is_control() || *c == '\n').take(MAX_SPOKEN).collect();
    if !text.trim().is_empty() {
        imp::speak(text, rate_of(crate::plugins::number(crate::plugins::Id::ReadAloud, crate::plugins::READ_ALOUD_RATE)));
    }
}

/// Débit demandé à Windows pour un réglage en pour cent : de 0,5 à 3 fois le débit normal.
fn rate_of(percent: i64) -> f64 {
    percent.clamp(50, 300) as f64 / 100.0
}

/// Arrête la lecture en cours.
pub fn stop() {
    imp::stop();
}

/// Une lecture est-elle en cours (synthèse comprise) ?
pub fn speaking() -> bool {
    imp::speaking()
}
