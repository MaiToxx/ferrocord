//! Source par défaut des mises à jour : les « Releases » d'un dépôt GitHub
//! public, nommé une seule fois dans `depot.txt` (intégré à la compilation).
//!
//! Le manifeste de la dernière version est toujours à la même adresse
//! (`…/releases/latest/download/ferrocord-manifeste.json`) ; il nomme
//! l'exécutable de sa version (`…/releases/download/v1.2.3/Ferrocord-1.2.3-x64.exe`).
//! Une adresse écrite dans `local.toml` l'emporte sur celle-ci.

/// Nom du manifeste signé parmi les fichiers d'une version.
pub const MANIFEST_NAME: &str = "ferrocord-manifeste.json";
/// Canal de cet exécutable : un manifeste d'un autre canal est refusé.
pub const CHANNEL: &str = "stable";

/// `propriétaire/dépôt` de `depot.txt` ; `None` : pas de source par défaut.
pub fn repository() -> Option<&'static str> {
    parse_repository(include_str!("depot.txt"))
}

/// Première ligne hors commentaires, si elle a la forme `propriétaire/dépôt`
/// (lettres, chiffres, `-`, `_`, `.` : rien qui puisse changer l'adresse).
pub fn parse_repository(text: &str) -> Option<&str> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#'))?;
    let (owner, repo) = line.split_once('/')?;
    let plain = |s: &str| !s.is_empty() && s.len() <= 100 && s != "." && s != ".." && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    (plain(owner) && plain(repo)).then_some(line)
}

/// Adresse fixe du manifeste de la dernière version publiée de `repo`.
pub fn manifest_url(repo: &str) -> String {
    format!("https://github.com/{repo}/releases/latest/download/{MANIFEST_NAME}")
}

/// Nom de l'exécutable publié pour `version`.
pub fn binary_name(version: &str) -> String {
    format!("Ferrocord-{version}-x64.exe")
}

/// Adresse de l'exécutable de `version` dans les Releases de `repo`.
pub fn binary_url(repo: &str, version: &str) -> String {
    format!("https://github.com/{repo}/releases/download/v{version}/{}", binary_name(version))
}

/// Adresse par défaut du manifeste ; vide sans dépôt configuré.
pub fn default_manifest_url() -> String {
    repository().map(manifest_url).unwrap_or_default()
}

/// Adresse utilisée : celle de `local.toml` si elle est donnée, sinon celle
/// du dépôt configuré à la compilation ; vide : mises à jour désactivées.
pub fn effective(local: &str, default: &str) -> String {
    let local = local.trim();
    if local.is_empty() { default.trim().to_string() } else { local.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_line_is_strict() {
        assert_eq!(parse_repository("# rien\n\n"), None);
        assert_eq!(parse_repository("# dépôt\nmoncompte/ferrocord\n"), Some("moncompte/ferrocord"));
        for bad in ["moncompte", "a/b/c", "a/b?x=1", "a /b", "../b", "a/..", "https://github.com/a/b", "a/b#c", "/b", "a/"] {
            assert_eq!(parse_repository(bad), None, "{bad}");
        }
    }

    #[test]
    fn addresses() {
        assert_eq!(manifest_url("moi/ferrocord"), "https://github.com/moi/ferrocord/releases/latest/download/ferrocord-manifeste.json");
        assert_eq!(binary_url("moi/ferrocord", "1.2.3"), "https://github.com/moi/ferrocord/releases/download/v1.2.3/Ferrocord-1.2.3-x64.exe");
        assert_eq!(effective("  ", "https://d/m.json"), "https://d/m.json");
        assert_eq!(effective(" https://l/m.json ", "https://d/m.json"), "https://l/m.json");
        assert_eq!(effective("", ""), "");
        // L'adresse par défaut suit `depot.txt` : vide tant qu'il l'est.
        assert_eq!(default_manifest_url().is_empty(), repository().is_none());
    }
}
