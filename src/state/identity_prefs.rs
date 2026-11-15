//! Identité annoncée à Discord : réglage de cet ordinateur (section
//! `[identite]` de `local.toml`). Décision de l'utilisateur du 14 novembre
//! 2026 : se présenter comme l'application de bureau, par défaut ; voir
//! `net::identity` et `docs/passerelle-utilisateur.md`.

/// Section de `local.toml`.
pub const SECTION: &str = "identite";
/// Le mode bureau n'existe que sous Windows (on n'annonce pas un système qu'on n'a pas).
pub const AVAILABLE: bool = cfg!(windows);
const K_DESKTOP: &str = "bureau";
const K_CLIENT_VERSION: &str = "client_version";
const K_BUILD: &str = "client_build_number";
const K_NATIVE: &str = "native_build_number";
const K_ELECTRON: &str = "electron";
const K_CHROME: &str = "chrome";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityPrefs {
    /// Se présenter comme l'application de bureau officielle (sinon : comme Ferrocord).
    pub desktop: bool,
    /// Numéros de version imposés à la main (à part : rares, et `LocalPrefs` reste petit).
    pub forced: Option<Box<ForcedVersions>>,
}

/// Numéros de version imposés ; absents : ceux de Discord (rafraîchis une
/// fois par jour) ou, à défaut, ceux embarqués.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ForcedVersions {
    pub client_version: Option<String>,
    pub client_build_number: Option<u32>,
    pub native_build_number: Option<u32>,
    pub electron: Option<String>,
    pub chrome: Option<String>,
}

impl Default for IdentityPrefs {
    fn default() -> Self {
        IdentityPrefs { desktop: true, forced: None }
    }
}

/// Numéro de version pointé (`1.0.9268`, `152.0.7977.169`) : chiffres et
/// points seulement, pour qu'une valeur du fichier n'entre jamais telle
/// quelle dans un en-tête.
pub fn dotted_version(s: &str) -> bool {
    !s.is_empty() && s.len() <= 32 && s.split('.').all(|p| !p.is_empty() && p.len() <= 6 && p.bytes().all(|b| b.is_ascii_digit()))
}

impl IdentityPrefs {
    pub fn forced(&self) -> ForcedVersions {
        self.forced.as_deref().cloned().unwrap_or_default()
    }

    pub fn to_toml(&self) -> String {
        let f = self.forced();
        let mut s = format!(
            "[{SECTION}]\n# Se présenter à Discord comme l'application de bureau (false : comme Ferrocord).\n\
             # Appliqué à la prochaine connexion. Numéros de version : imposés ici s'ils sont présents\n\
             # ({K_CLIENT_VERSION}, {K_BUILD}, {K_NATIVE}, {K_ELECTRON}, {K_CHROME}).\n{K_DESKTOP} = {}\n",
            self.desktop
        );
        let mut text = |k: &str, v: &Option<String>| {
            if let Some(v) = v.as_deref().filter(|v| dotted_version(v)) {
                s.push_str(&format!("{k} = \"{v}\"\n"));
            }
        };
        text(K_CLIENT_VERSION, &f.client_version);
        text(K_ELECTRON, &f.electron);
        text(K_CHROME, &f.chrome);
        for (k, v) in [(K_BUILD, f.client_build_number), (K_NATIVE, f.native_build_number)] {
            if let Some(v) = v {
                s.push_str(&format!("{k} = {v}\n"));
            }
        }
        s
    }

    /// Clé inconnue ou valeur illisible : ignorée.
    pub fn from_table(t: &toml::de::DeTable<'_>) -> Self {
        let mut p = IdentityPrefs::default();
        let mut f = ForcedVersions::default();
        for (k, v) in t.iter() {
            let v = v.get_ref();
            let text = || v.as_str().filter(|s| dotted_version(s)).map(str::to_string);
            let number = || v.as_integer().and_then(|i| u32::from_str_radix(i.as_str(), i.radix()).ok()).filter(|n| *n > 0);
            match k.get_ref().as_ref() {
                K_DESKTOP => {
                    if let Some(b) = v.as_bool() {
                        p.desktop = b;
                    }
                }
                K_CLIENT_VERSION => f.client_version = text(),
                K_ELECTRON => f.electron = text(),
                K_CHROME => f.chrome = text(),
                K_BUILD => f.client_build_number = number(),
                K_NATIVE => f.native_build_number = number(),
                _ => {}
            }
        }
        p.forced = (f != ForcedVersions::default()).then(|| Box::new(f));
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::local::LocalPrefs;

    #[test]
    fn desktop_by_default_and_round_trip() {
        assert!(LocalPrefs::default().identity.desktop, "bureau par défaut");
        assert!(LocalPrefs::parse("").unwrap().identity.desktop, "fichier sans section : bureau");
        let forced = ForcedVersions { client_version: Some("1.0.9300".into()), client_build_number: Some(700_000), native_build_number: Some(99_000), electron: Some("45.0.1".into()), chrome: Some("154.0.1.2".into()) };
        let p = LocalPrefs { identity: IdentityPrefs { desktop: false, forced: Some(Box::new(forced)) }, ..LocalPrefs::default() };
        assert_eq!(LocalPrefs::parse(&p.to_toml()).unwrap(), p);
    }

    #[test]
    fn unsafe_values_are_ignored() {
        let p = LocalPrefs::parse("[identite]\nbureau = \"oui\"\nclient_version = \"1.0\\r\\nX-Injecte: 1\"\nelectron = \"abc\"\nclient_build_number = -4\nnative_build_number = \"x\"\n").unwrap();
        assert_eq!(p.identity, IdentityPrefs::default());
        assert!(dotted_version("152.0.7977.169") && !dotted_version("1..2") && !dotted_version("") && !dotted_version("1.0 "));
    }
}
