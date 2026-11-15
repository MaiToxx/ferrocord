//! Charge utile RTP VP8, RFC 7741 : descripteur d'un octet à l'envoi (S
//! sur le premier paquet, partition 0) ; à la réception, les extensions
//! facultatives du descripteur (X : I, L, T, K) sont sautées.

/// Charges utiles RTP d'une image, `max` octets au plus chacune.
pub fn packetize(frame: &[u8], max: usize) -> Vec<Vec<u8>> {
    let chunk = max.max(2) - 1;
    frame
        .chunks(chunk)
        .enumerate()
        .map(|(k, part)| {
            let mut p = Vec::with_capacity(1 + part.len());
            p.push(if k == 0 { 0x10 } else { 0 });
            p.extend_from_slice(part);
            p
        })
        .collect()
}

/// Longueur du descripteur (RFC 7741, 4.2) et bit S.
fn descriptor(p: &[u8]) -> Option<(usize, bool)> {
    let b0 = *p.first()?;
    let start = b0 & 0x10 != 0 && b0 & 0x07 == 0;
    let mut n = 1;
    if b0 & 0x80 != 0 {
        let x = *p.get(1)?;
        n = 2;
        if x & 0x80 != 0 {
            n += if *p.get(n)? & 0x80 != 0 { 2 } else { 1 };
        }
        if x & 0x40 != 0 {
            n += 1;
        }
        if x & 0x30 != 0 {
            n += 1;
        }
    }
    (n <= p.len()).then_some((n, start))
}

/// Image VP8 à partir des charges utiles d'une image, dans l'ordre.
pub fn depacketize<'a>(payloads: impl IntoIterator<Item = &'a [u8]>) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for (k, p) in payloads.into_iter().enumerate() {
        let (n, start) = descriptor(p)?;
        if k == 0 && !start {
            return None;
        }
        out.extend_from_slice(&p[n..]);
    }
    (!out.is_empty()).then_some(out)
}

/// Image clé : bit P (bit 0 du premier octet) à 0 (RFC 6386, 9.1).
pub fn is_keyframe(frame: &[u8]) -> bool {
    frame.first().is_some_and(|b| b & 1 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aller_retour() {
        let frame: Vec<u8> = (0..20_000u32).map(|i| (i * 7) as u8).collect();
        let p = packetize(&frame, 1100);
        assert!(p.iter().all(|p| p.len() <= 1100));
        assert_eq!(p[0][0], 0x10);
        assert_eq!(p[1][0], 0);
        assert_eq!(depacketize(p.iter().map(Vec::as_slice)), Some(frame.clone()));
        assert!(is_keyframe(&frame));
        assert!(!is_keyframe(&[0x01, 2]));
    }

    use super::super::recv::{Incoming, Output, Receiver};
    use crate::video::Codec;
    use std::time::{Duration, Instant};

    struct Fixture {
        /// (horodatage, image clé, image VP8).
        frames: Vec<(u32, bool, Vec<u8>)>,
        packets: Vec<Incoming>,
    }

    /// `tests/fixtures/vp8/rtp_stream.json` : six images VP8 réelles (clés
    /// aux images 0 et 3), 17 paquets RTP au descripteur étendu, numéros
    /// qui rebouclent (écrit par `ecrit_jeu_rtp_vp8`).
    fn fixture() -> Fixture {
        let path = format!("{}/tests/fixtures/vp8/rtp_stream.json", env!("CARGO_MANIFEST_DIR"));
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).expect("jeu d'essai")).expect("json");
        let unhex = |s: &serde_json::Value| -> Vec<u8> {
            let s = s.as_str().expect("hex");
            (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex")).collect()
        };
        let num = |v: &serde_json::Value| v.as_u64().expect("nombre");
        let frames = v["frames"].as_array().expect("frames").iter().map(|f| (num(&f["ts"]) as u32, f["keyframe"] == true, unhex(&f["data"]))).collect();
        let packets = v["packets"]
            .as_array()
            .expect("packets")
            .iter()
            .map(|p| Incoming { seq: num(&p["seq"]) as u16, ts: num(&p["ts"]) as u32, marker: p["marker"] == true, payload: unhex(&p["payload"]) })
            .collect();
        Fixture { frames, packets }
    }

    fn feed(r: &mut Receiver, pkts: &[Incoming], now: Instant) -> Output {
        let mut out = Output::default();
        for p in pkts {
            r.push(p.clone(), now, &mut out);
        }
        out
    }

    /// Fin factice d'une image précédente, pour que le récepteur se cale.
    fn primed(first_seq: u16, now: Instant) -> Receiver {
        let mut r = Receiver::new(Codec::Vp8);
        feed(&mut r, &[Incoming { seq: first_seq.wrapping_sub(1), ts: 0, marker: true, payload: vec![0x10] }], now);
        r
    }

    #[test]
    fn jeu_rtp_depaquete() {
        let fx = fixture();
        assert_eq!(fx.frames.len(), 6);
        assert_eq!(fx.packets.len(), 17);
        for (ts, key, data) in &fx.frames {
            let parts: Vec<&[u8]> = fx.packets.iter().filter(|p| p.ts == *ts).map(|p| p.payload.as_slice()).collect();
            assert!(parts.len() > 1, "image en plusieurs paquets");
            assert_eq!(depacketize(parts).as_ref(), Some(data));
            assert_eq!(is_keyframe(data), *key);
        }
        // Notre propre mise en paquets redonne les mêmes images.
        for (_, _, data) in &fx.frames {
            assert_eq!(depacketize(packetize(data, 1100).iter().map(Vec::as_slice)).as_ref(), Some(data));
        }
    }

    #[test]
    fn jeu_rtp_desordre_et_perte_reparee() {
        let fx = fixture();
        let now = Instant::now();
        let mut r = primed(fx.packets[0].seq, now);
        let mut pkts = fx.packets.clone();
        pkts.swap(1, 2);
        pkts.swap(7, 9);
        // Un paquet de la deuxième image perdu, puis retransmis.
        let lost = pkts.remove(4);
        let mut out = feed(&mut r, &pkts, now);
        assert!(out.nack.contains(&lost.seq), "paquet manquant redemandé");
        assert!(!out.frames.is_empty() && out.frames.iter().all(|f| f.ts < lost.ts), "la suite attend le paquet perdu");
        r.push(lost, now + Duration::from_millis(30), &mut out);
        let got: Vec<(u32, bool, Vec<u8>)> = out.frames.into_iter().map(|f| (f.ts, f.keyframe, f.data)).collect();
        assert_eq!(got, fx.frames);
    }

    #[test]
    fn jeu_rtp_image_cle_perdue() {
        let fx = fixture();
        let now = Instant::now();
        // La première image clé n'arrive jamais : on se cale sur la fin de
        // l'image 1, l'image 2 (intermédiaire) est jetée, l'image clé 3 repart.
        let start = fx.packets.iter().position(|p| p.ts == fx.frames[1].0).expect("image 1");
        let mut r = Receiver::new(Codec::Vp8);
        let out = feed(&mut r, &fx.packets[start..], now);
        assert!(out.pli, "image clé demandée");
        let got: Vec<u32> = out.frames.iter().map(|f| f.ts).collect();
        assert_eq!(got, [fx.frames[3].0, fx.frames[4].0, fx.frames[5].0]);
        assert!(out.frames[0].keyframe);
    }

    #[test]
    fn jeu_rtp_decode_par_windows() {
        let Some(mut dec) = crate::video::codec::decoder(Codec::Vp8) else {
            eprintln!("décodeur VP8 absent : sauté");
            return;
        };
        let fx = fixture();
        for (ts, keyframe, data) in fx.frames {
            let f = crate::video::EncodedFrame { codec: Codec::Vp8, keyframe, ts90k: ts, data };
            let img = dec.decode(&f).expect("décodage").expect("image");
            assert_eq!((img.width, img.height), (96, 64));
        }
    }

    #[test]
    fn descripteur_etendu() {
        // X, I (identifiant d'image sur 15 bits), L, T : 6 octets de descripteur.
        let p = [0x90, 0xE0, 0x81, 0x23, 0x05, 0x40, 0xAA, 0xBB];
        assert_eq!(depacketize([&p[..]]), Some(vec![0xAA, 0xBB]));
        // Premier paquet sans S : début d'image manquant.
        assert_eq!(depacketize([&[0x00u8, 1][..]]), None);
    }
}
