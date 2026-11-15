//! Copier et enregistrer une image (lot VID) : depuis la visionneuse et le
//! menu contextuel d'une image du fil.
//!
//! Le dessin n'a pas accès à l'application : la demande est déposée dans la
//! mémoire d'egui ([`ask`]) et exécutée à la fin de l'image
//! ([`App::image_ops_tick`]). Le fichier d'origine est alors téléchargé (un
//! geste de l'utilisateur, hôtes de Discord seulement, taille bornée par
//! `net::cdn`), hors du fil d'interface, puis décodé et posé dans le
//! presse-papiers, ou écrit là où la boîte « Enregistrer sous » le dit. Le
//! résultat revient par la même mémoire et devient une notification.

use super::viewer::ViewSrc;
use super::App;
use crate::i18n::table::messages::{f as mf, t as mt};
use crate::i18n::{t as tx, Text};
use crate::theme::{icons::Icon, Tokens};
use eframe::egui::{self, Id};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Op {
    Copy,
    Save,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Ask {
    op: Op,
    url: String,
    name: String,
}

/// Ce qu'un travail terminé dit à l'utilisateur.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Done {
    Ok(Text),
    Failed(String),
}

/// L'enregistrement existe sur cette plateforme (boîte de dialogue native).
pub(super) const CAN_SAVE: bool = cfg!(windows);

fn asks() -> Id {
    Id::unique("images-demandes")
}

fn dones() -> Id {
    Id::unique("images-résultats")
}

/// Adresse du fichier à copier ou enregistrer : l'original s'il est chez
/// Discord (pièce jointe), sinon la copie de son proxy (image d'un embed :
/// le site tiers n'est jamais contacté).
pub(super) fn source<'a>(fetch: &'a str, original: &'a str) -> &'a str {
    if crate::net::cdn::served_by_discord(original) { original } else { fetch }
}

/// Demande la copie ou l'enregistrement de l'image à `url`.
pub(super) fn ask(ctx: &egui::Context, op: Op, url: &str, name: &str) {
    let a = Ask { op, url: url.to_string(), name: name.to_string() };
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Vec<Ask>>(asks()).push(a));
    ctx.request_repaint();
}

fn done(ctx: &egui::Context, d: Done) {
    ctx.data_mut(|m| m.get_temp_mut_or_default::<Vec<Done>>(dones()).push(d));
    ctx.request_repaint();
}

/// Menu contextuel d'une image du fil.
pub(super) fn menu(ui: &mut egui::Ui, t: &Tokens, src: &ViewSrc<'_>) {
    let lang = crate::i18n::language();
    let url = source(src.fetch, src.original);
    if super::menu::item(ui, t, mt::COPY_IMAGE.get(lang), Some(Icon::Copy), false).clicked() {
        ask(ui.ctx(), Op::Copy, url, src.name);
    }
    if CAN_SAVE && super::menu::item(ui, t, mt::SAVE_IMAGE.get(lang), Some(Icon::Download), false).clicked() {
        ask(ui.ctx(), Op::Save, url, src.name);
    }
    super::menu::separator(ui, t);
    if super::menu::item(ui, t, tx::COPY_IMAGE_LINK.get(lang), Some(Icon::Link), false).clicked() {
        ui.ctx().copy_text(src.original.to_string());
    }
    if super::menu::item(ui, t, mt::OPEN_LINK.get(lang), Some(Icon::ArrowRight), false).clicked() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(src.original));
    }
}

/// Nom de fichier proposé : celui de l'image, sans rien qui sorte du dossier
/// choisi ; l'extension vient du contenu si le nom n'en a pas.
pub(super) fn file_name(name: &str, bytes: &[u8]) -> String {
    let last = name.split(['?', '#']).next().unwrap_or(name).rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = last.chars().filter(|c| !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*')).take(120).collect();
    let clean = clean.trim_matches(['.', ' ']).to_string();
    let base = if clean.is_empty() { "image".to_string() } else { clean };
    let ext = match image::guess_format(bytes) {
        Ok(image::ImageFormat::Png) => "png",
        Ok(image::ImageFormat::Jpeg) => "jpg",
        Ok(image::ImageFormat::WebP) => "webp",
        Ok(image::ImageFormat::Gif) => "gif",
        _ => return base,
    };
    let known = base.rsplit_once('.').is_some_and(|(_, e)| ["png", "jpg", "jpeg", "webp", "gif"].contains(&e.to_ascii_lowercase().as_str()));
    if known { base } else { format!("{base}.{ext}") }
}

/// Image décodée (première image d'une animation), prête pour le presse-papiers.
pub(super) fn decode(bytes: &[u8]) -> Option<egui::ColorImage> {
    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let size = [usize::try_from(img.width()).ok()?, usize::try_from(img.height()).ok()?];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}

/// Raison d'un téléchargement manqué, en clair.
fn why(e: crate::net::cdn::FetchError, lang: crate::i18n::Lang) -> String {
    use crate::net::cdn::FetchError as E;
    match e {
        E::NotFound => tx::IMAGE_UNAVAILABLE.get(lang).to_string(),
        E::Refused(s) | E::Network(s) => s,
        E::RateLimited(_) => "HTTP 429".to_string(),
    }
}

#[cfg(windows)]
fn save(ctx: egui::Context, name: String, bytes: Vec<u8>) {
    // La boîte de dialogue bloque : dans son propre fil.
    let spawned = std::thread::Builder::new().name("ferrocord-enregistrer".into()).spawn(move || {
        let lang = crate::i18n::language();
        let Some(path) = rfd::FileDialog::new().set_title(mt::SAVE_IMAGE.get(lang)).set_file_name(file_name(&name, &bytes)).save_file() else { return };
        match std::fs::write(&path, &bytes) {
            Ok(()) => done(&ctx, Done::Ok(mt::IMAGE_SAVED)),
            Err(e) => done(&ctx, Done::Failed(mf::image_failed(lang, e.kind()))),
        }
    });
    drop(spawned);
}

#[cfg(not(windows))]
fn save(_: egui::Context, _: String, _: Vec<u8>) {}

impl App {
    /// Demandes de l'image qui vient d'être dessinée, résultats des précédentes.
    pub(super) fn image_ops_tick(&mut self, ctx: &egui::Context) {
        let (asked, finished) = ctx.data_mut(|d| (d.remove_temp::<Vec<Ask>>(asks()).unwrap_or_default(), d.remove_temp::<Vec<Done>>(dones()).unwrap_or_default()));
        for a in asked {
            let c = ctx.clone();
            let started = self.images.fetch_raw(a.url, move |got| {
                let lang = crate::i18n::language();
                match (got, a.op) {
                    (Err(e), _) => done(&c, Done::Failed(mf::image_failed(lang, why(e, lang)))),
                    (Ok(bytes), Op::Save) => save(c, a.name, bytes),
                    (Ok(bytes), Op::Copy) => match decode(&bytes) {
                        Some(img) => {
                            c.copy_image(img);
                            done(&c, Done::Ok(mt::IMAGE_COPIED));
                        }
                        None => done(&c, Done::Failed(mf::image_failed(lang, tx::IMAGE_UNAVAILABLE.get(lang)))),
                    },
                }
            });
            if !started {
                self.notify(tx::IMAGE_UNAVAILABLE);
            }
        }
        for d in finished {
            match d {
                Done::Ok(text) => self.notify(text),
                Done::Failed(why) => self.notify(why),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Vec<u8> {
        let img = image::RgbaImage::from_fn(3, 2, |x, _| image::Rgba([x as u8 * 100, 10, 20, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img).write_to(&mut out, image::ImageFormat::Png).unwrap_or_else(|e| panic!("{e}"));
        out.into_inner()
    }

    #[test]
    fn noms_de_fichier_surs() {
        let p = png();
        assert_eq!(file_name("chat.png", &p), "chat.png");
        assert_eq!(file_name("chat", &p), "chat.png");
        assert_eq!(file_name("https://site.invalid/a/b/photo.JPG?x=1", &p), "photo.JPG");
        assert_eq!(file_name("..\\..\\secret", &p), "secret.png");
        assert_eq!(file_name("a<b>:c|d?.gif", &p), "abcd.png");
        assert_eq!(file_name("", &p), "image.png");
        assert_eq!(file_name("...", b"rien"), "image");
        assert_eq!(file_name("notes.txt", &p), "notes.txt.png", "extension d'image d'après le contenu");
    }

    #[test]
    fn decode_et_source() {
        let img = decode(&png()).unwrap_or_else(|| panic!("décodage"));
        assert_eq!(img.size, [3, 2]);
        assert_eq!(img.pixels[1], egui::Color32::from_rgb(100, 10, 20));
        assert!(decode(b"pas une image").is_none());
        let proxy = "https://images-ext-1.discordapp.net/external/x/https/site.invalid/a.png";
        assert_eq!(source(proxy, "https://site.invalid/a.png"), proxy, "site tiers : jamais contacté");
        let cdn = "https://cdn.discordapp.com/attachments/1/2/a.png?ex=1";
        assert_eq!(source("https://media.discordapp.net/attachments/1/2/a.png", cdn), cdn);
    }

    /// La demande déposée pendant le dessin est exécutée au passage suivant :
    /// l'image arrive dans le presse-papiers d'egui et une notification le dit.
    #[test]
    fn copie_demandee_puis_notifiee() {
        struct One(Vec<u8>);
        impl crate::images::Source for One {
            fn url(&self, _: &crate::state::model::ImageRef, _: u32) -> String {
                String::new()
            }
            fn fetch(&self, url: String) -> crate::images::FetchFuture {
                let bytes = self.0.clone();
                Box::pin(async move { if url.ends_with("a.png") { Ok(bytes) } else { Err(crate::net::cdn::FetchError::NotFound) } })
            }
        }
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap_or_else(|e| panic!("{e}"));
        let ctx = egui::Context::default();
        let images = crate::images::Images::start(ctx.clone(), rt.handle(), std::sync::Arc::new(One(png())), None, crate::images::Config::default(), std::sync::Arc::new(|| {}));
        struct Quiet;
        impl crate::ui::Backend for Quiet {
            fn set_wake(&mut self, _: crate::ui::Wake) {}
            fn connect(&mut self, _: crate::account::Token) {}
            fn disconnect(&mut self) {}
            fn is_connected(&self) -> bool {
                false
            }
            fn poll(&mut self) -> Vec<crate::state::Event> {
                Vec::new()
            }
            fn send(&mut self, _: Vec<crate::state::Cmd>) {}
        }
        let mut app = App::with_context(ctx.clone(), crate::ui::tests::offline_boot(Box::new(Quiet), crate::state::Store::new(), images));
        let wait = |app: &mut App| {
            let t0 = std::time::Instant::now();
            while app.toast.is_none() && t0.elapsed() < std::time::Duration::from_secs(10) {
                app.image_ops_tick(&ctx);
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            app.toast.take().map(|t| t.text.to_string())
        };
        ask(&ctx, Op::Copy, "https://cdn.discordapp.com/attachments/1/2/a.png", "a.png");
        assert_eq!(wait(&mut app).as_deref(), Some(mt::IMAGE_COPIED.get(app.lang)));
        let copied = ctx.output_mut(|o| o.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyImage(i) if i.size == [3, 2])));
        assert!(copied, "image posée dans le presse-papiers");
        ask(&ctx, Op::Copy, "https://cdn.discordapp.com/attachments/1/2/absente.png", "x");
        assert!(wait(&mut app).is_some_and(|t| t != mt::IMAGE_COPIED.get(app.lang)), "échec dit à l'utilisateur");
    }
}
