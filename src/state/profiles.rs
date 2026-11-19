//! Profil complet d'un autre utilisateur (`GET /users/{id}/profile`, non
//! documenté par Discord, forme lue sur docs.discord.food) et note
//! personnelle (`GET` / `PUT /users/@me/notes/{id}`).
//!
//! Comme le reste de `people`, ces tables vivent dans le [`PeopleHub`]
//! partagé : le réseau y écrit la réponse et réveille l'interface, sans
//! événement. Chaque table est bornée (éviction du huitième le plus ancien,
//! voir `people::Lru`).
//!
//! Rythme : un profil est demandé à l'ouverture d'une carte, une fois par
//! utilisateur (et par serveur) ; rouvrir la carte plus de
//! [`PROFILE_TTL_MS`] après la dernière demande le redemande. Une note est
//! lue une fois par session, écrite à la sortie du champ si elle a changé.
//!
//! [`PeopleHub`]: super::people::PeopleHub

use super::event::Cmd;
use super::markdown::{self, Doc, Resolve, TimeStyle};
use super::model::ImageHash;
use super::people::Lru;
use super::store::{now_ms, Store};
use crate::i18n::{t, time, Lang};

/// Âge au-delà duquel une carte rouverte redemande le profil.
pub const PROFILE_TTL_MS: i64 = 5 * 60 * 1000;
/// Profils gardés (un par couple utilisateur, serveur).
pub const MAX_PROFILES: usize = 200;
/// Notes gardées.
pub const MAX_NOTES: usize = 500;
/// Longueur gardée d'une bio (Discord en accepte 190), en caractères.
pub const MAX_BIO: usize = 400;
/// Longueur maximale d'une note (documentée : 256).
pub const MAX_NOTE: usize = 256;
/// Longueur gardée des petits textes (pronoms, badges, comptes liés).
pub const MAX_SHORT: usize = 128;
/// Éléments gardés par liste (badges, comptes liés, serveurs et amis en commun).
pub const MAX_LIST: usize = 100;

/// Texte reçu ramené à `n` caractères.
pub fn clip_to(s: &str, n: usize) -> String {
    match s.char_indices().nth(n) {
        Some((i, _)) => s[..i].to_string(),
        None => s.to_string(),
    }
}

/// Métadonnées d'un profil (`user_profile`, ou `guild_member_profile` pour
/// le profil propre à un serveur). Absent ou vide : `None`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileText {
    pub bio: Option<String>,
    pub pronouns: Option<String>,
    pub banner: Option<ImageHash>,
    /// Couleur de bannière 0xRRGGBB (ignorée par Discord sur un profil de serveur).
    pub accent: Option<u32>,
    /// Couleurs du thème du profil (principale, secondaire), 0xRRGGBB.
    pub theme: Option<[u32; 2]>,
    /// Effet de profil porté : son identifiant (`profile_effect.id`, `state::effects`).
    pub effect: Option<u64>,
    /// SKU du produit de cet effet, si le profil le donne (`profile_effect.sku_id`).
    pub effect_sku: Option<u64>,
}

/// Badge du profil : description toujours, icône si son empreinte est lisible.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileBadge {
    pub id: String,
    pub description: String,
    pub icon: Option<ImageHash>,
    /// Lien du badge, seulement en `https://`.
    pub link: Option<String>,
}

/// Compte lié public (`connected_accounts`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connection {
    /// Service (`github`, `steam`…), tel que reçu.
    pub kind: String,
    pub id: String,
    pub name: String,
    pub verified: bool,
}

/// Caractères admis dans un morceau d'adresse construit à partir d'un nom
/// reçu : rien qui puisse changer d'hôte, de chemin ou ajouter une requête.
fn safe_part(s: &str) -> bool {
    !s.is_empty() && s.len() <= 100 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

fn safe_host(s: &str) -> bool {
    safe_part(s) && s.contains('.') && !s.starts_with('.') && !s.ends_with('.') && !s.contains("..")
}

impl Connection {
    /// Page publique du compte, pour les services dont l'adresse se déduit
    /// sans ambiguïté du nom ou de l'identifiant ; sinon `None` (le nom est
    /// affiché sans lien).
    pub fn url(&self) -> Option<String> {
        let (name, id) = (self.name.as_str(), self.id.as_str());
        let by_name = |fmt: &dyn Fn(&str) -> String| safe_part(name).then(|| fmt(name));
        let by_id = |fmt: &dyn Fn(&str) -> String| safe_part(id).then(|| fmt(id));
        match self.kind.as_str() {
            "github" => by_name(&|n| format!("https://github.com/{n}")),
            "twitch" => by_name(&|n| format!("https://www.twitch.tv/{n}")),
            "reddit" => by_name(&|n| format!("https://www.reddit.com/user/{n}")),
            "twitter" => by_name(&|n| format!("https://x.com/{n}")),
            "tiktok" => by_name(&|n| format!("https://www.tiktok.com/@{n}")),
            "ebay" => by_name(&|n| format!("https://www.ebay.com/usr/{n}")),
            "bluesky" => safe_host(name).then(|| format!("https://bsky.app/profile/{name}")),
            "domain" => safe_host(name).then(|| format!("https://{name}")),
            "steam" => by_id(&|i| format!("https://steamcommunity.com/profiles/{i}")),
            "youtube" => by_id(&|i| format!("https://www.youtube.com/channel/{i}")),
            "spotify" => by_id(&|i| format!("https://open.spotify.com/user/{i}")),
            "roblox" => by_id(&|i| format!("https://www.roblox.com/users/{i}/profile")),
            // `@utilisateur@instance` : la page est sur l'instance.
            "mastodon" => {
                let (user, host) = name.strip_prefix('@').unwrap_or(name).split_once('@')?;
                (safe_part(user) && safe_host(host)).then(|| format!("https://{host}/@{user}"))
            }
            _ => None,
        }
    }
}

/// Serveur en commun et pseudo de l'utilisateur sur ce serveur.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MutualGuild {
    pub id: u64,
    pub nick: Option<String>,
}

/// Réponse de `GET /users/{id}/profile`. Ce qui est absent (profil privé,
/// utilisateur qui m'a bloqué, champ non fourni) reste vide ou `None`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Profile {
    pub user: ProfileText,
    /// Profil propre au serveur demandé.
    pub guild: Option<ProfileText>,
    /// Badges du profil puis ceux du serveur, sans doublon.
    pub badges: Vec<ProfileBadge>,
    /// Forme des badges reçus (noms des champs et types, identifiants des
    /// badges, aucune autre valeur), pour le journal de débogage : les
    /// badges à paliers du client n'ont pas de description publique.
    pub badge_shape: String,
    pub premium_type: Option<u8>,
    /// Début de Nitro (ms Unix).
    pub premium_since: Option<i64>,
    /// Début du boost de serveurs (ms Unix).
    pub premium_guild_since: Option<i64>,
    pub connections: Vec<Connection>,
    /// `None` : non fourni.
    pub mutual_guilds: Option<Vec<MutualGuild>>,
    /// Amis en commun (leurs utilisateurs sont dans `people`).
    pub mutual_friends: Option<Vec<u64>>,
    pub mutual_friends_count: Option<u32>,
    /// Ancien `nom#1234`, si Discord le montre.
    pub legacy_username: Option<String>,
    /// Widgets de jeux (`widgets`, docs.discord.food, resources/widgets).
    pub widgets: Vec<super::widgets::GameWidget>,
    /// Un widget reçu n'a pas été compris : les réécrire le perdrait, on ne les modifie pas.
    pub widgets_opaque: bool,
}

impl Profile {
    /// Bio à afficher : celle du serveur si elle existe, sinon celle du compte.
    pub fn bio(&self) -> Option<&str> {
        self.guild.as_ref().and_then(|g| g.bio.as_deref()).or(self.user.bio.as_deref())
    }

    pub fn pronouns(&self) -> Option<&str> {
        self.guild.as_ref().and_then(|g| g.pronouns.as_deref()).or(self.user.pronouns.as_deref())
    }

    /// Couleurs du thème (serveur d'abord).
    pub fn theme(&self) -> Option<[u32; 2]> {
        self.guild.as_ref().and_then(|g| g.theme).or(self.user.theme)
    }

    /// Effet de profil porté (celui du profil de serveur d'abord).
    pub fn effect(&self) -> Option<u64> {
        self.effect_ref().map(|(id, _)| id)
    }

    /// Effet porté et, s'il est donné, le SKU de son produit (profil de serveur d'abord).
    pub fn effect_ref(&self) -> Option<(u64, Option<u64>)> {
        let of = |t: &ProfileText| t.effect.map(|id| (id, t.effect_sku));
        self.guild.as_ref().and_then(of).or_else(|| of(&self.user))
    }

    /// Nombre d'amis en commun : le compte s'il est fourni, sinon la liste.
    pub fn friends_count(&self) -> Option<usize> {
        self.mutual_friends_count.map(|n| n as usize).or(self.mutual_friends.as_ref().map(Vec::len))
    }
}

/// Où en est le profil complet d'un utilisateur.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileState {
    Loading,
    Ready(Box<Profile>),
    /// Refusé, introuvable, illisible ou compte bot : la carte s'en passe, sans message.
    Unavailable,
    /// Discord demande un captcha : à faire dans l'application officielle.
    Captcha,
}

impl ProfileState {
    pub fn profile(&self) -> Option<&Profile> {
        match self {
            ProfileState::Ready(p) => Some(p),
            _ => None,
        }
    }
}

struct Entry {
    state: ProfileState,
    /// Dernière demande (ms Unix).
    asked_at: i64,
}

/// Note personnelle sur un utilisateur.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Note {
    /// Texte connu de Discord ; `None` tant qu'il n'est pas lu.
    pub text: Option<String>,
    /// Écriture en cours (texte envoyé).
    pub writing: Option<String>,
    /// La dernière écriture a échoué (le texte connu est rétabli).
    pub failed: bool,
    /// Discord demande un captcha pour l'écrire.
    pub captcha: bool,
    /// Lecture impossible (compte bot, refus, erreur) : pas de champ.
    pub unavailable: bool,
}

impl Note {
    /// Texte à montrer dans le champ.
    pub fn shown(&self) -> Option<&str> {
        self.writing.as_deref().or(self.text.as_deref())
    }
}

/// Demandes au réseau.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileCmd {
    /// `GET /users/{id}/profile` (avec `guild_id` sur un serveur).
    Fetch { user_id: u64, guild_id: Option<u64> },
    /// `GET /users/@me/notes/{id}`.
    Note { user_id: u64 },
    /// `PUT /users/@me/notes/{id}`.
    SetNote { user_id: u64, note: String },
}

/// Profils et notes, rangés dans `People`.
pub struct ProfileTables {
    profiles: Lru<(u64, Option<u64>), Entry>,
    notes: Lru<u64, Note>,
}

impl Default for ProfileTables {
    fn default() -> Self {
        ProfileTables { profiles: Lru::new(MAX_PROFILES), notes: Lru::new(MAX_NOTES) }
    }
}

impl ProfileTables {
    pub fn profile(&self, user_id: u64, guild_id: Option<u64>) -> Option<&ProfileState> {
        self.profiles.get(&(user_id, guild_id)).map(|e| &e.state)
    }

    /// Profil connu de cet utilisateur, de préférence celui de ce serveur
    /// (amis et serveurs en commun ne dépendent pas du serveur).
    pub fn best(&self, user_id: u64, guild_id: Option<u64>) -> Option<&ProfileState> {
        let exact = self.profile(user_id, guild_id);
        if exact.is_some_and(|s| s.profile().is_some()) || guild_id.is_none() {
            return exact;
        }
        self.profile(user_id, None).filter(|s| s.profile().is_some()).or(exact)
    }

    /// Ouverture d'une carte à `now` (ms Unix) : vrai si la demande doit partir
    /// (jamais demandé, ou dernière demande terminée depuis plus de
    /// [`PROFILE_TTL_MS`]). Un profil déjà reçu reste affiché pendant qu'il
    /// est redemandé.
    pub fn begin(&mut self, user_id: u64, guild_id: Option<u64>, now: i64) -> bool {
        let key = (user_id, guild_id);
        match self.profiles.get_mut(&key) {
            Some(e) if e.state == ProfileState::Loading => false,
            Some(e) if now.saturating_sub(e.asked_at) < PROFILE_TTL_MS => false,
            Some(e) => {
                e.asked_at = now;
                if e.state.profile().is_none() {
                    e.state = ProfileState::Loading;
                }
                true
            }
            None => {
                self.profiles.insert(key, Entry { state: ProfileState::Loading, asked_at: now }, |_| false);
                true
            }
        }
    }

    /// La relation avec cet utilisateur a changé (demande acceptée, ami retiré,
    /// blocage, déblocage) : ce que Discord rend de son profil change avec elle
    /// (amis en commun, profil d'une personne bloquée), alors que la carte ne le
    /// demande qu'à son ouverture et le panneau d'un MP une fois par
    /// conversation. Rend les serveurs (`None` : sans serveur) des profils
    /// gardés de cet utilisateur, à redemander tout de suite ; l'ancien profil
    /// reste affiché en attendant. Jamais en masse : un seul utilisateur, et
    /// seulement là où sa carte a déjà été ouverte.
    pub fn relation_changed(&mut self, user_id: u64, now: i64) -> Vec<Option<u64>> {
        let mut guilds = Vec::new();
        for (key, e) in self.profiles.iter_mut() {
            if key.0 == user_id {
                e.asked_at = now;
                guilds.push(key.1);
            }
        }
        guilds.sort_unstable();
        guilds
    }

    /// Réponse du réseau. Un échec après un profil déjà reçu (rafraîchissement)
    /// garde l'ancien.
    pub fn set(&mut self, user_id: u64, guild_id: Option<u64>, state: ProfileState) {
        let key = (user_id, guild_id);
        match self.profiles.get_mut(&key) {
            Some(e) if state == ProfileState::Unavailable && e.state.profile().is_some() => {}
            Some(e) => e.state = state,
            None => self.profiles.insert(key, Entry { state, asked_at: now_ms() }, |_| false),
        }
    }

    /// Profil que l'on vient d'écrire soi-même : la copie gardée suit, sans
    /// nouvelle demande. `guild` : le profil de ce serveur, sinon celui du
    /// compte. Sans profil reçu, rien n'est inventé.
    pub fn edit_text(&mut self, user_id: u64, guild_id: Option<u64>, guild: bool, edit: impl FnOnce(&mut ProfileText)) {
        let Some(Entry { state: ProfileState::Ready(p), .. }) = self.profiles.get_mut(&(user_id, guild_id)) else { return };
        match guild {
            true => edit(p.guild.get_or_insert_with(ProfileText::default)),
            false => edit(&mut p.user),
        }
    }

    /// Widgets que l'on vient d'écrire : la copie gardée suit.
    pub fn set_widgets(&mut self, user_id: u64, guild_id: Option<u64>, list: &[super::widgets::GameWidget]) {
        if let Some(Entry { state: ProfileState::Ready(p), .. }) = self.profiles.get_mut(&(user_id, guild_id)) {
            p.widgets = list.to_vec();
        }
    }

    pub fn note(&self, user_id: u64) -> Option<&Note> {
        self.notes.get(&user_id)
    }

    /// Première lecture de la note dans la session : vrai si elle doit partir.
    pub fn begin_note(&mut self, user_id: u64) -> bool {
        if self.notes.get(&user_id).is_some() {
            return false;
        }
        self.notes.insert(user_id, Note::default(), |_| false);
        true
    }

    /// Note lue (vide : pas de note).
    pub fn note_loaded(&mut self, user_id: u64, text: Option<String>) {
        let note = Note { unavailable: text.is_none(), text, ..Note::default() };
        match self.notes.get_mut(&user_id) {
            // Une écriture partie entre-temps l'emporte.
            Some(n) if n.writing.is_some() => {}
            Some(n) => *n = note,
            None => self.notes.insert(user_id, note, |_| false),
        }
    }

    /// Écriture demandée : vrai si elle doit partir (note lue, texte changé,
    /// aucune écriture en cours).
    pub fn begin_note_write(&mut self, user_id: u64, text: &str) -> bool {
        let Some(n) = self.notes.get_mut(&user_id) else { return false };
        if n.unavailable || n.writing.is_some() || n.text.is_none() || n.text.as_deref() == Some(text) {
            return false;
        }
        n.writing = Some(text.to_string());
        n.failed = false;
        n.captcha = false;
        true
    }

    /// Fin d'une écriture : acceptée, refusée, ou captcha demandé.
    pub fn note_written(&mut self, user_id: u64, result: NoteWrite) {
        let Some(n) = self.notes.get_mut(&user_id) else { return };
        let sent = n.writing.take();
        match result {
            NoteWrite::Done => n.text = sent,
            NoteWrite::Failed => n.failed = true,
            NoteWrite::Captcha => n.captcha = true,
        }
    }

    pub fn clear(&mut self) {
        self.profiles.clear();
        self.notes.clear();
    }

    /// (profils, notes) gardés.
    pub fn sizes(&self) -> (usize, usize) {
        (self.profiles.len(), self.notes.len())
    }
}

/// Issue d'une écriture de note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteWrite {
    Done,
    Failed,
    Captcha,
}

/// Mise en mots des mentions d'une bio : noms connus des tables, rôles et
/// salons du serveur, horodatages dans la langue.
struct BioContext<'a> {
    store: &'a Store,
    guild_id: Option<u64>,
    lang: Lang,
    h24: bool,
}

impl Resolve for BioContext<'_> {
    fn user(&self, id: u64) -> String {
        let known = self.store.people.lock().user(id).map(|u| u.display_name().to_string());
        known.unwrap_or_else(|| t::UNKNOWN_USER.get(self.lang).to_string())
    }

    fn role(&self, id: u64) -> (String, u32) {
        let role = self.guild_id.and_then(|g| self.store.guilds.iter().find(|x| x.id == g)).and_then(|g| g.roles.iter().find(|r| r.id == id));
        match role {
            Some(r) => (r.name.clone(), r.color),
            None => (t::UNKNOWN_ROLE.get(self.lang).to_string(), 0),
        }
    }

    fn channel(&self, id: u64) -> String {
        self.store.channel_name(id).unwrap_or_else(|| t::UNKNOWN_CHANNEL.get(self.lang)).to_string()
    }

    fn time(&self, secs: i64, style: TimeStyle) -> Option<String> {
        let ms = secs.checked_mul(1000)?;
        let s = time::stamp(self.lang, ms, now_ms(), style.letter(), self.h24);
        (!s.is_empty()).then_some(s)
    }
}

/// Bio mise en forme (à faire une fois, puis garder le résultat).
pub fn parse_bio(store: &Store, guild_id: Option<u64>, bio: &str, lang: Lang, h24: bool) -> Doc {
    markdown::parse(bio, &BioContext { store, guild_id, lang, h24 })
}

impl Store {
    /// Mon profil vient d'être enregistré : les copies gardées (profil du
    /// compte, et celui vu depuis `shown`, le serveur affiché) suivent.
    /// `guild` : c'est le profil de ce serveur qui a été écrit.
    pub fn own_profile_saved(&mut self, shown: Option<u64>, guild: Option<u64>, edit: impl Fn(&mut ProfileText)) {
        let Some(me) = self.me.as_ref().map(|m| m.id) else { return };
        let mut people = self.people.lock();
        let tables = people.profiles_mut();
        match guild {
            Some(g) => tables.edit_text(me, Some(g), true, &edit),
            None => {
                tables.edit_text(me, None, false, &edit);
                if shown.is_some() {
                    tables.edit_text(me, shown, false, &edit);
                }
            }
        }
    }

    /// Note de `user_id`, lue une fois par session (carte ouverte).
    pub fn want_note(&mut self, user_id: u64) {
        if self.me.as_ref().is_some_and(|m| m.id == user_id) {
            return;
        }
        if self.people.lock().profiles_mut().begin_note(user_id) {
            self.push_command(Cmd::Profile(ProfileCmd::Note { user_id }));
        }
    }

    /// Sortie du champ de note : écrite si elle a changé (256 caractères au plus).
    pub fn save_note(&mut self, user_id: u64, text: &str) {
        let text = clip_to(text, MAX_NOTE);
        if self.people.lock().profiles_mut().begin_note_write(user_id, &text) {
            self.push_command(Cmd::Profile(ProfileCmd::SetNote { user_id, note: text }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::User;
    use crate::state::Event;

    fn conn(kind: &str, id: &str, name: &str) -> Connection {
        Connection { kind: kind.into(), id: id.into(), name: name.into(), verified: true }
    }

    #[test]
    fn connection_urls_only_from_safe_parts() {
        assert_eq!(conn("github", "1", "alice-fictive").url().as_deref(), Some("https://github.com/alice-fictive"));
        assert_eq!(conn("steam", "76561190000000000", "Alice").url().as_deref(), Some("https://steamcommunity.com/profiles/76561190000000000"));
        assert_eq!(conn("domain", "x", "exemple.invalid").url().as_deref(), Some("https://exemple.invalid"));
        assert_eq!(conn("mastodon", "1", "@alice@social.example").url().as_deref(), Some("https://social.example/@alice"));
        // Rien qui puisse changer d'hôte ou de chemin.
        for (kind, id, name) in [
            ("github", "1", "a/../b"),
            ("github", "1", "evil.tld?x=1"),
            ("github", "1", ""),
            ("twitch", "1", "a b"),
            ("domain", "x", "localhost"),
            ("domain", "x", "a..b"),
            ("domain", "x", "exemple.invalid/chemin"),
            ("steam", "1/2", "Alice"),
            ("mastodon", "1", "@alice@evil.tld/x"),
            ("battlenet", "1", "Alice#1234"),
            ("inconnu", "1", "alice"),
        ] {
            assert_eq!(conn(kind, id, name).url(), None, "{kind} {id} {name}");
        }
    }

    #[test]
    fn profile_is_asked_once_then_refreshed_after_ttl() {
        let mut p = ProfileTables::default();
        assert!(p.begin(1, Some(9), 1_000));
        assert!(!p.begin(1, Some(9), 1_000 + PROFILE_TTL_MS * 2), "demande en cours");
        p.set(1, Some(9), ProfileState::Ready(Box::default()));
        assert!(!p.begin(1, Some(9), 1_000 + PROFILE_TTL_MS - 1));
        assert!(p.begin(1, Some(9), 1_000 + PROFILE_TTL_MS));
        // Pendant le rafraîchissement, l'ancien profil reste ; un échec ne l'efface pas.
        assert!(p.profile(1, Some(9)).and_then(ProfileState::profile).is_some());
        p.set(1, Some(9), ProfileState::Unavailable);
        assert!(p.profile(1, Some(9)).and_then(ProfileState::profile).is_some());
        // Autre serveur : autre demande ; le profil sans serveur sert en attendant.
        assert!(p.begin(1, None, 0));
        p.set(1, None, ProfileState::Ready(Box::default()));
        assert!(p.begin(1, Some(10), 0));
        assert!(p.best(1, Some(10)).and_then(ProfileState::profile).is_some());
        // Échec sans rien de connu : indisponible, redemandé après le délai.
        assert!(p.begin(2, None, 0));
        p.set(2, None, ProfileState::Unavailable);
        assert!(!p.begin(2, None, 10));
        assert!(p.begin(2, None, PROFILE_TTL_MS));
        assert_eq!(p.profile(2, None), Some(&ProfileState::Loading));
    }

    #[test]
    fn own_saved_profile_updates_the_kept_copies() {
        let mut s = Store::new();
        s.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: Vec::new(), dms: Vec::new() });
        {
            let mut people = s.people.lock();
            let t = people.profiles_mut();
            t.set(1, None, ProfileState::Ready(Box::default()));
            t.set(1, Some(9), ProfileState::Ready(Box::default()));
            t.set(1, Some(10), ProfileState::Loading);
        }
        s.own_profile_saved(Some(9), None, |p| p.bio = Some("nouvelle".into()));
        s.own_profile_saved(Some(9), Some(9), |p| p.theme = Some([1, 2]));
        // Profil pas encore reçu : rien n'est inventé.
        s.own_profile_saved(Some(10), Some(10), |p| p.bio = Some("x".into()));
        let people = s.people.lock();
        let t = people.profiles();
        let of = |g| t.profile(1, g).and_then(ProfileState::profile).cloned().unwrap_or_default();
        assert_eq!(of(None).user.bio.as_deref(), Some("nouvelle"));
        assert_eq!(of(Some(9)).user.bio.as_deref(), Some("nouvelle"));
        assert_eq!(of(Some(9)).guild.and_then(|g| g.theme), Some([1, 2]));
        assert_eq!(t.profile(1, Some(10)), Some(&ProfileState::Loading));
    }

    #[test]
    fn tables_are_bounded() {
        let mut p = ProfileTables::default();
        for id in 0..3 * MAX_PROFILES as u64 {
            p.begin(id, None, 0);
            p.begin_note(id);
        }
        let (a, b) = p.sizes();
        assert!(a <= MAX_PROFILES && b <= MAX_NOTES, "{a} {b}");
        assert!(p.profile(3 * MAX_PROFILES as u64 - 1, None).is_some() && p.profile(0, None).is_none());
    }

    #[test]
    fn note_is_read_once_and_written_when_changed() {
        let mut p = ProfileTables::default();
        assert!(p.begin_note(5));
        assert!(!p.begin_note(5));
        assert!(!p.begin_note_write(5, "x"), "pas encore lue");
        p.note_loaded(5, Some(String::new()));
        assert!(!p.begin_note_write(5, ""), "inchangée");
        assert!(p.begin_note_write(5, "ami du club"));
        assert!(!p.begin_note_write(5, "autre"), "une écriture à la fois");
        assert_eq!(p.note(5).and_then(Note::shown), Some("ami du club"));
        p.note_written(5, NoteWrite::Done);
        assert_eq!(p.note(5).and_then(|n| n.text.as_deref()), Some("ami du club"));
        // Refus : le texte connu revient.
        assert!(p.begin_note_write(5, "refusé"));
        p.note_written(5, NoteWrite::Failed);
        let n = p.note(5).cloned().unwrap_or_default();
        assert_eq!((n.shown(), n.failed), (Some("ami du club"), true));
        // Lecture impossible : pas d'écriture.
        p.begin_note(6);
        p.note_loaded(6, None);
        assert!(p.note(6).is_some_and(|n| n.unavailable));
        assert!(!p.begin_note_write(6, "x"));
    }

    /// Demande d'ami acceptée, blocage, déblocage : le profil gardé est
    /// redemandé sans attendre le délai ni la réouverture de la carte.
    #[test]
    fn relation_change_asks_the_kept_profiles_again() {
        use crate::state::relations::{RelKind, Relation, RelationsEvent};
        let fetch = |guild_id| Cmd::Profile(ProfileCmd::Fetch { user_id: 5, guild_id });
        let mut s = Store::new();
        s.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: Vec::new(), dms: Vec::new() });
        s.want_profile(5, Some(9));
        s.want_profile(5, None);
        s.want_profile(6, None);
        for g in [Some(9), None] {
            s.people.lock().profiles_mut().set(5, g, ProfileState::Ready(Box::default()));
        }
        s.take_commands();
        let rel = |kind| Relation { id: 5, kind, nickname: None, ignored: false, stranger: false, user: None };
        // Demande acceptée.
        s.apply(Event::Relations(RelationsEvent::RelationAdd(rel(RelKind::Friend))));
        assert_eq!(s.take_commands(), vec![fetch(None), fetch(Some(9))], "les deux profils gardés, pas celui d'un autre");
        // L'ancien profil reste affiché pendant la demande ; la carte rouverte n'en ajoute pas une.
        assert!(s.people.lock().profiles().profile(5, None).and_then(ProfileState::profile).is_some());
        s.want_profile(5, None);
        assert_eq!(s.take_commands(), Vec::new());
        // Blocage (RELATIONSHIP_ADD de type bloqué), puis déblocage (RELATIONSHIP_REMOVE).
        s.apply(Event::Relations(RelationsEvent::RelationAdd(rel(RelKind::Blocked))));
        assert_eq!(s.take_commands(), vec![fetch(None), fetch(Some(9))]);
        s.apply(Event::Relations(RelationsEvent::RelationRemove { id: 5 }));
        assert_eq!(s.take_commands(), vec![fetch(None), fetch(Some(9))]);
        // Blocage accepté par Discord : appliqué à la réponse ; le RELATIONSHIP_ADD
        // qui le répète ne redemande pas le profil une seconde fois.
        s.apply(Event::Relations(RelationsEvent::Done { id: 5, what: crate::state::relations::Done::Blocked }));
        assert_eq!(s.take_commands(), vec![fetch(None), fetch(Some(9))]);
        s.apply(Event::Relations(RelationsEvent::RelationAdd(rel(RelKind::Blocked))));
        assert_eq!(s.take_commands(), Vec::new());
        // Personne dont le profil n'a jamais été ouvert : rien ne part.
        s.apply(Event::Relations(RelationsEvent::RelationAdd(Relation { id: 7, ..rel(RelKind::Incoming) })));
        assert_eq!(s.take_commands(), Vec::new());
    }

    /// USER_UPDATE : mon nom et mon avatar suivent ; les comptes liés ne sont
    /// relus que si la page les avait déjà demandés.
    #[test]
    fn my_account_follows_changes_made_elsewhere() {
        use crate::state::usersettings::{AccountCmd, AccountEvent, Remote};
        let mut s = Store::new();
        s.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: Vec::new(), dms: Vec::new() });
        s.apply(Event::MeUpdate(User::new(2, "Autre".into())));
        assert_eq!(s.me.as_ref().map(|m| m.name.as_str()), Some("Moi"));
        s.apply(Event::MeUpdate(User::new(1, "Moi Nouveau".into())));
        assert_eq!(s.me.as_ref().map(|m| m.name.as_str()), Some("Moi Nouveau"));
        s.apply(Event::Account(AccountEvent::ConnectionsChanged));
        assert_eq!(s.account.connections, Remote::Idle, "jamais ouverte : rien ne part");
        s.apply(Event::Account(AccountEvent::Connections(Ok(Vec::new()))));
        s.apply(Event::Account(AccountEvent::ConnectionsChanged));
        assert_eq!(s.account.connections, Remote::Loading);
        assert!(s.take_commands().contains(&Cmd::Account(AccountCmd::LoadConnections)));
    }

    #[test]
    fn store_asks_profile_and_note_once() {
        let mut s = Store::new();
        s.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: Vec::new(), dms: Vec::new() });
        s.take_commands();
        s.want_profile(5, Some(9));
        s.want_profile(5, Some(9));
        s.want_note(5);
        s.want_note(5);
        s.want_note(1);
        assert_eq!(
            s.take_commands(),
            vec![Cmd::Profile(ProfileCmd::Fetch { user_id: 5, guild_id: Some(9) }), Cmd::Profile(ProfileCmd::Note { user_id: 5 })]
        );
        s.people.lock().profiles_mut().note_loaded(5, Some("a".into()));
        s.save_note(5, "a");
        s.save_note(5, &"é".repeat(MAX_NOTE + 5));
        assert_eq!(s.take_commands(), vec![Cmd::Profile(ProfileCmd::SetNote { user_id: 5, note: "é".repeat(MAX_NOTE) })]);
    }
}
