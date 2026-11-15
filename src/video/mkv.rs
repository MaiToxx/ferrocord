//! Démultiplexeur Matroska/WebM léger, écrit à la main (lot VID) : juste ce
//! qu'il faut pour lire un fichier WebM tenu en mémoire (VP8 ou VP9, Vorbis
//! ou Opus). Format : EBML (RFC 8794) et Matroska (RFC 9559).
//!
//! Le fichier est parcouru une fois à l'ouverture : pistes, durée, et la
//! place de chaque image et de chaque paquet de son ([`Block`]). Aucune
//! copie des données ; un fichier abîmé ou coupé rend ce qui précède.
//! Les tailles inconnues (fichiers enregistrés en direct par un navigateur)
//! sont admises pour le segment et les grappes.

/// Signature EBML : début de tout fichier Matroska ou WebM.
pub const MAGIC: [u8; 4] = [0x1A, 0x45, 0xDF, 0xA3];

const SEGMENT: u32 = 0x1853_8067;
const INFO: u32 = 0x1549_A966;
const TIMECODE_SCALE: u32 = 0x2A_D7B1;
const DURATION: u32 = 0x4489;
const TRACKS: u32 = 0x1654_AE6B;
const TRACK_ENTRY: u32 = 0xAE;
const TRACK_NUMBER: u32 = 0xD7;
const TRACK_TYPE: u32 = 0x83;
const CODEC_ID: u32 = 0x86;
const CODEC_PRIVATE: u32 = 0x63A2;
const CODEC_DELAY: u32 = 0x56AA;
const DEFAULT_DURATION: u32 = 0x23_E383;
const VIDEO: u32 = 0xE0;
const PIXEL_WIDTH: u32 = 0xB0;
const PIXEL_HEIGHT: u32 = 0xBA;
const AUDIO: u32 = 0xE1;
const SAMPLING: u32 = 0xB5;
const CHANNELS: u32 = 0x9F;
const CLUSTER: u32 = 0x1F43_B675;
const TIMECODE: u32 = 0xE7;
const SIMPLE_BLOCK: u32 = 0xA3;
const BLOCK_GROUP: u32 = 0xA0;
const BLOCK: u32 = 0xA1;
const REFERENCE_BLOCK: u32 = 0xFB;
const ALPHA_MODE: u32 = 0x53C0;
const BLOCK_ADDITIONS: u32 = 0x75A1;
const BLOCK_MORE: u32 = 0xA6;
const BLOCK_ADD_ID: u32 = 0xEE;
const BLOCK_ADDITIONAL: u32 = 0xA5;

/// Images et paquets relevés, au plus (un fichier de 64 Mio n'en a pas tant).
const MAX_BLOCKS: usize = 2_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodecId {
    Vp8,
    Vp9,
    Opus,
    Vorbis,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub number: u64,
    pub codec: CodecId,
    /// Données d'initialisation du codec (`CodecPrivate`).
    pub private: Vec<u8>,
    /// Vidéo : pixels.
    pub width: u32,
    pub height: u32,
    /// Vidéo avec transparence (`AlphaMode` à 1) : l'alpha de chaque image
    /// est un second flux du même codec, rangé dans les `BlockAdditions` du
    /// groupe ([`Block::alpha`]) ; sa luminance est l'alpha.
    pub alpha: bool,
    /// Son : fréquence et voies.
    pub rate: u32,
    pub channels: u8,
    /// Son à jeter au début du flux décodé (Opus), en microsecondes.
    pub delay_us: u64,
    /// Durée d'une image ou d'un paquet, si le fichier la donne (µs).
    pub frame_us: u64,
}

/// Une image ou un paquet de son : où il est dans le fichier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    pub pts_us: u64,
    /// Image clé (toujours vrai pour le son).
    pub key: bool,
    pub start: usize,
    pub len: usize,
    /// Image d'alpha qui accompagne cette image (`BlockAdditional`
    /// d'identifiant 1 du même groupe) : (début, longueur).
    pub alpha: Option<(usize, usize)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct File {
    /// 0 si rien ne la donne.
    pub duration_us: u64,
    pub video: Option<Track>,
    pub audio: Option<Track>,
    pub video_blocks: Vec<Block>,
    pub audio_blocks: Vec<Block>,
}

/// Entier EBML à longueur variable en `at` : (valeur, octets lus). `keep` :
/// garde le bit de longueur (identifiants). Taille inconnue : `u64::MAX`.
fn vint(d: &[u8], at: usize, keep: bool) -> Option<(u64, usize)> {
    let first = *d.get(at)?;
    let len = first.leading_zeros() as usize + 1;
    if len > 8 {
        return None;
    }
    let bytes = d.get(at..at + len)?;
    let mut v = u64::from(if keep { first } else { first & (0xffu16 >> len) as u8 });
    let mut ones = !keep && v == u64::from((0xffu16 >> len) as u8);
    for b in &bytes[1..] {
        v = (v << 8) | u64::from(*b);
        ones &= *b == 0xff;
    }
    Some((if ones { u64::MAX } else { v }, len))
}

/// En-tête d'un élément en `at` : (identifiant, début du contenu, taille ; `None` : inconnue).
fn element(d: &[u8], at: usize) -> Option<(u32, usize, Option<usize>)> {
    let (id, n) = vint(d, at, true)?;
    let (size, m) = vint(d, at + n, false)?;
    let size = if size == u64::MAX { None } else { Some(usize::try_from(size).ok()?) };
    Some((u32::try_from(id).ok()?, at + n + m, size))
}

fn uint(d: &[u8]) -> u64 {
    d.iter().take(8).fold(0, |v, b| (v << 8) | u64::from(*b))
}

fn float(d: &[u8]) -> f64 {
    match d.len() {
        4 => f64::from(f32::from_be_bytes([d[0], d[1], d[2], d[3]])),
        8 => f64::from_be_bytes([d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]]),
        _ => 0.0,
    }
}

/// Enfants directs d'un élément de contenu `d` : (identifiant, contenu).
fn children(d: &[u8]) -> impl Iterator<Item = (u32, &[u8])> {
    let mut at = 0;
    std::iter::from_fn(move || {
        let (id, start, size) = element(d, at)?;
        let body = d.get(start..start.checked_add(size?)?)?;
        at = start + body.len();
        Some((id, body))
    })
}

fn track(d: &[u8]) -> Option<(u64, Track)> {
    let mut t = Track { number: 0, codec: CodecId::Vp8, private: Vec::new(), width: 0, height: 0, alpha: false, rate: 0, channels: 0, delay_us: 0, frame_us: 0 };
    let (mut kind, mut codec) = (0, None);
    for (id, body) in children(d) {
        match id {
            TRACK_NUMBER => t.number = uint(body),
            TRACK_TYPE => kind = uint(body),
            CODEC_ID => {
                codec = match body.split(|b| *b == 0).next().unwrap_or(body) {
                    b"V_VP8" => Some(CodecId::Vp8),
                    b"V_VP9" => Some(CodecId::Vp9),
                    b"A_OPUS" => Some(CodecId::Opus),
                    b"A_VORBIS" => Some(CodecId::Vorbis),
                    _ => None,
                }
            }
            CODEC_PRIVATE => t.private = body.to_vec(),
            CODEC_DELAY => t.delay_us = uint(body) / 1000,
            DEFAULT_DURATION => t.frame_us = uint(body) / 1000,
            VIDEO => {
                for (id, body) in children(body) {
                    match id {
                        PIXEL_WIDTH => t.width = u32::try_from(uint(body)).unwrap_or(0),
                        PIXEL_HEIGHT => t.height = u32::try_from(uint(body)).unwrap_or(0),
                        ALPHA_MODE => t.alpha = uint(body) == 1,
                        _ => {}
                    }
                }
            }
            AUDIO => {
                for (id, body) in children(body) {
                    match id {
                        SAMPLING => t.rate = float(body) as u32,
                        CHANNELS => t.channels = u8::try_from(uint(body)).unwrap_or(0),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    t.codec = codec?;
    let video = matches!(t.codec, CodecId::Vp8 | CodecId::Vp9);
    // Le genre annoncé doit être celui du codec (1 : vidéo, 2 : son).
    (kind == if video { 1 } else { 2 }).then_some((kind, t))
}

/// Tailles des trames d'un bloc lacé (`lacing` : 1 Xiph, 2 fixe, 3 EBML) ;
/// `at` : après l'en-tête du bloc. Rend (début des données, tailles).
fn laces(d: &[u8], mut at: usize, end: usize, lacing: u8) -> Option<(usize, Vec<usize>)> {
    let count = usize::from(*d.get(at)?) + 1;
    at += 1;
    let mut sizes = Vec::with_capacity(count);
    match lacing {
        1 => {
            for _ in 1..count {
                let mut size = 0usize;
                loop {
                    let b = *d.get(at)?;
                    at += 1;
                    size += usize::from(b);
                    if b != 255 {
                        break;
                    }
                }
                sizes.push(size);
            }
        }
        3 => {
            let mut last = 0i64;
            for i in 1..count {
                let (v, n) = vint(d, at, false)?;
                at += n;
                // Les suivantes sont des écarts signés (biais de la moitié de l'étendue).
                last = if i == 1 { i64::try_from(v).ok()? } else { last + i64::try_from(v).ok()? - ((1i64 << (7 * n - 1)) - 1) };
                sizes.push(usize::try_from(last).ok()?);
            }
        }
        _ => {
            let each = end.checked_sub(at)? / count;
            sizes.extend(std::iter::repeat_n(each, count - 1));
        }
    }
    let used: usize = sizes.iter().sum();
    sizes.push(end.checked_sub(at)?.checked_sub(used)?);
    Some((at, sizes))
}

/// `BlockAdditional` d'identifiant 1 (l'alpha, pour VP8 et VP9) parmi les
/// `BlockMore` de `d[start..end]` : (début, longueur).
fn additional(d: &[u8], start: usize, end: usize) -> Option<(usize, usize)> {
    let mut at = start;
    while at < end {
        let (id, body, size) = element(d, at)?;
        let stop = body.checked_add(size?).filter(|e| *e <= end)?;
        if id == BLOCK_MORE {
            // `BlockAddID` vaut 1 quand il est absent.
            let (mut add_id, mut found, mut p) = (1, None, body);
            while p < stop {
                let (id, inner, size) = element(d, p)?;
                let inner_end = inner.checked_add(size?).filter(|e| *e <= stop)?;
                match id {
                    BLOCK_ADD_ID => add_id = uint(&d[inner..inner_end]),
                    BLOCK_ADDITIONAL => found = Some((inner, inner_end - inner)),
                    _ => {}
                }
                p = inner_end;
            }
            if add_id == 1 && found.is_some() {
                return found;
            }
        }
        at = stop;
    }
    None
}

struct Reader<'a> {
    d: &'a [u8],
    out: File,
    /// Nanosecondes par unité de temps.
    scale: u64,
    /// Durée en unités de temps.
    duration: f64,
    cluster: u64,
    /// Premier bloc vidéo du groupe en cours (`ReferenceBlock` le déclasse).
    group: Option<usize>,
}

impl Reader<'_> {
    fn us(&self, ticks: i64) -> u64 {
        (u128::try_from(ticks.max(0)).unwrap_or(0) * u128::from(self.scale) / 1000).try_into().unwrap_or(u64::MAX)
    }

    fn block(&mut self, start: usize, end: usize, simple: bool) -> Option<()> {
        let d = self.d;
        let (number, n) = vint(d, start, false)?;
        let head = d.get(start + n..start + n + 3)?;
        let rel = i64::from(i16::from_be_bytes([head[0], head[1]]));
        let flags = head[2];
        let at = start + n + 3;
        let video = self.out.video.as_ref().is_some_and(|t| t.number == number);
        let audio = self.out.audio.as_ref().is_some_and(|t| t.number == number);
        if !video && !audio {
            return Some(());
        }
        let step = if video { &self.out.video } else { &self.out.audio }.as_ref().map_or(0, |t| t.frame_us);
        let pts_us = self.us(i64::try_from(self.cluster).ok()? + rel);
        let key = audio || !simple || flags & 0x80 != 0;
        let lacing = (flags >> 1) & 3;
        let (mut at, sizes) = if lacing == 0 { (at, vec![end.checked_sub(at)?]) } else { laces(d, at, end, lacing)? };
        let list = if video { &mut self.out.video_blocks } else { &mut self.out.audio_blocks };
        if video && !simple {
            self.group = Some(list.len());
        }
        for (i, len) in sizes.into_iter().enumerate() {
            if at + len > end || list.len() >= MAX_BLOCKS {
                return None;
            }
            if len > 0 {
                list.push(Block { pts_us: pts_us + i as u64 * step, key, start: at, len, alpha: None });
            }
            at += len;
        }
        Some(())
    }

    /// Parcourt les éléments à plat : le segment, les grappes et les groupes
    /// sont traversés (leur taille peut être inconnue), le reste est sauté.
    fn walk(&mut self) {
        let d = self.d;
        let mut at = 0;
        while let Some((id, start, size)) = element(d, at) {
            let end = size.and_then(|s| start.checked_add(s)).filter(|e| *e <= d.len());
            match (id, end) {
                (SEGMENT | CLUSTER | BLOCK_GROUP, _) => {
                    if id != SEGMENT {
                        self.group = None;
                    }
                    at = start;
                    continue;
                }
                (_, None) => break,
                (INFO, Some(end)) => {
                    for (id, body) in children(&d[start..end]) {
                        match id {
                            TIMECODE_SCALE => self.scale = uint(body).max(1),
                            DURATION => self.duration = float(body),
                            _ => {}
                        }
                    }
                }
                (TRACKS, Some(end)) => {
                    for (id, body) in children(&d[start..end]) {
                        if id != TRACK_ENTRY {
                            continue;
                        }
                        // La première piste lisible de chaque genre.
                        match track(body) {
                            Some((1, t)) if self.out.video.is_none() => self.out.video = Some(t),
                            Some((2, t)) if self.out.audio.is_none() => self.out.audio = Some(t),
                            _ => {}
                        }
                    }
                }
                (TIMECODE, Some(end)) => self.cluster = uint(&d[start..end]),
                (SIMPLE_BLOCK | BLOCK, Some(end)) => {
                    if id == SIMPLE_BLOCK {
                        self.group = None;
                    }
                    if self.block(start, end, id == SIMPLE_BLOCK).is_none() {
                        break;
                    }
                }
                (BLOCK_ADDITIONS, Some(end)) => {
                    // L'alpha de l'image du groupe en cours (une image par groupe).
                    if let Some(b) = self.group.and_then(|first| self.out.video_blocks.get_mut(first)) {
                        b.alpha = additional(d, start, end);
                    }
                }
                (REFERENCE_BLOCK, Some(_)) => {
                    if let Some(first) = self.group {
                        for b in self.out.video_blocks.iter_mut().skip(first) {
                            b.key = false;
                        }
                    }
                }
                _ => {}
            }
            at = end.unwrap_or(d.len());
        }
    }
}

/// Lit `data` ; `None` : ni Matroska ni WebM, ou aucune piste lisible.
pub fn parse(data: &[u8]) -> Option<File> {
    if !data.starts_with(&MAGIC) {
        return None;
    }
    let (_, start, size) = element(data, 0)?;
    let body = start.checked_add(size?)?;
    let mut r = Reader { d: data.get(body..)?, out: File::default(), scale: 1_000_000, duration: 0.0, cluster: 0, group: None };
    r.walk();
    let mut f = r.out;
    // Positions relatives au fichier entier.
    for b in f.video_blocks.iter_mut().chain(f.audio_blocks.iter_mut()) {
        b.start += body;
        if let Some((start, _)) = b.alpha.as_mut() {
            *start += body;
        }
    }
    if f.video.is_none() && f.audio.is_none() {
        return None;
    }
    let last = |v: &[Block], t: &Option<Track>| v.last().map_or(0, |b| b.pts_us + t.as_ref().map_or(0, |t| t.frame_us));
    let seen = last(&f.video_blocks, &f.video).max(last(&f.audio_blocks, &f.audio));
    let told = (r.duration * r.scale as f64 / 1000.0) as u64;
    f.duration_us = if told > 0 { told } else { seen };
    // L'ordre de présentation est celui du fichier, à de rares écarts près.
    f.video_blocks.sort_by_key(|b| b.pts_us);
    f.audio_blocks.sort_by_key(|b| b.pts_us);
    Some(f)
}

/// Écriture d'un WebM minimal, pour les tests du lecteur.
#[cfg(test)]
pub mod write {
    use super::*;

    fn id(out: &mut Vec<u8>, id: u32) {
        let b = id.to_be_bytes();
        out.extend_from_slice(&b[b.iter().position(|x| *x != 0).unwrap_or(3)..]);
    }

    /// Élément de taille connue (écrite sur 4 octets).
    pub fn el(out: &mut Vec<u8>, i: u32, body: &[u8]) {
        id(out, i);
        out.extend_from_slice(&(0x1000_0000u32 | body.len() as u32).to_be_bytes());
        out.extend_from_slice(body);
    }

    fn num(out: &mut Vec<u8>, i: u32, v: u64) {
        el(out, i, &v.to_be_bytes());
    }

    /// Paquet d'une piste : (piste, instant en ms, image clé, données).
    pub type Packet = (u64, u64, bool, Vec<u8>);

    /// Fichier d'une piste vidéo (1) et d'une piste Opus (2), une grappe par
    /// seconde. `unknown` : segment et grappes de taille inconnue, sans durée.
    pub fn webm(video: Option<(&str, u32, u32)>, opus_head: Option<&[u8]>, packets: &[Packet], duration_ms: f64, unknown: bool) -> Vec<u8> {
        let mut seg = Vec::new();
        let mut info = Vec::new();
        num(&mut info, TIMECODE_SCALE, 1_000_000);
        if !unknown {
            el(&mut info, DURATION, &duration_ms.to_be_bytes());
        }
        el(&mut seg, INFO, &info);
        let mut tracks = Vec::new();
        if let Some((codec, w, h)) = video {
            let mut t = Vec::new();
            num(&mut t, TRACK_NUMBER, 1);
            num(&mut t, TRACK_TYPE, 1);
            el(&mut t, CODEC_ID, codec.as_bytes());
            let mut v = Vec::new();
            num(&mut v, PIXEL_WIDTH, u64::from(w));
            num(&mut v, PIXEL_HEIGHT, u64::from(h));
            el(&mut t, VIDEO, &v);
            el(&mut tracks, TRACK_ENTRY, &t);
        }
        if let Some(head) = opus_head {
            let mut t = Vec::new();
            num(&mut t, TRACK_NUMBER, 2);
            num(&mut t, TRACK_TYPE, 2);
            el(&mut t, CODEC_ID, b"A_OPUS");
            el(&mut t, CODEC_PRIVATE, head);
            num(&mut t, CODEC_DELAY, u64::from(u16::from_le_bytes([head[10], head[11]])) * 1_000_000_000 / 48_000);
            let mut a = Vec::new();
            el(&mut a, SAMPLING, &48_000f64.to_be_bytes());
            num(&mut a, CHANNELS, u64::from(head[9]));
            el(&mut t, AUDIO, &a);
            el(&mut tracks, TRACK_ENTRY, &t);
        }
        el(&mut seg, TRACKS, &tracks);
        let mut at = 0;
        while at < packets.len() {
            let base = packets[at].1 / 1000 * 1000;
            let mut c = Vec::new();
            num(&mut c, TIMECODE, base);
            while at < packets.len() && packets[at].1 / 1000 * 1000 == base {
                let (track, ms, key, data) = &packets[at];
                let mut b = vec![0x80 | *track as u8];
                b.extend_from_slice(&((ms - base) as i16).to_be_bytes());
                b.push(if *key { 0x80 } else { 0 });
                b.extend_from_slice(data);
                el(&mut c, SIMPLE_BLOCK, &b);
                at += 1;
            }
            if unknown {
                id(&mut seg, CLUSTER);
                seg.push(0xff);
                seg.extend_from_slice(&c);
            } else {
                el(&mut seg, CLUSTER, &c);
            }
        }
        finish(seg, unknown)
    }

    /// Image d'une vidéo avec transparence : (instant en ms, image clé,
    /// image de couleur, image d'alpha).
    pub type AlphaFrame = (u64, bool, Vec<u8>, Vec<u8>);

    /// Fichier d'une piste vidéo avec transparence, comme les écrit libwebm :
    /// un groupe par image, l'alpha dans ses `BlockAdditions` (identifiant 1),
    /// une seule grappe (moins de 32 s). `flagged` : la piste annonce son
    /// alpha (`AlphaMode` à 1).
    pub fn webm_alpha(codec: &str, (w, h): (u32, u32), frames: &[AlphaFrame], duration_ms: f64, flagged: bool) -> Vec<u8> {
        let mut seg = Vec::new();
        let mut info = Vec::new();
        num(&mut info, TIMECODE_SCALE, 1_000_000);
        el(&mut info, DURATION, &duration_ms.to_be_bytes());
        el(&mut seg, INFO, &info);
        let mut t = Vec::new();
        num(&mut t, TRACK_NUMBER, 1);
        num(&mut t, TRACK_TYPE, 1);
        el(&mut t, CODEC_ID, codec.as_bytes());
        let mut v = Vec::new();
        num(&mut v, PIXEL_WIDTH, u64::from(w));
        num(&mut v, PIXEL_HEIGHT, u64::from(h));
        if flagged {
            num(&mut v, ALPHA_MODE, 1);
        }
        el(&mut t, VIDEO, &v);
        let mut tracks = Vec::new();
        el(&mut tracks, TRACK_ENTRY, &t);
        el(&mut seg, TRACKS, &tracks);
        let mut c = Vec::new();
        num(&mut c, TIMECODE, 0);
        for (ms, key, data, alpha) in frames {
            let mut g = Vec::new();
            let mut b = vec![0x81];
            b.extend_from_slice(&(*ms as i16).to_be_bytes());
            b.push(0);
            b.extend_from_slice(data);
            el(&mut g, BLOCK, &b);
            let mut more = Vec::new();
            num(&mut more, BLOCK_ADD_ID, 1);
            el(&mut more, BLOCK_ADDITIONAL, alpha);
            let mut adds = Vec::new();
            el(&mut adds, BLOCK_MORE, &more);
            el(&mut g, BLOCK_ADDITIONS, &adds);
            if !key {
                el(&mut g, REFERENCE_BLOCK, &[0xff]);
            }
            el(&mut c, BLOCK_GROUP, &g);
        }
        el(&mut seg, CLUSTER, &c);
        finish(seg, false)
    }

    fn finish(seg: Vec<u8>, unknown: bool) -> Vec<u8> {
        let mut out = Vec::new();
        el(&mut out, u32::from_be_bytes(MAGIC), &[]);
        if unknown {
            id(&mut out, SEGMENT);
            out.push(0xff);
            out.extend_from_slice(&seg);
        } else {
            el(&mut out, SEGMENT, &seg);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head() -> Vec<u8> {
        let mut h = b"OpusHead".to_vec();
        h.extend_from_slice(&[1, 2, 0x38, 0x01, 0x80, 0xbb, 0, 0, 0, 0, 0]);
        h
    }

    fn sample(unknown: bool) -> Vec<u8> {
        let packets: Vec<write::Packet> = vec![(1, 0, true, vec![1; 10]), (2, 0, true, vec![2; 5]), (1, 40, false, vec![3; 7]), (2, 20, true, vec![4; 6]), (1, 1040, true, vec![5; 9])];
        write::webm(Some(("V_VP8", 320, 240)), Some(&head()), &packets, 1080.0, unknown)
    }

    #[test]
    fn entiers_ebml() {
        assert_eq!(vint(&[0x81], 0, false), Some((1, 1)));
        assert_eq!(vint(&[0x40, 0x02], 0, false), Some((2, 2)));
        assert_eq!(vint(&[0xff], 0, false), Some((u64::MAX, 1)), "taille inconnue");
        assert_eq!(vint(&[0x1A, 0x45, 0xDF, 0xA3], 0, true), Some((0x1A45_DFA3, 4)));
        assert_eq!(vint(&[0x40], 0, false), None, "coupé");
        assert_eq!(vint(&[0x00], 0, false), None);
    }

    #[test]
    fn pistes_durees_et_blocs() {
        for unknown in [false, true] {
            let data = sample(unknown);
            let f = parse(&data).unwrap_or_else(|| panic!("lecture (inconnue : {unknown})"));
            let v = f.video.as_ref().unwrap_or_else(|| panic!("vidéo"));
            assert_eq!((v.codec, v.width, v.height), (CodecId::Vp8, 320, 240));
            let a = f.audio.as_ref().unwrap_or_else(|| panic!("son"));
            assert_eq!((a.codec, a.rate, a.channels, a.delay_us), (CodecId::Opus, 48_000, 2, 6500));
            assert_eq!(a.private, head());
            assert_eq!(f.duration_us, if unknown { 1_040_000 } else { 1_080_000 });
            let v: Vec<(u64, bool, &[u8])> = f.video_blocks.iter().map(|b| (b.pts_us, b.key, &data[b.start..b.start + b.len])).collect();
            assert_eq!(v, [(0, true, &[1u8; 10][..]), (40_000, false, &[3; 7][..]), (1_040_000, true, &[5; 9][..])]);
            let a: Vec<(u64, usize)> = f.audio_blocks.iter().map(|b| (b.pts_us, b.len)).collect();
            assert_eq!(a, [(0, 5), (20_000, 6)]);
        }
    }

    #[test]
    fn groupes_et_lacage() {
        // Groupe avec référence : image intermédiaire. Bloc de son lacé (Xiph, EBML, fixe).
        let mut c = Vec::new();
        write::el(&mut c, TIMECODE, &[0]);
        let mut g = Vec::new();
        write::el(&mut g, BLOCK, &[0x81, 0, 0, 0, 9, 9]);
        write::el(&mut g, REFERENCE_BLOCK, &[0xff]);
        write::el(&mut c, BLOCK_GROUP, &g);
        let mut g = Vec::new();
        write::el(&mut g, BLOCK, &[0x81, 0, 5, 0, 8]);
        write::el(&mut c, BLOCK_GROUP, &g);
        write::el(&mut c, SIMPLE_BLOCK, &[0x82, 0, 0, 0x02, 2, 2, 1, 7, 7, 6, 5, 5, 5]);
        write::el(&mut c, SIMPLE_BLOCK, &[0x82, 0, 0, 0x06, 2, 0x82, 0xBE, 7, 7, 6, 5, 5, 5]);
        write::el(&mut c, SIMPLE_BLOCK, &[0x82, 0, 0, 0x04, 2, 7, 7, 6, 6, 5, 5]);
        let mut data = sample(false);
        // La grappe s'ajoute à la suite du segment (taille du segment : laissée, la lecture à plat continue).
        write::el(&mut data, CLUSTER, &c);
        let f = parse(&data).unwrap_or_else(|| panic!("lecture"));
        let keys: Vec<bool> = f.video_blocks.iter().map(|b| b.key).collect();
        assert_eq!(keys.iter().filter(|k| !**k).count(), 2, "{keys:?}");
        assert!(f.video_blocks.iter().any(|b| b.len == 1 && b.key), "groupe sans référence : image clé");
        let laced: Vec<usize> = f.audio_blocks.iter().filter(|b| b.len <= 3 && b.pts_us == 0).map(|b| b.len).collect();
        assert_eq!(laced, [2, 1, 3, 2, 1, 3, 2, 2, 2]);
    }

    /// Vidéo avec transparence : la piste l'annonce, chaque image porte son
    /// alpha (`BlockAdditions`), et rien d'autre ne change (clés, instants).
    #[test]
    fn alpha_dans_les_additions_du_groupe() {
        let frames: Vec<write::AlphaFrame> = vec![(0, true, vec![1; 10], vec![7; 4]), (40, false, vec![2; 6], vec![8; 3]), (80, false, vec![3; 5], vec![9; 2])];
        let data = write::webm_alpha("V_VP9", (448, 84), &frames, 120.0, true);
        let f = parse(&data).unwrap_or_else(|| panic!("lecture"));
        let v = f.video.as_ref().unwrap_or_else(|| panic!("vidéo"));
        assert_eq!((v.codec, v.width, v.height, v.alpha), (CodecId::Vp9, 448, 84, true));
        let got: Vec<_> = f.video_blocks.iter().map(|b| (b.pts_us, b.key, &data[b.start..b.start + b.len], b.alpha.map(|(s, n)| &data[s..s + n]))).collect();
        assert_eq!(got, [(0, true, &[1u8; 10][..], Some(&[7u8; 4][..])), (40_000, false, &[2; 6][..], Some(&[8; 3][..])), (80_000, false, &[3; 5][..], Some(&[9; 2][..]))]);
        // Sans `AlphaMode` : la piste ne l'annonce pas ; les additions restent relevées.
        let plain = parse(&write::webm_alpha("V_VP9", (448, 84), &frames, 120.0, false)).unwrap_or_else(|| panic!("lecture"));
        assert!(plain.video.as_ref().is_some_and(|v| !v.alpha) && plain.video_blocks.iter().all(|b| b.alpha.is_some()));
        // Blocs simples : aucun alpha. Additions d'un autre identifiant : ignorées.
        assert!(parse(&sample(false)).is_some_and(|f| f.video_blocks.iter().all(|b| b.alpha.is_none())));
        let mut more = Vec::new();
        write::el(&mut more, BLOCK_ADD_ID, &[4]);
        write::el(&mut more, BLOCK_ADDITIONAL, &[5, 5]);
        let mut adds = Vec::new();
        write::el(&mut adds, BLOCK_MORE, &more);
        assert_eq!(additional(&adds, 0, adds.len()), None);
        // Coupé n'importe où : jamais de panique, jamais d'alpha hors du fichier.
        for cut in 0..data.len() {
            if let Some(f) = parse(&data[..cut]) {
                assert!(f.video_blocks.iter().all(|b| b.alpha.is_none_or(|(s, n)| s + n <= cut)));
            }
        }
    }

    #[test]
    fn fichiers_abimes() {
        assert!(parse(b"pas du webm").is_none());
        assert!(parse(&MAGIC).is_none());
        let data = sample(false);
        // Coupé n'importe où : jamais de panique, et le début reste lisible.
        for cut in 0..data.len() {
            let _ = parse(&data[..cut]);
        }
        let f = parse(&data[..data.len() - 4]).unwrap_or_else(|| panic!("coupé"));
        assert_eq!(f.video_blocks.len(), 2);
        // Octets au hasard après l'en-tête.
        let mut junk = data.clone();
        for (i, b) in junk.iter_mut().enumerate().skip(40) {
            *b = (i * 131 % 251) as u8;
        }
        let _ = parse(&junk);
    }
}
