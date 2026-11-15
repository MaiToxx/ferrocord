//! Accueil des nouveaux membres (lot A7) : fonctions du serveur (`features`,
//! salon des règles, salon des mises à jour), salons vocaux et de conférence
//! (lieux des événements), écran de bienvenue et intégration (onboarding),
//! lecture, réponses d'un membre et édition. Réseau : `net::admin::community`.

use super::events::Failure;
use crate::state::event::Cmd;
use crate::state::perms::{base_permissions, Permissions};
use crate::state::store::Store;
use std::collections::{HashMap, HashSet};

/// Écran de bienvenue (documentation) : description de 140 caractères au
/// plus, 5 salons au plus, description d'un salon de 1 à 50 caractères.
pub const WELCOME_DESCRIPTION_MAX: usize = 140;
pub const WELCOME_CHANNELS_MAX: usize = 5;
pub const WELCOME_CHANNEL_DESCRIPTION_MAX: usize = 50;
/// Intégration : limites de l'application officielle (non chiffrées par la
/// documentation), appliquées avant l'envoi pour ne pas essuyer de refus.
pub const PROMPTS_MAX: usize = 15;
pub const OPTIONS_MAX: usize = 50;
pub const TITLE_MAX: usize = 100;
/// Réponses d'un membre : 750 au plus (documentation).
pub const RESPONSES_MAX: usize = 750;
/// Début de l'ère des identifiants de Discord (ms Unix).
const DISCORD_EPOCH: i64 = 1_420_070_400_000;

/// Fonctions qui intéressent ce lot (chaînes de la documentation).
pub const COMMUNITY: &str = "COMMUNITY";
pub const WELCOME_SCREEN_ENABLED: &str = "WELCOME_SCREEN_ENABLED";
pub const GUILD_ONBOARDING: &str = "GUILD_ONBOARDING";
pub const DISCOVERABLE: &str = "DISCOVERABLE";

/// Salon vocal ou de conférence (le magasin principal ne garde que les salons écrits).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Venue {
    pub id: u64,
    pub name: String,
    pub stage: bool,
    pub position: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GuildInfo {
    pub features: Vec<String>,
    pub rules_channel: Option<u64>,
    pub updates_channel: Option<u64>,
    pub description: String,
    pub venues: Vec<Venue>,
}

impl GuildInfo {
    pub fn has(&self, feature: &str) -> bool {
        self.features.iter().any(|f| f == feature)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WelcomeChannel {
    pub channel_id: u64,
    pub description: String,
    pub emoji_id: Option<u64>,
    /// Emoji Unicode, ou nom de l'emoji personnalisé.
    pub emoji_name: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WelcomeScreen {
    pub description: String,
    pub channels: Vec<WelcomeChannel>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PromptOption {
    pub id: u64,
    pub title: String,
    pub description: String,
    pub channel_ids: Vec<u64>,
    pub role_ids: Vec<u64>,
    pub emoji_id: Option<u64>,
    pub emoji_name: Option<String>,
    pub emoji_animated: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prompt {
    pub id: u64,
    /// 0 : choix multiple, 1 : liste déroulante.
    pub kind: u8,
    pub title: String,
    pub single_select: bool,
    pub required: bool,
    /// Posée à l'arrivée (sinon seulement dans « Salons et rôles »).
    pub in_onboarding: bool,
    pub options: Vec<PromptOption>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Onboarding {
    pub prompts: Vec<Prompt>,
    pub default_channels: Vec<u64>,
    pub enabled: bool,
    /// 0 : seuls les salons par défaut comptent ; 1 : salons et questions.
    pub mode: u8,
    /// Mes réponses (options choisies), données par Get Guild Onboarding.
    pub responses: Vec<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Loadable<T> {
    #[default]
    Loading,
    Ready(T),
    Failed(Failure),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommunityCmd {
    /// `GET /guilds/{id}/welcome-screen`.
    LoadWelcome { guild_id: u64 },
    /// `PATCH /guilds/{id}/welcome-screen` (MANAGE_GUILD).
    SaveWelcome { guild_id: u64, enabled: bool, screen: WelcomeScreen },
    /// `GET /guilds/{id}/onboarding`.
    LoadOnboarding { guild_id: u64 },
    /// `PUT /guilds/{id}/onboarding` (MANAGE_GUILD et MANAGE_ROLES).
    SaveOnboarding { guild_id: u64, onboarding: Onboarding },
    /// `POST` (premières réponses) ou `PUT /guilds/{id}/onboarding-responses`
    /// (compte utilisateur, docs.discord.food) ; `seen` : questions et
    /// options montrées, avec l'instant (ms Unix).
    Respond { guild_id: u64, options: Vec<u64>, prompts_seen: Vec<u64>, options_seen: Vec<u64>, at_ms: i64, first: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommunityAction {
    Welcome,
    SaveWelcome,
    Onboarding,
    SaveOnboarding,
    Respond,
}

impl CommunityCmd {
    pub fn action(&self) -> (CommunityAction, u64) {
        match self {
            CommunityCmd::LoadWelcome { guild_id } => (CommunityAction::Welcome, *guild_id),
            CommunityCmd::SaveWelcome { guild_id, .. } => (CommunityAction::SaveWelcome, *guild_id),
            CommunityCmd::LoadOnboarding { guild_id } => (CommunityAction::Onboarding, *guild_id),
            CommunityCmd::SaveOnboarding { guild_id, .. } => (CommunityAction::SaveOnboarding, *guild_id),
            CommunityCmd::Respond { guild_id, .. } => (CommunityAction::Respond, *guild_id),
        }
    }
}

#[derive(Debug)]
pub enum CommunityEvent {
    /// GUILD_CREATE et READY : tout est connu (salons vocaux compris).
    Info { guild_id: u64, info: GuildInfo },
    /// GUILD_UPDATE : les salons vocaux ne changent pas.
    InfoUpdate { guild_id: u64, features: Vec<String>, rules_channel: Option<u64>, updates_channel: Option<u64>, description: String },
    VenueUpsert { guild_id: u64, venue: Venue },
    VenueDelete { guild_id: u64, id: u64 },
    Welcome { guild_id: u64, screen: WelcomeScreen },
    Onboarding { guild_id: u64, onboarding: Onboarding },
    /// Mes réponses enregistrées.
    Responded { guild_id: u64, options: Vec<u64> },
    /// Écriture réussie (welcome, onboarding).
    Saved { action: CommunityAction, guild_id: u64 },
    Failed { action: CommunityAction, guild_id: u64, failure: Failure },
}

/// Fenêtre d'accueil à montrer (à l'arrivée sur un serveur, ou demandée).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrival {
    Welcome(u64),
    Onboarding(u64),
}

/// Pourquoi une page n'est pas enregistrée (dit avant tout envoi).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveError {
    TooLong,
    TooMany,
    /// Titre ou description obligatoire vide.
    Empty,
    /// Question sans option.
    NoOption,
    /// Réponse obligatoire manquante, ou plusieurs à une question à choix unique.
    Answer,
    Busy,
}

#[derive(Default)]
pub struct Community {
    info: HashMap<u64, GuildInfo>,
    pub welcome: HashMap<u64, Loadable<WelcomeScreen>>,
    pub onboarding: HashMap<u64, Loadable<Onboarding>>,
    pending: HashSet<(CommunityAction, u64)>,
    /// Serveur rejoint à l'instant : son accueil s'ouvre dès que ses fonctions sont connues.
    arriving: Option<u64>,
    /// Fenêtre d'accueil ouverte.
    pub show: Option<Arrival>,
    notices: Vec<(CommunityAction, Failure)>,
    saved: Option<(CommunityAction, u64)>,
}

impl Community {
    pub fn clear(&mut self) {
        *self = Community::default();
    }

    pub fn info(&self, guild: u64) -> Option<&GuildInfo> {
        self.info.get(&guild)
    }

    pub fn forget_guild(&mut self, guild: u64) {
        self.info.remove(&guild);
        self.welcome.remove(&guild);
        self.onboarding.remove(&guild);
        if matches!(self.show, Some(Arrival::Welcome(g) | Arrival::Onboarding(g)) if g == guild) {
            self.show = None;
        }
    }

    pub fn pending(&self, action: CommunityAction, guild: u64) -> bool {
        self.pending.contains(&(action, guild))
    }

    pub fn take_notice(&mut self) -> Option<(CommunityAction, Failure)> {
        (!self.notices.is_empty()).then(|| self.notices.remove(0))
    }

    pub fn take_saved(&mut self) -> Option<(CommunityAction, u64)> {
        self.saved.take()
    }

    /// Salons vocaux et de conférence, dans l'ordre du serveur.
    pub fn venues(&self, guild: u64) -> &[Venue] {
        self.info.get(&guild).map_or(&[], |i| i.venues.as_slice())
    }
}

/// Identifiant neuf pour une question ou une option créée ici (forme d'un
/// identifiant de Discord, tiré de l'heure ; `salt` sépare ceux d'une même ms).
pub fn new_id(now_ms: i64, salt: u64) -> u64 {
    (u64::try_from(now_ms - DISCORD_EPOCH).unwrap_or(0) << 22) | (salt & 0x3f_ffff)
}

fn too_long(s: &str, max: usize) -> bool {
    s.chars().count() > max
}

/// Vérifie un écran de bienvenue avant l'envoi (descriptions coupées des espaces).
pub fn check_welcome(screen: &WelcomeScreen) -> Result<WelcomeScreen, SaveError> {
    if too_long(screen.description.trim(), WELCOME_DESCRIPTION_MAX) {
        return Err(SaveError::TooLong);
    }
    if screen.channels.len() > WELCOME_CHANNELS_MAX {
        return Err(SaveError::TooMany);
    }
    let mut out = WelcomeScreen { description: screen.description.trim().to_string(), channels: Vec::new() };
    for c in &screen.channels {
        let d = c.description.trim();
        if d.is_empty() || c.channel_id == 0 {
            return Err(SaveError::Empty);
        }
        if too_long(d, WELCOME_CHANNEL_DESCRIPTION_MAX) {
            return Err(SaveError::TooLong);
        }
        let emoji_name = c.emoji_name.as_deref().map(str::trim).filter(|e| !e.is_empty()).map(str::to_string);
        out.channels.push(WelcomeChannel { channel_id: c.channel_id, description: d.to_string(), emoji_id: c.emoji_id, emoji_name });
    }
    Ok(out)
}

/// Vérifie une intégration avant l'envoi.
pub fn check_onboarding(o: &Onboarding) -> Result<Onboarding, SaveError> {
    if o.prompts.len() > PROMPTS_MAX {
        return Err(SaveError::TooMany);
    }
    let mut out = o.clone();
    for p in &mut out.prompts {
        p.title = p.title.trim().to_string();
        if p.title.is_empty() {
            return Err(SaveError::Empty);
        }
        if too_long(&p.title, TITLE_MAX) {
            return Err(SaveError::TooLong);
        }
        if p.options.is_empty() {
            return Err(SaveError::NoOption);
        }
        if p.options.len() > OPTIONS_MAX {
            return Err(SaveError::TooMany);
        }
        for opt in &mut p.options {
            opt.title = opt.title.trim().to_string();
            opt.description = opt.description.trim().to_string();
            if opt.title.is_empty() {
                return Err(SaveError::Empty);
            }
            if too_long(&opt.title, TITLE_MAX) || too_long(&opt.description, TITLE_MAX) {
                return Err(SaveError::TooLong);
            }
        }
    }
    Ok(out)
}

/// Vérifie des réponses : une question à choix unique en a une au plus, une
/// question obligatoire posée à l'arrivée en a au moins une.
pub fn check_answers(o: &Onboarding, chosen: &[u64]) -> Result<(), SaveError> {
    if chosen.len() > RESPONSES_MAX {
        return Err(SaveError::TooMany);
    }
    for p in &o.prompts {
        let n = p.options.iter().filter(|opt| chosen.contains(&opt.id)).count();
        if (p.single_select && n > 1) || (p.required && p.in_onboarding && n == 0) {
            return Err(SaveError::Answer);
        }
    }
    Ok(())
}

impl Store {
    fn community_cmd(&mut self, cmd: CommunityCmd) -> bool {
        if !self.community.pending.insert(cmd.action()) {
            return false;
        }
        self.push_command(Cmd::Community(cmd));
        true
    }

    pub fn guild_info(&self, guild: u64) -> Option<&GuildInfo> {
        self.community.info(guild)
    }

    /// Modifier l'intégration : MANAGE_GUILD et MANAGE_ROLES (inconnu : proposé).
    pub fn can_edit_onboarding(&self, guild: u64) -> bool {
        let (Some(me), Some(g)) = (self.me.as_ref(), self.guilds.iter().find(|g| g.id == guild)) else { return false };
        base_permissions(g, me.id).is_none_or(|p| p.contains(Permissions::MANAGE_GUILD) && p.contains(Permissions::MANAGE_ROLES))
    }

    pub fn load_welcome(&mut self, guild_id: u64) -> bool {
        if !self.community_cmd(CommunityCmd::LoadWelcome { guild_id }) {
            return false;
        }
        self.community.welcome.insert(guild_id, Loadable::Loading);
        true
    }

    pub fn save_welcome(&mut self, guild_id: u64, enabled: bool, screen: &WelcomeScreen) -> Result<(), SaveError> {
        let screen = check_welcome(screen)?;
        if self.community_cmd(CommunityCmd::SaveWelcome { guild_id, enabled, screen }) { Ok(()) } else { Err(SaveError::Busy) }
    }

    pub fn load_onboarding(&mut self, guild_id: u64) -> bool {
        if !self.community_cmd(CommunityCmd::LoadOnboarding { guild_id }) {
            return false;
        }
        // Une intégration déjà lue reste affichée pendant la relecture.
        self.community.onboarding.entry(guild_id).or_insert(Loadable::Loading);
        true
    }

    pub fn save_onboarding(&mut self, guild_id: u64, onboarding: &Onboarding) -> Result<(), SaveError> {
        let onboarding = check_onboarding(onboarding)?;
        if self.community_cmd(CommunityCmd::SaveOnboarding { guild_id, onboarding }) { Ok(()) } else { Err(SaveError::Busy) }
    }

    /// Enregistre mes réponses (options choisies) aux questions de l'intégration.
    pub fn respond_onboarding(&mut self, guild_id: u64, chosen: &[u64]) -> Result<(), SaveError> {
        let Some(Loadable::Ready(o)) = self.community.onboarding.get(&guild_id) else { return Err(SaveError::Busy) };
        check_answers(o, chosen)?;
        let first = o.responses.is_empty();
        let prompts_seen = o.prompts.iter().map(|p| p.id).collect();
        let options_seen = o.prompts.iter().flat_map(|p| p.options.iter().map(|x| x.id)).collect();
        let cmd = CommunityCmd::Respond { guild_id, options: chosen.to_vec(), prompts_seen, options_seen, at_ms: self.now(), first };
        if self.community_cmd(cmd) { Ok(()) } else { Err(SaveError::Busy) }
    }

    /// Ouvre l'accueil d'un serveur (menu, ou arrivée) et le demande à Discord.
    pub fn open_arrival(&mut self, a: Arrival) {
        self.community.show = Some(a);
        match a {
            Arrival::Welcome(g) => {
                self.load_welcome(g);
            }
            Arrival::Onboarding(g) => {
                self.load_onboarding(g);
            }
        }
    }

    /// Serveur rejoint (lot A1) : son accueil s'ouvre dès que ses fonctions sont connues.
    pub(crate) fn community_joined(&mut self, guild: u64) {
        self.community.arriving = Some(guild);
        self.arrival_check(guild);
    }

    fn arrival_check(&mut self, guild: u64) {
        if self.community.arriving != Some(guild) {
            return;
        }
        let Some(info) = self.community.info(guild) else { return };
        let a = if info.has(GUILD_ONBOARDING) {
            Some(Arrival::Onboarding(guild))
        } else if info.has(WELCOME_SCREEN_ENABLED) {
            Some(Arrival::Welcome(guild))
        } else {
            None
        };
        self.community.arriving = None;
        if let Some(a) = a {
            self.open_arrival(a);
        }
    }

    pub(crate) fn apply_community(&mut self, e: CommunityEvent) {
        let c = &mut self.community;
        match e {
            CommunityEvent::Info { guild_id, mut info } => {
                info.venues.sort_by_key(|v| (v.position, v.id));
                c.info.insert(guild_id, info);
                self.arrival_check(guild_id);
            }
            CommunityEvent::InfoUpdate { guild_id, features, rules_channel, updates_channel, description } => {
                let i = c.info.entry(guild_id).or_default();
                (i.features, i.rules_channel, i.updates_channel, i.description) = (features, rules_channel, updates_channel, description);
            }
            CommunityEvent::VenueUpsert { guild_id, venue } => {
                let i = c.info.entry(guild_id).or_default();
                i.venues.retain(|v| v.id != venue.id);
                i.venues.push(venue);
                i.venues.sort_by_key(|v| (v.position, v.id));
            }
            CommunityEvent::VenueDelete { guild_id, id } => {
                if let Some(i) = c.info.get_mut(&guild_id) {
                    i.venues.retain(|v| v.id != id);
                }
            }
            CommunityEvent::Welcome { guild_id, screen } => {
                c.pending.remove(&(CommunityAction::Welcome, guild_id));
                c.welcome.insert(guild_id, Loadable::Ready(screen));
            }
            CommunityEvent::Onboarding { guild_id, onboarding } => {
                c.pending.remove(&(CommunityAction::Onboarding, guild_id));
                c.onboarding.insert(guild_id, Loadable::Ready(onboarding));
            }
            CommunityEvent::Responded { guild_id, options } => {
                c.pending.remove(&(CommunityAction::Respond, guild_id));
                if let Some(Loadable::Ready(o)) = c.onboarding.get_mut(&guild_id) {
                    o.responses = options;
                }
                c.saved = Some((CommunityAction::Respond, guild_id));
            }
            CommunityEvent::Saved { action, guild_id } => {
                c.pending.remove(&(action, guild_id));
                c.saved = Some((action, guild_id));
                // Ce que Discord a gardé est relu (pas d'état supposé).
                match action {
                    CommunityAction::SaveWelcome => {
                        self.load_welcome(guild_id);
                    }
                    CommunityAction::SaveOnboarding => {
                        self.load_onboarding(guild_id);
                    }
                    _ => {}
                }
            }
            CommunityEvent::Failed { action, guild_id, failure } => {
                c.pending.remove(&(action, guild_id));
                match action {
                    CommunityAction::Welcome => {
                        c.welcome.insert(guild_id, Loadable::Failed(failure.clone()));
                    }
                    CommunityAction::Onboarding => {
                        c.onboarding.insert(guild_id, Loadable::Failed(failure.clone()));
                    }
                    _ => {}
                }
                c.notices.push((action, failure));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::event::Event;
    use crate::state::model::{Guild, User};
    use crate::state::notice::Problem;

    fn opt(id: u64) -> PromptOption {
        PromptOption { id, title: format!("o{id}"), ..Default::default() }
    }

    fn onboarding() -> Onboarding {
        Onboarding {
            prompts: vec![
                Prompt { id: 1, title: "Jeux ?".into(), single_select: true, required: true, in_onboarding: true, options: vec![opt(11), opt(12)], ..Default::default() },
                Prompt { id: 2, title: "Langues".into(), options: vec![opt(21), opt(22)], ..Default::default() },
            ],
            enabled: true,
            ..Default::default()
        }
    }

    #[test]
    fn welcome_screen_limits() {
        let ch = |d: &str| WelcomeChannel { channel_id: 5, description: d.into(), emoji_id: None, emoji_name: Some(" 👋 ".into()) };
        let ok = check_welcome(&WelcomeScreen { description: " Salut ".into(), channels: vec![ch(" Règles ")] });
        assert_eq!(ok.map(|s| (s.description, s.channels[0].description.clone(), s.channels[0].emoji_name.clone())), Ok(("Salut".into(), "Règles".into(), Some("👋".into()))));
        assert_eq!(check_welcome(&WelcomeScreen { description: "x".repeat(141), channels: Vec::new() }), Err(SaveError::TooLong));
        assert_eq!(check_welcome(&WelcomeScreen { description: String::new(), channels: vec![ch("a"); 6] }), Err(SaveError::TooMany));
        assert_eq!(check_welcome(&WelcomeScreen { description: String::new(), channels: vec![ch(" ")] }), Err(SaveError::Empty));
        assert_eq!(check_welcome(&WelcomeScreen { description: String::new(), channels: vec![ch(&"x".repeat(51))] }), Err(SaveError::TooLong));
    }

    #[test]
    fn onboarding_checks_and_answers() {
        assert!(check_onboarding(&onboarding()).is_ok());
        let mut bad = onboarding();
        bad.prompts[1].options.clear();
        assert_eq!(check_onboarding(&bad), Err(SaveError::NoOption));
        bad.prompts[1].options.push(PromptOption { title: "  ".into(), ..opt(3) });
        assert_eq!(check_onboarding(&bad), Err(SaveError::Empty));
        let o = onboarding();
        assert_eq!(check_answers(&o, &[11, 21, 22]), Ok(()));
        assert_eq!(check_answers(&o, &[11, 12]), Err(SaveError::Answer), "choix unique");
        assert_eq!(check_answers(&o, &[21]), Err(SaveError::Answer), "obligatoire");
        assert!(new_id(1_763_000_000_000, 3) > new_id(1_763_000_000_000, 2));
        assert_eq!(new_id(DISCORD_EPOCH, 0), 0);
    }

    #[test]
    fn arrival_opens_onboarding_then_answers_are_sent() {
        let mut s = Store::new();
        s.apply(Event::Ready { me: User::new(1, "moi".into()), guilds: vec![Guild::new(7, "Club".into(), Vec::new())], dms: Vec::new() });
        s.community_joined(7);
        assert_eq!(s.community.show, None, "fonctions pas encore connues");
        s.apply(Event::Community(CommunityEvent::Info { guild_id: 7, info: GuildInfo { features: vec![GUILD_ONBOARDING.into()], ..Default::default() } }));
        assert_eq!(s.community.show, Some(Arrival::Onboarding(7)));
        assert_eq!(s.take_commands(), vec![Cmd::Community(CommunityCmd::LoadOnboarding { guild_id: 7 })]);
        assert_eq!(s.respond_onboarding(7, &[11]), Err(SaveError::Busy), "pas encore lue");
        s.apply(Event::Community(CommunityEvent::Onboarding { guild_id: 7, onboarding: onboarding() }));
        assert_eq!(s.respond_onboarding(7, &[21]), Err(SaveError::Answer));
        assert_eq!(s.respond_onboarding(7, &[11, 21]), Ok(()));
        let cmds = s.take_commands();
        assert!(matches!(cmds.as_slice(), [Cmd::Community(CommunityCmd::Respond { guild_id: 7, first: true, options, prompts_seen, .. })] if options == &vec![11, 21] && prompts_seen == &vec![1, 2]), "{cmds:?}");
        s.apply(Event::Community(CommunityEvent::Responded { guild_id: 7, options: vec![11, 21] }));
        assert_eq!(s.community.take_saved(), Some((CommunityAction::Respond, 7)));
        let Some(Loadable::Ready(o)) = s.community.onboarding.get(&7) else { panic!("lue") };
        assert_eq!(o.responses, vec![11, 21]);
        // Réponses suivantes : `PUT`.
        assert_eq!(s.respond_onboarding(7, &[12]), Ok(()));
        assert!(matches!(s.take_commands().as_slice(), [Cmd::Community(CommunityCmd::Respond { first: false, .. })]));
    }

    #[test]
    fn welcome_saved_is_read_again_and_failure_is_shown() {
        let mut s = Store::new();
        assert_eq!(s.save_welcome(7, true, &WelcomeScreen::default()), Ok(()));
        assert_eq!(s.save_welcome(7, true, &WelcomeScreen::default()), Err(SaveError::Busy));
        s.take_commands();
        s.apply(Event::Community(CommunityEvent::Saved { action: CommunityAction::SaveWelcome, guild_id: 7 }));
        assert_eq!(s.take_commands(), vec![Cmd::Community(CommunityCmd::LoadWelcome { guild_id: 7 })]);
        s.apply(Event::Community(CommunityEvent::Failed { action: CommunityAction::Welcome, guild_id: 7, failure: Failure::Problem(Problem::Forbidden) }));
        assert_eq!(s.community.welcome.get(&7), Some(&Loadable::Failed(Failure::Problem(Problem::Forbidden))));
        assert_eq!(s.community.take_notice(), Some((CommunityAction::Welcome, Failure::Problem(Problem::Forbidden))));
        // Salons vocaux triés par position ; serveur oublié avec lui.
        let v = |id, position| Venue { id, name: format!("v{id}"), stage: false, position };
        s.apply(Event::Community(CommunityEvent::Info { guild_id: 7, info: GuildInfo { venues: vec![v(2, 5), v(3, 1)], ..Default::default() } }));
        s.apply(Event::Community(CommunityEvent::VenueUpsert { guild_id: 7, venue: v(4, 0) }));
        s.apply(Event::Community(CommunityEvent::VenueDelete { guild_id: 7, id: 2 }));
        assert_eq!(s.community.venues(7).iter().map(|v| v.id).collect::<Vec<_>>(), vec![4, 3]);
        s.apply(Event::GuildDelete(7));
        assert!(s.guild_info(7).is_none());
    }
}
