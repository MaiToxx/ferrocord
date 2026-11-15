//! Recadrage d'une image d'avatar (carré) ou de bannière (5:2) avant envoi :
//! calcul seul, sans image ni egui.

/// Ce qu'on envoie.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImageKind {
    Avatar,
    Banner,
    /// Icône d'un serveur (Modify Guild, `icon`).
    GuildIcon,
    /// Bannière d'un serveur (`banner`, 16:9).
    GuildBanner,
    /// Arrière-plan des invitations d'un serveur (`splash`, 16:9).
    Splash,
}

impl ImageKind {
    /// Largeur sur hauteur du cadre.
    pub fn aspect(self) -> f32 {
        match self {
            ImageKind::Avatar | ImageKind::GuildIcon => 1.0,
            // Proportions de la bannière de profil de l'application officielle (600 × 240).
            ImageKind::Banner => 2.5,
            ImageKind::GuildBanner | ImageKind::Splash => 16.0 / 9.0,
        }
    }

    /// Taille de l'image envoyée (pixels) ; celles d'un serveur suivent les
    /// tailles conseillées par l'application officielle.
    pub fn output(self) -> (u32, u32) {
        match self {
            ImageKind::Avatar | ImageKind::GuildIcon => (512, 512),
            ImageKind::Banner => (600, 240),
            ImageKind::GuildBanner => (960, 540),
            ImageKind::Splash => (1920, 1080),
        }
    }

    /// Nom du champ JSON (`PATCH /users/@me`, `PATCH /guilds/{id}/members/@me`, `PATCH /guilds/{id}`).
    pub fn field(self) -> &'static str {
        match self {
            ImageKind::Avatar => "avatar",
            ImageKind::Banner | ImageKind::GuildBanner => "banner",
            ImageKind::GuildIcon => "icon",
            ImageKind::Splash => "splash",
        }
    }
}

/// Zoom maximal du recadrage.
pub const MAX_ZOOM: f32 = 8.0;

/// Position du cadre : agrandissement (1 : le plus grand cadre qui tient) et
/// centre, en fraction de la largeur et de la hauteur de l'image.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    pub zoom: f32,
    pub cx: f32,
    pub cy: f32,
}

impl Default for Crop {
    fn default() -> Self {
        Crop { zoom: 1.0, cx: 0.5, cy: 0.5 }
    }
}

/// Rectangle dans l'image source, en pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Crop {
    /// Rectangle découpé dans une image de `w` × `h` : aux proportions de
    /// `kind`, entièrement dans l'image, jamais vide.
    pub fn rect(&self, w: u32, h: u32, kind: ImageKind) -> Rect {
        let (wf, hf) = (w.max(1) as f32, h.max(1) as f32);
        let a = kind.aspect();
        let (bw, bh) = if wf / hf > a { (hf * a, hf) } else { (wf, wf / a) };
        let z = if self.zoom.is_finite() { self.zoom.clamp(1.0, MAX_ZOOM) } else { 1.0 };
        let (cw, ch) = ((bw / z).max(1.0).min(wf), (bh / z).max(1.0).min(hf));
        let fx = if self.cx.is_finite() { self.cx.clamp(0.0, 1.0) } else { 0.5 };
        let fy = if self.cy.is_finite() { self.cy.clamp(0.0, 1.0) } else { 0.5 };
        let x = (fx * wf - cw / 2.0).clamp(0.0, wf - cw);
        let y = (fy * hf - ch / 2.0).clamp(0.0, hf - ch);
        let (x, y) = (x.round() as u32, y.round() as u32);
        let cw = (cw.round() as u32).clamp(1, w.max(1) - x.min(w.max(1) - 1));
        let ch = (ch.round() as u32).clamp(1, h.max(1) - y.min(h.max(1) - 1));
        Rect { x, y, w: cw, h: ch }
    }

    /// Cadre déplacé de (`dx`, `dy`) pixels de l'image source ; le centre
    /// reste là où le cadre tient dans l'image.
    pub fn pan(&mut self, dx: f32, dy: f32, w: u32, h: u32, kind: ImageKind) {
        let r = self.rect(w, h, kind);
        let (wf, hf) = (w.max(1) as f32, h.max(1) as f32);
        let half = (r.w as f32 / 2.0 / wf, r.h as f32 / 2.0 / hf);
        let (cx, cy) = ((r.x as f32 + r.w as f32 / 2.0 + dx) / wf, (r.y as f32 + r.h as f32 / 2.0 + dy) / hf);
        self.cx = cx.clamp(half.0, 1.0 - half.0);
        self.cy = cy.clamp(half.1, 1.0 - half.1);
    }
}

/// `data:` d'une image PNG, pour les champs « image data » de Discord.
pub fn data_uri(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", super::usersettings::b64::encode(png))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn largest_frame_is_centred_and_has_the_right_shape() {
        assert_eq!(Crop::default().rect(400, 200, ImageKind::Avatar), Rect { x: 100, y: 0, w: 200, h: 200 });
        assert_eq!(Crop::default().rect(1000, 1000, ImageKind::Banner), Rect { x: 0, y: 300, w: 1000, h: 400 });
        let r = Crop { zoom: 2.0, ..Default::default() }.rect(400, 200, ImageKind::Avatar);
        assert_eq!(r, Rect { x: 150, y: 50, w: 100, h: 100 });
    }

    #[test]
    fn frame_never_leaves_the_image() {
        for (cx, cy, z) in [(0.0, 0.0, 1.0), (1.0, 1.0, 3.0), (f32::NAN, 2.0, f32::INFINITY), (0.9, 0.1, 100.0)] {
            for (w, h) in [(400, 200), (1, 1), (3, 1000), (1000, 3)] {
                for kind in [ImageKind::Avatar, ImageKind::Banner] {
                    let r = Crop { zoom: z, cx, cy }.rect(w, h, kind);
                    assert!(r.w >= 1 && r.h >= 1 && r.x + r.w <= w && r.y + r.h <= h, "{r:?} dans {w}×{h}");
                }
            }
        }
    }

    #[test]
    fn panning_moves_then_stops_at_the_edge() {
        let mut c = Crop { zoom: 2.0, ..Default::default() };
        c.pan(-30.0, 0.0, 400, 200, ImageKind::Avatar);
        assert_eq!(c.rect(400, 200, ImageKind::Avatar).x, 120);
        c.pan(-10_000.0, -10_000.0, 400, 200, ImageKind::Avatar);
        let r = c.rect(400, 200, ImageKind::Avatar);
        assert_eq!((r.x, r.y), (0, 0));
    }

    #[test]
    fn data_uri_is_base64_png() {
        assert_eq!(data_uri(&[1, 2, 3]), "data:image/png;base64,AQID");
    }
}
