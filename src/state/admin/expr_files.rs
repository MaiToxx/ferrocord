//! Vérification des fichiers envoyés comme emojis, autocollants et sons,
//! d'après leur contenu (jamais leur nom) : calcul seul, sans disque.
//!
//! Limites lues sur docs.discord.food (« Emoji », « Sticker »,
//! « Soundboard », 13 novembre 2026) : emoji 256 Kio, autocollant 500 Kio
//! (PNG, APNG ou GIF), son 512 Kio et 5,2 s au plus (MP3 ou Ogg).

use super::super::usersettings::b64;

pub const MAX_EMOJI_BYTES: usize = 256 * 1024;
/// Côté d'un emoji envoyé (documentation : « 128x128 emoji image »).
pub const EMOJI_SIDE: u32 = 128;
pub const MAX_STICKER_BYTES: usize = 500 * 1024;
/// Côté d'un autocollant converti (taille de l'application officielle).
pub const STICKER_SIDE: u32 = 320;
pub const MAX_SOUND_BYTES: usize = 512 * 1024;
pub const MAX_SOUND_MS: u64 = 5200;

/// `data:` d'un fichier, pour les champs « image data » et « sound data ».
pub fn data_uri(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", b64::encode(bytes))
}

/// Format d'un autocollant accepté tel quel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StickerKind {
    Png,
    /// PNG animé (bloc `acTL` avant les données).
    Apng,
    Gif,
}

impl StickerKind {
    pub fn mime(self) -> &'static str {
        match self {
            StickerKind::Png | StickerKind::Apng => "image/png",
            StickerKind::Gif => "image/gif",
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            StickerKind::Png | StickerKind::Apng => "png",
            StickerKind::Gif => "gif",
        }
    }
}

const PNG_SIG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// PNG (animé ou non) ou GIF, d'après les premiers octets.
pub fn sticker_kind(b: &[u8]) -> Option<StickerKind> {
    if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        return Some(StickerKind::Gif);
    }
    if !b.starts_with(PNG_SIG) {
        return None;
    }
    let mut i = PNG_SIG.len();
    // Les blocs sont parcourus jusqu'aux premières données d'image.
    while let Some(head) = b.get(i..i + 8) {
        let len = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
        match &head[4..8] {
            b"acTL" => return Some(StickerKind::Apng),
            b"IDAT" | b"IEND" => return Some(StickerKind::Png),
            _ => {}
        }
        i = i.checked_add(12)?.checked_add(len)?;
    }
    Some(StickerKind::Png)
}

/// Format d'un son accepté.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundKind {
    Mp3,
    Ogg,
}

impl SoundKind {
    pub fn mime(self) -> &'static str {
        match self {
            SoundKind::Mp3 => "audio/mpeg",
            SoundKind::Ogg => "audio/ogg",
        }
    }
}

/// Pourquoi un son est refusé avant l'envoi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundError {
    /// Ni MP3 ni Ogg lisible.
    Unsupported,
    TooBig,
    /// Durée (ms) au-delà de [`MAX_SOUND_MS`].
    TooLong(u64),
}

/// Son prêt à envoyer : format et durée (ms).
pub fn check_sound(b: &[u8]) -> Result<(SoundKind, u64), SoundError> {
    let (kind, ms) = if b.starts_with(b"OggS") {
        (SoundKind::Ogg, ogg_duration_ms(b).ok_or(SoundError::Unsupported)?)
    } else {
        (SoundKind::Mp3, mp3_duration_ms(b).ok_or(SoundError::Unsupported)?)
    };
    if b.len() > MAX_SOUND_BYTES {
        return Err(SoundError::TooBig);
    }
    if ms > MAX_SOUND_MS {
        return Err(SoundError::TooLong(ms));
    }
    Ok((kind, ms))
}

/// Octets de déchets tolérés avant la première trame MP3.
const MP3_SYNC_SEARCH: usize = 64 * 1024;

/// Durée d'un MP3 : somme des trames (MPEG 1, 2 et 2.5, couches I à III),
/// après une éventuelle étiquette ID3v2. `None` sans aucune trame valide.
pub fn mp3_duration_ms(b: &[u8]) -> Option<u64> {
    let mut i = 0usize;
    if b.len() >= 10 && &b[..3] == b"ID3" {
        let size = b[6..10].iter().fold(0usize, |acc, x| (acc << 7) | usize::from(x & 0x7F));
        let footer = if b[5] & 0x10 != 0 { 10 } else { 0 };
        i = 10 + size + footer;
    }
    let start = i;
    let (mut frames, mut secs) = (0u64, 0f64);
    while let Some(h) = b.get(i..i + 4) {
        let h = u32::from_be_bytes([h[0], h[1], h[2], h[3]]);
        match mp3_frame(h) {
            Some((len, samples, rate)) if i + len <= b.len() || frames > 0 => {
                frames += 1;
                secs += f64::from(samples) / f64::from(rate);
                i += len;
            }
            _ if frames == 0 && i - start < MP3_SYNC_SEARCH => i += 1,
            // Fin des trames (étiquette ID3v1, remplissage) : on s'arrête.
            _ => break,
        }
    }
    (frames > 0).then(|| (secs * 1000.0).round() as u64)
}

/// Longueur (octets), échantillons et fréquence d'une trame MP3 d'en-tête `h`.
fn mp3_frame(h: u32) -> Option<(usize, u32, u32)> {
    if h & 0xFFE0_0000 != 0xFFE0_0000 {
        return None;
    }
    let version = (h >> 19) & 3; // 0 : 2.5, 1 : réservé, 2 : 2, 3 : 1
    let layer = (h >> 17) & 3; // 1 : III, 2 : II, 3 : I
    let br_idx = ((h >> 12) & 0xF) as usize;
    let sr_idx = ((h >> 10) & 3) as usize;
    let pad = (h >> 9) & 1;
    if version == 1 || layer == 0 || br_idx == 0 || br_idx == 15 || sr_idx == 3 {
        return None;
    }
    const V1_L1: [u32; 15] = [0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448];
    const V1_L2: [u32; 15] = [0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384];
    const V1_L3: [u32; 15] = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];
    const V2_L1: [u32; 15] = [0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256];
    const V2_L23: [u32; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    let v1 = version == 3;
    let kbps = match (v1, layer) {
        (true, 3) => V1_L1[br_idx],
        (true, 2) => V1_L2[br_idx],
        (true, _) => V1_L3[br_idx],
        (false, 3) => V2_L1[br_idx],
        (false, _) => V2_L23[br_idx],
    };
    let rate = match version {
        3 => [44_100, 48_000, 32_000][sr_idx],
        2 => [22_050, 24_000, 16_000][sr_idx],
        _ => [11_025, 12_000, 8_000][sr_idx],
    };
    let br = kbps * 1000;
    let (len, samples) = match layer {
        3 => ((12 * br / rate + pad) * 4, 384),
        2 => (144 * br / rate + pad, 1152),
        _ if v1 => (144 * br / rate + pad, 1152),
        _ => (72 * br / rate + pad, 576),
    };
    (len >= 4).then_some((len as usize, samples, rate))
}

/// Durée d'un Ogg Vorbis ou Opus : position de la dernière page du premier
/// flux, rapportée à sa fréquence (Opus : 48 kHz, moins le pré-saut).
pub fn ogg_duration_ms(b: &[u8]) -> Option<u64> {
    let mut i = 0usize;
    let mut first: Option<(u32, Codec)> = None;
    let mut last_granule: Option<u64> = None;
    while let Some(h) = b.get(i..i + 27) {
        if &h[..4] != b"OggS" {
            break;
        }
        let granule = u64::from_le_bytes(h[6..14].try_into().ok()?);
        let serial = u32::from_le_bytes(h[14..18].try_into().ok()?);
        let nsegs = usize::from(h[26]);
        let table = b.get(i + 27..i + 27 + nsegs)?;
        let body_len: usize = table.iter().map(|x| usize::from(*x)).sum();
        let body_at = i + 27 + nsegs;
        match first {
            None => {
                let body = b.get(body_at..body_at + body_len)?;
                first = Some((serial, Codec::of(body)?));
            }
            Some((s, _)) if s == serial && granule != u64::MAX => last_granule = Some(granule),
            Some(_) => {}
        }
        i = body_at + body_len;
    }
    let (_, codec) = first?;
    let g = last_granule?;
    let (samples, rate) = match codec {
        Codec::Opus { pre_skip } => (g.saturating_sub(u64::from(pre_skip)), 48_000),
        Codec::Vorbis { rate } => (g, u64::from(rate)),
    };
    (rate > 0).then(|| samples * 1000 / rate)
}

#[derive(Clone, Copy)]
enum Codec {
    Opus { pre_skip: u16 },
    Vorbis { rate: u32 },
}

impl Codec {
    fn of(body: &[u8]) -> Option<Codec> {
        if body.starts_with(b"OpusHead") {
            return Some(Codec::Opus { pre_skip: u16::from_le_bytes(body.get(10..12)?.try_into().ok()?) });
        }
        if body.starts_with(b"\x01vorbis") {
            return Some(Codec::Vorbis { rate: u32::from_le_bytes(body.get(12..16)?.try_into().ok()?) });
        }
        None
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// `n` trames MPEG-1 couche III, 128 kbit/s, 44,1 kHz (417 octets chacune).
    pub(crate) fn mp3(n: usize) -> Vec<u8> {
        let mut out = b"ID3\x03\x00\x00\x00\x00\x00\x0A".to_vec();
        out.extend_from_slice(&[0u8; 10]);
        for _ in 0..n {
            out.extend_from_slice(&0xFFFB_9000u32.to_be_bytes());
            out.extend_from_slice(&[0u8; 413]);
        }
        out.extend_from_slice(b"TAG");
        out
    }

    fn page(serial: u32, granule: u64, body: &[u8]) -> Vec<u8> {
        let mut out = b"OggS\x00\x00".to_vec();
        out.extend_from_slice(&granule.to_le_bytes());
        out.extend_from_slice(&serial.to_le_bytes());
        out.extend_from_slice(&[0u8; 8]);
        let mut segs = Vec::new();
        let mut left = body.len();
        while left >= 255 {
            segs.push(255u8);
            left -= 255;
        }
        segs.push(left as u8);
        out.push(segs.len() as u8);
        out.extend_from_slice(&segs);
        out.extend_from_slice(body);
        out
    }

    /// Ogg Opus de `ms` millisecondes (pré-saut de 312 échantillons).
    pub(crate) fn opus(ms: u64) -> Vec<u8> {
        let mut head = b"OpusHead\x01\x02".to_vec();
        head.extend_from_slice(&312u16.to_le_bytes());
        head.extend_from_slice(&48_000u32.to_le_bytes());
        let mut out = page(7, 0, &head);
        out.extend(page(7, 0, b"OpusTags"));
        out.extend(page(7, 312 + ms * 48, &[0u8; 300]));
        out
    }

    #[test]
    fn mp3_duration_counts_frames() {
        // 100 trames × 1152 / 44100 = 2612 ms.
        assert_eq!(mp3_duration_ms(&mp3(100)), Some(2612));
        assert_eq!(check_sound(&mp3(100)), Ok((SoundKind::Mp3, 2612)));
        assert_eq!(check_sound(&mp3(250)), Err(SoundError::TooLong(6531)));
        assert_eq!(mp3_duration_ms(b"pas un son du tout"), None);
        assert_eq!(check_sound(b""), Err(SoundError::Unsupported));
        // MPEG-2 couche III, 64 kbit/s, 22,05 kHz : 576 échantillons par trame.
        assert_eq!(mp3_frame(0xFFF3_8000), Some((208, 576, 22_050)));
    }

    #[test]
    fn ogg_duration_from_granule() {
        assert_eq!(ogg_duration_ms(&opus(3000)), Some(3000));
        assert_eq!(check_sound(&opus(5300)), Err(SoundError::TooLong(5300)));
        let mut vorbis = b"\x01vorbis\x00\x00\x00\x00\x01".to_vec();
        vorbis.extend_from_slice(&44_100u32.to_le_bytes());
        let mut v = page(1, 0, &vorbis);
        v.extend(page(2, 999_999_999, b"autre flux"));
        v.extend(page(1, 88_200, &[0u8; 10]));
        assert_eq!(check_sound(&v), Ok((SoundKind::Ogg, 2000)));
        assert_eq!(ogg_duration_ms(b"OggS"), None);
    }

    #[test]
    fn big_sound_is_refused() {
        let mut big = opus(1000);
        big.extend(vec![0u8; MAX_SOUND_BYTES]);
        assert_eq!(check_sound(&big), Err(SoundError::TooBig));
    }

    #[test]
    fn sticker_kinds() {
        let mut apng = PNG_SIG.to_vec();
        for (kind, len) in [(&b"IHDR"[..], 13u32), (b"acTL", 8), (b"IDAT", 1)] {
            apng.extend_from_slice(&len.to_be_bytes());
            apng.extend_from_slice(kind);
            apng.extend(vec![0u8; len as usize + 4]);
        }
        assert_eq!(sticker_kind(&apng), Some(StickerKind::Apng));
        let png = [&apng[..8 + 25], &apng[8 + 25 + 20..]].concat();
        assert_eq!(sticker_kind(&png), Some(StickerKind::Png));
        assert_eq!(sticker_kind(b"GIF89a...."), Some(StickerKind::Gif));
        assert_eq!(sticker_kind(b"\xFF\xD8\xFF"), None);
        assert_eq!(data_uri("image/gif", &[1, 2, 3]), "data:image/gif;base64,AQID");
    }
}
