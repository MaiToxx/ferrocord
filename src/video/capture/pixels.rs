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

/// Réduit (ou recopie) une image BGRA de `sw` × `sh` dont les lignes sont
/// espacées de `stride` octets vers `dw` × `dh`, lignes serrées, dans `out`.
/// Chaque pixel de sortie est la moyenne du bloc de source qu'il couvre
/// (pas de crénelage sur le texte d'un écran réduit). `false` si la source
/// est trop courte.
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
    // Bornes [début, fin) des blocs de source, au moins un pixel chacun.
    let spans = |s: usize, d: usize| -> Vec<(usize, usize)> {
        (0..d)
            .map(|i| {
                let a = i * s / d;
                let b = ((i + 1) * s / d).max(a + 1).min(s);
                (a.min(s - 1), b)
            })
            .collect()
    };
    let (xs, ys) = (spans(sw, dw), spans(sh, dh));
    let mut acc = vec![[0u32; 4]; dw];
    for (oy, &(y0, y1)) in ys.iter().enumerate() {
        acc.iter_mut().for_each(|a| *a = [0; 4]);
        for r in y0..y1 {
            let line = &src[r * stride..r * stride + 4 * sw];
            for (a, &(x0, x1)) in acc.iter_mut().zip(&xs) {
                for px in line[4 * x0..4 * x1].as_chunks::<4>().0 {
                    a[0] += u32::from(px[0]);
                    a[1] += u32::from(px[1]);
                    a[2] += u32::from(px[2]);
                    a[3] += u32::from(px[3]);
                }
            }
        }
        let rows = (y1 - y0) as u32;
        let dst = &mut out[4 * dw * oy..4 * dw * (oy + 1)];
        for ((px, a), &(x0, x1)) in dst.as_chunks_mut::<4>().0.iter_mut().zip(&acc).zip(&xs) {
            let n = rows * (x1 - x0) as u32;
            for (p, sum) in px.iter_mut().zip(a) {
                *p = ((sum + n / 2) / n) as u8;
            }
        }
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

    #[test]
    fn swap_channels() {
        let mut px = [1, 2, 3, 4, 5, 6, 7, 8];
        bgra_to_rgba_in_place(&mut px);
        assert_eq!(px, [3, 2, 1, 4, 7, 6, 5, 8]);
    }
}
