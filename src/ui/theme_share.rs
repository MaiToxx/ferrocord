//! Thèmes partageables (lot E5, plugin `themes_partageables`) : exporter le
//! thème en cours dans un fichier `.ferrotheme`, en importer un (choisi, ou
//! déposé sur la fenêtre), galerie des thèmes fournis. Un thème n'est jamais
//! appliqué sans aperçu : la fenêtre d'import le montre d'abord.
//!
//! Le format et sa lecture stricte sont dans `crate::theme::share` ; les
//! fichiers sont lus et écrits par le fil de `crate::look`. Plugin coupé : la
//! section n'est pas dessinée et un fichier déposé n'est pas regardé.

use super::appearance::row;
use super::look::{contrast_report, preview_card, swatch};
use super::settings::{note, section, toggle};
use super::widgets::{self, Kind as Button};
use super::App;
use crate::i18n::table::e5::{f as ef, t as et};
use crate::i18n::table::perso::t as pt;
use crate::look::{self, Look};
use crate::plugins::{self, Id};
use crate::theme::share::{self, Package, ShareError};
use crate::theme::Tokens;
use eframe::egui::{self, RichText};

/// Largeur du contenu de la fenêtre d'import.
const MODAL_WIDTH: f32 = 540.0;

/// Thème lu, en attente de la décision de l'utilisateur.
struct Incoming {
    pkg: Box<Package>,
    /// Aperçu de l'image de fond embarquée.
    thumb: Option<egui::TextureHandle>,
}

#[derive(Default)]
pub(super) struct ShareUi {
    name: String,
    author: String,
    with_image: bool,
    incoming: Option<Incoming>,
    /// Thèmes fournis, lus à la première ouverture de la section.
    bundled: Option<Vec<Package>>,
}

impl ShareUi {
    /// La fenêtre d'import est ouverte : Échap la ferme, pas les Paramètres.
    pub(super) fn modal_open(&self) -> bool {
        self.incoming.is_some()
    }
}

fn is_theme_file(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(share::EXT))
}

/// Premier nom libre parmi `taken` : `name`, puis `name 2`, `name 3`…
fn free_name(name: &str, taken: &[String]) -> String {
    let used = |n: &str| taken.iter().any(|t| t.eq_ignore_ascii_case(n));
    if !used(name) {
        return name.to_string();
    }
    (2..1000).filter_map(|i| look::clean_name(&format!("{} {i}", name.chars().take(look::NAME_MAX - 4).collect::<String>()))).find(|n| !used(n)).unwrap_or_else(|| name.to_string())
}

fn bundled() -> Vec<Package> {
    share::BUNDLED.iter().filter_map(|(_, text)| share::parse(text.as_bytes(), crate::i18n::language()).ok()).collect()
}

impl App {
    /// Aperçu hors ligne : fenêtre d'import ouverte sur le premier thème fourni.
    #[doc(hidden)]
    pub fn theme_share_preview(&mut self) {
        if let Some(p) = bundled().into_iter().next() {
            self.fx.share.incoming = Some(Incoming { pkg: Box::new(p), thumb: None });
        }
    }

    /// Thème en attente dans la fenêtre d'import (tests).
    #[cfg(test)]
    pub(super) fn theme_share_incoming(&self) -> Option<&Package> {
        self.fx.share.incoming.as_ref().map(|i| &*i.pkg)
    }

    /// Fichiers `.ferrotheme` déposés sur la fenêtre : retirés du dépôt (ils
    /// ne deviennent pas des pièces jointes), et le premier est lu. À appeler
    /// avant le reste de l'interface.
    pub(super) fn theme_share_drop(&mut self, ctx: &egui::Context) {
        if !plugins::on(Id::ShareThemes) {
            return;
        }
        let dropped = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).find(|p| is_theme_file(p)));
        let Some(path) = dropped else { return };
        ctx.input_mut(|i| i.raw.dropped_files.retain(|f| !is_theme_file(f.path())));
        self.look_service().read_package(path);
    }

    /// Réponses du fil des fichiers qui concernent les thèmes partageables.
    pub(super) fn theme_share_done(&mut self, result: Result<Box<Package>, ShareError>, preview: Option<egui::ColorImage>) {
        match result {
            Ok(pkg) => {
                let thumb = preview.map(|i| self.ctx.load_texture("theme-importe", i, egui::TextureOptions::LINEAR));
                self.fx.share.incoming = Some(Incoming { pkg, thumb });
                self.fx.message = None;
            }
            Err(e) => {
                let text = ef::share_refused(self.lang, e.message(self.lang));
                self.fx.message = Some(text.clone());
                self.notify(text);
            }
        }
        self.ctx.request_repaint();
    }

    pub(super) fn theme_share_exported(&mut self, result: Result<String, String>) {
        let text = match result {
            Ok(path) => ef::share_exported(self.lang, path),
            Err(reason) => ef::share_export_failed(self.lang, reason),
        };
        self.fx.message = Some(text.clone());
        self.notify(text);
    }

    /// Applique le thème en attente : ajouté aux profils d'apparence (image
    /// copiée dans le dossier de Ferrocord), puis appliqué.
    fn theme_share_apply(&mut self) {
        let Some(Incoming { pkg, .. }) = self.fx.share.incoming.take() else { return };
        let mut tokens = pkg.tokens.clone();
        share::keep_personal(&mut tokens, &self.look.edited);
        let prefs = share::merged(&self.local.prefs.look, &pkg.look, String::new());
        let name = free_name(&pkg.name, &self.fx.profiles);
        if self.look_service().has_folder() {
            let theme = crate::theme::export(&tokens.sanitized());
            self.look_service().install_package(name, theme, prefs, pkg.image.clone());
        } else {
            // Rien ne peut être écrit (aperçu, dossier refusé) : appliqué sans image, pas enregistré.
            self.look.edited = tokens;
            let mut p = self.local_prefs();
            p.look = Box::new(prefs);
            self.set_local_prefs(p);
            self.fx.message = Some(et::SHARE_UNSAVED.get(self.lang).into());
        }
        self.ctx.request_repaint();
    }

    /// Section « Thèmes partageables » de la page Apparence.
    pub(super) fn theme_share_section(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        if !plugins::on(Id::ShareThemes) {
            return;
        }
        let lang = self.lang;
        section(ui, t, et::SEC_SHARE.get(lang));
        note(ui, t, et::SHARE_TIP.get(lang));

        // Galerie des thèmes fournis : un clic ouvre l'aperçu.
        ui.label(RichText::new(et::BUNDLED.get(lang)).family(crate::ui::fonts::demi()).color(t.colors.text_strong));
        let list = self.fx.share.bundled.get_or_insert_with(bundled);
        let mut chosen = None;
        ui.horizontal_wrapped(|ui| {
            for (i, p) in list.iter().enumerate() {
                let c = &p.tokens.colors;
                if swatch(ui, t, &p.name, &[c.bg0, c.bg1, c.bg2, c.accent, c.text], false).clicked() {
                    chosen = Some(i);
                }
            }
        });
        if let Some(p) = chosen.and_then(|i| list.get(i)) {
            self.fx.share.incoming = Some(Incoming { pkg: Box::new(p.clone()), thumb: None });
        }
        ui.add_space(t.spacing.item_y);

        // Export du thème en cours, puis import d'un fichier.
        let busy = self.look_service().picking();
        let (mut export, mut import) = (false, false);
        let s = &mut self.fx.share;
        row(ui, et::SHARE_EXPORT_ROW.get(lang), et::SHARE_EXPORT_ROW_TIP.get(lang), |ui| {
            let width = ((ui.available_width() - ui.spacing().item_spacing.x) / 2.0).max(80.0);
            let r = ui.add(egui::TextEdit::singleline(&mut s.name).hint_text(et::SHARE_NAME.get(lang)).char_limit(look::NAME_MAX).desired_width(width));
            super::a11y::name(ui.ctx(), r.id, et::SHARE_NAME.get(lang));
            let r = ui.add(egui::TextEdit::singleline(&mut s.author).hint_text(et::SHARE_AUTHOR.get(lang)).char_limit(share::AUTHOR_MAX).desired_width(f32::INFINITY));
            super::a11y::name(ui.ctx(), r.id, et::SHARE_AUTHOR.get(lang));
        });
        let image = self.local.prefs.look.backdrop.path.clone();
        if !image.is_empty()
            && let Some(v) = toggle(ui, et::SHARE_WITH_IMAGE.get(lang), et::SHARE_WITH_IMAGE_TIP.get(lang), Some(s.with_image), true)
        {
            s.with_image = v;
        }
        ui.horizontal(|ui| {
            export = ui.add_enabled(Look::CAN_PICK && !busy, egui::Button::new(et::SHARE_EXPORT.get(lang))).clicked();
            import = ui.add_enabled(Look::CAN_PICK && !busy, egui::Button::new(et::SHARE_IMPORT.get(lang))).clicked();
        });
        if import {
            self.look_service().pick_package(et::SHARE_IMPORT_TITLE.get(lang));
        }
        if export {
            let s = &self.fx.share;
            match look::clean_name(&s.name) {
                None => self.fx.message = Some(pt::BAD_NAME.get(lang).into()),
                Some(name) => {
                    let pkg = Package { name, author: s.author.trim().to_string(), tokens: self.look.edited.clone().sanitized(), look: share::shared(&self.local.prefs.look), image: None };
                    let image = (s.with_image && !image.is_empty()).then_some(image);
                    self.fx.message = None;
                    self.look_service().export_package(et::SHARE_EXPORT_TITLE.get(lang), pkg, image);
                }
            }
        }
    }

    /// Fenêtre d'import : aperçu du thème, puis « Appliquer » ou « Annuler ».
    /// Dessinée par-dessus tout, où que le fichier ait été déposé.
    pub(super) fn theme_share_ui(&mut self, ctx: &egui::Context) {
        let Some(incoming) = &self.fx.share.incoming else { return };
        if !plugins::on(Id::ShareThemes) {
            self.fx.share.incoming = None;
            return;
        }
        let t = self.theme.clone();
        let lang = self.lang;
        let pkg = &incoming.pkg;
        let (mut apply, mut cancel) = (false, false);
        let out = widgets::modal(
            ctx,
            &t,
            egui::Id::unique("theme-partage"),
            MODAL_WIDTH,
            &ef::share_import_title(lang, &pkg.name),
            crate::i18n::t::CLOSE_TIP.get(lang),
            |ui| {
                if !pkg.author.is_empty() {
                    ui.label(RichText::new(ef::share_by(lang, &pkg.author)).color(t.colors.muted));
                    ui.add_space(t.spacing.item_y);
                }
                preview_card(ui, &pkg.tokens.scaled(), &pkg.look, lang);
                if let Some(tex) = &incoming.thumb {
                    ui.horizontal(|ui| {
                        let size = tex.size_vec2();
                        let h = 56.0;
                        ui.add(egui::Image::new((tex.id(), egui::vec2(size.x / size.y.max(1.0) * h, h))).corner_radius(t.r_embed()));
                        ui.label(RichText::new(et::SHARE_HAS_IMAGE.get(lang)).color(t.colors.text));
                    });
                    ui.add_space(t.spacing.item_y);
                }
                let fonts: Vec<&str> = [pkg.look.ui_font.as_str(), pkg.look.code_font.as_str()].into_iter().filter(|f| !f.is_empty()).collect();
                if !fonts.is_empty() {
                    ui.add(egui::Label::new(RichText::new(ef::share_fonts(lang, fonts.join(", "))).color(t.colors.text)).wrap());
                    ui.add(egui::Label::new(RichText::new(et::SHARE_FONTS_NOTE.get(lang)).small().color(t.colors.muted)).wrap());
                }
                contrast_report(ui, &t, lang, &pkg.tokens);
                ui.add_space(t.spacing.item_y);
                ui.add(egui::Label::new(RichText::new(et::SHARE_KEEPS.get(lang)).small().color(t.colors.muted)).wrap());
            },
            |ui| {
                apply = widgets::button(ui, &t, Button::Primary, et::SHARE_APPLY.get(lang)).clicked();
                cancel = widgets::button(ui, &t, Button::Link, crate::i18n::t::CANCEL.get(lang)).clicked();
            },
        );
        if apply {
            self.theme_share_apply();
        } else if cancel || out.closed {
            self.fx.share.incoming = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::contrast_ratio;
    use crate::ui::keys::tests::app;
    use crate::ui::tests::{click, find, frame};

    /// Quelques images : une fenêtre modale n'a ses éléments qu'une fois posée.
    fn settle(a: &mut App) -> egui::FullOutput {
        for _ in 0..4 {
            frame(a, Vec::new());
        }
        frame(a, Vec::new())
    }

    #[test]
    fn names_stay_unique() {
        let taken = vec!["Braise".to_string(), "braise 2".to_string()];
        assert_eq!(free_name("Lagon", &taken), "Lagon");
        assert_eq!(free_name("Braise", &taken), "Braise 3");
        let long = "a".repeat(look::NAME_MAX);
        let got = free_name(&long, std::slice::from_ref(&long));
        assert!(got != long && got.chars().count() <= look::NAME_MAX);
        assert!(is_theme_file(std::path::Path::new(r"C:\x\Mon thème.FerroTheme")) && !is_theme_file(std::path::Path::new("photo.png")) && !is_theme_file(std::path::Path::new("ferrotheme")));
    }

    /// Les thèmes fournis passent le contrôle de contraste de l'éditeur,
    /// dégradé compris (voile le plus défavorable sur chaque fond).
    #[test]
    fn bundled_themes_are_readable() {
        let list = bundled();
        assert_eq!(list.len(), share::BUNDLED.len());
        for p in &list {
            for (name, fg, bg, min) in super::super::look::contrast_checks(&p.tokens) {
                assert!(contrast_ratio(fg, bg) >= min, "{} : {} ({:.2})", p.name, name.get(crate::i18n::Lang::Fr), contrast_ratio(fg, bg));
            }
            // Accent visible sur la discussion (élément graphique : 3:1).
            assert!(contrast_ratio(p.tokens.colors.accent, p.tokens.colors.bg2) >= 3.0, "{}", p.name);
            if p.look.gradient.on {
                let a = f32::from(p.look.gradient.strength) / 100.0;
                for c in &p.look.gradient.colors {
                    let c2 = p.tokens.colors.bg2;
                    let over = |g: u8, b: u8| (f32::from(g) * a + f32::from(b) * (1.0 - a)).round() as u8;
                    let veiled = egui::Color32::from_rgb(over(c[0], c2.r()), over(c[1], c2.g()), over(c[2], c2.b()));
                    assert!(contrast_ratio(p.tokens.colors.text, veiled) >= 4.5 && contrast_ratio(p.tokens.colors.muted, veiled) >= 4.5, "{} : texte sur le dégradé", p.name);
                }
            }
        }
    }

    /// Un thème reçu n'est pas appliqué avant « Appliquer » ; appliqué, il
    /// garde le zoom et les réglages personnels. « Annuler » ne change rien.
    #[test]
    fn import_previews_before_applying() {
        let mut a = app();
        a.open_appearance();
        let before = (a.look.edited.clone(), a.look_prefs());
        let mut mine = a.local_prefs();
        mine.look.hide_rail = true;
        a.set_local_prefs(mine);
        a.look.edited.layout.clock = crate::theme::Clock::H12;
        let text = share::BUNDLED[0].1;
        let pkg = share::parse(text.as_bytes(), crate::i18n::Lang::Fr).unwrap();
        a.theme_share_done(Ok(Box::new(pkg.clone())), None);
        let out = settle(&mut a);
        assert!(find(&out, "Appliquer").is_some() && find(&out, "Importer un fichier…").is_some(), "aperçu ouvert, section dessinée");
        assert_eq!(a.look.edited.colors, before.0.colors, "rien n'est appliqué par l'aperçu");
        click(&mut a, "Annuler");
        assert!(a.theme_share_incoming().is_none());
        assert_eq!(a.look.edited.colors, before.0.colors);

        a.theme_share_done(Ok(Box::new(pkg.clone())), None);
        click(&mut a, "Appliquer");
        assert!(a.theme_share_incoming().is_none());
        assert_eq!(a.look.edited.colors.accent, pkg.tokens.colors.accent);
        assert_eq!(a.look.edited.layout.clock, crate::theme::Clock::H12, "les réglages personnels du thème restent");
        let l = a.look_prefs();
        assert!(l.gradient.on && l.hide_rail && l.backdrop.path.is_empty());
        assert_eq!(l.gradient.colors, pkg.look.gradient.colors);
    }

    /// Fichier refusé : un message, pas de fenêtre, rien de changé.
    #[test]
    fn refused_file_changes_nothing() {
        let mut a = app();
        let before = a.look.edited.clone();
        let err = share::parse(b"ferrotheme = 1\nnom = \"x\"\n[code]\nexecuter = \"calc.exe\"\n", crate::i18n::Lang::Fr).unwrap_err();
        a.theme_share_done(Err(err), None);
        frame(&mut a, Vec::new());
        assert!(a.theme_share_incoming().is_none());
        assert!(a.fx.message.as_deref().is_some_and(|m| m.starts_with("Thème refusé")));
        assert_eq!(a.look.edited, before);
    }

    /// Plugin coupé : pas de section, un thème en attente est abandonné.
    #[test]
    fn nothing_when_the_plugin_is_off() {
        let mut a = app();
        a.open_appearance();
        a.theme_share_preview();
        assert!(find(&settle(&mut a), "Appliquer").is_some());
        a.set_plugin(Id::ShareThemes, false);
        let out = settle(&mut a);
        assert!(find(&out, "Importer un fichier…").is_none() && find(&out, "Appliquer").is_none());
        assert!(a.theme_share_incoming().is_none());
    }
}
