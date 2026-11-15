//! Usage des emojis fait dans Ferrocord (F5), gardé sur cet ordinateur : la
//! rangée « Fréquents » du sélecteur suit ce que l'on choisit ici, d'une
//! session à l'autre.
//!
//! Rien ne part chez Discord : la fréquence du compte
//! (`FrecencyUserSettings.emoji_frecency`) reste lue, jamais écrite (voir
//! `state::frecency` et `docs/passerelle-utilisateur.md` : le calcul de son
//! score n'est pas décrit par la source, et l'écrire à côté du client
//! officiel risquerait d'abîmer son classement). Les deux listes sont
//! fusionnées à l'affichage : l'usage local d'abord, puis celui du compte.
//!
//! Fichier : `emojis-<compte>.txt` dans le dossier des réglages, une ligne
//! par emoji (`clé<TAB>utilisations<TAB>dernier usage en ms`), réécrit à
//! chaque emoji choisi (un geste de l'utilisateur ; quelques kilo-octets).

use std::collections::HashMap;
use std::path::PathBuf;

/// Emojis gardés au plus (les moins bien classés sortent).
pub const MAX_KEPT: usize = 200;
/// Fichier plus gros : ignoré.
const MAX_FILE: u64 = 64 << 10;
const DAY_MS: i64 = 86_400_000;

#[derive(Debug, Default)]
pub struct LocalUse {
    /// Dossier du fichier ; `None` (tests, aperçu) : rien n'est lu ni écrit.
    pub dir: Option<PathBuf>,
    owner: Option<u64>,
    /// Clé → (utilisations, dernier usage en ms Unix).
    map: HashMap<String, (u32, i64)>,
    ranked: Vec<String>,
}

/// Clé acceptée : identifiant d'un emoji personnalisé ou nom court.
fn valid_key(k: &str) -> bool {
    !k.is_empty() && k.len() <= 64 && k.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'+' | b'-'))
}

/// Poids d'un emoji : ses utilisations, d'autant plus comptées que la
/// dernière est récente (même idée que le client officiel).
fn score(uses: u32, last_ms: i64, now_ms: i64) -> u64 {
    let weight = match (now_ms - last_ms).max(0) / DAY_MS {
        0..=3 => 100,
        4..=15 => 70,
        16..=30 => 50,
        31..=45 => 30,
        46..=80 => 10,
        _ => 1,
    };
    u64::from(uses) * weight
}

pub fn parse(text: &str) -> HashMap<String, (u32, i64)> {
    text.lines()
        .filter_map(|l| {
            let mut p = l.split('\t');
            let (k, uses, last) = (p.next()?, p.next()?.parse::<u32>().ok()?, p.next()?.parse::<i64>().ok()?);
            (valid_key(k) && uses > 0).then(|| (k.to_string(), (uses, last)))
        })
        .take(MAX_KEPT)
        .collect()
}

impl LocalUse {
    fn path(&self) -> Option<PathBuf> {
        Some(self.dir.as_ref()?.join(format!("emojis-{}.txt", self.owner?)))
    }

    /// Compte affiché : son usage est lu (une fois par changement de compte).
    /// Vrai si la liste a changé.
    pub fn bind(&mut self, me: Option<u64>, now_ms: i64) -> bool {
        if self.owner == me {
            return false;
        }
        self.owner = me;
        self.map = self
            .path()
            .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.len() <= MAX_FILE))
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| parse(&t))
            .unwrap_or_default();
        self.rank(now_ms);
        true
    }

    fn rank(&mut self, now_ms: i64) {
        let mut all: Vec<(u64, i64, &String)> = self.map.iter().map(|(k, (uses, last))| (score(*uses, *last, now_ms), *last, k)).collect();
        all.sort_by(|a, b| (b.0, b.1, a.2).cmp(&(a.0, a.1, b.2)));
        self.ranked = all.into_iter().map(|(_, _, k)| k.clone()).collect();
    }

    pub fn to_text(&self) -> String {
        self.ranked.iter().filter_map(|k| self.map.get(k).map(|(uses, last)| format!("{k}\t{uses}\t{last}\n"))).collect()
    }

    /// L'emoji `key` vient d'être choisi.
    pub fn note(&mut self, key: &str, now_ms: i64) {
        if !valid_key(key) {
            return;
        }
        let e = self.map.entry(key.to_string()).or_insert((0, now_ms));
        *e = (e.0.saturating_add(1), now_ms);
        self.rank(now_ms);
        while self.ranked.len() > MAX_KEPT {
            if let Some(k) = self.ranked.pop() {
                self.map.remove(&k);
            }
        }
        if let Some(p) = self.path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(p, self.to_text());
        }
    }

    /// Clés, les plus utilisées d'abord.
    pub fn ranked(&self) -> &[String] {
        &self.ranked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn use_counts_rank_and_recent_use_weighs_more() {
        let mut u = LocalUse::default();
        let now = 1_800_000_000_000;
        u.bind(Some(1), now);
        u.note("fire", now - 100 * DAY_MS);
        u.note("fire", now - 100 * DAY_MS);
        u.note("fire", now - 100 * DAY_MS);
        u.note("thumbsup", now);
        u.note("123456", now);
        u.note("123456", now);
        u.note("pas valide", now);
        // « fire » : trois fois, mais c'était il y a cent jours.
        u.rank(now);
        assert_eq!(u.ranked(), ["123456", "thumbsup", "fire"]);
        let back = parse(&u.to_text());
        assert_eq!(back.get("123456"), Some(&(2, now)));
        assert_eq!(back.len(), 3);
        assert!(parse("x\t0\t5\nmauvais\n../a\t1\t1\n").is_empty());
    }

    #[test]
    fn file_follows_the_account_and_nothing_is_written_without_a_folder() {
        let dir = std::env::temp_dir().join(format!("ferrocord-emojis-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut u = LocalUse { dir: Some(dir.clone()), ..Default::default() };
        u.bind(Some(7), 10);
        u.note("fire", 10);
        assert!(dir.join("emojis-7.txt").exists());
        // Autre compte : liste vide ; retour au premier : relue.
        assert!(u.bind(Some(8), 20));
        assert!(u.ranked().is_empty());
        u.bind(Some(7), 30);
        assert_eq!(u.ranked(), ["fire"]);
        let _ = std::fs::remove_dir_all(&dir);
        let mut none = LocalUse::default();
        none.bind(Some(7), 10);
        none.note("fire", 10);
        assert_eq!(none.ranked(), ["fire"]);
    }
}
