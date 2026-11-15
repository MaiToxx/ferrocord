//! Chiffrement de bout en bout des trames audio, branché sur la connexion
//! vocale. La connexion ne connaît que ce trait : elle lui relaie les
//! opcodes 21 à 31 (DAVE) et l'appelle sur chaque trame Opus, avant le
//! chiffrement du transport à l'envoi et après lui à la réception.
//! `Passthrough` sert quand DAVE n'est pas disponible (version 0).

/// Message que le chiffrement de bout en bout demande d'envoyer sur la
/// passerelle vocale.
#[derive(Clone, Debug, PartialEq)]
pub enum Outgoing {
    /// Paquet JSON complet (`op` et `d`), envoyé tel quel.
    Json(serde_json::Value),
    /// Paquet binaire : opcode puis charge utile (le client n'envoie pas de numéro de séquence).
    Binary(u8, Vec<u8>),
}

pub trait FrameCrypto: Send {
    /// 0 = pas de DAVE ; envoyé dans l'Identify (`max_dave_protocol_version`).
    fn max_protocol_version(&self) -> u16;
    /// Opcodes 21 à 31, en JSON (`d`) ou binaires (charge utile, sans
    /// séquence ni opcode). La Session Description (op 4) est relayée aussi :
    /// elle porte `dave_protocol_version`, la version choisie par le serveur.
    fn on_op(&mut self, op: u8, json: Option<&serde_json::Value>, binary: Option<&[u8]>, out: &mut Vec<Outgoing>);
    /// Participants connus de la connexion (moi non compris), après chaque arrivée ou départ.
    fn on_users(&mut self, connected: &[u64]);
    /// Avant le chiffrement du transport ; `None` : trame non envoyée.
    fn encrypt(&mut self, opus: &[u8]) -> Option<Vec<u8>>;
    /// Après le déchiffrement du transport ; `None` : trame ignorée.
    fn decrypt(&mut self, user_id: u64, frame: &[u8]) -> Option<Vec<u8>>;
    /// Code de vérification affichable de la session avec `user_id`.
    fn verification_code(&self, user_id: u64) -> Option<String>;
    /// Image vidéo entière (Annex B pour H.264), avant la mise en paquets ;
    /// les octets que lit le paquétiseur restent en clair. `None` : image non envoyée.
    fn encrypt_video(&mut self, codec: crate::video::Codec, frame: &[u8]) -> Option<Vec<u8>>;
    /// Image vidéo réassemblée ; `None` : image ignorée.
    fn decrypt_video(&mut self, user_id: u64, codec: crate::video::Codec, frame: &[u8]) -> Option<Vec<u8>>;
}

/// Aucun chiffrement de bout en bout : les trames passent telles quelles.
#[derive(Clone, Copy, Debug, Default)]
pub struct Passthrough;

impl FrameCrypto for Passthrough {
    fn max_protocol_version(&self) -> u16 {
        0
    }

    fn on_op(&mut self, _op: u8, _json: Option<&serde_json::Value>, _binary: Option<&[u8]>, _out: &mut Vec<Outgoing>) {}

    fn on_users(&mut self, _connected: &[u64]) {}

    fn encrypt(&mut self, opus: &[u8]) -> Option<Vec<u8>> {
        Some(opus.to_vec())
    }

    fn decrypt(&mut self, _user_id: u64, frame: &[u8]) -> Option<Vec<u8>> {
        Some(frame.to_vec())
    }

    fn encrypt_video(&mut self, _codec: crate::video::Codec, frame: &[u8]) -> Option<Vec<u8>> {
        Some(frame.to_vec())
    }

    fn decrypt_video(&mut self, _user_id: u64, _codec: crate::video::Codec, frame: &[u8]) -> Option<Vec<u8>> {
        Some(frame.to_vec())
    }

    fn verification_code(&self, _user_id: u64) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_is_identity() {
        let mut p = Passthrough;
        assert_eq!(p.max_protocol_version(), 0);
        assert_eq!(p.encrypt(&[1, 2, 3]).as_deref(), Some(&[1u8, 2, 3][..]));
        assert_eq!(p.decrypt(7, &[4, 5]).as_deref(), Some(&[4u8, 5][..]));
        let mut out = Vec::new();
        p.on_op(25, None, Some(&[1]), &mut out);
        assert!(out.is_empty());
        assert_eq!(p.verification_code(7), None);
    }
}
