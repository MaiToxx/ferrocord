//! Liens et intégration à Windows côté interface (lot C6) : tout lien
//! demandé à egui (`ctx.open_url`, `hyperlink`) est repris à la fin de
//! l'image ; un lien Discord s'ouvre dans Ferrocord, les autres partent par
//! `install::browser`, sans `DISCORD_TOKEN`. Aussi : demandes d'un second
//! lancement, démarrage réduit, état du registre et des mises à jour.
//!
//! Lien externe (lot VID) : comme Discord, une fenêtre montre l'adresse
//! entière et demande confirmation avant d'ouvrir le navigateur, sauf vers
//! les domaines de Discord et ceux que l'utilisateur a dit de confiance
//! (liste gardée sur cet ordinateur, `domaines-de-confiance.txt`).

use super::App;
use crate::i18n::table::install::t as it;
use crate::install::licenses::Licenses;
use crate::install::links::{self, Target};
use crate::install::single::{Args, Listener, Request};
use crate::install::update::Status;
use crate::install::{Autostart, InstallEvent, Installer, Protocol, Quiet};
use crate::i18n::table::messages::{f as mf, t as mt};
use crate::i18n::t as tx;
use crate::theme::Tokens;
use eframe::egui::{self, Id, RichText, ViewportCommand};
use std::path::PathBuf;
use std::sync::Arc;

/// Domaines de Discord : jamais de confirmation (sous-domaines compris).
const OWN_DOMAINS: &[&str] = &["discord.com", "discordapp.com", "discordapp.net", "discord.gg", "discord.media", "discord.new", "discord.gift", "discordstatus.com", "dis.gd"];
/// Domaines de confiance retenus, au plus.
const MAX_TRUSTED: usize = 500;
/// Caractères de l'adresse montrés dans la fenêtre, au plus.
const MAX_SHOWN: usize = 400;

/// Hôte d'un lien `http` ou `https`, en minuscules (forme ASCII d'un nom
/// international) ; `None` : autre schéma ou adresse illisible.
pub(super) fn host_of(url: &str) -> Option<String> {
    let u = reqwest::Url::parse(url.trim()).ok()?;
    matches!(u.scheme(), "http" | "https").then(|| u.host_str().map(str::to_ascii_lowercase)).flatten().filter(|h| !h.is_empty())
}

/// `host` est `domain` ou l'un de ses sous-domaines.
fn covers(domain: &str, host: &str) -> bool {
    host == domain || host.strip_suffix(domain).is_some_and(|rest| rest.ends_with('.'))
}

/// Lien `https` vers un domaine de Discord, sans identifiants ni port : le
/// seul qu'un bouton d'une notification officielle ouvre d'un clic.
pub(super) fn own_https(url: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url.trim()) else { return false };
    u.scheme() == "https"
        && u.username().is_empty()
        && u.password().is_none()
        && u.port().is_none()
        && u.host_str().map(str::to_ascii_lowercase).is_some_and(|h| OWN_DOMAINS.iter().any(|d| covers(d, &h)))
}

fn valid_domain(d: &str) -> bool {
    !d.is_empty() && d.len() <= 253 && d.contains('.') && d.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
}

/// Domaines vers lesquels un lien s'ouvre sans confirmation.
#[derive(Default)]
pub(super) struct Trusted {
    /// Fichier de la liste (un domaine par ligne) ; `None` : en mémoire seulement.
    file: Option<PathBuf>,
    /// Lu au premier lien externe, pas au démarrage.
    loaded: bool,
    list: Vec<String>,
}

impl Trusted {
    fn load(&mut self) {
        if std::mem::replace(&mut self.loaded, true) {
            return;
        }
        let text = self.file.as_ref().and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
        for d in text.lines().map(|l| l.trim().to_ascii_lowercase()).filter(|d| valid_domain(d)).take(MAX_TRUSTED) {
            if !self.list.contains(&d) {
                self.list.push(d);
            }
        }
    }

    pub(super) fn has(&mut self, host: &str) -> bool {
        if OWN_DOMAINS.iter().any(|d| covers(d, host)) {
            return true;
        }
        self.load();
        self.list.iter().any(|d| covers(d, host))
    }

    fn add(&mut self, host: &str) {
        if !valid_domain(host) || self.has(host) || self.list.len() >= MAX_TRUSTED {
            return;
        }
        self.list.push(host.to_string());
        self.save();
    }

    fn save(&self) {
        if let Some(f) = &self.file {
            let _ = std::fs::write(f, self.list.join("\n") + "\n");
        }
    }

    /// Domaines retenus (page « Avancé », lot FF2) : lus au premier affichage.
    pub(super) fn domains(&mut self) -> &[String] {
        self.load();
        &self.list
    }

    /// Domaine tapé dans la page : une adresse entière est ramenée à son hôte.
    /// `false` : illisible, déjà couvert, ou liste pleine.
    pub(super) fn add_typed(&mut self, text: &str) -> bool {
        let text = text.trim().to_ascii_lowercase();
        let host = text.strip_prefix("https://").or_else(|| text.strip_prefix("http://")).unwrap_or(&text).split(['/', '?', '#']).next().unwrap_or("").to_string();
        if !valid_domain(&host) || self.has(&host) || self.list.len() >= MAX_TRUSTED {
            return false;
        }
        self.add(&host);
        true
    }

    pub(super) fn remove(&mut self, host: &str) {
        self.load();
        self.list.retain(|d| d != host);
        self.save();
    }
}

/// Lien externe qui attend la confirmation de l'utilisateur.
pub(super) struct Leaving {
    /// Adresse ouverte si l'utilisateur confirme.
    url: String,
    /// Adresse montrée : telle que le navigateur la comprendra.
    shown: String,
    /// `None` : pas un lien web, la confiance ne se propose pas.
    host: Option<String>,
    pub(super) trust: bool,
}

/// Site de vidéos connu : le triangle de lecture d'un aperçu y mène sans
/// confirmation. Tout autre site passe par « Vous quittez Discord » : la
/// page d'un inconnu peut se faire passer pour une vidéo par ses balises
/// (audit 0.1.33).
fn video_site(url: &str) -> bool {
    const SITES: [&str; 5] = ["youtube.com", "youtu.be", "twitch.tv", "vimeo.com", "dailymotion.com"];
    let Ok(u) = reqwest::Url::parse(url.trim()) else { return false };
    let host = u.host_str().unwrap_or_default().to_ascii_lowercase();
    u.scheme() == "https" && SITES.iter().any(|s| host == *s || host.strip_suffix(s).is_some_and(|sub| sub.ends_with('.')))
}

fn direct_id() -> Id {
    Id::unique("liens-directs")
}

/// Ouvre `url` à la fin de l'image, sans confirmation : pour un bouton dont
/// l'effet est déjà clair (lecture d'une vidéo YouTube dans le navigateur).
pub(super) fn open_direct(ctx: &egui::Context, url: &str) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Vec<String>>(direct_id()).push(url.to_string()));
    ctx.request_repaint();
}

pub(super) struct InstallUi {
    pub(super) backend: Box<dyn Installer>,
    /// `None` : registre pas encore lu.
    pub(super) autostart: Option<Autostart>,
    pub(super) protocol: Option<Protocol>,
    pub(super) update: Option<Status>,
    /// Dernière version annoncée par une notification (une fois par version).
    announced: Option<crate::install::update::Version>,
    /// Adresse du manifeste confiée au fil (vérifiée à chaque changement).
    configured: Option<String>,
    requests: Option<Listener>,
    /// Liens reçus avant la connexion : ouverts une fois le compte prêt.
    pending: Vec<Target>,
    minimize: bool,
    confirmed: bool,
    /// Licences décompressées à la première ouverture de « À propos ».
    pub(super) licenses: Option<Result<Arc<Licenses>, String>>,
    pub(super) license_filter: String,
    pub(super) license_open: Option<usize>,
    /// Lien externe en attente de confirmation (lot VID).
    pub(super) leaving: Option<Leaving>,
    pub(super) trusted: Trusted,
}

impl Default for InstallUi {
    fn default() -> Self {
        InstallUi {
            backend: Box::new(Quiet::default()),
            autostart: None,
            protocol: None,
            update: None,
            announced: None,
            configured: None,
            requests: None,
            pending: Vec::new(),
            minimize: false,
            confirmed: false,
            licenses: None,
            license_filter: String::new(),
            license_open: None,
            leaving: None,
            trusted: Trusted::default(),
        }
    }
}

impl App {
    /// Branche le vrai système (registre, mises à jour, navigateur), le
    /// canal des lancements suivants et la ligne de commande.
    pub fn attach_install(&mut self, mut backend: Box<dyn Installer>, requests: Option<Listener>, args: &Args) {
        backend.load();
        self.install.backend = backend;
        if let Some(l) = &requests {
            let ctx = self.ctx.clone();
            l.set_wake(Arc::new(move || ctx.request_repaint()));
        }
        self.install.requests = requests;
        self.install.minimize = args.minimized;
        self.install.configured = None;
        if let Some(t) = args.link.as_deref().and_then(links::parse) {
            self.install.pending.push(t);
        }
    }

    /// Où garder les domaines de confiance des liens externes.
    pub fn attach_trusted_links(&mut self, file: Option<PathBuf>) {
        self.install.trusted = Trusted { file, ..Trusted::default() };
    }

    /// Ouvre un lien comme un clic : dans Ferrocord si c'est un lien
    /// Discord, sinon dans le navigateur (sans le jeton), après
    /// confirmation si son domaine n'est pas de confiance.
    pub fn open_link(&mut self, url: &str) {
        self.open_link_with(url, true);
    }

    fn open_link_with(&mut self, url: &str, confirm: bool) {
        match links::parse(url) {
            Some(t) => {
                self.install.pending.push(t);
                self.open_pending_links();
            }
            // Un `discord:` incompris n'est confié à personne.
            None if url.trim_start().to_ascii_lowercase().starts_with("discord:") => {}
            None if confirm => self.leave(url),
            None => self.install.backend.open_external(url),
        }
    }

    /// Lien externe : ouvert tout de suite vers un domaine de confiance,
    /// sinon mis en attente de la confirmation.
    fn leave(&mut self, url: &str) {
        let host = host_of(url);
        if host.as_deref().is_some_and(|h| self.install.trusted.has(h)) {
            self.install.backend.open_external(url);
            return;
        }
        let parsed = reqwest::Url::parse(url.trim()).map(String::from).unwrap_or_else(|_| url.trim().to_string());
        let mut shown: String = parsed.chars().filter(|c| !c.is_control()).take(MAX_SHOWN).collect();
        if parsed.chars().count() > MAX_SHOWN {
            shown.push('…');
        }
        self.install.leaving = Some(Leaving { url: url.to_string(), shown, host, trust: false });
        self.ctx.request_repaint();
    }

    /// Réponse à la fenêtre du lien externe : `go` ouvre le navigateur (et
    /// retient le domaine si la case est cochée).
    pub(super) fn answer_leaving(&mut self, go: bool) {
        let Some(l) = self.install.leaving.take() else { return };
        if !go {
            return;
        }
        if let (true, Some(h)) = (l.trust, &l.host) {
            self.install.trusted.add(h);
        }
        self.install.backend.open_external(&l.url);
    }

    /// Fenêtre « Vous quittez Discord » : l'adresse entière, la case de
    /// confiance, « Retour » et « Visiter le site ».
    fn leaving_ui(&mut self, ctx: &egui::Context) {
        let Some(mut l) = self.install.leaving.take() else { return };
        let t: &Tokens = &self.theme;
        let lang = self.lang;
        let (mut go, mut back) = (false, false);
        let out = super::widgets::modal(
            ctx,
            t,
            Id::unique("lien-externe"),
            440.0,
            mt::LEAVING_TITLE.get(lang),
            tx::CLOSE_TIP.get(lang),
            |ui| {
                ui.label(RichText::new(mt::LEAVING_BODY.get(lang)).color(t.colors.text));
                egui::Frame::new().fill(t.colors.bg1).stroke(t.border()).corner_radius(t.r_control()).inner_margin(t.embed_margin()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    // Une adresse n'a pas d'espaces : coupée n'importe où plutôt que de déborder.
                    let mut job = egui::text::LayoutJob::simple(l.shown.clone(), egui::TextStyle::Body.resolve(ui.style()), t.colors.text_strong, ui.available_width());
                    job.wrap.break_anywhere = true;
                    ui.add(egui::Label::new(job));
                });
                if let Some(h) = &l.host {
                    ui.checkbox(&mut l.trust, RichText::new(mf::trust_domain(lang, h)).color(t.colors.text));
                }
            },
            |ui| {
                go = super::widgets::button(ui, t, super::widgets::Kind::Primary, mt::LEAVING_VISIT.get(lang)).clicked();
                back = super::widgets::button(ui, t, super::widgets::Kind::Link, mt::LEAVING_BACK.get(lang)).clicked();
            },
        );
        go |= ctx.input(|i| i.key_pressed(egui::Key::Enter));
        let done = go || back || out.closed;
        self.install.leaving = Some(l);
        if done {
            self.answer_leaving(go);
        }
    }

    /// Liens demandés à egui pendant l'image : repris ici, egui n'ouvre rien.
    pub(super) fn route_links(&mut self, ctx: &egui::Context) {
        let urls = ctx.output_mut(|o| {
            let mut urls = Vec::new();
            o.commands.retain(|c| match c {
                egui::OutputCommand::OpenUrl(u) => {
                    urls.push(u.url.clone());
                    false
                }
                _ => true,
            });
            urls
        });
        for u in urls {
            self.open_link(&u);
        }
        // Boutons de lecture vers un site (YouTube, Twitch) : sans confirmation.
        for u in ctx.data_mut(|d| d.remove_temp::<Vec<String>>(direct_id())).unwrap_or_default() {
            self.open_link_with(&u, !video_site(&u));
        }
        self.leaving_ui(ctx);
        // Images à copier ou à enregistrer, demandées pendant l'image (lot VID).
        self.image_ops_tick(ctx);
        // Première image dessinée : la version à l'essai a démarré.
        if !self.install.confirmed {
            self.install.confirmed = true;
            self.install.backend.confirm_started();
        }
    }

    /// À chaque passage de logique : réponses du fil, demandes d'un second
    /// lancement, réduction au démarrage, liens en attente.
    pub(super) fn install_tick(&mut self) {
        for e in self.install.backend.poll() {
            match e {
                InstallEvent::Registry { autostart, protocol } => {
                    self.install.autostart = Some(autostart);
                    self.install.protocol = Some(protocol);
                }
                InstallEvent::Update(s) => {
                    // Version disponible : dite une fois, par le vrai système seulement.
                    if let Status::Available(m) = &s
                        && self.install.backend.announces()
                        && self.install.announced.replace(m.version) != Some(m.version)
                    {
                        let text = crate::i18n::table::install::f::update_toast(self.lang, m.version);
                        self.notify(text);
                    }
                    self.install.update = Some(s);
                }
                InstallEvent::Failed(why) => {
                    let text = crate::i18n::table::install::f::registry_failed(self.lang, why);
                    self.notify(text);
                }
            }
        }
        let requests = self.install.requests.as_ref().map(Listener::poll).unwrap_or_default();
        for r in requests {
            self.show_window();
            if let Request::Open(link) = r
                && let Some(t) = links::parse(&link)
            {
                self.install.pending.push(t);
            }
        }
        if std::mem::take(&mut self.install.minimize) {
            // Sans icône, une fenêtre masquée ne pourrait plus revenir : réduite.
            if self.desk.prefs.tray {
                self.hide_window();
            } else {
                self.ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
            }
        }
        let url = self.local.prefs.update_manifest.trim().to_string();
        if self.install.configured.as_ref() != Some(&url) {
            self.install.backend.configure_updates(url.clone());
            self.install.configured = Some(url);
        }
        self.open_pending_links();
    }

    fn open_pending_links(&mut self) {
        if self.install.pending.is_empty() || !self.online() {
            return;
        }
        for t in std::mem::take(&mut self.install.pending) {
            self.open_target(t);
        }
        self.flush_commands();
        self.ctx.request_repaint();
    }

    fn open_target(&mut self, t: Target) {
        let channel_place = |app: &mut App, guild: Option<u64>, channel: u64| -> bool {
            if !app.store.channel_exists(channel) || guild.is_some_and(|g| !app.store.guilds.iter().any(|x| x.id == g)) {
                return false;
            }
            match guild {
                Some(g) => app.store.guild = Some(g),
                None => app.store.select_dms(),
            }
            true
        };
        let found = match t {
            Target::Home => {
                self.store.select_dms();
                true
            }
            Target::Guild(g) => {
                let known = self.store.guilds.iter().any(|x| x.id == g);
                if known {
                    self.store.select_guild(g);
                }
                known
            }
            Target::Channel { guild, channel } => {
                let ok = channel_place(self, guild, channel);
                if ok {
                    self.open_channel(channel);
                }
                ok
            }
            Target::Message { guild, channel, message } => {
                let ok = channel_place(self, guild, channel);
                if ok {
                    self.jump_to_message(channel, message);
                }
                ok
            }
            Target::Invite(code) => {
                self.open_join_with(&code);
                true
            }
            Target::User(id) => {
                self.open_full_profile(id, None);
                true
            }
        };
        if !found {
            self.notify(it::LINK_NOT_FOUND);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::{Channel, ChannelKind, Guild, User};
    use crate::state::{Cmd, Event, Store};
    use std::sync::Mutex;

    struct Online;
    impl crate::ui::Backend for Online {
        fn set_wake(&mut self, _: crate::ui::Wake) {}
        fn connect(&mut self, _: crate::account::Token) {}
        fn disconnect(&mut self) {}
        fn is_connected(&self) -> bool {
            true
        }
        fn poll(&mut self) -> Vec<Event> {
            Vec::new()
        }
        fn send(&mut self, _: Vec<Cmd>) {}
    }

    /// Système factice : retient les liens externes et les confirmations.
    #[derive(Clone, Default)]
    struct Fake {
        opened: Arc<Mutex<Vec<String>>>,
        configured: Arc<Mutex<Vec<String>>>,
        confirmed: Arc<Mutex<u32>>,
    }

    impl Installer for Fake {
        fn load(&mut self) {}
        fn set_autostart(&mut self, _: Autostart) {}
        fn set_protocol(&mut self, _: bool) {}
        fn configure_updates(&mut self, url: String) {
            self.configured.lock().unwrap().push(url);
        }
        fn check_updates(&mut self) {}
        fn download(&mut self, _: crate::install::update::Manifest) {}
        fn confirm_started(&mut self) {
            *self.confirmed.lock().unwrap() += 1;
        }
        fn open_external(&mut self, url: &str) {
            self.opened.lock().unwrap().push(url.to_string());
        }
        fn poll(&mut self) -> Vec<InstallEvent> {
            Vec::new()
        }
    }

    const G: u64 = 1_000_000_000_000_000_001;
    const C: u64 = 1_000_000_000_000_000_011;

    fn app(link: Option<&str>) -> (App, Fake) {
        let ctx = egui::Context::default();
        let mut store = Store::new();
        let chan = Channel { id: C, name: "général".into(), kind: ChannelKind::Text, position: 0, parent: None, overwrites: Some(Vec::new()), slowmode: 0 };
        store.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: vec![Guild::new(G, "Alpha".into(), vec![chan])], dms: Vec::new() });
        store.take_commands();
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = App::with_context(ctx, crate::ui::tests::offline_boot(Box::new(Online), store, images));
        let fake = Fake::default();
        let args = Args { link: link.map(str::to_string), ..Default::default() };
        app.attach_install(Box::new(fake.clone()), None, &args);
        (app, fake)
    }

    fn frame(app: &mut App, f: impl FnOnce(&egui::Context)) -> egui::FullOutput {
        app.logic_pass();
        let ctx = app.ctx.clone();
        let mut f = Some(f);
        let mut out = ctx.clone().run_ui(egui::RawInput::default(), |ui| {
            if let Some(f) = f.take() {
                f(ui.ctx());
            }
            app.ui_pass(ui);
        });
        out.textures_delta.clear();
        out
    }

    #[test]
    fn external_links_leave_through_our_opener_and_egui_opens_nothing() {
        let (mut app, fake) = app(None);
        let out = frame(&mut app, |ctx| {
            ctx.open_url(egui::OpenUrl::new_tab("https://exemple.invalid/page"));
            ctx.open_url(egui::OpenUrl::new_tab("discord://-/inconnu"));
        });
        assert!(!out.platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::OpenUrl(_))), "egui n'ouvre rien lui-même");
        // Domaine inconnu : rien ne part avant la confirmation.
        assert!(fake.opened.lock().unwrap().is_empty());
        assert!(app.install.leaving.as_ref().is_some_and(|l| l.host.as_deref() == Some("exemple.invalid") && l.shown == "https://exemple.invalid/page"));
        frame(&mut app, |_| {});
        assert!(app.install.leaving.is_some(), "la fenêtre attend la réponse");
        app.answer_leaving(true);
        assert_eq!(fake.opened.lock().unwrap().as_slice(), ["https://exemple.invalid/page"]);
        assert_eq!(*fake.confirmed.lock().unwrap(), 1, "démarrage confirmé une fois");
        frame(&mut app, |_| {});
        assert_eq!(*fake.confirmed.lock().unwrap(), 1);
        assert_eq!(fake.configured.lock().unwrap().as_slice(), [""], "pas d'adresse : configurée une fois, vide");
    }

    /// Page des domaines de confiance (lot FF2) : ajout tapé, retrait.
    #[test]
    fn trusted_domains_are_added_and_removed_from_the_page() {
        let mut t = Trusted::default();
        assert!(t.add_typed(" https://Exemple.test/page?x=1 "));
        assert!(!t.add_typed("exemple.test"), "déjà là");
        assert!(!t.add_typed("sous.exemple.test"), "déjà couvert");
        assert!(!t.add_typed("pas un domaine") && !t.add_typed("discord.com"));
        assert_eq!(t.domains(), ["exemple.test".to_string()]);
        assert!(t.has("sous.exemple.test"));
        t.remove("exemple.test");
        assert!(t.domains().is_empty() && !t.has("exemple.test"));
    }

    #[test]
    fn external_links_are_confirmed_unless_trusted() {
        let (mut app, fake) = app(None);
        let opened = |fake: &Fake| fake.opened.lock().unwrap().clone();
        // « Retour » : rien ne s'ouvre.
        app.open_link("https://site.exemple.invalid/a?b=c");
        app.answer_leaving(false);
        assert!(opened(&fake).is_empty() && app.install.leaving.is_none());
        // Case cochée : ouvert, et le domaine (sous-domaines compris) ne demande plus rien.
        app.open_link("https://site.exemple.invalid/a?b=c");
        app.install.leaving.as_mut().unwrap().trust = true;
        app.answer_leaving(true);
        app.open_link("https://sous.site.exemple.invalid/suite");
        assert_eq!(opened(&fake), ["https://site.exemple.invalid/a?b=c", "https://sous.site.exemple.invalid/suite"]);
        // Un domaine qui finit pareil sans en être un sous-domaine reste à confirmer.
        app.open_link("https://fauxsite.exemple.invalid/");
        assert!(app.install.leaving.take().is_some());
        // Discord et ses hôtes de fichiers : jamais de fenêtre.
        app.open_link("https://cdn.discordapp.com/attachments/1/2/a.png");
        app.open_link("https://support.discord.com/hc/fr");
        assert!(app.install.leaving.is_none());
        assert_eq!(opened(&fake).len(), 4);
        // Faux domaine de Discord, nom international (montré en ASCII), autre schéma.
        app.open_link("https://discord.com.exemple.invalid/x");
        assert!(app.install.leaving.take().is_some_and(|l| l.host.as_deref() == Some("discord.com.exemple.invalid")));
        app.open_link("https://dïscord.com/x");
        assert!(app.install.leaving.take().is_some_and(|l| l.shown.starts_with("https://xn--")));
        app.open_link("mailto:quelqu.un@exemple.invalid");
        assert!(app.install.leaving.take().is_some_and(|l| l.host.is_none()));
        // Bouton de lecture d'une vidéo sur un site connu : direct, sans fenêtre.
        frame(&mut app, |ctx| open_direct(ctx, "https://www.youtube.com/watch?v=1"));
        assert!(app.install.leaving.is_none());
        assert_eq!(opened(&fake).last().map(String::as_str), Some("https://www.youtube.com/watch?v=1"));
        // Sur un site inconnu : la fenêtre, comme pour tout autre lien.
        frame(&mut app, |ctx| open_direct(ctx, "https://video.exemple.invalid/v/1"));
        assert!(app.install.leaving.take().is_some());
        assert_eq!(opened(&fake).last().map(String::as_str), Some("https://www.youtube.com/watch?v=1"));
        // La liste survit au redémarrage, dans son fichier.
        let dir = std::env::temp_dir().join(format!("ferrocord-liens-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("domaines-de-confiance.txt");
        let mut t = Trusted { file: Some(file.clone()), ..Trusted::default() };
        assert!(!t.has("a.exemple.invalid"));
        t.add("a.exemple.invalid");
        t.add("pas un domaine");
        let mut again = Trusted { file: Some(file), ..Trusted::default() };
        assert!(again.has("a.exemple.invalid") && again.has("x.a.exemple.invalid") && !again.has("b.exemple.invalid"));
        assert_eq!(again.list.len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn discord_links_stay_in_ferrocord() {
        let (mut app, fake) = app(Some("discord://-/channels/1000000000000000001/1000000000000000011"));
        // Lien de la ligne de commande : ouvert dès que le compte est prêt.
        frame(&mut app, |_| {});
        assert_eq!((app.store.guild, app.store.channel), (Some(G), Some(C)));
        app.store.select_dms();
        frame(&mut app, |ctx| ctx.open_url(egui::OpenUrl::new_tab(format!("https://discord.com/channels/{G}/{C}"))));
        assert_eq!((app.store.guild, app.store.channel), (Some(G), Some(C)));
        app.open_link("https://discord.gg/RustFr");
        assert!(app.servers.dialog.as_ref().is_some_and(|d| d.page == super::super::guild_create::Page::Join));
        // Salon inaccessible : un message, rien n'est ouvert ni confié au navigateur.
        app.open_link("https://discord.com/channels/9/9");
        assert!(app.toast.is_some());
        assert!(fake.opened.lock().unwrap().is_empty());
    }
}

#[cfg(test)]
mod video_site_tests {
    #[test]
    fn only_known_video_sites_open_without_asking() {
        for ok in ["https://www.youtube.com/watch?v=x", "https://youtu.be/x", "https://clips.twitch.tv/x", "https://vimeo.com/1"] {
            assert!(super::video_site(ok), "{ok}");
        }
        for bad in ["https://video.exemple.invalid/regarder", "https://youtube.com.exemple.invalid/x", "https://fauxyoutube.com/x", "http://youtube.com/x", "pas une adresse"] {
            assert!(!super::video_site(bad), "{bad}");
        }
    }
}
