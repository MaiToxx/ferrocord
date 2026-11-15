//! Garde-fous sur les jeux de données eux-mêmes.

use serde_json::Value;

fn all() -> Vec<(String, Value)> {
    let dir = format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{dir} : {e}")).flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?} : {e}"));
            let v = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path:?} : JSON invalide : {e}"));
            out.push((path.display().to_string(), v));
        }
    }
    assert!(out.len() >= 20, "jeux de données manquants : {}", out.len());
    out
}

fn walk<'a>(v: &'a Value, f: &mut impl FnMut(Option<&'a str>, &'a Value)) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                f(Some(k), x);
                walk(x, f);
            }
        }
        Value::Array(a) => {
            for x in a {
                f(None, x);
                walk(x, f);
            }
        }
        _ => {}
    }
}

#[test]
fn fixtures_are_synthetic() {
    for (name, v) in all() {
        walk(&v, &mut |key, x| {
            // Pas de jeton, même factice, dans les jeux de données.
            assert!(key != Some("token"), "{name} : champ token");
            if let Value::String(s) = x {
                // Identifiants inventés : tous dans les plages 1…–7… de 19 chiffres.
                if key.is_some_and(|k| k == "id" || k.ends_with("_id")) && s.chars().all(|c| c.is_ascii_digit()) {
                    assert_eq!(s.len(), 19, "{name} : identifiant {s} hors des plages fictives");
                    assert!(matches!(s.as_bytes()[0], b'1'..=b'7'), "{name} : identifiant {s} hors des plages fictives");
                }
                // Toute adresse pointe vers un domaine réservé ou un chemin fictif.
                // Exception (lot G) : la règle des hôtes d'images n'accepte que le
                // vrai `media.tenor.com` ; ses jeux le nomment donc, avec un chemin
                // inventé (`synthetique…`, fiche `…-gif-7000…`). Rien n'y est demandé.
                if s.starts_with("http") || s.starts_with("wss") {
                    let tenor = s.contains("://media.tenor.com/synthetique") || (s.starts_with("https://tenor.com/view/") && s.contains("-gif-7000000000000000"));
                    assert!(
                        tenor || s.contains(".invalid") || s.contains("/7000000000000000") || s.contains("/fictif/"),
                        "{name} : adresse non fictive {s}"
                    );
                }
            }
        });
    }
}

#[test]
fn gateway_fixtures_have_envelope() {
    for (name, v) in all() {
        // Réponses REST (`rest_*.json`) : pas d'enveloppe de passerelle.
        if name.ends_with("rest_history.json") || name.ends_with("rest_reactors.json") {
            assert!(v.is_array(), "{name}");
            continue;
        }
        if name.ends_with("rest_pins.json") {
            assert!(v["items"].is_array() && v["has_more"].is_boolean(), "{name}");
            continue;
        }
        // Corps de réponse REST (dont les réglages du compte) : un objet ou une
        // liste, sans enveloppe de passerelle.
        if name.rsplit(['/', '\\']).next().is_some_and(|f| f.starts_with("rest_")) {
            assert!(v.is_array() || (v.is_object() && v.get("op").is_none()), "{name}");
            continue;
        }
        assert_eq!(v["op"], 0, "{name}");
        assert!(v["t"].is_string() && v["s"].is_u64() && v["d"].is_object(), "{name} : enveloppe op/t/s/d");
    }
}
