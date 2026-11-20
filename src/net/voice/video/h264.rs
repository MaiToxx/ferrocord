//! Charge utile RTP H.264, RFC 6184 (mode de paquétisation 1, celui que
//! Discord annonce : `packetization-mode=1`, docs.discord.food, Voice
//! Connections) : NAL simple, agrégation STAP-A, fragmentation FU-A.
//! Les images se lisent et s'écrivent en Annex B ; à la sortie du
//! dépaquétiseur, chaque NAL est précédée d'un code de début de 4 octets,
//! comme le fait DAVE avant de chiffrer (libdave, `codec_utils.cpp`) : les
//! deux côtés voient ainsi exactement les mêmes octets.

/// Code de début long, ajouté avant chaque NAL.
pub const START_CODE: [u8; 4] = [0, 0, 0, 1];
const STAP_A: u8 = 24;
const FU_A: u8 = 28;
pub const NAL_SLICE: u8 = 1;
pub const NAL_IDR: u8 = 5;
pub const NAL_SPS: u8 = 7;
pub const NAL_PPS: u8 = 8;

/// Position des codes de début : (début du code, début de la NAL). Un seul
/// zéro de tête est rattaché au code (règle de WebRTC) : une NAL chiffrée
/// par DAVE qui finit par des zéros les garde.
fn start_codes(b: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 3 <= b.len() {
        if b[i] == 0 && b[i + 1] == 0 && b[i + 2] == 1 {
            let sc = if i > 0 && b[i - 1] == 0 { i - 1 } else { i };
            out.push((sc, i + 3));
            i += 3;
        } else {
            i += 1;
        }
    }
    out
}

/// NAL d'une image Annex B (sans les codes de début). Sans aucun code de
/// début, l'image entière est prise pour une seule NAL.
pub fn nal_units(b: &[u8]) -> Vec<&[u8]> {
    let starts = start_codes(b);
    if starts.is_empty() {
        return if b.is_empty() { Vec::new() } else { vec![b] };
    }
    let mut out = Vec::with_capacity(starts.len());
    for (k, &(_, from)) in starts.iter().enumerate() {
        let to = starts.get(k + 1).map_or(b.len(), |s| s.0);
        if to > from {
            out.push(&b[from..to]);
        }
    }
    out
}

pub fn nal_type(nal: &[u8]) -> u8 {
    nal.first().map_or(0, |h| h & 0x1F)
}

/// Image clé : elle contient une NAL IDR.
pub fn is_keyframe(annexb: &[u8]) -> bool {
    nal_units(annexb).iter().any(|n| nal_type(n) == NAL_IDR)
}

/// Charges utiles RTP d'une image, `max` octets au plus chacune.
pub fn packetize(frame: &[u8], max: usize) -> Vec<Vec<u8>> {
    let max = max.max(3);
    let nals = nal_units(frame);
    let mut out = Vec::new();
    let mut i = 0;
    while i < nals.len() {
        let n = nals[i];
        if n.len() > max {
            fragment(n, max, &mut out);
            i += 1;
            continue;
        }
        // Petites NAL voisines (SPS, PPS, SEI…) réunies dans un STAP-A.
        let mut j = i;
        let mut size = 1;
        while j < nals.len() && size + 2 + nals[j].len() <= max {
            size += 2 + nals[j].len();
            j += 1;
        }
        if j - i >= 2 {
            let group = &nals[i..j];
            let f = group.iter().fold(0u8, |a, n| a | (n[0] & 0x80));
            let nri = group.iter().map(|n| n[0] & 0x60).max().unwrap_or(0);
            let mut p = Vec::with_capacity(size);
            p.push(f | nri | STAP_A);
            for n in group {
                p.extend_from_slice(&(n.len() as u16).to_be_bytes());
                p.extend_from_slice(n);
            }
            out.push(p);
            i = j;
        } else {
            out.push(n.to_vec());
            i += 1;
        }
    }
    out
}

/// Comme [`packetize`], sans agrégation : une NAL par paquet, fragmentée
/// (FU-A) au-delà de `max`. C'est ce qui part vers Discord : SPS et PPS
/// voyagent chacun dans leur paquet, comme le font les clients officiels.
pub fn packetize_plain(frame: &[u8], max: usize) -> Vec<Vec<u8>> {
    let max = max.max(3);
    let mut out = Vec::new();
    for n in nal_units(frame) {
        if n.len() > max {
            fragment(n, max, &mut out);
        } else {
            out.push(n.to_vec());
        }
    }
    out
}

/// Octets sans les octets de prévention d'émulation (`00 00 03` → `00 00`).
fn unescape(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut zeros = 0;
    for &x in b {
        if zeros >= 2 && x == 3 {
            zeros = 0;
            continue;
        }
        zeros = if x == 0 { zeros + 1 } else { 0 };
        out.push(x);
    }
    out
}

/// Inverse de [`unescape`] : un `03` avant tout octet ≤ 3 qui suit deux zéros.
fn escape(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len() + 4);
    let mut zeros = 0;
    for &x in b {
        if zeros >= 2 && x <= 3 {
            out.push(3);
            zeros = 0;
        }
        zeros = if x == 0 { zeros + 1 } else { 0 };
        out.push(x);
    }
    out
}

/// Lecture bit à bit (poids fort d'abord).
struct Bits<'a> {
    d: &'a [u8],
    at: usize,
}

impl Bits<'_> {
    fn u(&mut self, n: u32) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            let b = self.d.get(self.at / 8)? >> (7 - self.at % 8) & 1;
            v = v << 1 | u32::from(b);
            self.at += 1;
        }
        Some(v)
    }

    fn flag(&mut self) -> Option<bool> {
        Some(self.u(1)? == 1)
    }

    /// Exp-Golomb non signé.
    fn ue(&mut self) -> Option<u32> {
        let mut lz = 0;
        while self.u(1)? == 0 {
            lz += 1;
            if lz > 31 {
                return None;
            }
        }
        Some(((1u64 << lz) - 1 + u64::from(self.u(lz)?)) as u32)
    }

    fn skip_ue(&mut self, n: usize) -> Option<()> {
        for _ in 0..n {
            self.ue()?;
        }
        Some(())
    }

    /// Paramètres HRD (E.1.2) : sautés.
    fn skip_hrd(&mut self) -> Option<()> {
        let count = self.ue()?.checked_add(1).filter(|c| *c <= 32)?;
        self.u(8)?;
        for _ in 0..count {
            self.skip_ue(2)?;
            self.u(1)?;
        }
        self.u(20).map(|_| ())
    }
}

/// Écriture bit à bit.
#[derive(Default)]
struct BitsOut {
    d: Vec<u8>,
    n: usize,
}

impl BitsOut {
    fn put(&mut self, v: u32, bits: u32) {
        for k in (0..bits).rev() {
            if self.n.is_multiple_of(8) {
                self.d.push(0);
            }
            if v >> k & 1 == 1 && let Some(last) = self.d.last_mut() {
                *last |= 1 << (7 - self.n % 8);
            }
            self.n += 1;
        }
    }

    fn ue(&mut self, v: u32) {
        let x = u64::from(v) + 1;
        let bits = 64 - x.leading_zeros();
        self.put(0, bits - 1);
        for k in (0..bits).rev() {
            self.put((x >> k & 1) as u32, 1);
        }
    }

    /// Copie les bits `from..to` de `src`.
    fn copy(&mut self, src: &[u8], from: usize, to: usize) {
        for i in from..to {
            self.put(u32::from(src[i / 8] >> (7 - i % 8) & 1), 1);
        }
    }
}

/// Ce qu'il faut savoir d'un SPS pour le rendre conforme (positions en bits
/// dans le RBSP, après l'octet d'en-tête de la NAL).
struct SpsShape {
    max_ref: u32,
    /// `vui_parameters_present_flag`.
    vui_at: usize,
    vui: bool,
    /// `bitstream_restriction_flag` (avec un VUI).
    restriction_at: usize,
    restriction: bool,
    /// `max_num_reorder_frames` (avec une restriction).
    reorder_at: usize,
    reorder: u32,
    max_dec: u32,
}

fn sps_shape(rbsp: &[u8]) -> Option<SpsShape> {
    let mut r = Bits { d: rbsp, at: 0 };
    let profile = r.u(8)?;
    r.u(16)?;
    r.ue()?;
    if matches!(profile, 100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135) {
        let chroma = r.ue()?;
        if chroma == 3 {
            // Plans de couleur séparés : pas réécrit par WebRTC non plus.
            return None;
        }
        r.skip_ue(2)?;
        r.u(1)?;
        if r.flag()? {
            for i in 0..8 {
                if !r.flag()? {
                    continue;
                }
                let (mut last, mut next) = (8i32, 8i32);
                for _ in 0..if i < 6 { 16 } else { 64 } {
                    if next != 0 {
                        let k = r.ue()?;
                        let delta = if k % 2 == 1 { i64::from(k / 2) + 1 } else { -i64::from(k / 2) };
                        next = ((i64::from(last) + delta + 256) % 256) as i32;
                    }
                    if next != 0 {
                        last = next;
                    }
                }
            }
        }
    }
    r.ue()?;
    match r.ue()? {
        0 => {
            r.ue()?;
        }
        1 => {
            r.u(1)?;
            r.skip_ue(2)?;
            let n = r.ue()?;
            if n > 255 {
                return None;
            }
            r.skip_ue(n as usize)?;
        }
        _ => {}
    }
    let max_ref = r.ue()?;
    r.u(1)?;
    r.skip_ue(2)?;
    if !r.flag()? {
        r.u(1)?;
    }
    r.u(1)?;
    if r.flag()? {
        r.skip_ue(4)?;
    }
    let vui_at = r.at;
    let vui = r.flag()?;
    let mut s = SpsShape { max_ref, vui_at, vui, restriction_at: 0, restriction: false, reorder_at: 0, reorder: 0, max_dec: 0 };
    if !vui {
        return Some(s);
    }
    if r.flag()? && r.u(8)? == 255 {
        r.u(32)?;
    }
    if r.flag()? {
        r.u(1)?;
    }
    if r.flag()? {
        r.u(4)?;
        if r.flag()? {
            r.u(24)?;
        }
    }
    if r.flag()? {
        r.skip_ue(2)?;
    }
    if r.flag()? {
        r.u(32)?;
        r.u(32)?;
        r.u(1)?;
    }
    let nal_hrd = r.flag()?;
    if nal_hrd {
        r.skip_hrd()?;
    }
    let vcl_hrd = r.flag()?;
    if vcl_hrd {
        r.skip_hrd()?;
    }
    if nal_hrd || vcl_hrd {
        r.u(1)?;
    }
    r.u(1)?;
    s.restriction_at = r.at;
    s.restriction = r.flag()?;
    if s.restriction {
        r.u(1)?;
        r.skip_ue(4)?;
        s.reorder_at = r.at;
        s.reorder = r.ue()?;
        s.max_dec = r.ue()?;
    }
    Some(s)
}

/// SPS rendu conforme à ce qu'attend WebRTC, `None` s'il l'est déjà (ou
/// s'il est illisible : il part alors tel quel).
///
/// Les clients officiels (libwebrtc, `sps_vui_rewriter.cc`) réécrivent à
/// la réception tout SPS dont le VUI n'annonce pas « aucune image
/// réordonnée » (`bitstream_restriction_flag`, `max_num_reorder_frames` à
/// 0, `max_dec_frame_buffering` ≤ `max_num_ref_frames`). Or le SPS fait
/// partie des octets authentifiés par DAVE : réécrit, l'image ne se
/// déchiffre plus et le direct charge sans fin chez ceux qui regardent
/// (constaté par l'utilisateur, 0.1.22). La réécriture est donc faite ici,
/// avant le chiffrement, avec les mêmes valeurs que libwebrtc.
pub fn conform_sps(nal: &[u8]) -> Option<Vec<u8>> {
    if nal_type(nal) != NAL_SPS {
        return None;
    }
    let rbsp = unescape(&nal[1..]);
    let s = sps_shape(&rbsp)?;
    if s.vui && s.restriction && s.reorder == 0 && s.max_dec <= s.max_ref {
        return None;
    }
    let mut out = BitsOut::default();
    out.copy(&rbsp, 0, s.vui_at);
    out.put(1, 1);
    if s.vui {
        out.copy(&rbsp, s.vui_at + 1, s.restriction_at);
    } else {
        // Aucune des autres informations du VUI.
        out.put(0, 8);
    }
    out.put(1, 1);
    if s.restriction {
        out.copy(&rbsp, s.restriction_at + 1, s.reorder_at);
    } else {
        // Valeurs de libwebrtc (`AddBitstreamRestriction`).
        out.put(1, 1);
        out.ue(2);
        out.ue(1);
        out.ue(16);
        out.ue(16);
    }
    out.ue(0);
    out.ue(s.max_ref);
    // Bit d'arrêt, puis des zéros jusqu'à l'octet.
    out.put(1, 1);
    let mut nal_out = vec![nal[0]];
    nal_out.extend(escape(&out.d));
    Some(nal_out)
}

/// Le SPS `nal` est-il conforme pour WebRTC (aucune réécriture à la
/// réception) ? `None` : illisible.
pub fn sps_conforming(nal: &[u8]) -> Option<bool> {
    let s = sps_shape(&unescape(nal.get(1..)?))?;
    Some(s.vui && s.restriction && s.reorder == 0 && s.max_dec <= s.max_ref)
}

/// Met en forme les images H.264 à envoyer : SPS conforme
/// ([`conform_sps`]), et SPS et PPS redonnés devant toute image clé qui ne
/// les porte pas (certains encodeurs ne les donnent qu'une fois : une
/// personne arrivée ensuite n'aurait rien à décoder).
#[derive(Default)]
pub struct Normalizer {
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
}

impl Normalizer {
    pub fn frame(&mut self, annexb: &[u8]) -> Vec<u8> {
        let nals = nal_units(annexb);
        let has = |t: u8| nals.iter().any(|n| nal_type(n) == t);
        let mut out = Vec::with_capacity(annexb.len() + 96);
        let push = |out: &mut Vec<u8>, n: &[u8]| {
            out.extend_from_slice(&START_CODE);
            out.extend_from_slice(n);
        };
        if has(NAL_IDR)
            && !has(NAL_SPS)
            && let (Some(s), Some(p)) = (&self.sps, &self.pps)
        {
            push(&mut out, s);
            if !has(NAL_PPS) {
                push(&mut out, p);
            }
        }
        for n in nals {
            match nal_type(n) {
                NAL_SPS => {
                    let s = conform_sps(n).unwrap_or_else(|| n.to_vec());
                    push(&mut out, &s);
                    self.sps = Some(s);
                }
                NAL_PPS => {
                    self.pps = Some(n.to_vec());
                    push(&mut out, n);
                }
                _ => push(&mut out, n),
            }
        }
        out
    }
}

fn fragment(n: &[u8], max: usize, out: &mut Vec<Vec<u8>>) {
    let h = n[0];
    let body = &n[1..];
    let chunk = max - 2;
    let count = body.len().div_ceil(chunk);
    for (k, part) in body.chunks(chunk).enumerate() {
        let mut fu = h & 0x1F;
        if k == 0 {
            fu |= 0x80;
        }
        if k + 1 == count {
            fu |= 0x40;
        }
        let mut p = Vec::with_capacity(2 + part.len());
        p.push((h & 0xE0) | FU_A);
        p.push(fu);
        p.extend_from_slice(part);
        out.push(p);
    }
}

/// Image Annex B à partir des charges utiles d'une image, dans l'ordre.
/// `None` : fragment manquant ou charge utile d'un type non pris en charge.
pub fn depacketize<'a>(payloads: impl IntoIterator<Item = &'a [u8]>) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut in_fu = false;
    for p in payloads {
        let h = *p.first()?;
        match h & 0x1F {
            1..=23 => {
                if in_fu {
                    return None;
                }
                out.extend_from_slice(&START_CODE);
                out.extend_from_slice(p);
            }
            STAP_A => {
                if in_fu {
                    return None;
                }
                let mut r = &p[1..];
                while !r.is_empty() {
                    let n = usize::from(u16::from_be_bytes([*r.first()?, *r.get(1)?]));
                    r = &r[2..];
                    if n == 0 || n > r.len() {
                        return None;
                    }
                    out.extend_from_slice(&START_CODE);
                    out.extend_from_slice(&r[..n]);
                    r = &r[n..];
                }
            }
            FU_A => {
                let fu = *p.get(1)?;
                if fu & 0x80 != 0 {
                    if in_fu {
                        return None;
                    }
                    out.extend_from_slice(&START_CODE);
                    out.push((h & 0xE0) | (fu & 0x1F));
                    in_fu = true;
                } else if !in_fu {
                    return None;
                }
                out.extend_from_slice(&p[2..]);
                if fu & 0x40 != 0 {
                    in_fu = false;
                }
            }
            _ => return None,
        }
    }
    (!in_fu && !out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nal(t: u8, len: usize, seed: u8) -> Vec<u8> {
        // Corps sans 00 00 0x (aucun code de début émulé), comme après la prévention d'émulation.
        let mut n = vec![0x60 | t];
        n.extend((0..len - 1).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed) | 1));
        n
    }

    fn annexb(nals: &[Vec<u8>]) -> Vec<u8> {
        nals.iter().flat_map(|n| START_CODE.iter().chain(n.iter()).copied()).collect()
    }

    #[test]
    fn decoupage_annex_b() {
        let mut f = vec![0, 0, 1, 0x67, 1, 2];
        f.extend_from_slice(&[0, 0, 0, 1, 0x68, 3]);
        f.extend_from_slice(&[0, 0, 1, 0x65, 4, 0]);
        f.extend_from_slice(&[0, 0, 0, 1, 0x41, 9]);
        let n = nal_units(&f);
        assert_eq!(n, vec![&[0x67, 1, 2][..], &[0x68, 3], &[0x65, 4, 0], &[0x41, 9]]);
        assert!(is_keyframe(&f));
        assert!(!is_keyframe(&[0, 0, 0, 1, 0x41, 2]));
        assert_eq!(nal_units(&[0x41, 7]), vec![&[0x41, 7][..]]);
    }

    #[test]
    fn grosse_image_cle_aller_retour() {
        let frame = annexb(&[nal(NAL_SPS, 12, 1), nal(NAL_PPS, 5, 2), nal(NAL_IDR, 150_000, 3), nal(NAL_SLICE, 900, 4)]);
        let p = packetize(&frame, 1100);
        assert!(p.iter().all(|p| p.len() <= 1100));
        assert_eq!(p[0][0] & 0x1F, STAP_A, "SPS et PPS réunis");
        assert_eq!(p[1][0] & 0x1F, FU_A);
        assert_eq!(p.last().map(|p| p[0] & 0x1F), Some(NAL_SLICE));
        let back = depacketize(p.iter().map(Vec::as_slice));
        assert_eq!(back.as_deref(), Some(frame.as_slice()));
    }

    /// SPS de profil de base 640×360, `max_num_ref_frames` = 1, avec le VUI `vui` (bits).
    fn sps_with(vui: impl Fn(&mut BitsOut)) -> Vec<u8> {
        let mut b = BitsOut::default();
        b.put(66, 8);
        b.put(0xC0, 8);
        b.put(30, 8);
        b.ue(0); // seq_parameter_set_id
        b.ue(0); // log2_max_frame_num_minus4
        b.ue(2); // pic_order_cnt_type
        b.ue(1); // max_num_ref_frames
        b.put(0, 1);
        b.ue(39);
        b.ue(22);
        b.put(1, 1); // frame_mbs_only_flag
        b.put(1, 1); // direct_8x8_inference_flag
        b.put(1, 1); // frame_cropping_flag
        b.ue(0);
        b.ue(0);
        b.ue(0);
        b.ue(4);
        vui(&mut b);
        b.put(1, 1);
        let mut nal = vec![0x67];
        nal.extend(escape(&b.d));
        nal
    }

    #[test]
    fn sps_rendu_conforme_pour_webrtc() {
        // Sans VUI : un VUI réduit à la restriction, valeurs de libwebrtc.
        let bare = sps_with(|b| b.put(0, 1));
        let fixed = conform_sps(&bare).unwrap_or_else(|| panic!("à réécrire"));
        assert_eq!(fixed[0], 0x67);
        let s = sps_shape(&unescape(&fixed[1..])).unwrap_or_else(|| panic!("lisible"));
        assert!(s.vui && s.restriction);
        assert_eq!((s.reorder, s.max_dec, s.max_ref), (0, 1, 1));
        assert_eq!(conform_sps(&fixed), None, "déjà conforme : inchangé");
        // VUI avec minutage mais sans restriction : le minutage est gardé.
        let timed = sps_with(|b| {
            b.put(1, 1);
            b.put(0, 4);
            b.put(1, 1);
            b.put(1, 32);
            b.put(60, 32);
            b.put(1, 1);
            b.put(0, 4);
        });
        let fixed = conform_sps(&timed).unwrap_or_else(|| panic!("à réécrire"));
        let (a, b) = (unescape(&timed[1..]), unescape(&fixed[1..]));
        let s = sps_shape(&b).unwrap_or_else(|| panic!("lisible"));
        assert!(s.restriction && s.reorder == 0 && s.max_dec == 1);
        let (mut x, mut y) = (Bits { d: &a, at: s.vui_at + 6 }, Bits { d: &b, at: s.vui_at + 6 });
        assert_eq!((x.u(32), x.u(32)), (Some(1), Some(60)));
        assert_eq!((y.u(32), y.u(32)), (Some(1), Some(60)));
        // Restriction qui annonce des images réordonnées : ramenée à zéro.
        let reorder = sps_with(|b| {
            b.put(1, 1);
            b.put(0, 8);
            b.put(1, 1);
            b.put(1, 1);
            b.ue(0);
            b.ue(0);
            b.ue(10);
            b.ue(10);
            b.ue(2);
            b.ue(4);
        });
        let fixed = conform_sps(&reorder).unwrap_or_else(|| panic!("à réécrire"));
        let s = sps_shape(&unescape(&fixed[1..])).unwrap_or_else(|| panic!("lisible"));
        assert_eq!((s.reorder, s.max_dec), (0, 1));
        // Illisible ou autre NAL : laissé tel quel.
        assert_eq!(conform_sps(&[0x67, 66]), None);
        assert_eq!(conform_sps(&[0x68, 1, 2, 3]), None);
        // Prévention d'émulation : aller et retour.
        let raw = [0u8, 0, 0, 1, 0, 0, 3, 0, 0, 2, 9];
        assert_eq!(unescape(&escape(&raw)), raw);
        assert!(!escape(&raw).windows(3).any(|w| w[0] == 0 && w[1] == 0 && w[2] <= 2));
    }

    #[test]
    fn images_normalisees_avant_l_envoi() {
        let sps = sps_with(|b| b.put(0, 1));
        let fixed = conform_sps(&sps).unwrap_or_else(|| panic!("à réécrire"));
        let (pps, idr, p) = (nal(NAL_PPS, 5, 2), nal(NAL_IDR, 40, 3), nal(NAL_SLICE, 30, 4));
        let mut n = Normalizer::default();
        // Image clé complète : SPS réécrit, le reste intact.
        assert_eq!(n.frame(&annexb(&[sps.clone(), pps.clone(), idr.clone()])), annexb(&[fixed.clone(), pps.clone(), idr.clone()]));
        assert_eq!(n.frame(&annexb(std::slice::from_ref(&p))), annexb(std::slice::from_ref(&p)));
        // Image clé nue : SPS et PPS connus redonnés devant.
        assert_eq!(n.frame(&annexb(std::slice::from_ref(&idr))), annexb(&[fixed, pps, idr.clone()]));
        // Rien de connu : inchangée.
        assert_eq!(Normalizer::default().frame(&annexb(std::slice::from_ref(&idr))), annexb(&[idr]));
        // Une NAL par paquet à l'envoi, jamais d'agrégation.
        let frame = annexb(&[nal(NAL_SPS, 12, 1), nal(NAL_PPS, 5, 2), nal(NAL_IDR, 3000, 3)]);
        let pk = packetize_plain(&frame, 1100);
        assert_eq!(pk.iter().map(|p| p[0] & 0x1F).collect::<Vec<_>>(), vec![NAL_SPS, NAL_PPS, FU_A, FU_A, FU_A]);
        assert_eq!(depacketize(pk.iter().map(Vec::as_slice)).as_deref(), Some(frame.as_slice()));
    }

    #[test]
    fn codes_courts_rendus_longs() {
        let f = [0, 0, 1, 0x41, 5, 6];
        assert_eq!(depacketize(packetize(&f, 1200).iter().map(Vec::as_slice)), Some(vec![0, 0, 0, 1, 0x41, 5, 6]));
    }

    #[test]
    fn fragment_perdu_refuse() {
        let frame = annexb(&[nal(NAL_IDR, 5000, 7)]);
        // Un fragment du milieu perdu ne se voit qu'aux numéros de séquence
        // (le récepteur n'assemble que des suites contiguës) ; ici : le début.
        let mut p = packetize(&frame, 1000);
        p.remove(0);
        assert_eq!(depacketize(p.iter().map(Vec::as_slice)), None, "début manquant");
        let p = packetize(&frame, 1000);
        assert_eq!(depacketize(p[..p.len() - 1].iter().map(Vec::as_slice)), None, "fin manquante");
        assert_eq!(depacketize([&[0x7Du8, 1][..]]), None, "type 29 inconnu");
        assert_eq!(depacketize([&[0x78u8, 0, 9, 1][..]]), None, "STAP-A tronqué");
    }
}
