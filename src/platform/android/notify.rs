//! Notifications d'Android (NotificationManager, par JNI) : une par salon
//! (la suivante remplace la précédente), un toucher ramène l'application.
//! Sur Android 13 et plus, la permission POST_NOTIFICATIONS est demandée à
//! la première notification ; refusée, rien ne s'affiche (sans erreur).

use super::jvm::with_activity;
use crate::state::notify::Alert;
use jni::objects::{JObject, JValue};
use jni::{jni_sig, jni_str, Env};

/// Canal des messages (Android 8 et plus).
const CHANNEL: &str = "messages";
/// Build.VERSION_CODES.TIRAMISU : la permission devient nécessaire.
const API_TIRAMISU: i32 = 33;

/// Montre `alert` (appelé par le fil du bureau, jamais par l'interface).
pub fn show(alert: &Alert) {
    let title = match &alert.place {
        Some(p) => format!("{} ({p})", alert.title),
        None => alert.title.clone(),
    };
    // Une notification par salon : l'identifiant est tiré du salon.
    let id = (alert.channel ^ (alert.channel >> 32)) as i32;
    let body = alert.body.clone();
    let sdk = with_activity(|env, _| sdk(env)).unwrap_or(0);
    if sdk >= API_TIRAMISU && !super::ensure_permission("android.permission.POST_NOTIFICATIONS") {
        return;
    }
    with_activity(|env, activity| post(env, activity, &title, &body, id));
}

fn sdk(env: &mut Env) -> jni::errors::Result<i32> {
    env.get_static_field(jni_str!("android/os/Build$VERSION"), jni_str!("SDK_INT"), jni_sig!("I"))?.i()
}

fn post(env: &mut Env, activity: &JObject, title: &str, body: &str, id: i32) -> jni::errors::Result<()> {
    let service = env.new_string("notification")?;
    let manager = env.call_method(activity, jni_str!("getSystemService"), jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"), &[JValue::Object(&service)])?.l()?;
    if manager.is_null() {
        return Ok(());
    }
    let channel_id = env.new_string(CHANNEL)?;
    let channel_name = env.new_string(crate::i18n::table::notify::t::CHANNEL_MESSAGES.get(crate::i18n::language()))?;
    // NotificationManager.IMPORTANCE_DEFAULT ; recréer un canal existant ne change rien.
    let channel = env.new_object(
        jni_str!("android/app/NotificationChannel"),
        jni_sig!("(Ljava/lang/String;Ljava/lang/CharSequence;I)V"),
        &[JValue::Object(&channel_id), JValue::Object(&channel_name), JValue::Int(3)],
    )?;
    env.call_method(&manager, jni_str!("createNotificationChannel"), jni_sig!("(Landroid/app/NotificationChannel;)V"), &[JValue::Object(&channel)])?;
    let builder = env.new_object(jni_str!("android/app/Notification$Builder"), jni_sig!("(Landroid/content/Context;Ljava/lang/String;)V"), &[JValue::Object(activity), JValue::Object(&channel_id)])?;
    let icon = env.get_static_field(jni_str!("android/R$drawable"), jni_str!("stat_notify_chat"), jni_sig!("I"))?.i()?;
    env.call_method(&builder, jni_str!("setSmallIcon"), jni_sig!("(I)Landroid/app/Notification$Builder;"), &[JValue::Int(icon)])?;
    let title = env.new_string(title)?;
    env.call_method(&builder, jni_str!("setContentTitle"), jni_sig!("(Ljava/lang/CharSequence;)Landroid/app/Notification$Builder;"), &[JValue::Object(&title)])?;
    let body = env.new_string(body)?;
    env.call_method(&builder, jni_str!("setContentText"), jni_sig!("(Ljava/lang/CharSequence;)Landroid/app/Notification$Builder;"), &[JValue::Object(&body)])?;
    env.call_method(&builder, jni_str!("setAutoCancel"), jni_sig!("(Z)Landroid/app/Notification$Builder;"), &[JValue::Bool(true)])?;
    // Toucher la notification : rouvrir l'application (sans aucune donnée).
    let pm = env.call_method(activity, jni_str!("getPackageManager"), jni_sig!("()Landroid/content/pm/PackageManager;"), &[])?.l()?;
    let package = env.call_method(activity, jni_str!("getPackageName"), jni_sig!("()Ljava/lang/String;"), &[])?.l()?;
    let launch = env.call_method(&pm, jni_str!("getLaunchIntentForPackage"), jni_sig!("(Ljava/lang/String;)Landroid/content/Intent;"), &[JValue::Object(&package)])?.l()?;
    if !launch.is_null() {
        // PendingIntent.FLAG_IMMUTABLE | FLAG_UPDATE_CURRENT.
        let pending = env
            .call_static_method(
                jni_str!("android/app/PendingIntent"),
                jni_str!("getActivity"),
                jni_sig!("(Landroid/content/Context;ILandroid/content/Intent;I)Landroid/app/PendingIntent;"),
                &[JValue::Object(activity), JValue::Int(0), JValue::Object(&launch), JValue::Int(0x0400_0000 | 0x0800_0000)],
            )?
            .l()?;
        env.call_method(&builder, jni_str!("setContentIntent"), jni_sig!("(Landroid/app/PendingIntent;)Landroid/app/Notification$Builder;"), &[JValue::Object(&pending)])?;
    }
    let notification = env.call_method(&builder, jni_str!("build"), jni_sig!("()Landroid/app/Notification;"), &[])?.l()?;
    env.call_method(&manager, jni_str!("notify"), jni_sig!("(ILandroid/app/Notification;)V"), &[JValue::Int(id), JValue::Object(&notification)])?;
    Ok(())
}
