//! Banc d'usage prolongé (lot « long ») : `preview --bench-long [visites]`.
//!
//! Les autres bancs regardent le démarrage ou quelques centaines d'images.
//! Celui-ci cherche ce qui **s'accumule** : il rejoue en accéléré, sur le
//! compte simulé (`compte`, READY de 100 serveurs décodé par le vrai
//! décodage), plusieurs tours identiques d'un usage de plusieurs heures :
//!
//! 1. `visites` salons ouverts à la suite (500 par défaut), tous différents,
//!    quelques images chacun, avec une remontée dans l'historique (une page de
//!    plus demandée) : chaque salon reçoit ses pages par le chemin habituel
//!    (`Cmd::History`), avec des messages variés, des images et des embeds ;
//! 2. deux heures d'événements de la passerelle au rythme mesuré sur un vrai
//!    compte (3 paquets par seconde, soit 21 600 paquets), passés par le vrai
//!    décodage et le vrai filtre des réveils : messages dans le salon affiché,
//!    dans des salons déjà visités et dans des salons jamais ouverts,
//!    modifications, suppressions, réactions, « écrit… », présences de 20 000
//!    personnes différentes, salons renommés ;
//! 3. 25 reconnexions : RESUMED, et un READY complet une fois sur cinq.
//!
//! À la fin de chaque tour, retour au salon de départ, puis un relevé : octets
//! vivants du tas (allocateur compteur, exact), mémoire privée et de travail,
//! tas engagé, caches (mises en page, textures), salons gardés par le magasin,
//! handles et fils du processus. Les trois premiers tours remplissent les
//! caches jusqu'à leurs plafonds ; les suivants ne doivent plus rien ajouter.
//! Verdicts (seuils larges, code de sortie 1 s'ils ne sont pas tenus) : entre
//! le troisième et le dernier tour, le tas vivant, la mémoire privée, les
//! handles et les fils ne grandissent pas.
//!
//! Ce que le banc ne dit pas : la mémoire du pilote graphique d'une autre
//! machine, et le vrai compte (formes et volumes réels).

use super::super::{Generated, Offline, View, INTEREST};
use super::{alloc, compte, message, mo, offline_boot, packet_channel, packet_delete, packet_embeds, packet_update, snowflake, store_size};
use eframe::egui;
use ferrocord::images::{self, Images};
use ferrocord::state::model::{Message, DISCORD_EPOCH};
use ferrocord::state::{Event, Store, HISTORY_PAGE};
use ferrocord::theme::{Preset, ThemeLink, WatchConfig};
use ferrocord::ui::{App, Wake};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    sync::{mpsc, Arc, Mutex},
};

/// Tours identiques. Les premiers remplissent les caches jusqu'à leurs
/// plafonds (16 Mo de textures : atteints au troisième tour) ; les verdicts
/// comparent le dernier tour à celui de [`FULL_LAP`].
const LAPS: usize = 5;
/// Tour à partir duquel les caches sont pleins.
const FULL_LAP: usize = 3;
/// Images passées dans chaque salon visité.
const VISIT_FRAMES: usize = 4;
/// Deux heures à 3 paquets par seconde.
const PACKETS: usize = 21_600;
/// Paquets décodés par image pendant la phase « événements ».
const PER_FRAME: usize = 150;
const RECONNECTS: usize = 25;
/// Images entre deux reconnexions, et de repos avant un relevé.
const RECONNECT_FRAMES: usize = 4;
const REST_FRAMES: usize = 90;
const WARM_FRAMES: usize = 60;
/// Personnes différentes des présences et des « écrit… ».
const PEOPLE: u64 = 20_000;

/// Croissance tolérée entre le troisième et le dernier tour. Le tas vivant est
/// exact ; la mémoire privée bouge de ± 5 Mo d'un relevé à l'autre (pilote).
const HEAP_GROWTH_MAX: i64 = 3 << 20;
const PRIVATE_GROWTH_MAX: i64 = 16 << 20;
const HANDLE_GROWTH_MAX: i64 = 24;
const THREAD_GROWTH_MAX: i64 = 2;
/// Tas vivant toléré à la fin (caches pleins : mises en page, images en
/// attente, 20 salons de messages). Mesuré : voir `docs/mesures.md`.
const HEAP_END_MAX: usize = 40 << 20;

pub struct LongCfg {
    /// Salons visités par tour (500 pour la mesure de référence).
    pub visits: usize,
}

/// Page d'historique d'un salon du compte simulé, fabriquée à la demande
/// (rien n'est gardé par le banc) : la dernière page, ou celle d'avant
/// `before`, sur deux jours au plus.
pub fn history_page(channel: u64, before: Option<u64>) -> Vec<Message> {
    const STEP: i64 = 90_000;
    let now = chrono::Utc::now().timestamp_millis();
    let base = before.map_or(now, |b| ((b >> 22) + DISCORD_EPOCH) as i64 - 1);
    if base < now - 2 * 86_400_000 {
        return Vec::new();
    }
    let guild = compte::guild_of(channel);
    let n = HISTORY_PAGE as i64;
    let mut page: Vec<Message> = (0..n).map(|j| message((base / STEP + j) as u64, channel, guild, base - (n - j) * STEP)).collect();
    // Comme `serve_page` : une page d'avant `before` arrive du plus récent au plus ancien.
    if before.is_some() {
        page.reverse();
    }
    page
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Step {
    Start,
    Warm,
    Visits,
    Events,
    Reconnect,
    Rest,
    Done,
}

struct Sample {
    name: String,
    heap: alloc::Heap,
    private: u64,
    working: u64,
    private_working: u64,
    heaps: u64,
    heaps_allocated: u64,
    layouts: (usize, usize),
    images: (usize, usize, usize),
    egui_textures: (usize, usize),
    store: usize,
    channels_kept: usize,
    messages_kept: usize,
    handles: Option<i64>,
    threads: Option<i64>,
}

impl Sample {
    fn json(&self) -> Value {
        json!({
            "releve": self.name,
            "tas_vivant_mo": mo(self.heap.bytes as u64), "blocs_vivants": self.heap.blocks, "allocations_depuis_le_debut": self.heap.total,
            "privee_mo": mo(self.private), "travail_mo": mo(self.working), "travail_privee_mo": mo(self.private_working),
            "tas_engage_mo": mo(self.heaps), "tas_alloue_mo": mo(self.heaps_allocated),
            "mises_en_page": { "nombre": self.layouts.0, "mo": mo(self.layouts.1 as u64) },
            "textures_des_images": { "nombre": self.images.0, "mo": mo(self.images.1 as u64), "en_attente": self.images.2 },
            "textures_egui": { "nombre": self.egui_textures.0, "mo": mo(self.egui_textures.1 as u64) },
            "magasin_estime_mo": mo(self.store as u64), "salons_gardes": self.channels_kept, "messages_gardes": self.messages_kept,
            "handles": self.handles, "fils": self.threads,
        })
    }
}

/// Handles et fils du processus (PowerShell, comme la mémoire du banc : ni
/// dépendance ni `unsafe`).
fn handles_and_threads() -> (Option<i64>, Option<i64>) {
    let cmd = format!("$p = Get-Process -Id {}; \"$($p.HandleCount) $($p.Threads.Count)\"", std::process::id());
    let Ok(out) = std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &cmd]).output() else {
        return (None, None);
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut it = text.split_whitespace().map(|v| v.parse::<i64>().ok());
    (it.next().flatten(), it.next().flatten())
}

#[cfg(windows)]
fn counters() -> (u64, u64, u64, u64, u64) {
    let u = ferrocord::perf::memory::usage().unwrap_or_default();
    let m = ferrocord::perf::memory::map();
    (u.private, u.working, u.private_working, m.heaps, m.heaps_allocated)
}

#[cfg(not(windows))]
fn counters() -> (u64, u64, u64, u64, u64) {
    (0, 0, 0, 0, 0)
}

struct LongApp {
    app: App,
    _rt: tokio::runtime::Runtime,
    cfg: LongCfg,
    inject: Arc<Mutex<Vec<Event>>>,
    replay: Option<(ferrocord::net::Replay, mpsc::Receiver<Event>)>,
    ready: String,
    games: String,
    step: Step,
    frames: usize,
    lap: usize,
    /// Visites faites depuis le début (les tours ne revisitent pas les mêmes salons).
    visit: usize,
    sent: usize,
    reconnects: usize,
    seq: u64,
    /// Derniers messages créés par le banc : (salon, serveur, message).
    recent: VecDeque<(u64, u64, u64)>,
    /// Derniers salons visités (encore gardés par le magasin).
    visited: VecDeque<(u64, u64)>,
    wheel: bool,
    center: Option<egui::Pos2>,
    samples: Vec<Sample>,
    packets_total: usize,
    ready_total: usize,
    /// Époque de l'allocateur ouverte après le premier tour.
    epoch: usize,
    /// Temps d'image (ms) pendant les visites de salons et pendant les événements.
    visit_ms: Vec<f64>,
    event_ms: Vec<f64>,
    last_step: Step,
    closing: bool,
}

fn home() -> (u64, u64) {
    (compte::guild_id(0), compte::channel_id(0, 5))
}

impl LongApp {
    /// Salon numéro `v` : tous différents tant que `v` reste sous le nombre
    /// de salons textuels du compte ; le serveur change toutes les neuf visites.
    fn target(v: usize) -> (u64, u64) {
        let size = compte::Size::default();
        let text = size.channels.saturating_sub(5).max(1);
        let g = (v / 9 + 1) % size.guilds;
        let c = 5 + (v % 9 + 9 * (v / (9 * size.guilds))) % text;
        (compte::guild_id(g), compte::channel_id(g, c))
    }

    fn fresh(&mut self) -> u64 {
        self.seq += 1;
        snowflake(chrono::Utc::now().timestamp_millis(), self.seq)
    }

    fn dispatch(&mut self, kind: &str, d: &str) {
        let Some((replay, rx)) = &mut self.replay else { return };
        replay.dispatch(kind, d);
        let events: Vec<Event> = rx.try_iter().collect();
        if let Ok(mut q) = self.inject.lock() {
            q.extend(events);
        }
    }

    /// Premier READY : par le vrai décodage, appliqué par l'interface comme
    /// à la connexion ; puis les jeux détectables.
    fn connect(&mut self) {
        let wake: Wake = Arc::new(|| {});
        let (replay, rx) = ferrocord::net::Replay::new(wake, INTEREST.get().cloned().unwrap_or_default(), None);
        if let Ok(mut q) = self.inject.lock() {
            q.push(Event::PeopleHub(replay.people()));
        }
        self.replay = Some((replay, rx));
        let ready = std::mem::take(&mut self.ready);
        self.dispatch("READY", &ready);
        self.ready = ready;
        let games = std::mem::take(&mut self.games);
        let index = ferrocord::net::decode_detectable_to_index(&games, chrono::Utc::now().timestamp_millis()).unwrap_or_default();
        if let Ok(mut q) = self.inject.lock() {
            q.push(Event::Activity(ferrocord::state::activity::ActivityEvent::Detectable(Arc::new(index))));
        }
    }

    /// Paquet numéro `k` des deux heures d'événements.
    fn packet(&mut self, k: usize) {
        let id = self.fresh();
        // Cible : le salon affiché, un salon déjà visité (encore en mémoire), un salon jamais ouvert.
        let (guild, channel) = match k % 4 {
            0 => home(),
            1 | 2 => self.visited.get(k % self.visited.len().max(1)).copied().unwrap_or_else(home),
            _ => {
                let g = 1 + k % 90;
                (compte::guild_id(g), compte::channel_id(g, 40 + k % 5))
            }
        };
        let user = 2_000_000_000_000_000_000 + (k as u64).wrapping_mul(7919) % PEOPLE;
        let old = self.recent.get(k % self.recent.len().max(1)).copied();
        match k % 10 {
            0..=2 => {
                let text = match k % 7 {
                    0 => format!("message {k} avec **du gras**, un lien https://exemple.invalid/p/{k} et `du code`"),
                    1 => "ok".to_string(),
                    _ => format!("message numéro {k} du banc d'usage prolongé, ni trop court ni trop long"),
                };
                let d = json!({
                    "id": id.to_string(), "channel_id": channel.to_string(), "guild_id": guild.to_string(),
                    "author": { "id": user.to_string(), "username": format!("personne_{}", user % PEOPLE), "global_name": format!("Personne {}", user % PEOPLE) },
                    "member": { "roles": [], "nick": null, "joined_at": "2021-03-04T10:11:12.000000+00:00" },
                    "content": text, "type": 0, "mentions": [], "mention_roles": [], "mention_everyone": false,
                    "attachments": [], "embeds": [], "pinned": false, "flags": 0, "timestamp": "2026-01-01T00:00:00.000000+00:00",
                })
                .to_string();
                self.dispatch("MESSAGE_CREATE", &d);
                self.recent.push_back((channel, guild, id));
                if self.recent.len() > 64 {
                    self.recent.pop_front();
                }
            }
            3 => {
                if let Some((c, g, m)) = old {
                    self.dispatch("MESSAGE_UPDATE", &packet_update(c, g, m, &format!("modifié {k}")));
                }
            }
            4 => {
                if let Some((c, g, m)) = old {
                    self.dispatch("MESSAGE_UPDATE", &packet_embeds(c, g, m));
                }
            }
            5 => {
                if let Some((c, g, m)) = self.recent.pop_front() {
                    self.dispatch("MESSAGE_DELETE", &packet_delete(c, g, m));
                }
            }
            6 => {
                let d = json!({ "channel_id": channel.to_string(), "guild_id": guild.to_string(), "user_id": user.to_string(), "timestamp": 1_700_000_000, "member": { "user": { "id": user.to_string(), "username": format!("personne_{}", user % PEOPLE) }, "roles": [], "nick": null } }).to_string();
                self.dispatch("TYPING_START", &d);
            }
            7 => {
                let activities = if k.is_multiple_of(3) { json!([{ "name": format!("Jeu inventé numéro {}", k % 500), "type": 0, "created_at": 1_700_000_000_000u64 }]) } else { json!([]) };
                let d = json!({ "user": { "id": user.to_string() }, "guild_id": guild.to_string(), "status": if k.is_multiple_of(2) { "online" } else { "idle" }, "client_status": { "desktop": "online" }, "activities": activities }).to_string();
                self.dispatch("PRESENCE_UPDATE", &d);
            }
            8 => {
                if let Some((c, g, m)) = old {
                    let d = json!({ "user_id": user.to_string(), "channel_id": c.to_string(), "message_id": m.to_string(), "guild_id": g.to_string(), "emoji": { "id": null, "name": "👍" }, "type": 0 }).to_string();
                    self.dispatch("MESSAGE_REACTION_ADD", &d);
                }
            }
            _ => {
                let g = 1 + k % 90;
                self.dispatch("CHANNEL_UPDATE", &packet_channel(compte::guild_id(g), compte::channel_id(g, 7), compte::channel_id(g, 2), &format!("salon-renommé-{}", k % 50)));
            }
        }
        self.packets_total += 1;
    }

    fn sample(&mut self, name: String, ctx: &egui::Context) {
        // Le texte du READY que le banc garde pour le rejouer n'est pas à l'application.
        let mut heap = alloc::live();
        heap.bytes = heap.bytes.saturating_sub(self.ready.capacity());
        let (private, working, private_working, heaps, heaps_allocated) = counters();
        let tex = ctx.tex_manager();
        let egui_textures = tex.read().allocated().fold((0, 0), |(n, b), (_, m)| (n + 1, b + m.size[0] * m.size[1] * m.bytes_per_pixel));
        let img = self.app.image_stats();
        let store: &Store = self.app.store();
        let (handles, threads) = handles_and_threads();
        let s = Sample {
            name,
            heap,
            private,
            working,
            private_working,
            heaps,
            heaps_allocated,
            layouts: self.app.text_layouts(),
            images: (img.textures, img.bytes, img.pending),
            egui_textures,
            store: store_size(store),
            channels_kept: store.chans.len(),
            messages_kept: store.chans.values().map(|c| c.msgs.len()).sum(),
            handles,
            threads,
        };
        eprintln!("{}", s.json());
        self.samples.push(s);
    }

    /// Avant la logique de l'image : ce que l'« utilisateur » et la
    /// « passerelle » font pendant cette image.
    fn drive(&mut self, ctx: &egui::Context) {
        self.frames += 1;
        match self.step {
            Step::Start => {
                self.connect();
                self.step = Step::Warm;
                self.frames = 0;
            }
            Step::Warm => {
                if self.frames == 3 {
                    let (g, c) = home();
                    self.app.show_channel(Some(g), c);
                }
                if self.frames >= WARM_FRAMES {
                    self.sample("depart".into(), ctx);
                    self.enter(Step::Visits);
                }
            }
            Step::Visits => {
                let at = (self.frames - 1) % VISIT_FRAMES;
                if at == 0 {
                    let done = (self.frames - 1) / VISIT_FRAMES;
                    if done >= self.cfg.visits {
                        self.enter(Step::Events);
                        return;
                    }
                    let (g, c) = Self::target(self.visit);
                    self.visit += 1;
                    self.app.show_channel(Some(g), c);
                    self.visited.push_back((g, c));
                    if self.visited.len() > 12 {
                        self.visited.pop_front();
                    }
                } else if at == 2 {
                    // Remontée : la page d'avant est demandée.
                    self.wheel = true;
                }
            }
            Step::Events => {
                if self.frames == 1 {
                    let (g, c) = home();
                    self.app.show_channel(Some(g), c);
                }
                for _ in 0..PER_FRAME {
                    if self.sent >= PACKETS {
                        self.enter(Step::Reconnect);
                        return;
                    }
                    let k = self.sent;
                    self.sent += 1;
                    self.packet(k);
                }
            }
            Step::Reconnect => {
                if (self.frames - 1).is_multiple_of(RECONNECT_FRAMES) {
                    if self.reconnects >= RECONNECTS {
                        self.enter(Step::Rest);
                        return;
                    }
                    self.reconnects += 1;
                    if self.reconnects.is_multiple_of(5) {
                        let ready = std::mem::take(&mut self.ready);
                        self.dispatch("READY", &ready);
                        self.ready = ready;
                        self.ready_total += 1;
                    } else {
                        self.dispatch("RESUMED", "{}");
                    }
                } else if (self.frames - 1) % RECONNECT_FRAMES == 2 {
                    // Après un READY, le salon se rouvre comme le fait l'utilisateur.
                    let (g, c) = home();
                    self.app.show_channel(Some(g), c);
                }
            }
            Step::Rest => {
                if self.frames == 1 {
                    let (g, c) = home();
                    self.app.show_channel(Some(g), c);
                }
                if self.frames >= REST_FRAMES {
                    self.lap += 1;
                    self.sample(format!("tour_{}", self.lap), ctx);
                    if self.lap == 1 {
                        self.epoch = alloc::mark();
                    }
                    if self.lap >= LAPS {
                        self.step = Step::Done;
                    } else {
                        self.enter(Step::Visits);
                    }
                }
            }
            Step::Done => {}
        }
    }

    fn enter(&mut self, step: Step) {
        self.step = step;
        self.frames = 0;
        self.sent = 0;
        self.reconnects = 0;
    }

    fn report(&self) -> bool {
        let first = self.samples.iter().find(|s| s.name == format!("tour_{FULL_LAP}"));
        let last = self.samples.last();
        let (Some(a), Some(b)) = (first, last) else { return false };
        let growth = |x: u64, y: u64| y as i64 - x as i64;
        let heap = growth(a.heap.bytes as u64, b.heap.bytes as u64);
        let private = growth(a.private, b.private);
        let handles = a.handles.zip(b.handles).map(|(x, y)| y - x);
        let threads = a.threads.zip(b.threads).map(|(x, y)| y - x);
        let verdict = |ok: bool| if ok { "tenu" } else { "non tenu" };
        let ok = [heap <= HEAP_GROWTH_MAX, private <= PRIVATE_GROWTH_MAX, handles.is_none_or(|h| h <= HANDLE_GROWTH_MAX), threads.is_none_or(|t| t <= THREAD_GROWTH_MAX), b.heap.bytes <= HEAP_END_MAX];
        let signed = |v: i64| (v as f64 / 1_048_576.0 * 10.0).round() / 10.0;
        let report = json!({
            "scenario": { "tours": LAPS, "salons_visites_par_tour": self.cfg.visits, "salons_visites": self.visit, "paquets_par_tour": PACKETS, "paquets": self.packets_total, "reconnexions_par_tour": RECONNECTS, "ready_complets": self.ready_total },
            "rendu": ferrocord::rendu::active().name(),
            "releves": self.samples.iter().map(Sample::json).collect::<Vec<_>>(),
            "images_ms": { "visites_de_salons": super::stats(&self.visit_ms), "evenements": super::stats(&self.event_ms) },
            "croissance_du_tour_3_au_dernier": { "tas_vivant_mo": signed(heap), "privee_mo": signed(private), "blocs": b.heap.blocks as i64 - a.heap.blocks as i64, "handles": handles, "fils": threads },
            "verdicts": {
                "tas_vivant_plafonne": { "verdict": verdict(ok[0]), "croissance_mo": signed(heap), "seuil_mo": signed(HEAP_GROWTH_MAX) },
                "memoire_privee_plafonne": { "verdict": verdict(ok[1]), "croissance_mo": signed(private), "seuil_mo": signed(PRIVATE_GROWTH_MAX) },
                "handles_stables": { "verdict": verdict(ok[2]), "croissance": handles, "seuil": HANDLE_GROWTH_MAX },
                "fils_stables": { "verdict": verdict(ok[3]), "croissance": threads, "seuil": THREAD_GROWTH_MAX },
                "tas_vivant_a_la_fin": { "verdict": verdict(ok[4]), "mo": mo(b.heap.bytes as u64), "seuil_mo": mo(HEAP_END_MAX as u64) },
            },
        });
        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
        println!("=== Banc d'usage prolongé : {LAPS} tours de {} salons, {PACKETS} paquets, {RECONNECTS} reconnexions ===", self.cfg.visits);
        for s in &self.samples {
            println!(
                "{:<8} tas vivant {:>5.1} Mo ({} blocs) ; privée {:>6.1} Mo ; travail privée {:>6.1} Mo ; tas engagé {:>6.1} Mo ; mises en page {:>4.1} Mo ; textures {:>4.1} Mo ; salons gardés {} ; handles {} ; fils {}",
                s.name,
                mo(s.heap.bytes as u64),
                s.heap.blocks,
                mo(s.private),
                mo(s.private_working),
                mo(s.heaps),
                mo(s.layouts.1 as u64),
                mo(s.images.1 as u64),
                s.channels_kept,
                s.handles.map_or("?".into(), |h| h.to_string()),
                s.threads.map_or("?".into(), |t| t.to_string()),
            );
        }
        for (name, key) in [("visites de salons", "visites_de_salons"), ("événements", "evenements")] {
            let v = &report["images_ms"][key];
            println!("image pendant les {name} : médiane {} ms, p95 {} ms, maximum {} ms ({} images)", v["mediane_ms"], v["p95_ms"], v["max_ms"], v["n"]);
        }
        if let Some(v) = report["verdicts"].as_object() {
            for (k, x) in v {
                println!("  {k:<26} {}", x["verdict"].as_str().unwrap_or_default());
            }
        }
        // Blocs demandés après le premier tour et encore vivants, et qui les a
        // demandés (`FERROCORD_BANC_TRACE`, exécutable avec symboles) : ce
        // qu'un tour de plus laisse derrière lui.
        for (bytes, blocks, stack) in alloc::survivors_since(self.epoch).into_iter().take(40) {
            eprintln!("{:.2} Mo en {blocks} blocs : {stack}", bytes as f64 / 1_048_576.0);
        }
        ok.iter().all(|o| *o)
    }
}

impl eframe::App for LongApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        ferrocord::ui::fonts::limit_atlas(raw);
        // Entrées réelles ignorées : le banc ne dépend pas de la souris.
        raw.events.retain(|e| {
            !matches!(
                e,
                egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. } | egui::Event::MouseMoved(_) | egui::Event::MouseWheel { .. } | egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::PointerGone
            )
        });
        if std::mem::take(&mut self.wheel)
            && let Some(c) = self.center
        {
            raw.events.push(egui::Event::PointerMoved(c));
            raw.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 4000.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: egui::Modifiers::NONE,
            });
        }
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Temps de l'image précédente mesuré par eframe (mise en page,
        // tessellation, envoi des textures), rangé avec l'étape qui l'a produite.
        if let Some(ms) = frame.info().cpu_usage.map(|s| f64::from(s) * 1000.0) {
            match self.last_step {
                Step::Visits => self.visit_ms.push(ms),
                Step::Events => self.event_ms.push(ms),
                _ => {}
            }
        }
        self.last_step = self.step;
        if !self.closing {
            self.drive(ctx);
        }
        eframe::App::logic(&mut self.app, ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        eframe::App::ui(&mut self.app, ui, frame);
        let ctx = ui.ctx().clone();
        if self.center.is_none() {
            let r = ctx.content_rect();
            self.center = Some(egui::pos2(r.max.x * 0.65, r.max.y * 0.5));
        }
        if self.closing {
            return;
        }
        if self.step == Step::Done {
            self.closing = true;
            let ok = self.report();
            if !ok {
                eprintln!("usage prolongé : un verdict n'est pas tenu");
                std::process::exit(1);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        ctx.request_repaint();
    }
}

pub fn run(cfg: LongCfg) -> eframe::Result {
    alloc::trace_from_env();
    let size = compte::Size::default();
    let (ready, games) = (compte::ready(&size), compte::detectable(size.games));
    let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("runtime : {e}");
            std::process::exit(1);
        }
    };
    let mut options = ferrocord::native_options();
    options.viewport = options.viewport.with_title("Ferrocord — banc d'usage prolongé");
    // Sans attente de l'écran : des milliers d'images en quelques dizaines de secondes.
    options.glow_options.vsync = false;
    let theme = ThemeLink::open(None, false, WatchConfig::default());
    eframe::run_native(
        "Ferrocord banc long",
        ferrocord::with_rendu(options),
        Box::new(move |cc| {
            let inject: Arc<Mutex<Vec<Event>>> = Arc::default();
            let backend = Offline { connected: true, queue: Vec::new(), wake: None, next_id: 1, view: View::Guild, inject: Some(inject.clone()), pages: HashMap::new(), served: None, make: Some(history_page) };
            let wake_ctx = cc.egui_ctx.clone();
            let wake: Wake = Arc::new(move || wake_ctx.request_repaint());
            let images = Images::start(cc.egui_ctx.clone(), rt.handle(), Arc::new(Generated), None, images::Config::default(), wake);
            let mut app = App::new(cc, offline_boot(backend, Store::new(), theme, images));
            app.set_theme(Preset::Rouille.tokens());
            Ok(Box::new(LongApp {
                app,
                _rt: rt,
                cfg,
                inject,
                replay: None,
                ready,
                games,
                step: Step::Start,
                frames: 0,
                lap: 0,
                visit: 0,
                sent: 0,
                reconnects: 0,
                seq: 0x80_0000,
                recent: VecDeque::new(),
                visited: VecDeque::new(),
                wheel: false,
                center: None,
                samples: Vec::new(),
                packets_total: 0,
                ready_total: 0,
                epoch: 0,
                visit_ms: Vec::new(),
                event_ms: Vec::new(),
                last_step: Step::Start,
                closing: false,
            }))
        }),
    )
}
