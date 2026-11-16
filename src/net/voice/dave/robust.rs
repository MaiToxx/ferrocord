//! Lot « fiable » : trames et messages DAVE hostiles (tronqués, abîmés,
//! bruit). Rien ne doit paniquer : une trame illisible est une trame perdue.

use super::frame::{parse, sender_base, Decryptor, Encryptor, EpochKeys};
use super::*;
use crate::net::voice::robust::hostile;
use crate::video::Codec;

#[test]
fn trames_et_messages_hostiles_sans_panique() {
    let root = vec![9u8; 32];
    let mut e = Encryptor::default();
    e.set_base(Some(sender_base(&root, 42)));
    let mut d = Decryptor::default();
    let now = Instant::now();
    d.install(EpochKeys::new(root, vec![42]), now);
    let audio = e.encrypt(&[7u8; 80]).unwrap_or_default();
    // Trame vidéo avec plages en clair : la partie « supplémentaire » porte des longueurs.
    let ranged = e.encrypt_ranges(&[5u8; 200], &[(0, 10), (50, 4)], false).unwrap_or_default();
    assert!(!audio.is_empty() && !ranged.is_empty());
    let cases = hostile(&[audio, ranged]);

    let mut session = DaveSession::new(1, 2).unwrap_or_else(|e| panic!("session DAVE : {e}"));
    let mut out = Vec::new();
    let odd = [json!(null), json!({}), json!({ "transition_id": "x", "protocol_version": -1, "epoch": [] }), json!({ "transition_id": 1, "protocol_version": 1, "epoch": 1 })];
    for p in &cases {
        if let Some(parsed) = parse(p) {
            let _ = d.decrypt(42, &parsed, now);
            let _ = d.decrypt(7, &parsed, now);
        }
        for codec in [Codec::H264, Codec::Vp8, Codec::Vp9, Codec::Av1] {
            let _ = super::video::layout(codec, p);
            let _ = session.decrypt_video(42, codec, p);
        }
        let _ = FrameCrypto::decrypt(&mut session, 42, p);
        // Ops binaires de la passerelle vocale (MLS : clés, propositions, Commit, Welcome).
        for op in 21..=31u8 {
            session.on_op(op, None, Some(p), &mut out);
        }
        out.clear();
    }
    for v in &odd {
        for op in [4u8, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31] {
            session.on_op(op, Some(v), None, &mut out);
        }
    }
}
