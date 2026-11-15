//! Pages des paramètres du serveur ajoutées par le lot V (groupe 2) : widget
//! (`/guilds/{id}/widget`), modèle de serveur (`/guilds/{id}/templates`) et
//! élagage des membres inactifs (`/guilds/{id}/prune`). Routes documentées
//! par Discord ; elles passent par la file d'administration du lot A8.
//!
//! Rien ne part sans geste : une page demande sa donnée à l'ouverture,
//! chaque écriture suit un clic (l'élagage, une confirmation).

use super::guild::{AdminAction, AdminCmd, AdminNotice, Failure, GuildAdmin, Load};
use crate::state::perms::Permissions;
use crate::state::store::Store;

/// Longueurs acceptées (documentation, Guild Template).
pub const TEMPLATE_NAME_MAX: usize = 100;
pub const TEMPLATE_DESCRIPTION_MAX: usize = 120;
/// Jours d'inactivité proposés pour l'élagage (la documentation accepte 1 à 30).
pub const PRUNE_DAYS: [u8; 2] = [7, 30];

/// Réglages du widget (`Guild Widget Settings`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Widget {
    pub enabled: bool,
    /// Salon de l'invitation du widget (`None` : pas d'invitation).
    pub channel_id: Option<u64>,
}

/// Modèle du serveur (`Guild Template`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Template {
    pub code: String,
    pub name: String,
    pub description: String,
    pub usage_count: u32,
    /// Le serveur a changé depuis la dernière synchronisation.
    pub dirty: bool,
}

impl Template {
    /// Lien à partager.
    pub fn url(&self) -> String {
        format!("https://discord.new/{}", self.code)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtraCmd {
    /// `GET /guilds/{id}/widget`.
    Widget { guild_id: u64 },
    /// `PATCH /guilds/{id}/widget`.
    SetWidget { guild_id: u64, widget: Widget },
    /// `GET /guilds/{id}/templates`.
    Templates { guild_id: u64 },
    /// `POST /guilds/{id}/templates`.
    CreateTemplate { guild_id: u64, name: String, description: String },
    /// `PUT /guilds/{id}/templates/{code}`.
    SyncTemplate { guild_id: u64, code: String },
    /// `DELETE /guilds/{id}/templates/{code}`.
    DeleteTemplate { guild_id: u64, code: String },
    /// `GET /guilds/{id}/prune?days=N`.
    PruneCount { guild_id: u64, days: u8 },
    /// `POST /guilds/{id}/prune`.
    Prune { guild_id: u64, days: u8, reason: Option<String> },
}

impl ExtraCmd {
    pub fn guild_id(&self) -> u64 {
        match self {
            ExtraCmd::Widget { guild_id }
            | ExtraCmd::SetWidget { guild_id, .. }
            | ExtraCmd::Templates { guild_id }
            | ExtraCmd::CreateTemplate { guild_id, .. }
            | ExtraCmd::SyncTemplate { guild_id, .. }
            | ExtraCmd::DeleteTemplate { guild_id, .. }
            | ExtraCmd::PruneCount { guild_id, .. }
            | ExtraCmd::Prune { guild_id, .. } => *guild_id,
        }
    }

    pub fn action(&self) -> ExtraAction {
        match self {
            ExtraCmd::Widget { .. } => ExtraAction::Widget,
            ExtraCmd::SetWidget { .. } => ExtraAction::SaveWidget,
            ExtraCmd::Templates { .. } => ExtraAction::Templates,
            ExtraCmd::CreateTemplate { .. } | ExtraCmd::SyncTemplate { .. } => ExtraAction::SaveTemplate,
            ExtraCmd::DeleteTemplate { .. } => ExtraAction::DeleteTemplate,
            ExtraCmd::PruneCount { .. } => ExtraAction::PruneCount,
            ExtraCmd::Prune { .. } => ExtraAction::Prune,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExtraAction {
    Widget,
    SaveWidget,
    Templates,
    SaveTemplate,
    DeleteTemplate,
    PruneCount,
    Prune,
}

impl ExtraAction {
    /// Une écriture annonce son échec ; une lecture le dit sur sa page.
    pub fn writes(self) -> bool {
        matches!(self, ExtraAction::SaveWidget | ExtraAction::SaveTemplate | ExtraAction::DeleteTemplate | ExtraAction::Prune)
    }
}

#[derive(Debug)]
pub enum ExtraEvent {
    Widget { guild_id: u64, widget: Widget, saved: bool },
    Templates { guild_id: u64, list: Vec<Template> },
    TemplateSaved { guild_id: u64, template: Template },
    TemplateDeleted { guild_id: u64, code: String },
    PruneCount { guild_id: u64, days: u8, count: u32 },
    /// `count` : membres retirés, si Discord l'a dit.
    Pruned { guild_id: u64, count: Option<u32> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtraNotice {
    WidgetSaved,
    TemplateSaved,
    TemplateDeleted,
    Pruned(Option<u32>),
}

/// Ce que la fenêtre garde de ces pages (oublié à sa fermeture, avec le reste).
#[derive(Default)]
pub struct Extras {
    pub widget: Option<Load<Widget>>,
    pub widget_busy: bool,
    pub templates: Option<Load<Vec<Template>>>,
    pub template_busy: bool,
    /// Nombre de membres qu'un élagage à tant de jours retirerait.
    pub prune: Option<(u8, Load<u32>)>,
    pub pruning: bool,
}

impl GuildAdmin {
    pub(super) fn fail_extra(&mut self, action: ExtraAction, failure: Failure) {
        let x = &mut self.extras;
        match action {
            ExtraAction::Widget => x.widget = Some(Load::Failed(failure.clone())),
            ExtraAction::SaveWidget => x.widget_busy = false,
            ExtraAction::Templates => x.templates = Some(Load::Failed(failure.clone())),
            ExtraAction::SaveTemplate | ExtraAction::DeleteTemplate => x.template_busy = false,
            ExtraAction::PruneCount => {
                if let Some((_, l)) = &mut x.prune {
                    *l = Load::Failed(failure.clone());
                }
            }
            ExtraAction::Prune => x.pruning = false,
        }
        if action.writes() {
            self.notice(AdminNotice::Failed { action: AdminAction::Extra(action), failure });
        }
    }
}

impl Store {
    fn extra_cmd(&mut self, cmd: ExtraCmd) {
        self.admin_cmd(AdminCmd::Extra(cmd));
    }

    /// La fenêtre est-elle ouverte sur ce serveur, et ai-je `p` ?
    fn extras_allowed(&self, guild_id: u64, p: Permissions) -> bool {
        self.guild_admin.open == Some(guild_id) && self.may_in_guild(guild_id, p)
    }

    /// Lit les réglages du widget (une fois par ouverture, ou après un échec).
    pub fn load_widget(&mut self, guild_id: u64) -> bool {
        if !self.extras_allowed(guild_id, Permissions::MANAGE_GUILD) || matches!(self.guild_admin.extras.widget, Some(Load::Loading | Load::Ready(_))) {
            return false;
        }
        self.guild_admin.extras.widget = Some(Load::Loading);
        self.extra_cmd(ExtraCmd::Widget { guild_id });
        true
    }

    pub fn save_widget(&mut self, guild_id: u64, widget: Widget) -> bool {
        if !self.extras_allowed(guild_id, Permissions::MANAGE_GUILD) || self.guild_admin.extras.widget_busy {
            return false;
        }
        self.guild_admin.extras.widget_busy = true;
        self.extra_cmd(ExtraCmd::SetWidget { guild_id, widget });
        true
    }

    pub fn load_templates(&mut self, guild_id: u64) -> bool {
        if !self.extras_allowed(guild_id, Permissions::MANAGE_GUILD) || matches!(self.guild_admin.extras.templates, Some(Load::Loading | Load::Ready(_))) {
            return false;
        }
        self.guild_admin.extras.templates = Some(Load::Loading);
        self.extra_cmd(ExtraCmd::Templates { guild_id });
        true
    }

    /// Crée le modèle du serveur (un seul par serveur) ; `false` si le nom ne convient pas.
    pub fn create_template(&mut self, guild_id: u64, name: &str, description: &str) -> bool {
        let (name, description) = (name.trim(), description.trim());
        let fits = (1..=TEMPLATE_NAME_MAX).contains(&name.chars().count()) && description.chars().count() <= TEMPLATE_DESCRIPTION_MAX;
        if !fits || !self.extras_allowed(guild_id, Permissions::MANAGE_GUILD) || self.guild_admin.extras.template_busy {
            return false;
        }
        self.guild_admin.extras.template_busy = true;
        self.extra_cmd(ExtraCmd::CreateTemplate { guild_id, name: name.to_string(), description: description.to_string() });
        true
    }

    fn template_write(&mut self, guild_id: u64, code: &str, delete: bool) -> bool {
        let known = self.guild_admin.extras.templates.as_ref().and_then(Load::ready).is_some_and(|l| l.iter().any(|t| t.code == code));
        if !known || !self.extras_allowed(guild_id, Permissions::MANAGE_GUILD) || self.guild_admin.extras.template_busy {
            return false;
        }
        self.guild_admin.extras.template_busy = true;
        let code = code.to_string();
        self.extra_cmd(if delete { ExtraCmd::DeleteTemplate { guild_id, code } } else { ExtraCmd::SyncTemplate { guild_id, code } });
        true
    }

    /// Met le modèle à l'image du serveur d'aujourd'hui.
    pub fn sync_template(&mut self, guild_id: u64, code: &str) -> bool {
        self.template_write(guild_id, code, false)
    }

    pub fn delete_template(&mut self, guild_id: u64, code: &str) -> bool {
        self.template_write(guild_id, code, true)
    }

    /// Puis-je élaguer ? (KICK_MEMBERS et MANAGE_GUILD, documentation.)
    pub fn can_prune(&self, guild_id: u64) -> bool {
        self.guild_has(guild_id, Permissions::KICK_MEMBERS) && self.guild_has(guild_id, Permissions::MANAGE_GUILD)
    }

    /// Droits inconnus (hors de l'interface) : la demande part, Discord répondra.
    fn may_prune(&self, guild_id: u64) -> bool {
        self.may_in_guild(guild_id, Permissions::KICK_MEMBERS) && self.may_in_guild(guild_id, Permissions::MANAGE_GUILD)
    }

    /// Demande combien de membres un élagage à `days` jours retirerait.
    pub fn load_prune_count(&mut self, guild_id: u64, days: u8) -> bool {
        if !(1..=30).contains(&days) || self.guild_admin.open != Some(guild_id) || !self.may_prune(guild_id) {
            return false;
        }
        if matches!(&self.guild_admin.extras.prune, Some((d, Load::Loading | Load::Ready(_))) if *d == days) {
            return false;
        }
        self.guild_admin.extras.prune = Some((days, Load::Loading));
        self.extra_cmd(ExtraCmd::PruneCount { guild_id, days });
        true
    }

    /// Élague (après confirmation dans l'interface).
    pub fn prune_members(&mut self, guild_id: u64, days: u8, reason: &str) -> bool {
        if !(1..=30).contains(&days) || self.guild_admin.open != Some(guild_id) || !self.may_prune(guild_id) || self.guild_admin.extras.pruning {
            return false;
        }
        self.guild_admin.extras.pruning = true;
        let reason = Some(reason.trim().to_string()).filter(|r| !r.is_empty());
        self.extra_cmd(ExtraCmd::Prune { guild_id, days, reason });
        true
    }

    pub(super) fn apply_extras(&mut self, e: ExtraEvent) {
        let Some(open) = self.guild_admin.open else { return };
        let a = &mut self.guild_admin;
        match e {
            ExtraEvent::Widget { guild_id, widget, saved } if guild_id == open => {
                a.extras.widget = Some(Load::Ready(widget));
                if saved {
                    a.extras.widget_busy = false;
                    a.notice(AdminNotice::Extra(ExtraNotice::WidgetSaved));
                }
            }
            ExtraEvent::Templates { guild_id, list } if guild_id == open => a.extras.templates = Some(Load::Ready(list)),
            ExtraEvent::TemplateSaved { guild_id, template } if guild_id == open => {
                a.extras.template_busy = false;
                match &mut a.extras.templates {
                    Some(Load::Ready(list)) => match list.iter_mut().find(|t| t.code == template.code) {
                        Some(t) => *t = template,
                        None => list.push(template),
                    },
                    other => *other = Some(Load::Ready(vec![template])),
                }
                a.notice(AdminNotice::Extra(ExtraNotice::TemplateSaved));
            }
            ExtraEvent::TemplateDeleted { guild_id, code } if guild_id == open => {
                a.extras.template_busy = false;
                if let Some(Load::Ready(list)) = &mut a.extras.templates {
                    list.retain(|t| t.code != code);
                }
                a.notice(AdminNotice::Extra(ExtraNotice::TemplateDeleted));
            }
            ExtraEvent::PruneCount { guild_id, days, count } if guild_id == open => {
                // Une réponse pour d'autres jours que ceux affichés est écartée.
                if let Some((d, l)) = &mut a.extras.prune
                    && *d == days
                {
                    *l = Load::Ready(count);
                }
            }
            ExtraEvent::Pruned { guild_id, count } if guild_id == open => {
                a.extras.pruning = false;
                a.extras.prune = None;
                a.notice(AdminNotice::Extra(ExtraNotice::Pruned(count)));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::admin::guild::AdminEvent;
    use crate::state::model::{Guild, User};
    use crate::state::{Cmd, Event};

    fn store() -> Store {
        let mut s = Store::new();
        s.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: vec![Guild::new(9, "G".into(), Vec::new())], dms: Vec::new() });
        s.open_guild_admin(9);
        s.take_commands();
        s
    }

    fn last(s: &mut Store) -> Option<ExtraCmd> {
        match s.take_commands().pop() {
            Some(Cmd::GuildAdmin(AdminCmd::Extra(c))) => Some(c),
            _ => None,
        }
    }

    fn ev(s: &mut Store, e: ExtraEvent) {
        s.apply(Event::GuildAdmin(AdminEvent::Extra(e)));
    }

    #[test]
    fn widget_is_read_once_and_saved() {
        let mut s = store();
        assert!(s.load_widget(9));
        assert!(!s.load_widget(9), "déjà demandé");
        assert!(!s.load_widget(8), "autre serveur que la fenêtre");
        ev(&mut s, ExtraEvent::Widget { guild_id: 9, widget: Widget::default(), saved: false });
        let w = Widget { enabled: true, channel_id: Some(5) };
        assert!(s.save_widget(9, w));
        assert!(!s.save_widget(9, w), "déjà en cours");
        assert_eq!(last(&mut s), Some(ExtraCmd::SetWidget { guild_id: 9, widget: w }));
        ev(&mut s, ExtraEvent::Widget { guild_id: 9, widget: w, saved: true });
        assert_eq!(s.guild_admin.extras.widget, Some(Load::Ready(w)));
        assert_eq!(s.guild_admin.take_notice(), Some(AdminNotice::Extra(ExtraNotice::WidgetSaved)));
        assert!(s.save_widget(9, Widget::default()));
        s.apply(Event::GuildAdmin(AdminEvent::Failed { guild_id: 9, action: AdminAction::Extra(ExtraAction::SaveWidget), failure: Failure::Forbidden, target: None }));
        assert!(!s.guild_admin.extras.widget_busy);
        assert!(matches!(s.guild_admin.take_notice(), Some(AdminNotice::Failed { .. })));
    }

    #[test]
    fn template_is_created_synced_and_deleted() {
        let mut s = store();
        assert!(!s.create_template(9, "  ", ""), "nom vide");
        assert!(!s.create_template(9, "Club", &"x".repeat(121)), "description trop longue");
        assert!(s.create_template(9, " Club ", ""));
        assert_eq!(last(&mut s), Some(ExtraCmd::CreateTemplate { guild_id: 9, name: "Club".into(), description: String::new() }));
        let t = Template { code: "abc".into(), name: "Club".into(), dirty: true, ..Default::default() };
        ev(&mut s, ExtraEvent::TemplateSaved { guild_id: 9, template: t.clone() });
        assert_eq!(t.url(), "https://discord.new/abc");
        assert!(!s.sync_template(9, "inconnu"));
        assert!(s.sync_template(9, "abc"));
        ev(&mut s, ExtraEvent::TemplateSaved { guild_id: 9, template: Template { dirty: false, ..t } });
        assert_eq!(s.guild_admin.extras.templates.as_ref().and_then(Load::ready).map(|l| (l.len(), l[0].dirty)), Some((1, false)));
        assert!(s.delete_template(9, "abc"));
        ev(&mut s, ExtraEvent::TemplateDeleted { guild_id: 9, code: "abc".into() });
        assert_eq!(s.guild_admin.extras.templates.as_ref().and_then(Load::ready).map(Vec::len), Some(0));
    }

    #[test]
    fn prune_counts_then_prunes() {
        let mut s = store();
        assert!(!s.load_prune_count(9, 0) && !s.load_prune_count(9, 31));
        assert!(s.load_prune_count(9, 7));
        assert!(!s.load_prune_count(9, 7), "déjà demandé");
        assert!(s.load_prune_count(9, 30));
        ev(&mut s, ExtraEvent::PruneCount { guild_id: 9, days: 7, count: 99 });
        assert_eq!(s.guild_admin.extras.prune, Some((30, Load::Loading)), "réponse d'une autre durée écartée");
        ev(&mut s, ExtraEvent::PruneCount { guild_id: 9, days: 30, count: 4 });
        assert_eq!(s.guild_admin.extras.prune, Some((30, Load::Ready(4))));
        assert!(s.prune_members(9, 30, " ménage "));
        assert!(!s.prune_members(9, 30, ""), "déjà en cours");
        assert_eq!(last(&mut s), Some(ExtraCmd::Prune { guild_id: 9, days: 30, reason: Some("ménage".into()) }));
        ev(&mut s, ExtraEvent::Pruned { guild_id: 9, count: Some(4) });
        assert_eq!(s.guild_admin.take_notice(), Some(AdminNotice::Extra(ExtraNotice::Pruned(Some(4)))));
        assert!(s.guild_admin.extras.prune.is_none() && !s.guild_admin.extras.pruning);
    }
}
