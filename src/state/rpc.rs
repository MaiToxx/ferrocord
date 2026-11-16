//! Rich Presence locale (lot R), côté magasin : applications branchées sur
//! le tube local (`crate::rpc`), leur activité nettoyée, et ce qui part dans
//! l'op 3 ou s'affiche dans mon profil.
//!
//! Le serveur du tube ne garde rien : il signale chaque branchement,
//! chaque `SET_ACTIVITY` accepté et chaque départ ; le magasin tient la
//! liste et choisit l'activité montrée (la dernière mise à jour).

use super::people::{Activity, ActivityImage, ActivityKind, Rich};

/// Applications branchées gardées (le serveur en refuse au-delà).
pub const MAX_CLIENTS: usize = 8;
/// Longueur gardée d'un texte d'activité, en caractères (documentation du
/// SDK : 128 au plus pour `details`, `state` et les textes des images).
pub const MAX_TEXT: usize = 128;
/// Libellé d'un bouton, en caractères (documentation : 32).
pub const MAX_LABEL: usize = 32;
/// Adresse d'un bouton, en octets (documentation : 512).
pub const MAX_URL: usize = 512;

/// Bouton d'une activité : libellé montré à tous, adresse ouverte au clic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RpcButton {
    pub label: String,
    pub url: String,
}

/// Activité reçue d'une application locale, déjà nettoyée par `crate::rpc`
/// (champs inconnus retirés, longueurs bornées, secrets de groupe ignorés).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RpcActivity {
    /// Nom à montrer : celui de la liste des jeux de Discord si l'application
    /// y est, sinon celui que l'application donne, sinon son programme.
    pub name: String,
    /// Type d'activité : 0 joue, 2 écoute, 3 regarde, 5 participe.
    pub kind: u8,
    pub details: Option<String>,
    pub state: Option<String>,
    /// Début et fin (ms Unix).
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    /// Clés d'images : identifiant numérique d'image de l'application, image
    /// du proxy de Discord (`mp:`), ou nom d'une image de l'application. Un
    /// nom ne part dans l'op 3 qu'une fois traduit en identifiant par la
    /// liste des images de l'application ([`RpcState::assets`]).
    pub large_image: Option<String>,
    pub large_text: Option<String>,
    pub small_image: Option<String>,
    pub small_text: Option<String>,
    pub party_id: Option<String>,
    /// Taille du groupe : (actuelle, maximale).
    pub party_size: Option<(u32, u32)>,
    pub buttons: Vec<RpcButton>,
}

/// Application branchée sur le tube.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RpcClient {
    /// Numéro de la connexion, propre à ce serveur.
    pub conn: u64,
    /// Application Discord déclarée à la poignée de main (`client_id`).
    pub app_id: u64,
    /// Nom du programme (d'après le `pid` donné par l'application).
    pub process: Option<String>,
    pub activity: Option<RpcActivity>,
    /// Ordre des mises à jour : l'activité montrée est la plus récente.
    seq: u64,
    /// Heure du branchement (ms Unix).
    pub since_ms: i64,
}

/// Serveur → magasin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RpcEvent {
    /// Tube ouvert (`discord-ipc-{n}`), ou aucun : tous pris, ou serveur arrêté.
    Listening(Option<u8>),
    Connected { conn: u64, app_id: u64, since_ms: i64 },
    /// `SET_ACTIVITY` accepté ; `None` : activité effacée.
    Activity { conn: u64, process: Option<String>, activity: Option<Box<RpcActivity>> },
    Disconnected { conn: u64 },
}

/// État de la Rich Presence dans le magasin.
#[derive(Debug, Default)]
pub struct RpcState {
    /// Réglage de cet ordinateur (section `[rich_presence]` de `local.toml`),
    /// recopié par l'interface.
    pub enabled: bool,
    /// Remplace le préfixe des tubes (tests : jamais les vrais tubes).
    pub pipe_prefix: Option<String>,
    /// Tube ouvert, s'il y en a un ; `listening_known` : réponse reçue.
    pub listening: Option<u8>,
    pub listening_known: bool,
    pub clients: Vec<RpcClient>,
    next_seq: u64,
    /// Images des applications, par nom (demandées par « Charger les
    /// images ») : les plus anciennes sortent au-delà de [`MAX_ASSET_APPS`].
    pub assets: Vec<(u64, AssetTable)>,
}

/// Applications dont la liste d'images est gardée.
pub const MAX_ASSET_APPS: usize = 8;
/// Images gardées par application.
pub const MAX_ASSETS: usize = 300;

/// Liste des images d'une application : (nom en minuscules, identifiant).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetTable {
    Loading,
    Ready(Vec<(String, String)>),
    Failed,
}

impl RpcState {
    /// Nouvelle connexion : le serveur d'avant est arrêté, ses applications aussi.
    pub fn session_reset(&mut self) {
        self.clients.clear();
        self.listening = None;
        self.listening_known = false;
    }

    /// Applique un événement ; `name_of` donne le nom d'une application
    /// d'après la liste des jeux de Discord.
    pub fn apply(&mut self, e: RpcEvent, name_of: impl Fn(u64) -> Option<String>) {
        match e {
            RpcEvent::Listening(n) => {
                self.listening = n;
                self.listening_known = true;
                if n.is_none() {
                    self.clients.clear();
                }
            }
            RpcEvent::Connected { conn, app_id, since_ms } => {
                if self.clients.len() < MAX_CLIENTS && !self.clients.iter().any(|c| c.conn == conn) {
                    self.clients.push(RpcClient { conn, app_id, process: None, activity: None, seq: 0, since_ms });
                }
            }
            RpcEvent::Activity { conn, process, activity } => {
                self.next_seq += 1;
                let seq = self.next_seq;
                let Some(c) = self.clients.iter_mut().find(|c| c.conn == conn) else { return };
                if process.is_some() {
                    c.process = process;
                }
                c.activity = activity.map(|a| {
                    let mut a = *a;
                    a.name = resolve_name(name_of(c.app_id), &a.name, c.process.as_deref(), c.app_id);
                    a
                });
                c.seq = seq;
            }
            RpcEvent::Disconnected { conn } => self.clients.retain(|c| c.conn != conn),
        }
    }

    /// Liste des jeux reçue : les noms sont repris d'elle.
    pub fn rename(&mut self, name_of: impl Fn(u64) -> Option<String>) {
        for c in &mut self.clients {
            if let (Some(a), Some(n)) = (c.activity.as_mut(), name_of(c.app_id)) {
                a.name = n;
            }
        }
    }

    /// Liste des images de `app_id`, si elle a été demandée.
    pub fn asset_table(&self, app_id: u64) -> Option<&AssetTable> {
        self.assets.iter().find(|(id, _)| *id == app_id).map(|(_, t)| t)
    }

    /// « Charger les images » : vrai s'il faut demander la liste (ni reçue,
    /// ni en cours).
    pub fn ask_assets(&mut self, app_id: u64) -> bool {
        if matches!(self.asset_table(app_id), Some(AssetTable::Loading | AssetTable::Ready(_))) {
            return false;
        }
        self.assets.retain(|(id, _)| *id != app_id);
        if self.assets.len() >= MAX_ASSET_APPS {
            self.assets.remove(0);
        }
        self.assets.push((app_id, AssetTable::Loading));
        true
    }

    /// Réponse à [`RpcState::ask_assets`] ; `None` : échec.
    pub fn set_assets(&mut self, app_id: u64, list: Option<Vec<(String, String)>>) {
        if let Some((_, t)) = self.assets.iter_mut().find(|(id, _)| *id == app_id) {
            *t = match list {
                Some(mut l) => {
                    l.retain(|(name, id)| valid_asset_name(name) && valid_asset_id(id));
                    l.truncate(MAX_ASSETS);
                    AssetTable::Ready(l.into_iter().map(|(n, id)| (n.to_lowercase(), id)).collect())
                }
                None => AssetTable::Failed,
            };
        }
    }

    /// Clé transmissible : un identifiant ou une image du proxy tels quels,
    /// un nom traduit par la liste des images ; sinon rien.
    fn resolve(&self, app_id: u64, key: &Option<String>) -> Option<String> {
        let k = key.as_deref()?;
        if !valid_asset_name(k) {
            return Some(k.to_string());
        }
        let Some(AssetTable::Ready(list)) = self.asset_table(app_id) else { return None };
        let low = k.to_lowercase();
        list.iter().find(|(n, _)| *n == low).map(|(_, id)| id.clone())
    }

    /// Une image de cette activité attend-elle la liste des images ?
    pub fn unresolved(&self, app_id: u64, a: &RpcActivity) -> bool {
        [&a.large_image, &a.small_image].into_iter().any(|k| k.is_some() && self.resolve(app_id, k).is_none())
    }

    /// Activité montrée, prête pour l'op 3 et mon profil : images nommées
    /// traduites, ou retirées tant que la liste manque.
    pub fn current_out(&self) -> Option<RichOut> {
        let (app_id, a) = self.current()?;
        let mut activity = a.clone();
        activity.large_image = self.resolve(app_id, &a.large_image);
        activity.small_image = self.resolve(app_id, &a.small_image);
        Some(RichOut { app_id, activity })
    }

    /// Activité montrée : la plus récemment mise à jour.
    pub fn current(&self) -> Option<(u64, &RpcActivity)> {
        self.clients.iter().filter_map(|c| Some((c.seq, c.app_id, c.activity.as_ref()?))).max_by_key(|(s, _, _)| *s).map(|(_, id, a)| (id, a))
    }

    /// Nom d'une application branchée, pour la liste.
    pub fn label(&self, c: &RpcClient, name_of: impl Fn(u64) -> Option<String>) -> String {
        c.activity.as_ref().map(|a| a.name.clone()).or_else(|| name_of(c.app_id)).or_else(|| c.process.clone()).unwrap_or_else(|| c.app_id.to_string())
    }
}

fn resolve_name(listed: Option<String>, given: &str, process: Option<&str>, app_id: u64) -> String {
    let given = given.trim();
    listed.or_else(|| (!given.is_empty()).then(|| given.to_string())).or_else(|| process.map(str::to_string)).unwrap_or_else(|| app_id.to_string())
}

/// Activité Rich Presence prête à partir dans l'op 3.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RichOut {
    pub app_id: u64,
    pub activity: RpcActivity,
}

/// Adresse d'affichage d'une image (mêmes règles que `net::people` : CDN
/// de Discord seulement).
fn asset_url(app_id: u64, key: &str) -> Option<String> {
    if let Some(path) = key.strip_prefix("mp:") {
        return valid_mp(path).then(|| format!("https://media.discordapp.net/{path}"));
    }
    valid_asset_id(key).then(|| format!("https://cdn.discordapp.com/app-assets/{app_id}/{key}.png"))
}

/// Identifiant numérique d'une image de l'application.
pub fn valid_asset_id(key: &str) -> bool {
    !key.is_empty() && key.len() <= 20 && key.bytes().all(|b| b.is_ascii_digit())
}

/// Nom d'une image de l'application (clé donnée dans le portail des
/// développeurs) : lettres, chiffres, `_`, `-` et `.`, pas un identifiant.
pub fn valid_asset_name(key: &str) -> bool {
    !key.is_empty() && key.len() <= 64 && !valid_asset_id(key) && key.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
}

/// Chemin d'une image du proxy de Discord (après `mp:`).
pub fn valid_mp(path: &str) -> bool {
    !path.is_empty() && path.len() <= 300 && !path.starts_with('/') && !path.contains("..") && path.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~/%:".contains(&b))
}

impl RichOut {
    /// Activité telle que mon profil la montre (comme les autres la voient).
    pub fn to_activity(&self) -> Activity {
        let a = &self.activity;
        let image = |key: &Option<String>, text: &Option<String>| {
            let url = key.as_deref().and_then(|k| asset_url(self.app_id, k));
            (url.is_some() || text.is_some()).then(|| ActivityImage { url, text: text.clone() })
        };
        let rich = Rich {
            app_id: Some(self.app_id),
            end_ms: a.end_ms,
            large: image(&a.large_image, &a.large_text),
            small: image(&a.small_image, &a.small_text),
            // Mes propres boutons : pas de session à interroger, libellés seuls.
            buttons: a.buttons.iter().map(|b| b.label.clone()).collect(),
            party: a.party_size.filter(|(now, max)| *now > 0 && now <= max),
            ..Default::default()
        };
        Activity {
            kind: ActivityKind::from_code(u64::from(a.kind)).unwrap_or(ActivityKind::Playing),
            name: a.name.clone(),
            details: a.details.clone(),
            state: a.state.clone(),
            emoji: None,
            start_ms: a.start_ms,
            rich: Some(Box::new(rich)),
        }
    }
}

/// Section de `local.toml` (rien n'en part chez Discord).
pub const SECTION: &str = "rich_presence";
const K_ON: &str = "actif";

pub fn rpc_toml(enabled: bool) -> String {
    format!("[{SECTION}]\n# Tube local des jeux et logiciels (discord-ipc) ; seule l'activité est acceptée.\n{K_ON} = {enabled}\n")
}

/// Lit la section ; valeur absente ou illisible : `default`.
pub fn rpc_from_table(t: &toml::de::DeTable<'_>, default: bool) -> bool {
    t.iter().find(|(k, _)| k.get_ref().as_ref() == K_ON).and_then(|(_, v)| v.get_ref().as_bool()).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn act(name: &str) -> RpcActivity {
        RpcActivity { name: name.into(), details: Some("Niveau 3".into()), ..Default::default() }
    }

    #[test]
    fn latest_update_is_shown_and_names_resolve() {
        let mut s = RpcState::default();
        let listed = |id: u64| (id == 7).then(|| "Jeu Connu".to_string());
        s.apply(RpcEvent::Connected { conn: 1, app_id: 7, since_ms: 0 }, listed);
        s.apply(RpcEvent::Connected { conn: 2, app_id: 8, since_ms: 0 }, listed);
        assert_eq!(s.current(), None);
        s.apply(RpcEvent::Activity { conn: 1, process: Some("jeu".into()), activity: Some(Box::new(act("Nom donné"))) }, listed);
        assert_eq!(s.current().map(|(id, a)| (id, a.name.as_str())), Some((7, "Jeu Connu")), "liste de Discord d'abord");
        s.apply(RpcEvent::Activity { conn: 2, process: Some("outil".into()), activity: Some(Box::new(act(""))) }, listed);
        assert_eq!(s.current().map(|(id, a)| (id, a.name.as_str())), Some((8, "outil")), "la plus récente ; nom du programme");
        s.apply(RpcEvent::Activity { conn: 2, process: None, activity: None }, listed);
        assert_eq!(s.current().map(|(id, _)| id), Some(7), "effacée : l'autre reprend");
        assert_eq!(s.clients[1].process.as_deref(), Some("outil"), "programme gardé");
        s.apply(RpcEvent::Disconnected { conn: 1 }, listed);
        assert_eq!(s.current(), None);
        s.apply(RpcEvent::Activity { conn: 9, process: None, activity: Some(Box::new(act("x"))) }, listed);
        assert_eq!(s.clients.len(), 1, "connexion inconnue : ignorée");
        s.apply(RpcEvent::Listening(None), listed);
        assert!(s.clients.is_empty() && s.listening_known);
    }

    #[test]
    fn named_images_wait_for_the_asset_list() {
        let mut s = RpcState::default();
        s.apply(RpcEvent::Connected { conn: 1, app_id: 7, since_ms: 0 }, |_| None);
        let a = RpcActivity { name: "Jeu".into(), large_image: Some("Logo".into()), small_image: Some("123".into()), ..Default::default() };
        s.apply(RpcEvent::Activity { conn: 1, process: None, activity: Some(Box::new(a.clone())) }, |_| None);
        assert!(s.unresolved(7, &a));
        let out = s.current_out().unwrap_or_else(|| panic!("activité"));
        assert_eq!((out.activity.large_image.as_deref(), out.activity.small_image.as_deref()), (None, Some("123")), "nom inconnu : rien ne part");
        // Demandée une fois ; la réponse traduit le nom, sans tenir compte de la casse.
        assert!(s.ask_assets(7) && !s.ask_assets(7));
        assert_eq!(s.asset_table(7), Some(&AssetTable::Loading));
        s.set_assets(7, Some(vec![("logo".into(), "1131721726514954381".into()), ("mauvais nom".into(), "1".into()), ("x".into(), "pas-un-id".into())]));
        assert_eq!(s.asset_table(7), Some(&AssetTable::Ready(vec![("logo".into(), "1131721726514954381".into())])));
        assert!(!s.unresolved(7, &a));
        assert_eq!(s.current_out().and_then(|o| o.activity.large_image).as_deref(), Some("1131721726514954381"));
        // Échec : redemandable ; liste jamais demandée : réponse ignorée.
        s.set_assets(7, None);
        assert!(s.ask_assets(7));
        s.set_assets(9, Some(Vec::new()));
        assert_eq!(s.asset_table(9), None);
        for app in 10..30 {
            s.ask_assets(app);
        }
        assert_eq!(s.assets.len(), MAX_ASSET_APPS);
        assert!(valid_asset_name("logo_2-b.x") && !valid_asset_name("123") && !valid_asset_name("mp:a") && !valid_asset_name("https://x") && !valid_asset_name(""));
    }

    #[test]
    fn clients_are_bounded() {
        let mut s = RpcState::default();
        for conn in 0..20 {
            s.apply(RpcEvent::Connected { conn, app_id: 1, since_ms: 0 }, |_| None);
        }
        assert_eq!(s.clients.len(), MAX_CLIENTS);
    }

    #[test]
    fn own_profile_activity() {
        let a = RpcActivity {
            name: "Jeu".into(),
            kind: 2,
            large_image: Some("123".into()),
            large_text: Some("Carte".into()),
            small_image: Some("mp:external/abc/x.png".into()),
            buttons: vec![RpcButton { label: "Site".into(), url: "https://exemple.invalid".into() }],
            ..Default::default()
        };
        let v = RichOut { app_id: 9, activity: a }.to_activity();
        assert_eq!(v.kind, ActivityKind::Listening);
        let r = v.rich().unwrap_or_else(|| panic!("enrichie"));
        assert_eq!(r.large.as_ref().and_then(|i| i.url.as_deref()), Some("https://cdn.discordapp.com/app-assets/9/123.png"));
        assert_eq!(r.small.as_ref().and_then(|i| i.url.as_deref()), Some("https://media.discordapp.net/external/abc/x.png"));
        assert_eq!(r.buttons, ["Site"]);
        assert_eq!(r.session_id, None, "aucune requête pour mes propres boutons");
    }

    #[test]
    fn section_round_trip() {
        for on in [true, false] {
            let text = rpc_toml(on);
            let root = toml::de::DeTable::parse(&text).unwrap_or_else(|e| panic!("{e:?}"));
            let t = root.get_ref().iter().find(|(k, _)| k.get_ref().as_ref() == SECTION).and_then(|(_, v)| v.get_ref().as_table().cloned());
            assert_eq!(t.map(|t| rpc_from_table(&t, !on)), Some(on));
        }
        let odd = toml::de::DeTable::parse("actif = \"oui\"").unwrap_or_else(|e| panic!("{e:?}"));
        assert!(rpc_from_table(odd.get_ref(), true));
    }
}
