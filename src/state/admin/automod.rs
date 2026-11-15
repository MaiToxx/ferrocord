//! AutoMod (lot A5) : règles d'un serveur et leurs limites, telles que les
//! décrit la ressource Auto Moderation (documentée par Discord ; limites
//! relues sur docs.discord.food le 13 novembre 2026).

/// Limites documentées.
pub const NAME_MAX: usize = 100;
pub const KEYWORDS_MAX: usize = 1000;
pub const KEYWORD_LEN_MAX: usize = 60;
pub const REGEX_MAX: usize = 10;
pub const REGEX_LEN_MAX: usize = 260;
pub const ALLOW_MAX_KEYWORD: usize = 100;
pub const ALLOW_MAX_PRESET: usize = 1000;
pub const MENTION_LIMIT_MAX: u32 = 50;
pub const TIMEOUT_MAX_SECS: u32 = 2_419_200;
pub const EXEMPT_ROLES_MAX: usize = 20;
pub const EXEMPT_CHANNELS_MAX: usize = 50;
/// Message personnalisé d'un blocage (limite de l'application officielle,
/// la documentation n'en donne pas).
pub const CUSTOM_MESSAGE_MAX: usize = 150;

/// Listes prédéfinies (`presets`).
pub const PRESET_PROFANITY: u8 = 1;
pub const PRESET_SEXUAL: u8 = 2;
pub const PRESET_SLURS: u8 = 3;

/// Ce qui déclenche une règle (`trigger_type` et `trigger_metadata`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// 1 : mots-clés et motifs.
    Keyword { keywords: Vec<String>, regex: Vec<String>, allow: Vec<String> },
    /// 3 : contenu suspect de spam.
    Spam,
    /// 4 : listes prédéfinies.
    Preset { presets: Vec<u8>, allow: Vec<String> },
    /// 5 : trop de mentions.
    MentionSpam { limit: u32, raid: bool },
    /// 6 : profil de membre (nom, pseudo).
    Profile { keywords: Vec<String>, regex: Vec<String>, allow: Vec<String> },
    /// Type inconnu de Ferrocord : affiché, jamais modifié.
    Other(u8),
}

impl Trigger {
    pub fn code(&self) -> u8 {
        match self {
            Trigger::Keyword { .. } => 1,
            Trigger::Spam => 3,
            Trigger::Preset { .. } => 4,
            Trigger::MentionSpam { .. } => 5,
            Trigger::Profile { .. } => 6,
            Trigger::Other(c) => *c,
        }
    }

    /// `event_type` : 2 (membre) pour le profil, 1 (message envoyé) sinon.
    pub fn event_type(&self) -> u8 {
        if matches!(self, Trigger::Profile { .. }) { 2 } else { 1 }
    }

    /// Nouveau déclencheur vide de ce type.
    pub fn empty(code: u8) -> Trigger {
        match code {
            1 => Trigger::Keyword { keywords: Vec::new(), regex: Vec::new(), allow: Vec::new() },
            3 => Trigger::Spam,
            4 => Trigger::Preset { presets: vec![PRESET_PROFANITY], allow: Vec::new() },
            5 => Trigger::MentionSpam { limit: 20, raid: true },
            6 => Trigger::Profile { keywords: Vec::new(), regex: Vec::new(), allow: Vec::new() },
            c => Trigger::Other(c),
        }
    }
}

/// Réponse d'une règle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleAction {
    /// 1 : bloquer le message, avec une explication facultative.
    Block { message: Option<String> },
    /// 2 : alerte dans un salon.
    Alert { channel_id: u64 },
    /// 3 : exclusion temporaire (secondes).
    Timeout { seconds: u32 },
    /// 4 : bloquer les interactions du membre (profil).
    Quarantine,
    Other(u8),
}

impl RuleAction {
    pub fn code(&self) -> u8 {
        match self {
            RuleAction::Block { .. } => 1,
            RuleAction::Alert { .. } => 2,
            RuleAction::Timeout { .. } => 3,
            RuleAction::Quarantine => 4,
            RuleAction::Other(c) => *c,
        }
    }
}

/// Règle AutoMod (`id` 0 : pas encore créée).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub id: u64,
    pub name: String,
    pub enabled: bool,
    pub trigger: Trigger,
    pub actions: Vec<RuleAction>,
    pub exempt_roles: Vec<u64>,
    pub exempt_channels: Vec<u64>,
}

impl Rule {
    pub fn new(trigger: Trigger, name: String) -> Rule {
        let actions = if matches!(trigger, Trigger::Profile { .. }) { vec![RuleAction::Quarantine] } else { vec![RuleAction::Block { message: None }] };
        Rule { id: 0, name, enabled: true, trigger, actions, exempt_roles: Vec::new(), exempt_channels: Vec::new() }
    }

    /// Ce type de règle peut-il porter cette action ? (Documentation : Action Types.)
    pub fn allows(&self, action: u8) -> bool {
        match (&self.trigger, action) {
            (Trigger::Profile { .. }, 4 | 2) => true,
            (Trigger::Profile { .. }, _) => false,
            (_, 4) => false,
            (Trigger::Keyword { .. } | Trigger::MentionSpam { .. }, 3) => true,
            (_, 3) => false,
            (_, 1 | 2) => true,
            _ => false,
        }
    }

    /// Règle prête à partir, ou la première chose qui ne va pas.
    pub fn check(&self) -> Result<(), RuleError> {
        let n = self.name.trim().chars().count();
        if n == 0 || n > NAME_MAX {
            return Err(RuleError::Name);
        }
        let words = |list: &[String], max: usize, len: usize| list.len() <= max && list.iter().all(|k| (1..=len).contains(&k.chars().count()));
        match &self.trigger {
            Trigger::Keyword { keywords, regex, allow } | Trigger::Profile { keywords, regex, allow } => {
                if keywords.is_empty() && regex.is_empty() {
                    return Err(RuleError::Empty);
                }
                if !words(keywords, KEYWORDS_MAX, KEYWORD_LEN_MAX) {
                    return Err(RuleError::Keywords);
                }
                if !words(regex, REGEX_MAX, REGEX_LEN_MAX) {
                    return Err(RuleError::Regex);
                }
                if !words(allow, ALLOW_MAX_KEYWORD, KEYWORD_LEN_MAX) {
                    return Err(RuleError::Allow);
                }
            }
            Trigger::Preset { presets, allow } => {
                if presets.is_empty() || presets.iter().any(|p| !(PRESET_PROFANITY..=PRESET_SLURS).contains(p)) {
                    return Err(RuleError::Empty);
                }
                if !words(allow, ALLOW_MAX_PRESET, KEYWORD_LEN_MAX) {
                    return Err(RuleError::Allow);
                }
            }
            Trigger::MentionSpam { limit, .. } => {
                if !(1..=MENTION_LIMIT_MAX).contains(limit) {
                    return Err(RuleError::Mentions);
                }
            }
            Trigger::Spam => {}
            Trigger::Other(_) => return Err(RuleError::Unsupported),
        }
        if self.actions.is_empty() {
            return Err(RuleError::NoAction);
        }
        for a in &self.actions {
            if !self.allows(a.code()) {
                return Err(RuleError::Action);
            }
            match a {
                RuleAction::Timeout { seconds } if *seconds == 0 || *seconds > TIMEOUT_MAX_SECS => return Err(RuleError::Action),
                RuleAction::Alert { channel_id: 0 } => return Err(RuleError::AlertChannel),
                RuleAction::Block { message: Some(m) } if m.chars().count() > CUSTOM_MESSAGE_MAX => return Err(RuleError::Action),
                RuleAction::Other(_) => return Err(RuleError::Unsupported),
                _ => {}
            }
        }
        if self.exempt_roles.len() > EXEMPT_ROLES_MAX || self.exempt_channels.len() > EXEMPT_CHANNELS_MAX {
            return Err(RuleError::Exempt);
        }
        Ok(())
    }
}

/// Ce qui empêche d'enregistrer une règle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleError {
    Name,
    Empty,
    Keywords,
    Regex,
    Allow,
    Mentions,
    NoAction,
    Action,
    AlertChannel,
    Exempt,
    Unsupported,
}

/// Liste découpée d'une saisie : une entrée par ligne ou par virgule, sans vides.
pub fn split_list(text: &str) -> Vec<String> {
    text.split(['\n', ',']).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// Message bloqué ou alerte reçue (AUTO_MODERATION_ACTION_EXECUTION).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Execution {
    pub guild_id: u64,
    pub rule_id: u64,
    pub trigger: u8,
    pub action: u8,
    pub user_id: u64,
    pub channel_id: Option<u64>,
    pub content: String,
    pub matched: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kw(words: &[&str]) -> Rule {
        let t = Trigger::Keyword { keywords: words.iter().map(|s| s.to_string()).collect(), regex: Vec::new(), allow: Vec::new() };
        Rule::new(t, "Mots interdits".into())
    }

    #[test]
    fn limits_are_checked_before_sending() {
        assert_eq!(kw(&["chat*"]).check(), Ok(()));
        assert_eq!(kw(&[]).check(), Err(RuleError::Empty));
        assert_eq!(kw(&[&"x".repeat(61)]).check(), Err(RuleError::Keywords));
        let mut r = kw(&["a"]);
        r.name = "  ".into();
        assert_eq!(r.check(), Err(RuleError::Name));
        let mut r = kw(&["a"]);
        r.actions = vec![RuleAction::Timeout { seconds: TIMEOUT_MAX_SECS + 1 }];
        assert_eq!(r.check(), Err(RuleError::Action));
        r.actions = vec![RuleAction::Timeout { seconds: 60 }, RuleAction::Alert { channel_id: 0 }];
        assert_eq!(r.check(), Err(RuleError::AlertChannel));
        r.actions.clear();
        assert_eq!(r.check(), Err(RuleError::NoAction));
        // Exclusion temporaire : mots-clés et mentions seulement.
        let mut spam = Rule::new(Trigger::Spam, "Spam".into());
        assert!(!spam.allows(3) && spam.allows(1));
        spam.actions = vec![RuleAction::Timeout { seconds: 60 }];
        assert_eq!(spam.check(), Err(RuleError::Action));
        let profile = Rule::new(Trigger::empty(6), "Profil".into());
        assert_eq!(profile.actions, vec![RuleAction::Quarantine]);
        assert_eq!(profile.trigger.event_type(), 2);
        assert_eq!(Rule::new(Trigger::MentionSpam { limit: 51, raid: false }, "m".into()).check(), Err(RuleError::Mentions));
        let mut r = kw(&["a"]);
        r.exempt_roles = (0..21).collect();
        assert_eq!(r.check(), Err(RuleError::Exempt));
        assert_eq!(split_list(" a, b \n\n c ,"), vec!["a", "b", "c"]);
    }
}
