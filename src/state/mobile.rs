//! Réglages de la disposition mobile (phase 7, lot P2), section `[mobile]`
//! de `local.toml` : rien ne part chez Discord.

/// Choix de la disposition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Layout {
    /// Mobile sur Android ou en fenêtre étroite, bureau sinon.
    #[default]
    Auto,
    Desktop,
    Mobile,
}

impl Layout {
    pub const ALL: [Layout; 3] = [Layout::Auto, Layout::Desktop, Layout::Mobile];

    fn key(self) -> &'static str {
        match self {
            Layout::Auto => "auto",
            Layout::Desktop => "bureau",
            Layout::Mobile => "mobile",
        }
    }

    fn of(key: &str) -> Option<Layout> {
        Self::ALL.into_iter().find(|l| l.key() == key)
    }
}

/// Marge sous la zone de saisie quand le clavier virtuel est ouvert, au plus.
pub const MAX_KEYBOARD_MARGIN: u16 = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct MobilePrefs {
    pub layout: Layout,
    /// Marge (points) gardée sous la saisie pendant qu'un champ a le
    /// clavier, quand le système ne donne pas la hauteur du clavier virtuel.
    pub keyboard_margin: u16,
}

pub const SECTION: &str = "mobile";
const K_LAYOUT: &str = "disposition";
const K_MARGIN: &str = "marge_clavier";

impl MobilePrefs {
    /// Section `[mobile]` de `local.toml`.
    pub fn to_toml(&self) -> String {
        format!("[{SECTION}]\n# auto, bureau ou mobile.\n{K_LAYOUT} = \"{}\"\n{K_MARGIN} = {}\n", self.layout.key(), self.keyboard_margin)
    }

    /// Lit la section ; valeur illisible : valeur par défaut.
    pub fn from_table(t: &toml::de::DeTable<'_>) -> MobilePrefs {
        let mut p = MobilePrefs::default();
        for (k, v) in t.iter() {
            let v = v.get_ref();
            match k.get_ref().as_ref() {
                K_LAYOUT => {
                    if let Some(l) = v.as_str().and_then(Layout::of) {
                        p.layout = l;
                    }
                }
                K_MARGIN => {
                    if let Some(i) = v.as_integer()
                        && let Ok(n) = i64::from_str_radix(i.as_str(), i.radix())
                    {
                        p.keyboard_margin = n.clamp(0, i64::from(MAX_KEYBOARD_MARGIN)) as u16;
                    }
                }
                _ => {}
            }
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::local::LocalPrefs;

    #[test]
    fn section_round_trips_and_ignores_garbage() {
        let p = LocalPrefs { mobile: MobilePrefs { layout: Layout::Mobile, keyboard_margin: 280 }, ..Default::default() };
        assert_eq!(LocalPrefs::parse(&p.to_toml()), Ok(p.clone()));
        let odd = LocalPrefs::parse("[mobile]\ndisposition = \"tablette\"\nmarge_clavier = 99999\n").unwrap_or_default();
        assert_eq!(odd.mobile, MobilePrefs { layout: Layout::Auto, keyboard_margin: MAX_KEYBOARD_MARGIN });
        assert_eq!(LocalPrefs::parse("").unwrap_or_default().mobile, MobilePrefs::default());
    }
}
