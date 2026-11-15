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
            if d > ring.0 && d < ring.1 {
                px_rgba[..3].copy_from_slice(&[255, 255, 255]);
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
}
