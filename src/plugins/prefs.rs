//! État des plugins dans `local.toml` : section `[plugins]` (une ligne
//! `clé = true|false` par plugin) et `[plugins.reglages.<clé>]` pour les
//! réglages propres. Clé inconnue ou valeur illisible : ignorée.

use super::{by_key, get, Id, Kind, Setting, ALL, DEFAULT_BITS};
use toml::de::{DeTable, DeValue};

pub const SECTION: &str = "plugins";
const S_VALUES: &str = "reglages";

/// Valeur d'un réglage de plugin (un choix est gardé par sa clé).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Str(String),
}

/// Plugins activés et réglages changés. Ce que veut l'utilisateur : pour un
/// plugin à redémarrage, l'état en vigueur est celui de [`super::on`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginPrefs {
    bits: u64,
    /// Réglages qui ont une valeur, triés par plugin puis par clé.
    values: Vec<(Id, &'static str, Value)>,
}

impl Default for PluginPrefs {
    fn default() -> Self {
        PluginPrefs { bits: DEFAULT_BITS, values: Vec::new() }
    }
}

/// `v` ramenée à la forme de `s` (bornes, choix connus, longueur) ; `None` : pas du bon type.
fn fitted(s: &Setting, v: Value) -> Option<Value> {
    match (s.kind, v) {
        (Kind::Bool(_), Value::Bool(b)) => Some(Value::Bool(b)),
        (Kind::Number { min, max, .. }, Value::Int(n)) => Some(Value::Int(n.clamp(min, max))),
        (Kind::Choice(options, _), Value::Str(k)) => options.iter().any(|(o, _)| *o == k).then_some(Value::Str(k)),
        (Kind::Text { max, .. }, Value::Str(t)) => Some(Value::Str(t.chars().filter(|c| !c.is_control()).take(max).collect())),
        _ => None,
    }
}

impl PluginPrefs {
    /// Un bit par plugin, au rang de son identifiant.
    pub fn bits(&self) -> u64 {
        self.bits
    }

    pub fn on(&self, id: Id) -> bool {
        self.bits & (1 << id as u8) != 0
    }

    pub fn set(&mut self, id: Id, on: bool) {
        if on {
            self.bits |= 1 << id as u8;
        } else {
            self.bits &= !(1 << id as u8);
        }
    }

    /// Nombre de plugins activés.
    pub fn count(&self) -> usize {
        ALL.iter().filter(|p| self.on(p.id)).count()
    }

    pub fn disable_all(&mut self) {
        self.bits = 0;
    }

    fn raw(&self, id: Id, key: &str) -> Option<&Value> {
        self.values.iter().find(|(i, k, _)| *i == id && *k == key).map(|(_, _, v)| v)
    }

    /// Valeur du réglage `s` du plugin : celle choisie, sinon celle par défaut.
    pub fn value(&self, id: Id, s: &Setting) -> Value {
        if let Some(v) = self.raw(id, s.key) {
            return v.clone();
        }
        match s.kind {
            Kind::Bool(b) => Value::Bool(b),
            Kind::Choice(_, d) => Value::Str(d.to_string()),
            Kind::Number { default, .. } => Value::Int(default),
            Kind::Text { default, .. } => Value::Str(default.to_string()),
        }
    }

    /// Règle `key` (réglage déclaré par le plugin ; valeur d'un autre type : ignorée).
    pub fn put(&mut self, id: Id, key: &str, v: Value) {
        let Some(s) = get(id).setting(key) else {
            debug_assert!(false, "réglage non déclaré : {}.{key}", get(id).key);
            return;
        };
        self.put_declared(id, s, v);
    }

    /// Comme [`PluginPrefs::put`], pour un réglage dont on tient la déclaration.
    pub fn put_declared(&mut self, id: Id, s: &Setting, v: Value) {
        let Some(v) = fitted(s, v) else { return };
        match self.values.iter_mut().find(|(i, k, _)| *i == id && *k == s.key) {
            Some(slot) => slot.2 = v,
            None => {
                self.values.push((id, s.key, v));
                self.values.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
            }
        }
    }

    /// Rend au réglage sa valeur par défaut.
    pub fn clear(&mut self, id: Id, key: &str) {
        self.values.retain(|(i, k, _)| !(*i == id && *k == key));
    }

    fn declared(id: Id, key: &str) -> Option<&'static Setting> {
        let s = get(id).setting(key);
        debug_assert!(s.is_some(), "réglage non déclaré : {}.{key}", get(id).key);
        s
    }

    pub fn flag(&self, id: Id, key: &str) -> bool {
        match (self.raw(id, key), Self::declared(id, key).map(|s| s.kind)) {
            (Some(Value::Bool(b)), _) => *b,
            (_, Some(Kind::Bool(b))) => b,
            _ => false,
        }
    }

    pub fn number(&self, id: Id, key: &str) -> i64 {
        match (self.raw(id, key), Self::declared(id, key).map(|s| s.kind)) {
            (Some(Value::Int(n)), _) => *n,
            (_, Some(Kind::Number { default, .. })) => default,
            _ => 0,
        }
    }

    /// Clé de l'option choisie (une des clés déclarées).
    pub fn choice(&self, id: Id, key: &str) -> &'static str {
        let Some(Kind::Choice(options, default)) = Self::declared(id, key).map(|s| s.kind) else { return "" };
        match self.raw(id, key) {
            Some(Value::Str(k)) => options.iter().find(|(o, _)| o == k).map_or(default, |(o, _)| o),
            _ => default,
        }
    }

    pub fn text(&self, id: Id, key: &str) -> &str {
        match (self.raw(id, key), Self::declared(id, key).map(|s| s.kind)) {
            (Some(Value::Str(t)), _) => t,
            (_, Some(Kind::Text { default, .. })) => default,
            _ => "",
        }
    }

    /// Section `[plugins]` de `local.toml`.
    pub fn to_toml(&self) -> String {
        use std::fmt::Write as _;
        let mut out = format!("[{SECTION}]\n# Fonctions propres à Ferrocord (Paramètres > Plugins).\n");
        for p in ALL {
            let _ = writeln!(out, "{} = {}", p.key, self.on(p.id));
        }
        for p in ALL {
            let mut lines = self.values.iter().filter(|(i, _, _)| *i == p.id).peekable();
            if lines.peek().is_some() {
                let _ = writeln!(out, "\n[{SECTION}.{S_VALUES}.{}]", p.key);
            }
            for (_, key, v) in lines {
                let _ = match v {
                    Value::Bool(b) => writeln!(out, "{key} = {b}"),
                    Value::Int(n) => writeln!(out, "{key} = {n}"),
                    Value::Str(s) => writeln!(out, "{key} = {}", quoted(s)),
                };
            }
        }
        out
    }

    /// Lit la section dans `self` : ce qui n'y est pas garde sa valeur.
    pub fn read_table(&mut self, t: &DeTable<'_>) {
        for (k, v) in t.iter() {
            let k: &str = k.get_ref();
            let v = v.get_ref();
            if k == S_VALUES {
                let Some(per_plugin) = v.as_table() else { continue };
                for (pk, pv) in per_plugin.iter() {
                    let pk: &str = pk.get_ref();
                    let (Some(plugin), Some(values)) = (by_key(pk), pv.get_ref().as_table()) else { continue };
                    for (sk, sv) in values.iter() {
                        let sk: &str = sk.get_ref();
                        if let (Some(s), Some(v)) = (plugin.setting(sk), value_of(sv.get_ref())) {
                            self.put_declared(plugin.id, s, v);
                        }
                    }
                }
            } else if let (Some(plugin), Some(on)) = (by_key(k), v.as_bool()) {
                self.set(plugin.id, on);
            }
        }
    }
}

fn value_of(v: &DeValue<'_>) -> Option<Value> {
    if let Some(b) = v.as_bool() {
        Some(Value::Bool(b))
    } else if let Some(i) = v.as_integer() {
        i64::from_str_radix(i.as_str(), i.radix()).ok().map(Value::Int)
    } else {
        v.as_str().map(|s| Value::Str(s.to_string()))
    }
}

fn quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars().filter(|c| !c.is_control()) {
        if matches!(c, '"' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::super::READ_ALOUD_AUTHOR;
    use super::*;
    use crate::i18n::t as it;

    fn parse(text: &str) -> PluginPrefs {
        let root = DeTable::parse(text).unwrap();
        let mut p = PluginPrefs::default();
        if let Some((_, v)) = root.get_ref().iter().find(|(k, _)| k.get_ref() == SECTION) {
            p.read_table(v.get_ref().as_table().unwrap());
        }
        p
    }

    #[test]
    fn states_and_settings_round_trip() {
        let mut p = PluginPrefs::default();
        p.set(Id::Backdrop, false);
        p.set(Id::RichPresence, false);
        p.put(Id::ReadAloud, READ_ALOUD_AUTHOR, Value::Bool(false));
        let text = p.to_toml();
        assert_eq!(parse(&text), p, "{text}");
        assert_eq!(p.count(), ALL.iter().filter(|p| p.default_on).count() - 2);
        assert!(!p.flag(Id::ReadAloud, READ_ALOUD_AUTHOR));
        p.clear(Id::ReadAloud, READ_ALOUD_AUTHOR);
        assert!(p.flag(Id::ReadAloud, READ_ALOUD_AUTHOR), "valeur par défaut");
        p.disable_all();
        assert_eq!((p.count(), parse(&p.to_toml()).bits()), (0, 0));
    }

    #[test]
    fn unknown_keys_and_bad_values_are_ignored() {
        let odd = "[plugins]\ninconnu = false\nanimations = 3\nfond_personnalise = false\n[plugins.reglages.lecture_voix_haute]\nannoncer_auteur = \"oui\"\nautre = 1\n[plugins.reglages.inconnu]\nx = 1\n";
        let p = parse(odd);
        assert!(p.on(Id::Animations) && !p.on(Id::Backdrop));
        assert!(p.flag(Id::ReadAloud, READ_ALOUD_AUTHOR));
        assert_eq!(parse(""), PluginPrefs::default());
    }

    /// Les quatre formes de réglage : bornes, choix connus, longueur.
    #[test]
    fn values_fit_their_declaration() {
        const MODES: &[(&str, crate::i18n::Text)] = &[("a", it::SAVE), ("b", it::CANCEL)];
        let flag = Setting { key: "f", label: it::SAVE, detail: it::SAVE, kind: Kind::Bool(false) };
        let pick = Setting { key: "c", label: it::SAVE, detail: it::SAVE, kind: Kind::Choice(MODES, "a") };
        let num = Setting { key: "n", label: it::SAVE, detail: it::SAVE, kind: Kind::Number { min: 1, max: 10, default: 5 } };
        let text = Setting { key: "t", label: it::SAVE, detail: it::SAVE, kind: Kind::Text { default: "x", max: 4 } };
        let id = Id::ReadAloud;
        let mut p = PluginPrefs::default();
        assert_eq!((p.value(id, &flag), p.value(id, &pick), p.value(id, &num), p.value(id, &text)), (Value::Bool(false), Value::Str("a".into()), Value::Int(5), Value::Str("x".into())));
        p.put_declared(id, &num, Value::Int(99));
        p.put_declared(id, &pick, Value::Str("b".into()));
        p.put_declared(id, &pick, Value::Str("zzz".into()));
        p.put_declared(id, &text, Value::Str("abc\ndef".into()));
        p.put_declared(id, &flag, Value::Int(1));
        assert_eq!((p.value(id, &flag), p.value(id, &pick), p.value(id, &num), p.value(id, &text)), (Value::Bool(false), Value::Str("b".into()), Value::Int(10), Value::Str("abcd".into())));
    }
}
