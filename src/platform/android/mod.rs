//! Android (phase 7) : point d'entrée NativeActivity, dossiers privés de
//! l'application, et les services du système appelés par JNI.
//!
//! Sans objet sur Android, donc absents : zone de notification, démarrage
//! avec le système, registre, liens `discord://`, instance unique, mises à
//! jour et installateur (c'est le magasin ou l'APK qui installe).

mod jvm;
pub mod keyboard;
pub mod logcat;
pub mod notify;
pub mod vault;

use jni::objects::JValue;
use jni::{jni_sig, jni_str};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

pub use winit::platform::android::activity::AndroidApp;

static APP: OnceLock<AndroidApp> = OnceLock::new();
static FILES: OnceLock<PathBuf> = OnceLock::new();

/// L'activité, pour la boucle d'événements de winit.
pub fn app() -> Option<AndroidApp> {
    APP.get().cloned()
}

/// `/data/data/<paquet>/files` : réglages, thème, comptes, coffre.
pub fn files_dir() -> Option<PathBuf> {
    FILES.get().cloned()
}

/// `/data/data/<paquet>/cache` : vidable par le système.
pub fn cache_dir() -> Option<PathBuf> {
    let files = FILES.get()?;
    Some(files.parent().map_or_else(|| files.join("cache"), |p| p.join("cache")))
}

/// Point d'entrée, appelé par `android_main` (crate `android/`).
pub fn main(app: AndroidApp) {
    // D'abord : toute panique qui suit est lisible dans logcat.
    logcat::install_panic_hook();
    match app.internal_data_path() {
        Some(dir) => {
            let _ = FILES.set(dir);
        }
        None => logcat::error("stockage privé introuvable : réglages et coffre indisponibles"),
    }
    let _ = APP.set(app);
    if let Err(e) = crate::run() {
        logcat::error(&format!("arrêt de la boucle d'événements : {e}"));
    }
}

/// Ouvre un lien par un Intent ACTION_VIEW (navigateur ou application
/// choisie par l'utilisateur). Seule l'adresse part : aucun jeton, aucun
/// extra ajouté à l'Intent.
pub fn open_url(url: &str) -> bool {
    jvm::with_activity(|env, activity| {
        let text = env.new_string(url)?;
        let uri = env
            .call_static_method(jni_str!("android/net/Uri"), jni_str!("parse"), jni_sig!("(Ljava/lang/String;)Landroid/net/Uri;"), &[JValue::Object(&text)])?
            .l()?;
        let action = env.new_string("android.intent.action.VIEW")?;
        let intent = env.new_object(jni_str!("android/content/Intent"), jni_sig!("(Ljava/lang/String;Landroid/net/Uri;)V"), &[JValue::Object(&action), JValue::Object(&uri)])?;
        // Intent.FLAG_ACTIVITY_NEW_TASK : appelé hors du fil de l'activité.
        env.call_method(&intent, jni_str!("addFlags"), jni_sig!("(I)Landroid/content/Intent;"), &[JValue::Int(0x1000_0000)])?;
        env.call_method(activity, jni_str!("startActivity"), jni_sig!("(Landroid/content/Intent;)V"), &[JValue::Object(&intent)])?;
        Ok(())
    })
    .is_some()
}

/// Demande une permission « dangereuse » (micro, notifications) si elle
/// n'est pas déjà accordée. La réponse de l'utilisateur arrive plus tard :
/// `false` maintenant, `true` au prochain appel s'il a accepté.
pub fn ensure_permission(name: &str) -> bool {
    jvm::with_activity(|env, activity| {
        let perm = env.new_string(name)?;
        let granted = env.call_method(activity, jni_str!("checkSelfPermission"), jni_sig!("(Ljava/lang/String;)I"), &[JValue::Object(&perm)])?.i()?;
        // PackageManager.PERMISSION_GRANTED.
        if granted == 0 {
            return Ok(true);
        }
        let string_class = env.find_class(jni_str!("java/lang/String"))?;
        let list = env.new_object_array(1, &string_class, &perm)?;
        env.call_method(activity, jni_str!("requestPermissions"), jni_sig!("([Ljava/lang/String;I)V"), &[JValue::Object(&list), JValue::Int(1)])?;
        Ok(false)
    })
    .unwrap_or(false)
}

/// Permission du micro, demandée une seule fois par lancement (au premier
/// appel ou message vocal) : redemander à chaque essai harcèlerait.
pub fn ask_microphone() {
    static ASKED: AtomicBool = AtomicBool::new(false);
    if !ASKED.swap(true, Ordering::SeqCst) {
        ensure_permission("android.permission.RECORD_AUDIO");
    }
}
