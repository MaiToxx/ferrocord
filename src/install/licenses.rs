//! Textes complets des licences des dépendances, intégrés à l'exécutable
//! (`licences.json.gz`, produit par `tools/licences.ps1` à partir de
//! `cargo metadata` et des fichiers de licence de chaque paquet, dont les
//! bibliothèques C : libopus, OpenH264, aws-lc). Décompressés seulement
//! quand on ouvre Paramètres > À propos.

use serde::Deserialize;
use std::io::Read;

static EMBEDDED: &[u8] = include_bytes!("licences.json.gz");
/// Texte décompressé plus gros : refusé (le fichier en fait moins de 2 Mo).
const MAX_TEXT: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Package {
    #[serde(rename = "nom")]
    pub name: String,
    pub version: String,
    /// Expression SPDX déclarée par le paquet.
    #[serde(rename = "licence", default)]
    pub license: String,
    /// Indices dans [`Licenses::texts`] (textes identiques partagés).
    #[serde(rename = "textes", default)]
    pub texts: Vec<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Licenses {
    #[serde(rename = "paquets", default)]
    pub packages: Vec<Package>,
    #[serde(rename = "textes", default)]
    pub texts: Vec<String>,
}

impl Licenses {
    /// Textes d'un paquet (indices hors limites ignorés).
    pub fn texts_of<'a>(&'a self, p: &'a Package) -> impl Iterator<Item = &'a str> + 'a {
        p.texts.iter().filter_map(|i| self.texts.get(*i).map(String::as_str))
    }
}

pub fn decode(gz: &[u8]) -> Result<Licenses, String> {
    let mut text = Vec::new();
    flate2::read::GzDecoder::new(gz).take(MAX_TEXT).read_to_end(&mut text).map_err(|e| e.to_string())?;
    serde_json::from_slice(&text).map_err(|e| e.to_string())
}

/// Les licences intégrées à cet exécutable.
pub fn embedded() -> Result<Licenses, String> {
    decode(EMBEDDED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_file_covers_rust_and_c_dependencies() {
        let l = embedded().unwrap();
        let find = |name: &str| l.packages.iter().find(|p| p.name == name).unwrap_or_else(|| panic!("{name} absent"));
        for name in ["eframe", "tokio", "aws-lc-sys", "opusic-sys", "openh264-sys2", "serde", "resvg", "symphonia"] {
            let p = find(name);
            assert!(l.texts_of(p).next().is_some(), "{name} sans texte");
        }
        // Les licences des bibliothèques C elles-mêmes, pas seulement celles des liaisons.
        let all = |name: &str| l.texts_of(find(name)).collect::<Vec<_>>().join("\n");
        assert!(all("opusic-sys").contains("Xiph"), "licence de libopus");
        assert!(all("openh264-sys2").contains("Cisco"), "licence d'OpenH264");
        assert!(all("aws-lc-sys").contains("OpenSSL"), "licence d'aws-lc");
        assert!(l.packages.iter().all(|p| p.texts.iter().all(|i| *i < l.texts.len())));
        // Liste refaite après chaque changement de dépendances : aucun paquet sans texte
        // (15 paquets manquaient et 6 n'avaient pas de texte, audit 0.1.33).
        let bare: Vec<_> = l.packages.iter().filter(|p| p.texts.is_empty()).map(|p| p.name.as_str()).collect();
        assert!(bare.is_empty(), "sans texte de licence : {bare:?}");
    }

    #[test]
    fn damaged_data_is_an_error() {
        assert!(decode(b"pas du gzip").is_err());
    }
}
