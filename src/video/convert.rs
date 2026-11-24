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
    // `resize` ne réalloue que si la capacité manque ; le contenu sera écrasé.
    buf.resize(len, 0);
}

#[inline(always)]
fn clamp8(x: i32) -> u8 {
    x.clamp(0, 255) as u8
}

// Coefficients BT.601 en virgule fixe (×64) : 1.164, 1.596, 0.391, 0.813, 2.018.
#[inline(always)]
fn chroma(u: u8, v: u8) -> (i32, i32, i32) {
    let (d, e) = (i32::from(u) - 128, i32::from(v) - 128);
    (102 * e + 32, -25 * d - 52 * e + 32, 129 * d + 32)
}

#[inline(always)]
fn put(px: &mut [u8], y: u8, r: i32, g: i32, b: i32) {
    let c = (i32::from(y) - 16) * 75;
    if let [pr, pg, pb, pa] = px {
        *pr = clamp8((c + r) >> 6);
        *pg = clamp8((c + g) >> 6);
        *pb = clamp8((c + b) >> 6);
        *pa = 255;
    }
}

/// I420 → RGBA (alpha opaque).
pub fn i420_to_rgba(src: I420<'_>, out: &mut Vec<u8>) -> Result<(), BadSize> {
    src.check()?;
    let (w, h) = (src.width, src.height);
    fit(out, w * h * 4);
    if w == 0 || h == 0 {
        return Ok(());
    }
    let cw = w.div_ceil(2);
    for (row, line) in out.chunks_exact_mut(w * 4).enumerate() {
        let ys = &src.y[row * w..row * w + w];
        let us = &src.u[(row / 2) * cw..(row / 2) * cw + cw];
        let vs = &src.v[(row / 2) * cw..(row / 2) * cw + cw];
        // Par paires de pixels qui partagent leur chrominance : pas d'indice
        // calculé ni de contrôle de bornes dans la boucle.
        let (pairs, rest) = line.as_chunks_mut::<8>();
        let (ypairs, yrest) = ys.as_chunks::<2>();
        for (((px, yy), &uu), &vv) in pairs.iter_mut().zip(ypairs).zip(us).zip(vs) {
            let (r, g, b) = chroma(uu, vv);
            put(&mut px[..4], yy[0], r, g, b);
            put(&mut px[4..], yy[1], r, g, b);
        }
        if let ([y0], Some(&uu), Some(&vv)) = (yrest, us.last(), vs.last()) {
            let (r, g, b) = chroma(uu, vv);
            put(rest, *y0, r, g, b);
        }
    }
    Ok(())
}

/// BGRA → RGBA (échange rouge/bleu, alpha forcé opaque : les captures
/// d'écran laissent souvent un alpha à 0).
pub fn bgra_to_rgba(width: usize, height: usize, src: &[u8], out: &mut Vec<u8>) -> Result<(), BadSize> {
    if src.len() != width * height * 4 {
        return Err(BadSize);
    }
    fit(out, src.len());
    for (d, s) in out.as_chunks_mut::<4>().0.iter_mut().zip(src.as_chunks::<4>().0) {
        *d = [s[2], s[1], s[0], 255];
    }
    Ok(())
}

/// N'importe quelle [`Frame`] → RGBA, pour une texture.
pub fn frame_to_rgba(f: &Frame, out: &mut Vec<u8>) -> Result<(), BadSize> {
    let (w, h) = (f.width as usize, f.height as usize);
    match &f.data {
        FrameData::I420 { y, u, v } => i420_to_rgba(I420 { width: w, height: h, y, u, v }, out),
        FrameData::Bgra(b) => bgra_to_rgba(w, h, b, out),
    }
}

#[inline(always)]
fn luma(b: u8, g: u8, r: u8) -> u8 {
    // BT.601, plage limitée, virgule fixe ×256 : 0.257, 0.504, 0.098.
    // Somme au plus 56 228 : tient sur 16 bits, que le compilateur vectorise par 16 ou 32 pixels.
    (((66 * u16::from(r) + 129 * u16::from(g) + 25 * u16::from(b) + 128) >> 8) + 16) as u8
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
    let stride = width * 4;
    for (dst, line) in y.chunks_exact_mut(width.max(1)).zip(src.chunks_exact(stride.max(1))) {
        for (d, p) in dst.iter_mut().zip(line.as_chunks::<4>().0) {
            *d = luma(p[0], p[1], p[2]);
        }
    }
    // Côtés pairs (toute image envoyée à un encodeur) : les blocs 2×2 sont
    // lus par tranches de huit octets sur deux lignes, sans calcul d'indice
    // ni test de bord, ce que le compilateur vectorise. Mêmes formules, mêmes
    // valeurs que la boucle générale (test `even_fast_path_matches`).
    // Mesuré en 4K : 9,0 ms par image avant, 7,7 ms après.
    if width.is_multiple_of(2) && height.is_multiple_of(2) && width > 0 {
        for ((rows, du), dv) in src.chunks_exact(stride * 2).zip(u.chunks_exact_mut(cw)).zip(v.chunks_exact_mut(cw)) {
            let (top, bottom) = rows.split_at(stride);
            for (((p, q), du), dv) in top.as_chunks::<8>().0.iter().zip(bottom.as_chunks::<8>().0).zip(du.iter_mut()).zip(dv.iter_mut()) {
                let sb = i32::from(u16::from(p[0]) + u16::from(p[4]) + u16::from(q[0]) + u16::from(q[4]));
                let sg = i32::from(u16::from(p[1]) + u16::from(p[5]) + u16::from(q[1]) + u16::from(q[5]));
                let sr = i32::from(u16::from(p[2]) + u16::from(p[6]) + u16::from(q[2]) + u16::from(q[6]));
                *du = clamp8(((-38 * sr - 74 * sg + 112 * sb + 512) >> 10) + 128);
                *dv = clamp8(((112 * sr - 94 * sg - 18 * sb + 512) >> 10) + 128);
            }
        }
        return Ok(());
    }
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
            // Somme de 4 pixels : les coefficients ×256 deviennent ×1024.
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
    let (dy, duv) = dst.split_at_mut(ew * eh);
    for (d, line) in dy.chunks_exact_mut(ew).zip(src.chunks_exact(stride)) {
        for (d, p) in d.iter_mut().zip(line[..ew * 4].as_chunks::<4>().0) {
            *d = luma(p[0], p[1], p[2]);
        }
    }
    for (rows, d) in src.chunks_exact(stride * 2).zip(duv.chunks_exact_mut(ew)) {
        let (top, bottom) = rows.split_at(stride);
        for ((p, q), d) in top[..ew * 4].as_chunks::<8>().0.iter().zip(bottom[..ew * 4].as_chunks::<8>().0).zip(d.as_chunks_mut::<2>().0) {
            let sb = i32::from(u16::from(p[0]) + u16::from(p[4]) + u16::from(q[0]) + u16::from(q[4]));
            let sg = i32::from(u16::from(p[1]) + u16::from(p[5]) + u16::from(q[1]) + u16::from(q[5]));
            let sr = i32::from(u16::from(p[2]) + u16::from(p[6]) + u16::from(q[2]) + u16::from(q[6]));
            *d = [clamp8(((-38 * sr - 74 * sg + 112 * sb + 512) >> 10) + 128), clamp8(((112 * sr - 94 * sg - 18 * sb + 512) >> 10) + 128)];
        }
    }
    Ok(())
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
        if (sw, sh) == (dw, dh) {
            out.copy_from_slice(src);
            return Ok(());
        }
        if self.key != (sw, sh, dw, dh) {
            table(sw, dw, &mut self.xs);
            table(sh, dh, &mut self.ys);
            self.key = (sw, sh, dw, dh);
        }
        let stride = sw * 4;
        for (line, &(y0, fy)) in out.chunks_exact_mut(dw * 4).zip(&self.ys) {
            let y0 = y0 as usize;
            let y1 = (y0 + 1).min(sh - 1);
            let (top, bottom) = (&src[y0 * stride..y0 * stride + stride], &src[y1 * stride..y1 * stride + stride]);
            for (px, &(x0, fx)) in line.as_chunks_mut::<4>().0.iter_mut().zip(&self.xs) {
                let x0 = x0 as usize;
                let x1 = (x0 + 1).min(sw - 1);
                for c in 0..4 {
                    let a = top[x0 * 4 + c] as u32;
                    let b = top[x1 * 4 + c] as u32;
                    let d = bottom[x0 * 4 + c] as u32;
                    let e = bottom[x1 * 4 + c] as u32;
                    let t = a * (256 - fx) + b * fx;
                    let m = d * (256 - fx) + e * fx;
                    px[c] = ((t * (256 - fy) + m * fy + (1 << 15)) >> 16) as u8;
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
