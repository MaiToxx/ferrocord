//! Événements programmés d'un serveur (lot A7) : liste tenue à jour par la
//! passerelle (GUILD_CREATE, `GUILD_SCHEDULED_EVENT_*`), nombre d'intéressés
//! et mes inscriptions lus à l'ouverture de la liste, création, modification,
//! changement de statut, suppression, « Ça m'intéresse ». Chaque écriture
//! suit un geste ; la liste ne change qu'à la réception de l'événement de la
//! passerelle ou de la réponse (rien d'inventé). Réseau : `net::admin::events`.

use crate::state::event::Cmd;
use crate::state::model::ImageHash;
use crate::state::notice::Problem;
use crate::state::perms::{base_permissions, Permissions};
use crate::state::store::Store;
use chrono::{Local, NaiveDate, NaiveTime, TimeZone};
use std::collections::{HashMap, HashSet, VecDeque};

/// Limites de la documentation (Guild Scheduled Event).
pub const NAME_MAX: usize = 100;
pub const DESCRIPTION_MAX: usize = 1000;
pub const LOCATION_MAX: usize = 100;
/// Au plus 100 événements programmés ou en cours par serveur : la liste est bornée d'autant.
pub const MAX_PER_GUILD: usize = 100;

/// Lieu d'un événement (`entity_type`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    /// Salon de conférence (1).
    Stage(u64),
    /// Salon vocal (2).
    Voice(u64),
    /// Ailleurs (3) : `entity_metadata.location`.
    External(String),
    /// Type inconnu de Ferrocord (affiché sans lieu, non modifiable ici).
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    Scheduled,
    Active,
    Completed,
    Canceled,
}

impl Status {
    pub fn from_api(n: u64) -> Option<Status> {
        Some(match n {
            1 => Status::Scheduled,
            2 => Status::Active,
            3 => Status::Completed,
            4 => Status::Canceled,
            _ => return None,
        })
    }

    pub fn api(self) -> u8 {
        match self {
            Status::Scheduled => 1,
            Status::Active => 2,
            Status::Completed => 3,
            Status::Canceled => 4,
        }
    }

    /// Passages permis à la main (documentation) : programmé -> en cours ou
    /// annulé, en cours -> terminé.
    pub fn can_become(self, to: Status) -> bool {
        matches!((self, to), (Status::Scheduled, Status::Active) | (Status::Scheduled, Status::Canceled) | (Status::Active, Status::Completed))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frequency {
    Yearly,
    Monthly,
    Weekly,
    Daily,
}

impl Frequency {
    pub fn from_api(n: u64) -> Option<Frequency> {
        Some(match n {
            0 => Frequency::Yearly,
            1 => Frequency::Monthly,
            2 => Frequency::Weekly,
            3 => Frequency::Daily,
            _ => return None,
        })
    }

    pub fn api(self) -> u8 {
        match self {
            Frequency::Yearly => 0,
            Frequency::Monthly => 1,
            Frequency::Weekly => 2,
            Frequency::Daily => 3,
        }
    }
}

/// Règle de récurrence (sous-ensemble de la documentation que Ferrocord sait
/// écrire et dire). Jours : 0 = lundi … 6 = dimanche ; mois : 1 à 12.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recurrence {
    pub frequency: Frequency,
    pub interval: u32,
    pub by_weekday: Vec<u8>,
    /// `by_n_weekday` : (n-ième, jour).
    pub by_n_weekday: Option<(u8, u8)>,
    /// `by_month` et `by_month_day` : (mois, jour du mois).
    pub by_month_day: Option<(u8, u8)>,
}

/// Répétitions proposées par la fenêtre de création, construites à partir de
/// la date de début (règles permises par la documentation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    None,
    Daily,
    /// Du lundi au vendredi (ensemble connu de la documentation).
    Weekdays,
    Weekly,
    Biweekly,
    /// Le n-ième même jour de la semaine chaque mois.
    Monthly,
    Yearly,
}

impl Repeat {
    pub const ALL: [Repeat; 7] = [Repeat::None, Repeat::Daily, Repeat::Weekdays, Repeat::Weekly, Repeat::Biweekly, Repeat::Monthly, Repeat::Yearly];

    /// Règle pour un premier événement commençant à `start_ms` (heure locale).
    pub fn rule(self, start_ms: i64) -> Option<Recurrence> {
        use chrono::Datelike;
        let d = Local.timestamp_millis_opt(start_ms).single()?.date_naive();
        let wd = d.weekday().num_days_from_monday() as u8;
        let base = |frequency| Recurrence { frequency, interval: 1, by_weekday: Vec::new(), by_n_weekday: None, by_month_day: None };
        Some(match self {
            Repeat::None => return None,
            Repeat::Daily => base(Frequency::Daily),
            Repeat::Weekdays => Recurrence { by_weekday: vec![0, 1, 2, 3, 4], ..base(Frequency::Daily) },
            Repeat::Weekly => Recurrence { by_weekday: vec![wd], ..base(Frequency::Weekly) },
            Repeat::Biweekly => Recurrence { interval: 2, by_weekday: vec![wd], ..base(Frequency::Weekly) },
            Repeat::Monthly => Recurrence { by_n_weekday: Some((d.day0() as u8 / 7 + 1, wd)), ..base(Frequency::Monthly) },
            Repeat::Yearly => Recurrence { by_month_day: Some((d.month() as u8, d.day() as u8)), ..base(Frequency::Yearly) },
        })
    }

    /// Le choix qui produit cette règle (fenêtre de modification).
    pub fn of(rule: Option<&Recurrence>) -> Repeat {
        let Some(r) = rule else { return Repeat::None };
        match (r.frequency, r.interval) {
            (Frequency::Daily, _) if r.by_weekday == [0, 1, 2, 3, 4] => Repeat::Weekdays,
            (Frequency::Daily, _) => Repeat::Daily,
            (Frequency::Weekly, 2) => Repeat::Biweekly,
            (Frequency::Weekly, _) => Repeat::Weekly,
            (Frequency::Monthly, _) => Repeat::Monthly,
            (Frequency::Yearly, _) => Repeat::Yearly,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduledEvent {
    pub id: u64,
    pub guild_id: u64,
    pub name: String,
    pub description: String,
    pub start_ms: i64,
    pub end_ms: Option<i64>,
    pub status: Status,
    pub place: Place,
    pub creator_id: Option<u64>,
    /// Nombre d'intéressés : seulement dans les réponses de l'API demandées
    /// avec `with_user_count` (pas dans la passerelle).
    pub user_count: Option<u32>,
    /// Image de couverture (`guild-events/{id}/{hash}`).
    pub image: Option<ImageHash>,
    pub recurrence: Option<Recurrence>,
    /// Occurrences sautées d'un événement récurrent (exceptions `is_canceled`) :
    /// leur début d'origine (ms Unix).
    pub skipped: Vec<i64>,
}

impl ScheduledEvent {
    /// Lien de partage (forme des liens de l'application officielle).
    pub fn share_url(&self) -> String {
        format!("https://discord.com/events/{}/{}", self.guild_id, self.id)
    }

    /// La prochaine occurrence (celle de `start_ms`) est sautée.
    pub fn next_skipped(&self) -> bool {
        self.skipped.contains(&self.start_ms)
    }

    pub fn upcoming(&self) -> bool {
        matches!(self.status, Status::Scheduled | Status::Active)
    }
}

/// Ce que l'utilisateur a saisi pour créer ou modifier un événement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventDraft {
    pub name: String,
    pub description: String,
    pub start_ms: i64,
    pub end_ms: Option<i64>,
    pub place: Place,
    pub recurrence: Option<Recurrence>,
    /// Nouvelle image de couverture (`data:image/png;base64,…`) ; `None` : inchangée.
    pub image: Option<String>,
}

/// Pourquoi un brouillon n'est pas envoyé (dit avant tout envoi).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DraftError {
    Name,
    Description,
    Location,
    /// Lieu externe : la fin est obligatoire.
    EndRequired,
    /// Fin avant le début.
    EndBeforeStart,
    /// Début déjà passé.
    StartPast,
    /// Salon vocal ou de conférence non choisi.
    Channel,
    Busy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventCmd {
    /// `GET /guilds/{id}/scheduled-events?with_user_count=true`, puis (compte
    /// utilisateur) `GET /users/@me/scheduled-events?guild_ids={id}`.
    Load { guild_id: u64 },
    /// `POST /guilds/{id}/scheduled-events`.
    Create { guild_id: u64, draft: EventDraft },
    /// `PATCH /guilds/{id}/scheduled-events/{event}`.
    Edit { guild_id: u64, event_id: u64, draft: EventDraft },
    /// `PATCH …` avec `status` seul.
    SetStatus { guild_id: u64, event_id: u64, status: Status },
    /// `DELETE /guilds/{id}/scheduled-events/{event}`.
    Delete { guild_id: u64, event_id: u64 },
    /// Sauter (`skip`) ou rétablir l'occurrence qui commence à `original_ms`
    /// d'un événement récurrent : `POST …/exceptions` (`is_canceled`), ou
    /// `DELETE …/exceptions/{id}` (docs.discord.food, « Guild Scheduled Event »).
    Occurrence { guild_id: u64, event_id: u64, original_ms: i64, skip: bool },
    /// `PUT` ou `DELETE /guilds/{id}/scheduled-events/{event}/users/@me`.
    Interest { guild_id: u64, event_id: u64, on: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EventAction {
    Load,
    Create,
    Edit,
    Status,
    Delete,
    Interest,
    Occurrence,
}

impl EventCmd {
    pub fn action(&self) -> (EventAction, u64) {
        match self {
            EventCmd::Load { guild_id } => (EventAction::Load, *guild_id),
            EventCmd::Create { guild_id, .. } => (EventAction::Create, *guild_id),
            EventCmd::Edit { event_id, .. } => (EventAction::Edit, *event_id),
            EventCmd::SetStatus { event_id, .. } => (EventAction::Status, *event_id),
            EventCmd::Delete { event_id, .. } => (EventAction::Delete, *event_id),
            EventCmd::Interest { event_id, .. } => (EventAction::Interest, *event_id),
            EventCmd::Occurrence { event_id, .. } => (EventAction::Occurrence, *event_id),
        }
    }
}

/// Pourquoi une écriture d'administration a échoué.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    Problem(Problem),
    NotFound,
}

#[derive(Debug)]
pub enum EventEvent {
    /// Liste complète d'un serveur (GUILD_CREATE, READY) : remplace l'ancienne.
    Guild { guild_id: u64, events: Vec<ScheduledEvent> },
    /// Réponse de la liste (avec les nombres d'intéressés) ; `mine` : mes
    /// inscriptions (`None` : non demandées ou illisibles).
    Loaded { guild_id: u64, events: Vec<ScheduledEvent>, mine: Option<Vec<u64>> },
    /// Créé ou modifié (passerelle ou réponse).
    Upsert(ScheduledEvent),
    Deleted { guild_id: u64, event_id: u64 },
    UserAdd { guild_id: u64, event_id: u64, user_id: u64 },
    UserRemove { guild_id: u64, event_id: u64, user_id: u64 },
    /// Mon intérêt enregistré (réponse au `PUT` / `DELETE`).
    Interest { guild_id: u64, event_id: u64, on: bool },
    /// Occurrence sautée (`skip`) ou rétablie, confirmée par Discord.
    Skipped { guild_id: u64, event_id: u64, original_ms: i64, skip: bool },
    /// Écriture réussie (la fenêtre qui l'a lancée peut se fermer).
    Done { action: EventAction, key: u64 },
    Failed { action: EventAction, key: u64, failure: Failure },
}

/// Message à afficher (mis en mots par l'interface).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventNotice {
    Failed { action: EventAction, failure: Failure },
}

#[derive(Clone, Debug, Default)]
pub struct GuildEvents {
    /// Triés par date de début.
    pub list: Vec<ScheduledEvent>,
    /// Événements qui m'intéressent.
    pub mine: HashSet<u64>,
    /// La liste a été lue par l'API (nombres d'intéressés connus).
    pub loaded: bool,
}

#[derive(Default)]
pub struct Events {
    guilds: HashMap<u64, GuildEvents>,
    pending: HashSet<(EventAction, u64)>,
    notices: VecDeque<EventNotice>,
    done: Option<(EventAction, u64)>,
    /// Événements passés à « en cours » pendant la session, pas encore
    /// annoncés : (serveur, événement) (lot FF).
    started: Vec<(u64, u64)>,
}

impl Events {
    pub fn clear(&mut self) {
        *self = Events::default();
    }

    pub fn guild(&self, id: u64) -> Option<&GuildEvents> {
        self.guilds.get(&id)
    }

    pub fn forget_guild(&mut self, id: u64) {
        self.guilds.remove(&id);
    }

    pub fn pending(&self, action: EventAction, key: u64) -> bool {
        self.pending.contains(&(action, key))
    }

    pub fn take_notice(&mut self) -> Option<EventNotice> {
        self.notices.pop_front()
    }

    pub fn take_done(&mut self) -> Option<(EventAction, u64)> {
        self.done.take()
    }

    /// Événements programmés ou en cours de ce serveur.
    pub fn upcoming(&self, guild: u64) -> usize {
        self.guilds.get(&guild).map_or(0, |g| g.list.iter().filter(|e| e.upcoming()).count())
    }

    /// Événements qui viennent de commencer, rendus une fois.
    pub fn take_started(&mut self) -> Vec<(u64, u64)> {
        std::mem::take(&mut self.started)
    }

    fn upsert(&mut self, mut e: ScheduledEvent) {
        let g = self.guilds.entry(e.guild_id).or_default();
        match g.list.iter_mut().find(|x| x.id == e.id) {
            Some(slot) => {
                if e.user_count.is_none() {
                    e.user_count = slot.user_count;
                }
                if e.status == Status::Active && slot.status == Status::Scheduled && self.started.len() < 8 {
                    self.started.push((e.guild_id, e.id));
                }
                *slot = e;
            }
            None => g.list.push(e),
        }
        sort(&mut g.list);
    }
}

/// Terminés et annulés d'abord retirés quand la borne est atteinte, puis tri par date.
fn sort(list: &mut Vec<ScheduledEvent>) {
    list.sort_by_key(|e| (!e.upcoming(), e.start_ms, e.id));
    list.truncate(MAX_PER_GUILD);
}

/// Instant (ms Unix) d'une date « AAAA-MM-JJ » et d'une heure « HH:MM » locales.
pub fn local_ms(date: &str, time: &str) -> Option<i64> {
    let d = NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d").ok()?;
    let t = NaiveTime::parse_from_str(time.trim(), "%H:%M").ok()?;
    Local.from_local_datetime(&d.and_time(t)).earliest().map(|dt| dt.timestamp_millis())
}

/// Date et heure locales d'un instant, dans le format de [`local_ms`].
pub fn local_fields(ms: i64) -> (String, String) {
    match Local.timestamp_millis_opt(ms).single() {
        Some(dt) => (dt.format("%Y-%m-%d").to_string(), dt.format("%H:%M").to_string()),
        None => (String::new(), String::new()),
    }
}

/// Vérifie un brouillon ; `now_ms` : début refusé s'il est passé (création seulement).
pub fn check(d: &EventDraft, now_ms: Option<i64>) -> Result<EventDraft, DraftError> {
    let name = d.name.trim();
    if name.is_empty() || name.chars().count() > NAME_MAX {
        return Err(DraftError::Name);
    }
    if d.description.chars().count() > DESCRIPTION_MAX {
        return Err(DraftError::Description);
    }
    let place = match &d.place {
        Place::External(l) => {
            let l = l.trim();
            if l.is_empty() || l.chars().count() > LOCATION_MAX {
                return Err(DraftError::Location);
            }
            if d.end_ms.is_none() {
                return Err(DraftError::EndRequired);
            }
            Place::External(l.to_string())
        }
        Place::Stage(0) | Place::Voice(0) | Place::Other => return Err(DraftError::Channel),
        p => p.clone(),
    };
    if now_ms.is_some_and(|now| d.start_ms <= now) {
        return Err(DraftError::StartPast);
    }
    if d.end_ms.is_some_and(|e| e <= d.start_ms) {
        return Err(DraftError::EndBeforeStart);
    }
    Ok(EventDraft { name: name.to_string(), description: d.description.trim().to_string(), place, ..d.clone() })
}

impl Store {
    fn event_cmd(&mut self, cmd: EventCmd) -> bool {
        if !self.events.pending.insert(cmd.action()) {
            return false;
        }
        self.push_command(Cmd::GuildEvents(cmd));
        true
    }

    fn my_guild_perms(&self, guild: u64) -> Option<Permissions> {
        let (me, g) = (self.me.as_ref()?, self.guilds.iter().find(|g| g.id == guild)?);
        base_permissions(g, me.id)
    }

    /// « Créer un événement » : CREATE_EVENTS ou MANAGE_EVENTS (inconnu : proposé, Discord répondra).
    pub fn can_create_events(&self, guild: u64) -> bool {
        self.me.is_some() && self.guilds.iter().any(|g| g.id == guild) && self.my_guild_perms(guild).is_none_or(|p| p.contains(Permissions::CREATE_EVENTS) || p.contains(Permissions::MANAGE_EVENTS))
    }

    /// Modifier, commencer, terminer, annuler ou supprimer cet événement :
    /// MANAGE_EVENTS, ou CREATE_EVENTS pour les miens.
    pub fn can_manage_event(&self, e: &ScheduledEvent) -> bool {
        let me = self.me.as_ref().map(|u| u.id);
        if self.me.is_none() || !self.guilds.iter().any(|g| g.id == e.guild_id) {
            return false;
        }
        self.my_guild_perms(e.guild_id).is_none_or(|p| p.contains(Permissions::MANAGE_EVENTS) || (p.contains(Permissions::CREATE_EVENTS) && e.creator_id.is_some() && e.creator_id == me))
    }

    pub fn find_event(&self, guild: u64, id: u64) -> Option<&ScheduledEvent> {
        self.events.guild(guild)?.list.iter().find(|e| e.id == id)
    }

    /// Liste avec les nombres d'intéressés, demandée à l'ouverture de la fenêtre.
    pub fn load_events(&mut self, guild_id: u64) -> bool {
        self.event_cmd(EventCmd::Load { guild_id })
    }

    pub fn create_event(&mut self, guild_id: u64, draft: &EventDraft) -> Result<(), DraftError> {
        let draft = check(draft, Some(self.now()))?;
        if self.event_cmd(EventCmd::Create { guild_id, draft }) { Ok(()) } else { Err(DraftError::Busy) }
    }

    pub fn edit_event(&mut self, guild_id: u64, event_id: u64, draft: &EventDraft) -> Result<(), DraftError> {
        // Un événement en cours garde son début (déjà passé).
        let draft = check(draft, None)?;
        if self.event_cmd(EventCmd::Edit { guild_id, event_id, draft }) { Ok(()) } else { Err(DraftError::Busy) }
    }

    /// Commencer, terminer ou annuler ; refusé si le passage n'est pas permis.
    pub fn set_event_status(&mut self, guild_id: u64, event_id: u64, status: Status) -> bool {
        let Some(e) = self.find_event(guild_id, event_id) else { return false };
        if !e.status.can_become(status) {
            return false;
        }
        self.event_cmd(EventCmd::SetStatus { guild_id, event_id, status })
    }

    pub fn delete_event(&mut self, guild_id: u64, event_id: u64) -> bool {
        self.event_cmd(EventCmd::Delete { guild_id, event_id })
    }

    pub fn set_event_interest(&mut self, guild_id: u64, event_id: u64, on: bool) -> bool {
        self.event_cmd(EventCmd::Interest { guild_id, event_id, on })
    }

    /// Sauter ou rétablir la prochaine occurrence d'un événement récurrent
    /// (geste de l'utilisateur ; MANAGE_EVENTS, comme le reste de la gestion).
    pub fn skip_occurrence(&mut self, guild_id: u64, event_id: u64, skip: bool) -> bool {
        let Some(e) = self.find_event(guild_id, event_id) else { return false };
        if e.recurrence.is_none() || !self.can_manage_event(e) {
            return false;
        }
        let original_ms = e.start_ms;
        self.event_cmd(EventCmd::Occurrence { guild_id, event_id, original_ms, skip })
    }

    pub fn event_interested(&self, guild: u64, id: u64) -> bool {
        self.events.guild(guild).is_some_and(|g| g.mine.contains(&id))
    }

    pub(crate) fn apply_guild_events(&mut self, e: EventEvent) {
        let me = self.me.as_ref().map(|u| u.id);
        let s = &mut self.events;
        match e {
            EventEvent::Guild { guild_id, events } => {
                let g = s.guilds.entry(guild_id).or_default();
                let counts: HashMap<u64, u32> = g.list.iter().filter_map(|e| Some((e.id, e.user_count?))).collect();
                g.list = events;
                for e in &mut g.list {
                    e.user_count = e.user_count.or_else(|| counts.get(&e.id).copied());
                }
                sort(&mut g.list);
            }
            EventEvent::Loaded { guild_id, events, mine } => {
                s.pending.remove(&(EventAction::Load, guild_id));
                let g = s.guilds.entry(guild_id).or_default();
                g.list = events;
                sort(&mut g.list);
                g.loaded = true;
                if let Some(mine) = mine {
                    g.mine = mine.into_iter().collect();
                }
            }
            EventEvent::Upsert(e) => s.upsert(e),
            EventEvent::Deleted { guild_id, event_id } => {
                if let Some(g) = s.guilds.get_mut(&guild_id) {
                    g.list.retain(|e| e.id != event_id);
                    g.mine.remove(&event_id);
                }
            }
            EventEvent::UserAdd { guild_id, event_id, user_id } | EventEvent::UserRemove { guild_id, event_id, user_id } => {
                let add = matches!(e, EventEvent::UserAdd { .. });
                if let Some(g) = s.guilds.get_mut(&guild_id) {
                    if let Some(ev) = g.list.iter_mut().find(|x| x.id == event_id)
                        && let Some(n) = ev.user_count.as_mut()
                    {
                        *n = if add { n.saturating_add(1) } else { n.saturating_sub(1) };
                    }
                    if me == Some(user_id) {
                        if add {
                            g.mine.insert(event_id);
                        } else {
                            g.mine.remove(&event_id);
                        }
                    }
                }
            }
            EventEvent::Interest { guild_id, event_id, on } => {
                s.pending.remove(&(EventAction::Interest, event_id));
                let g = s.guilds.entry(guild_id).or_default();
                if on {
                    g.mine.insert(event_id);
                } else {
                    g.mine.remove(&event_id);
                }
            }
            EventEvent::Skipped { guild_id, event_id, original_ms, skip } => {
                if let Some(e) = s.guilds.get_mut(&guild_id).and_then(|g| g.list.iter_mut().find(|e| e.id == event_id)) {
                    e.skipped.retain(|t| *t != original_ms);
                    if skip {
                        e.skipped.push(original_ms);
                    }
                }
            }
            EventEvent::Done { action, key } => {
                s.pending.remove(&(action, key));
                s.done = Some((action, key));
            }
            EventEvent::Failed { action, key, failure } => {
                s.pending.remove(&(action, key));
                s.notices.push_back(EventNotice::Failed { action, failure });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::event::Event;
    use crate::state::model::{Guild, Role, User};

    const G: u64 = 10;
    const ME: u64 = 1;

    fn store(perms: Permissions) -> Store {
        let mut s = Store::new();
        let mut g = Guild::new(G, "Club".into(), Vec::new());
        g.roles.push(Role { id: G, name: "@everyone".into(), color: 0, position: 0, permissions: perms, mentionable: false });
        g.owner_id = Some(99);
        g.me = Some(crate::state::model::Member::default());
        s.apply(Event::Ready { me: User::new(ME, "moi".into()), guilds: vec![g], dms: Vec::new() });
        s
    }

    fn ev(id: u64, start: i64, status: Status) -> ScheduledEvent {
        ScheduledEvent {
            id,
            guild_id: G,
            name: format!("é{id}"),
            description: String::new(),
            start_ms: start,
            end_ms: None,
            status,
            place: Place::External("Parc".into()),
            creator_id: Some(ME),
            user_count: None,
            image: None,
            recurrence: None,
            skipped: Vec::new(),
        }
    }

    fn draft(start: i64) -> EventDraft {
        EventDraft { name: " Pique-nique ".into(), description: String::new(), start_ms: start, end_ms: Some(start + 3_600_000), place: Place::External(" Parc ".into()), recurrence: None, image: None }
    }

    #[test]
    fn drafts_are_checked_before_sending() {
        let now = 1_000_000;
        assert_eq!(check(&draft(now + 1), Some(now)).map(|d| (d.name, d.place)), Ok(("Pique-nique".into(), Place::External("Parc".into()))));
        assert_eq!(check(&draft(now), Some(now)), Err(DraftError::StartPast));
        assert_eq!(check(&EventDraft { end_ms: None, ..draft(now + 1) }, None), Err(DraftError::EndRequired));
        assert_eq!(check(&EventDraft { end_ms: Some(now), ..draft(now + 1) }, None), Err(DraftError::EndBeforeStart));
        assert_eq!(check(&EventDraft { name: "  ".into(), ..draft(now + 1) }, None), Err(DraftError::Name));
        assert_eq!(check(&EventDraft { place: Place::Voice(0), ..draft(now + 1) }, None), Err(DraftError::Channel));
        // Vocal : la fin est facultative.
        assert!(check(&EventDraft { place: Place::Voice(5), end_ms: None, ..draft(now + 1) }, None).is_ok());
        assert_eq!(check(&EventDraft { description: "x".repeat(1001), ..draft(now + 1) }, None), Err(DraftError::Description));
    }

    #[test]
    fn next_occurrence_is_skipped_then_restored() {
        let mut s = store(Permissions::MANAGE_EVENTS);
        let rule = Recurrence { frequency: Frequency::Weekly, interval: 1, by_weekday: vec![0], by_n_weekday: None, by_month_day: None };
        let once = ev(1, 500, Status::Scheduled);
        let weekly = ScheduledEvent { recurrence: Some(rule), ..ev(2, 700, Status::Scheduled) };
        s.apply(Event::GuildEvents(EventEvent::Loaded { guild_id: G, events: vec![once, weekly], mine: None }));
        assert!(!s.skip_occurrence(G, 1, true), "événement unique : rien à sauter");
        assert!(s.skip_occurrence(G, 2, true));
        assert!(!s.skip_occurrence(G, 2, true), "une requête à la fois");
        let sent: Vec<EventCmd> = s.take_commands().into_iter().filter_map(|c| if let Cmd::GuildEvents(e) = c { Some(e) } else { None }).collect();
        assert_eq!(sent, vec![EventCmd::Occurrence { guild_id: G, event_id: 2, original_ms: 700, skip: true }]);
        s.apply(Event::GuildEvents(EventEvent::Skipped { guild_id: G, event_id: 2, original_ms: 700, skip: true }));
        s.apply(Event::GuildEvents(EventEvent::Done { action: EventAction::Occurrence, key: 2 }));
        assert!(s.find_event(G, 2).is_some_and(ScheduledEvent::next_skipped));
        assert!(s.skip_occurrence(G, 2, false));
        s.apply(Event::GuildEvents(EventEvent::Skipped { guild_id: G, event_id: 2, original_ms: 700, skip: false }));
        assert!(s.find_event(G, 2).is_some_and(|e| !e.next_skipped()));
    }

    #[test]
    fn status_transitions_follow_the_documentation() {
        assert!(Status::Scheduled.can_become(Status::Active));
        assert!(Status::Scheduled.can_become(Status::Canceled));
        assert!(Status::Active.can_become(Status::Completed));
        assert!(!Status::Active.can_become(Status::Canceled));
        assert!(!Status::Completed.can_become(Status::Active));
    }

    #[test]
    fn permissions_decide_what_is_offered() {
        let s = store(Permissions::VIEW_CHANNEL);
        assert!(!s.can_create_events(G));
        let mine = ev(1, 0, Status::Scheduled);
        assert!(!s.can_manage_event(&mine));
        let s = store(Permissions::CREATE_EVENTS);
        assert!(s.can_create_events(G));
        assert!(s.can_manage_event(&mine), "le mien");
        assert!(!s.can_manage_event(&ScheduledEvent { creator_id: Some(2), ..mine.clone() }), "pas le mien");
        let s = store(Permissions::MANAGE_EVENTS);
        assert!(s.can_manage_event(&ScheduledEvent { creator_id: Some(2), ..mine }));
        assert!(!s.can_create_events(99), "serveur inconnu");
    }

    #[test]
    fn list_follows_gateway_and_keeps_counts() {
        let mut s = store(Permissions::MANAGE_EVENTS);
        s.apply(Event::GuildEvents(EventEvent::Loaded { guild_id: G, events: vec![ScheduledEvent { user_count: Some(4), ..ev(2, 200, Status::Scheduled) }, ev(1, 100, Status::Active)], mine: Some(vec![2]) }));
        assert_eq!(s.events.guild(G).map(|g| g.list.iter().map(|e| e.id).collect::<Vec<_>>()), Some(vec![1, 2]));
        assert!(s.event_interested(G, 2));
        // Mise à jour de la passerelle sans nombre : le nombre connu reste.
        s.apply(Event::GuildEvents(EventEvent::Upsert(ScheduledEvent { name: "nouveau".into(), ..ev(2, 200, Status::Scheduled) })));
        assert_eq!(s.find_event(G, 2).and_then(|e| e.user_count), Some(4));
        s.apply(Event::GuildEvents(EventEvent::UserAdd { guild_id: G, event_id: 2, user_id: 7 }));
        assert_eq!(s.find_event(G, 2).and_then(|e| e.user_count), Some(5));
        s.apply(Event::GuildEvents(EventEvent::UserRemove { guild_id: G, event_id: 2, user_id: ME }));
        assert!(!s.event_interested(G, 2), "mon retrait vient de la passerelle");
        assert_eq!(s.events.upcoming(G), 2);
        s.apply(Event::GuildEvents(EventEvent::Upsert(ev(1, 100, Status::Completed))));
        assert_eq!(s.events.upcoming(G), 1);
        s.apply(Event::GuildEvents(EventEvent::Deleted { guild_id: G, event_id: 2 }));
        assert_eq!(s.events.upcoming(G), 0);
        s.apply(Event::GuildDelete(G));
        assert!(s.events.guild(G).is_none());
    }

    #[test]
    fn one_write_per_event_and_status_rules() {
        let mut s = store(Permissions::MANAGE_EVENTS);
        s.apply(Event::GuildEvents(EventEvent::Guild { guild_id: G, events: vec![ev(1, 100, Status::Scheduled)] }));
        assert!(!s.set_event_status(G, 1, Status::Completed), "pas permis");
        assert!(s.set_event_status(G, 1, Status::Active));
        assert!(!s.set_event_status(G, 1, Status::Active), "déjà en cours d'envoi");
        assert!(s.set_event_interest(G, 1, true));
        assert_eq!(
            s.take_commands(),
            vec![Cmd::GuildEvents(EventCmd::SetStatus { guild_id: G, event_id: 1, status: Status::Active }), Cmd::GuildEvents(EventCmd::Interest { guild_id: G, event_id: 1, on: true })]
        );
        s.apply(Event::GuildEvents(EventEvent::Failed { action: EventAction::Status, key: 1, failure: Failure::Problem(Problem::MfaRequired) }));
        assert_eq!(s.events.take_notice(), Some(EventNotice::Failed { action: EventAction::Status, failure: Failure::Problem(Problem::MfaRequired) }));
        assert!(!s.events.pending(EventAction::Status, 1));
        s.apply(Event::GuildEvents(EventEvent::Interest { guild_id: G, event_id: 1, on: true }));
        assert!(s.event_interested(G, 1));
        let start = s.now() + 86_400_000;
        assert_eq!(s.create_event(G, &draft(start)), Ok(()));
        assert_eq!(s.create_event(G, &draft(start)), Err(DraftError::Busy));
        s.apply(Event::GuildEvents(EventEvent::Done { action: EventAction::Create, key: G }));
        assert_eq!(s.events.take_done(), Some((EventAction::Create, G)));
    }

    #[test]
    fn repeats_build_documented_rules() {
        // Lundi 16 novembre 2026, 12 h (heure locale).
        let Some(start) = local_ms("2026-11-16", "12:00") else { panic!("date") };
        assert_eq!(local_fields(start), ("2026-11-16".to_string(), "12:00".to_string()));
        assert_eq!(Repeat::None.rule(start), None);
        let w = Repeat::Weekly.rule(start);
        assert_eq!(w.as_ref().map(|r| (r.frequency, r.interval, r.by_weekday.clone())), Some((Frequency::Weekly, 1, vec![0])));
        assert_eq!(Repeat::Biweekly.rule(start).map(|r| r.interval), Some(2));
        assert_eq!(Repeat::Monthly.rule(start).and_then(|r| r.by_n_weekday), Some((3, 0)), "troisième lundi");
        assert_eq!(Repeat::Yearly.rule(start).and_then(|r| r.by_month_day), Some((11, 16)));
        for r in Repeat::ALL {
            assert_eq!(Repeat::of(r.rule(start).as_ref()), r);
        }
        assert_eq!(local_ms("2026-13-01", "12:00"), None);
        assert_eq!(local_ms("2026-11-16", "25:00"), None);
    }
}
