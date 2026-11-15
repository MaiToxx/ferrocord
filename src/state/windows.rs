//! Onglets de salons, vue scindée, fenêtres détachées et zoom par salon
//! (plugins du lot E2) : le modèle des onglets, et ce que `local.toml` en
//! retient (section `[fenetres]`). Rien ici ne touche au réseau : un onglet
//! n'est qu'un endroit (serveur, salon) ; son historique se charge quand
//! l'utilisateur l'ouvre, comme pour n'importe quel salon.

use super::nav::Place;
use std::collections::BTreeMap;

pub const SECTION: &str = "fenetres";
const K_TABS: &str = "onglets";
const K_LIST: &str = "liste";
const K_ACTIVE: &str = "actif";
const K_GUILD: &str = "serveur";
const K_CHANNEL: &str = "salon";
const K_PINNED: &str = "epingle";
const K_SPLIT: &str = "scinde";
const K_ZOOM: &str = "zoom";
const K_FRAMES: &str = "detachees";

/// Onglets gardés au plus (par compte).
pub const MAX_TABS: usize = 30;
/// Salons dont le zoom ou la place de fenêtre sont retenus, au plus.
pub const MAX_KEPT: usize = 200;
/// Bornes du zoom d'un salon, en % de la taille du thème.
pub const ZOOM_MIN: u16 = 60;
pub const ZOOM_MAX: u16 = 250;
/// Un cran de zoom (Ctrl+molette, Ctrl+= / Ctrl+-).
pub const ZOOM_STEP: u16 = 10;
/// Bornes de la séparation de la vue scindée, en % de la largeur.
pub const SPLIT_MIN: u8 = 25;
pub const SPLIT_MAX: u8 = 75;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tab {
    pub place: Place,
    /// Épinglé : rangé en tête, réduit à son icône, il garde son salon.
    pub pinned: bool,
}

/// Les onglets d'un compte. L'onglet actif est toujours l'endroit affiché.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tabs {
    pub list: Vec<Tab>,
    pub active: usize,
}

impl Tabs {
    fn pinned_count(&self) -> usize {
        self.list.iter().take_while(|t| t.pinned).count()
    }

    fn position(&self, p: Place) -> Option<usize> {
        self.list.iter().position(|t| t.place == p)
    }

    /// L'endroit affiché est `now` (navigation ordinaire) : l'onglet qui le
    /// montre déjà devient actif ; sinon l'onglet actif y va, sauf épinglé
    /// (il garde son salon : un onglet s'ouvre après les épinglés).
    /// Renvoie `true` si quelque chose a changé.
    pub fn observe(&mut self, now: Place) -> bool {
        if self.list.is_empty() {
            self.list.push(Tab { place: now, pinned: false });
            self.active = 0;
            return true;
        }
        self.active = self.active.min(self.list.len() - 1);
        if self.list[self.active].place == now {
            return false;
        }
        if let Some(i) = self.position(now) {
            self.active = i;
        } else if self.list[self.active].pinned {
            let at = (self.active + 1).max(self.pinned_count());
            self.list.insert(at, Tab { place: now, pinned: false });
            self.active = at;
            self.trim();
        } else {
            self.list[self.active].place = now;
        }
        true
    }

    /// Ouvre `p` dans un onglet, à droite de l'onglet actif, sans y aller
    /// (comme un clic molette dans un navigateur) ; déjà ouvert : son rang.
    pub fn open(&mut self, p: Place) -> usize {
        if let Some(i) = self.position(p) {
            return i;
        }
        let at = if self.list.is_empty() { 0 } else { (self.active + 1).max(self.pinned_count()).min(self.list.len()) };
        self.list.insert(at, Tab { place: p, pinned: false });
        if self.list.len() > MAX_TABS {
            // Le plus à droite qui n'est ni actif, ni épinglé, ni le nouveau.
            if let Some(drop) = (0..self.list.len()).rev().find(|i| *i != at && *i != self.active && !self.list[*i].pinned) {
                self.remove(drop);
                return self.position(p).unwrap_or(0);
            }
        }
        at
    }

    fn trim(&mut self) {
        while self.list.len() > MAX_TABS {
            let Some(drop) = (0..self.list.len()).rev().find(|i| *i != self.active && !self.list[*i].pinned) else { return };
            self.remove(drop);
        }
    }

    fn remove(&mut self, i: usize) {
        self.list.remove(i);
        if i < self.active {
            self.active -= 1;
        }
        self.active = self.active.min(self.list.len().saturating_sub(1));
    }

    /// Ferme l'onglet `i` (jamais le dernier). Si c'était l'actif, renvoie
    /// l'endroit du nouvel onglet actif : à l'appelant d'y aller.
    pub fn close(&mut self, i: usize) -> Option<Place> {
        if self.list.len() < 2 || i >= self.list.len() {
            return None;
        }
        let was_active = i == self.active;
        self.list.remove(i);
        if i < self.active || (was_active && self.active >= self.list.len()) {
            self.active -= 1;
        }
        was_active.then(|| self.list[self.active].place)
    }

    /// Ferme tous les onglets sauf `i` et les épinglés.
    pub fn close_others(&mut self, i: usize) -> Option<Place> {
        let keep = self.list.get(i)?.place;
        let was = self.list.get(self.active).map(|t| t.place);
        self.list.retain(|t| t.pinned || t.place == keep);
        let now = was.and_then(|p| self.position(p)).or_else(|| self.position(keep)).unwrap_or(0);
        self.active = now;
        (was != Some(self.list[now].place)).then(|| self.list[now].place)
    }

    /// Épingle ou désépingle : l'onglet rejoint la fin des épinglés.
    pub fn pin(&mut self, i: usize, pinned: bool) {
        let Some(tab) = self.list.get(i).copied() else { return };
        if tab.pinned == pinned {
            return;
        }
        let active = self.list.get(self.active).map(|t| t.place);
        self.list.remove(i);
        let at = self.pinned_count();
        self.list.insert(at, Tab { pinned, ..tab });
        self.active = active.and_then(|p| self.position(p)).unwrap_or(0);
    }

    /// Déplace l'onglet `from` au rang `to` ; épinglés et autres ne se mêlent pas.
    pub fn move_to(&mut self, from: usize, to: usize) -> usize {
        let Some(tab) = self.list.get(from).copied() else { return from };
        let pins = self.pinned_count();
        let to = if tab.pinned { to.min(pins.saturating_sub(1)) } else { to.clamp(pins, self.list.len() - 1) };
        if to == from {
            return from;
        }
        let active = self.list.get(self.active).map(|t| t.place);
        self.list.remove(from);
        self.list.insert(to, tab);
        self.active = active.and_then(|p| self.position(p)).unwrap_or(0);
        to
    }

    /// Endroit de l'onglet suivant ou précédent (en boucle) ; `None` s'il n'y en a qu'un.
    pub fn cycle(&self, forward: bool) -> Option<Place> {
        let n = self.list.len();
        if n < 2 {
            return None;
        }
        let i = if forward { (self.active + 1) % n } else { (self.active + n - 1) % n };
        Some(self.list[i].place)
    }

    /// Retire les onglets dont l'endroit n'existe plus (`exists`), sauf le dernier.
    pub fn retain(&mut self, exists: impl Fn(&Place) -> bool) -> bool {
        let before = self.list.len();
        let active = self.list.get(self.active).map(|t| t.place);
        let mut i = 0;
        while i < self.list.len() {
            if self.list.len() > 1 && Some(self.list[i].place) != active && !exists(&self.list[i].place) {
                self.list.remove(i);
            } else {
                i += 1;
            }
        }
        self.active = active.and_then(|p| self.position(p)).unwrap_or(0);
        self.list.len() != before
    }
}

/// Place d'une fenêtre détachée (points) : coin extérieur, taille intérieure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub pos: [f32; 2],
    pub size: [f32; 2],
}

impl Eq for Frame {}

/// Section `[fenetres]` de `local.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowPrefs {
    /// Onglets par compte (identifiant de l'utilisateur).
    pub tabs: BTreeMap<u64, Tabs>,
    /// Part de la largeur donnée à la discussion de gauche, en %.
    pub split: u8,
    /// Zoom du texte par salon, en % (absent : 100).
    pub zoom: BTreeMap<u64, u16>,
    /// Place de la fenêtre détachée de chaque salon.
    pub frames: BTreeMap<u64, Frame>,
}

impl Default for WindowPrefs {
    fn default() -> Self {
        WindowPrefs { tabs: BTreeMap::new(), split: 50, zoom: BTreeMap::new(), frames: BTreeMap::new() }
    }
}

/// Garde une table bornée : la plus petite clé (le salon le plus ancien) part d'abord.
fn bound<V>(map: &mut BTreeMap<u64, V>) {
    while map.len() > MAX_KEPT {
        map.pop_first();
    }
}

impl WindowPrefs {
    pub fn zoom_of(&self, channel: u64) -> u16 {
        self.zoom.get(&channel).copied().unwrap_or(100)
    }

    /// Règle le zoom d'un salon (100 : oublié). Renvoie `true` s'il a changé.
    pub fn set_zoom(&mut self, channel: u64, percent: u16) -> bool {
        let percent = percent.clamp(ZOOM_MIN, ZOOM_MAX);
        if self.zoom_of(channel) == percent {
            return false;
        }
        if percent == 100 {
            self.zoom.remove(&channel);
        } else {
            self.zoom.insert(channel, percent);
            bound(&mut self.zoom);
        }
        true
    }

    pub fn set_frame(&mut self, channel: u64, frame: Frame) {
        self.frames.insert(channel, frame);
        bound(&mut self.frames);
    }

    pub fn to_toml(&self) -> String {
        use std::fmt::Write as _;
        let mut s = format!("[{SECTION}]\n# Onglets, vue scindée, fenêtres détachées, zoom par salon (plugins).\n{K_SPLIT} = {}\n", self.split);
        if !self.zoom.is_empty() {
            let _ = writeln!(s, "\n[{SECTION}.{K_ZOOM}]");
            for (c, z) in &self.zoom {
                let _ = writeln!(s, "{c} = {z}");
            }
        }
        if !self.frames.is_empty() {
            let _ = writeln!(s, "\n[{SECTION}.{K_FRAMES}]");
            for (c, f) in &self.frames {
                let _ = writeln!(s, "{c} = [{}, {}, {}, {}]", f.pos[0].round(), f.pos[1].round(), f.size[0].round(), f.size[1].round());
            }
        }
        for (user, tabs) in self.tabs.iter().filter(|(_, t)| !t.list.is_empty()) {
            let rows: Vec<String> = tabs
                .list
                .iter()
                .map(|t| {
                    let mut row = String::from("{ ");
                    if let Some(g) = t.place.guild {
                        let _ = write!(row, "{K_GUILD} = {g}, ");
                    }
                    if let Some(c) = t.place.channel {
                        let _ = write!(row, "{K_CHANNEL} = {c}, ");
                    }
                    let _ = write!(row, "{K_PINNED} = {} }}", t.pinned);
                    row
                })
                .collect();
            let _ = write!(s, "\n[{SECTION}.{K_TABS}.{user}]\n{K_ACTIVE} = {}\n{K_LIST} = [{}]\n", tabs.active, rows.join(", "));
        }
        s
    }

    /// Lit la section ; clé inconnue ou valeur illisible : ignorée.
    pub fn from_table(t: &toml::de::DeTable<'_>) -> WindowPrefs {
        use toml::de::DeValue;
        let int = |v: &DeValue<'_>| v.as_integer().and_then(|i| i64::from_str_radix(i.as_str(), i.radix()).ok());
        let num = |v: &DeValue<'_>| int(v).map(|n| n as f32).or_else(|| v.as_float().and_then(|f| f.as_str().parse::<f32>().ok())).filter(|n| n.is_finite());
        let id = |v: &DeValue<'_>| int(v).and_then(|n| u64::try_from(n).ok()).filter(|n| *n != 0);
        let mut p = WindowPrefs::default();
        for (k, v) in t.iter() {
            let v = v.get_ref();
            match k.get_ref().as_ref() {
                K_SPLIT => {
                    if let Some(n) = int(v) {
                        p.split = n.clamp(i64::from(SPLIT_MIN), i64::from(SPLIT_MAX)) as u8;
                    }
                }
                K_ZOOM => {
                    let Some(rows) = v.as_table() else { continue };
                    for (c, z) in rows.iter() {
                        if let (Ok(c), Some(z)) = (c.get_ref().parse::<u64>(), int(z.get_ref())) {
                            p.set_zoom(c, z.clamp(i64::from(ZOOM_MIN), i64::from(ZOOM_MAX)) as u16);
                        }
                    }
                }
                K_FRAMES => {
                    let Some(rows) = v.as_table() else { continue };
                    for (c, f) in rows.iter() {
                        let (Ok(c), Some(a)) = (c.get_ref().parse::<u64>(), f.get_ref().as_array()) else { continue };
                        let n: Vec<f32> = a.iter().filter_map(|x| num(x.get_ref())).collect();
                        if let [x, y, w, h] = n[..] {
                            p.set_frame(c, Frame { pos: [x, y], size: [w.max(1.0), h.max(1.0)] });
                        }
                    }
                }
                K_TABS => {
                    let Some(users) = v.as_table() else { continue };
                    for (user, one) in users.iter() {
                        let (Ok(user), Some(one)) = (user.get_ref().parse::<u64>(), one.get_ref().as_table()) else { continue };
                        let mut tabs = Tabs::default();
                        for (k, v) in one.iter() {
                            let v = v.get_ref();
                            match k.get_ref().as_ref() {
                                K_ACTIVE => tabs.active = int(v).and_then(|n| usize::try_from(n).ok()).unwrap_or(0),
                                K_LIST => {
                                    let Some(rows) = v.as_array() else { continue };
                                    for row in rows.iter().take(MAX_TABS) {
                                        let Some(e) = row.get_ref().as_table() else { continue };
                                        let field = |key: &str| e.iter().find(|(k, _)| k.get_ref().as_ref() == key).map(|(_, v)| v.get_ref());
                                        let place = Place { guild: field(K_GUILD).and_then(id), channel: field(K_CHANNEL).and_then(id) };
                                        if !tabs.list.iter().any(|t| t.place == place) {
                                            tabs.list.push(Tab { place, pinned: field(K_PINNED).and_then(|v| v.as_bool()).unwrap_or(false) });
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        // Épinglés en tête, comme l'interface les range.
                        tabs.list.sort_by_key(|t| !t.pinned);
                        tabs.active = tabs.active.min(tabs.list.len().saturating_sub(1));
                        if !tabs.list.is_empty() {
                            p.tabs.insert(user, tabs);
                        }
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

    fn at(guild: u64, channel: u64) -> Place {
        Place { guild: Some(guild), channel: Some(channel) }
    }

    fn places(t: &Tabs) -> Vec<u64> {
        t.list.iter().filter_map(|t| t.place.channel).collect()
    }

    #[test]
    fn navigation_moves_the_active_tab_and_never_duplicates() {
        let mut t = Tabs::default();
        assert!(t.observe(at(1, 10)));
        assert!(!t.observe(at(1, 10)), "même endroit : rien");
        t.observe(at(1, 11));
        assert_eq!(places(&t), vec![11], "un seul onglet : il suit la navigation");
        // Ouvert en arrière-plan, à droite : l'onglet actif ne bouge pas.
        assert_eq!(t.open(at(1, 12)), 1);
        assert_eq!((places(&t), t.active), (vec![11, 12], 0));
        assert_eq!(t.open(at(1, 12)), 1, "déjà ouvert");
        // Aller à un endroit déjà ouvert active son onglet.
        t.observe(at(1, 12));
        assert_eq!((places(&t), t.active), (vec![11, 12], 1));
        // Un onglet épinglé garde son salon : la navigation ouvre un onglet.
        t.pin(1, true);
        assert_eq!((places(&t), t.active), (vec![12, 11], 0));
        t.observe(at(1, 13));
        assert_eq!((places(&t), t.active), (vec![12, 13, 11], 1));
    }

    #[test]
    fn close_reorder_and_cycle() {
        let mut t = Tabs::default();
        for c in [10, 11, 12, 13] {
            t.observe(at(1, 10));
            t.open(at(1, c));
        }
        assert_eq!(places(&t), vec![10, 13, 12, 11]);
        assert_eq!(t.cycle(true), Some(at(1, 13)));
        assert_eq!(t.cycle(false), Some(at(1, 11)), "en boucle");
        assert_eq!(t.close(2), None, "un autre que l'actif : on reste");
        assert_eq!(t.close(0), Some(at(1, 13)), "l'actif : son voisin de droite");
        assert_eq!((places(&t), t.active), (vec![13, 11], 0));
        assert_eq!(t.close(0), Some(at(1, 11)));
        assert_eq!(t.close(0), None, "jamais le dernier");
        t.open(at(1, 20));
        t.open(at(1, 21));
        t.pin(2, true);
        assert_eq!(places(&t), vec![20, 11, 21]);
        // Un onglet ordinaire ne passe pas devant un épinglé, et l'inverse.
        assert_eq!(t.move_to(2, 0), 1);
        assert_eq!(places(&t), vec![20, 21, 11]);
        assert_eq!(t.move_to(0, 2), 0);
        assert_eq!(t.list[t.active].place, at(1, 11), "l'actif reste l'actif");
        assert_eq!(t.close_others(1), Some(at(1, 21)));
        assert_eq!(places(&t), vec![20, 21], "les épinglés restent");
        assert!(t.retain(|p| p.channel != Some(20)));
        assert_eq!(places(&t), vec![21]);
    }

    #[test]
    fn tab_count_is_bounded() {
        let mut t = Tabs::default();
        t.observe(at(1, 1));
        for c in 2..100 {
            t.open(at(1, c));
        }
        assert_eq!(t.list.len(), MAX_TABS);
        assert_eq!(t.list[t.active].place, at(1, 1));
    }

    #[test]
    fn section_round_trips_and_ignores_garbage() {
        let mut p = WindowPrefs { split: 40, ..Default::default() };
        let mut tabs = Tabs::default();
        tabs.observe(at(1, 10));
        tabs.open(Place { guild: None, channel: Some(50) });
        tabs.open(Place { guild: None, channel: None });
        tabs.pin(2, true);
        p.tabs.insert(7, tabs);
        assert!(p.set_zoom(10, 130));
        assert!(!p.set_zoom(10, 130));
        assert!(p.set_zoom(11, 9999));
        assert_eq!(p.zoom_of(11), ZOOM_MAX);
        p.set_frame(50, Frame { pos: [-20.0, 40.0], size: [640.0, 480.0] });
        let text = p.to_toml();
        let root = toml::de::DeTable::parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        let section = root.get_ref().iter().find(|(k, _)| k.get_ref().as_ref() == SECTION).and_then(|(_, v)| v.get_ref().as_table()).unwrap_or_else(|| panic!("section"));
        assert_eq!(WindowPrefs::from_table(section), p, "{text}");
        assert!(p.set_zoom(10, 100));
        assert!(!p.zoom.contains_key(&10), "100 % : oublié");
        let odd = "[fenetres]\nscinde = 99\nzoom = 3\n[fenetres.onglets.x]\nactif = 1\n[fenetres.onglets.4]\nactif = 9\nliste = [{ salon = 5 }, 3, { salon = 5 }]\n";
        let root = toml::de::DeTable::parse(odd).unwrap_or_else(|e| panic!("{e}"));
        let section = root.get_ref().iter().find(|(k, _)| k.get_ref().as_ref() == SECTION).and_then(|(_, v)| v.get_ref().as_table()).unwrap_or_else(|| panic!("section"));
        let read = WindowPrefs::from_table(section);
        assert_eq!(read.split, SPLIT_MAX);
        assert_eq!(read.tabs.get(&4).map(|t| (t.list.len(), t.active)), Some((1, 0)));
    }
}
