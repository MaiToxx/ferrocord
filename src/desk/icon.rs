//! Icône de Ferrocord dessinée au vol (aucun fichier embarqué) : un disque
//! de la couleur d'accent du thème, un anneau clair, et une pastille dans
//! le coin quand il y a des mentions non lues.

/// Côté de l'icône de la zone de notification, en pixels.
pub const TRAY_SIDE: u32 = 32;
/// Côté de l'icône des notifications (fichier PNG du registre).
pub const LOGO_SIDE: u32 = 64;

/// Pixels RGBA (`side` × `side`).
pub fn rgba(side: u32, accent: [u8; 3], badge: Option<[u8; 3]>) -> Vec<u8> {
    let s = side as f32;
    let c = s / 2.0;
    let r = s / 2.0 - 0.5;
    let ring = (r * 0.55, r * 0.72);
    let (bc, br) = (s * 0.76, s * 0.24);
    let mut out = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        for x in 0..side {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let d = ((px - c).powi(2) + (py - c).powi(2)).sqrt();
            // Bord adouci sur un pixel.
            let cover = |dist: f32, radius: f32| (radius - dist + 0.5).clamp(0.0, 1.0);
            let mut px_rgba = [accent[0], accent[1], accent[2], (cover(d, r) * 255.0) as u8];
            // Anneau : part de blanc adoucie sur un pixel à ses deux bords, comme le
            // bord du disque (sans cela il est en escalier aux grandes tailles
            // et irrégulier aux petites) ; mêmes rayons.
            let white = cover(d, ring.1) - cover(d, ring.0);
            if white > 0.0 {
                for c in &mut px_rgba[..3] {
                    *c = (f32::from(*c) + (255.0 - f32::from(*c)) * white).round() as u8;
                }
            }
            if let Some(b) = badge {
                let db = ((px - bc).powi(2) + (py - bc).powi(2)).sqrt();
                if db < br + 1.0 {
                    // Liseré blanc autour de la pastille.
                    let inner = cover(db, br - 1.0);
                    let mix = |a: u8, w: u8| (f32::from(a) * inner + f32::from(w) * (1.0 - inner)) as u8;
                    px_rgba = [mix(b[0], 255), mix(b[1], 255), mix(b[2], 255), px_rgba[3].max((cover(db, br + 0.5) * 255.0) as u8)];
                }
            }
            out.extend_from_slice(&px_rgba);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(px: &[u8], side: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * side + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    }

    #[test]
    fn disc_with_transparent_corners_and_badge() {
        let accent = [200, 80, 40];
        let plain = rgba(TRAY_SIDE, accent, None);
        assert_eq!(plain.len(), (TRAY_SIDE * TRAY_SIDE * 4) as usize);
        assert_eq!(at(&plain, TRAY_SIDE, 0, 0)[3], 0, "coin transparent");
        assert_eq!(at(&plain, TRAY_SIDE, 16, 16), [200, 80, 40, 255], "centre plein");
        let badged = rgba(TRAY_SIDE, accent, Some([230, 20, 20]));
        assert_eq!(at(&badged, TRAY_SIDE, 24, 24), [230, 20, 20, 255], "pastille dans le coin");
        assert_eq!(at(&badged, TRAY_SIDE, 16, 16), [200, 80, 40, 255]);
    }

    /// Propre à toutes les tailles de l'icône de l'application (lot « icône ») :
    /// bord du disque et bords de l'anneau adoucis, anneau lisible.
    #[test]
    fn edges_are_smoothed_and_the_ring_reads_at_every_size() {
        let accent = [0x58, 0x65, 0xf2];
        for side in [16, 20, 24, 32, 48, 256] {
            let px = rgba(side, accent, None);
            let mid = side / 2;
            // Rangée du milieu, du bord gauche au centre.
            let row: Vec<[u8; 4]> = (0..=mid).map(|x| at(&px, side, x, mid)).collect();
            assert!(row[0][3] > 0 && row[0][3] < 255, "{side} : bord du disque adouci ({})", row[0][3]);
            assert!(row[1..].iter().all(|p| p[3] == 255), "{side} : intérieur opaque");
            assert_eq!(*row.last().unwrap(), [0x58, 0x65, 0xf2, 255], "{side} : centre de la couleur d'accent");
            // L'anneau : au moins un pixel presque blanc en son milieu, de l'accent pur de part et d'autre.
            let ring = row.iter().map(|p| p[0].min(p[1])).max().unwrap_or(0);
            assert!(ring >= 200, "{side} : anneau lisible (au plus clair : {ring})");
            assert_eq!(row[1][..3], accent, "{side} : accent entre le bord et l'anneau");
            // Aucun pixel n'est d'une autre teinte qu'un mélange d'accent et de blanc.
            assert!(px.as_chunks::<4>().0.iter().all(|p| p[0] >= accent[0] && p[1] >= accent[1] && p[2] >= accent[2]), "{side} : teintes");
        }
        // En 256, les bords de l'anneau passent par des tons intermédiaires (pas d'escalier).
        let px = rgba(256, accent, None);
        let tones = px.as_chunks::<4>().0.iter().filter(|p| p[3] == 255 && p[0] > accent[0] && p[0] < 255).count();
        assert!(tones > 500, "anneau adouci en 256 ({tones} pixels intermédiaires)");
    }
}
