//! Ouverture des liens externes dans le navigateur, par nous-mêmes.
//!
//! egui (`ctx.open_url`) lançait le navigateur avec tout l'environnement du
//! processus, `DISCORD_TOKEN` compris (PLAN.md, « Points ouverts ») ; et
//! `std::env::remove_var` exige désormais de l'`unsafe`. Ici, chaque lien
//! part par une [`Command`] dont l'environnement est privé de la variable.
//! Seuls les liens web, `mailto:` et les dossiers locaux sont ouverts ;
//! rien n'est passé à un interpréteur de commandes.

use std::process::Command;

/// Variable du jeton, jamais transmise au navigateur.
pub const TOKEN_VAR: &str = "DISCORD_TOKEN";

/// Lien que Ferrocord accepte de confier au système.
pub fn allowed(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    !url.chars().any(char::is_control) && url.len() <= 8192 && ["https://", "http://", "mailto:", "file:///"].iter().any(|p| lower.starts_with(p) && lower.len() > p.len())
}

/// Commande qui ouvre `url` (sans la lancer) ; `None` si le lien est refusé.
///
/// `rundll32 url.dll,FileProtocolHandler` passe le lien tel quel au
/// gestionnaire enregistré (navigateur par défaut), sans `cmd.exe` : `&`, `|`
/// ou `^` dans une adresse ne sont jamais interprétés. Un dossier local
/// (`file:///C:/…`) est ouvert par l'Explorateur, et seulement s'il existe
/// et que c'est un dossier (l'Explorateur lancerait un programme).
pub fn command(url: &str) -> Option<Command> {
    if !allowed(url) {
        return None;
    }
    let url = url.trim();
    // Même comparaison que `allowed` (sans la casse) : « FILE:///… » passait
    // le filtre puis échappait à cette branche, et le fichier partait tel
    // quel vers le gestionnaire de Windows, qui l'aurait lancé (audit 0.1.33).
    const FILE: &str = "file:///";
    let local = url.get(..FILE.len()).is_some_and(|p| p.eq_ignore_ascii_case(FILE));
    let mut cmd = if let Some(path) = url.get(FILE.len()..).filter(|_| local) {
        let path = path.replace('/', "\\");
        if path.starts_with('\\') || !std::path::Path::new(&path).is_dir() {
            return None;
        }
        let mut c = Command::new("explorer.exe");
        c.arg(path);
        c
    } else {
        let mut c = Command::new("rundll32.exe");
        c.arg("url.dll,FileProtocolHandler").arg(url);
        c
    };
    cmd.env_remove(TOKEN_VAR);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Pas de console qui clignote.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    Some(cmd)
}

/// Ouvre `url` dans un fil à part (le lancement d'un processus, et pour un
/// dossier la lecture du disque : jamais sur le fil d'interface).
pub fn open(url: &str) {
    if !allowed(url) {
        return;
    }
    let url = url.to_string();
    let _ = std::thread::Builder::new().name("ferrocord-lien".into()).spawn(move || {
        // Android : un Intent ACTION_VIEW ; seule l'adresse est transmise
        // (pas de dossier local : rien à montrer hors du stockage privé).
        #[cfg(target_os = "android")]
        if !url.trim().to_ascii_lowercase().starts_with("file:") {
            crate::platform::android::open_url(url.trim());
        }
        #[cfg(not(target_os = "android"))]
        // Navigateur introuvable : rien d'autre à faire, l'adresse reste visible au survol.
        if let Some(mut cmd) = command(&url)
            && let Ok(mut child) = cmd.spawn()
        {
            let _ = child.wait();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn token_is_removed_from_the_browser_environment() {
        let dir = format!("file:///{}", std::env::temp_dir().display().to_string().replace('\\', "/"));
        for url in ["https://example.invalid/?a=1&b=2", "mailto:x@example.invalid", dir.as_str()] {
            let cmd = command(url).unwrap_or_else(|| panic!("{url}"));
            let removed: Vec<_> = cmd.get_envs().filter(|(k, v)| v.is_none() && *k == OsStr::new(TOKEN_VAR)).collect();
            assert_eq!(removed.len(), 1, "{url}");
        }
    }

    #[test]
    fn link_is_one_argument_without_shell() {
        let cmd = command("https://example.invalid/a&calc|x^y").unwrap();
        assert_eq!(cmd.get_program(), OsStr::new("rundll32.exe"));
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args, [OsStr::new("url.dll,FileProtocolHandler"), OsStr::new("https://example.invalid/a&calc|x^y")]);
        let temp = std::env::temp_dir();
        let folder = command(&format!("file:///{}", temp.display().to_string().replace('\\', "/"))).unwrap();
        assert_eq!(folder.get_program(), OsStr::new("explorer.exe"));
        assert_eq!(folder.get_args().count(), 1);
        // Un fichier (programme) n'est jamais confié à l'Explorateur.
        let file = temp.join(format!("ferrocord-lien-{}.exe", std::process::id()));
        std::fs::write(&file, b"x").unwrap();
        assert!(command(&format!("file:///{}", file.display().to_string().replace('\\', "/"))).is_none());
        // Ni en majuscules : jamais confié au gestionnaire de liens de Windows.
        for scheme in ["FILE:///", "File:///"] {
            assert!(command(&format!("{scheme}{}", file.display().to_string().replace('\\', "/"))).is_none(), "{scheme}");
            let folder = command(&format!("{scheme}{}", temp.display().to_string().replace('\\', "/"))).unwrap();
            assert_eq!(folder.get_program(), OsStr::new("explorer.exe"), "{scheme}");
        }
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn only_web_mail_and_folders() {
        for bad in ["javascript:alert(1)", "discord://-/channels/1/2", "C:\\Windows\\calc.exe", "https://", "https://a\nb", "ms-settings:", "\\\\serveur\\partage"] {
            assert!(command(bad).is_none(), "{bad}");
        }
    }
}
