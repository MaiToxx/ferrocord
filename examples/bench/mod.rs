//! Banc de mesure des budgets du chapitre 1.3 du plan (`preview --bench`).
//!
//! La vraie interface (`ui::App`, rendu glow d'eframe) dessine un magasin
//! synthétique : par défaut 100 serveurs × 50 salons, quatre salons de 500
//! messages variés (courts, longs, sur plusieurs lignes, liens, embeds,
//! pièces jointes), et un salon de 2 000 messages dont seule la dernière page
//! est en mémoire au départ : la connexion simulée sert les autres page par
//! page (`before`, `after`), comme Discord. Le banc enchaîne des phases de
//! plusieurs centaines d'images chacune et produit un rapport texte et JSON.
//!
//! Ce qui est mesuré, par image :
//! - « mise en page » : `App::logic` + `App::ui` (lecture des événements,
//!   construction de toute l'interface par egui, code de l'application) ;
//! - « image » : le temps qu'eframe mesure lui-même pour l'image
//!   (`Frame::info().cpu_usage`) : entrées, mise en page, tessellation, envoi
//!   des commandes OpenGL et des textures. Il exclut l'attente de la
//!   synchronisation verticale (`swap_buffers`).
//!
//! Ce qui ne l'est pas : le travail fait ensuite par le pilote graphique et
//! le GPU (après l'envoi des commandes, rien n'attend leur fin), le temps du
//! compositeur de Windows, et les images capturées (lecture de l'écran,
//! exclues des statistiques).
//!
//! Les verdicts « < 4 ms » portent sur le 95e centile des images de la phase
//! (le maximum est affiché à côté) ; « 0 redessin au repos » sur le nombre
//! strict d'images pendant l'observation, après une seconde de stabilisation
//! dont les images sont comptées et expliquées à part.
//! Pendant cette observation, 1 000 présences par seconde d'utilisateurs non
//! affichés sont écrites comme le fait la passerelle : aucune ne doit
//! provoquer de redessin.
//!
//! Dernière phase, « trafic » (lot « perf ») : ce que seul un vrai compte
//! montrait. Des paquets de la passerelle en JSON passent par le vrai
//! décodage et le vrai filtre des réveils (`net::Replay`) : une rafale de
//! démarrage, puis `--bench-quiet` secondes de repos avec trois paquets par
//! seconde (le rythme mesuré le 15 novembre 2026) dans des salons qui ne sont
//! pas à l'écran (MESSAGE_UPDATE, MESSAGE_DELETE, CHANNEL_UPDATE,
//! MESSAGE_CREATE d'un salon déjà non lu) : verdict « 0 image au repos sous
//! ce trafic ». Enfin cinq paquets qui changent l'écran (message affiché
//! modifié, supprimé, nouveau message, nouvelle pastille, salon renommé) :
//! chacun doit réveiller l'interface.

pub mod alloc;
pub mod compte;
pub mod cpu;
pub mod long;

use super::{Generated, Offline, View};
use eframe::egui;
use ferrocord::images::{self, Images};
use ferrocord::state::model::{Attachment, Avatar, Embed, EmbedKind, EmbedMedia, Channel, ChannelKind, Guild, ImageHash, Message, User, DISCORD_EPOCH};
use ferrocord::state::people::{ClientStatus, Presence, Status};
use ferrocord::state::{Event, PeopleHub, Store, HISTORY_PAGE};
use ferrocord::theme::{Preset, ThemeLink, WatchConfig};
use ferrocord::account::{AccountLink, MemoryVault};
use ferrocord::settings::SettingsLink;
use ferrocord::ui::{App, Boot, Wake};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

pub struct BenchCfg {
    pub guilds: usize,
    pub channels: usize,
    pub messages: usize,
    /// Messages du salon servi page par page (phase « historique »).
    pub deep: usize,
    /// Images par phase.
    pub frames: usize,
    /// Durée de l'observation au repos (s).
    pub quiet: u64,
    pub out: Option<PathBuf>,
    /// Nom de la série dans le rapport (par exemple « avant » ou « après »).
    pub label: String,
    /// Sans la borne de l'atlas des polices (`--bench-atlas-libre`, lot
    /// « long ») : même exécutable, pour isoler son effet sur le temps d'image.
    pub free_atlas: bool,
}

impl Default for BenchCfg {
    fn default() -> Self {
        BenchCfg { guilds: 100, channels: 50, messages: 500, deep: 2000, frames: 300, quiet: 10, out: None, label: String::from("banc"), free_atlas: false }
    }
}

const GUILD_BASE: u64 = 6_000_000_000_000_000_000;
const CHANNEL_BASE: u64 = 6_100_000_000_000_000_000;
const ME: u64 = 1_000_000_000_000_000_001;
const AUTHORS: u64 = 40;
/// Octets vivants du tas tolérés 3 s après le démarrage du compte simulé
/// (`--bench-mem demarrage` : READY de 100 serveurs, 10 000 jeux détectables,
/// un salon affiché). Mesuré : 11,1 Mo après le lot « ram », 20,3 Mo avant.
/// Au-dessus, le relevé se termine en échec (code de sortie 1).
const START_HEAP_MAX: usize = 16 << 20;
/// Octets vivants du tas tolérés au repos après le chargement du magasin du banc
/// (lot « ram » : magasin, 2 500 messages, polices, images de l'écran). Mesuré : 10,9 Mo. Le seuil est large (un cache de plus ne le
/// franchit pas), mais une structure gardée en double ou une liste décodée
/// sans compactage le franchit.
const REST_HEAP_MAX: usize = 16 << 20;
/// Salons remplis de messages (le premier est ouvert).
const FILLED: usize = 4;
/// Débit de la phase « arrivée ».
const LIVE_RATE: f64 = 50.0;
/// Défilement injecté par image (points).
const WHEEL: f32 = 60.0;
/// Défilement par image de la phase « historique » : la vue atteint le bord
/// en moins d'une seconde après chaque page.
const WHEEL_FAST: f32 = 400.0;
/// Paires d'images comparées après un changement de salon.
const CHECKS: usize = 8;
/// Images laissées aux animations (barre de défilement, survol) avant une capture.
const SETTLE_FRAMES: usize = 90;
/// Images passées dans chaque salon, pointeur parti, avant les captures de la vérification.
const CHECK_SETTLE: usize = 60;
/// Durée maximale de chaque sens de la phase « historique ».
const HISTORY_CAP: Duration = Duration::from_secs(150);
/// Stabilisation avant l'observation au repos.
const QUIET_SETTLE: Duration = Duration::from_secs(1);
/// Au repos, images attribuées à une entrée extérieure (souris réelle sur la
/// fenêtre) : celles qui la suivent de moins de ce délai (survol, info-bulle).
const EXTERNAL_GRACE: Duration = Duration::from_secs(1);
/// Présences par seconde pendant l'observation au repos (utilisateurs non affichés).
const FLOOD_RATE: f64 = 1000.0;
/// Utilisateurs fictifs de ces présences (jamais affichés : hors des auteurs du banc).
const FLOOD_BASE: u64 = 3_000_000_000_000_000_000;
const FLOOD_USERS: u64 = 20_000;
/// MESSAGE_UPDATE par seconde pendant l'observation au repos (lot C9) : messages
/// du salon ouvert hors de l'écran, filtrés comme par la passerelle.
const UPDATE_RATE: f64 = 20.0;
/// Phase « trafic » : serveurs touchés par la rafale de démarrage, attente
/// après elle, paquets par seconde au repos (mesuré sur un vrai compte :
/// environ 3), écart entre deux paquets visibles.
const TRAFFIC_BURST_GUILDS: usize = 40;
const TRAFFIC_SETTLE: Duration = Duration::from_secs(2);
const TRAFFIC_RATE: f64 = 3.0;
const TRAFFIC_VISIBLE_GAP: Duration = Duration::from_millis(400);
/// Étapes de la phase « trafic » : rafale puis stabilisation, repos sous
/// trafic, paquets visibles, fin.
const BURST: usize = 0;
const CALM: usize = 1;
const VISIBLE: usize = 2;
const OVER: usize = 3;

/// Auteur des messages de la phase « trafic ».
fn packet_author() -> String {
    format!(r#"{{ "id": "{}", "username": "auteur_du_trafic" }}"#, author_id(7))
}

/// MESSAGE_CREATE ordinaire (ni mention, ni réponse).
fn packet_create(channel: u64, guild: u64, id: u64, text: &str) -> String {
    json!({
        "id": id.to_string(), "channel_id": channel.to_string(), "guild_id": guild.to_string(),
        "author": serde_json::from_str::<Value>(&packet_author()).unwrap_or_default(),
        "content": text, "type": 0, "mentions": [], "mention_roles": [], "mention_everyone": false,
        "attachments": [], "embeds": [], "pinned": false, "flags": 0,
    })
    .to_string()
}

/// MESSAGE_UPDATE complet, comme Discord l'envoie quand il ajoute l'aperçu
/// d'un lien : texte, médias, épinglage, drapeaux (cinq événements).
fn packet_update(channel: u64, guild: u64, id: u64, text: &str) -> String {
    json!({
        "id": id.to_string(), "channel_id": channel.to_string(), "guild_id": guild.to_string(),
        "content": text, "pinned": false, "flags": 0, "attachments": [], "edited_timestamp": null,
        "embeds": [{ "type": "link", "title": "Aperçu du lien", "description": "ajouté après coup", "url": "https://exemple.invalid/a" }],
    })
    .to_string()
}

/// MESSAGE_UPDATE partiel : seulement l'aperçu d'un lien.
fn packet_embeds(channel: u64, guild: u64, id: u64) -> String {
    json!({
        "id": id.to_string(), "channel_id": channel.to_string(), "guild_id": guild.to_string(),
        "embeds": [{ "type": "link", "title": "Aperçu du lien", "url": "https://exemple.invalid/b" }],
    })
    .to_string()
}

fn packet_delete(channel: u64, guild: u64, id: u64) -> String {
    json!({ "id": id.to_string(), "channel_id": channel.to_string(), "guild_id": guild.to_string() }).to_string()
}

/// CHANNEL_UPDATE d'un salon textuel : seul le nom change (mêmes permissions, même catégorie).
fn packet_channel(guild: u64, channel: u64, parent: u64, name: &str) -> String {
    json!({
        "id": channel.to_string(), "guild_id": guild.to_string(), "type": 0, "name": name, "position": (channel % 1000) as i64,
        "parent_id": parent.to_string(), "permission_overwrites": [], "topic": null, "nsfw": false, "rate_limit_per_user": 0,
    })
    .to_string()
}

/// Comptes de la phase « trafic », par étape (rafale, repos, visibles).
#[derive(Default)]
struct TrafficCounts {
    stage: AtomicUsize,
    packets: [AtomicUsize; 3],
    events: [AtomicUsize; 3],
    /// Événements qui ont réveillé l'interface.
    wakes: [AtomicUsize; 3],
    /// Paquets visibles qui n'ont pas réveillé (attendu : aucun).
    missed: AtomicUsize,
}

#[derive(Default)]
struct Traffic {
    armed: bool,
    counts: Arc<TrafficCounts>,
    /// Images dessinées à chaque étape.
    frames: [usize; 3],
    /// Au repos : images qui suivent une entrée du système (comptées à part).
    external: usize,
    last_external: Option<Instant>,
    causes: Vec<String>,
}

fn guild_id(g: usize) -> u64 {
    GUILD_BASE + g as u64
}

fn channel_id(g: usize, c: usize) -> u64 {
    CHANNEL_BASE + (g as u64) * 1000 + c as u64
}

fn author_id(i: u64) -> u64 {
    2_000_000_000_000_000_000 + i % AUTHORS
}

/// Identifiant de message daté de `ms` (ms Unix).
fn snowflake(ms: i64, seq: u64) -> u64 {
    (((ms as u64).saturating_sub(DISCORD_EPOCH)) << 22) | (seq & 0x3F_FFFF)
}

const WORDS: &[&str] = &[
    "bonjour", "le", "serveur", "rust", "egui", "message", "salon", "demain", "compilation", "réunion", "merci", "voilà", "testé", "fenêtre", "texture",
    "défilement", "mémoire", "image", "pilote", "thème", "réglage", "liste", "historique", "clavier", "souris", "vraiment", "plutôt", "rapide", "lent",
];

fn sentence(seed: u64, words: usize) -> String {
    let mut s = String::new();
    for k in 0..words {
        if k > 0 {
            s.push(' ');
        }
        let w = WORDS[((seed.wrapping_mul(31).wrapping_add(k as u64 * 7)) % WORDS.len() as u64) as usize];
        s.push_str(w);
    }
    s
}

/// Message numéro `i` d'un salon : dix formes qui reviennent tour à tour.
pub fn message(i: u64, channel_id: u64, guild_id: u64, ms: i64) -> Message {
    // Groupes de trois messages d'un même auteur.
    let author = author_id(i / 3 + channel_id);
    let mut m = Message {
        id: snowflake(ms, i),
        channel_id,
        guild_id: Some(guild_id),
        author_id: author,
        author: format!("Auteur {}", author % AUTHORS),
        author_avatar: Avatar::of_user(author, Some(ImageHash::synthetic(author)), None),
        content: String::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        mentions: Vec::new(),
        mention_everyone: false,
        edited: false,
        system: None,
        meta: Default::default(),
    };
    m.content = match i % 10 {
        0 => "ok".into(),
        1 | 9 => sentence(i, 8),
        2 => sentence(i, 20),
        3 => sentence(i, 110),
        // Texte mis en forme : gras, citation, liste, spoiler, bloc de code coloré.
        4 => format!(
            "**{}** et *{}*\n> {}\n- {}\n- ||{}||\n```rust\nfn f{i}() -> u32 {{ {i} }} // fin\n```",
            sentence(i, 4),
            sentence(i + 1, 2),
            sentence(i + 2, 6),
            sentence(i + 3, 5),
            sentence(i + 4, 3)
        ),
        5 => format!("voir https://exemple.invalid/page/{i} et {}", sentence(i, 5)),
        6 => {
            let mut e = Embed::text(Some(&format!("Aperçu du lien {i}")), Some(&sentence(i, 25)));
            e.kind = EmbedKind::Article;
            e.url = Some(format!("https://exemple.invalid/article/{i}"));
            e.thumbnail = Some(EmbedMedia::new(format!("https://exemple.invalid/v/{i}.png"), Some(format!("https://images-ext-1.discordapp.net/external/{i}.png")), 160, 160, 0));
            m.embeds.push(e);
            format!("https://exemple.invalid/article/{i}")
        }
        7 => {
            let png = Attachment { id: i + 1, width: 640, height: 360, size: 54_321, content_type: Some("image/png".into()), ..Attachment::file(format!("capture-{i}.png"), format!("https://exemple.invalid/f/{i}.png")) };
            m.attachments.push(png.analysed(0));
            m.attachments.push(Attachment::file(format!("journal-{i}.txt"), format!("https://exemple.invalid/f/{i}.txt")));
            sentence(i, 4)
        }
        _ => format!("{}\nhttps://exemple.invalid/doc/{i}\n{}", sentence(i, 30), sentence(i + 1, 12)),
    };
    m
}

/// Magasin synthétique et ce que le banc en connaît.
pub struct Synthetic {
    pub store: Store,
    /// Salons remplis (le premier est ouvert).
    pub filled: Vec<u64>,
    /// Salon servi page par page, et tout son historique (ordre croissant).
    pub deep: (u64, Vec<Message>),
}

/// Magasin synthétique : `guilds` × `channels` (5 catégories par serveur),
/// `FILLED` salons du premier serveur remplis de `messages` messages étalés
/// sur plusieurs jours, et un salon de `deep` messages dont seule la dernière
/// page est chargée. Le premier salon rempli est ouvert.
pub fn store(cfg: &BenchCfg) -> Synthetic {
    let mut guilds = Vec::with_capacity(cfg.guilds);
    for g in 0..cfg.guilds {
        let mut channels = Vec::with_capacity(cfg.channels);
        let cats = 5.min(cfg.channels);
        for c in 0..cfg.channels {
            let (kind, parent) = if c < cats { (ChannelKind::Category, None) } else { (ChannelKind::Text, Some(channel_id(g, c % cats))) };
            channels.push(Channel { id: channel_id(g, c), name: format!("salon-{c}"), kind, position: c as i32, parent, overwrites: Some(Vec::new()), slowmode: 0 });
        }
        ferrocord::state::model::sort_channels(&mut channels);
        guilds.push(Guild::new(guild_id(g), format!("Serveur {g}"), channels));
    }
    let mut s = Store::new();
    s.apply(Event::Ready { me: User::new(ME, "Moi".into()), guilds, dms: Vec::new() });
    let text: Vec<u64> = s.guilds.first().map(|g| g.channels.iter().filter(|c| c.kind == ChannelKind::Text).take(FILLED + 1).map(|c| c.id).collect()).unwrap_or_default();
    let filled: Vec<u64> = text.iter().take(FILLED).copied().collect();
    let deep_id = text.get(FILLED).copied().unwrap_or_default();
    s.select_guild(guild_id(0));
    if let Some(first) = filled.first() {
        s.select_channel(*first);
    }
    s.take_commands();
    let now = chrono::Utc::now().timestamp_millis();
    // Quatre jours : des séparateurs de date apparaissent.
    let span = 4 * 86_400_000_i64;
    let step = span / cfg.messages.max(1) as i64;
    for c in &filled {
        let messages = (0..cfg.messages as u64).map(|i| message(i, *c, guild_id(0), now - span + step * i as i64)).collect();
        s.apply(Event::History { channel_id: *c, before: None, messages });
    }
    // Salon profond : vingt jours d'historique.
    let deep_span = 20 * 86_400_000_i64;
    let deep_step = deep_span / cfg.deep.max(1) as i64;
    let deep: Vec<Message> = (0..cfg.deep as u64).map(|i| message(i, deep_id, guild_id(0), now - deep_span + deep_step * i as i64)).collect();
    s.apply(Event::History { channel_id: deep_id, before: None, messages: deep[deep.len().saturating_sub(HISTORY_PAGE)..].to_vec() });
    Synthetic { store: s, filled, deep: (deep_id, deep) }
}

/// Mémoire du processus (octets) : (privée, de travail, pic de travail),
/// lue par PowerShell (aucune dépendance, aucun `unsafe`).
pub fn process_memory() -> Option<(u64, u64, u64)> {
    let pid = std::process::id();
    let cmd = format!("$p = Get-Process -Id {pid}; \"$($p.PrivateMemorySize64) $($p.WorkingSet64) $($p.PeakWorkingSet64)\"");
    let out = std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &cmd]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut it = text.split_whitespace().map(|v| v.parse::<u64>().ok());
    Some((it.next()??, it.next()??, it.next()??))
}

/// Compteurs de mémoire lus dans le processus (`perf::memory`), sans PowerShell.
#[cfg(windows)]
fn usage() -> Option<ferrocord::perf::memory::Usage> {
    ferrocord::perf::memory::usage()
}

/// Hors de Windows : pas de compteurs.
#[cfg(not(windows))]
fn usage() -> Option<Usage> {
    None
}

#[cfg(not(windows))]
struct Usage {
    private: u64,
    working: u64,
    private_working: u64,
}

fn usage_json() -> Value {
    match usage() {
        Some(u) => json!({ "privee_mo": mo(u.private), "travail_mo": mo(u.working), "travail_privee_mo": mo(u.private_working) }),
        None => Value::Null,
    }
}

/// Répartition de la mémoire privée engagée (`perf::memory::map`) : modules
/// (les plus lourds nommés), piles, tas (engagé et alloué), reste.
#[cfg(windows)]
fn map_json() -> Value {
    let m = ferrocord::perf::memory::map();
    // Bibliothèques graphiques réellement chargées (lot « gl ») : `libglesv2.dll` dit qu'ANGLE
    // sert, `atio6axx.dll`, `nvoglv64.dll`… que c'est le pilote OpenGL du fabricant.
    const GRAPHIQUES: [&str; 14] = ["libegl", "libgles", "opengl32", "atio", "atid", "amdx", "nvogl", "nvwgf", "nvldumd", "ig", "d3d1", "d3dcompiler", "dxgi", "vulkan"];
    let pilotes: Vec<&str> = m.by_module.iter().map(|(n, _)| n.as_str()).filter(|n| GRAPHIQUES.iter().any(|g| n.starts_with(g))).collect();
    let modules: Vec<Value> = m.by_module.iter().take(8).filter(|(_, b)| *b >= 512 * 1024).map(|(n, b)| json!({ "module": n, "mo": mo(*b) })).collect();
    json!({
        "pilotes": pilotes,
        "modules_mo": mo(m.images), "principaux_modules": modules,
        "piles_mo": mo(m.stacks), "piles": m.stack_count,
        "tas_engage_mo": mo(m.heaps), "tas_alloue_mo": mo(m.heaps_allocated), "tas": m.heap_count,
        "autre_mo": mo(m.other), "total_mo": mo(m.total()), "sections_partagees_mo": mo(m.mapped),
    })
}

#[cfg(not(windows))]
fn map_json() -> Value {
    Value::Null
}

/// Tas engagé, tas alloué et reste de la mémoire privée, pour une étape.
#[cfg(windows)]
fn stage_map() -> Option<(u64, u64, u64)> {
    let m = ferrocord::perf::memory::map();
    Some((m.heaps, m.heaps_allocated, m.other))
}

#[cfg(not(windows))]
fn stage_map() -> Option<(u64, u64, u64)> {
    None
}

/// Nombre de fils du processus (PowerShell, comme la mémoire).
fn thread_count() -> Option<u64> {
    let cmd = format!("(Get-Process -Id {}).Threads.Count", std::process::id());
    let out = std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &cmd]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

fn mem_json(m: Option<(u64, u64, u64)>) -> Value {
    match m {
        Some((p, w, peak)) => json!({ "privee_mo": mo(p), "travail_mo": mo(w), "pic_travail_mo": mo(peak) }),
        None => Value::Null,
    }
}

fn mo(b: u64) -> f64 {
    (b as f64 / 1_048_576.0 * 10.0).round() / 10.0
}

/// Mémoire lue dans un fil, à attendre.
type MemSample = std::thread::JoinHandle<Option<(u64, u64, u64)>>;

/// Lecture de la mémoire dans un fil (PowerShell met près d'une seconde).
fn sample_later() -> MemSample {
    std::thread::spawn(process_memory)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Warmup,
    Idle,
    Scroll,
    Live,
    Switch,
    Check,
    History,
    Quiet,
    Traffic,
    Done,
}

impl Phase {
    fn key(self) -> &'static str {
        match self {
            Phase::Warmup => "prechauffage",
            Phase::Idle => "repos",
            Phase::Scroll => "defilement",
            Phase::Live => "arrivee_50_par_s",
            Phase::Switch => "changement_de_salon",
            Phase::Check => "verification",
            Phase::History => "historique",
            Phase::Quiet => "silence",
            Phase::Traffic => "trafic",
            Phase::Done => "fin",
        }
    }
}

/// Étapes de la phase « historique ».
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Walk {
    /// Salon ouvert, pointeur parti : animations terminées avant la capture.
    Settle,
    /// Capture de départ demandée.
    ShotStart,
    Up,
    Down,
    /// Pointeur parti à nouveau.
    SettleEnd,
    ShotEnd,
    Over,
}

/// Captures (numéro d'image, image) de la vérification.
type Shots = Arc<Mutex<Vec<(usize, Arc<egui::ColorImage>)>>>;

#[derive(Default)]
struct Series {
    layout: Vec<f64>,
    frame: Vec<f64>,
    /// Allocations faites pendant l'image, tous fils (lot « long ») : ce que
    /// l'image refait à l'identique se voit d'abord ici.
    allocs: Vec<f64>,
}

/// Médiane, 95e centile et maximum d'un compte par image.
fn counts(v: &[f64]) -> Value {
    if v.is_empty() {
        return Value::Null;
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let q = |p: f64| s[((s.len() - 1) as f64 * p).round() as usize];
    json!({ "mediane": q(0.5), "p95": q(0.95), "max": s[s.len() - 1] })
}

fn stats(v: &[f64]) -> Value {
    if v.is_empty() {
        return Value::Null;
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let q = |p: f64| s[((s.len() - 1) as f64 * p).round() as usize];
    let r = |x: f64| (x * 1000.0).round() / 1000.0;
    json!({ "n": s.len(), "mediane_ms": r(q(0.5)), "p95_ms": r(q(0.95)), "max_ms": r(s[s.len() - 1]), "min_ms": r(s[0]) })
}

fn get(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN)
}

/// Zone (pixels) où deux captures diffèrent, ou `None` si elles sont identiques.
fn difference(a: &egui::ColorImage, b: &egui::ColorImage) -> Option<String> {
    if a.size != b.size {
        return Some(format!("tailles {:?} et {:?}", a.size, b.size));
    }
    if a.pixels == b.pixels {
        return None;
    }
    let w = a.size[0];
    let (mut x0, mut y0, mut x1, mut y1, mut n) = (usize::MAX, usize::MAX, 0, 0, 0);
    for (i, (p, q)) in a.pixels.iter().zip(&b.pixels).enumerate() {
        if p != q {
            let (x, y) = (i % w, i / w);
            (x0, y0, x1, y1, n) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y), n + 1);
        }
    }
    Some(format!("{n} pixels dans [{x0},{y0}]-[{x1},{y1}]"))
}

/// Pages servies par la connexion simulée : (vers le passé, vers le présent).
pub type Served = Arc<(AtomicUsize, AtomicUsize)>;

pub struct Bench {
    cfg: BenchCfg,
    phase: Phase,
    phase_frames: usize,
    phase_start: Instant,
    filled: Vec<u64>,
    deep: u64,
    deep_first: u64,
    deep_last: u64,
    inject: Arc<Mutex<Vec<Event>>>,
    served: Served,
    /// Image en cours : phase, début de `logic`.
    current: Option<(Phase, Instant, bool)>,
    /// Image précédente, en attente de son temps mesuré par eframe.
    previous: Option<(Phase, f64, bool)>,
    series: Vec<(Phase, Series)>,
    t_start: Instant,
    t_window: Option<f64>,
    t_first_ui_end: Option<f64>,
    t_second_frame: Option<f64>,
    first_frame_cpu: Option<f64>,
    mem_start: Option<(u64, u64, u64)>,
    mem_loaded: Option<std::thread::JoinHandle<Option<(u64, u64, u64)>>>,
    /// Mémoire au repos après le démarrage (lot « ram ») : compteurs, tas vivant et répartition, lus dans le processus.
    rest: Option<(Value, alloc::Heap)>,
    /// Mémoire à la fin des phases qui la font monter (changements de salon, historique).
    mem_phases: Vec<(&'static str, MemSample)>,
    live_sent: u64,
    live_seq: u64,
    switch_next: usize,
    /// Pointeur au milieu de la discussion.
    center: Option<egui::Pos2>,
    /// Événement de pointeur à injecter à l'image suivante.
    pointer: Option<egui::Event>,
    /// Captures (numéro d'image, image) de la vérification.
    shots: Shots,
    check_frames: usize,
    /// Paires comparées (chaque paire est rendue dès qu'elle l'est), identiques, écarts.
    pairs: usize,
    identical: usize,
    differences: Vec<String>,
    /// Premier message affiché : au début, au demi-tour et à la fin du défilement.
    scroll_marks: Vec<Option<u64>>,
    walk: Walk,
    walk_frames: usize,
    walk_start: Instant,
    walk_shots: Shots,
    /// Écart entre la capture d'avant et celle d'après l'aller-retour (`Some(None)` : identiques).
    round_trip: Option<Option<String>>,
    /// Durées (s) de la remontée et de la redescente, pages servies, mémoire du salon au plus haut.
    walk_report: serde_json::Map<String, Value>,
    walk_peak: usize,
    quiet_settle_frames: usize,
    quiet_frames: usize,
    /// Au repos : images qui suivent (à moins de `EXTERNAL_GRACE`) une entrée
    /// venue du système (souris réelle, focus), comptées à part : ce n'est pas
    /// l'application qui se réveille.
    quiet_external: usize,
    last_external: Option<Instant>,
    quiet_settled: Arc<AtomicBool>,
    quiet_over: Arc<AtomicBool>,
    quiet_armed: bool,
    /// Causes des redessins (fichier et ligne qui les ont demandés) : stabilisation, observation.
    quiet_settle_causes: Vec<String>,
    quiet_causes: Vec<String>,
    /// Présences injectées pendant l'observation au repos (utilisateurs non
    /// affichés), et celles qui ont demandé un redessin (attendu : aucune).
    people: PeopleHub,
    flood: Arc<AtomicUsize>,
    flood_wakes: Arc<AtomicUsize>,
    /// MESSAGE_UPDATE hors de l'écran injectés au repos, et ceux qui ont réveillé (attendu : aucun).
    updates: Arc<AtomicUsize>,
    update_wakes: Arc<AtomicUsize>,
    /// Salon à ouvrir au début de l'image suivante.
    pending: Option<u64>,
    frame_no: usize,
    /// Temps processeur par phase : images, fil d'interface (ms), autres fils (ms).
    cpu: Vec<(Phase, usize, f64, f64)>,
    cpu_mark: (f64, f64),
    /// Allocations comptées au début de l'image précédente.
    alloc_mark: usize,
    /// Phase « trafic » (lot « perf »).
    traffic: Traffic,
}

impl Bench {
    pub fn new(cfg: BenchCfg, syn: &Synthetic, inject: Arc<Mutex<Vec<Event>>>, served: Served, t_start: Instant, mem_start: Option<(u64, u64, u64)>) -> Self {
        Bench {
            cfg,
            phase: Phase::Warmup,
            phase_frames: 0,
            phase_start: Instant::now(),
            filled: syn.filled.clone(),
            deep: syn.deep.0,
            deep_first: syn.deep.1.first().map_or(0, |m| m.id),
            deep_last: syn.deep.1.last().map_or(0, |m| m.id),
            inject,
            served,
            current: None,
            previous: None,
            series: Vec::new(),
            t_start,
            t_window: None,
            t_first_ui_end: None,
            t_second_frame: None,
            first_frame_cpu: None,
            mem_start,
            mem_loaded: None,
            rest: None,
            mem_phases: Vec::new(),
            live_sent: 0,
            live_seq: 0,
            switch_next: 1,
            center: None,
            pointer: None,
            shots: Arc::default(),
            check_frames: 0,
            pairs: 0,
            identical: 0,
            differences: Vec::new(),
            scroll_marks: Vec::new(),
            walk: Walk::Settle,
            walk_frames: 0,
            walk_start: Instant::now(),
            walk_shots: Arc::default(),
            round_trip: None,
            walk_report: serde_json::Map::new(),
            walk_peak: 0,
            quiet_settle_frames: 0,
            quiet_frames: 0,
            quiet_external: 0,
            last_external: None,
            quiet_settled: Arc::default(),
            quiet_over: Arc::default(),
            quiet_armed: false,
            quiet_settle_causes: Vec::new(),
            quiet_causes: Vec::new(),
            people: syn.store.people.clone(),
            flood: Arc::default(),
            flood_wakes: Arc::default(),
            updates: Arc::default(),
            update_wakes: Arc::default(),
            pending: None,
            frame_no: 0,
            cpu: Vec::new(),
            cpu_mark: (0.0, 0.0),
            alloc_mark: 0,
            traffic: Traffic::default(),
        }
    }

    pub fn window_created(&mut self) {
        self.t_window = Some(self.t_start.elapsed().as_secs_f64() * 1000.0);
    }

    /// Entrées réelles ignorées (la mesure ne dépend pas de la souris) ;
    /// pointeur posé sur la discussion, molette pendant les défilements.
    pub fn input(&mut self, raw: &mut egui::RawInput) {
        // Au repos : ce que le système a envoyé à l'interface (origine des images).
        if self.phase == Phase::Quiet && self.quiet_armed && !self.quiet_over.load(Ordering::SeqCst) && !raw.events.is_empty() {
            let kinds: Vec<String> = raw.events.iter().map(|e| format!("{e:?}").chars().take(60).collect()).collect();
            let into = if self.quiet_settled.load(Ordering::SeqCst) { &mut self.quiet_causes } else { &mut self.quiet_settle_causes };
            if self.quiet_settled.load(Ordering::SeqCst) {
                self.last_external = Some(Instant::now());
            }
            into.push(format!("événements reçus : {kinds:?}"));
        }
        // Phase « trafic », au repos : même relevé.
        if self.phase == Phase::Traffic && self.traffic.counts.stage.load(Ordering::SeqCst) == CALM && !raw.events.is_empty() {
            let kinds: Vec<String> = raw.events.iter().map(|e| format!("{e:?}").chars().take(60).collect()).collect();
            self.traffic.last_external = Some(Instant::now());
            self.traffic.causes.push(format!("événements reçus : {kinds:?}"));
        }
        raw.events.retain(|e| {
            !matches!(
                e,
                egui::Event::PointerMoved(_)
                    | egui::Event::PointerButton { .. }
                    | egui::Event::MouseMoved(_)
                    | egui::Event::MouseWheel { .. }
                    | egui::Event::Key { .. }
                    | egui::Event::Text(_)
                    | egui::Event::PointerGone
            )
        });
        if let Some(e) = self.pointer.take() {
            raw.events.push(e);
        }
        let wheel = match (self.phase, self.walk) {
            (Phase::Scroll, _) => Some(if self.phase_frames < self.cfg.frames / 2 { WHEEL } else { -WHEEL }),
            (Phase::History, Walk::Up) => Some(WHEEL_FAST),
            (Phase::History, Walk::Down) => Some(-WHEEL_FAST),
            _ => None,
        };
        if let Some(dy) = wheel {
            raw.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, dy),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: egui::Modifiers::NONE,
            });
        }
    }

    pub fn before(&mut self, frame: &eframe::Frame, app: &mut App, ctx: &egui::Context) {
        self.frame_no += 1;
        let cpu = frame.info().cpu_usage.map(|s| f64::from(s) * 1000.0);
        let allocated = alloc::live().total;
        let allocs = allocated.saturating_sub(std::mem::replace(&mut self.alloc_mark, allocated)) as f64;
        if self.frame_no == 2 {
            self.t_second_frame = Some(self.t_start.elapsed().as_secs_f64() * 1000.0);
            self.first_frame_cpu = cpu;
        }
        if let (Some((phase, layout, shot)), Some(cpu)) = (self.previous.take(), cpu)
            && !shot
        {
            let s = self.series_mut(phase);
            s.layout.push(layout);
            s.frame.push(cpu);
            s.allocs.push(allocs);
        }
        if self.phase == Phase::Quiet && self.quiet_armed && !self.quiet_over.load(Ordering::SeqCst) {
            let causes = ctx.repaint_causes().iter().map(|c| format!("{c:?}")).collect::<Vec<_>>();
            let external = self.last_external.is_some_and(|t| t.elapsed() < EXTERNAL_GRACE);
            if self.quiet_settled.load(Ordering::SeqCst) && external {
                self.quiet_external += 1;
            } else if self.quiet_settled.load(Ordering::SeqCst) {
                self.quiet_frames += 1;
                self.quiet_causes.extend(causes);
            } else {
                self.quiet_settle_frames += 1;
                self.quiet_settle_causes.extend(causes);
            }
        }
        if self.phase == Phase::Traffic && self.traffic.armed {
            let stage = self.traffic.counts.stage.load(Ordering::SeqCst);
            let external = self.traffic.last_external.is_some_and(|t| t.elapsed() < EXTERNAL_GRACE);
            if stage == CALM && external {
                self.traffic.external += 1;
            } else if stage < OVER {
                self.traffic.frames[stage] += 1;
                if stage == CALM {
                    self.traffic.causes.extend(ctx.repaint_causes().iter().map(|c| format!("{c:?}")));
                }
            }
        }
        // Arrivée de messages : la file est lue par `App::logic` juste après.
        if self.phase == Phase::Live {
            let due = (self.phase_start.elapsed().as_secs_f64() * LIVE_RATE) as u64;
            let now = chrono::Utc::now().timestamp_millis();
            if let (Ok(mut q), Some(&c)) = (self.inject.lock(), self.filled.first()) {
                while self.live_sent < due {
                    self.live_sent += 1;
                    self.live_seq += 1;
                    let m = message(self.live_seq * 7 + 3, c, guild_id(0), now);
                    q.push(Event::MessageCreate(Message { id: snowflake(now, 0x20_0000 + self.live_seq), ..m }));
                }
            }
        }
        let start = Instant::now();
        // Changement de salon demandé par le banc : fait avant la mise en page,
        // comme un clic traité au début de l'image.
        if let Some(c) = self.pending.take() {
            app.show_channel(Some(guild_id(0)), c);
        }
        let shot = self.phase == Phase::Check || matches!(self.walk, Walk::ShotStart | Walk::ShotEnd) && self.phase == Phase::History;
        self.current = Some((self.phase, start, shot));
    }

    fn series_mut(&mut self, phase: Phase) -> &mut Series {
        if let Some(i) = self.series.iter().position(|(p, _)| *p == phase) {
            return &mut self.series[i].1;
        }
        self.series.push((phase, Series::default()));
        let last = self.series.len() - 1;
        &mut self.series[last].1
    }

    fn enter(&mut self, phase: Phase) {
        // Temps processeur de la phase qui finit : fil d'interface, autres fils.
        let now = ferrocord::perf::threads::totals();
        self.cpu.push((self.phase, self.phase_frames, now.0 - self.cpu_mark.0, now.1 - self.cpu_mark.1));
        self.cpu_mark = now;
        // Le relevé des fils (un instantané de tout le système) dure plusieurs
        // millisecondes : l'image qui le contient sort des statistiques.
        if let Some(p) = &mut self.previous {
            p.2 = true;
        }
        self.phase = phase;
        self.phase_frames = 0;
        self.phase_start = Instant::now();
        // Profil des allocations de la phase demandée (`FERROCORD_BANC_PROFIL`).
        alloc::profile(std::env::var("FERROCORD_BANC_PROFIL").is_ok_and(|p| p == phase.key()));
        let a = self.filled.first().copied().unwrap_or_default();
        match phase {
            Phase::Idle | Phase::Live | Phase::Quiet => self.pending = Some(a),
            // Pointeur parti : le survol d'un message et sa barre d'actions arrivent en
            // fondu (lot ANIM) et feraient différer les deux captures d'une paire.
            Phase::Check => self.pointer = Some(egui::Event::PointerGone),
            Phase::History => {
                self.pending = Some(self.deep);
                self.pointer = Some(egui::Event::PointerGone);
                self.walk = Walk::Settle;
                self.walk_frames = 0;
            }
            _ => {}
        }
    }

    /// Compare les paires de captures complètes et les rend aussitôt (lot C10) :
    /// gardées jusqu'au rapport, les seize captures et leurs copies faisaient
    /// monter la mémoire privée de plus de 300 Mo, comptés à tort comme des
    /// « pics » de l'application après les changements de salon et l'historique.
    fn compare_checks(&mut self) {
        let shots = self.shots.clone();
        let Ok(mut v) = shots.lock() else { return };
        for k in 0..CHECKS {
            let (Some(a), Some(b)) = (v.iter().position(|(n, _)| *n == 2 * k), v.iter().position(|(n, _)| *n == 2 * k + 1)) else { continue };
            self.pairs += 1;
            match difference(&v[a].1, &v[b].1) {
                None => self.identical += 1,
                Some(d) => {
                    // Différentes : les deux captures sont gardées à côté du rapport.
                    if let Some(out) = &self.cfg.out {
                        for (img, name) in [(&v[a].1, "a"), (&v[b].1, "b")] {
                            let path = out.with_file_name(format!("{}-paire-{k}-{name}.png", self.cfg.label));
                            if let Err(e) = super::save_png(&path, img) {
                                eprintln!("capture impossible ({}) : {e}", path.display());
                            }
                        }
                    }
                    self.differences.push(format!("paire {k} : {d}"));
                }
            }
            v.retain(|(n, _)| *n != 2 * k && *n != 2 * k + 1);
        }
    }

    /// Compare la capture d'avant l'aller-retour dans l'historique à celle d'après, puis les rend.
    fn compare_walk(&mut self) {
        let walk_shots = self.walk_shots.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default();
        self.round_trip = match (walk_shots.iter().find(|(n, _)| *n == 0), walk_shots.iter().find(|(n, _)| *n == 1)) {
            (Some((_, a)), Some((_, b))) => {
                let d = difference(a, b);
                // Différentes : les deux captures sont gardées à côté du rapport.
                if let (Some(_), Some(out)) = (&d, &self.cfg.out) {
                    for (img, name) in [(a, "historique-avant.png"), (b, "historique-apres.png")] {
                        let path = out.with_file_name(format!("{}-{name}", self.cfg.label));
                        if let Err(e) = super::save_png(&path, img) {
                            eprintln!("capture impossible ({}) : {e}", path.display());
                        }
                    }
                }
                Some(d)
            }
            _ => None,
        };
    }

    /// Premier message affiché dans le salon ouvert.
    fn first_shown(app: &App) -> Option<u64> {
        let s = app.store();
        s.channel.and_then(|c| s.chans.get(&c)).and_then(|c| c.viewing()).map(|(lo, _)| lo)
    }

    fn walk_to(&mut self, w: Walk) {
        self.walk = w;
        self.walk_frames = 0;
        self.walk_start = Instant::now();
    }

    /// Une étape de la phase « historique » (après l'image).
    fn walk_step(&mut self, app: &App, ctx: &egui::Context) {
        self.walk_frames += 1;
        let cs = app.store().chans.get(&self.deep);
        self.walk_peak = self.walk_peak.max(cs.map_or(0, |c| c.msgs.len()));
        let shown = cs.and_then(|c| c.viewing());
        match self.walk {
            // (Le relevé de mémoire d'avant la phase est fini : il ne compte pas la capture.)
            Walk::Settle | Walk::SettleEnd if self.walk_frames >= SETTLE_FRAMES && self.mem_phases.iter().all(|(_, h)| h.is_finished()) => {
                let shots = self.walk_shots.clone();
                let no = usize::from(self.walk == Walk::SettleEnd);
                ctx.request_screenshot(move |img| {
                    if let Ok(mut v) = shots.lock() {
                        v.push((no, img));
                    }
                });
                self.walk_to(if no == 0 { Walk::ShotStart } else { Walk::ShotEnd });
            }
            Walk::ShotStart => {
                self.pointer = self.center.map(egui::Event::PointerMoved);
                self.walk_to(Walk::Up);
            }
            Walk::Up => {
                let top = cs.is_some_and(|c| !c.more && c.msgs.front().map(|m| m.id) == Some(self.deep_first)) && shown.is_some_and(|(lo, _)| lo == self.deep_first);
                if top || self.walk_start.elapsed() > HISTORY_CAP {
                    self.walk_report.insert("remontee_s".into(), json!(self.walk_start.elapsed().as_secs_f64()));
                    self.walk_report.insert("haut_atteint".into(), json!(top));
                    self.walk_to(Walk::Down);
                }
            }
            Walk::Down => {
                let bottom = cs.is_some_and(|c| !c.newer && c.msgs.back().map(|m| m.id) == Some(self.deep_last)) && shown.is_some_and(|(_, hi)| hi == self.deep_last);
                if bottom || self.walk_start.elapsed() > HISTORY_CAP {
                    self.walk_report.insert("redescente_s".into(), json!(self.walk_start.elapsed().as_secs_f64()));
                    self.walk_report.insert("bas_atteint".into(), json!(bottom));
                    self.pointer = Some(egui::Event::PointerGone);
                    self.walk_to(Walk::SettleEnd);
                }
            }
            Walk::ShotEnd if self.walk_shots.lock().map(|v| v.len()).unwrap_or(0) >= 2 || self.walk_frames > 120 => {
                // Comparées et rendues tout de suite : la mémoire relevée ensuite ne les compte pas.
                self.compare_walk();
                self.pointer = self.center.map(egui::Event::PointerMoved);
                self.walk_to(Walk::Over);
            }
            _ => {}
        }
    }

    /// Fin d'une image : passage à la phase suivante, réveil de la suivante.
    /// Renvoie `true` quand le banc est fini (la fenêtre doit se fermer).
    pub fn after(&mut self, app: &mut App, ctx: &egui::Context) -> bool {
        if let Some((phase, start, shot)) = self.current.take() {
            let layout = start.elapsed().as_secs_f64() * 1000.0;
            self.previous = Some((phase, layout, shot));
        }
        if self.t_first_ui_end.is_none() {
            self.t_first_ui_end = Some(self.t_start.elapsed().as_secs_f64() * 1000.0);
            // Pointeur au milieu de la zone des messages.
            let r = ctx.content_rect();
            self.center = Some(egui::pos2(r.max.x * 0.65, r.max.y * 0.5));
            self.pointer = self.center.map(egui::Event::PointerMoved);
        }
        self.phase_frames += 1;
        let n = self.cfg.frames;
        let filled = self.filled.clone();
        match self.phase {
            Phase::Warmup => {
                // Chaque salon rempli est ouvert une fois : avatars chargés, hauteurs connues.
                let k = self.phase_frames / 20;
                if self.phase_frames.is_multiple_of(20) && k < filled.len() {
                    self.pending = Some(filled[k]);
                }
                let settled = app.image_stats().pending == 0;
                let waited = self.phase_start.elapsed();
                if (self.phase_frames > 20 * filled.len() + 60 && settled) || waited > Duration::from_secs(20) {
                    self.mem_loaded = Some(sample_later());
                    self.rest = Some((json!({ "compteurs": usage_json(), "carte": map_json() }), alloc::live()));
                    self.enter(Phase::Idle);
                }
            }
            Phase::Idle if self.phase_frames >= n => {
                self.enter(Phase::Scroll);
                self.scroll_marks.push(Self::first_shown(app));
            }
            Phase::Scroll => {
                if self.phase_frames == n / 2 {
                    self.scroll_marks.push(Self::first_shown(app));
                }
                if self.phase_frames >= n {
                    self.scroll_marks.push(Self::first_shown(app));
                    self.enter(Phase::Live);
                }
            }
            Phase::Live if self.phase_frames >= n => {
                self.enter(Phase::Switch);
            }
            Phase::Switch => {
                if self.phase_frames >= n {
                    self.enter(Phase::Check);
                } else {
                    let c = filled[self.switch_next % filled.len()];
                    self.switch_next += 1;
                    self.pending = Some(c);
                }
            }
            Phase::Check => {
                // Par trois images : changement demandé (fait au début de la
                // suivante) ; image du changement, capturée ; image sans aucun
                // changement, capturée. Les deux captures doivent être identiques.
                self.compare_checks();
                let settle = 2 * CHECK_SETTLE * filled.len();
                if self.phase_frames <= settle {
                    // Chaque salon est d'abord montré le temps que ses fondus finissent
                    // (survol, barre de défilement qui s'efface quand le pointeur part).
                    if self.phase_frames.is_multiple_of(CHECK_SETTLE) {
                        self.pending = Some(filled[(self.phase_frames / CHECK_SETTLE) % filled.len()]);
                    }
                } else if self.check_frames < 3 * CHECKS {
                    let step = self.check_frames % 3;
                    if step == 0 {
                        let c = filled[self.switch_next % filled.len()];
                        self.switch_next += 1;
                        self.pending = Some(c);
                        self.check_frames += 1;
                        ctx.request_repaint();
                        return false;
                    }
                    let shots = self.shots.clone();
                    let no = 2 * (self.check_frames / 3) + step - 1;
                    ctx.request_screenshot(move |img| {
                        if let Ok(mut v) = shots.lock() {
                            v.push((no, img));
                        }
                    });
                    self.check_frames += 1;
                } else if self.pairs >= CHECKS || self.phase_frames > settle + 4 * CHECKS + 120 {
                    // Mémoire après les changements de salon : aucune capture n'est plus gardée.
                    self.mem_phases.push(("apres_changements_de_salon", sample_later()));
                    self.enter(Phase::History);
                }
            }
            Phase::History => {
                self.walk_step(app, ctx);
                if self.walk == Walk::Over {
                    self.mem_phases.push(("apres_historique", sample_later()));
                    self.enter(Phase::Quiet);
                }
            }
            Phase::Quiet => {
                if !self.quiet_armed {
                    // Dernière image demandée par le banc : plus rien n'est
                    // redemandé. Un fil marque la fin de la stabilisation, puis
                    // celle de l'observation, et réveille l'interface à la fin.
                    if self.phase_frames > 2 {
                        self.quiet_armed = true;
                        let (settled, over, ctx2, secs) = (self.quiet_settled.clone(), self.quiet_over.clone(), ctx.clone(), self.cfg.quiet);
                        std::thread::spawn(move || {
                            std::thread::sleep(QUIET_SETTLE);
                            settled.store(true, Ordering::SeqCst);
                            std::thread::sleep(Duration::from_secs(secs));
                            over.store(true, Ordering::SeqCst);
                            ctx2.request_repaint();
                        });
                        self.spawn_flood(ctx);
                        self.spawn_updates(app, ctx);
                        return false;
                    }
                    ctx.request_repaint();
                    return false;
                }
                if self.quiet_over.load(Ordering::SeqCst) {
                    self.enter(Phase::Traffic);
                    ctx.request_repaint();
                }
                return false;
            }
            Phase::Traffic => {
                if !self.traffic.armed {
                    // Comme au repos : dernière image demandée par le banc, puis
                    // seuls les paquets rejoués peuvent réveiller l'interface.
                    if self.phase_frames > 2 {
                        self.traffic.armed = true;
                        self.spawn_traffic(app, ctx);
                        return false;
                    }
                    ctx.request_repaint();
                    return false;
                }
                if self.traffic.counts.stage.load(Ordering::SeqCst) >= OVER {
                    self.phase = Phase::Done;
                    self.report(app, ctx);
                    return true;
                }
                return false;
            }
            _ => {}
        }
        ctx.request_repaint();
        false
    }

    /// Pendant l'observation au repos : `FLOOD_RATE` présences par seconde
    /// d'utilisateurs jamais affichés, écrites comme le fait la passerelle
    /// (`People::set_presence`, réveil seulement si elle le demande).
    fn spawn_flood(&self, ctx: &egui::Context) {
        let (people, settled, over, sent, wakes, ctx) = (self.people.clone(), self.quiet_settled.clone(), self.quiet_over.clone(), self.flood.clone(), self.flood_wakes.clone(), ctx.clone());
        std::thread::spawn(move || {
            while !settled.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(10));
            }
            let start = Instant::now();
            let mut k: u64 = 0;
            while !over.load(Ordering::SeqCst) {
                // Rattrape le rythme : `FLOOD_RATE` par seconde depuis le début.
                let due = (start.elapsed().as_secs_f64() * FLOOD_RATE) as u64;
                while k < due {
                    let status = if k.is_multiple_of(2) { Status::Online } else { Status::Idle };
                    let p = Presence { status, client: ClientStatus { desktop: Some(status), ..Default::default() }, activities: Vec::new() };
                    if people.lock().set_presence(FLOOD_BASE + k % FLOOD_USERS, p) {
                        wakes.fetch_add(1, Ordering::SeqCst);
                        ctx.request_repaint();
                    }
                    k += 1;
                }
                sent.store(k as usize, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(5));
            }
        });
    }

    /// Pendant l'observation au repos (lot C9) : `UPDATE_RATE` MESSAGE_UPDATE
    /// par seconde sur des messages du salon ouvert hors de l'écran (même
    /// texte : rien ne change à l'écran), décidés par le filtre de la
    /// passerelle (`Interest::verdict`) ; seul un réveil demande une image.
    fn spawn_updates(&self, app: &App, ctx: &egui::Context) {
        let s = app.store();
        let Some((cid, cs)) = s.channel.and_then(|c| s.chans.get(&c).map(|cs| (c, cs))) else { return };
        let Some((lo, hi)) = cs.viewing() else { return };
        let targets: Vec<(u64, String)> = cs.msgs.iter().filter(|m| m.id < lo || m.id > hi).take(200).map(|m| (m.id, m.content.clone())).collect();
        let Some(interest) = super::INTEREST.get().cloned() else { return };
        if targets.is_empty() {
            return;
        }
        let (inject, settled, over, sent, wakes, ctx) = (self.inject.clone(), self.quiet_settled.clone(), self.quiet_over.clone(), self.updates.clone(), self.update_wakes.clone(), ctx.clone());
        std::thread::spawn(move || {
            while !settled.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(10));
            }
            let start = Instant::now();
            let mut k = 0usize;
            while !over.load(Ordering::SeqCst) {
                let due = (start.elapsed().as_secs_f64() * UPDATE_RATE) as usize;
                while k < due {
                    let (id, content) = targets[k % targets.len()].clone();
                    let e = Event::MessageUpdate { channel_id: cid, id, content };
                    let verdict = interest.verdict(&e);
                    if verdict != ferrocord::state::interest::Verdict::Drop
                        && let Ok(mut q) = inject.lock()
                    {
                        q.push(e);
                    }
                    if verdict == ferrocord::state::interest::Verdict::Wake {
                        wakes.fetch_add(1, Ordering::SeqCst);
                        ctx.request_repaint();
                    }
                    k += 1;
                }
                sent.store(k, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(5));
            }
        });
    }

    /// Phase « trafic » : des paquets de la passerelle, en JSON, passent par
    /// le décodage et le filtre des réveils de la vraie connexion
    /// (`net::Replay`). Rafale de démarrage, repos sous trafic dans des salons
    /// qui ne sont pas à l'écran, puis paquets qui changent l'écran.
    fn spawn_traffic(&self, app: &App, ctx: &egui::Context) {
        let counts = self.traffic.counts.clone();
        let give_up = |ctx: &egui::Context| {
            counts.stage.store(OVER, Ordering::SeqCst);
            ctx.request_repaint();
        };
        let s = app.store();
        let (Some(interest), Some(open), Some(&b), Some(&c)) = (super::INTEREST.get().cloned(), s.channel, self.filled.get(1), self.filled.get(2)) else { return give_up(ctx) };
        let ids = |ch: u64| s.chans.get(&ch).map(|cs| cs.msgs.iter().map(|m| m.id).collect::<Vec<u64>>()).unwrap_or_default();
        let (lo, hi) = s.chans.get(&open).and_then(|cs| cs.viewing()).unwrap_or((0, 0));
        let (shown, hidden): (Vec<u64>, Vec<u64>) = ids(open).into_iter().partition(|id| (lo..=hi).contains(id));
        let (in_b, in_c) = (ids(b), ids(c));
        if shown.len() < 2 || hidden.is_empty() || in_b.is_empty() || in_c.is_empty() || s.guilds.len() < 3 {
            return give_up(ctx);
        }
        // Serveurs 1 à `last - 1` : rafale puis repos ; serveur `last` : gardé sans pastille jusqu'à la fin.
        let last = s.guilds.len().min(TRAFFIC_BURST_GUILDS + 2) - 1;
        let (inject, ctx, quiet) = (self.inject.clone(), ctx.clone(), self.cfg.quiet);
        let counts = counts.clone();
        std::thread::spawn(move || {
            let woken = Arc::new(AtomicUsize::new(0));
            let w = woken.clone();
            // Le réveil est compté ici et donné à l'interface après le dépôt
            // des événements : l'image qu'il demande les trouve dans la file.
            let wake: Wake = Arc::new(move || {
                w.fetch_add(1, Ordering::SeqCst);
            });
            let (mut replay, rx) = ferrocord::net::Replay::new(wake, interest, Some(ME));
            let mut send = |stage: usize, kind: &str, d: String| -> usize {
                replay.dispatch(kind, &d);
                let events: Vec<Event> = rx.try_iter().collect();
                let wakes = woken.swap(0, Ordering::SeqCst);
                counts.packets[stage].fetch_add(1, Ordering::SeqCst);
                counts.events[stage].fetch_add(events.len(), Ordering::SeqCst);
                counts.wakes[stage].fetch_add(wakes, Ordering::SeqCst);
                if let Ok(mut q) = inject.lock() {
                    q.extend(events);
                }
                if wakes > 0 {
                    ctx.request_repaint();
                }
                wakes
            };
            let mut seq = 0x30_0000u64;
            let mut fresh = || {
                seq += 1;
                snowflake(chrono::Utc::now().timestamp_millis(), seq)
            };
            // Salon textuel `k` du serveur `g` (les cinq premiers salons sont des catégories).
            let far = |g: usize, k: usize| (guild_id(g), channel_id(g, 5 + k));
            // Rafale de démarrage : dans chaque serveur, un premier message (la
            // pastille apparaît : réveil), un CHANNEL_UPDATE (premier du salon :
            // réveil), une modification ; et un message dans deux salons du serveur affiché.
            for g in 1..last {
                let (gid, ch) = far(g, 0);
                send(BURST, "MESSAGE_CREATE", packet_create(ch, gid, fresh(), "premier message"));
                send(BURST, "CHANNEL_UPDATE", packet_channel(gid, far(g, 1).1, channel_id(g, 1), "salon-6"));
                send(BURST, "MESSAGE_UPDATE", packet_update(ch, gid, fresh(), "aperçu ajouté"));
            }
            for ch in [b, c] {
                send(BURST, "MESSAGE_CREATE", packet_create(ch, guild_id(0), fresh(), "premier message"));
            }
            std::thread::sleep(TRAFFIC_SETTLE);
            // Repos : `TRAFFIC_RATE` paquets par seconde, aucun ne change l'écran.
            counts.stage.store(CALM, Ordering::SeqCst);
            let start = Instant::now();
            let mut k = 0usize;
            while start.elapsed() < Duration::from_secs(quiet) {
                let due = (start.elapsed().as_secs_f64() * TRAFFIC_RATE) as usize;
                while k < due {
                    let (gid, ch) = far(1 + k % (last - 1), 0);
                    match k % 6 {
                        // Message d'un autre salon du serveur affiché, modifié (cinq événements).
                        0 => send(CALM, "MESSAGE_UPDATE", packet_update(b, guild_id(0), in_b[k / 6 % in_b.len()], &format!("modifié {k}"))),
                        // Nouveau message d'un salon déjà non lu.
                        1 => send(CALM, "MESSAGE_CREATE", packet_create(ch, gid, fresh(), "encore un")),
                        // Message du salon ouvert, hors de l'écran, modifié.
                        2 => send(CALM, "MESSAGE_UPDATE", packet_update(open, guild_id(0), hidden[k / 6 % hidden.len()], &format!("modifié {k}"))),
                        3 => send(CALM, "MESSAGE_DELETE", packet_delete(c, guild_id(0), in_c[k / 6 % in_c.len()])),
                        // Salon d'un serveur qui n'est pas à l'écran, renommé.
                        4 => send(CALM, "CHANNEL_UPDATE", packet_channel(gid, channel_id(1 + k % (last - 1), 6), channel_id(1 + k % (last - 1), 1), &format!("salon-renommé-{k}"))),
                        // Aperçu de lien ajouté après coup (paquet partiel) dans un salon jamais ouvert.
                        _ => send(CALM, "MESSAGE_UPDATE", packet_embeds(ch, gid, fresh())),
                    };
                    k += 1;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            // Paquets qui changent l'écran : chacun doit réveiller.
            counts.stage.store(VISIBLE, Ordering::SeqCst);
            let unread_free = far(last, 0);
            let visible = [
                ("MESSAGE_UPDATE", packet_update(open, guild_id(0), shown[shown.len() / 2], "message affiché, modifié")),
                ("MESSAGE_CREATE", packet_create(open, guild_id(0), fresh(), "nouveau message du salon ouvert")),
                ("MESSAGE_DELETE", packet_delete(open, guild_id(0), shown[shown.len() / 2 - 1])),
                // Salon lu d'un serveur encore sans pastille : elle apparaît.
                ("MESSAGE_CREATE", packet_create(unread_free.1, unread_free.0, fresh(), "premier non-lu de ce serveur")),
                // Salon du serveur affiché, renommé : la colonne des salons change.
                ("CHANNEL_UPDATE", packet_channel(guild_id(0), channel_id(0, 9), channel_id(0, 4), "salon-renommé")),
            ];
            for (kind, d) in visible {
                if send(VISIBLE, kind, d) == 0 {
                    counts.missed.fetch_add(1, Ordering::SeqCst);
                }
                std::thread::sleep(TRAFFIC_VISIBLE_GAP);
            }
            counts.stage.store(OVER, Ordering::SeqCst);
            ctx.request_repaint();
        });
    }

    fn report(&mut self, app: &App, ctx: &egui::Context) {
        let mem_loaded = self.mem_loaded.take().and_then(|h| h.join().ok().flatten());
        // Mémoire au repos après le démarrage : le tas vivant ne dépend ni du pilote ni de la machine, d'où le seuil.
        let rest_heap = self.rest.as_ref().map(|(_, h)| *h);
        let rest_ok = rest_heap.is_some_and(|h| h.bytes <= REST_HEAP_MAX);
        let rest = match self.rest.take() {
            Some((mut v, h)) => {
                v["tas_vivant_mo"] = json!(mo(h.bytes as u64));
                v["blocs_vivants"] = json!(h.blocks);
                v["seuil_tas_vivant_mo"] = json!(mo(REST_HEAP_MAX as u64));
                v
            }
            None => Value::Null,
        };
        let mut mem_phases = serde_json::Map::new();
        for (k, h) in std::mem::take(&mut self.mem_phases) {
            mem_phases.insert(k.into(), mem_json(h.join().ok().flatten()));
        }
        let mut phases = serde_json::Map::new();
        for (p, s) in &self.series {
            phases.insert(p.key().into(), json!({ "mise_en_page": stats(&s.layout), "image": stats(&s.frame), "allocations": counts(&s.allocs) }));
        }
        // Temps processeur par image, tous fils : ce que le temps d'image ne voit pas
        // (pilote graphique, décodage) ; les phases à captures ou à attente sont écartées.
        let mut cpu = serde_json::Map::new();
        for (p, n, main, others) in &self.cpu {
            if *n > 0 && matches!(p, Phase::Idle | Phase::Scroll | Phase::Live | Phase::Switch | Phase::History) {
                let r = |x: f64| (x / *n as f64 * 1000.0).round() / 1000.0;
                cpu.insert(p.key().into(), json!({ "images": n, "fil_interface_ms": r(*main), "autres_fils_ms": r(*others) }));
            }
        }
        self.compare_checks();
        let (pairs, identical, differences) = (self.pairs, self.identical, std::mem::take(&mut self.differences));
        let round_trip = self.round_trip.take();
        let mem_end = process_memory();
        let heap_end = alloc::live();
        // Ce que tiennent les caches à ce moment (estimation de leurs octets).
        let (layouts, layout_bytes) = app.text_layouts();
        let img = app.image_stats();
        let (egui_tex, egui_tex_bytes) = ctx.tex_manager().read().allocated().fold((0, 0), |(n, b), (_, m)| (n + 1, b + m.size[0] * m.size[1] * m.bytes_per_pixel));
        let held = json!({
            "mises_en_page": { "nombre": layouts, "mo": mo(layout_bytes as u64) },
            "textures_des_images": { "nombre": img.textures, "mo": mo(img.bytes as u64) },
            "textures_egui": { "nombre": egui_tex, "mo": mo(egui_tex_bytes as u64), "atlas_polices_px": ctx.fonts(|f| f.font_image_size()) },
        });
        let budget = 4.0;
        let stat = |k: &str, s: &str| phases.get(k).map(|v| get(&v["image"], s)).unwrap_or(f64::NAN);
        let frame_verdict = |k: &str| {
            let (p95, max) = (stat(k, "p95_ms"), stat(k, "max_ms"));
            json!({ "verdict": if p95 < budget { "tenu" } else { "non tenu" }, "statistique": "p95", "p95_ms": p95, "max_ms": max, "budget_ms": budget })
        };
        // « < 1 image » : le salon est entièrement affiché dès l'image du changement
        // (capture identique à la suivante) et cette image tient dans une image à 60 Hz.
        let switch_p95 = stat("changement_de_salon", "p95_ms");
        let switch_ok = pairs > 0 && identical == pairs && switch_p95 < 1000.0 / 60.0;
        let moved = self.scroll_marks.len() == 3 && self.scroll_marks[0] != self.scroll_marks[1] && self.scroll_marks[1] != self.scroll_marks[2] && self.scroll_marks.iter().all(Option::is_some);
        let walked = self.walk_report.get("haut_atteint") == Some(&json!(true)) && self.walk_report.get("bas_atteint") == Some(&json!(true));
        let round_ok = round_trip == Some(None);
        self.walk_report.insert("pages_vers_le_passe".into(), json!(self.served.0.load(Ordering::SeqCst)));
        self.walk_report.insert("pages_vers_le_present".into(), json!(self.served.1.load(Ordering::SeqCst)));
        self.walk_report.insert("messages_en_memoire_max".into(), json!(self.walk_peak));
        self.walk_report.insert(
            "capture_finale_identique".into(),
            match &round_trip {
                Some(None) => json!(true),
                Some(Some(d)) => json!(d),
                None => json!("captures manquantes"),
            },
        );
        let verdict = |ok: bool| if ok { "tenu" } else { "non tenu" };
        // Une image sans cause ni événement pendant la stabilisation : egui
        // redessine deux fois après chaque demande (`request_repaint`, « Each
        // request results in two repaints » dans egui::context) ; la dernière
        // demande est celle du banc, juste avant le repos. Ce n'est pas l'application.
        let settle_note = if self.quiet_settle_causes.is_empty() && self.quiet_settle_frames <= 1 {
            "seconde image accordée par egui après la dernière demande d'image du banc (pas l'application)"
        } else {
            "voir les causes"
        };
        // Phase « trafic » : paquets, événements décodés, événements qui ont réveillé, images.
        let tc = &self.traffic.counts;
        let stage = |i: usize| json!({ "paquets": tc.packets[i].load(Ordering::SeqCst), "evenements": tc.events[i].load(Ordering::SeqCst), "evenements_ayant_reveille": tc.wakes[i].load(Ordering::SeqCst), "images": self.traffic.frames[i] });
        let traffic_ran = tc.packets[CALM].load(Ordering::SeqCst) > 0;
        let traffic_calm = traffic_ran && self.traffic.frames[CALM] == 0;
        let visible_sent = tc.packets[VISIBLE].load(Ordering::SeqCst);
        let visible_ok = visible_sent > 0 && tc.missed.load(Ordering::SeqCst) == 0 && self.traffic.frames[VISIBLE] >= visible_sent;
        let traffic = json!({
            "rafale_de_demarrage": stage(BURST), "stabilisation_s": TRAFFIC_SETTLE.as_secs_f64(),
            "repos": stage(CALM), "repos_s": self.cfg.quiet, "paquets_par_s": TRAFFIC_RATE, "causes": self.traffic.causes, "images_apres_entree_exterieure": self.traffic.external,
            "visibles": stage(VISIBLE), "paquets_visibles_sans_reveil": tc.missed.load(Ordering::SeqCst),
        });
        let store = app.store();
        let msgs: usize = store.chans.values().map(|c| c.msgs.len()).sum();
        let first = json!({
            "fenetre_creee_ms": self.t_window,
            "premiere_image_construite_ms": self.t_first_ui_end,
            "premiere_image_cpu_ms": self.first_frame_cpu,
            "debut_image_suivante_ms": self.t_second_frame,
        });
        let report = json!({
            "serie": self.cfg.label,
            "rendu": { "utilise": ferrocord::rendu::active().name(), "carte": ferrocord::rendu::carte(), "repli": ferrocord::rendu::repli() },
            "magasin": { "serveurs": store.guilds.len(), "salons": store.guilds.iter().map(|g| g.channels.len()).sum::<usize>(), "messages_en_memoire": msgs, "messages_par_salon_rempli": self.cfg.messages, "messages_du_salon_profond": self.cfg.deep },
            "images_par_phase": self.cfg.frames,
            "demarrage": first,
            "phases": Value::Object(phases.clone()),
            "processeur_par_image": Value::Object(cpu.clone()),
            "defilement_premier_message_affiche": { "debut": self.scroll_marks.first(), "demi_tour": self.scroll_marks.get(1), "fin": self.scroll_marks.get(2) },
            "changement_de_salon_verifie": { "paires": pairs, "identiques": identical, "differences": differences },
            "historique": Value::Object(self.walk_report.clone()),
            "redessins_au_repos": {
                "stabilisation_s": QUIET_SETTLE.as_secs_f64(), "images_stabilisation": self.quiet_settle_frames, "causes_stabilisation": self.quiet_settle_causes, "origine_stabilisation": settle_note,
                "secondes": self.cfg.quiet, "images": self.quiet_frames, "causes": self.quiet_causes, "images_apres_entree_exterieure": self.quiet_external,
                "presences_par_s": FLOOD_RATE, "presences_injectees": self.flood.load(Ordering::SeqCst), "presences_ayant_demande_un_redessin": self.flood_wakes.load(Ordering::SeqCst),
                "maj_hors_ecran_par_s": UPDATE_RATE, "maj_hors_ecran_injectees": self.updates.load(Ordering::SeqCst), "maj_hors_ecran_ayant_demande_un_redessin": self.update_wakes.load(Ordering::SeqCst),
            },
            "trafic": traffic,
            "memoire": { "depart": mem_json(self.mem_start), "apres_chargement": mem_json(mem_loaded), "repos_apres_demarrage": rest, "phases": Value::Object(mem_phases), "fin": mem_json(mem_end), "tas_a_la_fin": { "vivant_mo": mo(heap_end.bytes as u64), "blocs": heap_end.blocks }, "caches_a_la_fin": held },
            "verdicts": {
                "image_repos_moins_4ms": frame_verdict("repos"),
                "image_defilement_moins_4ms": frame_verdict("defilement"),
                "image_arrivee_moins_4ms": frame_verdict("arrivee_50_par_s"),
                "image_changement_de_salon_moins_4ms": frame_verdict("changement_de_salon"),
                "changement_de_salon_moins_1_image": { "verdict": verdict(switch_ok), "statistique": "captures identiques et p95 < 16,7 ms", "p95_ms": switch_p95, "max_ms": stat("changement_de_salon", "max_ms") },
                "zero_redessin_au_repos": { "verdict": verdict(self.quiet_frames == 0), "statistique": "nombre strict après stabilisation", "images": self.quiet_frames },
                "zero_image_au_repos_sous_trafic": { "verdict": verdict(traffic_calm), "statistique": "nombre strict, paquets de la passerelle dans des salons qui ne sont pas à l'écran", "images": self.traffic.frames[CALM] },
                "paquets_visibles_reveillent": { "verdict": verdict(visible_ok), "statistique": "chaque paquet qui change l'écran réveille et une image suit", "images": self.traffic.frames[VISIBLE] },
                "memoire_au_repos_apres_demarrage": { "verdict": verdict(rest_ok), "statistique": "octets vivants du tas après chargement, sous le seuil", "tas_vivant_mo": rest_heap.map(|h| mo(h.bytes as u64)), "seuil_mo": mo(REST_HEAP_MAX as u64) },
                "defilement_effectif": { "verdict": verdict(moved), "statistique": "premier message affiché différent au demi-tour et à la fin" },
                "historique_aller_retour": { "verdict": verdict(walked && round_ok), "statistique": "haut et bas atteints, capture finale identique à l'initiale" },
            },
        });
        // Profil des allocations demandé : les sites les plus fréquents, par image de la phase.
        if let Ok(wanted) = std::env::var("FERROCORD_BANC_PROFIL") {
            let frames = self.series.iter().find(|(p, _)| p.key() == wanted).map_or(1, |(_, s)| s.frame.len().max(1));
            for (n, bytes, stack) in alloc::profile_report().into_iter().take(60) {
                eprintln!("{:>8.1} allocations et {:>9.0} octets par image : {stack}", n as f64 / frames as f64, bytes as f64 / frames as f64);
            }
        }
        println!("=== Banc Ferrocord — série « {} » ===", self.cfg.label);
        println!(
            "magasin : {} serveurs, {} salons, {} messages en mémoire ({} par salon rempli, salon profond de {})",
            report["magasin"]["serveurs"], report["magasin"]["salons"], msgs, self.cfg.messages, self.cfg.deep
        );
        let f = |v: Option<f64>| v.map_or("?".into(), |x| format!("{x:.1} ms"));
        println!(
            "démarrage : fenêtre créée {} ; première image construite {} (dont {} mesurés par eframe) ; image suivante commencée à {}",
            f(self.t_window),
            f(self.t_first_ui_end),
            f(self.first_frame_cpu),
            f(self.t_second_frame)
        );
        println!("{:<22} {:>34} {:>34}", "phase", "mise en page (méd / p95 / max)", "image (méd / p95 / max)");
        for (p, _) in &self.series {
            if let Some(v) = phases.get(p.key()) {
                let g = |s: &Value| format!("{:.3} / {:.3} / {:.3}", get(s, "mediane_ms"), get(s, "p95_ms"), get(s, "max_ms"));
                println!("{:<22} {:>34} {:>34}   allocations par image : médiane {}, p95 {}, max {}", p.key(), g(&v["mise_en_page"]), g(&v["image"]), v["allocations"]["mediane"], v["allocations"]["p95"], v["allocations"]["max"]);
            }
        }
        println!("processeur par image, tous fils (ms) : {}", cpu.iter().map(|(k, v)| format!("{k} {} + {}", v["fil_interface_ms"], v["autres_fils_ms"])).collect::<Vec<_>>().join(" ; "));
        println!("défilement : premier message affiché {:?}", self.scroll_marks);
        println!("changement de salon : {identical} paires identiques sur {pairs} (image du changement = image suivante)");
        for d in &differences {
            println!("  {d}");
        }
        println!("historique : {}", Value::Object(self.walk_report.clone()));
        println!("stabilisation ({} s) : {} images ({settle_note})", QUIET_SETTLE.as_secs_f64(), self.quiet_settle_frames);
        for c in &self.quiet_settle_causes {
            println!("  cause : {c}");
        }
        println!("redessins pendant {} s au repos (après stabilisation) : {}", self.cfg.quiet, self.quiet_frames);
        if self.quiet_external > 0 {
            println!("  (plus {} images dues à des entrées extérieures, souris réelle ou focus : comptées à part)", self.quiet_external);
        }
        println!("  pendant ce temps : {} présences injectées ({FLOOD_RATE} par s, utilisateurs non affichés), {} ont demandé un redessin", self.flood.load(Ordering::SeqCst), self.flood_wakes.load(Ordering::SeqCst));
        println!("  et {} MESSAGE_UPDATE du salon ouvert hors de l'écran ({UPDATE_RATE} par s), {} ont demandé un redessin", self.updates.load(Ordering::SeqCst), self.update_wakes.load(Ordering::SeqCst));
        for c in &self.quiet_causes {
            println!("  cause : {c}");
        }
        let t = &report["trafic"];
        let line = |v: &Value| format!("{} paquets, {} événements dont {} ont réveillé, {} images", v["paquets"], v["evenements"], v["evenements_ayant_reveille"], v["images"]);
        println!("trafic de la passerelle rejoué (décodage et filtre des réveils de la connexion) :");
        println!("  rafale de démarrage, puis {} s : {}", TRAFFIC_SETTLE.as_secs_f64(), line(&t["rafale_de_demarrage"]));
        println!("  repos de {} s à {TRAFFIC_RATE} paquets par s dans des salons qui ne sont pas à l'écran : {}", self.cfg.quiet, line(&t["repos"]));
        if self.traffic.external > 0 {
            println!("  (plus {} images dues à des entrées extérieures, souris réelle ou focus : comptées à part)", self.traffic.external);
        }
        for c in &self.traffic.causes {
            println!("  cause : {c}");
        }
        println!("  paquets qui changent l'écran : {} ; {} sans réveil", line(&t["visibles"]), t["paquets_visibles_sans_reveil"]);
        let m = |v: &Value| format!("privée {} Mo, travail {} Mo", v["privee_mo"], v["travail_mo"]);
        println!("mémoire : départ {} ; après chargement {} ; fin {}", m(&report["memoire"]["depart"]), m(&report["memoire"]["apres_chargement"]), m(&report["memoire"]["fin"]));
        {
            let r = &report["memoire"]["repos_apres_demarrage"];
            let (c, k) = (&r["compteurs"], &r["carte"]);
            println!(
                "mémoire au repos après démarrage : privée {} Mo, de travail {} Mo (dont privée {} Mo) ; tas vivant {} Mo en {} blocs (seuil {} Mo)",
                c["privee_mo"], c["travail_mo"], c["travail_privee_mo"], r["tas_vivant_mo"], r["blocs_vivants"], r["seuil_tas_vivant_mo"]
            );
            println!(
                "  répartition de la mémoire privée : tas {} Mo (alloués {} Mo), réservations directes {} Mo, modules {} Mo, piles {} Mo ({} fils)",
                k["tas_engage_mo"], k["tas_alloue_mo"], k["autre_mo"], k["modules_mo"], k["piles_mo"], k["piles"]
            );
        }
        for (k, v) in report["memoire"]["phases"].as_object().into_iter().flatten() {
            println!("  {k} : {}", m(v));
        }
        println!("  tas vivant à la fin : {} Mo en {} blocs", report["memoire"]["tas_a_la_fin"]["vivant_mo"], report["memoire"]["tas_a_la_fin"]["blocs"]);
        println!("  caches à la fin : {}", report["memoire"]["caches_a_la_fin"]);
        println!("verdicts (budgets du 1.3) :");
        if let Some(v) = report["verdicts"].as_object() {
            for (k, x) in v {
                let detail = match (x.get("p95_ms").and_then(Value::as_f64), x.get("max_ms").and_then(Value::as_f64)) {
                    (Some(p), Some(m)) => format!(" (p95 {p:.2} ms, max {m:.2} ms)"),
                    _ => format!(" ({})", x["statistique"].as_str().unwrap_or_default()),
                };
                println!("  {k:<38} {}{detail}", x["verdict"].as_str().unwrap_or_default());
            }
        }
        let text = serde_json::to_string_pretty(&report).unwrap_or_default();
        match &self.cfg.out {
            Some(p) => match std::fs::write(p, &text) {
                Ok(()) => println!("rapport JSON : {}", p.display()),
                Err(e) => eprintln!("rapport JSON impossible ({}) : {e}", p.display()),
            },
            None => println!("{text}"),
        }
    }
}

/// Lancement du banc : magasin synthétique, vraie interface, fenêtre qui se ferme seule.
pub fn run(cfg: BenchCfg) -> eframe::Result {
    let mem_start = process_memory();
    let t_start = Instant::now();
    let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("runtime : {e}");
            std::process::exit(1);
        }
    };
    let syn = store(&cfg);
    if syn.filled.is_empty() || syn.deep.0 == 0 {
        eprintln!("--bench : pas assez de salons textuels à remplir");
        std::process::exit(2);
    }
    let inject: Arc<Mutex<Vec<Event>>> = Arc::default();
    let served: Served = Arc::default();
    let mut options = ferrocord::native_options();
    options.viewport = options.viewport.with_title("Ferrocord — banc de mesure");
    let theme = ThemeLink::open(None, false, WatchConfig::default());
    eframe::run_native(
        "Ferrocord banc",
        ferrocord::with_rendu(options),
        Box::new(move |cc| {
            let bench = Bench::new(cfg, &syn, inject.clone(), served.clone(), t_start, mem_start);
            let Synthetic { store, deep, .. } = syn;
            let pages = HashMap::from([(deep.0, deep.1)]);
            let backend = Offline { connected: true, queue: Vec::new(), wake: None, next_id: 5_000_000_000_000_001_000, view: View::Guild, inject: Some(inject), pages, served: Some(served), make: None };
            let wake_ctx = cc.egui_ctx.clone();
            let wake: Wake = Arc::new(move || wake_ctx.request_repaint());
            let images = Images::start(cc.egui_ctx.clone(), rt.handle(), Arc::new(Generated), None, images::Config::default(), wake);
            let mut app = App::new(cc, offline_boot(backend, store, theme, images));
            app.set_theme(Preset::Rouille.tokens());
            let mut bench = bench;
            bench.window_created();
            Ok(Box::new(BenchApp { app, bench, _rt: rt, closing: false }))
        }),
    )
}

struct BenchApp {
    app: App,
    bench: Bench,
    _rt: tokio::runtime::Runtime,
    closing: bool,
}

impl eframe::App for BenchApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        // Comme l'application (`App::raw_input_hook`) : atlas des glyphes borné.
        if !self.bench.cfg.free_atlas {
            ferrocord::ui::fonts::limit_atlas(raw);
        }
        self.bench.input(raw);
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.bench.before(frame, &mut self.app, ctx);
        eframe::App::logic(&mut self.app, ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        eframe::App::ui(&mut self.app, ui, frame);
        if self.closing {
            return;
        }
        let ctx = ui.ctx().clone();
        if self.bench.after(&mut self.app, &ctx) {
            self.closing = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

/// Scénarios de `--bench-mem` : une fenêtre qui vit quelques secondes, puis
/// la mémoire du processus et celle des textures d'egui.
pub fn run_mem(scenario: &str) -> eframe::Result {
    let start = process_memory();
    alloc::trace_from_env();
    let scenario = scenario.to_string();
    let heap_start = alloc::live();
    let mut options = ferrocord::native_options();
    options.viewport = options.viewport.with_title("Ferrocord — mesure mémoire");
    if scenario == "vide-logiciel" {
        options.glow_options.hardware_acceleration = eframe::egui_glow::HardwareAcceleration::Off;
    }
    let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("runtime : {e}");
            std::process::exit(1);
        }
    };
    eframe::run_native(
        "Ferrocord mesure mémoire",
        ferrocord::with_rendu(options),
        Box::new(move |cc| {
            // Fenêtre et contexte OpenGL créés, aucune image encore dessinée.
            let at_window = process_memory();
            let mut stages = vec![Stage::new("avant_la_fenetre", alloc::Heap::default(), heap_start), Stage::since("fenetre_et_opengl", heap_start)];
            let app = match scenario.as_str() {
                "demarrage-parties" => {
                    // Dissection : le magasin du compte simulé est rendu partie par partie.
                    let store = startup_store(&compte::Size::default(), &mut stages);
                    let mut mark = alloc::live();
                    let mut parts: Vec<(&'static str, i64, i64)> = Vec::new();
                    store.release_by_part(|name| {
                        let now = alloc::live();
                        parts.push((name, mark.bytes as i64 - now.bytes as i64, mark.blocks as i64 - now.blocks as i64));
                        mark = now;
                    });
                    parts.sort_by_key(|p| std::cmp::Reverse(p.1));
                    for (name, bytes, blocks) in parts.iter().filter(|p| p.1 >= 16 * 1024) {
                        println!("magasin.{name} : {:.2} Mo, {blocks} blocs", *bytes as f64 / 1_048_576.0);
                    }
                    None
                }
                s if s.starts_with("demarrage") => {
                    let size = match s {
                        "demarrage-demi" => compte::Size::default().scaled(1, 2),
                        "demarrage-double" => compte::Size::default().scaled(2, 1),
                        _ => compte::Size::default(),
                    };
                    let store = startup_store(&size, &mut stages);
                    let backend = Offline { connected: true, queue: Vec::new(), wake: None, next_id: 1, view: View::Guild, inject: None, pages: HashMap::new(), served: None, make: None };
                    let before = alloc::live();
                    alloc::reset_peak();
                    let wake_ctx = cc.egui_ctx.clone();
                    let wake: Wake = Arc::new(move || wake_ctx.request_repaint());
                    let images = Images::start(cc.egui_ctx.clone(), rt.handle(), Arc::new(Generated), None, images::Config::default(), wake);
                    let theme = ThemeLink::open(None, false, WatchConfig::default());
                    let mut app = App::new(cc, offline_boot(backend, store, theme, images));
                    app.set_theme(Preset::Rouille.tokens());
                    stages.push(Stage::since("application_polices_theme", before));
                    Some(app)
                }
                "app" | "app-sans-images" | "connexion" => {
                    let cfg = BenchCfg::default();
                    let store = if scenario == "connexion" { Store::new() } else { store(&cfg).store };
                    let backend = Offline { connected: scenario != "connexion", queue: Vec::new(), wake: None, next_id: 1, view: View::Guild, inject: None, pages: HashMap::new(), served: None, make: None };
                    let images = if scenario == "app" {
                        let wake_ctx = cc.egui_ctx.clone();
                        let wake: Wake = Arc::new(move || wake_ctx.request_repaint());
                        Images::start(cc.egui_ctx.clone(), rt.handle(), Arc::new(Generated), None, images::Config::default(), wake)
                    } else {
                        Images::disabled(cc.egui_ctx.clone())
                    };
                    let theme = ThemeLink::open(None, false, WatchConfig::default());
                    let mut app = App::new(cc, offline_boot(backend, store, theme, images));
                    app.set_theme(Preset::Rouille.tokens());
                    Some(app)
                }
                "vide-sans-polices" => {
                    cc.egui_ctx.set_fonts(egui::FontDefinitions::empty());
                    None
                }
                _ => None,
            };
            alloc::reset_peak();
            Ok(Box::new(MemApp { app, scenario, start, at_window, frames: 0, started: Instant::now(), done: false, _rt: rt, mark: alloc::live(), stages }))
        }),
    )
}

struct MemApp {
    app: Option<App>,
    scenario: String,
    start: Option<(u64, u64, u64)>,
    at_window: Option<(u64, u64, u64)>,
    frames: usize,
    started: Instant,
    done: bool,
    _rt: tokio::runtime::Runtime,
    /// Tas au début de l'étape en cours, et étapes déjà relevées (lot « ram »).
    mark: alloc::Heap,
    stages: Vec<Stage>,
}

/// Étape du démarrage : ce que le tas a gagné pendant elle (octets vivants,
/// blocs), la pointe atteinte au-dessus de son départ, les allocations faites.
struct Stage {
    name: &'static str,
    bytes: i64,
    blocks: i64,
    peak: i64,
    allocations: usize,
    /// Mémoire privée et de travail du processus à la fin de l'étape (octets).
    process: Option<(u64, u64)>,
    /// Répartition à la fin de l'étape : tas engagé, tas alloué, reste (octets).
    map: Option<(u64, u64, u64)>,
}

impl Stage {
    fn new(name: &'static str, before: alloc::Heap, after: alloc::Heap) -> Stage {
        Stage { name, bytes: after.bytes as i64 - before.bytes as i64, blocks: after.blocks as i64 - before.blocks as i64, peak: alloc::peak() as i64 - before.bytes as i64, allocations: after.total - before.total, process: usage().map(|u| (u.private, u.working)), map: stage_map() }
    }

    /// Étape commencée à `before` et finie maintenant.
    fn since(name: &'static str, before: alloc::Heap) -> Stage {
        Stage::new(name, before, alloc::live())
    }

    fn json(&self) -> Value {
        let m = |b: i64| (b as f64 / 1_048_576.0 * 100.0).round() / 100.0;
        json!({ "etape": self.name, "garde_mo": m(self.bytes), "blocs": self.blocks, "pointe_mo": m(self.peak), "allocations": self.allocations, "privee_mo": self.process.map(|p| mo(p.0)), "travail_mo": self.process.map(|p| mo(p.1)), "tas_engage_mo": self.map.map(|p| mo(p.0)), "tas_alloue_mo": self.map.map(|p| mo(p.1)), "autre_mo": self.map.map(|p| mo(p.2)) })
    }
}

/// Magasin du relevé « démarrage » : un READY de compte utilisateur et la liste
/// des jeux détectables passent par le vrai décodage, puis la dernière page du
/// salon ouvert. Chaque étape est relevée dans `stages`.
fn startup_store(size: &compte::Size, stages: &mut Vec<Stage>) -> Store {
    let mut store = Store::new();
    // READY : le texte du paquet est compté dans la pointe, pas dans ce qui reste.
    let before = alloc::live();
    alloc::reset_peak();
    {
        let d = compte::ready(size);
        let wake: Wake = Arc::new(|| {});
        let (mut replay, rx) = ferrocord::net::Replay::new(wake, Arc::default(), None);
        replay.dispatch("READY", &d);
        drop(d);
        store.apply(Event::PeopleHub(replay.people()));
        for e in rx.try_iter() {
            store.apply(e);
        }
    }
    store.take_commands();
    stages.push(Stage::since("ready_et_magasin", before));
    // Jeux détectables : réponse de Discord décodée, puis index.
    let before = alloc::live();
    alloc::reset_peak();
    {
        let body = compte::detectable(size.games);
        let index = ferrocord::net::decode_detectable_to_index(&body, chrono::Utc::now().timestamp_millis()).unwrap_or_default();
        drop(body);
        store.apply(Event::Activity(ferrocord::state::activity::ActivityEvent::Detectable(Arc::new(index))));
    }
    stages.push(Stage::since("jeux_detectables", before));
    // Salon ouvert : sa dernière page.
    let before = alloc::live();
    alloc::reset_peak();
    let (g, c) = (compte::guild_id(0), compte::channel_id(0, 5));
    store.select_guild(g);
    store.select_channel(c);
    store.take_commands();
    let now = chrono::Utc::now().timestamp_millis();
    let messages = (0..HISTORY_PAGE as u64).map(|i| message(i, c, g, now - (HISTORY_PAGE as i64 - i as i64) * 90_000)).collect();
    store.apply(Event::History { channel_id: c, before: None, messages });
    store.take_commands();
    stages.push(Stage::since("salon_ouvert", before));
    store
}

impl eframe::App for MemApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        // Comme l'application (`App::raw_input_hook`) : atlas des glyphes borné.
        ferrocord::ui::fonts::limit_atlas(raw);
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if let Some(app) = self.app.as_mut() {
            eframe::App::logic(app, ctx, frame);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if let Some(app) = self.app.as_mut() {
            eframe::App::ui(app, ui, frame);
        }
        self.frames += 1;
        let ctx = ui.ctx().clone();
        if self.done {
            return;
        }
        if self.frames == 1 {
            self.stages.push(Stage::since("premiere_image", self.mark));
            self.mark = alloc::live();
            alloc::reset_peak();
        }
        // Quelques secondes d'images, puis la mesure.
        if self.started.elapsed() < Duration::from_secs(3) {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        self.done = true;
        self.stages.push(Stage::since("images_suivantes_3_s", self.mark));
        let heap = alloc::live();
        let mem = process_memory();
        let atlas = ctx.fonts(|f| f.font_image_size());
        let tex = ctx.tex_manager();
        let (count, bytes) = tex.read().allocated().fold((0, 0), |(n, b), (_, m)| (n + 1, b + m.size[0] * m.size[1] * m.bytes_per_pixel));
        let store_bytes = self.app.as_ref().map(|a| store_size(a.store()));
        // Compte simulé : seuil sur les octets vivants (ni le pilote ni la machine n'y entrent).
        let over = self.scenario.starts_with("demarrage").then_some(heap.bytes > START_HEAP_MAX);
        let report = json!({
            "scenario": self.scenario,
            "rendu": ferrocord::rendu::active().name(),
            "depart": mem_json(self.start),
            "fenetre_creee": mem_json(self.at_window),
            "apres_3_s": mem_json(mem),
            "images_dessinees": self.frames,
            "atlas_polices_px": atlas,
            "textures": { "nombre": count, "mo": mo(bytes as u64) },
            "magasin_estime_mo": store_bytes.map(|b| mo(b as u64)),
            "tas": { "vivant_mo": mo(heap.bytes as u64), "blocs": heap.blocks, "allocations_depuis_le_debut": heap.total },
            "seuil_tas_vivant_mo": over.map(|_| mo(START_HEAP_MAX as u64)),
            "verdict": over.map(|o| if o { "non tenu" } else { "tenu" }),
            "etapes": self.stages.iter().map(Stage::json).collect::<Vec<_>>(),
            "caches": self.app.as_ref().map(|a| {
                let (layouts, layout_bytes) = a.text_layouts();
                let img = a.image_stats();
                json!({ "mises_en_page": { "nombre": layouts, "mo": mo(layout_bytes as u64) }, "textures_des_images": { "nombre": img.textures, "mo": mo(img.bytes as u64) } })
            }),
            "fils": thread_count(),
            "compteurs": usage_json(),
            "carte": map_json(),
        });
        // Gros blocs encore vivants et qui les a demandés (`FERROCORD_BANC_TRACE`).
        for (bytes, blocks, stack) in alloc::survivors().into_iter().take(60) {
            eprintln!("{:.2} Mo en {blocks} blocs : {stack}", bytes as f64 / 1_048_576.0);
        }
        println!("{}", serde_json::to_string(&report).unwrap_or_default());
        if over == Some(true) {
            eprintln!("mémoire au repos après démarrage : {:.1} Mo vivants dans le tas, au-dessus du seuil de {:.0} Mo", heap.bytes as f64 / 1_048_576.0, START_HEAP_MAX as f64 / 1_048_576.0);
            std::process::exit(1);
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

/// Taille estimée du magasin (structures et chaînes), sans les tables de hachage internes.
pub fn store_size(s: &Store) -> usize {
    use std::mem::size_of;
    let mut b = 0;
    for g in &s.guilds {
        b += size_of::<Guild>() + g.name.capacity();
        for c in &g.channels {
            b += size_of::<Channel>() + c.name.capacity() + c.overwrites.as_ref().map_or(0, |o| o.capacity() * 48);
        }
        b += g.roles.capacity() * 64;
    }
    for cs in s.chans.values() {
        for m in &cs.msgs {
            b += size_of::<Message>() + m.author.capacity() + m.content.capacity();
            b += m.attachments.iter().map(|a| size_of::<Attachment>() + a.name.capacity() + a.url.capacity() + a.proxy_url.capacity()).sum::<usize>();
            let text = |s: &Option<String>| s.as_ref().map_or(0, String::capacity);
            b += m.embeds.iter().map(|e| size_of::<Embed>() + text(&e.title) + text(&e.description) + text(&e.url)).sum::<usize>();
        }
    }
    b
}

/// Démarrage du banc : réglages et coffre en mémoire (aucun fichier lu ni
/// écrit), aucun compte, aucun ancien fichier de jeton, aucun `DISCORD_TOKEN`.
fn offline_boot(backend: Offline, store: Store, theme: (ThemeLink, Option<ferrocord::theme::Update>), images: Images) -> Boot {
    Boot {
        backend: Box::new(backend),
        store,
        theme,
        settings: SettingsLink::open(None, false, WatchConfig::default()),
        accounts: AccountLink::start(Arc::new(MemoryVault::new())),
        env_token: None,
        legacy: None,
        images,
    }
}
