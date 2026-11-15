//! Sons courts et messages vocaux (phase 6) : ce que l'interface demande à
//! l'orchestrateur du vocal (`crate::voice`) et ce qu'il rend. Lecture des
//! sons du tableau et des messages vocaux reçus, enregistrement d'un
//! message vocal, touche de l'appui-pour-parler global.
//!
//! Format des messages vocaux (docs.discord.food, « Message Resource »,
//! sections « Voice Messages » et « Attachment Object ») : indicateur de
//! message `IS_VOICE_MESSAGE` (1 << 13), une seule pièce jointe `audio/…`,
//! ni texte ni autre pièce jointe ; la pièce jointe porte `duration_secs`
//! (flottant) et `waveform` (base64, un octet par point, 256 points au plus).

use std::sync::Arc;

/// `IS_VOICE_MESSAGE` (message).
pub const IS_VOICE_MESSAGE: u64 = 1 << 13;
/// Nom donné par les clients officiels ; l'extension fixe le type `audio/ogg`.
pub const FILE_NAME: &str = "voice-message.ogg";

/// Ce qui fait d'une pièce jointe un message vocal.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct VoiceNote {
    pub duration_ms: u32,
    /// Un octet par point (0 à 255).
    pub waveform: Vec<u8>,
}

impl VoiceNote {
    /// D'après les champs reçus ; `None` si aucun des deux n'est là.
    pub fn parse(duration_secs: Option<f64>, waveform: Option<&str>) -> Option<VoiceNote> {
        if duration_secs.is_none() && waveform.is_none() {
            return None;
        }
        let duration_ms = duration_secs.filter(|d| d.is_finite() && *d > 0.0).map_or(0, |d| (d * 1000.0).min(f64::from(u32::MAX)) as u32);
        let mut waveform = waveform.and_then(super::usersettings::b64::decode).unwrap_or_default();
        waveform.truncate(256);
        Some(VoiceNote { duration_ms, waveform })
    }
}

/// Adresse d'un son du tableau (documentation de Discord, « CDN Endpoints » :
/// `soundboard-sounds/{sound_id}`).
pub fn sound_url(sound_id: u64) -> String {
    format!("https://cdn.discordapp.com/soundboard-sounds/{sound_id}")
}

/// Ce qu'un son joue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClipKey {
    /// Son du tableau (numéro de lecture : plusieurs peuvent se superposer).
    Sound(u64),
    /// Message vocal (identifiant de la pièce jointe).
    Note(u64),
}

/// Son à jouer.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipPlay {
    pub key: ClipKey,
    pub url: String,
    /// 0 à 1, multiplié par le volume de sortie.
    pub volume: f32,
    /// Reprise à cette position.
    pub from_ms: u32,
}

/// Enregistrement d'un message vocal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordCmd {
    Start,
    /// Arrête et rend le message ([`ClipUp::Recorded`]).
    Finish,
    /// Arrête et jette.
    Cancel,
}

/// Message vocal enregistré, prêt à partir.
#[derive(Clone, PartialEq)]
pub struct Recorded {
    pub ogg: Arc<[u8]>,
    pub duration_ms: u32,
    pub waveform: Vec<u8>,
}

impl std::fmt::Debug for Recorded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Recorded({} octets, {} ms)", self.ogg.len(), self.duration_ms)
    }
}

/// Orchestrateur -> interface.
#[derive(Clone, Debug, PartialEq)]
pub enum ClipUp {
    /// Le son commence (décodé, envoyé à la sortie).
    Started { key: ClipKey, duration_ms: u32 },
    Ended(ClipKey),
    /// Téléchargement ou décodage impossible, ou aucune sortie audio.
    Failed { key: ClipKey, why: String },
    Recorded(Recorded),
    RecordFailed(String),
}

/// Touche de l'appui-pour-parler (nom de touche d'egui et modificateurs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PttKey<'a> {
    pub key: &'a str,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_note_fields_are_lenient() {
        assert_eq!(VoiceNote::parse(None, None), None);
        let n = VoiceNote::parse(Some(3.52), Some("AP8=")).unwrap();
        assert_eq!(n, VoiceNote { duration_ms: 3520, waveform: vec![0, 255] });
        let n = VoiceNote::parse(Some(f64::NAN), Some("pas du base64 !")).unwrap();
        assert_eq!(n, VoiceNote::default());
        assert_eq!(sound_url(5), "https://cdn.discordapp.com/soundboard-sounds/5");
    }
}
