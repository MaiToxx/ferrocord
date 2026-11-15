//! Raccourcis clavier : actions, combinaisons par défaut (celles de Discord,
//! « Raccourcis clavier » de son centre d'aide), lecture et écriture de
//! `keys.toml`. Ne dépend pas d'egui : une touche porte le nom que lui donne
//! egui (`Key::name`, par exemple `K`, `Up`, `Escape`, `Slash`) ;
//! l'interface fait la correspondance.
//!
//! Le fichier ne contient que les combinaisons changées par l'utilisateur ;
//! une action absente garde sa combinaison par défaut, `""` la retire.

use std::fmt::Write as _;

/// Taille au-delà de laquelle `keys.toml` est refusé sans être analysé.
pub const MAX_FILE: usize = 64 * 1024;

/// Ce qu'un raccourci déclenche.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    QuickSwitcher,
    Inbox,
    Shortcuts,
    Settings,
    PrevChannel,
    NextChannel,
    PrevUnread,
    NextUnread,
    PrevGuild,
    NextGuild,
    Back,
    Forward,
    MarkRead,
    MarkGuildRead,
    ReplyLast,
    PageUp,
    PageDown,
    Emojis,
    Pins,
    Members,
    // Vocal (phase 4, lot V4).
    ToggleMute,
    ToggleDeafen,
    PushToTalk,
    // Message sélectionné au clavier (lot C5).
    MsgReply,
    MsgEdit,
    MsgReact,
    MsgPin,
    MsgCopy,
    MsgDelete,
    // Raccourcis de Discord ajoutés par le lot N (phase 10).
    Search,
    Upload,
    Gifs,
    Stickers,
    CreateGuild,
    Threads,
    Help,
    OldestUnread,
    AnswerCall,
    DeclineCall,
    StartCall,
    // Créer un MP de groupe (lot R du groupe 3).
    CreateGroup,
}

impl Action {
    /// Toutes les actions, dans l'ordre de la page des réglages.
    pub const ALL: [Action; 41] = [
        Action::QuickSwitcher,
        Action::Inbox,
        Action::Shortcuts,
        Action::Settings,
        Action::PrevChannel,
        Action::NextChannel,
        Action::PrevUnread,
        Action::NextUnread,
        Action::PrevGuild,
        Action::NextGuild,
        Action::Back,
        Action::Forward,
        Action::MarkRead,
        Action::MarkGuildRead,
        Action::ReplyLast,
        Action::PageUp,
        Action::PageDown,
        Action::Emojis,
        Action::Pins,
        Action::Members,
        Action::ToggleMute,
        Action::ToggleDeafen,
        Action::PushToTalk,
        Action::MsgReply,
        Action::MsgEdit,
        Action::MsgReact,
        Action::MsgPin,
        Action::MsgCopy,
        Action::MsgDelete,
        Action::Search,
        Action::Upload,
        Action::Gifs,
        Action::Stickers,
        Action::CreateGuild,
        Action::Threads,
        Action::Help,
        Action::OldestUnread,
        Action::AnswerCall,
        Action::DeclineCall,
        Action::StartCall,
        Action::CreateGroup,
    ];

    /// Nom de l'action dans `keys.toml`.
    pub fn key(self) -> &'static str {
        match self {
            Action::QuickSwitcher => "selecteur_rapide",
            Action::Inbox => "boite_de_reception",
            Action::Shortcuts => "liste_des_raccourcis",
            Action::Settings => "parametres",
            Action::PrevChannel => "salon_precedent",
            Action::NextChannel => "salon_suivant",
            Action::PrevUnread => "non_lu_precedent",
            Action::NextUnread => "non_lu_suivant",
            Action::PrevGuild => "serveur_precedent",
            Action::NextGuild => "serveur_suivant",
            Action::Back => "precedent",
            Action::Forward => "suivant",
            Action::MarkRead => "marquer_lu",
            Action::MarkGuildRead => "marquer_serveur_lu",
            Action::ReplyLast => "repondre_dernier",
            Action::PageUp => "page_haut",
            Action::PageDown => "page_bas",
            Action::Emojis => "emojis",
            Action::Pins => "epingles",
            Action::Members => "membres",
            Action::ToggleMute => "vocal_muet",
            Action::ToggleDeafen => "vocal_sourdine",
            Action::PushToTalk => "vocal_appui_pour_parler",
            Action::MsgReply => "message_repondre",
            Action::MsgEdit => "message_modifier",
            Action::MsgReact => "message_reagir",
            Action::MsgPin => "message_epingler",
            Action::MsgCopy => "message_copier",
            Action::MsgDelete => "message_supprimer",
            Action::Search => "recherche",
            Action::Upload => "joindre_fichier",
            Action::Gifs => "gifs",
            Action::Stickers => "autocollants",
            Action::CreateGuild => "creer_serveur",
            Action::Threads => "fils",
            Action::Help => "aide",
            Action::OldestUnread => "plus_ancien_non_lu",
            Action::AnswerCall => "appel_repondre",
            Action::DeclineCall => "appel_refuser",
            Action::StartCall => "appel_demarrer",
            Action::CreateGroup => "creer_groupe",
        }
    }

    /// Action sur le message sélectionné : ne vaut que pendant une sélection
    /// (`ui::msg_select`), jamais comme raccourci général.
    pub fn on_message(self) -> bool {
        matches!(self, Action::MsgReply | Action::MsgEdit | Action::MsgReact | Action::MsgPin | Action::MsgCopy | Action::MsgDelete)
    }

    /// Réponse à un appel entrant : ne vaut que pendant qu'un appel sonne,
    /// et passe alors avant l'action générale de la même combinaison
    /// (Échap refuse l'appel au lieu de marquer le salon comme lu).
    pub fn on_ring(self) -> bool {
        matches!(self, Action::AnswerCall | Action::DeclineCall)
    }

    pub fn from_key(k: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.key() == k)
    }

    /// Combinaison par défaut (celle de Discord) ; `None` : aucune
    /// (appui-pour-parler, que Discord laisse à choisir).
    pub fn default_chord(self) -> Option<Chord> {
        let c = |ctrl, alt, shift, key: &str| Chord { ctrl, alt, shift, key: key.to_string() };
        Some(match self {
            Action::QuickSwitcher => c(true, false, false, "K"),
            Action::Inbox => c(true, false, false, "I"),
            Action::Shortcuts => c(true, false, false, "Slash"),
            Action::Settings => c(true, false, false, "Comma"),
            Action::PrevChannel => c(false, true, false, "Up"),
            Action::NextChannel => c(false, true, false, "Down"),
            Action::PrevUnread => c(false, true, true, "Up"),
            Action::NextUnread => c(false, true, true, "Down"),
            Action::PrevGuild => c(true, true, false, "Up"),
            Action::NextGuild => c(true, true, false, "Down"),
            Action::Back => c(false, true, false, "Left"),
            Action::Forward => c(false, true, false, "Right"),
            Action::MarkRead => c(false, false, false, "Escape"),
            Action::MarkGuildRead => c(false, false, true, "Escape"),
            Action::ReplyLast => c(true, false, true, "A"),
            Action::PageUp => c(false, false, false, "PageUp"),
            Action::PageDown => c(false, false, false, "PageDown"),
            Action::Emojis => c(true, false, false, "E"),
            Action::Pins => c(true, false, false, "P"),
            Action::Members => c(true, false, false, "U"),
            Action::ToggleMute => c(true, false, true, "M"),
            Action::ToggleDeafen => c(true, false, true, "D"),
            Action::PushToTalk => return None,
            // Comme Discord, message choisi au clavier.
            Action::MsgReply => c(false, false, false, "R"),
            Action::MsgEdit => c(false, false, false, "E"),
            Action::MsgReact => c(false, false, false, "Plus"),
            Action::MsgPin => c(false, false, false, "P"),
            Action::MsgCopy => c(true, false, false, "C"),
            Action::MsgDelete => c(false, false, false, "Backspace"),
            Action::Search => c(true, false, false, "F"),
            Action::Upload => c(true, false, true, "U"),
            Action::Gifs => c(true, false, false, "G"),
            Action::Stickers => c(true, false, false, "S"),
            Action::CreateGuild => c(true, false, true, "N"),
            Action::Threads => c(true, false, false, "T"),
            Action::Help => c(true, false, true, "H"),
            Action::OldestUnread => c(false, false, true, "PageUp"),
            Action::AnswerCall => c(true, false, false, "Enter"),
            Action::DeclineCall => c(false, false, false, "Escape"),
            Action::StartCall => c(true, false, false, "Quote"),
            // Comme Discord : Ctrl+Maj+T.
            Action::CreateGroup => c(true, false, true, "T"),
        })
    }
}

/// Une combinaison : modificateurs exacts et une touche (nom d'egui).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: String,
}

impl Chord {
    /// Pendant la saisie, cette combinaison écrirait ou déplacerait le
    /// curseur (touche de caractère ou d'édition sans Ctrl ni Alt) : elle ne
    /// se déclenche pas tant qu'un champ de texte a le clavier.
    pub fn disturbs_typing(&self) -> bool {
        !(self.ctrl || self.alt) && !is_special(&self.key)
    }

    /// Forme écrite dans `keys.toml` : `Ctrl+Alt+Shift+K`.
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        for (on, name) in [(self.ctrl, "Ctrl"), (self.alt, "Alt"), (self.shift, "Shift")] {
            if on {
                s.push_str(name);
                s.push('+');
            }
        }
        s.push_str(&self.key);
        s
    }

    /// Lit `Ctrl+Alt+Shift+K` (modificateurs dans n'importe quel ordre, sans
    /// casse ; `Maj` vaut `Shift`). `None` si la touche manque.
    pub fn parse(text: &str) -> Option<Chord> {
        let mut c = Chord { ctrl: false, alt: false, shift: false, key: String::new() };
        let parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let (key, mods) = parts.split_last()?;
        for m in mods {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => c.ctrl = true,
                "alt" => c.alt = true,
                "shift" | "maj" => c.shift = true,
                _ => return None,
            }
        }
        if key.is_empty() || !key.chars().all(|ch| ch.is_ascii_alphanumeric()) {
            return None;
        }
        c.key = (*key).to_string();
        Some(c)
    }
}

/// Touche sans caractère ni rôle d'édition (Échap, F1, Page haut…) : toutes
/// les autres (lettres, flèches, Entrée, Origine…) servent à la saisie.
fn is_special(key: &str) -> bool {
    matches!(key, "Escape" | "PageUp" | "PageDown") || (key.starts_with('F') && key.len() > 1 && key[1..].chars().all(|c| c.is_ascii_digit()))
}

/// Combinaison de chaque action (`None` : retirée par l'utilisateur).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bindings {
    chords: Vec<(Action, Option<Chord>)>,
}

impl Default for Bindings {
    fn default() -> Self {
        Bindings { chords: Action::ALL.iter().map(|a| (*a, a.default_chord())).collect() }
    }
}

impl Bindings {
    pub fn get(&self, a: Action) -> Option<&Chord> {
        self.chords.iter().find(|(x, _)| *x == a).and_then(|(_, c)| c.as_ref())
    }

    pub fn set(&mut self, a: Action, chord: Option<Chord>) {
        if let Some(slot) = self.chords.iter_mut().find(|(x, _)| *x == a) {
            slot.1 = chord;
        }
    }

    pub fn reset(&mut self, a: Action) {
        self.set(a, a.default_chord());
    }

    pub fn is_default(&self, a: Action) -> bool {
        self.get(a) == a.default_chord().as_ref()
    }

    /// Actions et combinaisons, dans l'ordre de [`Action::ALL`].
    pub fn iter(&self) -> impl Iterator<Item = (Action, Option<&Chord>)> {
        self.chords.iter().map(|(a, c)| (*a, c.as_ref()))
    }

    /// Action de cette combinaison (la première, en cas de conflit).
    pub fn action_of(&self, chord: &Chord) -> Option<Action> {
        self.actions_of(chord).next()
    }

    /// Actions de cette combinaison : celles d'un appel qui sonne d'abord,
    /// puis les autres dans l'ordre de [`Action::ALL`].
    pub fn actions_of<'a>(&'a self, chord: &'a Chord) -> impl Iterator<Item = Action> + 'a {
        let hits = move |ring: bool| self.chords.iter().filter(move |(a, c)| a.on_ring() == ring && c.as_ref() == Some(chord)).map(|(a, _)| *a);
        hits(true).chain(hits(false))
    }

    /// Autres actions qui ont la même combinaison que `a` (conflit signalé).
    pub fn conflicts(&self, a: Action) -> Vec<Action> {
        let Some(mine) = self.get(a) else { return Vec::new() };
        // Une action d'appel entrant partage sa combinaison sans conflit :
        // elle ne vaut que pendant la sonnerie.
        self.chords.iter().filter(|(x, c)| *x != a && x.on_ring() == a.on_ring() && c.as_ref() == Some(mine)).map(|(x, _)| *x).collect()
    }

    /// Lit `keys.toml` : `[raccourcis]` puis `action = "Ctrl+K"`. Les lignes
    /// fautives sont ignorées et rendues (numéro, texte) ; le reste vaut.
    pub fn parse(text: &str) -> (Bindings, Vec<(usize, String)>) {
        let mut b = Bindings::default();
        let mut bad = Vec::new();
        let mut in_section = true;
        for (i, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                in_section = line == "[raccourcis]";
                continue;
            }
            if !in_section {
                continue;
            }
            let parsed = line.split_once('=').and_then(|(k, v)| {
                let action = Action::from_key(k.trim())?;
                let v = v.trim();
                let v = v.strip_prefix('"')?.strip_suffix('"')?;
                if v.is_empty() { Some((action, None)) } else { Chord::parse(v).map(|c| (action, Some(c))) }
            });
            match parsed {
                Some((a, c)) => b.set(a, c),
                None => bad.push((i + 1, raw.to_string())),
            }
        }
        (b, bad)
    }

    /// Fichier complet : les combinaisons changées seulement (les autres
    /// suivent les valeurs par défaut, même si elles changent un jour).
    pub fn to_toml(&self) -> String {
        let mut s = String::from("# Raccourcis clavier de Ferrocord (combinaisons changées seulement).\n# Forme : action = \"Ctrl+Alt+Shift+Touche\", ou \"\" pour retirer le raccourci.\n[raccourcis]\n");
        for (a, c) in self.iter() {
            if self.is_default(a) {
                continue;
            }
            let _ = writeln!(s, "{} = \"{}\"", a.key(), c.map(Chord::to_text).unwrap_or_default());
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_discord_and_without_conflict() {
        let b = Bindings::default();
        assert_eq!(b.get(Action::QuickSwitcher).map(Chord::to_text).as_deref(), Some("Ctrl+K"));
        assert_eq!(b.get(Action::NextUnread).map(Chord::to_text).as_deref(), Some("Alt+Shift+Down"));
        assert_eq!(b.get(Action::PrevGuild).map(Chord::to_text).as_deref(), Some("Ctrl+Alt+Up"));
        // Vocal : muet et sourdine comme Discord ; appui-pour-parler à choisir.
        assert_eq!(b.get(Action::ToggleMute).map(Chord::to_text).as_deref(), Some("Ctrl+Shift+M"));
        assert_eq!(b.get(Action::ToggleDeafen).map(Chord::to_text).as_deref(), Some("Ctrl+Shift+D"));
        assert_eq!(b.get(Action::PushToTalk), None);
        assert!(b.is_default(Action::PushToTalk));
        for a in Action::ALL {
            assert!(b.conflicts(a).is_empty(), "{a:?}");
            assert_eq!(Action::from_key(a.key()), Some(a));
        }
        // Lot N : Échap refuse un appel qui sonne avant de marquer comme lu.
        assert_eq!(b.get(Action::Search).map(Chord::to_text).as_deref(), Some("Ctrl+F"));
        assert_eq!(b.get(Action::Upload).map(Chord::to_text).as_deref(), Some("Ctrl+Shift+U"));
        let esc = Chord::parse("Escape").unwrap_or_else(|| panic!("Escape"));
        assert_eq!(b.actions_of(&esc).collect::<Vec<_>>(), vec![Action::DeclineCall, Action::MarkRead]);
        assert!(Chord::parse("Shift+PageUp").is_some_and(|c| !c.disturbs_typing()));
        assert_eq!(b.to_toml().lines().filter(|l| !l.starts_with('#') && l.contains('=')).count(), 0, "rien de changé, rien d'écrit");
    }

    #[test]
    fn chords_round_trip_and_reject_garbage() {
        for t in ["Ctrl+K", "Alt+Shift+Up", "Escape", "Ctrl+Alt+F5"] {
            assert_eq!(Chord::parse(t).map(|c| c.to_text()).as_deref(), Some(t));
        }
        assert_eq!(Chord::parse("maj+ctrl+a"), Some(Chord { ctrl: true, alt: false, shift: true, key: "a".into() }));
        for bad in ["", "Ctrl+", "Hyper+K", "Ctrl+K K", "Ctrl+\"x\""] {
            assert_eq!(Chord::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn typing_guard() {
        let c = |t: &str| Chord::parse(t).unwrap_or_else(|| panic!("{t}"));
        assert!(c("K").disturbs_typing());
        assert!(c("Shift+K").disturbs_typing());
        assert!(c("Up").disturbs_typing());
        assert!(c("Shift+Home").disturbs_typing());
        assert!(!c("Ctrl+K").disturbs_typing());
        assert!(!c("Alt+Up").disturbs_typing());
        assert!(!c("Escape").disturbs_typing());
        assert!(!c("Shift+Escape").disturbs_typing());
        assert!(!c("PageDown").disturbs_typing());
        assert!(!c("F2").disturbs_typing());
    }

    #[test]
    fn file_keeps_changes_and_reports_bad_lines() {
        let mut b = Bindings::default();
        b.set(Action::QuickSwitcher, Chord::parse("Ctrl+T"));
        b.set(Action::Pins, None);
        let text = b.to_toml();
        let (back, bad) = Bindings::parse(&text);
        assert!(bad.is_empty(), "{bad:?}");
        assert_eq!(back, b);

        let (b, bad) = Bindings::parse("[raccourcis]\nselecteur_rapide = \"Ctrl+T\"\ninconnu = \"Ctrl+X\"\nmembres = Ctrl+U\n[autre]\nepingles = \"\"\n");
        assert_eq!(bad.iter().map(|(n, _)| *n).collect::<Vec<_>>(), vec![3, 4]);
        assert_eq!(b.get(Action::QuickSwitcher).map(Chord::to_text).as_deref(), Some("Ctrl+T"));
        assert!(b.get(Action::Pins).is_some(), "hors de [raccourcis] : ignoré");
    }

    #[test]
    fn conflicts_are_found_both_ways() {
        let mut b = Bindings::default();
        b.set(Action::Pins, Chord::parse("Ctrl+K"));
        assert_eq!(b.conflicts(Action::Pins), vec![Action::QuickSwitcher]);
        assert_eq!(b.conflicts(Action::QuickSwitcher), vec![Action::Pins]);
        assert_eq!(Action::QuickSwitcher.default_chord().and_then(|c| b.action_of(&c)), Some(Action::QuickSwitcher));
        b.reset(Action::Pins);
        assert!(b.conflicts(Action::QuickSwitcher).is_empty());
    }
}
