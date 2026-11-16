//! Banc du processeur (lot « cpu ») : `preview --bench-cpu <secondes>`.
//!
//! Rejoue le démarrage d'un compte **simulé** comme le fait la connexion :
//! la fenêtre s'ouvre sur un magasin vide, puis un fil joue la passerelle
//! (READY de 100 serveurs décodé par le vrai décodage, `net::Replay`), le
//! salon s'ouvre, sa dernière page arrive. Ce qui est mesuré, pour tout le
//! processus (`GetProcessTimes` : interface, pilote graphique, décodage) :
//!
//! - **processeur consommé du lancement jusqu'au calme** et **délai jusqu'au
//!   calme**, avec la règle de `tools/mesures.ps1` : première fenêtre de 2 s
//!   sous 5 % d'un cœur, relevés de 250 ms ;
//! - le READY : durée du décodage, allocations, image qui l'applique ;
//! - la liste des jeux détectables (décodée après le calme, comme en vrai) ;
//! - **processeur au repos** (sans trafic), puis **sous trafic simulé**
//!   (3 paquets par seconde dans des salons qui ne sont pas à l'écran) :
//!   moyenne, **pointe sur 1 s**, images dessinées ;
//! - le coût d'un paquet de chaque sorte courante (µs et allocations).
//!
//! Le texte des paquets est fabriqué dans un fil à part avant la fenêtre ;
//! son temps processeur est retranché des totaux.

use super::super::{Generated, Offline, View, INTEREST};
use super::{alloc, compte, message, offline_boot, packet_channel, packet_create, packet_delete, packet_embeds, packet_update, snowflake};
use eframe::egui;
use ferrocord::images::{self, Images};
use ferrocord::perf::threads::{by_name, process_cpu_ms};
use ferrocord::state::{Event, Store, HISTORY_PAGE};
use ferrocord::theme::{Preset, ThemeLink, WatchConfig};
use ferrocord::ui::{App, Wake};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

/// Règle du calme de `tools/mesures.ps1` : 5 % d'un cœur sur 2 s, relevés de 250 ms.
const CALM_WINDOW: Duration = Duration::from_secs(2);
const CALM_STEP: Duration = Duration::from_millis(250);
const CALM_MS: f64 = 100.0;
/// Au-delà, le calme est déclaré non atteint.
const CALM_CAP: Duration = Duration::from_secs(40);
/// Paquets par seconde au repos sous trafic (mesuré sur un vrai compte : environ 3).
const RATE: f64 = 3.0;
/// Serveurs touchés par le trafic.
const FAR: usize = 40;
/// Paquets de chaque sorte du relevé « coût d'un paquet ».
const EACH: usize = 2000;

/// Étapes de `Shared::step` (0 : rien à faire) : salon à ouvrir, salon ouvert.
const OPEN: usize = 1;
const OPENED: usize = 2;

#[derive(Default)]
struct Shared {
    frames: AtomicUsize,
    /// Durée de la construction de l'image la plus longue depuis la dernière remise à zéro (µs).
    frame_max: AtomicU64,
    step: AtomicUsize,
    done: AtomicBool,
}

fn round(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Âge du processus (ms) : l'origine des délais est sa création par le système.
fn age_ms(fallback: Instant) -> f64 {
    #[cfg(windows)]
    if let Some(a) = ferrocord::perf::process::age_ms() {
        return a;
    }
    fallback.elapsed().as_secs_f64() * 1000.0
}

/// Attend que `cond` soit vraie (au plus `max`).
fn wait_until(max: Duration, cond: impl Fn() -> bool) -> bool {
    let until = Instant::now() + max;
    while Instant::now() < until {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    cond()
}

/// Temps processeur par fil, renommés pour le rapport : `by_name` appelle
/// « principal » le fil appelant (ici celui qui joue la passerelle), et le fil
/// d'interface garde le nom que lui donne le système (`main`).
fn threads_now() -> HashMap<String, f64> {
    by_name()
        .into_iter()
        .map(|(n, ms)| {
            let n = match n.as_str() {
                "principal" => "banc_passerelle".to_string(),
                "main" => "principal".to_string(),
                _ => n,
            };
            (n, ms)
        })
        .collect()
}

/// Fils qui ont consommé au moins 1 ms depuis `before`, du plus gourmand au plus sobre.
fn threads_since(before: &HashMap<String, f64>) -> Vec<Value> {
    let mut v: Vec<(String, f64)> = threads_now().into_iter().map(|(n, ms)| (n.clone(), ms - before.get(&n).copied().unwrap_or(0.0))).filter(|(_, ms)| *ms >= 1.0).collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1));
    v.into_iter().map(|(n, ms)| json!({ "fil": n, "ms": round(ms) })).collect()
}

/// Processeur relevé chaque seconde pendant `secs` : moyenne et pointe (% d'un
/// cœur), images, fils qui ont travaillé. Les relevés par fil (coûteux : liste
/// de tous les fils du système) sont faits hors de la fenêtre mesurée.
fn observe(shared: &Shared, secs: u64, mut each_tick: impl FnMut(Duration)) -> Value {
    let threads = threads_now();
    let (frames, cpu0, start) = (shared.frames.load(Ordering::SeqCst), process_cpu_ms(), Instant::now());
    let (mut last, mut peak, mut second) = (cpu0, 0.0f64, 1u64);
    while start.elapsed() < Duration::from_secs(secs) {
        each_tick(start.elapsed());
        std::thread::sleep(Duration::from_millis(50));
        if start.elapsed() >= Duration::from_secs(second) {
            let now = process_cpu_ms();
            peak = peak.max((now - last) / 10.0);
            last = now;
            second += 1;
        }
    }
    let total = process_cpu_ms() - cpu0;
    json!({
        "secondes": secs,
        "processeur_ms": round(total),
        "moyenne_pct_d_un_coeur": round(total / (secs.max(1) as f64 * 10.0)),
        "pointe_sur_1_s_pct": round(peak),
        "images": shared.frames.load(Ordering::SeqCst) - frames,
        "fils": threads_since(&threads),
    })
}

/// Coût d'un relevé de la surveillance de `theme.toml` et `settings.toml` :
/// l'ancien (date et taille du fichier) et le nouveau (signal du dossier), en µs.
fn watch_cost() -> Value {
    let dir = std::env::temp_dir().join(format!("ferrocord-banc-cpu-{}", std::process::id()));
    let file = dir.join("theme.toml");
    if std::fs::create_dir_all(&dir).is_err() || std::fs::write(&file, "# banc\n").is_err() {
        return Value::Null;
    }
    const N: u32 = 2000;
    let t = Instant::now();
    for _ in 0..N {
        let _ = std::hint::black_box(std::fs::metadata(&file).map(|m| (m.modified().ok(), m.len())));
    }
    let stat = t.elapsed().as_secs_f64() * 1e6 / f64::from(N);
    let signal = ferrocord::platform::dirwatch::DirSignal::open(&dir).map(|mut s| {
        let t = Instant::now();
        for _ in 0..N {
            let _ = std::hint::black_box(s.changed());
        }
        t.elapsed().as_secs_f64() * 1e6 / f64::from(N)
    });
    let _ = std::fs::remove_dir_all(&dir);
    json!({ "releve_du_fichier_us": round(stat), "signal_du_dossier_us": signal.map(round) })
}

/// Fabrique le texte d'un paquet à partir d'un identifiant neuf.
type Make = Box<dyn Fn(u64) -> String>;

struct Driver {
    shared: Arc<Shared>,
    inject: Arc<Mutex<Vec<Event>>>,
    ctx: egui::Context,
    ready: String,
    games: String,
    /// Temps processeur passé à fabriquer les textes (retranché des totaux).
    generation_ms: f64,
    quiet: u64,
    origin: Instant,
}

impl Driver {
    fn push(&self, events: Vec<Event>) {
        if let Ok(mut q) = self.inject.lock() {
            q.extend(events);
        }
        self.ctx.request_repaint();
    }

    fn run(self) -> Value {
        let s = &self.shared;
        // 1) Première image (magasin vide).
        wait_until(Duration::from_secs(30), || s.frames.load(Ordering::SeqCst) >= 1);
        let first = json!({ "delai_ms": round(age_ms(self.origin)), "processeur_ms": round(process_cpu_ms() - self.generation_ms), "construction_ms": round(s.frame_max.swap(0, Ordering::SeqCst) as f64 / 1000.0) });
        // 2) READY : vrai décodage, puis l'image qui l'applique au magasin.
        let woken = Arc::new(AtomicUsize::new(0));
        let w = woken.clone();
        let wake: Wake = Arc::new(move || {
            w.fetch_add(1, Ordering::SeqCst);
        });
        let (mut replay, rx) = ferrocord::net::Replay::new(wake, INTEREST.get().cloned().unwrap_or_default(), None);
        let (heap, cpu, t) = (alloc::live(), process_cpu_ms(), Instant::now());
        replay.dispatch("READY", &self.ready);
        let decode_ms = t.elapsed().as_secs_f64() * 1000.0;
        let allocations = alloc::live().total - heap.total;
        let mut events = vec![Event::PeopleHub(replay.people())];
        events.extend(rx.try_iter());
        let (count, frames) = (events.len(), s.frames.load(Ordering::SeqCst));
        s.frame_max.store(0, Ordering::SeqCst);
        self.push(events);
        wait_until(Duration::from_secs(10), || s.frames.load(Ordering::SeqCst) > frames);
        let ready = json!({
            "octets": self.ready.len(),
            "decodage_ms": round(decode_ms),
            "allocations": allocations,
            "evenements": count,
            "image_qui_l_applique_ms": round(s.frame_max.load(Ordering::SeqCst) as f64 / 1000.0),
            "processeur_ms": round(process_cpu_ms() - cpu),
        });
        // 3) Salon ouvert, dernière page reçue.
        s.step.store(OPEN, Ordering::SeqCst);
        self.ctx.request_repaint();
        wait_until(Duration::from_secs(10), || s.step.load(Ordering::SeqCst) == OPENED);
        let (g, c) = (compte::guild_id(0), compte::channel_id(0, 5));
        let now = chrono::Utc::now().timestamp_millis();
        let messages = (0..HISTORY_PAGE as u64).map(|i| message(i, c, g, now - (HISTORY_PAGE as i64 - i as i64) * 90_000)).collect();
        self.push(vec![Event::History { channel_id: c, before: None, messages }]);
        // 4) Calme : première fenêtre de 2 s sous 100 ms de processeur.
        let mut samples: Vec<(f64, f64, usize)> = Vec::new();
        let started = Instant::now();
        let window = (CALM_WINDOW.as_millis() / CALM_STEP.as_millis()) as usize;
        let calm = loop {
            samples.push((age_ms(self.origin), process_cpu_ms(), s.frames.load(Ordering::SeqCst)));
            let n = samples.len();
            if n > window && samples[n - 1].1 - samples[n - 1 - window].1 < CALM_MS {
                break Some(samples[n - 1 - window]);
            }
            if started.elapsed() > CALM_CAP {
                break None;
            }
            std::thread::sleep(CALM_STEP);
        };
        let threads = threads_since(&HashMap::new());
        let calm = match calm {
            Some((at, cpu, frames)) => json!({ "atteint": true, "delai_ms": round(at), "processeur_ms": round(cpu - self.generation_ms), "images": frames }),
            None => json!({ "atteint": false }),
        };
        // 5) Jeux détectables : décodés après le calme, comme en vrai (60 à 90 s après la connexion).
        let (heap, cpu, t) = (alloc::live(), process_cpu_ms(), Instant::now());
        let index = ferrocord::net::decode_detectable_to_index(&self.games, now).unwrap_or_default();
        let decode_ms = t.elapsed().as_secs_f64() * 1000.0;
        let allocations = alloc::live().total - heap.total;
        self.push(vec![Event::Activity(ferrocord::state::activity::ActivityEvent::Detectable(Arc::new(index)))]);
        std::thread::sleep(Duration::from_secs(1));
        let games = json!({ "octets": self.games.len(), "decodage_ms": round(decode_ms), "allocations": allocations, "processeur_ms": round(process_cpu_ms() - cpu) });
        // 6) Repos sans trafic.
        let rest = observe(s, self.quiet, |_| {});
        // 7) Repos sous trafic : une rafale d'abord (les pastilles apparaissent), puis 3 paquets par seconde.
        let mut seq = 0x40_0000u64;
        let mut fresh = || {
            seq += 1;
            snowflake(chrono::Utc::now().timestamp_millis(), seq)
        };
        let far = |k: usize| (compte::guild_id(1 + k % FAR), compte::channel_id(1 + k % FAR, 6));
        let mut send = |kind: &str, d: String| {
            replay.dispatch(kind, &d);
            let events: Vec<Event> = rx.try_iter().collect();
            if let Ok(mut q) = self.inject.lock() {
                q.extend(events);
            }
            if woken.swap(0, Ordering::SeqCst) > 0 {
                self.ctx.request_repaint();
                return 1usize;
            }
            0
        };
        for k in 0..FAR {
            let (gid, ch) = far(k);
            send("MESSAGE_CREATE", packet_create(ch, gid, fresh(), "premier message"));
            send("CHANNEL_UPDATE", packet_channel(gid, ch + 1, compte::channel_id(1 + k % FAR, 1), "salon-7"));
        }
        std::thread::sleep(Duration::from_secs(2));
        let (mut k, mut wakes) = (0usize, 0usize);
        let mut traffic = observe(s, self.quiet, |elapsed| {
            let due = (elapsed.as_secs_f64() * RATE) as usize;
            while k < due {
                let (gid, ch) = far(k);
                wakes += match k % 5 {
                    0 => send("MESSAGE_UPDATE", packet_update(ch, gid, fresh(), &format!("modifié {k}"))),
                    1 => send("MESSAGE_CREATE", packet_create(ch, gid, fresh(), "encore un")),
                    2 => send("MESSAGE_DELETE", packet_delete(ch, gid, fresh())),
                    3 => send("CHANNEL_UPDATE", packet_channel(gid, ch + 1, compte::channel_id(1 + k % FAR, 1), &format!("salon-renommé-{k}"))),
                    _ => send("MESSAGE_UPDATE", packet_embeds(ch, gid, fresh())),
                };
                k += 1;
            }
        });
        traffic["paquets"] = json!(k);
        traffic["paquets_ayant_reveille"] = json!(wakes);
        // 8) Coût d'un paquet de chaque sorte (décodage seul : les événements ne vont pas à l'interface).
        let (gid, ch) = far(3);
        let user = 2_000_000_000_000_000_007u64;
        let kinds: [(&str, Make); 6] = [
            ("MESSAGE_CREATE", Box::new(move |id| packet_create(ch, gid, id, "un message ordinaire, ni trop court ni trop long"))),
            ("MESSAGE_UPDATE", Box::new(move |id| packet_update(ch, gid, id, "texte modifié"))),
            ("MESSAGE_DELETE", Box::new(move |id| packet_delete(ch, gid, id))),
            ("TYPING_START", Box::new(move |_| json!({ "channel_id": ch.to_string(), "guild_id": gid.to_string(), "user_id": user.to_string(), "timestamp": 1_700_000_000, "member": { "user": { "id": user.to_string(), "username": "quelqu_un" }, "roles": [], "nick": null } }).to_string())),
            ("PRESENCE_UPDATE", Box::new(move |_| json!({ "user": { "id": user.to_string() }, "guild_id": gid.to_string(), "status": "online", "client_status": { "desktop": "online" }, "activities": [] }).to_string())),
            ("MESSAGE_REACTION_ADD", Box::new(move |id| json!({ "user_id": user.to_string(), "channel_id": ch.to_string(), "message_id": id.to_string(), "guild_id": gid.to_string(), "emoji": { "id": null, "name": "👍" }, "type": 0 }).to_string())),
        ];
        let mut per_packet = Vec::new();
        for (kind, make) in &kinds {
            let packets: Vec<String> = (0..EACH).map(|_| make(fresh())).collect();
            let (heap, t) = (alloc::live(), Instant::now());
            for d in &packets {
                replay.dispatch(kind, d);
            }
            let us = t.elapsed().as_secs_f64() * 1e6 / EACH as f64;
            let allocations = (alloc::live().total - heap.total) as f64 / EACH as f64;
            let events = rx.try_iter().count();
            woken.store(0, Ordering::SeqCst);
            per_packet.push(json!({ "paquet": kind, "microsecondes": round(us), "allocations": round(allocations), "evenements": round(events as f64 / EACH as f64) }));
        }
        json!({
            "generation_des_textes_ms": round(self.generation_ms),
            "premiere_image": first,
            "ready": ready,
            "calme": calm,
            "fils_jusqu_au_calme": threads,
            "jeux_detectables": games,
            "repos": rest,
            "repos_sous_trafic": traffic,
            "cout_par_paquet": per_packet,
            "surveillance_des_fichiers": watch_cost(),
        })
    }
}

pub fn run(quiet: u64) -> eframe::Result {
    let origin = Instant::now();
    // Textes des paquets : fabriqués dans un fil à part, dont le temps est retranché.
    let cpu = process_cpu_ms();
    let size = compte::Size::default();
    let made = std::thread::spawn(move || (compte::ready(&size), compte::detectable(size.games))).join();
    let Ok((ready, games)) = made else {
        eprintln!("--bench-cpu : textes du compte simulé impossibles à fabriquer");
        std::process::exit(1);
    };
    let generation_ms = process_cpu_ms() - cpu;
    let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("runtime : {e}");
            std::process::exit(1);
        }
    };
    let mut options = ferrocord::native_options();
    options.viewport = options.viewport.with_title("Ferrocord — banc du processeur");
    // Comme `ferrocord::run` : icône de la fenêtre réduite d'avance.
    let options = ferrocord::platform::app_icon::skip_default(options);
    let theme = ThemeLink::open(None, false, WatchConfig::default());
    eframe::run_native(
        "Ferrocord banc processeur",
        ferrocord::with_rendu(options),
        Box::new(move |cc| {
            ferrocord::platform::app_icon::set(cc);
            let inject: Arc<Mutex<Vec<Event>>> = Arc::default();
            let shared: Arc<Shared> = Arc::default();
            let backend = Offline { connected: true, queue: Vec::new(), wake: None, next_id: 1, view: View::Guild, inject: Some(inject.clone()), pages: HashMap::new(), served: None, make: None };
            let wake_ctx = cc.egui_ctx.clone();
            let wake: Wake = Arc::new(move || wake_ctx.request_repaint());
            let images = Images::start(cc.egui_ctx.clone(), rt.handle(), Arc::new(Generated), None, images::Config::default(), wake);
            let mut app = App::new(cc, offline_boot(backend, Store::new(), theme, images));
            app.set_theme(Preset::Rouille.tokens());
            let driver = Driver { shared: shared.clone(), inject, ctx: cc.egui_ctx.clone(), ready, games, generation_ms, quiet, origin };
            let ctx = cc.egui_ctx.clone();
            let end = shared.clone();
            std::thread::Builder::new().name("banc-passerelle".into()).spawn(move || {
                let report = driver.run();
                println!("{}", serde_json::to_string(&report).unwrap_or_default());
                end.done.store(true, Ordering::SeqCst);
                ctx.request_repaint();
            })?;
            Ok(Box::new(CpuApp { app, shared, _rt: rt, started: None, closing: false }))
        }),
    )
}

struct CpuApp {
    app: App,
    shared: Arc<Shared>,
    _rt: tokio::runtime::Runtime,
    started: Option<Instant>,
    closing: bool,
}

impl eframe::App for CpuApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        // Comme l'application (`App::raw_input_hook`) : atlas des glyphes borné.
        ferrocord::ui::fonts::limit_atlas(raw);
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.started = Some(Instant::now());
        if self.shared.step.load(Ordering::SeqCst) == OPEN {
            self.app.open(Some(compte::guild_id(0)), compte::channel_id(0, 5));
            self.shared.step.store(OPENED, Ordering::SeqCst);
        }
        eframe::App::logic(&mut self.app, ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        eframe::App::ui(&mut self.app, ui, frame);
        if let Some(t) = self.started.take() {
            self.shared.frame_max.fetch_max(t.elapsed().as_micros() as u64, Ordering::SeqCst);
        }
        self.shared.frames.fetch_add(1, Ordering::SeqCst);
        if self.shared.done.load(Ordering::SeqCst) && !self.closing {
            self.closing = true;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
