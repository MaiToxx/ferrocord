//! Couche réseau : passerelle WebSocket (temps réel) + API REST de Discord.
//! Ne connaît pas l'interface : elle publie des `Event` et se contente
//! d'appeler un réveil fourni par l'appelant.
//!
//! Optimisations :
//! - l'enveloppe de chaque événement est lue sans construire d'arbre JSON
//!   (`RawValue`) : les événements qui ne nous intéressent pas (présences,
//!   statuts, etc.) sont ignorés sans être décodés ;
//! - les événements utiles sont décodés directement dans des structures
//!   typées qui ne gardent que les champs affichés.

mod account;
// Administration des serveurs (phase 3).
mod admin;
/// Décodeurs des réponses de modération (aperçu) : lus comme le réseau les lit.
pub use admin::moderation::{decode_audit, decode_bans, decode_rules, decode_safety};
pub mod cdn;
// Traduction d'un message par le service choisi (plugin du lot E3) : pas une requête à Discord.
pub mod translate;
mod gateway;
// Activité de jeu détectée, op 3, boutons d'activité (lot C3).
mod games;
/// Statut « inactif » automatique (lot FF).
pub mod idle;
pub use games::decode_detectable_list;
mod guilds;
mod gwlimit;
// Identité annoncée à Discord : bureau ou Ferrocord (décision du 14 novembre 2026).
pub mod identity;
// Mentions récentes de la boîte de réception (lot navigation).
mod inbox;
mod inflate;
// Commandes et composants des applications (lot T11).
mod interactions;
mod media;
mod members;
mod people;
// Profil complet et notes (lot profil-complet).
mod profile;
mod cosmetics;
mod ratelimit;
mod raw;
mod rest;
// Recherche de messages et saut vers un message (lot T12).
mod search;
pub mod social;
// Connexion vocale : passerelle v8, UDP, chiffrement du transport (lot V1).
pub mod voice;
mod threads;
#[cfg(debug_assertions)]
mod shapes;
mod stickers;
// GIF, favoris et fréquence du compte, super réaction (lot G).
mod gifs;
pub use gifs::{decode_gif_list, decode_gif_trending};
// Fonctions non documentées (lot ND) : effets de profil, quêtes.
mod effects;
pub use effects::{decode_effect_catalog, decode_effect_product, decode_owned_effects, decode_owned_looks};
mod quests;
mod tags;
mod widgets;
pub use widgets::{decode_games, decode_suggested, decode_widgets};
mod shop;
pub use shop::decode_shop;
mod inventory;
pub use inventory::decode_entitlements;
mod discovery;
pub use discovery::{decode_discoverable, decode_discovery_categories};
mod summaries;
pub use summaries::decode_summaries;
mod verification;
pub use verification::{decode_join_requests, decode_join_status, decode_member_verification};
mod appdir;
pub use appdir::decode_app_directory;
mod safety;
pub use safety::decode_safety_hub;
mod saved;
pub use saved::decode_saved_messages;
mod suggestions;
pub use suggestions::decode_suggestions;
pub use quests::{decode_quest_status, decode_quests};
// Amis, MP de groupe, accusés de lecture (lot S3 + T9).
mod relations;
// Sondages, annonces, invitations, adresses expirées (lot T13).
mod announce;
mod polls;
// Vocal sur la passerelle principale et modération vocale (phase 4, lot V4).
mod voicegw;
// Caméra et directs sur la passerelle principale (phase 5, lot M4).
mod streamgw;
// Appels privés, conférences, tableau de sons (lot V6).
mod calls;
// Connexion par QR code depuis l'application mobile (phase 6, lot C2).
pub mod remote_auth;
pub use calls::decode_default_sounds;

/// Décode une liste d'événements programmés (aperçu hors ligne).
pub fn decode_scheduled_events(body: &str) -> Vec<crate::state::admin::events::ScheduledEvent> {
    admin::events::decode_list(body)
}

/// Décode un écran de bienvenue (aperçu hors ligne).
pub fn decode_welcome(body: &str) -> Option<crate::state::admin::community::WelcomeScreen> {
    admin::community::decode_welcome(body)
}

/// Décode une intégration (aperçu hors ligne).
pub fn decode_onboarding(body: &str) -> Option<crate::state::admin::community::Onboarding> {
    admin::community::decode_onboarding(body)
}

/// Décode la réponse d'une commande d'administration du serveur (aperçu hors ligne).
pub fn decode_admin(cmd: &crate::state::admin::guild::AdminCmd, body: &str) -> Option<crate::state::admin::guild::AdminEvent> {
    admin::guild::answer(cmd, body).ok()
}

/// Décode une réponse de Get Invite (aperçu hors ligne).
pub fn decode_invite(code: &str, body: &str) -> Option<crate::state::invites::Invite> {
    announce::invite_of(code, body)
}

/// Décode la réponse de Discord à une commande d'expressions (aperçu hors ligne, lot A6).
pub fn decode_expressions(cmd: &crate::state::admin::expressions::ExprCmd, body: &str) -> Option<crate::state::admin::expressions::ExprEvent> {
    admin::expressions::answer(cmd, body)
}

pub use crate::account::Token;
pub use crate::state::{Cmd, Event, PeopleHub};
use crate::state::members::{ListUpdate, ListWatch};
use crate::state::interest::{Interest, Verdict};
use crate::perf::wakes::{note, Wake as Noted};
/// Plafond local d'Identify par 24 h (valeur de production).
pub use gwlimit::IDENTIFY_PER_DAY;

use std::{
    sync::{mpsc, Arc},
    time::Duration,
};
use tokio::{runtime::Handle, sync::mpsc as tmpsc, task::JoinHandle};

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Heure murale (ms Unix) d'un instant à venir : les statuts et erreurs la
/// portent, l'interface l'affiche à sa façon (langue, 12/24 h).
fn wall_clock_ms(at: std::time::Instant) -> i64 {
    let wait = at.saturating_duration_since(std::time::Instant::now());
    chrono::Utc::now().timestamp_millis().saturating_add(i64::try_from(wait.as_millis()).unwrap_or(i64::MAX))
}
/// Appelé après chaque événement publié, pour que l'interface se redessine
/// seulement quand il y a du nouveau (0 % de CPU au repos).
pub type Wake = Arc<dyn Fn() + Send + Sync>;

thread_local! {
    /// Paquet en cours de décodage sur ce fil : ses événements attendent (voir [`Out::calm`]).
    static CALM: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Gardien de [`Out::calm`].
struct Calm;

impl Drop for Calm {
    fn drop(&mut self) {
        CALM.set(false);
    }
}

/// Relais des événements vers l'interface.
#[derive(Clone)]
struct Out {
    tx: mpsc::Sender<Event>,
    wake: Wake,
    /// Utilisateurs, membres et présences de la connexion, écrits sans
    /// passer par la file des événements (voir `state::people`).
    people: PeopleHub,
    /// Ce que l'interface montre de la liste des membres.
    watch: Arc<ListWatch>,
    /// Ce que l'interface montre du reste : décide des réveils (lot C8).
    interest: Arc<Interest>,
}

impl Out {
    fn send(&self, e: Event) {
        #[cfg(debug_assertions)]
        match &e {
            Event::Status(s) => eprintln!("[gateway] {s:?}"),
            Event::Fatal { problem, .. } => eprintln!("[gateway] fatal: {problem:?}"),
            Event::Ready { guilds, dms, .. } => eprintln!("[gateway] READY: {} serveurs, {} MP", guilds.len(), dms.len()),
            Event::Error(s) => eprintln!("[gateway] erreur: {s:?}"),
            _ => {}
        }
        match self.interest.verdict(&e) {
            Verdict::Drop => note(Noted::Dropped),
            Verdict::Defer => {
                let _ = self.tx.send(e);
                note(Noted::Deferred);
            }
            // Paquet qui attend en entier (CHANNEL_UPDATE d'un serveur qui n'est pas à l'écran).
            Verdict::Wake if CALM.get() && !self.interest.deferred() => {
                let _ = self.tx.send(e);
                note(Noted::Deferred);
            }
            Verdict::Wake => {
                let _ = self.tx.send(e);
                note(Noted::Now);
                (self.wake)();
            }
        }
    }

    /// Réponse à un geste de l'utilisateur : réveil dans tous les cas, même
    /// si le même événement venu de la passerelle aurait attendu.
    fn send_now(&self, e: Event) {
        let _ = self.tx.send(e);
        note(Noted::Now);
        (self.wake)();
    }

    /// Jusqu'à la fin de vie du gardien, les événements envoyés depuis ce fil
    /// qui auraient réveillé l'interface attendent la prochaine image
    /// (`Interest::calm_channel_update` a dit que le paquet ne change rien à l'écran).
    fn calm(&self) -> Calm {
        CALM.set(true);
        Calm
    }

    /// Réveil sans événement : seulement quand une donnée lue par
    /// l'interface sous le verrou de `people` a changé pour un utilisateur affiché.
    fn poke(&self) {
        note(Noted::Now);
        (self.wake)();
    }

    /// Mise à jour de la liste des membres : l'interface n'est réveillée que
    /// si elle montre les lignes touchées (`ListWatch`).
    fn send_list(&self, u: ListUpdate) {
        let wake = self.watch.wants(&u);
        let _ = self.tx.send(Event::MemberList(u));
        if wake {
            note(Noted::Now);
            (self.wake)();
        } else {
            note(Noted::Deferred);
        }
    }
}

/// Paquets de la passerelle rejoués sans réseau (banc `preview --bench`,
/// tests) : même décodage et mêmes décisions de réveil que la connexion.
pub struct Replay {
    out: Out,
    resume: Option<gateway::Resume>,
    me: Option<u64>,
}

impl Replay {
    /// `wake` : appelé pour chaque événement qui réveille ; `me` : mon
    /// identifiant. Les événements décodés arrivent dans le canal rendu.
    pub fn new(wake: Wake, interest: Arc<Interest>, me: Option<u64>) -> (Replay, mpsc::Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        (Replay { out: Out { tx, wake, people: PeopleHub::default(), watch: Arc::default(), interest }, resume: None, me }, rx)
    }

    /// Un paquet : son type (`MESSAGE_UPDATE`…) et son champ `d` en JSON.
    pub fn dispatch(&mut self, kind: &str, d: &str) {
        gateway::dispatch(kind, d, ResumeHosts::Discord, &mut self.resume, &mut self.me, &self.out);
    }
}

/// Adresses de Discord et délais de reconnexion.
///
/// L'application n'utilise que `Endpoints::default()` : aucune variable
/// d'environnement ni aucun fichier ne peut changer ces adresses, pour que
/// le jeton ne parte jamais ailleurs par accident. Seuls les tests en
/// construisent d'autres (faux serveur local).
#[derive(Clone, Debug)]
pub struct Endpoints {
    /// Passerelle WebSocket, sans le `/?v=10&encoding=json` final.
    pub gateway: String,
    /// Racine de l'API REST, sans `/` final.
    pub api: String,
    /// Délai avant une reconnexion ; doublé à chaque échec consécutif (×32 au plus).
    pub reconnect_base: Duration,
    /// Attente après une session invalidée (op 9), avant de se reconnecter.
    pub invalid_session_wait: Duration,
    /// Adresses de reprise (`resume_gateway_url`, reçue du réseau) acceptées.
    pub resume_hosts: ResumeHosts,
    /// Racine du CDN des images, sans `/` final (aucun jeton n'y est envoyé).
    pub cdn: String,
    /// Domaines d'images acceptés.
    pub cdn_hosts: cdn::CdnHosts,
    /// Demander la compression du transport (`compress=zlib-stream`). Après
    /// un flux illisible, les connexions suivantes s'en passent.
    pub compress: bool,
    /// Taille maximale d'un paquet de la passerelle, décompressé ou non.
    pub max_payload: usize,
    /// Écart minimal entre deux Identify.
    pub identify_interval: Duration,
    /// Identify par 24 h, au plus.
    pub identify_per_day: usize,
    /// Identité annoncée (Identify, User-Agent, en-têtes) ; par défaut : celle de Ferrocord.
    pub identity: identity::Config,
}

/// Le paquet READY d'un compte utilisateur peut peser plusieurs Mo ; au-delà
/// de cette taille, un paquet est refusé (et la décompression s'arrête avant
/// de l'avoir alloué).
pub const MAX_PAYLOAD: usize = 256 << 20;

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            gateway: "wss://gateway.discord.gg".into(),
            api: "https://discord.com/api/v10".into(),
            reconnect_base: Duration::from_secs(1),
            invalid_session_wait: Duration::from_secs(2),
            resume_hosts: ResumeHosts::Discord,
            cdn: "https://cdn.discordapp.com".into(),
            cdn_hosts: cdn::CdnHosts::Discord,
            compress: true,
            max_payload: MAX_PAYLOAD,
            identify_interval: gwlimit::IDENTIFY_INTERVAL,
            identify_per_day: gwlimit::IDENTIFY_PER_DAY,
            identity: identity::Config::default(),
        }
    }
}

/// Règle de validation de `resume_gateway_url`. C'est la seule donnée reçue
/// qui choisit où part le jeton : une adresse refusée est ignorée et la
/// reprise passe par `Endpoints::gateway`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResumeHosts {
    /// `wss://discord.gg` ou `wss://<sous-domaine>.discord.gg`, sans
    /// identifiants, port explicite, chemin, requête ni fragment.
    Discord,
    /// Tests seulement : `ws://127.0.0.1` (tout port, tout chemin).
    Loopback,
}

impl ResumeHosts {
    /// Adresse normalisée si elle est acceptée. On se connecte ensuite à
    /// cette forme normalisée, pour que l'hôte vérifié soit exactement celui
    /// contacté (deux analyseurs d'URL pourraient diverger sur une adresse piégée).
    pub fn check(self, url: &str) -> Option<String> {
        if !url.is_ascii() {
            return None;
        }
        let u = reqwest::Url::parse(url).ok()?;
        if !u.username().is_empty() || u.password().is_some() || u.query().is_some() || u.fragment().is_some() {
            return None;
        }
        let ok = match self {
            ResumeHosts::Discord => {
                u.scheme() == "wss"
                    && u.port().is_none()
                    && matches!(u.path(), "" | "/")
                    && u.domain().is_some_and(|h| h == "discord.gg" || h.ends_with(".discord.gg"))
            }
            ResumeHosts::Loopback => u.scheme() == "ws" && u.host_str() == Some("127.0.0.1"),
        };
        ok.then(|| u.as_str().to_string())
    }
}

pub struct Connection {
    pub cmd: tmpsc::UnboundedSender<Cmd>,
    pub events: mpsc::Receiver<Event>,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for Connection {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

/// Lance la passerelle et la tâche de commandes sur le runtime `rt`.
/// Le jeton ne quitte son enveloppe qu'ici, pour les deux tâches qui l'envoient.
pub fn connect(rt: &Handle, token: Token, endpoints: Endpoints, wake: Wake) -> Connection {
    connect_watched(rt, token, endpoints, wake, Arc::default())
}

/// Comme [`connect`], avec ce que l'interface montre de la liste des membres
/// (les mises à jour hors de la vue ne la réveillent pas).
pub fn connect_watched(rt: &Handle, token: Token, endpoints: Endpoints, wake: Wake, watch: Arc<ListWatch>) -> Connection {
    connect_filtered(rt, token, endpoints, wake, watch, Arc::default())
}

/// Comme [`connect_watched`], avec ce que l'interface montre du reste (salon
/// ouvert, salons calmes) : les événements sans effet à l'écran ne la
/// réveillent pas ([`Interest`]).
pub fn connect_filtered(rt: &Handle, token: Token, endpoints: Endpoints, wake: Wake, watch: Arc<ListWatch>, interest: Arc<Interest>) -> Connection {
    let token = token.expose().to_string();
    let (tx, events) = mpsc::channel();
    let (cmd, mut cmd_rx) = tmpsc::unbounded_channel();
    // Les souscriptions vont à la passerelle, le reste à la tâche REST.
    let (sub_tx, sub_rx) = tmpsc::unbounded_channel();
    let (rest_tx, rest_rx) = tmpsc::unbounded_channel();
    // Op 4 du vocal : à la passerelle (phase 4).
    let (voice_tx, voice_rx) = tmpsc::unbounded_channel();
    // Caméra et directs : à la passerelle aussi (phase 5).
    let (video_tx, video_rx) = tmpsc::unbounded_channel();
    // Ma présence (op 3) à la passerelle, sondage des jeux à sa tâche (lot C3).
    let (presence_tx, presence_rx) = tmpsc::unbounded_channel();
    let (scan_tx, scan_rx) = tokio::sync::watch::channel(crate::state::activity::ScanConfig::default());
    // Tube local de la Rich Presence (lot R) : fermé tant que le magasin ne l'ouvre pas.
    let (rpc_tx, rpc_rx) = tokio::sync::watch::channel(None::<String>);
    let bot = token.starts_with("Bot ");
    // Une identité par connexion, la même pour la passerelle et l'API.
    let who = Arc::new(identity::Identity::new(&endpoints.identity, bot));
    let ws_headers = if who.is_desktop() { endpoints.identity.ws_headers() } else { Vec::new() };
    let subs = gateway::Subs::new(sub_rx, &token).with_voice(voice_rx).with_video(video_rx).with_presence(presence_rx).with_identity(who.clone(), ws_headers);
    let out = Out { tx, wake, people: PeopleHub::default(), watch, interest };
    // Premier événement : le magasin adopte les tables de cette connexion.
    out.send(Event::PeopleHub(out.people.clone()));
    let http = crate::platform::http_builder()
        .user_agent(who.user_agent())
        // Une variable HTTP(S)_PROXY ne doit pas détourner les requêtes qui
        // portent le jeton. Les proxys ne sont pas pris en charge pour l'instant.
        .no_proxy()
        .build()
        // Échoue seulement si la pile TLS ne s'initialise pas : rien à voir avec les données reçues.
        .expect("client HTTP");
    // Numéros de version du client de bureau : au plus une demande par jour, pour le prochain lancement.
    let refresh = endpoints.identity.refresh.clone().filter(|_| who.is_desktop()).map(|r| identity::refresh(r, endpoints.identity.clone()));
    let scan_out = out.clone();
    let rpc_out = out.clone();
    let mut tasks = vec![
        rt.spawn(gateway::gateway_task(token.clone(), endpoints.clone(), out.clone(), subs)),
        rt.spawn(rest::command_task(http, token, endpoints.api, rest_rx, out, who.clone())),
        rt.spawn(async move {
            while let Some(c) = cmd_rx.recv().await {
                match c {
                    Cmd::Subscribe(s) => {
                        let _ = sub_tx.send(s);
                    }
                    Cmd::Voice(v) if voicegw::for_gateway(&v) => {
                        let _ = voice_tx.send(v);
                    }
                    Cmd::Video(v) if streamgw::for_gateway(&v) => {
                        let _ = video_tx.send(v);
                    }
                    Cmd::Activity(crate::state::activity::ActivityCmd::Presence(p)) => {
                        let _ = presence_tx.send(p);
                    }
                    Cmd::Activity(crate::state::activity::ActivityCmd::Scan(c)) => {
                        let _ = scan_tx.send(c);
                    }
                    Cmd::Activity(crate::state::activity::ActivityCmd::Rpc(p)) => {
                        let _ = rpc_tx.send(p);
                    }
                    other => {
                        let _ = rest_tx.send(other);
                    }
                }
            }
        }),
    ];
    if !bot {
        tasks.push(rt.spawn(games::scan_task(scan_rx, scan_out.clone(), crate::state::activity::SCAN_EVERY)));
        tasks.push(rt.spawn(idle::idle_task(scan_out)));
        let sink: crate::rpc::Sink = Arc::new(move |e| rpc_out.send(Event::Activity(crate::state::activity::ActivityEvent::Rpc(e))));
        tasks.push(rt.spawn(crate::rpc::serve(rpc_rx, sink)));
    }
    if let Some(r) = refresh {
        tasks.push(rt.spawn(r));
    }
    Connection { cmd, events, tasks }
}

/// Décode un paquet de passerelle (`t`, `d`) en événements, sans connexion
/// ni jeton : sert à l'aperçu hors ligne (`examples/preview.rs`). Chaque
/// appel est indépendant (voir [`Decoder`] pour une suite de paquets).
pub fn decode_dispatch(kind: &str, d: &str) -> Vec<Event> {
    Decoder::default().decode(kind, d)
}

/// Décodeur hors ligne d'une suite de paquets : garde, comme une session,
/// mon identifiant lu dans READY (il sert à trouver mon membre ensuite).
#[derive(Default)]
pub struct Decoder {
    me: Option<u64>,
    people: PeopleHub,
}

impl Decoder {
    /// Utilisateurs, membres et présences sont écrits dans `people` (à remettre
    /// au magasin par `Event::PeopleHub`).
    pub fn with_people(people: PeopleHub) -> Self {
        Decoder { me: None, people }
    }

    pub fn people(&self) -> PeopleHub {
        self.people.clone()
    }

    pub fn decode(&mut self, kind: &str, d: &str) -> Vec<Event> {
        let (tx, rx) = mpsc::channel();
        let out = Out { tx, wake: Arc::new(|| {}), people: self.people.clone(), watch: Arc::default(), interest: Arc::default() };
        gateway::dispatch(kind, d, ResumeHosts::Discord, &mut None, &mut self.me, &out);
        drop(out);
        rx.try_iter().collect()
    }
}

/// Décode une réponse de `GET /users/{id}/profile` dans `hub`, sans
/// connexion (aperçu hors ligne) ; `false` si elle est illisible.
pub fn decode_profile(hub: &PeopleHub, user_id: u64, guild_id: Option<u64>, body: &str) -> bool {
    profile::store(hub, user_id, guild_id, body)
}

/// Décode une réponse de `GET /users/@me/notes/{id}` dans `hub` (aperçu).
pub fn decode_note(hub: &PeopleHub, user_id: u64, body: &str) {
    hub.lock().profiles_mut().note_loaded(user_id, profile::note_of(body));
}

/// Décode une réponse de recherche de messages (aperçu hors ligne) en
/// l'événement que publierait le réseau pour la demande `seq`.
pub fn decode_search(body: &str, seq: u64) -> Option<Event> {
    search::decode(body, seq)
}

/// Décode une réponse de `GET /users/@me/mentions` (aperçu hors ligne).
pub fn decode_mentions(body: &str) -> Option<Vec<crate::state::model::Message>> {
    inbox::decode_messages(body)
}

/// Décode une page d'historique REST (du plus récent au plus ancien, comme
/// l'envoie Discord) dans l'ordre d'affichage.
pub fn decode_history(body: &str) -> Option<Vec<crate::state::model::Message>> {
    let raw: Vec<raw::RawMessage> = serde_json::from_str(body).ok()?;
    Some(raw.into_iter().rev().map(raw::RawMessage::into_message).collect())
}

/// Décode une réponse de `GET …/application-command-index` (aperçu hors ligne).
pub fn decode_command_index(body: &str) -> Option<crate::state::interactions::Index> {
    interactions::index_of(body)
}

#[cfg(test)]
mod tests {
    use super::ResumeHosts::{self, *};

    fn ok(rule: ResumeHosts, url: &str) -> bool {
        rule.check(url).is_some()
    }

    #[test]
    fn discord_rule_accepts_discord_gateways() {
        for url in [
            "wss://discord.gg",
            "wss://gateway.discord.gg",
            "wss://gateway.discord.gg/",
            "wss://gateway-us-east1-b.discord.gg",
            "wss://GATEWAY.Discord.GG",
            "wss://gateway.discord.gg:443",
        ] {
            assert!(ok(Discord, url), "{url} devrait être acceptée");
        }
        // Forme normalisée : hôte en minuscules, port par défaut retiré.
        assert_eq!(Discord.check("wss://GATEWAY.Discord.GG:443").as_deref(), Some("wss://gateway.discord.gg/"));
    }

    #[test]
    fn discord_rule_rejects_lookalikes() {
        for url in [
            "",
            "pas une adresse",
            "wss://discord.gg.evil.tld",
            "wss://evil.tld/.discord.gg",
            "wss://evil.tld?.discord.gg",
            "wss://evil.tld#.discord.gg",
            r"wss://evil.tld\.discord.gg",
            "wss://user@evil.tld",
            "wss://gateway.discord.gg@evil.tld",
            "wss://user@gateway.discord.gg",
            "wss://user:mdp@gateway.discord.gg",
            "wss://evildiscord.gg",
            "wss://gateway.discord.gg.",
            "wss://gateway.discord.gg:8443",
            "wss://gateway.discord.gg/chemin",
            "wss://gateway.discord.gg/?v=10",
            "ws://gateway.discord.gg",
            "https://gateway.discord.gg",
            "wss://162.159.135.233",
            "wss://127.0.0.1",
            "wss://évil.discord.gg",
            "wss://discord.gg%2eevil.tld",
        ] {
            assert!(!ok(Discord, url), "{url} devrait être refusée");
        }
    }

    #[test]
    fn loopback_rule_is_loopback_only() {
        assert!(ok(Loopback, "ws://127.0.0.1:1234/resume"));
        for url in ["ws://localhost:1234", "ws://127.0.0.2:1", "wss://gateway.discord.gg", "ws://u@127.0.0.1:1", "ws://127.0.0.1.evil.tld"] {
            assert!(!ok(Loopback, url), "{url} devrait être refusée");
        }
    }
}
