//! Vidéo dans la connexion vocale (lot M1) : négociation des codecs,
//! op 12 Video, RTP vidéo, RTX, NACK, PLI et FIR. Pièce pure : la
//! connexion (`conn.rs`) lui passe les paquets déchiffrés et envoie ce
//! qu'elle rend.
//!
//! Formats non documentés par Discord, vérifiés sur docs.discord.food
//! (Topics > Voice Connections) :
//! - Identify : `video` (booléen) et `streams` (`type` `video` ou `screen`,
//!   `rid`, `quality`) ; Ready renvoie les `streams` avec `ssrc` et
//!   `rtx_ssrc` attribués (sans `rtx_ssrc` : `ssrc + 1`) ;
//! - Select Protocol : `codecs` (`name`, `type`, `priority` multiple de 1000
//!   unique par type, Opus à 1000, `payload_type`, `rtx_payload_type`,
//!   `encode`, `decode`) ; jamais le type de charge 96 (sondes) ;
//! - Session Description : `video_codec`, le codec à envoyer ;
//! - op 12 Video : à l'envoi `audio_ssrc`, `video_ssrc`, `rtx_ssrc`,
//!   `streams` (de type `video` même pour un direct : `screen` ne sert que
//!   dans l'Identify) ; à la réception `user_id`, `audio_ssrc`, `video_ssrc`,
//!   `streams` (avec `rtx_ssrc`) ; `video_ssrc` à 0 : vidéo coupée ;
//! - RTP vidéo : horloge à 90 kHz, marqueur sur le dernier paquet d'une
//!   image ; NACK servis par RTX (numéro d'origine en tête de charge).

pub mod av1;
pub mod h264;
pub mod recv;
pub mod rtcp;
pub mod send;
pub mod vp8;
pub mod vp9;

use std::collections::HashMap;
use std::time::Instant;

use serde_json::{Value, json};

use super::rtp;
use crate::video::Codec;
use recv::{Assembled, Incoming, Receiver};
use send::{RtpOut, Sender};

/// Un codec vidéo proposé dans Select Protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodecOffer {
    pub codec: Codec,
    pub payload_type: u8,
    pub rtx_payload_type: u8,
    pub encode: bool,
    pub decode: bool,
}

impl CodecOffer {
    /// Types de charge de l'exemple de docs.discord.food (AV1 101/102,
    /// H264 103/104), complétés pour VP8 et VP9.
    pub fn new(codec: Codec, encode: bool, decode: bool) -> Self {
        let (payload_type, rtx_payload_type) = match codec {
            Codec::Av1 => (101, 102),
            Codec::H264 => (103, 104),
            Codec::Vp8 => (105, 106),
            Codec::Vp9 => (107, 108),
        };
        CodecOffer { codec, payload_type, rtx_payload_type, encode, decode }
    }

    /// Codecs mis en paquets ici à l'envoi. VP9 et AV1 ne sont que reçus.
    pub fn packetizable(self) -> bool {
        matches!(self.codec, Codec::H264 | Codec::Vp8)
    }

    /// Ce que l'offre annonce vraiment : (encode, décode).
    pub fn usable(self) -> (bool, bool) {
        (self.encode && self.packetizable(), self.decode)
    }
}

/// Offre par défaut : H.264 seul, que toute machine encode et décode
/// (OpenH264) ; un appel la remplace par [`offers`] de son moteur vidéo.
pub fn default_offers() -> Vec<CodecOffer> {
    vec![CodecOffer::new(Codec::H264, true, true)]
}

/// Offre d'après ce que la machine sait encoder et décoder, H.264 d'abord
/// (encodeur matériel, et le choix de Discord), puis VP8, puis VP9 et AV1,
/// en réception seulement.
pub fn offers(encode: &[Codec], decode: &[Codec]) -> Vec<CodecOffer> {
    [Codec::H264, Codec::Vp8, Codec::Vp9, Codec::Av1]
        .into_iter()
        .map(|c| CodecOffer::new(c, encode.contains(&c), decode.contains(&c)))
        .map(|o| CodecOffer { encode: o.usable().0, ..o })
        .filter(|o| o.encode || o.decode)
        .collect()
}

/// `codecs` de Select Protocol : Opus, puis la vidéo dans l'ordre de préférence.
pub fn select_codecs(offers: &[CodecOffer]) -> Value {
    let mut list = vec![json!({"name": "opus", "type": "audio", "priority": 1000, "payload_type": rtp::PAYLOAD_TYPE})];
    for (k, o) in offers.iter().filter(|o| o.usable() != (false, false)).enumerate() {
        list.push(json!({
            "name": o.codec.name(),
            "type": "video",
            "priority": 1000 * (k + 1),
            "payload_type": o.payload_type,
            "rtx_payload_type": o.rtx_payload_type,
            "encode": o.usable().0,
            "decode": o.decode,
        }));
    }
    Value::Array(list)
}

/// `streams` de l'Identify : un seul flux (pas de diffusion multiple), de
/// type `screen` pour un direct.
pub fn identify_streams(screen: bool) -> Value {
    json!([{"type": if screen { "screen" } else { "video" }, "rid": "100", "quality": 100}])
}

/// Débit maximal annoncé : environ 0,1 bit par pixel, borné (au-dessus de
/// ce que vise l'encodeur, jusqu'à 18 Mb/s pour un partage en 4K).
fn max_bitrate(width: u32, height: u32, fps: u32) -> u64 {
    (u64::from(width) * u64::from(height) * u64::from(fps.max(1)) / 10).clamp(300_000, 20_000_000)
}

/// En-tête d'un paquet RTP reçu.
#[derive(Clone, Copy, Debug)]
pub struct RtpMeta {
    pub ssrc: u32,
    pub payload_type: u8,
    pub seq: u16,
    pub ts: u32,
    pub marker: bool,
}

/// Flux vidéo d'un autre participant.
struct Remote {
    user: u64,
    rx: Option<Receiver>,
}

/// Ce qu'a donné un paquet vidéo reçu.
#[derive(Default, Debug)]
pub struct Received {
    pub user_id: u64,
    pub codec: Option<Codec>,
    pub frames: Vec<Assembled>,
    /// RTCP en clair à envoyer (NACK, PLI).
    pub rtcp: Vec<Vec<u8>>,
}

/// Ce qu'a donné un RTCP reçu.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Feedback {
    pub retransmit: Vec<RtpOut>,
    pub keyframe: bool,
}

#[derive(Default)]
pub struct Link {
    offers: Vec<CodecOffer>,
    screen: bool,
    audio_ssrc: u32,
    video_ssrc: u32,
    rtx_ssrc: u32,
    /// Codec d'envoi (Session Description).
    send_codec: Option<Codec>,
    sender: Option<Sender>,
    /// Vidéo annoncée par op 12 : (largeur, hauteur, images/s).
    active: Option<(u32, u32, u32)>,
    remotes: HashMap<u32, Remote>,
    rtx: HashMap<u32, u32>,
    fir_seq: u8,
    /// Qualité voulue par personne (op 15), renvoyée quand ses SSRC changent.
    wants: HashMap<u64, u8>,
}

impl Link {
    pub fn new(offers: Vec<CodecOffer>, screen: bool) -> Self {
        Link { offers, screen, ..Link::default() }
    }

    /// La connexion propose la vidéo (Identify `video`).
    pub fn enabled(&self) -> bool {
        self.offers.iter().any(|o| o.usable() != (false, false))
    }

    pub fn offers(&self) -> &[CodecOffer] {
        &self.offers
    }

    /// Octets par seconde à ne pas dépasser en envoyant une image : quatre
    /// fois le débit annoncé. Une image clé en taille source part ainsi
    /// étalée sur un quart de seconde au lieu d'une seule rafale, que les
    /// routeurs et le serveur ne tiennent pas (paquets perdus, image figée).
    pub fn pace_rate(&self) -> u64 {
        let (w, h, fps) = self.active.unwrap_or((1280, 720, 30));
        max_bitrate(w, h, fps) / 2
    }

    /// Connexion d'un direct (Go Live).
    pub fn screen(&self) -> bool {
        self.screen
    }

    pub fn video_ssrc(&self) -> u32 {
        self.video_ssrc
    }

    /// Ready : SSRC vidéo et RTX attribués.
    pub fn on_ready(&mut self, d: &Value, audio_ssrc: u32) {
        self.audio_ssrc = audio_ssrc;
        let s = &d["streams"][0];
        let ssrc = s["ssrc"].as_u64().and_then(|v| u32::try_from(v).ok()).filter(|v| *v != 0);
        self.video_ssrc = ssrc.unwrap_or(audio_ssrc.wrapping_add(1));
        let rtx = s["rtx_ssrc"].as_u64().and_then(|v| u32::try_from(v).ok()).filter(|v| *v != 0);
        self.rtx_ssrc = rtx.unwrap_or(self.video_ssrc.wrapping_add(1));
    }

    /// Session Description ou Session Update : codec vidéo à envoyer.
    pub fn on_session(&mut self, d: &Value) {
        let Some(name) = d["video_codec"].as_str() else { return };
        let wanted = Codec::from_name(name);
        let offer = self.offers.iter().find(|o| Some(o.codec) == wanted && o.encode && o.packetizable());
        let codec = offer.or_else(|| self.offers.iter().find(|o| o.encode && o.packetizable())).map(|o| o.codec);
        if codec != self.send_codec {
            self.send_codec = codec;
            self.sender = None;
        }
    }

    /// Codec d'envoi : celui de la Session Description, à défaut le premier encodable.
    pub fn send_codec(&self) -> Option<Codec> {
        self.send_codec.or_else(|| self.offers.iter().find(|o| o.encode && o.packetizable()).map(|o| o.codec))
    }

    fn offer_for(&self, codec: Codec) -> Option<CodecOffer> {
        self.offers.iter().copied().find(|o| o.codec == codec)
    }

    /// op 12 à envoyer pour annoncer (ou couper) notre vidéo.
    pub fn set_state(&mut self, active: bool, width: u32, height: u32, fps: u32) -> Value {
        if !active {
            self.active = None;
            self.sender = None;
            return json!({"op": 12, "d": {"audio_ssrc": self.audio_ssrc, "video_ssrc": 0, "rtx_ssrc": 0, "streams": []}});
        }
        self.active = Some((width, height, fps));
        json!({"op": 12, "d": {
            "audio_ssrc": self.audio_ssrc,
            "video_ssrc": self.video_ssrc,
            "rtx_ssrc": self.rtx_ssrc,
            "streams": [{
                "type": "video",
                "rid": "100",
                "ssrc": self.video_ssrc,
                "rtx_ssrc": self.rtx_ssrc,
                "active": true,
                "quality": 100,
                "max_bitrate": max_bitrate(width, height, fps),
                "max_framerate": fps,
                "max_resolution": {"type": "fixed", "width": width, "height": height},
            }],
        }})
    }

    /// op 12 reçu : `(utilisateur, vidéo active)`.
    pub fn on_video_op(&mut self, d: &Value, me: u64) -> Option<(u64, bool)> {
        let user = match &d["user_id"] {
            Value::String(s) => s.parse().ok()?,
            v => v.as_u64()?,
        };
        if user == me {
            return None;
        }
        self.user_left(user);
        let ssrc32 = |v: &Value| v.as_u64().and_then(|v| u32::try_from(v).ok()).filter(|v| *v != 0);
        let streams = d["streams"].as_array().map(Vec::as_slice).unwrap_or_default();
        let video = ssrc32(&d["video_ssrc"]).or_else(|| streams.iter().find_map(|s| ssrc32(&s["ssrc"])));
        let any_active = streams.is_empty() || streams.iter().any(|s| s["active"].as_bool().unwrap_or(true));
        let Some(video) = video.filter(|_| any_active) else { return Some((user, false)) };
        self.remotes.insert(video, Remote { user, rx: None });
        let rtx = streams.iter().find(|s| ssrc32(&s["ssrc"]) == Some(video)).and_then(|s| ssrc32(&s["rtx_ssrc"]));
        self.rtx.insert(rtx.unwrap_or(video.wrapping_add(1)), video);
        Some((user, true))
    }

    pub fn user_left(&mut self, user: u64) {
        self.remotes.retain(|_, r| r.user != user);
        let remotes = &self.remotes;
        self.rtx.retain(|_, v| remotes.contains_key(v));
    }

    /// Le paquet de cette SSRC est de la vidéo (ou son RTX).
    pub fn owns(&self, ssrc: u32) -> bool {
        self.remotes.contains_key(&ssrc) || self.rtx.contains_key(&ssrc)
    }

    fn codec_of(&self, payload_type: u8) -> Option<(Codec, bool)> {
        self.offers.iter().find_map(|o| {
            if o.payload_type == payload_type {
                Some((o.codec, false))
            } else if o.rtx_payload_type == payload_type {
                Some((o.codec, true))
            } else {
                None
            }
        })
    }

    fn feedback_ssrc(&self) -> u32 {
        if self.video_ssrc != 0 { self.video_ssrc } else { self.audio_ssrc }
    }

    /// Paquet vidéo reçu (charge utile déchiffrée par le transport).
    pub fn on_rtp(&mut self, m: RtpMeta, payload: &[u8], now: Instant) -> Option<Received> {
        let RtpMeta { ssrc, payload_type, seq, ts, marker } = m;
        let (codec, is_rtx) = self.codec_of(payload_type)?;
        if !self.offer_for(codec).is_some_and(|o| o.decode) {
            return None;
        }
        let (media, p) = if is_rtx {
            let media = *self.rtx.get(&ssrc)?;
            let (osn, original) = send::unwrap_rtx(payload)?;
            (media, Incoming { seq: osn, ts, marker, payload: original.to_vec() })
        } else {
            (ssrc, Incoming { seq, ts, marker, payload: payload.to_vec() })
        };
        let sender = self.feedback_ssrc();
        let remote = self.remotes.get_mut(&media)?;
        if remote.rx.as_ref().is_none_or(|r| r.codec() != codec) {
            remote.rx = Some(Receiver::new(codec));
        }
        let rx = remote.rx.as_mut()?;
        let mut out = recv::Output::default();
        rx.push(p, now, &mut out);
        let mut rtcp_out = Vec::new();
        if !out.nack.is_empty() {
            rtcp_out.push(rtcp::nack(sender, media, &out.nack));
        }
        if out.pli {
            rtcp_out.push(rtcp::pli(sender, media));
        }
        Some(Received { user_id: remote.user, codec: Some(codec), frames: out.frames, rtcp: rtcp_out })
    }

    /// Image perdue après réassemblage (DAVE, file pleine) : le récepteur
    /// jette jusqu'à la prochaine image clé et la demande (PLI, rythme borné).
    pub fn lost_frame(&mut self, user: u64) {
        for r in self.remotes.values_mut().filter(|r| r.user == user) {
            if let Some(rx) = r.rx.as_mut() {
                rx.want_keyframe();
            }
        }
    }

    /// Image clé demandée au flux de `user` : le récepteur jette jusqu'à
    /// elle, et un PLI et un FIR partent (FIR pour les SFU qui ignorent le PLI).
    pub fn request_keyframe(&mut self, user: u64) -> Option<Vec<u8>> {
        let sender = self.feedback_ssrc();
        let (&media, remote) = self.remotes.iter_mut().find(|(_, r)| r.user == user)?;
        if let Some(rx) = remote.rx.as_mut() {
            rx.want_keyframe();
        }
        self.fir_seq = self.fir_seq.wrapping_add(1);
        let mut p = rtcp::pli(sender, media);
        p.extend(rtcp::fir(sender, media, self.fir_seq));
        Some(p)
    }

    /// RTCP reçu (en clair) : retransmissions et demande d'image clé.
    pub fn on_rtcp(&mut self, plain: &[u8]) -> Feedback {
        let mut out = Feedback::default();
        let mine = self.video_ssrc;
        for f in rtcp::parse(plain) {
            match f {
                rtcp::Feedback::Nack { media, seqs } if media == mine => {
                    if let Some(s) = self.sender.as_mut() {
                        out.retransmit.extend(s.retransmit(&seqs));
                    }
                }
                rtcp::Feedback::Pli { media } if media == mine => out.keyframe = true,
                rtcp::Feedback::Fir { ssrcs } if ssrcs.contains(&mine) => out.keyframe = true,
                _ => {}
            }
        }
        out.keyframe &= self.active.is_some();
        out
    }

    /// Paquets d'une image à envoyer (`data` : l'image après DAVE) et, s'il
    /// est dû, le rapport d'émetteur. Rien tant que la vidéo n'est pas
    /// annoncée ou si le codec n'est pas celui négocié.
    pub fn send_frame(&mut self, codec: Codec, ts90k: u32, data: &[u8], now: Instant) -> (Vec<RtpOut>, Option<Vec<u8>>) {
        if self.active.is_none() || self.video_ssrc == 0 {
            return (Vec::new(), None);
        }
        if self.send_codec() != Some(codec) {
            return (Vec::new(), None);
        }
        if self.sender.as_ref().is_none_or(|s| s.codec != codec) {
            let Some(o) = self.offer_for(codec) else { return (Vec::new(), None) };
            self.sender = Some(Sender::new(codec, self.video_ssrc, self.rtx_ssrc, o.payload_type, o.rtx_payload_type));
        }
        let Some(s) = self.sender.as_mut() else { return (Vec::new(), None) };
        let pkts = s.frame(ts90k, data).unwrap_or_default();
        let sr = s.sender_report(now);
        (pkts, sr)
    }

    /// Qualités voulues (0 à 100 ; 0 : ne plus rien envoyer) pour la vidéo
    /// de ces personnes ; rend l'op 15 si elle change quelque chose.
    pub fn set_wants(&mut self, wants: &[(u64, u8)]) -> Option<Value> {
        let mut changed = false;
        for &(user, q) in wants {
            let q = q.min(100);
            changed |= self.wants.insert(user, q) != Some(q);
        }
        if !changed {
            return None;
        }
        self.wants_op()
    }

    /// Op 15 Media Sink Wants (docs.discord.food, Voice Connections) : clés =
    /// SSRC vidéo principales (pas RTX), valeurs de 0 à 100. `None` si aucune
    /// personne réglée n'a encore de SSRC connue.
    pub fn wants_op(&self) -> Option<Value> {
        let mut d = serde_json::Map::new();
        for (ssrc, r) in &self.remotes {
            if let Some(q) = self.wants.get(&r.user) {
                d.insert(ssrc.to_string(), json!(q));
            }
        }
        (!d.is_empty()).then(|| json!({"op": 15, "d": d}))
    }

    /// Une qualité est réglée pour cette personne (à renvoyer après son op 12).
    pub fn wanted(&self, user: u64) -> bool {
        self.wants.contains_key(&user)
    }
}

#[cfg(test)]
mod tests;
