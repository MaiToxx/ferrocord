//! Enregistrement local d'un appel (plugin, lot E4) : ce que l'interface
//! demande à l'orchestrateur du vocal et ce qu'il répond. Rien ici ne touche
//! au réseau : les voix reçues et le micro sont écrits dans des fichiers de
//! cet ordinateur, et seulement après un clic de l'utilisateur.

use std::path::PathBuf;

/// Demande de l'interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallRecCmd {
    /// Démarre : dossier, nom du fichier sans extension, une piste par
    /// personne en plus du mixage, mon identifiant, noms connus.
    Start { dir: PathBuf, base: String, tracks: bool, me: u64, names: Vec<(u64, String)> },
    /// Noms des participants (pour les pistes de ceux qui arrivent ensuite).
    Names(Vec<(u64, String)>),
    Stop,
}

/// Pourquoi l'enregistrement n'a pas eu lieu ou s'est arrêté.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallRecFail {
    /// Aucun appel en cours.
    NoCall,
    /// Le fichier n'a pas pu être créé dans le dossier.
    Create,
    /// Une écriture a échoué (disque plein, dossier retiré).
    Write,
}

/// Réponse de l'orchestrateur.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallRecUp {
    /// Fini : le fichier mixé, le nombre de fichiers écrits, la durée.
    Done { file: PathBuf, files: usize, ms: u64 },
    Failed(CallRecFail),
}

/// Nom de fichier sûr à partir d'un texte libre (nom de salon, pseudo) :
/// sans séparateur de chemin, caractère réservé de Windows ni point final.
pub fn file_stem(text: &str) -> String {
    let cleaned: String = text.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).take(60).collect();
    let cleaned = cleaned.trim().trim_end_matches('.').trim();
    if cleaned.is_empty() { "_".to_string() } else { cleaned.to_string() }
}

/// Durée affichée de l'enregistrement : `m:ss`, ou `h:mm:ss` au-delà d'une heure.
pub fn clock(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_are_safe_file_names() {
        assert_eq!(file_stem("Général / vocal"), "Général _ vocal");
        assert_eq!(file_stem("a<b>c:d\"e\\f|g?h*i"), "a_b_c_d_e_f_g_h_i");
        assert_eq!(file_stem("  fin.  "), "fin");
        assert_eq!(file_stem("..."), "_");
        assert_eq!(file_stem(""), "_");
        assert_eq!(file_stem(&"x".repeat(200)).chars().count(), 60);
        assert_eq!(file_stem("a\u{0}b\nc"), "a_b_c");
    }

    #[test]
    fn clock_text() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(75), "1:15");
        assert_eq!(clock(3_600 + 62), "1:01:02");
    }
}
