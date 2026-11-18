//! Catégories de contacts : des groupes que l'on crée soi-même pour ranger
//! ses amis (« Famille », « Travail »…). Discord n'a rien de tel : tout reste
//! sur cet ordinateur, dans `local.toml` (section `[categories_amis]`).
//! Une personne peut être dans plusieurs catégories. Ni egui, ni réseau, ni disque.

pub const SECTION: &str = "categories_amis";
const K_LIST: &str = "categories";
const K_NAME: &str = "nom";
const K_MEMBERS: &str = "membres";

/// Catégories gardées au plus.
pub const MAX_GROUPS: usize = 30;
/// Longueur d'un nom (caractères).
pub const MAX_NAME: usize = 32;
/// Personnes par catégorie au plus.
pub const MAX_MEMBERS: usize = 1000;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FriendGroup {
    pub name: String,
    pub members: Vec<u64>,
}

/// Catégories, dans l'ordre de création.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FriendGroups {
    pub groups: Vec<FriendGroup>,
}

/// Nom tel qu'il est gardé : sans blancs autour ni caractères de contrôle, borné.
pub fn clean(name: &str) -> String {
    name.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME).collect::<String>().trim().to_string()
}

impl FriendGroups {
    fn position(&self, name: &str) -> Option<usize> {
        let wanted = name.to_lowercase();
        self.groups.iter().position(|g| g.name.to_lowercase() == wanted)
    }

    /// Crée une catégorie ; faux si le nom est vide, déjà pris (à la casse près) ou de trop.
    pub fn create(&mut self, name: &str) -> bool {
        let name = clean(name);
        if name.is_empty() || self.groups.len() >= MAX_GROUPS || self.position(&name).is_some() {
            return false;
        }
        self.groups.push(FriendGroup { name, members: Vec::new() });
        true
    }

    pub fn delete(&mut self, name: &str) {
        if let Some(i) = self.position(name) {
            self.groups.remove(i);
        }
    }

    /// Renomme ; faux si le nouveau nom est vide ou déjà pris par une autre.
    pub fn rename(&mut self, name: &str, new: &str) -> bool {
        let new = clean(new);
        let Some(i) = self.position(name) else { return false };
        if new.is_empty() || self.position(&new).is_some_and(|j| j != i) {
            return false;
        }
        self.groups[i].name = new;
        true
    }

    /// Met une personne dans une catégorie, ou l'en retire.
    pub fn set(&mut self, name: &str, user: u64, inside: bool) {
        let Some(i) = self.position(name) else { return };
        let m = &mut self.groups[i].members;
        m.retain(|u| *u != user);
        if inside && user != 0 && m.len() < MAX_MEMBERS {
            m.push(user);
        }
    }

    pub fn contains(&self, name: &str, user: u64) -> bool {
        self.position(name).is_some_and(|i| self.groups[i].members.contains(&user))
    }

    /// Membres d'une catégorie ; `None` : elle n'existe pas (ou plus).
    pub fn members(&self, name: &str) -> Option<&[u64]> {
        self.position(name).map(|i| self.groups[i].members.as_slice())
    }

    /// Section `[categories_amis]` de `local.toml`.
    pub fn to_toml(&self) -> String {
        let q = super::audio_prefs::quoted;
        let list: Vec<String> = self
            .groups
            .iter()
            .map(|g| {
                let members: Vec<String> = g.members.iter().map(u64::to_string).collect();
                format!("{{ {K_NAME} = {}, {K_MEMBERS} = [{}] }}", q(&g.name), members.join(", "))
            })
            .collect();
        format!("[{SECTION}]\n# Catégories de contacts (page Amis), gardées sur cet ordinateur seulement.\n{K_LIST} = [{}]\n", list.join(", "))
    }

    /// Lit la section ; entrée illisible : ignorée.
    pub fn from_table(t: &toml::de::DeTable<'_>) -> FriendGroups {
        let id = |v: &toml::de::DeValue<'_>| v.as_integer().and_then(|i| u64::from_str_radix(i.as_str(), i.radix()).ok()).filter(|id| *id != 0);
        let mut p = FriendGroups::default();
        for (k, v) in t.iter() {
            if k.get_ref().as_ref() != K_LIST {
                continue;
            }
            let Some(items) = v.get_ref().as_array() else { continue };
            for item in items.iter() {
                let Some(e) = item.get_ref().as_table() else { continue };
                let field = |key: &str| e.iter().find(|(k, _)| k.get_ref().as_ref() == key).map(|(_, v)| v.get_ref());
                let name = field(K_NAME).and_then(|v| v.as_str()).map(clean).unwrap_or_default();
                if !p.create(&name) {
                    continue;
                }
                for user in field(K_MEMBERS).and_then(|v| v.as_array()).into_iter().flat_map(|a| a.iter()).filter_map(|v| id(v.get_ref())) {
                    p.set(&name, user, true);
                }
            }
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_are_created_filled_renamed_and_deleted() {
        let mut g = FriendGroups::default();
        assert!(g.create("  Famille "));
        assert!(!g.create("famille"), "même nom à la casse près");
        assert!(!g.create("   "));
        assert!(g.create("Travail"));
        g.set("Famille", 5, true);
        g.set("famille", 5, true);
        g.set("Famille", 6, true);
        g.set("Travail", 5, true);
        g.set("Inconnue", 5, true);
        assert_eq!(g.members("Famille"), Some(&[5, 6][..]));
        assert!(g.contains("Travail", 5) && !g.contains("Travail", 6));
        g.set("Famille", 5, false);
        assert_eq!(g.members("Famille"), Some(&[6][..]));
        assert!(!g.rename("Famille", "travail"), "nom déjà pris");
        assert!(g.rename("Famille", "Proches") && g.rename("Proches", "proches"));
        assert_eq!(g.members("proches"), Some(&[6][..]));
        g.delete("Travail");
        assert_eq!(g.members("Travail"), None);
        // Bornes : nom coupé, nombre de catégories.
        let mut g = FriendGroups::default();
        assert!(g.create(&"é".repeat(80)));
        assert_eq!(g.groups[0].name.chars().count(), MAX_NAME);
        for i in 0..MAX_GROUPS * 2 {
            g.create(&format!("c{i}"));
        }
        assert_eq!(g.groups.len(), MAX_GROUPS);
    }

    #[test]
    fn categories_survive_the_file() {
        let mut g = FriendGroups::default();
        g.create("Jeux \"du soir\"");
        g.create("Vide");
        g.set("Jeux \"du soir\"", 123_456_789_012_345_678, true);
        g.set("Jeux \"du soir\"", 7, true);
        let text = g.to_toml();
        let doc = toml::de::DeTable::parse(&text).expect("section lisible");
        let section = doc.get_ref().iter().find(|(k, _)| k.get_ref().as_ref() == SECTION).and_then(|(_, v)| v.get_ref().as_table()).expect("section présente");
        assert_eq!(FriendGroups::from_table(section), g);
    }
}
