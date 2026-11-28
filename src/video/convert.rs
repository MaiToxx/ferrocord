//! Conversions de couleurs et mise à l'échelle des images vidéo.
//!
//! Tout écrit dans des tampons fournis par l'appelant (`&mut Vec<u8>`) :
//! ils ne sont agrandis que si l'image grandit, donc aucune allocation par
//! image en régime établi. Couleurs : BT.601 à plage limitée (16–235), ce
//! que produisent et attendent les codecs du temps réel (WebRTC, OpenH264).

use super::{Frame, FrameData};

/// Taille de tampon incohérente avec les dimensions annoncées.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadSize;

impl std::fmt::Display for BadSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("taille d'image incohérente")
    }
}

impl std::error::Error for BadSize {}

/// Plans I420 contigus, dimensions comprises.
#[derive(Debug, Clone, Copy)]
pub struct I420<'a> {
    pub width: usize,
    pub height: usize,
    pub y: &'a [u8],
    pub u: &'a [u8],
    pub v: &'a [u8],
}

impl I420<'_> {
    fn check(&self) -> Result<(), BadSize> {
        let c = self.width.div_ceil(2) * self.height.div_ceil(2);
        if self.y.len() == self.width * self.height && self.u.len() == c && self.v.len() == c {
            Ok(())
        } else {
            Err(BadSize)
        }
    }
}

fn fit(buf: &mut Vec<u8>, len: usize) {
    if buf.capacity() < len {
        // Tampon neuf ou trop petit : demandé déjà à zéro au système, qui
        // donne des pages vierges sans les écrire. `resize` aurait recopié
        // l'ancien contenu puis rempli le reste, avant que la conversion ne
        // réécrive tout.
        *buf = vec![0; len];
    } else {
        // Le contenu sera écrasé.
        buf.resize(len, 0);
    }
}

#[inline(always)]
fn clamp8(x: i32) -> u8 {
    x.clamp(0, 255) as u8
}

#[inline(always)]
fn sat8(x: i16) -> u8 {
    x.clamp(0, 255) as u8
}

// Vitesse (lot « vidéo ») : ces boucles traitent chaque pixel d'une image, et
// leur forme décide de tout. Écrites ligne par ligne, sur des tranches de
// même longueur, en entiers de 16 bits et sans indice calculé, le
// compilateur les traite par 16 octets à la fois ; compilées une seconde
// fois pour AVX2 (`*_avx2`, choisies à l'exécution quand le processeur le
// sait), par 32. Mesuré sur un i9 à 32 cœurs logiques, une image de 1080p :
// affichage d'un direct 22 ms → 0,27 ms (la boucle d'origine calculait une
// division et quatre contrôles de bornes par pixel), lecteur vidéo 9,5 ms →
// 0,27 ms.

/// Une ligne : un Y, un U et un V par pixel → RGBA opaque. BT.601 à plage
/// limitée, virgule fixe ×64 : 1,164 (74,5 : ×149 puis ÷2), 1,596, 0,391,
/// 0,813, 2,018. Tout tient sur 16 bits, sauf le bleu d'une couleur hors
/// gamme, que l'addition saturée borne comme l'aurait fait le calcul exact.
#[inline(always)]
fn row444(y: &[u8], u: &[u8], v: &[u8], out: &mut [u8]) {
    for (((px, &y), &u), &v) in out.as_chunks_mut::<4>().0.iter_mut().zip(y).zip(u).zip(v) {
        // −16 × 74,5 = −1192, et +32 d'arrondi avant la division par 64.
        let c = ((u16::from(y) * 149) >> 1) as i16 - 1160;
        let d = i16::from(u) - 128;
        let e = i16::from(v) - 128;
        let r = c + 102 * e;
        let g = c - 25 * d - 52 * e;
        let b = c.saturating_add(129 * d);
        *px = [sat8(r >> 6), sat8(g >> 6), sat8(b >> 6), 255];
    }
}

/// Lignes de travail de [`i420_to_rgba_sized`], gardées par fil : rien n'est
/// alloué d'une image à l'autre.
#[derive(Default)]
struct Rows {
    /// (largeur source, largeur de sortie) de `xs`.
    key: (usize, usize),
    /// Colonne source de chaque colonne de sortie (plus proche voisin).
    xs: Vec<u32>,
    /// Y, U et V de la ligne en cours, un par pixel de sortie.
    yuv: Vec<u8>,
}

thread_local! {
    static ROWS: std::cell::RefCell<Rows> = std::cell::RefCell::default();
}

impl Rows {
    fn prepare(&mut self, sw: usize, w: usize) {
        if self.key != (sw, w) || self.xs.len() != w {
            self.xs.clear();
            self.xs.extend((0..w).map(|ox| (ox * sw / w) as u32));
            self.key = (sw, w);
        }
        fit(&mut self.yuv, w * 3);
    }
}

#[inline(always)]
fn i420_rows(src: I420<'_>, w: usize, h: usize, out: &mut [u8], rows: &mut Rows) {
    let (sw, sh) = (src.width, src.height);
    let cw = sw.div_ceil(2);
    let (ty, rest) = rows.yuv.split_at_mut(w);
    let (tu, tv) = rest.split_at_mut(w);
    for (oy, line) in out.chunks_exact_mut(w * 4).enumerate() {
        let sy = oy * sh / h;
        let ys = &src.y[sy * sw..sy * sw + sw];
        let us = &src.u[(sy / 2) * cw..(sy / 2) * cw + cw];
        let vs = &src.v[(sy / 2) * cw..(sy / 2) * cw + cw];
        if sw == w {
            // Même largeur : la chrominance est doublée, la luminance lue en place.
            for (d, &s) in tu.as_chunks_mut::<2>().0.iter_mut().zip(us) {
                *d = [s, s];
            }
            for (d, &s) in tv.as_chunks_mut::<2>().0.iter_mut().zip(vs) {
                *d = [s, s];
            }
            if !w.is_multiple_of(2) {
                tu[w - 1] = us[cw - 1];
                tv[w - 1] = vs[cw - 1];
            }
            row444(ys, tu, tv, line);
        } else {
            for (((a, b), c), &x) in ty.iter_mut().zip(tu.iter_mut()).zip(tv.iter_mut()).zip(&rows.xs) {
                let x = x as usize;
                *a = ys[x];
                *b = us[x / 2];
                *c = vs[x / 2];
            }
            row444(ty, tu, tv, line);
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn i420_rows_avx2(src: I420<'_>, w: usize, h: usize, out: &mut [u8], rows: &mut Rows) {
    i420_rows(src, w, h, out, rows)
}

/// Met `buf` à `pixels` pixels et rend ses octets : quatre par pixel, dans
/// l'ordre rouge, vert, bleu, alpha. Un tampon de mots de 32 bits peut devenir
/// une image d'egui sans être recopié : les pixels d'egui sont alignés sur
/// quatre octets, ce qu'un `Vec<u8>` ne garantit pas (lot « textures »).
pub fn pixel_bytes(buf: &mut Vec<u32>, pixels: usize) -> &mut [u8] {
    if buf.capacity() < pixels {
        // Demandé à zéro au système : des pages vierges, que personne n'écrit avant la conversion.
        *buf = vec![0; pixels];
    } else {
        buf.resize(pixels, 0);
    }
    bytemuck::cast_slice_mut(buf)
}

/// I420 → RGBA (alpha opaque) de `w` × `h`, au plus proche voisin : pour une
/// texture à la taille affichée. Des plans plus longs qu'annoncé sont
/// acceptés ; trop courts, ou une taille nulle : refusés.
pub fn i420_to_rgba_sized(src: I420<'_>, w: usize, h: usize, out: &mut Vec<u8>) -> Result<(), BadSize> {
    if w == 0 || h == 0 {
        return Err(BadSize);
    }
    fit(out, w * h * 4);
    i420_to_rgba_into(src, w, h, out)
}

/// Comme [`i420_to_rgba_sized`], dans une tranche de `w * h * 4` octets.
pub fn i420_to_rgba_into(src: I420<'_>, w: usize, h: usize, out: &mut [u8]) -> Result<(), BadSize> {
    let (sw, sh) = (src.width, src.height);
    let c = sw.div_ceil(2) * sh.div_ceil(2);
    if sw == 0 || sh == 0 || w == 0 || h == 0 || out.len() != w * h * 4 || src.y.len() < sw * sh || src.u.len() < c || src.v.len() < c {
        return Err(BadSize);
    }
    ROWS.with_borrow_mut(|rows| {
        rows.prepare(sw, w);
        #[cfg(target_arch = "x86_64")]
        if std::is_x86_feature_detected!("avx2") {
            // SAFETY: AVX2 vient d'être constaté sur ce processeur.
            return unsafe { i420_rows_avx2(src, w, h, out, rows) };
        }
        i420_rows(src, w, h, out, rows)
    });
    Ok(())
}

/// I420 → RGBA (alpha opaque).
pub fn i420_to_rgba(src: I420<'_>, out: &mut Vec<u8>) -> Result<(), BadSize> {
    src.check()?;
    if src.width == 0 || src.height == 0 {
        out.clear();
        return Ok(());
    }
    i420_to_rgba_sized(src, src.width, src.height, out)
}

#[inline(always)]
fn bgra_rows(src: &[u8], sw: usize, sh: usize, w: usize, h: usize, out: &mut [u8], xs: &[u32]) {
    for (oy, line) in out.chunks_exact_mut(w * 4).enumerate() {
        let sy = oy * sh / h;
        let row = src[sy * sw * 4..(sy * sw + sw) * 4].as_chunks::<4>().0;
        if sw == w {
            for (d, s) in line.as_chunks_mut::<4>().0.iter_mut().zip(row) {
                *d = [s[2], s[1], s[0], 255];
            }
        } else {
            for (d, &x) in line.as_chunks_mut::<4>().0.iter_mut().zip(xs) {
                let s = row[x as usize];
                *d = [s[2], s[1], s[0], 255];
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn bgra_rows_avx2(src: &[u8], sw: usize, sh: usize, w: usize, h: usize, out: &mut [u8], xs: &[u32]) {
    bgra_rows(src, sw, sh, w, h, out, xs)
}

/// BGRA de `sw` × `sh` → RGBA de `w` × `h` (plus proche voisin, échange
/// rouge/bleu, alpha forcé opaque). Une source plus longue qu'annoncé est
/// acceptée ; trop courte, ou une taille nulle : refusée.
pub fn bgra_to_rgba_sized(sw: usize, sh: usize, src: &[u8], w: usize, h: usize, out: &mut Vec<u8>) -> Result<(), BadSize> {
    if w == 0 || h == 0 {
        return Err(BadSize);
    }
    fit(out, w * h * 4);
    bgra_to_rgba_into(sw, sh, src, w, h, out)
}

/// Comme [`bgra_to_rgba_sized`], dans une tranche de `w * h * 4` octets.
pub fn bgra_to_rgba_into(sw: usize, sh: usize, src: &[u8], w: usize, h: usize, out: &mut [u8]) -> Result<(), BadSize> {
    if sw == 0 || sh == 0 || w == 0 || h == 0 || out.len() != w * h * 4 || src.len() < sw * sh * 4 {
        return Err(BadSize);
    }
    ROWS.with_borrow_mut(|rows| {
        rows.prepare(sw, w);
        #[cfg(target_arch = "x86_64")]
        if std::is_x86_feature_detected!("avx2") {
            // SAFETY: AVX2 vient d'être constaté sur ce processeur.
            return unsafe { bgra_rows_avx2(src, sw, sh, w, h, out, &rows.xs) };
        }
        bgra_rows(src, sw, sh, w, h, out, &rows.xs)
    });
    Ok(())
}

/// BGRA → RGBA (échange rouge/bleu, alpha forcé opaque : les captures
/// d'écran laissent souvent un alpha à 0).
pub fn bgra_to_rgba(width: usize, height: usize, src: &[u8], out: &mut Vec<u8>) -> Result<(), BadSize> {
    if src.len() != width * height * 4 {
        return Err(BadSize);
    }
    if src.is_empty() {
        out.clear();
        return Ok(());
    }
    bgra_to_rgba_sized(width, height, src, width, height, out)
}

/// N'importe quelle [`Frame`] → RGBA, pour une texture.
pub fn frame_to_rgba(f: &Frame, out: &mut Vec<u8>) -> Result<(), BadSize> {
    let (w, h) = (f.width as usize, f.height as usize);
    match &f.data {
        FrameData::I420 { y, u, v } => i420_to_rgba(I420 { width: w, height: h, y, u, v }, out),
        FrameData::Bgra(b) => bgra_to_rgba(w, h, b, out),
    }
}

/// N'importe quelle [`Frame`] → RGBA à sa taille, dans une tranche de
/// `largeur * hauteur * 4` octets.
pub fn frame_to_rgba_into(f: &Frame, out: &mut [u8]) -> Result<(), BadSize> {
    let (w, h) = (f.width as usize, f.height as usize);
    match &f.data {
        FrameData::I420 { y, u, v } => {
            let src = I420 { width: w, height: h, y, u, v };
            src.check()?;
            i420_to_rgba_into(src, w, h, out)
        }
        FrameData::Bgra(b) if b.len() == w * h * 4 => bgra_to_rgba_into(w, h, b, w, h, out),
        FrameData::Bgra(_) => Err(BadSize),
    }
}

#[inline(always)]
fn luma(b: u8, g: u8, r: u8) -> u8 {
    // BT.601, plage limitée, virgule fixe ×256 : 0.257, 0.504, 0.098.
    // Somme au plus 56 228 : tient sur 16 bits, que le compilateur vectorise par 16 ou 32 pixels.
    (((66 * u16::from(r) + 129 * u16::from(g) + 25 * u16::from(b) + 128) >> 8) + 16) as u8
}

/// Luminance des `ew` premiers pixels de chaque ligne, la source ayant des
/// lignes de `width` pixels.
#[inline(always)]
fn luma_rows(width: usize, src: &[u8], ew: usize, dy: &mut [u8]) {
    for (d, line) in dy.chunks_exact_mut(ew).zip(src.chunks_exact(width * 4)) {
        for (d, p) in d.iter_mut().zip(line[..ew * 4].as_chunks::<4>().0) {
            *d = luma(p[0], p[1], p[2]);
        }
    }
}

/// U et V d'un bloc 2×2 : `p` en est les deux pixels du haut, `q` ceux du bas.
#[inline(always)]
fn chroma2x2(p: &[u8; 8], q: &[u8; 8]) -> [u8; 2] {
    let sb = i32::from(u16::from(p[0]) + u16::from(p[4]) + u16::from(q[0]) + u16::from(q[4]));
    let sg = i32::from(u16::from(p[1]) + u16::from(p[5]) + u16::from(q[1]) + u16::from(q[5]));
    let sr = i32::from(u16::from(p[2]) + u16::from(p[6]) + u16::from(q[2]) + u16::from(q[6]));
    // Somme de 4 pixels : les coefficients ×256 deviennent ×1024.
    [clamp8(((-38 * sr - 74 * sg + 112 * sb + 512) >> 10) + 128), clamp8(((112 * sr - 94 * sg - 18 * sb + 512) >> 10) + 128)]
}

/// Côtés pairs (toute image envoyée à un encodeur) : les blocs 2×2 sont lus
/// par tranches de huit octets sur deux lignes, sans calcul d'indice ni test
/// de bord, ce que le compilateur vectorise. Mêmes formules, mêmes valeurs
/// que la boucle générale (test `even_fast_path_matches`).
/// Mesuré en 4K : 9,0 ms par image avant, 7,7 ms après.
#[inline(always)]
fn i420_even(width: usize, src: &[u8], y: &mut [u8], u: &mut [u8], v: &mut [u8]) {
    let (stride, cw) = (width * 4, width / 2);
    luma_rows(width, src, width, y);
    for ((rows, du), dv) in src.chunks_exact(stride * 2).zip(u.chunks_exact_mut(cw)).zip(v.chunks_exact_mut(cw)) {
        let (top, bottom) = rows.split_at(stride);
        for (((p, q), du), dv) in top.as_chunks::<8>().0.iter().zip(bottom.as_chunks::<8>().0).zip(du.iter_mut()).zip(dv.iter_mut()) {
            [*du, *dv] = chroma2x2(p, q);
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn i420_even_avx2(width: usize, src: &[u8], y: &mut [u8], u: &mut [u8], v: &mut [u8]) {
    i420_even(width, src, y, u, v)
}

/// BGRA → I420. La chrominance est la moyenne de chaque bloc 2×2 (ou de ce
/// qu'il en reste sur un bord impair).
pub fn bgra_to_i420(width: usize, height: usize, src: &[u8], y: &mut Vec<u8>, u: &mut Vec<u8>, v: &mut Vec<u8>) -> Result<(), BadSize> {
    if src.len() != width * height * 4 {
        return Err(BadSize);
    }
    let (cw, ch) = (width.div_ceil(2), height.div_ceil(2));
    fit(y, width * height);
    fit(u, cw * ch);
    fit(v, cw * ch);
    if width == 0 || height == 0 {
        return Ok(());
    }
    let stride = width * 4;
    if width.is_multiple_of(2) && height.is_multiple_of(2) {
        #[cfg(target_arch = "x86_64")]
        if std::is_x86_feature_detected!("avx2") {
            // SAFETY: AVX2 vient d'être constaté sur ce processeur.
            unsafe { i420_even_avx2(width, src, y, u, v) };
            return Ok(());
        }
        i420_even(width, src, y, u, v);
        return Ok(());
    }
    luma_rows(width, src, width, y);
    for cy in 0..ch {
        let r0 = cy * 2;
        let r1 = (r0 + 1).min(height - 1);
        for cx in 0..cw {
            let x0 = cx * 2;
            let x1 = (x0 + 1).min(width - 1);
            let (mut sb, mut sg, mut sr) = (0i32, 0i32, 0i32);
            for (row, col) in [(r0, x0), (r0, x1), (r1, x0), (r1, x1)] {
                let p = &src[row * stride + col * 4..row * stride + col * 4 + 4];
                sb += i32::from(p[0]);
                sg += i32::from(p[1]);
                sr += i32::from(p[2]);
            }
            u[cy * cw + cx] = clamp8(((-38 * sr - 74 * sg + 112 * sb + 512) >> 10) + 128);
            v[cy * cw + cx] = clamp8(((112 * sr - 94 * sg - 18 * sb + 512) >> 10) + 128);
        }
    }
    Ok(())
}

/// BGRA (lignes de `width` pixels) → NV12 de `ew` × `eh`, écrit dans `dst`
/// (`ew * eh * 3 / 2` octets : la luminance, puis U et V entrelacés).
/// `ew` et `eh` sont pairs et tiennent dans la source : une dernière colonne
/// ou ligne impaire est rognée. Mêmes valeurs que [`bgra_to_i420`].
///
/// Pour l'encodeur de la carte graphique : la conversion s'écrit dans son
/// tampon d'entrée, sans passer par trois plans intermédiaires (12 Mo gardés
/// pendant un partage en 4K, et une copie de plus par image).
pub fn bgra_to_nv12(width: usize, src: &[u8], ew: usize, eh: usize, dst: &mut [u8]) -> Result<(), BadSize> {
    let stride = width * 4;
    if ew == 0 || eh == 0 || !ew.is_multiple_of(2) || !eh.is_multiple_of(2) || ew > width || src.len() < stride * eh || dst.len() != ew * eh * 3 / 2 {
        return Err(BadSize);
    }
    #[cfg(target_arch = "x86_64")]
    if std::is_x86_feature_detected!("avx2") {
        // SAFETY: AVX2 vient d'être constaté sur ce processeur.
        unsafe { nv12_rows_avx2(width, src, ew, eh, dst) };
        return Ok(());
    }
    nv12_rows(width, src, ew, eh, dst);
    Ok(())
}

#[inline(always)]
fn nv12_rows(width: usize, src: &[u8], ew: usize, eh: usize, dst: &mut [u8]) {
    let stride = width * 4;
    let (dy, duv) = dst.split_at_mut(ew * eh);
    luma_rows(width, src, ew, dy);
    for (rows, d) in src.chunks_exact(stride * 2).zip(duv.chunks_exact_mut(ew)) {
        let (top, bottom) = rows.split_at(stride);
        for ((p, q), d) in top[..ew * 4].as_chunks::<8>().0.iter().zip(bottom[..ew * 4].as_chunks::<8>().0).zip(d.as_chunks_mut::<2>().0) {
            *d = chroma2x2(p, q);
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn nv12_rows_avx2(width: usize, src: &[u8], ew: usize, eh: usize, dst: &mut [u8]) {
    nv12_rows(width, src, ew, eh, dst)
}

/// Mise à l'échelle bilinéaire d'images RGBA. Garde ses tables de
/// coefficients d'un appel à l'autre : rien n'est alloué tant que les
/// dimensions ne changent pas.
#[derive(Default)]
pub struct Scaler {
    key: (usize, usize, usize, usize),
    /// Par colonne de destination : indice de la colonne source de gauche et poids de droite (×256).
    xs: Vec<(u32, u32)>,
    ys: Vec<(u32, u32)>,
    /// Les deux lignes sources d'une ligne de sortie, mêlées, sur 16 bits.
    mix: Vec<u16>,
}

fn table(src: usize, dst: usize, out: &mut Vec<(u32, u32)>) {
    out.clear();
    // Centres de pixels alignés (comme la plupart des bibliothèques d'images), en ×65536.
    let step = ((src as u64) << 16) / dst as u64;
    let max = (src - 1) as i64;
    for i in 0..dst {
        let pos = ((i as i64 * step as i64) + (step as i64 >> 1) - (1 << 15)).max(0);
        let i0 = (pos >> 16).min(max);
        let frac = if i0 >= max { 0 } else { (pos & 0xffff) >> 8 };
        out.push((i0 as u32, frac as u32));
    }
}

impl Scaler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Met `src` (`sw`×`sh`, RGBA) à la taille `dw`×`dh` dans `out`.
    pub fn scale_rgba(&mut self, src: &[u8], sw: usize, sh: usize, out: &mut Vec<u8>, dw: usize, dh: usize) -> Result<(), BadSize> {
        if src.len() != sw * sh * 4 || sw == 0 || sh == 0 || dw == 0 || dh == 0 {
            return Err(BadSize);
        }
        fit(out, dw * dh * 4);
        self.scale_rgba_into(src, sw, sh, out, dw, dh)
    }

    /// Comme [`Scaler::scale_rgba`], dans une tranche de `dw * dh * 4` octets.
    pub fn scale_rgba_into(&mut self, src: &[u8], sw: usize, sh: usize, out: &mut [u8], dw: usize, dh: usize) -> Result<(), BadSize> {
        if src.len() != sw * sh * 4 || sw == 0 || sh == 0 || dw == 0 || dh == 0 || out.len() != dw * dh * 4 {
            return Err(BadSize);
        }
        if (sw, sh) == (dw, dh) {
            out.copy_from_slice(src);
            return Ok(());
        }
        let stride = sw * 4;
        // Moitié exacte : les quatre poids valent un quart, et le calcul
        // général se réduit à la moyenne arrondie de chaque bloc 2×2.
        // Mesuré, de la 4K au 1080p : 7,5 ms → 1,8 ms.
        if (sw, sh) == (dw * 2, dh * 2) {
            for (line, rows) in out.chunks_exact_mut(dw * 4).zip(src.chunks_exact(stride * 2)) {
                let (top, bottom) = rows.split_at(stride);
                for ((d, p), q) in line.as_chunks_mut::<4>().0.iter_mut().zip(top.as_chunks::<8>().0).zip(bottom.as_chunks::<8>().0) {
                    for c in 0..4 {
                        d[c] = ((u16::from(p[c]) + u16::from(p[c + 4]) + u16::from(q[c]) + u16::from(q[c + 4]) + 2) >> 2) as u8;
                    }
                }
            }
            return Ok(());
        }
        if self.key != (sw, sh, dw, dh) {
            table(sw, dw, &mut self.xs);
            table(sh, dh, &mut self.ys);
            self.key = (sw, sh, dw, dh);
        }
        self.mix.resize(stride, 0);
        // En deux passes, aux mêmes valeurs qu'en une (aucun arrondi entre
        // les deux) : la verticale mêle deux lignes entières sans indice, ce
        // que le compilateur vectorise ; l'horizontale ne lit plus que deux
        // pixels par pixel de sortie. Mesuré, de la 4K au 1440p : 13,2 ms → 7,8 ms.
        for (line, &(y0, fy)) in out.chunks_exact_mut(dw * 4).zip(&self.ys) {
            let y0 = y0 as usize;
            let y1 = (y0 + 1).min(sh - 1);
            let (top, bottom) = (&src[y0 * stride..y0 * stride + stride], &src[y1 * stride..y1 * stride + stride]);
            // 255 × 256 = 65 280 : tient sur 16 bits.
            let (fy, gy) = (fy as u16, 256 - fy as u16);
            for ((m, &a), &b) in self.mix.iter_mut().zip(top).zip(bottom) {
                *m = u16::from(a) * gy + u16::from(b) * fy;
            }
            let px = self.mix.as_chunks::<4>().0;
            for (d, &(x0, fx)) in line.as_chunks_mut::<4>().0.iter_mut().zip(&self.xs) {
                let x0 = x0 as usize;
                let x1 = (x0 + 1).min(sw - 1);
                let (l, r) = (px[x0], px[x1]);
                for c in 0..4 {
                    d[c] = ((u32::from(l[c]) * (256 - fx) + u32::from(r[c]) * fx + (1 << 15)) >> 16) as u8;
                }
            }
        }
        Ok(())
    }
}

/// Plus grande taille qui tient dans `max_w`×`max_h` en gardant les
/// proportions (jamais agrandie, jamais nulle).
pub fn fit_within(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    if w == 0 || h == 0 || (w <= max_w && h <= max_h) {
        return (w, h);
    }
    let s = (f64::from(max_w) / f64::from(w)).min(f64::from(max_h) / f64::from(h));
    (((f64::from(w) * s).round() as u32).max(1), ((f64::from(h) * s).round() as u32).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn i420_gris_et_couleurs_primaires() {
        // Y=235, U=V=128 : blanc ; Y=16 : noir.
        let (y, u, v) = (vec![235, 16, 235, 16], vec![128], vec![128]);
        let mut out = Vec::new();
        i420_to_rgba(I420 { width: 2, height: 2, y: &y, u: &u, v: &v }, &mut out).unwrap();
        assert_eq!(&out[0..4], &[255, 255, 255, 255]);
        assert_eq!(&out[4..8], &[0, 0, 0, 255]);
    }

    #[test]
    fn aller_retour_bgra_i420_rgba() {
        // Aplats (la chrominance 4:2:0 ne perd alors rien) de couleurs variées, taille impaire.
        let colors = [[30u8, 60, 200], [200, 30, 60], [60, 200, 30], [128, 128, 128]];
        for c in colors {
            let (w, h) = (5, 3);
            let bgra: Vec<u8> = (0..w * h).flat_map(|_| [c[2], c[1], c[0], 0]).collect();
            let (mut y, mut u, mut v, mut rgba) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            bgra_to_i420(w, h, &bgra, &mut y, &mut u, &mut v).unwrap();
            assert_eq!((y.len(), u.len(), v.len()), (15, 6, 6));
            i420_to_rgba(I420 { width: w, height: h, y: &y, u: &u, v: &v }, &mut rgba).unwrap();
            for px in rgba.as_chunks::<4>().0 {
                for k in 0..3 {
                    assert!((i32::from(px[k]) - i32::from(c[k])).abs() <= 3, "{c:?} → {px:?}");
                }
                assert_eq!(px[3], 255);
            }
        }
    }

    #[test]
    fn tailles_incoherentes_refusees() {
        let mut out = Vec::new();
        assert_eq!(bgra_to_rgba(2, 2, &[0; 15], &mut out), Err(BadSize));
        assert_eq!(i420_to_rgba(I420 { width: 2, height: 2, y: &[0; 4], u: &[0; 2], v: &[0; 1] }, &mut out), Err(BadSize));
        assert_eq!(Scaler::new().scale_rgba(&[0; 4], 1, 1, &mut out, 0, 1), Err(BadSize));
    }

    #[test]
    fn mise_a_l_echelle_bilineaire() {
        let mut s = Scaler::new();
        let mut out = Vec::new();
        // Un aplat reste un aplat.
        let src = [10u8, 20, 30, 255].repeat(7 * 5);
        s.scale_rgba(&src, 7, 5, &mut out, 3, 2).unwrap();
        assert!(out.as_chunks::<4>().0.iter().all(|p| p == &[10, 20, 30, 255]));
        // Un dégradé horizontal 0 → 255 réduit de moitié reste croissant, aux bonnes bornes.
        let src: Vec<u8> = (0..2).flat_map(|_| (0..4).flat_map(|x| [x * 85, 0, 0, 255])).collect();
        s.scale_rgba(&src, 4, 2, &mut out, 2, 1).unwrap();
        assert!(out[0] < out[4], "{out:?}");
        assert!((40..=45).contains(&out[0]) && (210..=215).contains(&out[4]), "{out:?}");
        // Agrandissement : les bords gardent les valeurs des bords.
        s.scale_rgba(&src, 4, 2, &mut out, 8, 4).unwrap();
        assert_eq!(out[0], 0);
        assert_eq!(out[7 * 4], 255);
    }

    /// La mise à l'échelle d'origine, en une passe : quatre pixels lus et
    /// pesés par pixel de sortie.
    fn bilinear(src: &[u8], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<u8> {
        let (mut xs, mut ys) = (Vec::new(), Vec::new());
        table(sw, dw, &mut xs);
        table(sh, dh, &mut ys);
        let mut out = Vec::with_capacity(dw * dh * 4);
        for &(y0, fy) in &ys {
            let y1 = (y0 as usize + 1).min(sh - 1);
            for &(x0, fx) in &xs {
                let x1 = (x0 as usize + 1).min(sw - 1);
                for c in 0..4 {
                    let at = |x: usize, y: usize| u32::from(src[(y * sw + x) * 4 + c]);
                    let t = at(x0 as usize, y0 as usize) * (256 - fx) + at(x1, y0 as usize) * fx;
                    let m = at(x0 as usize, y1) * (256 - fx) + at(x1, y1) * fx;
                    out.push(((t * (256 - fy) + m * fy + (1 << 15)) >> 16) as u8);
                }
            }
        }
        out
    }

    #[test]
    fn mise_a_l_echelle_identique_a_la_reference() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut s = Scaler::new();
        let mut out = Vec::new();
        // Moitié exacte, réductions quelconques, agrandissement, une seule ligne ou colonne.
        for (sw, sh, dw, dh) in [(64, 36, 32, 18), (2, 2, 1, 1), (64, 36, 48, 27), (65, 37, 21, 9), (33, 17, 64, 40), (1, 9, 3, 3), (9, 1, 4, 1), (128, 2, 64, 2)] {
            let src: Vec<u8> = (0..sw * sh * 4)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    (seed >> 24) as u8
                })
                .collect();
            s.scale_rgba(&src, sw, sh, &mut out, dw, dh).unwrap();
            assert_eq!(out, bilinear(&src, sw, sh, dw, dh), "{sw}x{sh} → {dw}x{dh}");
        }
        // Tout blanc : aucun débordement des lignes mêlées sur 16 bits.
        s.scale_rgba(&[255; 7 * 5 * 4], 7, 5, &mut out, 4, 3).unwrap();
        assert!(out.iter().all(|&b| b == 255));
    }

    /// I420 → RGBA par le calcul exact en nombres à virgule, pixel par pixel.
    fn exact(src: I420<'_>, w: usize, h: usize) -> Vec<f64> {
        let cw = src.width.div_ceil(2);
        let mut out = Vec::with_capacity(w * h * 3);
        for oy in 0..h {
            let sy = oy * src.height / h;
            for ox in 0..w {
                let sx = ox * src.width / w;
                let c = 1.164 * (f64::from(src.y[sy * src.width + sx]) - 16.0);
                let d = f64::from(src.u[(sy / 2) * cw + sx / 2]) - 128.0;
                let e = f64::from(src.v[(sy / 2) * cw + sx / 2]) - 128.0;
                out.extend([c + 1.596 * e, c - 0.391 * d - 0.813 * e, c + 2.018 * d].map(|x| x.clamp(0.0, 255.0)));
            }
        }
        out
    }

    #[test]
    fn i420_a_la_taille_affichee() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut noise = |len: usize| -> Vec<u8> {
            (0..len)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    (seed >> 24) as u8
                })
                .collect()
        };
        let mut out = Vec::new();
        // Même taille (paire, impaire, plus large qu'un registre), réduite, agrandie.
        for (sw, sh, w, h) in [(64usize, 36usize, 64usize, 36usize), (67, 5, 67, 5), (1, 1, 1, 1), (3, 3, 3, 3), (64, 36, 43, 24), (67, 37, 20, 11), (16, 10, 40, 25), (130, 70, 1, 1)] {
            let (cw, ch) = (sw.div_ceil(2), sh.div_ceil(2));
            // Toute la plage, couleurs hors gamme comprises : le calcul sur 16 bits ne doit jamais déborder.
            let (y, u, v) = (noise(sw * sh), noise(cw * ch), noise(cw * ch));
            let src = I420 { width: sw, height: sh, y: &y, u: &u, v: &v };
            i420_to_rgba_sized(src, w, h, &mut out).unwrap();
            assert_eq!(out.len(), w * h * 4);
            for (i, (got, want)) in out.as_chunks::<4>().0.iter().zip(exact(src, w, h).as_chunks::<3>().0).enumerate() {
                assert_eq!(got[3], 255);
                for k in 0..3 {
                    // Virgule fixe ×64 : un niveau et demi d'écart au plus avec le calcul exact.
                    assert!((f64::from(got[k]) - want[k]).abs() <= 1.6, "{sw}x{sh} → {w}x{h}, pixel {i}, canal {k} : {} au lieu de {:.2}", got[k], want[k]);
                }
            }
        }
        // Extrêmes : rien ne déborde.
        for (yy, uu, vv) in [(255u8, 255u8, 255u8), (255, 0, 0), (0, 255, 255), (0, 0, 0), (255, 255, 0), (0, 0, 255)] {
            let (y, u, v) = (vec![yy; 40 * 2], vec![uu; 20], vec![vv; 20]);
            let src = I420 { width: 40, height: 2, y: &y, u: &u, v: &v };
            i420_to_rgba_sized(src, 40, 2, &mut out).unwrap();
            for (got, want) in out.as_chunks::<4>().0.iter().zip(exact(src, 40, 2).as_chunks::<3>().0) {
                for k in 0..3 {
                    assert!((f64::from(got[k]) - want[k]).abs() <= 1.6, "Y {yy} U {uu} V {vv} : {got:?} au lieu de {want:?}");
                }
            }
        }
        // Plans plus longs qu'annoncé : acceptés ; trop courts ou taille nulle : refusés.
        let (y, u, v) = (vec![100; 20], vec![128; 6], vec![128; 6]);
        assert!(i420_to_rgba_sized(I420 { width: 4, height: 4, y: &y, u: &u, v: &v }, 2, 2, &mut out).is_ok());
        assert_eq!(i420_to_rgba_sized(I420 { width: 4, height: 4, y: &y[..15], u: &u, v: &v }, 2, 2, &mut out), Err(BadSize));
        assert_eq!(i420_to_rgba_sized(I420 { width: 4, height: 4, y: &y, u: &u[..3], v: &v }, 2, 2, &mut out), Err(BadSize));
        assert_eq!(i420_to_rgba_sized(I420 { width: 4, height: 4, y: &y, u: &u, v: &v }, 0, 2, &mut out), Err(BadSize));
        assert_eq!(i420_to_rgba_sized(I420 { width: 0, height: 0, y: &[], u: &[], v: &[] }, 2, 2, &mut out), Err(BadSize));
    }

    #[test]
    fn bgra_a_la_taille_affichee() {
        // 3 × 2, chaque pixel porte son rang dans le bleu.
        let src: Vec<u8> = (0..6u8).flat_map(|i| [i, 100, 200, 7]).collect();
        let mut out = Vec::new();
        bgra_to_rgba_sized(3, 2, &src, 3, 2, &mut out).unwrap();
        assert_eq!(out, (0..6u8).flat_map(|i| [200, 100, i, 255]).collect::<Vec<_>>());
        // Plus proche voisin : colonnes 0 et 1, ligne 0 ; puis agrandi.
        bgra_to_rgba_sized(3, 2, &src, 2, 1, &mut out).unwrap();
        assert_eq!(out, [200, 100, 0, 255, 200, 100, 1, 255]);
        bgra_to_rgba_sized(3, 2, &src, 6, 2, &mut out).unwrap();
        assert_eq!(out.as_chunks::<4>().0.iter().map(|p| p[2]).collect::<Vec<_>>(), [0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5]);
        // Deux tailles de suite sur le même fil : la table des colonnes suit.
        bgra_to_rgba_sized(3, 2, &src, 1, 2, &mut out).unwrap();
        assert_eq!(out, [200, 100, 0, 255, 200, 100, 3, 255]);
        assert_eq!(bgra_to_rgba_sized(3, 2, &src[..23], 3, 2, &mut out), Err(BadSize));
        assert_eq!(bgra_to_rgba_sized(3, 2, &src, 3, 0, &mut out), Err(BadSize));
        // Image vide : rien, sans erreur (comme avant).
        assert_eq!(bgra_to_rgba(0, 0, &[], &mut out), Ok(()));
        assert!(out.is_empty());
        assert_eq!(i420_to_rgba(I420 { width: 0, height: 0, y: &[], u: &[], v: &[] }, &mut out), Ok(()));
        assert!(out.is_empty());
    }

    #[test]
    fn proportions() {
        assert_eq!(fit_within(1920, 1080, 640, 640), (640, 360));
        assert_eq!(fit_within(100, 50, 640, 640), (100, 50));
        assert_eq!(fit_within(10000, 1, 100, 100), (100, 1));
    }

    #[test]
    fn tampons_reutilises_sans_reallocation() {
        let (w, h) = (64, 48);
        let bgra = vec![50u8; w * h * 4];
        let (mut y, mut u, mut v, mut rgba, mut small) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut s = Scaler::new();
        let mut run = |y: &mut Vec<u8>, u: &mut Vec<u8>, v: &mut Vec<u8>, rgba: &mut Vec<u8>, small: &mut Vec<u8>| {
            bgra_to_i420(w, h, &bgra, y, u, v).unwrap();
            i420_to_rgba(I420 { width: w, height: h, y, u, v }, rgba).unwrap();
            s.scale_rgba(rgba, w, h, small, 32, 24).unwrap();
        };
        run(&mut y, &mut u, &mut v, &mut rgba, &mut small);
        let ptrs = [y.as_ptr(), u.as_ptr(), v.as_ptr(), rgba.as_ptr(), small.as_ptr()];
        for _ in 0..3 {
            run(&mut y, &mut u, &mut v, &mut rgba, &mut small);
        }
        assert_eq!(ptrs, [y.as_ptr(), u.as_ptr(), v.as_ptr(), rgba.as_ptr(), small.as_ptr()]);
    }

    /// Mesure indicative (affichée avec `--nocapture`) ; le seuil n'est vérifié
    /// qu'en mode optimisé, le mode de débogage étant 10 à 30 fois plus lent.
    #[test]
    fn bench_1080p() {
        let (w, h) = (1920, 1080);
        let bgra: Vec<u8> = (0..w * h * 4).map(|i| (i * 7 % 251) as u8).collect();
        let (mut y, mut u, mut v, mut rgba, mut small) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut s = Scaler::new();
        let mut best = [f64::MAX; 4];
        for _ in 0..3 {
            let t = Instant::now();
            bgra_to_i420(w, h, &bgra, &mut y, &mut u, &mut v).unwrap();
            best[0] = best[0].min(t.elapsed().as_secs_f64() * 1e3);
            let t = Instant::now();
            i420_to_rgba(I420 { width: w, height: h, y: &y, u: &u, v: &v }, &mut rgba).unwrap();
            best[1] = best[1].min(t.elapsed().as_secs_f64() * 1e3);
            let t = Instant::now();
            bgra_to_rgba(w, h, &bgra, &mut rgba).unwrap();
            best[2] = best[2].min(t.elapsed().as_secs_f64() * 1e3);
            let t = Instant::now();
            s.scale_rgba(&rgba, w, h, &mut small, 640, 360).unwrap();
            best[3] = best[3].min(t.elapsed().as_secs_f64() * 1e3);
        }
        eprintln!(
            "1080p : BGRA→I420 {:.2} ms, I420→RGBA {:.2} ms, BGRA→RGBA {:.2} ms, RGBA 1080p→360p {:.2} ms",
            best[0], best[1], best[2], best[3]
        );
        if !cfg!(debug_assertions) {
            assert!(best[1] < 16.0, "I420→RGBA trop lent : {:.2} ms", best[1]);
        }
    }
}

#[cfg(test)]
mod direct_tests {
    use super::*;

    fn noise(len: usize) -> Vec<u8> {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        (0..len)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                (seed >> 24) as u8
            })
            .collect()
    }

    /// Référence : la boucle générale, bloc par bloc, sans chemin rapide.
    fn slow(width: usize, height: usize, src: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let (cw, ch) = (width.div_ceil(2), height.div_ceil(2));
        let y = src.as_chunks::<4>().0.iter().map(|p| ((66 * i32::from(p[2]) + 129 * i32::from(p[1]) + 25 * i32::from(p[0]) + 128) >> 8).wrapping_add(16) as u8).collect();
        let (mut u, mut v) = (vec![0; cw * ch], vec![0; cw * ch]);
        for cy in 0..ch {
            for cx in 0..cw {
                let (mut sb, mut sg, mut sr) = (0i32, 0i32, 0i32);
                for (row, col) in [(cy * 2, cx * 2), (cy * 2, (cx * 2 + 1).min(width - 1)), ((cy * 2 + 1).min(height - 1), cx * 2), ((cy * 2 + 1).min(height - 1), (cx * 2 + 1).min(width - 1))] {
                    let p = &src[(row * width + col) * 4..];
                    sb += i32::from(p[0]);
                    sg += i32::from(p[1]);
                    sr += i32::from(p[2]);
                }
                u[cy * cw + cx] = clamp8(((-38 * sr - 74 * sg + 112 * sb + 512) >> 10) + 128);
                v[cy * cw + cx] = clamp8(((112 * sr - 94 * sg - 18 * sb + 512) >> 10) + 128);
            }
        }
        (y, u, v)
    }

    #[test]
    fn even_fast_path_matches() {
        for (w, h) in [(64usize, 36usize), (2, 2), (130, 6), (7, 5), (8, 3)] {
            let src = noise(w * h * 4);
            let (mut y, mut u, mut v) = (Vec::new(), Vec::new(), Vec::new());
            bgra_to_i420(w, h, &src, &mut y, &mut u, &mut v).unwrap();
            assert_eq!((y, u, v), slow(w, h, &src), "{w}x{h}");
        }
    }

    #[test]
    fn nv12_written_directly_matches_the_planes() {
        // Source paire, puis impaire (dernière colonne et dernière ligne rognées).
        for (w, h) in [(64usize, 36usize), (65, 37), (2, 2)] {
            let (ew, eh) = (w & !1, h & !1);
            let src = noise(w * h * 4);
            let (y, u, v) = slow(w, h, &src);
            let mut dst = vec![0u8; ew * eh * 3 / 2];
            bgra_to_nv12(w, &src, ew, eh, &mut dst).unwrap();
            let cw = w.div_ceil(2);
            for r in 0..eh {
                assert_eq!(dst[r * ew..(r + 1) * ew], y[r * w..r * w + ew], "luminance, ligne {r} ({w}x{h})");
            }
            for r in 0..eh / 2 {
                for c in 0..ew / 2 {
                    assert_eq!([dst[ew * eh + r * ew + 2 * c], dst[ew * eh + r * ew + 2 * c + 1]], [u[r * cw + c], v[r * cw + c]], "chrominance {c},{r} ({w}x{h})");
                }
            }
        }
        // Tampon de la mauvaise taille, côtés impairs ou plus grands que la source : refusés.
        let src = noise(8 * 8 * 4);
        assert!(bgra_to_nv12(8, &src, 8, 8, &mut [0; 10]).is_err());
        assert!(bgra_to_nv12(8, &src, 7, 8, &mut [0; 84]).is_err());
        assert!(bgra_to_nv12(8, &src, 10, 8, &mut [0; 120]).is_err());
        assert!(bgra_to_nv12(8, &src, 8, 10, &mut [0; 120]).is_err());
    }
}
