//! Paramètres nommés et nombres.

use super::Lang;
use std::fmt::{Display, Write};

/// Séparateur des milliers en français : espace insécable (egui ne coupe
/// pas une ligne sur U+00A0 ; l'espace fine U+202F, elle, serait coupée).
const FR_GROUP: char = '\u{a0}';

/// Remplace chaque `{nom}` de `template` par la valeur du paramètre de ce
/// nom. Un nom inconnu reste tel quel (un test vérifie qu'il n'y en a pas).
pub(crate) fn fill(template: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let name = &after[..close];
        match args.iter().find(|(n, _)| *n == name) {
            Some((_, v)) => {
                let _ = write!(out, "{v}");
            }
            None => out.push_str(&rest[open..open + close + 2]),
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

/// Noms des paramètres `{…}` d'un modèle, dans l'ordre.
#[cfg(test)]
pub(crate) fn placeholders(template: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else { break };
        out.push(&after[..close]);
        rest = &after[close + 1..];
    }
    out
}

/// Entier écrit selon la langue : « 12 345 » (espace insécable) ou « 12,345 ».
pub fn number(lang: Lang, n: u64) -> String {
    let digits = n.to_string();
    let sep = match lang {
        Lang::Fr => FR_GROUP,
        Lang::En => ',',
    };
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 * 2);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

/// Nombre à `decimals` décimales : virgule en français, point en anglais.
pub fn decimal(lang: Lang, x: f64, decimals: usize) -> String {
    let s = format!("{x:.decimals$}");
    match lang {
        Lang::Fr => s.replace('.', ","),
        Lang::En => s,
    }
}
