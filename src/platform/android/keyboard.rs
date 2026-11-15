//! Clavier virtuel et touche retour d'Android, autour de l'interface :
//! hauteur du clavier lue dans les marges de la fenêtre (JNI) et passée à
//! [`App::set_keyboard_inset`] ; retour à la racine : application mise en
//! arrière-plan (comme les applications d'Android), pas fermée.

use super::jvm::with_activity;
use crate::ui::App;
use eframe::egui;
use jni::objects::JObject;
use jni::{jni_sig, jni_str, Env};
use std::time::Duration;

/// Relectures de la hauteur pendant que le clavier s'ouvre (pas d'événement
/// de winit pour lui) : toutes les 250 ms, 2 s au plus par champ touché.
const POLL: Duration = Duration::from_millis(250);
const MAX_POLLS: u8 = 8;

/// L'application, plus le suivi du clavier virtuel.
pub struct Mobile {
    app: App,
    polls: u8,
    inset_px: i32,
}

impl Mobile {
    pub fn new(app: App) -> Mobile {
        Mobile { app, polls: 0, inset_px: 0 }
    }

    fn track_keyboard(&mut self, ctx: &egui::Context) {
        if !ctx.egui_wants_keyboard_input() {
            self.polls = 0;
            if self.inset_px != 0 {
                self.inset_px = 0;
                self.app.set_keyboard_inset(0.0);
            }
            return;
        }
        if self.polls >= MAX_POLLS {
            return;
        }
        self.polls += 1;
        let px = with_activity(ime_bottom).unwrap_or(0);
        if px != self.inset_px {
            self.inset_px = px;
            self.app.set_keyboard_inset(px as f32 / ctx.pixels_per_point());
        }
        // Le clavier glisse en quelques centaines de ms : on relit encore un peu.
        ctx.request_repaint_after(POLL);
    }
}

impl eframe::App for Mobile {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        eframe::App::logic(&mut self.app, ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        eframe::App::ui(&mut self.app, ui, frame);
        let ctx = ui.ctx().clone();
        self.track_keyboard(&ctx);
    }
}

/// Hauteur du clavier au-dessus du bas de la fenêtre, en pixels :
/// marge système du bas (clavier compris) moins la marge stable (barre de
/// navigation). API 23 et plus ; 0 si la vue n'est pas encore attachée.
fn ime_bottom(env: &mut Env, activity: &JObject) -> jni::errors::Result<i32> {
    let window = env.call_method(activity, jni_str!("getWindow"), jni_sig!("()Landroid/view/Window;"), &[])?.l()?;
    let decor = env.call_method(&window, jni_str!("getDecorView"), jni_sig!("()Landroid/view/View;"), &[])?.l()?;
    let insets = env.call_method(&decor, jni_str!("getRootWindowInsets"), jni_sig!("()Landroid/view/WindowInsets;"), &[])?.l()?;
    if insets.is_null() {
        return Ok(0);
    }
    let all = env.call_method(&insets, jni_str!("getSystemWindowInsetBottom"), jni_sig!("()I"), &[])?.i()?;
    let stable = env.call_method(&insets, jni_str!("getStableInsetBottom"), jni_sig!("()I"), &[])?.i()?;
    Ok((all - stable).max(0))
}

/// Touche retour à la racine : l'application passe en arrière-plan.
pub fn to_background() {
    with_activity(|env, activity| env.call_method(activity, jni_str!("moveTaskToBack"), jni_sig!("(Z)Z"), &[jni::objects::JValue::Bool(true)]).map(|_| ()));
}
