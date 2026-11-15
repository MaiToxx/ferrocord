//! Point d'entrée de l'APK : la NativeActivity charge `libferrocord_android.so`
//! et android-activity appelle `android_main` dans son propre fil.

#[cfg(target_os = "android")]
// SAFETY: `no_mangle` exporte le symbole `android_main` attendu par
// android-activity (crate de winit) ; aucune autre fonction ne porte ce nom.
#[unsafe(no_mangle)]
fn android_main(app: ferrocord::platform::android::AndroidApp) {
    ferrocord::platform::android::main(app);
}
