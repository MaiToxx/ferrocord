//! Réglages persistants (`settings.toml`, à côté de `theme.toml`) : comptes
//! connus, dernier compte utilisé, dernier serveur et salon par compte,
//! fenêtre, panneau Apparence. Le thème reste dans `theme.toml`.
//!
//! Pur (aucune E/S) : le fichier est lu et écrit par le même mécanisme que le
//! thème ([`crate::theme::FileLink`]) : écriture atomique différée, fichier
//! refusé jamais écrasé, tout hors du fil d'interface.
//!
//! Le fichier ne contient jamais de jeton : il n'y a aucun champ pour cela.
//! Valeurs bornées : un nombre hors bornes est ramené dans ses bornes ; un
//! texte illisible (TOML cassé, mauvais type, identifiant invalide) refuse le
//! fichier entier, avec la ligne en cause.

use crate::account::{migrate::FileMark, AccountKey};
use crate::i18n::{
    language, t as tx,
    verbatim::{f as vf, t as vt},
    Choice, Text,
};
use crate::state::model::ImageHash;
use crate::theme::{Document, FileLink, FileUpdate};
use std::{ops::Range, path::PathBuf};
use toml::de::{DeTable, DeValue};

/// Lien vers `settings.toml`.
pub type SettingsLink = FileLink<Settings>;
/// Nouvelle du fil des réglages.
pub type SettingsUpdate = FileUpdate<Settings>;

/// Emplacement des réglages de l'utilisateur.
pub fn default_path() -> Option<PathBuf> {
    Some(crate::platform::config_dir()?.join("ferrocord").join("settings.toml"))
}

/// Comptes retenus au plus : au-delà, le fichier est refusé à la lecture (avec
/// la ligne) et l'interface n'en ajoute pas (elle le dit).
pub const MAX_ACCOUNTS: usize = 16;
/// Longueur maximale d'un nom de compte gardé (caractères).
pub const MAX_NAME: usize = 64;
/// Bornes de la taille intérieure de la fenêtre (points logiques).
pub const WIDTH: (f32, f32) = (520.0, 16384.0);
pub const HEIGHT: (f32, f32) = (320.0, 16384.0);
/// Bornes de la position de la fenêtre (points logiques).
pub const POSITION: (f32, f32) = (-32768.0, 32768.0);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    /// Compte ouvert au dernier lancement.
    pub last_account: Option<AccountKey>,
    pub window: Window,
    /// Panneau Apparence ouvert.
    pub appearance_open: bool,
    /// Langue de l'interface (Apparence > Essentiel) ; valeur absente ou
    /// inconnue : « Système ».
    pub language: Choice,
    /// Comptes connus, dans l'ordre d'ajout.
    pub accounts: Vec<Account>,
    /// L'utilisateur a retiré le compte importé alors que le fichier de jeton
    /// en clair existait encore : ce fichier-là (taille et date de
    /// modification, jamais son contenu) n'est plus importé tout seul.
    pub declined_import: Option<FileMark>,
    /// Instant (heure murale, secondes depuis 1970) où le jeton de l'ancien
    /// fichier a été trouvé ou écrit dans le coffre : l'étape 2 de la
    /// migration attend 24 h après lui. Jamais le jeton.
    pub vault_copy_since: Option<u64>,
    /// Dernière écriture réussie d'un jeton dans le coffre, par entrée
    /// (heure murale, jamais le jeton) : une copie de moins de 24 h peut se
    /// perdre dans une coupure de courant.
    pub written: Vec<(AccountKey, u64)>,
}

/// Fenêtre principale : taille intérieure et position (points logiques, hors
/// zoom), et état maximisé. La taille et la position sont celles de la
/// fenêtre non maximisée.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Window {
    pub size: Option<[f32; 2]>,
    pub position: Option<[f32; 2]>,
    pub maximized: bool,
}

/// Un compte connu. L'identité n'est connue qu'après READY : un compte
/// importé de l'ancien fichier n'a d'abord ni identifiant ni nom.
#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    pub key: AccountKey,
    pub user_id: Option<u64>,
    pub name: Option<String>,
    pub avatar: Option<ImageHash>,
    /// Dernier endroit ouvert.
    pub place: Option<Place>,
}

impl Account {
    pub fn new(key: AccountKey) -> Self {
        Account { key, user_id: None, name: None, avatar: None, place: None }
    }
}

/// Serveur (`None` : messages privés) et salon ouverts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub guild: Option<u64>,
    pub channel: Option<u64>,
}

impl Settings {
    pub fn account(&self, key: &AccountKey) -> Option<&Account> {
        self.accounts.iter().find(|a| &a.key == key)
    }

    pub fn account_mut(&mut self, key: &AccountKey) -> Option<&mut Account> {
        self.accounts.iter_mut().find(|a| &a.key == key)
    }

    /// Ramène toutes les valeurs dans leurs bornes.
    pub fn sanitized(mut self) -> Self {
        self.window.size = self.window.size.and_then(|[w, h]| Some([bound(w, WIDTH)?, bound(h, HEIGHT)?]));
        self.window.position = self.window.position.and_then(|[x, y]| Some([bound(x, POSITION)?, bound(y, POSITION)?]));
        let mut seen = Vec::new();
        self.accounts.retain(|a| {
            let new = !seen.contains(&a.key);
            seen.push(a.key.clone());
            new
        });
        self.accounts.truncate(MAX_ACCOUNTS);
        let mut seen = Vec::new();
        self.written.retain(|(k, _)| {
            let new = !seen.contains(k);
            seen.push(k.clone());
            new
        });
        self.written.truncate(MAX_ACCOUNTS + 1);
        // Dans l'ordre où le fichier les relit.
        self.written.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
        for a in &mut self.accounts {
            a.name = a.name.take().map(|n| clean_name(&n)).filter(|n| !n.is_empty());
        }
        self
    }
}

/// Un écran présent : position et taille en points logiques.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Monitor {
    pub position: [f32; 2],
    pub size: [f32; 2],
}

/// Bande du haut de la fenêtre (barre de titre) qui doit rester visible
/// pour qu'on puisse l'attraper : hauteur, et largeur minimale visible.
pub const GRAB_HEIGHT: f32 = 32.0;
pub const GRAB_WIDTH: f32 = 96.0;
/// Taille de la fenêtre quand aucune n'est retenue.
pub const DEFAULT_SIZE: [f32; 2] = [1100.0, 720.0];

/// Position où rouvrir la fenêtre : celle retenue si sa barre de titre
/// recoupe réellement un écran présent (assez pour l'attraper), sinon
/// `None` (place choisie par le système). Aucun écran connu : `None`.
pub fn visible_position(w: &Window, monitors: &[Monitor]) -> Option<[f32; 2]> {
    let [x, y] = w.position?;
    let [width, _] = w.size.unwrap_or(DEFAULT_SIZE);
    let overlap = |a0: f32, a1: f32, b0: f32, b1: f32| (a1.min(b1) - a0.max(b0)).max(0.0);
    let grab = |m: &Monitor| {
        let [mx, my] = m.position;
        let [mw, mh] = m.size;
        overlap(x, x + width, mx, mx + mw) >= GRAB_WIDTH.min(width) && overlap(y, y + GRAB_HEIGHT, my, my + mh) >= GRAB_HEIGHT / 2.0
    };
    monitors.iter().any(grab).then_some([x, y])
}

/// `None` si `v` n'est pas un nombre fini.
fn bound(v: f32, (min, max): (f32, f32)) -> Option<f32> {
    v.is_finite().then(|| v.clamp(min, max).round())
}

/// Nom affichable : sans caractères de contrôle, au plus [`MAX_NAME`] caractères.
pub fn clean_name(name: &str) -> String {
    name.chars().filter(|c| !c.is_control()).take(MAX_NAME).collect::<String>().trim().to_string()
}

impl Document for Settings {
    const FIX_HINT: Text = vt::SETTINGS_FIX_HINT;
    const SAVE_FAILED: Text = vt::SETTINGS_SAVE_FAILED;
    // L'instant de la copie ne compte pour l'étape 2 qu'une fois sur le disque.
    const ACK_SAVES: bool = true;

    fn parse(text: &str) -> Result<Self, (Option<usize>, String)> {
        parse(text)
    }

    fn export(&self) -> String {
        export(self)
    }
}

fn line_of(text: &str, span: Range<usize>) -> Option<usize> {
    Some(text.get(..span.start).unwrap_or(text).matches('\n').count() + 1)
}

/// Lit `settings.toml`. Les clés inconnues sont ignorées (fichier d'une
/// version plus récente) ; les valeurs connues doivent être lisibles.
pub fn parse(text: &str) -> Result<Settings, (Option<usize>, String)> {
    let root = DeTable::parse(text).map_err(|e| (e.span().and_then(|s| line_of(text, s)), syntax(e.message())))?;
    let root = root.get_ref();
    let err = |span: Range<usize>, msg: String| (line_of(text, span), msg);
    let mut s = Settings::default();
    for (key, value) in root.iter() {
        let v = value.get_ref();
        match key.get_ref().as_ref() {
            "dernier_compte" => s.last_account = Some(account_key(v).map_err(|m| err(value.span(), vf::field_error(language(), "dernier_compte", m)))?),
            "fenetre" => {
                let t = v.as_table().ok_or_else(|| err(key.span(), vf::must_be_section(language(), "fenetre")))?;
                let (mut w, mut h, mut x, mut y) = (None, None, None, None);
                for (k, v) in t.iter() {
                    let name = k.get_ref().as_ref();
                    let slot = match name {
                        "largeur" => &mut w,
                        "hauteur" => &mut h,
                        "x" => &mut x,
                        "y" => &mut y,
                        "maximisee" => {
                            s.window.maximized = v.get_ref().as_bool().ok_or_else(|| err(v.span(), vf::field_error(language(), "fenetre.maximisee", vt::TRUE_OR_FALSE.get(language()))))?;
                            continue;
                        }
                        _ => continue,
                    };
                    *slot = Some(number(v.get_ref()).map_err(|m| err(v.span(), vf::field_error(language(), format_args!("fenetre.{name}"), m)))?);
                }
                s.window.size = w.zip(h).map(|(w, h)| [w, h]);
                s.window.position = x.zip(y).map(|(x, y)| [x, y]);
            }
            "ancien_fichier_refuse" => {
                let t = v.as_table().ok_or_else(|| err(key.span(), vf::must_be_section(language(), "ancien_fichier_refuse")))?;
                let field = |name: &str| -> Result<u64, (Option<usize>, String)> {
                    let x = t.get(name).ok_or_else(|| err(key.span(), vf::field_missing(language(), format_args!("ancien_fichier_refuse.{name}"))))?;
                    x.get_ref().as_str().and_then(|s| s.parse::<u64>().ok()).ok_or_else(|| err(x.span(), vf::field_error(language(), format_args!("ancien_fichier_refuse.{name}"), vt::QUOTED_NUMBER.get(language()))))
                };
                s.declined_import = Some(FileMark { size: field("taille")?, modified_ns: field("modifie")? });
            }
            "migration" => {
                let t = v.as_table().ok_or_else(|| err(key.span(), vf::must_be_section(language(), "migration")))?;
                if let Some(x) = t.get("copie_depuis") {
                    s.vault_copy_since = Some(x.get_ref().as_str().and_then(|s| s.parse::<u64>().ok()).ok_or_else(|| err(x.span(), vf::field_error(language(), "migration.copie_depuis", vt::QUOTED_NUMBER.get(language()))))?);
                }
            }
            "ecritures" => {
                let t = v.as_table().ok_or_else(|| err(key.span(), vf::must_be_section(language(), "ecritures")))?;
                for (k, x) in t.iter() {
                    let Some(account) = AccountKey::parse(k.get_ref().as_ref()) else { continue };
                    let at = x.get_ref().as_str().and_then(|s| s.parse::<u64>().ok()).ok_or_else(|| err(x.span(), vf::field_error(language(), "ecritures", vt::QUOTED_NUMBER.get(language()))))?;
                    s.written.push((account, at));
                }
            }
            "apparence" => {
                let t = v.as_table().ok_or_else(|| err(key.span(), vf::must_be_section(language(), "apparence")))?;
                if let Some(o) = t.get("panneau_ouvert") {
                    s.appearance_open = o.get_ref().as_bool().ok_or_else(|| err(o.span(), vf::field_error(language(), "apparence.panneau_ouvert", vt::TRUE_OR_FALSE.get(language()))))?;
                }
                // Langue : une valeur inconnue (ou d'un autre type) vaut « Système », sans refuser le fichier.
                if let Some(l) = t.get("langue") {
                    s.language = l.get_ref().as_str().and_then(Choice::from_key).unwrap_or_default();
                }
            }
            "comptes" => {
                let list = v.as_array().ok_or_else(|| err(key.span(), vt::ACCOUNTS_NOT_LIST.get(language()).into()))?;
                for item in list.iter() {
                    let t = item.get_ref().as_table().ok_or_else(|| err(item.span(), vt::EACH_ACCOUNT_SECTION.get(language()).into()))?;
                    let a = account(t).map_err(|(span, m)| err(span.unwrap_or_else(|| item.span()), m))?;
                    // Au-delà de la limite, refus (avec la ligne) plutôt qu'un oubli silencieux.
                    if !s.accounts.iter().any(|x| x.key == a.key) && s.accounts.len() >= MAX_ACCOUNTS {
                        return Err(err(item.span(), vf::too_many_accounts(language(), MAX_ACCOUNTS)));
                    }
                    s.accounts.push(a);
                }
            }
            _ => {}
        }
    }
    Ok(s.sanitized())
}

type FieldError = (Option<Range<usize>>, String);

fn account(t: &DeTable<'_>) -> Result<Account, FieldError> {
    let field = |name: &str| t.get(name);
    let text = |name: &str| -> Result<Option<(&str, Range<usize>)>, FieldError> {
        match field(name) {
            None => Ok(None),
            Some(v) => v.get_ref().as_str().map(|s| Some((s, v.span()))).ok_or_else(|| (Some(v.span()), vf::field_error(language(), format_args!("comptes.{name}"), vt::QUOTED_TEXT.get(language())))),
        }
    };
    let (key, span) = text("cle")?.ok_or((None, vt::KEY_MISSING.get(language()).to_string()))?;
    let key = AccountKey::parse(key).ok_or((Some(span), vf::field_error(language(), "comptes.cle", vf::invalid_key(language(), clean_name(key)))))?;
    let id = |name: &str| -> Result<Option<u64>, FieldError> {
        match text(name)? {
            None => Ok(None),
            Some((s, span)) => s.parse::<u64>().map(Some).map_err(|_| (Some(span), vf::field_error(language(), format_args!("comptes.{name}"), vt::INVALID_ID.get(language())))),
        }
    };
    let user_id = id("id")?;
    let name = text("nom")?.map(|(s, _)| s.to_string());
    let avatar = match text("avatar")? {
        None => None,
        Some((s, span)) => Some(ImageHash::parse(s).ok_or((Some(span), vf::field_error(language(), "comptes.avatar", vt::INVALID_HASH.get(language()))))?),
    };
    let channel = id("salon")?;
    let place = match text("serveur")? {
        None => channel.map(|c| Place { guild: None, channel: Some(c) }),
        Some(("mp", _)) => Some(Place { guild: None, channel }),
        Some((s, span)) => Some(Place { guild: Some(s.parse::<u64>().map_err(|_| (Some(span), vf::field_error(language(), "comptes.serveur", vt::ID_OR_MP.get(language()))))?), channel }),
    };
    Ok(Account { key, user_id, name, avatar, place })
}

fn account_key(v: &DeValue<'_>) -> Result<AccountKey, String> {
    let s = v.as_str().ok_or_else(|| vt::QUOTED_TEXT.get(language()).to_string())?;
    AccountKey::parse(s).ok_or_else(|| vf::invalid_key(language(), clean_name(s)))
}

fn number(v: &DeValue<'_>) -> Result<f32, String> {
    let x = if let Some(i) = v.as_integer() {
        i64::from_str_radix(i.as_str(), i.radix()).map_err(|_| tx::THEME_INT_TOO_BIG.get(language()).to_string())? as f64
    } else if let Some(f) = v.as_float() {
        f.as_str().parse::<f64>().map_err(|_| tx::THEME_BAD_NUMBER.get(language()).to_string())?
    } else {
        return Err(vt::NUMBER_EXPECTED.get(language()).into());
    };
    if x.is_finite() { Ok(x as f32) } else { Err(tx::THEME_FINITE.get(language()).into()) }
}

fn syntax(detail: &str) -> String {
    let detail = detail.lines().next().unwrap_or_default().trim();
    if detail.is_empty() { tx::THEME_SYNTAX.get(language()).into() } else { vf::settings_syntax_detail(language(), detail) }
}

/// Texte TOML entre guillemets, échappé.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Fichier complet, relu à l'identique par [`parse`].
pub fn export(s: &Settings) -> String {
    let s = s.clone().sanitized();
    let mut out = String::from(
        "# Réglages de Ferrocord, écrits par l'application (le thème est dans theme.toml).\n\
         # Aucun jeton ici : ils sont rangés dans le Gestionnaire d'identifiants.\n",
    );
    if let Some(k) = &s.last_account {
        out.push_str(&format!("\ndernier_compte = {}\n", quote(k.as_str())));
    }
    out.push_str("\n[fenetre]\n");
    if let Some([w, h]) = s.window.size {
        out.push_str(&format!("largeur = {w}\nhauteur = {h}\n"));
    }
    if let Some([x, y]) = s.window.position {
        out.push_str(&format!("x = {x}\ny = {y}\n"));
    }
    out.push_str(&format!("maximisee = {}\n", s.window.maximized));
    out.push_str(&format!("\n[apparence]\npanneau_ouvert = {}\nlangue = {}\n", s.appearance_open, quote(s.language.key())));
    if let Some(m) = s.declined_import {
        out.push_str(&format!(
            "\n# Ancien fichier de jeton dont l'import a été refusé (sa taille et sa date, pas son contenu).\n[ancien_fichier_refuse]\ntaille = \"{}\"\nmodifie = \"{}\"\n",
            m.size, m.modified_ns
        ));
    }
    if let Some(t) = s.vault_copy_since {
        out.push_str(&format!("\n# Copie du jeton de l'ancien fichier dans le coffre : instant (secondes depuis 1970).\n[migration]\ncopie_depuis = \"{t}\"\n"));
    }
    if !s.written.is_empty() {
        out.push_str("\n# Dernière écriture d'un jeton dans le coffre, par compte : instant (secondes depuis 1970).\n[ecritures]\n");
        for (k, at) in &s.written {
            out.push_str(&format!("{} = \"{at}\"\n", quote(k.as_str())));
        }
    }
    for a in &s.accounts {
        out.push_str(&format!("\n[[comptes]]\ncle = {}\n", quote(a.key.as_str())));
        if let Some(id) = a.user_id {
            out.push_str(&format!("id = \"{id}\"\n"));
        }
        if let Some(n) = &a.name {
            out.push_str(&format!("nom = {}\n", quote(n)));
        }
        if let Some(h) = a.avatar {
            out.push_str(&format!("avatar = \"{h}\"\n"));
        }
        if let Some(p) = a.place {
            match p.guild {
                Some(g) => out.push_str(&format!("serveur = \"{g}\"\n")),
                None => out.push_str("serveur = \"mp\"\n"),
            }
            if let Some(c) = p.channel {
                out.push_str(&format!("salon = \"{c}\"\n"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::WatchConfig;
    use std::time::{Duration, Instant};

    fn sample() -> Settings {
        Settings {
            last_account: Some(AccountKey::of_user(1000000000000000001)),
            window: Window { size: Some([1280.0, 800.0]), position: Some([-1200.0, 40.0]), maximized: true },
            appearance_open: true,
            language: Choice::En,
            accounts: vec![
                Account {
                    key: AccountKey::of_user(1000000000000000001),
                    user_id: Some(1000000000000000001),
                    name: Some("Moi \"Fictif\" \\ é".into()),
                    avatar: Some(ImageHash::synthetic(5)),
                    place: Some(Place { guild: Some(2000000000000000001), channel: Some(3000000000000000011) }),
                },
                Account { key: AccountKey::imported(), user_id: None, name: None, avatar: None, place: Some(Place { guild: None, channel: Some(4) }) },
                Account { place: Some(Place { guild: None, channel: None }), ..Account::new(AccountKey::of_user(9)) },
            ],
            declined_import: Some(FileMark { size: 57, modified_ns: 1_700_000_000_123_456_789 }),
            vault_copy_since: Some(1_700_000_123),
            written: vec![(AccountKey::of_user(9), 1_700_000_300), (AccountKey::imported(), 1_700_000_200)],
        }
    }

    #[test]
    fn round_trip() {
        let s = sample();
        let text = export(&s);
        assert_eq!(parse(&text), Ok(s.clone()), "{text}");
        assert_eq!(export(&parse(&text).unwrap_or_default()), text, "deuxième tour identique");
        assert_eq!(parse(&export(&Settings::default())), Ok(Settings::default()));
    }

    /// Choix de langue : relu tel quel ; absent, inconnu ou d'un autre type :
    /// « Système », sans refuser le fichier.
    #[test]
    fn language_choice_is_kept_and_unknown_means_system() {
        for c in Choice::ALL {
            let s = Settings { language: c, ..Settings::default() };
            assert_eq!(parse(&export(&s)).map(|s| s.language), Ok(c));
        }
        for text in ["", "[apparence]\n", "[apparence]\nlangue = \"klingon\"\n", "[apparence]\nlangue = 3\n"] {
            assert_eq!(parse(text).map(|s| s.language), Ok(Choice::System), "{text:?}");
        }
    }

    #[test]
    fn values_are_bounded() {
        let text = "[fenetre]\nlargeur = 10\nhauteur = 1e9\nx = -1e12\ny = 5.4\nmaximisee = false\n\n[[comptes]]\ncle = \"5\"\nnom = \"\\u0007abc\"\n";
        let s = parse(text).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(s.window.size, Some([WIDTH.0, HEIGHT.1]));
        assert_eq!(s.window.position, Some([POSITION.0, 5.0]));
        assert_eq!(s.accounts[0].name.as_deref(), Some("abc"));

        let long = "x".repeat(MAX_NAME * 3);
        let mut many = Settings::default();
        for i in 0..(MAX_ACCOUNTS as u64 + 5) {
            many.accounts.push(Account { name: Some(long.clone()), ..Account::new(AccountKey::of_user(i % MAX_ACCOUNTS as u64)) });
        }
        let back = parse(&export(&many)).unwrap_or_default();
        assert_eq!(back.accounts.len(), MAX_ACCOUNTS, "doublons retirés, liste bornée");
        assert!(back.accounts.iter().all(|a| a.name.as_ref().is_some_and(|n| n.chars().count() == MAX_NAME)));
        // Nombre non fini : la valeur est oubliée, pas écrite.
        let mut nan = Settings::default();
        nan.window.size = Some([f32::NAN, 600.0]);
        assert_eq!(parse(&export(&nan)).map(|s| s.window.size), Ok(None));
    }

    /// Plus de comptes que la limite : refusé avec la ligne, jamais tronqué en silence.
    #[test]
    fn too_many_accounts_are_refused() {
        let mut text = String::new();
        for i in 0..=MAX_ACCOUNTS as u64 {
            text.push_str(&format!("[[comptes]]\ncle = \"{}\"\n", i + 1));
        }
        match parse(&text) {
            Err((line, m)) => {
                assert_eq!(line, Some(MAX_ACCOUNTS * 2 + 1), "{m}");
                assert!(m.contains("plus de 16 comptes"), "{m}");
            }
            Ok(s) => panic!("accepté : {} comptes", s.accounts.len()),
        }
    }

    /// Position de la fenêtre : gardée seulement si sa barre de titre
    /// recoupe un écran présent.
    #[test]
    fn window_position_is_kept_only_on_a_present_screen() {
        let primary = Monitor { position: [0.0, 0.0], size: [1920.0, 1080.0] };
        // Écran secondaire à gauche, plus haut (coordonnées négatives).
        let left = Monitor { position: [-1280.0, -200.0], size: [1280.0, 1024.0] };
        let at = |x: f32, y: f32| Window { size: Some([1100.0, 720.0]), position: Some([x, y]), maximized: false };
        type Case<'a> = (&'a str, Window, &'a [Monitor], Option<[f32; 2]>);
        let cases: [Case; 10] = [
            ("écran principal", at(100.0, 100.0), &[primary], Some([100.0, 100.0])),
            ("écran secondaire présent", at(-1000.0, -100.0), &[primary, left], Some([-1000.0, -100.0])),
            ("écran secondaire débranché", at(-1000.0, -100.0), &[primary], None),
            ("à cheval sur deux écrans", at(-500.0, 50.0), &[primary, left], Some([-500.0, 50.0])),
            ("presque entièrement à droite", at(1900.0, 100.0), &[primary], None),
            ("assez de barre de titre à droite", at(1800.0, 100.0), &[primary], Some([1800.0, 100.0])),
            ("barre de titre au-dessus de l'écran", at(100.0, -40.0), &[primary], None),
            ("barre de titre à moitié visible", at(100.0, -16.0), &[primary], Some([100.0, -16.0])),
            ("sous l'écran", at(100.0, 1075.0), &[primary], None),
            ("aucun écran connu", at(100.0, 100.0), &[], None),
        ];
        for (what, w, monitors, expected) in cases {
            assert_eq!(visible_position(&w, monitors), expected, "{what}");
        }
        // Pas de position retenue : rien à décider.
        assert_eq!(visible_position(&Window::default(), &[primary]), None);
        // Taille inconnue : celle par défaut.
        let w = Window { size: None, position: Some([-1000.0, 10.0]), maximized: false };
        assert_eq!(visible_position(&w, &[primary]), Some([-1000.0, 10.0]), "{} de large", DEFAULT_SIZE[0]);
    }

    #[test]
    fn invalid_files_are_refused_with_line() {
        for (text, line) in [
            ("dernier_compte = \"1\"\n[fenetre\n", 2),
            ("[fenetre]\nlargeur = \"grand\"\n", 2),
            ("\n[[comptes]]\ncle = \"abc\"\n", 3),
            ("[[comptes]]\ncle = \"1\"\nid = \"x1\"\n", 3),
            ("[[comptes]]\ncle = \"1\"\navatar = \"../../x\"\n", 3),
            ("dernier_compte = 12\n", 1),
            ("[apparence]\npanneau_ouvert = \"oui\"\n", 2),
        ] {
            match parse(text) {
                Err((l, m)) => assert_eq!(l, Some(line), "{text:?} : {m}"),
                Ok(s) => panic!("{text:?} accepté : {s:?}"),
            }
        }
        // Clés inconnues : ignorées (fichier d'une version plus récente).
        assert_eq!(parse("futur = 1\n[fenetre]\nautre = true\n"), Ok(Settings::default()));
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ferrocord-reglages-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
        dir
    }

    /// Mêmes garanties que le thème : écriture différée et atomique, et un
    /// fichier refusé n'est jamais écrasé, pas même à la fermeture.
    #[test]
    fn link_saves_and_never_overwrites_a_broken_file() {
        let dir = temp("lien");
        let path = dir.join("settings.toml");
        let cfg = WatchConfig { poll: Duration::from_millis(20), debounce: Duration::from_millis(60) };
        {
            let (link, first) = SettingsLink::open(Some(path.clone()), true, cfg);
            assert_eq!(first, None);
            link.save(&sample());
            let end = Instant::now() + Duration::from_secs(5);
            while std::fs::read_to_string(&path).ok() != Some(export(&sample())) {
                assert!(Instant::now() < end, "jamais écrit");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        let (_, first) = SettingsLink::open(Some(path.clone()), true, cfg);
        assert_eq!(first, Some(FileUpdate::Loaded(sample())));

        let broken = "[[comptes]]\ncle = \"pas une clé\"\n";
        std::fs::write(&path, broken).unwrap_or_else(|e| panic!("{e}"));
        {
            let (link, first) = SettingsLink::open(Some(path.clone()), true, cfg);
            assert!(matches!(&first, Some(FileUpdate::FileError(m)) if m.contains("settings.toml contient une erreur ligne 2")), "{first:?}");
            link.save_now(&Settings::default());
            std::thread::sleep(Duration::from_millis(150));
        }
        assert_eq!(std::fs::read_to_string(&path).ok().as_deref(), Some(broken));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
