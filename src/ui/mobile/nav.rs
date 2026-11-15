//! Navigation par écrans de la disposition mobile, sans egui : onglet de la
//! barre du bas, pile d'écrans, retour, glissements, et choix du mode.

use crate::state::mobile::Layout;

/// En deçà de cette largeur (points), la disposition automatique est mobile.
pub const NARROW: f32 = 600.0;

/// Onglets de la barre du bas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    /// Rail des serveurs et salons.
    #[default]
    Home,
    /// Messages privés.
    Dms,
    /// Boîte de réception.
    Inbox,
    /// Profil et Paramètres.
    You,
}

/// Écrans empilés au-dessus de l'onglet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// Racine de l'onglet (liste des salons, MP, boîte, profil).
    List,
    /// Discussion (ou vue d'appel, forum, page Amis).
    Chat,
    /// Liste des membres, glissée depuis la droite par-dessus la discussion.
    Members,
}

/// Sens d'un glissement horizontal (vers où va le doigt).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Swipe {
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Nav {
    tab: Tab,
    /// Toujours non vide ; `stack[0]` est la racine de l'onglet.
    stack: Vec<Screen>,
}

impl Default for Nav {
    fn default() -> Self {
        Nav { tab: Tab::Home, stack: vec![Screen::List] }
    }
}

impl Nav {
    pub fn tab(&self) -> Tab {
        self.tab
    }

    pub fn screen(&self) -> Screen {
        self.stack.last().copied().unwrap_or(Screen::List)
    }

    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// La barre d'onglets n'est montrée qu'à la racine (comme Discord).
    pub fn shows_tab_bar(&self) -> bool {
        self.stack.len() <= 1
    }

    /// Onglet touché : sa racine. Toucher l'onglet courant y revient aussi.
    pub fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.stack.truncate(1);
    }

    /// Une discussion est ouverte (depuis la liste, la boîte, un lien…).
    pub fn open_chat(&mut self) {
        if matches!(self.tab, Tab::Inbox | Tab::You) {
            // Ouvrir un salon depuis la boîte : on revient à l'onglet qui
            // montre les salons, avec la discussion par-dessus.
            self.tab = Tab::Home;
        }
        self.stack = vec![Screen::List, Screen::Chat];
    }

    /// Panneau des membres, seulement au-dessus d'une discussion.
    pub fn open_members(&mut self) -> bool {
        if self.screen() == Screen::Chat {
            self.stack.push(Screen::Members);
            return true;
        }
        false
    }

    /// Retour : écran précédent, puis onglet Accueil ; `false` à la racine
    /// de l'Accueil (le système peut alors quitter l'application).
    pub fn back(&mut self) -> bool {
        if self.stack.len() > 1 {
            self.stack.pop();
            return true;
        }
        if self.tab != Tab::Home {
            self.tab = Tab::Home;
            return true;
        }
        false
    }

    /// Glissement horizontal : vers la droite, retour ; vers la gauche,
    /// l'écran suivant s'il existe (discussion ouverte, liste des membres).
    pub fn swipe(&mut self, dir: Swipe, has_chat: bool, has_members: bool) -> bool {
        match (dir, self.screen()) {
            (Swipe::Right, _) if self.stack.len() > 1 => self.back(),
            (Swipe::Left, Screen::List) if has_chat && matches!(self.tab, Tab::Home | Tab::Dms) => {
                self.open_chat();
                true
            }
            (Swipe::Left, Screen::Chat) if has_members => self.open_members(),
            _ => false,
        }
    }
}

/// Mode mobile ou non, selon le réglage, la largeur (points) et la plateforme.
pub fn decide(choice: Layout, width: f32, android: bool) -> bool {
    match choice {
        Layout::Desktop => false,
        Layout::Mobile => true,
        Layout::Auto => android || width < NARROW,
    }
}

/// Lieu affiché : (serveur, salon).
pub type Place = (Option<u64>, Option<u64>);

/// Après la liste des salons : un salon a été ouvert dans ce même serveur
/// (ou rouvert d'un toucher), ce qui mène à la discussion. Changer de
/// serveur dans le rail, lui, reste sur la liste.
pub fn opened_from_list(before: Place, after: Place, reopened: bool) -> bool {
    before.0 == after.0 && (before.1 != after.1 || (reopened && after.1.is_some()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_push_back_and_tabs() {
        let mut n = Nav::default();
        assert_eq!((n.tab(), n.screen(), n.depth()), (Tab::Home, Screen::List, 1));
        assert!(n.shows_tab_bar());
        assert!(!n.open_members(), "pas de membres sans discussion");
        n.open_chat();
        assert_eq!(n.screen(), Screen::Chat);
        assert!(!n.shows_tab_bar());
        assert!(n.open_members());
        assert_eq!(n.depth(), 3);
        assert!(n.back());
        assert_eq!(n.screen(), Screen::Chat);
        assert!(n.back());
        assert_eq!(n.screen(), Screen::List);
        assert!(!n.back(), "racine de l'Accueil : le système reprend la main");
        n.set_tab(Tab::You);
        assert_eq!((n.tab(), n.screen()), (Tab::You, Screen::List));
        assert!(n.back());
        assert_eq!(n.tab(), Tab::Home);
    }

    #[test]
    fn opening_from_inbox_goes_home_and_retap_pops() {
        let mut n = Nav::default();
        n.set_tab(Tab::Inbox);
        n.open_chat();
        assert_eq!((n.tab(), n.screen(), n.depth()), (Tab::Home, Screen::Chat, 2));
        n.set_tab(Tab::Dms);
        n.open_chat();
        assert_eq!(n.tab(), Tab::Dms, "les MP gardent leur onglet");
        n.set_tab(Tab::Dms);
        assert_eq!(n.screen(), Screen::List);
    }

    #[test]
    fn swipes() {
        let mut n = Nav::default();
        assert!(!n.swipe(Swipe::Right, true, true), "rien à droite de la racine");
        assert!(!n.swipe(Swipe::Left, false, true), "pas de salon ouvert");
        assert!(n.swipe(Swipe::Left, true, true));
        assert_eq!(n.screen(), Screen::Chat);
        assert!(!n.swipe(Swipe::Left, true, false), "MP à deux : pas de membres");
        assert!(n.swipe(Swipe::Left, true, true));
        assert_eq!(n.screen(), Screen::Members);
        assert!(!n.swipe(Swipe::Left, true, true));
        assert!(n.swipe(Swipe::Right, true, true));
        assert!(n.swipe(Swipe::Right, true, true));
        assert_eq!(n.screen(), Screen::List);
        n.set_tab(Tab::You);
        assert!(!n.swipe(Swipe::Left, true, true), "l'onglet Vous n'a pas de discussion");
    }

    #[test]
    fn mode_choice() {
        assert!(!decide(Layout::Auto, 1280.0, false));
        assert!(decide(Layout::Auto, 390.0, false));
        assert!(decide(Layout::Auto, 1280.0, true), "Android : toujours mobile en automatique");
        assert!(!decide(Layout::Auto, NARROW, false));
        assert!(!decide(Layout::Desktop, 390.0, true));
        assert!(decide(Layout::Mobile, 1920.0, false));
    }

    #[test]
    fn list_opens_chat_only_in_same_guild() {
        let g = Some(1);
        assert!(opened_from_list((g, Some(10)), (g, Some(11)), false));
        assert!(opened_from_list((g, Some(10)), (g, Some(10)), true), "salon rouvert d'un toucher");
        assert!(!opened_from_list((g, Some(10)), (g, Some(10)), false));
        assert!(!opened_from_list((g, Some(10)), (Some(2), Some(20)), true), "serveur changé dans le rail");
        assert!(opened_from_list((None, Some(5)), (None, None), false), "« Amis » dans les MP");
        assert!(!opened_from_list((None, None), (None, None), true));
    }
}
