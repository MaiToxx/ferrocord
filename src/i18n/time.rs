//! Heures et dates selon la langue et l'horloge (12/24 h) : noms des jours
//! et des mois, « Aujourd'hui », « Hier ».

use super::{t, Lang};
use chrono::{Datelike, Local, NaiveDate, TimeZone};

const DAYS: [[&str; 7]; 2] = [
    ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"],
    ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"],
];
const MONTHS: [[&str; 12]; 2] = [
    ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"],
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"],
];

/// Nom d'un jour (lundi = 0).
pub fn weekday(lang: Lang, from_monday: usize) -> &'static str {
    DAYS[lang.index()][from_monday % 7]
}

/// Nom d'un mois (janvier = 0).
pub fn month(lang: Lang, month0: usize) -> &'static str {
    MONTHS[lang.index()][month0 % 12]
}

/// Heure locale d'un horodatage (ms Unix) : seule (`full` faux), ou
/// précédée de « Aujourd'hui à » / de la date. `h24` : 24 h, sinon 12 h (AM/PM).
pub fn clock(lang: Lang, ms: i64, full: bool, h24: bool) -> String {
    clock_in(lang, &Local, Local::now().date_naive(), ms, full, h24)
}

/// `clock` avec le fuseau et la date du jour en paramètres (testable
/// quel que soit le fuseau de la machine). Hors de la plage représentable : vide.
pub fn clock_in<Tz: TimeZone>(lang: Lang, tz: &Tz, today: NaiveDate, ms: i64, full: bool, h24: bool) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let Some(dt) = tz.timestamp_millis_opt(ms).single() else { return String::new() };
    let hm = dt.format(if h24 { "%H:%M" } else { "%-I:%M %p" });
    if !full {
        return hm.to_string();
    }
    if dt.date_naive() == today {
        return super::f::today_at(lang, hm);
    }
    format!("{} {hm}", dt.format(numeric_date(lang)))
}

/// Heure d'un message dans le fil, comme dans le client : l'heure seule pour
/// aujourd'hui, « Hier à 15:18 », sinon la date puis l'heure.
pub fn message(lang: Lang, ms: i64, h24: bool) -> String {
    message_in(lang, &Local, Local::now().date_naive(), ms, h24)
}

/// `message` avec le fuseau et la date du jour en paramètres.
pub fn message_in<Tz: TimeZone>(lang: Lang, tz: &Tz, today: NaiveDate, ms: i64, h24: bool) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let Some(dt) = tz.timestamp_millis_opt(ms).single() else { return String::new() };
    let hm = dt.format(if h24 { "%H:%M" } else { "%-I:%M %p" });
    let day = dt.date_naive();
    if day == today {
        return hm.to_string();
    }
    if day.succ_opt() == Some(today) {
        return super::f::yesterday_at(lang, hm);
    }
    format!("{} {hm}", dt.format(numeric_date(lang)))
}

/// Date et heure en milieu de phrase : « aujourd'hui à 14:05 » (sans la
/// majuscule de [`clock`]), sinon « 12/11/2026 à 14:05 ».
pub fn clock_mid(lang: Lang, ms: i64, h24: bool) -> String {
    clock_mid_in(lang, &Local, Local::now().date_naive(), ms, h24)
}

/// `clock_mid` avec le fuseau et la date du jour en paramètres.
pub fn clock_mid_in<Tz: TimeZone>(lang: Lang, tz: &Tz, today: NaiveDate, ms: i64, h24: bool) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let Some(dt) = tz.timestamp_millis_opt(ms).single() else { return String::new() };
    let hm = dt.format(if h24 { "%H:%M" } else { "%-I:%M %p" });
    if dt.date_naive() == today {
        return super::f::today_at_mid(lang, hm);
    }
    super::f::date_at(lang, dt.format(numeric_date(lang)), hm)
}

/// Échéance en milieu de phrase : « jusqu'à 14:05 », « until 2:05 PM » si
/// c'est aujourd'hui ; sinon « jusqu'au 01/01/2099 à 01:00 »,
/// « until 01/01/2099 at 1:00 AM ». Jamais de majuscule.
pub fn until(lang: Lang, ms: i64, h24: bool) -> String {
    until_in(lang, &Local, Local::now().date_naive(), ms, h24)
}

/// `until` avec le fuseau et la date du jour en paramètres.
pub fn until_in<Tz: TimeZone>(lang: Lang, tz: &Tz, today: NaiveDate, ms: i64, h24: bool) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let Some(dt) = tz.timestamp_millis_opt(ms).single() else { return String::new() };
    let hm = dt.format(if h24 { "%H:%M" } else { "%-I:%M %p" });
    if dt.date_naive() == today {
        return super::f::until_time(lang, hm);
    }
    super::f::until_date(lang, dt.format(numeric_date(lang)), hm)
}

/// Horodatage `<t:…:X>` de Discord (`ms` : ms Unix ; `now_ms` : maintenant,
/// pour `R`) dans le style `X` : `t` 16:20, `T` 16:20:30, `d` 20/04/2021,
/// `D` 20 avril 2021, `f` 20 avril 2021 16:20, `F` mardi 20 avril 2021
/// 16:20, `R` il y a 2 mois. Style inconnu : `f`. Hors plage : vide.
pub fn stamp(lang: Lang, ms: i64, now_ms: i64, style: char, h24: bool) -> String {
    stamp_in(lang, &Local, ms, now_ms, style, h24)
}

/// `stamp` avec le fuseau en paramètre.
pub fn stamp_in<Tz: TimeZone>(lang: Lang, tz: &Tz, ms: i64, now_ms: i64, style: char, h24: bool) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let Some(dt) = tz.timestamp_millis_opt(ms).single() else { return String::new() };
    if style == 'R' {
        return relative(lang, ms, now_ms);
    }
    let hm = dt.format(if h24 { "%H:%M" } else { "%-I:%M %p" }).to_string();
    let day = dt.date_naive();
    let long = || {
        let m = month(lang, day.month0() as usize);
        match lang {
            Lang::Fr if day.day() == 1 => format!("1er {m} {}", day.year()),
            Lang::Fr => format!("{} {m} {}", day.day(), day.year()),
            Lang::En => format!("{m} {}, {}", day.day(), day.year()),
        }
    };
    match style {
        't' => hm,
        'T' => dt.format(if h24 { "%H:%M:%S" } else { "%-I:%M:%S %p" }).to_string(),
        'd' => dt.format(numeric_date(lang)).to_string(),
        'D' => long(),
        'F' => {
            let w = weekday(lang, day.weekday().num_days_from_monday() as usize);
            match lang {
                Lang::Fr => format!("{w} {} {hm}", long()),
                Lang::En => format!("{w}, {} {hm}", long()),
            }
        }
        _ => format!("{} {hm}", long()),
    }
}

/// « il y a 3 jours », « dans 2 heures » : l'unité la plus grande qui fait au moins 1.
pub fn relative(lang: Lang, ms: i64, now_ms: i64) -> String {
    let diff = ms.saturating_sub(now_ms);
    let secs = (diff / 1000).unsigned_abs();
    const MIN: u64 = 60;
    const HOUR: u64 = 60 * MIN;
    const DAY: u64 = 24 * HOUR;
    let duration = if secs < MIN {
        super::p::seconds(lang, secs)
    } else if secs < HOUR {
        super::p::minutes(lang, secs / MIN)
    } else if secs < DAY {
        super::p::hours(lang, secs / HOUR)
    } else if secs < 30 * DAY {
        super::p::days(lang, secs / DAY)
    } else if secs < 365 * DAY {
        super::p::months(lang, secs / (30 * DAY))
    } else {
        super::p::years(lang, secs / (365 * DAY))
    };
    if diff < 0 { super::f::ago(lang, duration) } else { super::f::in_time(lang, duration) }
}

/// Délai (ms) avant que [`relative`] ne change de
/// texte. Sous la minute, les secondes ne sont pas suivies : le texte suivant
/// attendu est « 1 minute » (un réveil par minute au plus, par l'heure ou le
/// jour au-delà).
pub fn relative_changes_in(ms: i64, now_ms: i64) -> i64 {
    const MIN: i64 = 60_000;
    const HOUR: i64 = 60 * MIN;
    const DAY: i64 = 24 * HOUR;
    let d = ms.saturating_sub(now_ms).saturating_abs();
    let unit = if d < HOUR {
        MIN
    } else if d < DAY {
        HOUR
    } else {
        DAY
    };
    let wait = if ms <= now_ms {
        // Passé : le compte augmente au prochain multiple de l'unité.
        unit - d % unit
    } else {
        // Futur : il diminue quand l'écart passe sous le multiple en cours.
        d % unit + 1
    };
    wait.max(1000)
}

/// Date en toutes lettres, sans le jour de la semaine : « 12 novembre 2026 »,
/// « November 12, 2026 » (date de création d'un compte, d'arrivée sur un
/// serveur). Hors de la plage représentable : vide.
pub fn long_date(lang: Lang, ms: i64) -> String {
    long_date_in(lang, &Local, ms)
}

/// `long_date` avec le fuseau en paramètre.
pub fn long_date_in<Tz: TimeZone>(lang: Lang, tz: &Tz, ms: i64) -> String {
    let Some(day) = tz.timestamp_millis_opt(ms).single().map(|d| d.date_naive()) else { return String::new() };
    let m = month(lang, day.month0() as usize);
    match lang {
        Lang::Fr if day.day() == 1 => format!("1er {m} {}", day.year()),
        Lang::Fr => format!("{} {m} {}", day.day(), day.year()),
        Lang::En => format!("{m} {}, {}", day.day(), day.year()),
    }
}

/// Date courte, sans l'année : « 16/11 » en français, « 11/16 » en anglais
/// (échéance d'une quête). Hors de la plage représentable : vide.
pub fn short_date(lang: Lang, ms: i64) -> String {
    short_date_in(lang, &Local, ms)
}

/// `short_date` avec le fuseau en paramètre.
pub fn short_date_in<Tz: TimeZone>(lang: Lang, tz: &Tz, ms: i64) -> String {
    let Some(day) = tz.timestamp_millis_opt(ms).single().map(|d| d.date_naive()) else { return String::new() };
    match lang {
        Lang::Fr => format!("{:02}/{:02}", day.day(), day.month()),
        Lang::En => format!("{:02}/{:02}", day.month(), day.day()),
    }
}

/// Date en chiffres : jour/mois/année en français, mois/jour/année en anglais.
fn numeric_date(lang: Lang) -> &'static str {
    match lang {
        Lang::Fr => "%d/%m/%Y",
        Lang::En => "%m/%d/%Y",
    }
}

/// Séparateur de jour : « Aujourd'hui », « Hier », ou la date en toutes
/// lettres (« jeudi 12 novembre 2026 », « Thursday, November 12, 2026 »).
pub fn day_label(lang: Lang, day: NaiveDate, today: NaiveDate) -> String {
    if day == today {
        return t::TODAY.get(lang).to_string();
    }
    if today.pred_opt() == Some(day) {
        return t::YESTERDAY.get(lang).to_string();
    }
    let (w, m) = (weekday(lang, day.weekday().num_days_from_monday() as usize), month(lang, day.month0() as usize));
    match lang {
        // Le premier du mois s'écrit « 1er », les autres jours en chiffres seuls.
        Lang::Fr if day.day() == 1 => format!("{w} 1er {m} {}", day.year()),
        Lang::Fr => format!("{w} {} {m} {}", day.day(), day.year()),
        Lang::En => format!("{w}, {m} {}, {}", day.day(), day.year()),
    }
}
