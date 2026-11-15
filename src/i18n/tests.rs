use super::fill::{fill, placeholders};
use super::*;
use chrono::{FixedOffset, NaiveDate, Utc};

/// Préfixe des clés des textes relus mot à mot (`verbatim.rs`).
const VERBATIM: &str = "verbatim::";

/// Toutes les chaînes de la table, avec leur clé, par langue.
pub(crate) fn every_string() -> Vec<(String, Lang, &'static str)> {
    let mut out = Vec::new();
    for lang in Lang::ALL {
        let l = lang.index();
        for (k, v) in t::ALL {
            out.push((format!("t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in f::ALL {
            out.push((format!("f::{k}"), lang, v[l]));
        }
        for (k, v) in verbatim::t::ALL {
            out.push((format!("{VERBATIM}t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in verbatim::f::ALL {
            out.push((format!("{VERBATIM}f::{k}"), lang, v[l]));
        }
        for (k, v) in table::s::t::ALL {
            out.push((format!("s::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::s::f::ALL {
            out.push((format!("s::f::{k}"), lang, v[l]));
        }
        for (k, v) in p::ALL {
            out.push((format!("p::{k} (singulier)"), lang, v[l][0]));
            out.push((format!("p::{k} (pluriel)"), lang, v[l][1]));
        }
        for (k, v) in table::social::t::ALL {
            out.push((format!("social::t::{k}"), lang, v.get(lang)));
        }
        for (k, v) in table::emoji::t::ALL {
            out.push((format!("emoji::t::{k}"), lang, v.get(lang)));
        }
        for (k, v) in table::gifs::t::ALL {
            out.push((format!("gifs::t::{k}"), lang, v.get(lang)));
        }
        for (k, v) in table::ff::t::ALL {
            out.push((format!("ff::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::ff::f::ALL {
            out.push((format!("ff::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::ff::p::ALL {
            out.push((format!("ff::p::{k} (singulier)"), lang, v[l][0]));
            out.push((format!("ff::p::{k} (pluriel)"), lang, v[l][1]));
        }
        for (k, v) in table::nd::t::ALL {
            out.push((format!("nd::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::nd::f::ALL {
            out.push((format!("nd::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::nd::p::ALL {
            out.push((format!("nd::p::{k} (singulier)"), lang, v[l][0]));
            out.push((format!("nd::p::{k} (pluriel)"), lang, v[l][1]));
        }
        for (k, v) in table::lotv::t::ALL {
            out.push((format!("lotv::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::lotv::f::ALL {
            out.push((format!("lotv::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::lotv::p::ALL {
            out.push((format!("lotv::p::{k} (singulier)"), lang, v[l][0]));
            out.push((format!("lotv::p::{k} (pluriel)"), lang, v[l][1]));
        }
        for (k, _, v) in table::social::f::ALL {
            out.push((format!("social::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::social::p::ALL {
            out.push((format!("social::p::{k} (singulier)"), lang, v[l][0]));
            out.push((format!("social::p::{k} (pluriel)"), lang, v[l][1]));
        }
        for (k, v) in table::upload::t::ALL {
            out.push((format!("upload::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::upload::f::ALL {
            out.push((format!("upload::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::upload::p::ALL {
            out.push((format!("upload::p::{k} (singulier)"), lang, v[l][0]));
            out.push((format!("upload::p::{k} (pluriel)"), lang, v[l][1]));
        }
        for (k, v) in table::s2::t::ALL {
            out.push((format!("s2::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::s2::f::ALL {
            out.push((format!("s2::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::notify::t::ALL {
            out.push((format!("notify::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::notify::f::ALL {
            out.push((format!("notify::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::notify::p::ALL {
            out.push((format!("notify::p::{k} (singulier)"), lang, v[l][0]));
            out.push((format!("notify::p::{k} (pluriel)"), lang, v[l][1]));
        }
        for (k, v) in table::threads::t::ALL {
            out.push((format!("threads::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::threads::f::ALL {
            out.push((format!("threads::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::threads::p::ALL {
            out.push((format!("threads::p::{k} (singulier)"), lang, v[l][0]));
            out.push((format!("threads::p::{k} (pluriel)"), lang, v[l][1]));
        }
        for (k, v) in table::polls::t::ALL {
            out.push((format!("polls::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::polls::f::ALL {
            out.push((format!("polls::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::polls::p::ALL {
            out.push((format!("polls::p::{k} (singulier)"), lang, v[l][0]));
            out.push((format!("polls::p::{k} (pluriel)"), lang, v[l][1]));
        }
        for (k, v) in table::admin::t::ALL {
            out.push((format!("admin::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::admin::f::ALL {
            out.push((format!("admin::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::admin::p::ALL {
            out.push((format!("admin::p::{k} (singulier)"), lang, v[l][0]));
            out.push((format!("admin::p::{k} (pluriel)"), lang, v[l][1]));
        }
        for (k, v) in table::profile::t::ALL {
            out.push((format!("profile::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::profile::f::ALL {
            out.push((format!("profile::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::quick::t::ALL {
            out.push((format!("quick::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::quick::f::ALL {
            out.push((format!("quick::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::restes::t::ALL {
            out.push((format!("restes::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::restes::f::ALL {
            out.push((format!("restes::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::people::t::ALL {
            out.push((format!("people::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::people::f::ALL {
            out.push((format!("people::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::profile::p::ALL {
            out.push((format!("profile::p::{k} (singulier)"), lang, v[l][0]));
            out.push((format!("profile::p::{k} (pluriel)"), lang, v[l][1]));
        }
        for (k, v) in table::guilds::t::ALL {
            out.push((format!("guilds::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::guilds::f::ALL {
            out.push((format!("guilds::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::guilds::p::ALL {
            out.push((format!("guilds::p::{k} (singulier)"), lang, v[l][0]));
            out.push((format!("guilds::p::{k} (pluriel)"), lang, v[l][1]));
        }
        for (k, v) in table::events::t::ALL {
            out.push((format!("events::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::events::f::ALL {
            out.push((format!("events::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::events::p::ALL {
            out.push((format!("events::p::{k} (singulier)"), lang, v[l][0]));
            out.push((format!("events::p::{k} (pluriel)"), lang, v[l][1]));
        }
        for (k, v) in table::expressions::t::ALL {
            out.push((format!("expressions::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::expressions::f::ALL {
            out.push((format!("expressions::f::{k}"), lang, v[l]));
        }
        // Lot V6.
        for (k, v) in table::calls::t::ALL {
            out.push((format!("calls::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::calls::f::ALL {
            out.push((format!("calls::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::calls::p::ALL {
            out.push((format!("calls::p::{k} (singulier)"), lang, v[l][0]));
            out.push((format!("calls::p::{k} (pluriel)"), lang, v[l][1]));
        }
        // Lot M4.
        for (k, v) in table::video::t::ALL {
            out.push((format!("video::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::video::f::ALL {
            out.push((format!("video::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::video::p::ALL {
            out.push((format!("video::p::{k} (singulier)"), lang, v[l][0]));
            out.push((format!("video::p::{k} (pluriel)"), lang, v[l][1]));
        }
        // Lot V5.
        for (k, v) in table::audio::t::ALL {
            out.push((format!("audio::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::audio::f::ALL {
            out.push((format!("audio::f::{k}"), lang, v[l]));
        }
        // Lot C6.
        for (k, v) in table::install::t::ALL {
            out.push((format!("install::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::install::f::ALL {
            out.push((format!("install::f::{k}"), lang, v[l]));
        }
        // Lot C3.
        for (k, v) in table::activity::t::ALL {
            out.push((format!("activity::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::activity::f::ALL {
            out.push((format!("activity::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::activity::p::ALL {
            out.push((format!("activity::p::{k} (singulier)"), lang, v[l][0]));
            out.push((format!("activity::p::{k} (pluriel)"), lang, v[l][1]));
        }
        // Lot R.
        for (k, v) in table::rpc::t::ALL {
            out.push((format!("rpc::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::rpc::f::ALL {
            out.push((format!("rpc::f::{k}"), lang, v[l]));
        }
        // Lot M3.
        for (k, v) in table::share::t::ALL {
            out.push((format!("share::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::share::f::ALL {
            out.push((format!("share::f::{k}"), lang, v[l]));
        }
        // Phase 6 : messages vocaux.
        for (k, v) in table::voice_msg::t::ALL {
            out.push((format!("voice_msg::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::voice_msg::f::ALL {
            out.push((format!("voice_msg::f::{k}"), lang, v[l]));
        }
        // Lot C2.
        for (k, v) in table::qr::t::ALL {
            out.push((format!("qr::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::qr::f::ALL {
            out.push((format!("qr::f::{k}"), lang, v[l]));
        }
        // Phase 10, lot M.
        for (k, v) in table::messages::t::ALL {
            out.push((format!("messages::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::messages::f::ALL {
            out.push((format!("messages::f::{k}"), lang, v[l]));
        }
        // Phase 7, lot P2.
        for (k, v) in table::mobile::t::ALL {
            out.push((format!("mobile::t::{k}"), lang, v.get(lang)));
        }
        // Lot H.
        for (k, v) in table::offline::t::ALL {
            out.push((format!("offline::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::offline::f::ALL {
            out.push((format!("offline::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::search::t::ALL {
            out.push((format!("search::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::search::f::ALL {
            out.push((format!("search::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::voice::t::ALL {
            out.push((format!("voice::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::voice::f::ALL {
            out.push((format!("voice::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::search::p::ALL {
            out.push((format!("search::p::{k} (singulier)"), lang, v[l][0]));
            out.push((format!("search::p::{k} (pluriel)"), lang, v[l][1]));
        }
        for (k, v) in table::lotn::t::ALL {
            out.push((format!("lotn::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::lotn::f::ALL {
            out.push((format!("lotn::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::plug::t::ALL {
            out.push((format!("plug::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::plug::f::ALL {
            out.push((format!("plug::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::e4::t::ALL {
            out.push((format!("e4::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::e4::f::ALL {
            out.push((format!("e4::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::e3::t::ALL {
            out.push((format!("e3::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::e3::f::ALL {
            out.push((format!("e3::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::e2::t::ALL {
            out.push((format!("e2::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::e2::f::ALL {
            out.push((format!("e2::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::journal::t::ALL {
            out.push((format!("journal::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::journal::f::ALL {
            out.push((format!("journal::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::e5::t::ALL {
            out.push((format!("e5::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::e5::f::ALL {
            out.push((format!("e5::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::perso::t::ALL {
            out.push((format!("perso::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::perso::f::ALL {
            out.push((format!("perso::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::nav::t::ALL {
            out.push((format!("nav::t::{k}"), lang, v.get(lang)));
        }
        for (k, v) in table::a11y::t::ALL {
            out.push((format!("a11y::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::a11y::f::ALL {
            out.push((format!("a11y::f::{k}"), lang, v[l]));
        }
        for (k, _, v) in table::nav::f::ALL {
            out.push((format!("nav::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::interactions::t::ALL {
            out.push((format!("interactions::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::interactions::f::ALL {
            out.push((format!("interactions::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::chadmin::t::ALL {
            out.push((format!("chadmin::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::chadmin::f::ALL {
            out.push((format!("chadmin::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::chadmin::p::ALL {
            out.push((format!("chadmin::p::{k} (singulier)"), lang, v[l][0]));
            out.push((format!("chadmin::p::{k} (pluriel)"), lang, v[l][1]));
        }
        for (k, v) in table::roles::t::ALL {
            out.push((format!("roles::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::roles::f::ALL {
            out.push((format!("roles::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::roles::p::ALL {
            out.push((format!("roles::p::{k} (singulier)"), lang, v[l][0]));
            out.push((format!("roles::p::{k} (pluriel)"), lang, v[l][1]));
        }
        for (k, v) in table::moderation::t::ALL {
            out.push((format!("moderation::t::{k}"), lang, v.get(lang)));
        }
        for (k, _, v) in table::moderation::f::ALL {
            out.push((format!("moderation::f::{k}"), lang, v[l]));
        }
        for (k, v) in table::moderation::p::ALL {
            out.push((format!("moderation::p::{k} (singulier)"), lang, v[l][0]));
            out.push((format!("moderation::p::{k} (pluriel)"), lang, v[l][1]));
        }
        for (g, k, v) in tokens::ALL {
            out.push((format!("tokens::{g}::{k} (libellé)"), lang, v.label(lang)));
            out.push((format!("tokens::{g}::{k} (explication)"), lang, v.detail(lang)));
        }
        for d in 0..7 {
            out.push((format!("jour {d}"), lang, time::weekday(lang, d)));
        }
        for m in 0..12 {
            out.push((format!("mois {m}"), lang, time::month(lang, m)));
        }
        for c in Choice::ALL {
            out.push((format!("langue {c:?}"), lang, c.label(lang)));
        }
    }
    out
}

/// Aucune clé vide, dans aucune langue (une clé absente ne compile pas).
#[test]
fn every_key_is_translated() {
    let empty: Vec<String> = every_string().into_iter().filter(|(_, _, s)| s.trim().is_empty()).map(|(k, l, _)| format!("{k} ({l:?})")).collect();
    assert!(empty.is_empty(), "textes vides : {empty:?}");
    assert!(t::ALL.len() > 100, "{}", t::ALL.len());
}

/// Les deux langues d'un modèle emploient exactement ses paramètres nommés.
#[test]
fn template_parameters_match() {
    let mut bad = Vec::new();
    let verbatim_f = verbatim::f::ALL.iter().map(|(k, a, t)| (format!("{VERBATIM}{k}"), *a, *t));
    let settings_f = table::s::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t));
    let social_f = table::social::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t));
    let upload_f = table::upload::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t));
    let notify_f = table::notify::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t));
    let profile_f = table::profile::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t));
    for (k, params, texts) in f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t)).chain(verbatim_f).chain(settings_f).chain(social_f).chain(upload_f).chain(notify_f).chain(table::threads::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t))).chain(profile_f).chain(table::guilds::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t))).chain(table::search::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t))).chain(table::nav::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t))).chain(table::interactions::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t))).chain(table::polls::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t))).chain(table::admin::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t))).chain(table::chadmin::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t))).chain(table::roles::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t))).chain(table::moderation::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t))).chain(table::events::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t))).chain(table::expressions::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t))).chain(table::voice::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t))).chain(table::audio::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t))).chain(table::share::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t))).chain(table::qr::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t))).chain(table::quick::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t))).chain(table::lotn::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t))).chain(table::perso::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t))).chain(table::plug::f::ALL.iter().map(|(k, a, t)| (k.to_string(), *a, *t))) {
        for lang in Lang::ALL {
            let mut used = placeholders(texts[lang.index()]);
            used.sort_unstable();
            used.dedup();
            let mut want = params.to_vec();
            want.sort_unstable();
            if used != want {
                bad.push(format!("f::{k} ({lang:?}) : {used:?} au lieu de {want:?}"));
            }
        }
    }
    for (k, forms) in p::ALL.iter().chain(table::social::p::ALL).chain(table::upload::p::ALL).chain(table::notify::p::ALL).chain(table::profile::p::ALL).chain(table::guilds::p::ALL).chain(table::search::p::ALL).chain(table::polls::p::ALL).chain(table::admin::p::ALL).chain(table::chadmin::p::ALL).chain(table::roles::p::ALL).chain(table::events::p::ALL) {
        for form in forms.iter().flatten() {
            if placeholders(form) != ["n"] {
                bad.push(format!("p::{k} : {form:?}"));
            }
        }
    }
    for (k, v) in t::ALL.iter().chain(verbatim::t::ALL).chain(table::social::t::ALL).chain(table::upload::t::ALL).chain(table::notify::t::ALL).chain(table::profile::t::ALL).chain(table::guilds::t::ALL).chain(table::nav::t::ALL).chain(table::polls::t::ALL).chain(table::admin::t::ALL).chain(table::chadmin::t::ALL).chain(table::roles::t::ALL).chain(table::events::t::ALL).chain(table::expressions::t::ALL).chain(table::voice::t::ALL).chain(table::audio::t::ALL).chain(table::share::t::ALL).chain(table::qr::t::ALL).chain(table::mobile::t::ALL).chain(table::quick::t::ALL).chain(table::lotn::t::ALL).chain(table::perso::t::ALL).chain(table::plug::t::ALL) {
        for lang in Lang::ALL {
            if !placeholders(v.get(lang)).is_empty() {
                bad.push(format!("t::{k} ({lang:?}) : paramètre dans un texte constant"));
            }
        }
    }
    // Lot V6.
    for (k, params, texts) in table::calls::f::ALL {
        for lang in Lang::ALL {
            let mut used = placeholders(texts[lang.index()]);
            used.sort_unstable();
            used.dedup();
            let mut want = params.to_vec();
            want.sort_unstable();
            if used != want {
                bad.push(format!("calls::f::{k} ({lang:?}) : {used:?} au lieu de {want:?}"));
            }
        }
    }
    for (k, v) in table::calls::t::ALL {
        if Lang::ALL.iter().any(|l| !placeholders(v.get(*l)).is_empty()) {
            bad.push(format!("calls::t::{k} : paramètre dans un texte constant"));
        }
    }
    // Lot C5.
    for (k, params, texts) in table::a11y::f::ALL {
        for lang in Lang::ALL {
            let mut used = placeholders(texts[lang.index()]);
            used.sort_unstable();
            used.dedup();
            let mut want = params.to_vec();
            want.sort_unstable();
            if used != want {
                bad.push(format!("a11y::f::{k} ({lang:?}) : {used:?} au lieu de {want:?}"));
            }
        }
    }
    for (k, v) in table::a11y::t::ALL {
        if Lang::ALL.iter().any(|l| !placeholders(v.get(*l)).is_empty()) {
            bad.push(format!("a11y::t::{k} : paramètre dans un texte constant"));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// Ponctuation propre à chaque langue : en français, espace insécable avant
/// « : ; ? ! » (ni espace ordinaire ni absence d'espace) et dans les
/// guillemets ; en anglais, ni espace insécable ni guillemets français.
/// Le français des textes relus mot à mot garde sa typographie d'origine.
#[test]
fn punctuation_follows_each_language() {
    let mut bad = Vec::new();
    for (k, lang, s) in every_string() {
        let wrong = match lang {
            Lang::Fr if k.starts_with(VERBATIM) => false,
            Lang::Fr => {
                let chars: Vec<char> = s.chars().collect();
                // « : » suivi d'un chiffre (rapport « 4.5:1 ») n'est pas une ponctuation.
                let unspaced = chars.iter().enumerate().any(|(i, c)| {
                    ":;?!".contains(*c) && !chars.get(i + 1).is_some_and(char::is_ascii_digit) && (i == 0 || chars[i - 1] != '\u{a0}')
                });
                unspaced || ["« ", " »"].iter().any(|p| s.contains(p))
            }
            Lang::En => s.contains('\u{a0}') || s.contains('«') || s.contains('»') || [" :", " ;", " ?", " !"].iter().any(|p| s.contains(p)),
        };
        if wrong {
            bad.push(format!("{k} ({lang:?}) : {s:?}"));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn parameters_are_filled() {
    assert_eq!(f::disconnected(Lang::Fr, "fermé"), "Déconnecté (fermé), nouvelle tentative…");
    assert_eq!(f::disconnected(Lang::En, "closed"), "Disconnected (closed), retrying…");
    assert_eq!(f::not_sent(Lang::Fr, "x"), "Message non envoyé\u{a0}: x");
    assert_eq!(f::not_sent(Lang::En, "x"), "Message not sent: x");
    assert_eq!(f::low_contrast(Lang::En, "text", "2.0", "4.5"), "Low contrast: text on the main background = 2.0:1 (at least 4.5:1 recommended)");
    assert_eq!(f::theme_not_section(Lang::Fr, "x"), "«\u{a0}x\u{a0}» doit être une section [x]");
    // Valeur reprise telle quelle, même si elle contient des accolades.
    assert_eq!(f::message_channel(Lang::En, "{name}"), "Message #{name}");
    // Comme Discord : pas d'espace après « # » ni « @ ».
    assert_eq!(f::message_channel(Lang::Fr, "général"), "Envoyer un message dans #général");
    assert_eq!(f::message_dm(Lang::Fr, "Alice"), "Envoyer un message à @Alice");
    assert_eq!(f::message_dm(Lang::En, "Alice"), "Message @Alice");
    // Sans accord à faire, au singulier comme au pluriel.
    assert_eq!(f::theme_color_count(Lang::Fr, 2, 1), "nombre de couleurs attendu\u{a0}: 2, trouvé\u{a0}: 1");
    assert_eq!(f::theme_number_expected(Lang::Fr, t::KIND_ARRAY.get(Lang::Fr)), "nombre attendu, trouvé\u{a0}: une liste");
    assert_eq!(f::theme_number_expected(Lang::En, t::KIND_ARRAY.get(Lang::En)), "number expected, found: an array");
    assert_eq!(fill("a {x} b {y} {", &[("x", &1)]), "a 1 b {y} {");
}

#[test]
fn plurals_in_both_languages() {
    let fr = |n| p::new_messages(Lang::Fr, n);
    let en = |n| p::new_messages(Lang::En, n);
    assert_eq!(fr(0), "0 nouveau message");
    assert_eq!(fr(1), "1 nouveau message");
    assert_eq!(fr(2), "2 nouveaux messages");
    assert_eq!(fr(10_000), "10\u{a0}000 nouveaux messages");
    assert_eq!(fr(1_234_567), "1\u{a0}234\u{a0}567 nouveaux messages");
    assert_eq!(en(0), "0 new messages");
    assert_eq!(en(1), "1 new message");
    assert_eq!(en(2), "2 new messages");
    assert_eq!(en(10_000), "10,000 new messages");
    assert_eq!(en(u64::MAX), "18,446,744,073,709,551,615 new messages");
}

#[test]
fn numbers_in_both_languages() {
    assert_eq!(number(Lang::Fr, 999), "999");
    assert_eq!(number(Lang::Fr, 1000), "1\u{a0}000");
    assert_eq!(number(Lang::En, 1000), "1,000");
    assert_eq!(number(Lang::En, 0), "0");
    assert_eq!(decimal(Lang::Fr, 4.46, 1), "4,5");
    assert_eq!(decimal(Lang::En, 4.46, 1), "4.5");
}

#[test]
fn dates_in_both_languages() {
    use time::{clock_in, day_label};
    // 2016-04-30 11:18:25.796 UTC (samedi)
    let ms = 1_462_015_105_796;
    let day = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap_or_default();
    let plus2 = FixedOffset::east_opt(2 * 3600).unwrap_or_else(|| panic!("fuseau"));
    let minus12 = FixedOffset::west_opt(12 * 3600).unwrap_or_else(|| panic!("fuseau"));
    for lang in Lang::ALL {
        assert_eq!(clock_in(lang, &Utc, day(2000, 1, 1), ms, false, true), "11:18");
        assert_eq!(clock_in(lang, &plus2, day(2000, 1, 1), ms, false, true), "13:18");
        assert_eq!(clock_in(lang, &Utc, day(2000, 1, 1), ms, false, false), "11:18 AM");
        assert_eq!(clock_in(lang, &plus2, day(2000, 1, 1), ms, false, false), "1:18 PM");
        assert_eq!(clock_in(lang, &Utc, day(2016, 4, 30), i64::MAX, true, true), "");
    }
    // Français : comme avant le lot.
    let fr = Lang::Fr;
    assert_eq!(clock_in(fr, &Utc, day(2016, 4, 30), ms, true, true), "Aujourd'hui à 11:18");
    assert_eq!(clock_in(fr, &Utc, day(2016, 5, 1), ms, true, true), "30/04/2016 11:18");
    assert_eq!(clock_in(fr, &minus12, day(2016, 4, 30), ms, true, true), "29/04/2016 23:18");
    assert_eq!(clock_in(fr, &minus12, day(2016, 4, 29), ms, true, true), "Aujourd'hui à 23:18");
    assert_eq!(clock_in(fr, &minus12, day(2016, 4, 29), ms, true, false), "Aujourd'hui à 11:18 PM");
    assert_eq!(clock_in(fr, &Utc, day(2016, 5, 1), ms, true, false), "30/04/2016 11:18 AM");
    let en = Lang::En;
    assert_eq!(clock_in(en, &Utc, day(2016, 4, 30), ms, true, true), "Today at 11:18");
    assert_eq!(clock_in(en, &Utc, day(2016, 4, 30), ms, true, false), "Today at 11:18 AM");
    assert_eq!(clock_in(en, &Utc, day(2016, 5, 1), ms, true, false), "04/30/2016 11:18 AM");
    assert_eq!(clock_in(en, &minus12, day(2016, 5, 1), ms, true, true), "04/29/2016 23:18");
    // Heure d'un message : seule pour aujourd'hui, « Hier à », sinon la date.
    use crate::i18n::time::message_in;
    assert_eq!(message_in(fr, &Utc, day(2016, 4, 30), ms, true), "11:18");
    assert_eq!(message_in(en, &Utc, day(2016, 4, 30), ms, false), "11:18 AM");
    assert_eq!(message_in(fr, &Utc, day(2016, 5, 1), ms, true), "Hier à 11:18");
    assert_eq!(message_in(en, &Utc, day(2016, 5, 1), ms, true), "Yesterday at 11:18");
    assert_eq!(message_in(fr, &Utc, day(2016, 5, 2), ms, true), "30/04/2016 11:18");
    assert_eq!(message_in(fr, &Utc, day(2016, 4, 30), i64::MAX, true), "");
    // Milieu de phrase : pas de majuscule.
    use crate::i18n::time::clock_mid_in;
    assert_eq!(clock_mid_in(fr, &Utc, day(2016, 4, 30), ms, true), "aujourd'hui à 11:18");
    assert_eq!(clock_mid_in(fr, &Utc, day(2016, 5, 1), ms, true), "30/04/2016 à 11:18");
    assert_eq!(clock_mid_in(en, &Utc, day(2016, 4, 30), ms, false), "today at 11:18 AM");
    assert_eq!(clock_mid_in(en, &Utc, day(2016, 5, 1), ms, true), "04/30/2016 at 11:18");

    let d = day(2026, 11, 12);
    assert_eq!(day_label(fr, d, d), "Aujourd'hui");
    assert_eq!(day_label(fr, day(2026, 11, 11), d), "Hier");
    assert_eq!(day_label(fr, d, day(2026, 11, 14)), "jeudi 12 novembre 2026");
    assert_eq!(day_label(fr, day(2026, 1, 3), d), "samedi 3 janvier 2026");
    assert_eq!(day_label(fr, day(2025, 12, 31), day(2026, 1, 1)), "Hier", "hier par-dessus l'année");
    assert_eq!(day_label(en, d, d), "Today");
    assert_eq!(day_label(en, day(2026, 11, 11), d), "Yesterday");
    assert_eq!(day_label(en, d, day(2026, 11, 14)), "Thursday, November 12, 2026");
    assert_eq!(day_label(en, day(2026, 8, 3), d), "Monday, August 3, 2026");
    assert_eq!(time::month(fr, 7), "août");
    assert_eq!(time::weekday(en, 6), "Sunday");
    // « 1er » pour le premier du mois seulement.
    assert_eq!(day_label(fr, day(2026, 11, 1), d), "dimanche 1er novembre 2026");
    assert_eq!(day_label(fr, day(2026, 11, 2), d), "lundi 2 novembre 2026");
    assert_eq!(day_label(fr, day(2026, 11, 21), d), "samedi 21 novembre 2026");
    assert_eq!(day_label(en, day(2026, 11, 1), d), "Sunday, November 1, 2026");
}

/// Échéance en milieu de phrase : heure seule le jour même, date et heure
/// sinon, jamais de majuscule ; 12 h et 24 h, dans les deux langues.
#[test]
fn deadlines_in_both_languages() {
    use time::until_in;
    // 2016-04-30 14:05 UTC
    let ms = 1_462_025_100_000;
    let day = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap_or_default();
    let (same, other) = (day(2016, 4, 30), day(2016, 4, 29));
    assert_eq!(until_in(Lang::Fr, &Utc, same, ms, true), "jusqu'à 14:05");
    assert_eq!(until_in(Lang::Fr, &Utc, same, ms, false), "jusqu'à 2:05 PM");
    assert_eq!(until_in(Lang::En, &Utc, same, ms, true), "until 14:05");
    assert_eq!(until_in(Lang::En, &Utc, same, ms, false), "until 2:05 PM");
    assert_eq!(until_in(Lang::Fr, &Utc, other, ms, true), "jusqu'au 30/04/2016 à 14:05");
    assert_eq!(until_in(Lang::Fr, &Utc, other, ms, false), "jusqu'au 30/04/2016 à 2:05 PM");
    assert_eq!(until_in(Lang::En, &Utc, other, ms, true), "until 04/30/2016 at 14:05");
    assert_eq!(until_in(Lang::En, &Utc, other, ms, false), "until 04/30/2016 at 2:05 PM");
    assert_eq!(f::timeout_reason(Lang::Fr, until_in(Lang::Fr, &Utc, same, ms, true)), "exclusion temporaire jusqu'à 14:05");
    assert_eq!(f::timed_out_composer(Lang::En, until_in(Lang::En, &Utc, same, ms, false)), "Timed out until 2:05 PM: you can't send messages");
    assert_eq!(
        f::timed_out_composer(Lang::Fr, until_in(Lang::Fr, &Utc, other, ms, true)),
        "Exclusion temporaire jusqu'au 30/04/2016 à 14:05\u{a0}: vous ne pouvez pas envoyer de messages"
    );
}

/// Un texte constant est emprunté à la table, jamais copié : `get` renvoie
/// l'adresse même du littéral, pour toutes les clés, et s'évalue à la
/// compilation (une fonction `const` ne peut pas allouer). Compter les
/// allocations demanderait un allocateur global, donc du code `unsafe`,
/// interdit dans ce projet.
#[test]
fn constant_text_is_borrowed_from_the_table() {
    for (k, text) in t::ALL {
        for lang in Lang::ALL {
            assert!(std::ptr::eq(text.get(lang), text.0[lang.index()]), "t::{k} ({lang:?}) copié");
        }
    }
    set_language(Choice::En);
    assert!(std::ptr::eq(tr(t::RETRY), t::RETRY.0[Lang::En.index()]));
    set_language(Choice::Fr);
    const RETRY_EN: &str = t::RETRY.get(Lang::En);
    assert_eq!(RETRY_EN, "Retry");
}

#[test]
fn system_tags_and_choices() {
    for (tag, lang) in [("fr-FR", Lang::Fr), ("fr_CA.UTF-8", Lang::Fr), ("FR", Lang::Fr), ("en-US", Lang::En), ("de-DE", Lang::En), ("", Lang::En), ("f", Lang::En)] {
        assert_eq!(Lang::from_locale(tag), lang, "{tag:?}");
    }
    for c in Choice::ALL {
        assert_eq!(Choice::from_key(c.key()), Some(c));
    }
    assert_eq!(Choice::from_key("klingon"), None);
    assert_eq!(Choice::En.label(Lang::Fr), "English");
    assert_eq!(Choice::System.label(Lang::En), "System");
}

/// Changement à la volée (propre au fil du test : voir `TEST_CHOICE`).
#[test]
fn switching_language_on_the_fly() {
    assert_eq!(language(), Lang::Fr, "langue des tests par défaut");
    set_language(Choice::En);
    assert_eq!((language(), choice()), (Lang::En, Choice::En));
    assert_eq!(tr(t::RETRY), "Retry");
    set_language(Choice::Fr);
    assert_eq!(tr(t::RETRY), "Réessayer");
    // Un autre fil garde la langue commune.
    std::thread::spawn(|| assert_eq!(language(), Lang::Fr)).join().unwrap_or_else(|_| panic!("fil"));
}

/// Libellés du panneau : une ligne (34 caractères au plus), dans les deux langues.
#[test]
fn token_labels_fit_one_row() {
    for (g, k, tok) in tokens::ALL {
        for lang in Lang::ALL {
            let l = tok.label(lang);
            assert!(l.chars().count() <= 34, "tokens::{g}::{k} ({lang:?}) trop long : {l}");
        }
    }
}

/// Tests en parallèle : le changement de langue d'un autre test ne réveille
/// pas l'interface de celui-ci (sinon ses mesures de « repos » vacillent).
#[test]
fn language_change_in_another_test_does_not_wake_this_one() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let woken = std::sync::Arc::new(AtomicUsize::new(0));
    let w = woken.clone();
    set_wake(move || {
        w.fetch_add(1, Ordering::Relaxed);
    });
    let other = std::thread::spawn(|| set_language(Choice::En));
    assert!(other.join().is_ok());
    assert_eq!(woken.load(Ordering::Relaxed), 0, "réveillé par un autre fil");
    assert_eq!(language(), Lang::Fr);
    set_language(Choice::En);
    assert_eq!(woken.load(Ordering::Relaxed), 1);
}
