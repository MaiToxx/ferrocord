//! Mises en page de texte gardées d'une image à l'autre (lot C8).
//!
//! egui ne garde une mise en page (`Galley`) que si elle a servi à l'image
//! précédente : revenir dans un salon refait la mise en page de chaque
//! message affiché, et l'image du changement de salon dépassait le budget.
//! Ce cache garde les mises en page des messages ([`super::markdown`]) tant
//! qu'elles servent de temps en temps, avec une éviction écrite : au-delà de
//! [`MAX_BYTES`] octets (ou de [`MAX_ENTRIES`] mises en page), les moins
//! récemment servies partent jusqu'aux trois quarts.
//!
//! Le plafond est en octets (lot M) : une mise en page tient environ 170
//! octets par caractère (glyphe, quatre sommets et six indices du maillage),
//! soit 25 Ko en moyenne pour les messages du banc ; 1 000 mises en page en
//! gardaient 20 Mo, cause des +29 Mo de fin de banc après C8 (avec la
//! fragmentation du tas, 20 Mo gardés coûtent plus de 30 Mo de mémoire
//! privée). 12 Mo (environ 430 mises en page) gardent les écrans visibles et
//! leurs marges des quatre salons du banc : le changement de salon reste aussi
//! rapide ; à 6 Mo, il refaisait des mises en page (médiane 1,2 → 2,5 ms).
//!
//! Une mise en page désigne des glyphes dans l'atlas des polices d'egui :
//! tout est jeté quand l'atlas est refait (polices changées, atlas plein
//! vidé par egui, échelle changée).

use eframe::egui::{self, text::LayoutJob, Galley};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::{Arc, Mutex},
};

/// Octets de mises en page gardés au plus.
pub const MAX_BYTES: usize = 12 << 20;
/// Mises en page gardées au plus (beaucoup de messages très courts).
pub const MAX_ENTRIES: usize = 1000;

struct Entry {
    galley: Arc<Galley>,
    tick: u64,
    bytes: usize,
}

#[derive(Default)]
struct Cache {
    map: HashMap<u64, Entry>,
    bytes: usize,
    /// Passage d'egui de la dernière lecture, et compteur de passages vus.
    pass: Option<u64>,
    tick: u64,
    /// Remplissage de l'atlas et génération des polices au dernier passage.
    fill: f32,
    fonts: u64,
    /// Polices changées au passage précédent : egui ne les installe qu'au
    /// début du suivant, qui doit donc repartir de zéro lui aussi.
    again: bool,
}

impl Cache {
    fn clear(&mut self) {
        self.map.clear();
        self.bytes = 0;
    }

    fn evict(&mut self, max_bytes: usize) {
        if self.bytes <= max_bytes && self.map.len() <= MAX_ENTRIES {
            return;
        }
        // Garde les plus récemment servies, jusqu'aux trois quarts des deux plafonds.
        let mut by_age: Vec<(u64, usize)> = self.map.values().map(|e| (e.tick, e.bytes)).collect();
        by_age.sort_unstable_by_key(|a| std::cmp::Reverse(a.0));
        let (room, slots) = (max_bytes / 4 * 3, MAX_ENTRIES / 4 * 3);
        let mut kept = 0;
        let mut limit = u64::MAX;
        for (n, (tick, bytes)) in by_age.into_iter().enumerate() {
            if kept + bytes > room || n >= slots {
                limit = tick;
                break;
            }
            kept += bytes;
        }
        // Les mises en page servies au même passage que la première qui ne
        // tient plus partent avec elle : l'éviction ne se refait pas à chaque insertion.
        self.map.retain(|_, e| e.tick > limit);
        self.bytes = self.map.values().map(|e| e.bytes).sum();
    }
}

fn id() -> egui::Id {
    egui::Id::unique("ferrocord-mises-en-page")
}

/// Mise en page de `job` pour `ui` : celle d'une image précédente si le même
/// texte, au même format et à la même largeur, a déjà été mis en page.
pub(super) fn layout(ui: &egui::Ui, job: LayoutJob) -> Arc<Galley> {
    layout_within(ui, job, MAX_BYTES)
}

fn layout_within(ui: &egui::Ui, job: LayoutJob, max_bytes: usize) -> Arc<Galley> {
    let ctx = ui.ctx();
    let cache = ctx.data_mut(|d| d.get_temp_mut_or_default::<Arc<Mutex<Cache>>>(id()).clone());
    let Ok(mut c) = cache.lock() else {
        return ui.fonts_mut(|f| f.layout_job(job));
    };
    let pass = ctx.cumulative_pass_nr();
    if c.pass != Some(pass) {
        c.pass = Some(pass);
        c.tick += 1;
        // L'atlas ne fait que se remplir entre deux remises à zéro : s'il
        // s'est vidé, ou si les polices ont changé, plus rien ne vaut.
        let fill = ctx.fonts(|f| f.font_atlas_fill_ratio());
        let fonts = super::fonts::generation();
        if fill < c.fill || fonts != c.fonts || c.again {
            c.clear();
        }
        c.again = fonts != c.fonts;
        c.fill = fill;
        c.fonts = fonts;
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    job.hash(&mut h);
    ctx.pixels_per_point().to_bits().hash(&mut h);
    let key = h.finish();
    let tick = c.tick;
    if let Some(e) = c.map.get_mut(&key) {
        e.tick = tick;
        return e.galley.clone();
    }
    let g = ui.fonts_mut(|f| f.layout_job(job));
    let bytes = weight(&g);
    // Une mise en page plus grosse que le quart du plafond (message immense)
    // n'est pas gardée : egui la garde déjà tant qu'elle reste affichée.
    if bytes <= max_bytes / 4 {
        c.bytes += bytes;
        c.map.insert(key, Entry { galley: g.clone(), tick, bytes });
        c.evict(max_bytes);
    }
    g
}

/// Octets tenus par une mise en page : texte, glyphes et maillage de chaque ligne.
fn weight(g: &Galley) -> usize {
    use egui::epaint::{
        text::{Glyph, PlacedRow, Row},
        Vertex,
    };
    use std::mem::size_of;
    let rows: usize = g
        .rows
        .iter()
        .map(|r| {
            let m = &r.visuals.mesh;
            size_of::<PlacedRow>() + size_of::<Row>() + r.glyphs.capacity() * size_of::<Glyph>() + m.vertices.capacity() * size_of::<Vertex>() + m.indices.capacity() * size_of::<u32>()
        })
        .sum();
    size_of::<Galley>() + g.job.text.capacity() + g.job.sections.capacity() * size_of::<egui::text::LayoutSection>() + rows
}

/// Mises en page gardées et octets qu'elles tiennent (mesures du banc).
pub(super) fn stats(ctx: &egui::Context) -> (usize, usize) {
    let Some(c) = ctx.data(|d| d.get_temp::<Arc<Mutex<Cache>>>(id())) else { return (0, 0) };
    let Ok(c) = c.lock() else { return (0, 0) };
    (c.map.len(), c.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(ctx: &egui::Context, mut f: impl FnMut(&mut egui::Ui)) {
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| f(ui));
        out.textures_delta.clear();
    }

    fn job(text: &str) -> LayoutJob {
        let mut j = LayoutJob::simple(text.into(), egui::FontId::proportional(14.0), egui::Color32::WHITE, 200.0);
        j.wrap.max_width = 200.0;
        j
    }

    fn len(ctx: &egui::Context) -> usize {
        stats(ctx).0
    }

    /// Le même texte d'une image à l'autre : la même mise en page, sans la refaire.
    #[test]
    fn kept_across_frames_and_bounded() {
        let ctx = egui::Context::default();
        let mut first = None;
        for frame in 0..3 {
            run(&ctx, |ui| {
                let g = layout(ui, job("bonjour tout le monde"));
                match &first {
                    None => first = Some(g),
                    Some(f) => assert!(Arc::ptr_eq(f, &g), "image {frame}"),
                }
                // Une image où il ne sert pas : egui l'aurait oublié, pas ce cache.
                if frame == 1 {
                    for i in 0..10 {
                        layout(ui, job(&format!("autre {i}")));
                    }
                }
            });
        }
        assert!(len(&ctx) <= 11);
        run(&ctx, |ui| {
            for i in 0..MAX_ENTRIES + 100 {
                layout(ui, job(&format!("texte {i}")));
            }
        });
        assert!(len(&ctx) <= MAX_ENTRIES, "{}", len(&ctx));
    }

    /// Le plafond en octets : les plus anciennes partent, les plus récentes restent,
    /// et le compte des octets suit ce qui est gardé.
    #[test]
    fn bytes_bounded_oldest_leave_first() {
        let ctx = egui::Context::default();
        let long = |i: usize| job(&format!("{i} {}", "message assez long pour peser ".repeat(8)));
        let mut one = 0;
        let mut first = None;
        run(&ctx, |ui| {
            first = Some(layout_within(ui, long(0), usize::MAX));
            one = stats(ui.ctx()).1;
        });
        assert!(one > 30_000, "une mise en page de 250 caractères pèse plus de 30 Ko : {one}");
        let max = one * 20;
        let mut last = None;
        for i in 1..100 {
            run(&ctx, |ui| last = Some(layout_within(ui, long(i), max)));
        }
        let (n, bytes) = stats(&ctx);
        assert!(bytes <= max && n < 20, "{n} mises en page, {bytes} octets pour {max}");
        let held = ctx.data(|d| d.get_temp::<Arc<Mutex<Cache>>>(id())).expect("cache");
        let sum: usize = held.lock().expect("verrou").map.values().map(|e| weight(&e.galley)).sum();
        assert_eq!(sum, bytes);
        // La plus récente est encore là, la toute première non.
        run(&ctx, |ui| {
            assert!(Arc::ptr_eq(last.as_ref().expect("mise en page"), &layout_within(ui, long(99), max)));
        });
        run(&ctx, |ui| {
            assert!(!Arc::ptr_eq(first.as_ref().expect("mise en page"), &layout_within(ui, long(0), max)), "la première est partie");
        });
        // Trop grosse pour le plafond : rendue, pas gardée.
        let ctx = egui::Context::default();
        run(&ctx, |ui| {
            layout_within(ui, long(0), one);
        });
        assert_eq!(stats(&ctx), (0, 0));
    }

    /// Polices changées : les anciennes mises en page ne servent plus.
    #[test]
    fn fonts_change_clears() {
        let ctx = egui::Context::default();
        let mut a = None;
        run(&ctx, |ui| a = Some(layout(ui, job("x"))));
        super::super::fonts::install_builtin(&ctx);
        run(&ctx, |ui| {
            let b = layout(ui, job("x"));
            assert!(!Arc::ptr_eq(a.as_ref().expect("mise en page"), &b));
        });
    }
}
