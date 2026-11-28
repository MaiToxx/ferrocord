//! Conversions de pixels de la capture, en Rust sûr : NV12 (caméras) vers
//! I420, et réduction d'une image BGRA dont les lignes ont un pas quelconque
//! (textures D3D11, images de caméra) vers une image BGRA serrée.

/// Un plan dans un tampon : début et pas (octets par ligne).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plane {
    pub offset: usize,
    pub stride: usize,
}

/// NV12 (plan Y, puis plan UV entrelacé à demi-définition) vers I420.
/// `None` si le tampon est trop court pour les plans annoncés.
pub fn nv12_to_i420(src: &[u8], width: u32, height: u32, y_plane: Plane, uv_plane: Plane) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    let (w, h) = (width as usize, height as usize);
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    if w == 0 || h == 0 || y_plane.stride < w || uv_plane.stride < 2 * cw {
        return None;
    }
    let row = |p: Plane, r: usize, len: usize| src.get(p.offset + r * p.stride..p.offset + r * p.stride + len);
    let mut y = Vec::with_capacity(w * h);
    for r in 0..h {
        y.extend_from_slice(row(y_plane, r, w)?);
    }
    let mut u = Vec::with_capacity(cw * ch);
    let mut v = Vec::with_capacity(cw * ch);
    for r in 0..ch {
        for pair in row(uv_plane, r, 2 * cw)?.as_chunks::<2>().0 {
            u.push(pair[0]);
            v.push(pair[1]);
        }
    }
    Some((y, u, v))
}

/// Bornes [début, fin) des blocs de source, au moins un pixel chacun.
fn spans(s: usize, d: usize) -> Vec<(usize, usize)> {
    (0..d)
        .map(|i| {
            let a = i * s / d;
            let b = ((i + 1) * s / d).max(a + 1).min(s);
            (a.min(s - 1), b)
        })
        .collect()
}

/// Moitié exacte (un écran 4K partagé en 1080p) : chaque pixel est la moyenne
/// arrondie d'un bloc 2 × 2, par tranches de huit octets sur deux lignes.
fn halve(src: &[u8], sw: usize, stride: usize, out: &mut [u8]) {
    for (line, rows) in out.chunks_exact_mut(2 * sw).zip(src.chunks(2 * stride)) {
        let (top, bottom) = (&rows[..4 * sw], &rows[stride..stride + 4 * sw]);
        for ((d, p), q) in line.as_chunks_mut::<4>().0.iter_mut().zip(top.as_chunks::<8>().0).zip(bottom.as_chunks::<8>().0) {
            for c in 0..4 {
                d[c] = ((u16::from(p[c]) + u16::from(p[c + 4]) + u16::from(q[c]) + u16::from(q[c + 4]) + 2) >> 2) as u8;
            }
        }
    }
}

/// Plus haut bloc que [`average`] sait sommer sur 16 bits (256 × 255 < 65 536).
const TALLEST: usize = 256;
/// Plus grand bloc dont la division par multiplication est exacte : la somme
/// arrondie vaut moins de 256 n, et 256 n² ne doit pas dépasser 2³².
const LARGEST: usize = 4096;

/// Moyenne par blocs en deux temps : les lignes d'un bloc sont sommées
/// colonne par colonne (boucle sans indice, que le compilateur traite par
/// 16 ou 32 octets), puis chaque bloc additionne ses colonnes et divise par
/// une multiplication. Mêmes valeurs que [`average_any`].
fn average(src: &[u8], sw: usize, stride: usize, xs: &[(usize, usize)], ys: &[(usize, usize)], widest: usize, out: &mut [u8]) {
    let mut col = vec![0u16; 4 * sw];
    // Inverses de `lignes × colonnes`, refaits quand la hauteur du bloc change.
    let mut inv = vec![0u64; widest + 1];
    let mut known = 0;
    for (&(y0, y1), dst) in ys.iter().zip(out.chunks_exact_mut(4 * xs.len())) {
        for (c, &s) in col.iter_mut().zip(&src[y0 * stride..y0 * stride + 4 * sw]) {
            *c = u16::from(s);
        }
        for r in y0 + 1..y1 {
            for (c, &s) in col.iter_mut().zip(&src[r * stride..r * stride + 4 * sw]) {
                *c += u16::from(s);
            }
        }
        let rows = y1 - y0;
        if rows != known {
            for (k, v) in inv.iter_mut().enumerate().skip(1) {
                *v = (1u64 << 32).div_ceil((rows * k) as u64);
            }
            known = rows;
        }
        let px = col.as_chunks::<4>().0;
        for (d, &(x0, x1)) in dst.as_chunks_mut::<4>().0.iter_mut().zip(xs) {
            let span = &px[x0..x1];
            let mut a = [0u32; 4];
            for p in span {
                for c in 0..4 {
                    a[c] += u32::from(p[c]);
                }
            }
            let (n, m) = ((rows * span.len()) as u64, inv[span.len()]);
            for c in 0..4 {
                d[c] = (((u64::from(a[c]) + n / 2) * m) >> 32) as u8;
            }
        }
    }
}

/// Blocs d'un ou deux pixels de côté (réduction de moins de moitié : un écran
/// 1440p partagé en 1080p, un 4K en 1440p). Un bloc d'une seule colonne
/// compte la sienne deux fois : toute la ligne se divise alors par le même
/// nombre, 2 ou 4, donc par un décalage, et les quatre canaux d'un pixel se
/// calculent d'un coup dans un mot de 64 bits (quatre cases de 16 bits, dont
/// la somme ne dépasse pas 1 022). Mêmes valeurs que [`average_any`].
fn average_small(src: &[u8], sw: usize, stride: usize, xs: &[(usize, usize)], ys: &[(usize, usize)], out: &mut [u8]) {
    let word = |p: [u16; 4]| u64::from(p[0]) | u64::from(p[1]) << 16 | u64::from(p[2]) << 32 | u64::from(p[3]) << 48;
    let mut col = vec![0u16; 4 * sw];
    let pairs: Vec<(u32, u32)> = xs.iter().map(|&(x0, x1)| (x0 as u32, (x1 - 1) as u32)).collect();
    for (&(y0, y1), dst) in ys.iter().zip(out.chunks_exact_mut(4 * xs.len())) {
        for (c, &s) in col.iter_mut().zip(&src[y0 * stride..y0 * stride + 4 * sw]) {
            *c = u16::from(s);
        }
        for r in y0 + 1..y1 {
            for (c, &s) in col.iter_mut().zip(&src[r * stride..r * stride + 4 * sw]) {
                *c += u16::from(s);
            }
        }
        // Une ou deux lignes, deux colonnes comptées : division par 2 ou 4, arrondie.
        let rows = y1 - y0;
        let (round, shift) = (rows as u64 * 0x0001_0001_0001_0001, rows as u32);
        let px = col.as_chunks::<4>().0;
        for (d, &(a, b)) in dst.as_chunks_mut::<4>().0.iter_mut().zip(&pairs) {
            let v = (word(px[a as usize]) + word(px[b as usize]) + round) >> shift;
            *d = [v as u8, (v >> 16) as u8, (v >> 32) as u8, (v >> 48) as u8];
        }
    }
}

/// Moyenne par blocs de n'importe quelle taille (une division par canal).
fn average_any(src: &[u8], sw: usize, stride: usize, xs: &[(usize, usize)], ys: &[(usize, usize)], out: &mut [u8]) {
    let mut acc = vec![[0u32; 4]; xs.len()];
    for (&(y0, y1), dst) in ys.iter().zip(out.chunks_exact_mut(4 * xs.len())) {
        acc.iter_mut().for_each(|a| *a = [0; 4]);
        for r in y0..y1 {
            let line = &src[r * stride..r * stride + 4 * sw];
            for (a, &(x0, x1)) in acc.iter_mut().zip(xs) {
                for px in line[4 * x0..4 * x1].as_chunks::<4>().0 {
                    a[0] += u32::from(px[0]);
                    a[1] += u32::from(px[1]);
                    a[2] += u32::from(px[2]);
                    a[3] += u32::from(px[3]);
                }
            }
        }
        let rows = (y1 - y0) as u32;
        for ((px, a), &(x0, x1)) in dst.as_chunks_mut::<4>().0.iter_mut().zip(&acc).zip(xs) {
            let n = rows * (x1 - x0) as u32;
            for (p, sum) in px.iter_mut().zip(a) {
                *p = ((sum + n / 2) / n) as u8;
            }
        }
    }
}

/// Réduit (ou recopie) une image BGRA de `sw` × `sh` dont les lignes sont
/// espacées de `stride` octets vers `dw` × `dh`, lignes serrées, dans `out`.
/// Chaque pixel de sortie est la moyenne du bloc de source qu'il couvre
/// (pas de crénelage sur le texte d'un écran réduit). `false` si la source
/// est trop courte.
///
/// Appelée pour chaque image d'un écran partagé plus grand que la qualité
/// choisie. Mesuré, un écran 4K : vers le 1080p 13,6 ms → 1,9 ms, vers le
/// 1440p 21,8 ms → 5,8 ms, vers le 720p 7,5 ms → 3,8 ms (la première version
/// calculait quatre divisions par pixel de sortie), aux mêmes valeurs.
pub fn scale_bgra(src: &[u8], sw: u32, sh: u32, stride: usize, dw: u32, dh: u32, out: &mut Vec<u8>) -> bool {
    let (sw, sh, dw, dh) = (sw as usize, sh as usize, dw as usize, dh as usize);
    out.clear();
    if sw == 0 || sh == 0 || dw == 0 || dh == 0 || stride < 4 * sw || src.len() < stride * (sh - 1) + 4 * sw {
        return false;
    }
    if sw == dw && sh == dh {
        for r in 0..sh {
            out.extend_from_slice(&src[r * stride..r * stride + 4 * sw]);
        }
        return true;
    }
    out.resize(4 * dw * dh, 0);
    if sw == 2 * dw && sh == 2 * dh {
        halve(src, sw, stride, out);
        return true;
    }
    let (xs, ys) = (spans(sw, dw), spans(sh, dh));
    let widest = xs.iter().map(|(a, b)| b - a).max().unwrap_or(1);
    let tallest = ys.iter().map(|(a, b)| b - a).max().unwrap_or(1);
    if widest <= 2 && tallest <= 2 {
        average_small(src, sw, stride, &xs, &ys, out);
    } else if tallest <= TALLEST && widest * tallest <= LARGEST {
        average(src, sw, stride, &xs, &ys, widest, out);
    } else {
        average_any(src, sw, stride, &xs, &ys, out);
    }
    true
}

/// BGRA serré vers RGBA serré (textures egui des vignettes), sur place.
pub fn bgra_to_rgba_in_place(px: &mut [u8]) {
    for p in px.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nv12_with_padding_to_i420() {
        // 4 × 2, pas de 6 octets : Y puis UV (2 couples sur une ligne).
        let mut src = vec![0xEE; 6 * 2 + 6];
        src[0..4].copy_from_slice(&[1, 2, 3, 4]);
        src[6..10].copy_from_slice(&[5, 6, 7, 8]);
        src[12..16].copy_from_slice(&[10, 20, 11, 21]);
        let (y, u, v) = nv12_to_i420(&src, 4, 2, Plane { offset: 0, stride: 6 }, Plane { offset: 12, stride: 6 }).unwrap();
        assert_eq!(y, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(u, [10, 11]);
        assert_eq!(v, [20, 21]);
    }

    #[test]
    fn nv12_odd_size_rounds_chroma_up() {
        // 3 × 3 : chrominance 2 × 2.
        let y_plane = Plane { offset: 0, stride: 4 };
        let uv_plane = Plane { offset: 12, stride: 4 };
        let mut src = vec![0u8; 12 + 8];
        src[12..20].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let (y, u, v) = nv12_to_i420(&src, 3, 3, y_plane, uv_plane).unwrap();
        assert_eq!(y.len(), 9);
        assert_eq!((u, v), (vec![1, 3, 5, 7], vec![2, 4, 6, 8]));
    }

    #[test]
    fn nv12_short_buffer_is_refused() {
        let src = vec![0u8; 10];
        assert_eq!(nv12_to_i420(&src, 4, 2, Plane { offset: 0, stride: 4 }, Plane { offset: 8, stride: 4 }), None);
        assert_eq!(nv12_to_i420(&src, 4, 2, Plane { offset: 0, stride: 2 }, Plane { offset: 8, stride: 4 }), None, "pas plus court que la ligne");
    }

    #[test]
    fn scale_averages_blocks_and_drops_padding() {
        // 4 × 2 avec 8 octets de bourrage par ligne, réduit à 2 × 1.
        let stride = 4 * 4 + 8;
        let mut src = vec![0xAA; stride * 2];
        for r in 0..2 {
            for x in 0..4 {
                let v = (r * 4 + x) as u8 * 10;
                src[r * stride + 4 * x..r * stride + 4 * x + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        let mut out = Vec::new();
        assert!(scale_bgra(&src, 4, 2, stride, 2, 1, &mut out));
        // Bloc gauche : 0, 10, 40, 50 → 25 ; droit : 20, 30, 60, 70 → 45.
        assert_eq!(out, [25, 25, 25, 255, 45, 45, 45, 255]);
        assert!(scale_bgra(&src, 4, 2, stride, 4, 2, &mut out));
        assert_eq!(out.len(), 32, "même taille : recopie sans bourrage");
        assert_eq!(&out[16..20], &[40, 40, 40, 255]);
        assert!(!scale_bgra(&src[..stride], 4, 2, stride, 2, 1, &mut out), "source trop courte");
    }

    #[test]
    fn scale_non_integer_ratio_covers_everything() {
        let (sw, sh) = (7u32, 5u32);
        let src: Vec<u8> = (0..sw * sh).flat_map(|_| [100u8, 150, 200, 255]).collect();
        let mut out = Vec::new();
        assert!(scale_bgra(&src, sw, sh, 4 * sw as usize, 3, 2, &mut out));
        assert!(out.as_chunks::<4>().0.iter().all(|p| *p == [100, 150, 200, 255]), "couleur unie conservée");
    }

    /// La moyenne de chaque bloc, écrite sans détour.
    fn plain(src: &[u8], sw: usize, sh: usize, stride: usize, dw: usize, dh: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 * dw * dh);
        for oy in 0..dh {
            let y0 = oy * sh / dh;
            let y1 = ((oy + 1) * sh / dh).max(y0 + 1).min(sh);
            for ox in 0..dw {
                let x0 = ox * sw / dw;
                let x1 = ((ox + 1) * sw / dw).max(x0 + 1).min(sw);
                let n = ((y1 - y0) * (x1 - x0)) as u64;
                for c in 0..4 {
                    let sum: u64 = (y0..y1).flat_map(|r| (x0..x1).map(move |x| (r, x))).map(|(r, x)| u64::from(src[r * stride + 4 * x + c])).sum();
                    out.push(((sum + n / 2) / n) as u8);
                }
            }
        }
        out
    }

    #[test]
    fn every_path_gives_the_plain_average() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut out = Vec::new();
        // Moitié exacte ; rapports quelconques ; agrandissement ; plus grand
        // bloc du chemin rapide (64 × 64, puis 256 de haut) ; au-delà (bloc
        // de 300 de haut, bloc de 130 × 130) : calcul d'origine.
        let sizes = [(64, 36, 32, 18), (38, 22, 19, 11), (64, 36, 48, 27), (65, 37, 21, 9), (100, 60, 33, 20), (7, 5, 3, 2), (5, 3, 5, 2), (16, 16, 40, 40), (64, 64, 1, 1), (9, 512, 3, 2), (3, 600, 2, 2), (130, 130, 1, 1)];
        for (sw, sh, dw, dh) in sizes {
            // Du bruit, puis tout à 255 (les sommes les plus grandes), avec 12 octets de bourrage par ligne.
            for white in [false, true] {
                let stride = 4 * sw + 12;
                let src: Vec<u8> = (0..stride * sh)
                    .map(|_| {
                        seed ^= seed << 13;
                        seed ^= seed >> 7;
                        seed ^= seed << 17;
                        if white { 255 } else { (seed >> 24) as u8 }
                    })
                    .collect();
                // La dernière ligne s'arrête à son dernier pixel, comme une texture lue.
                let src = &src[..stride * (sh - 1) + 4 * sw];
                assert!(scale_bgra(src, sw as u32, sh as u32, stride, dw as u32, dh as u32, &mut out));
                assert_eq!(out, plain(src, sw, sh, stride, dw, dh), "{sw}x{sh} → {dw}x{dh}, blanc : {white}");
            }
        }
    }

    #[test]
    fn swap_channels() {
        let mut px = [1, 2, 3, 4, 5, 6, 7, 8];
        bgra_to_rgba_in_place(&mut px);
        assert_eq!(px, [3, 2, 1, 4, 7, 6, 5, 8]);
    }
}
