//! Tests de la carte de mon compte, et outils partagés avec ceux de
//! l'édition rapide (`profile_edit_tests.rs`) : application sans fenêtre ni
//! réseau, images dessinées en mémoire, clics simulés.

use super::*;
use crate::state::model::{Guild, Member, User};
use crate::state::people::UserInfo;
use crate::state::profiles::{ProfileState, ProfileText};
use crate::state::usersettings::proto::Msg;
use crate::state::usersettings::{AccountCmd, AccountEvent};
use crate::state::{Cmd, Event, Store};
use crate::i18n::Lang;

pub(in crate::ui) const ME: u64 = 1;
pub(in crate::ui) const GUILD: u64 = 9;
/// Panneau utilisateur fictif, en bas à gauche d'un écran de 1000 × 1600.
const PANEL: egui::Rect = egui::Rect { min: egui::pos2(72.0, 1548.0), max: egui::pos2(312.0, 1600.0) };

struct Offline;

impl super::super::Backend for Offline {
    fn set_wake(&mut self, _: super::super::Wake) {}
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

/// Moi (« Moi Fictif », pseudo « Pseudo » sur le serveur affiché), réglages
/// synchronisés reçus (vides), profil complet connu.
pub(in crate::ui) fn app() -> App {
    let ctx = egui::Context::default();
    let mut store = Store::new();
    let mut guild = Guild::new(GUILD, "G".into(), Vec::new());
    guild.me = Some(Member { nick: Some("Pseudo".into()), ..Default::default() });
    store.apply(Event::Ready { me: User::new(ME, "moi.fictif".into()), guilds: vec![guild], dms: Vec::new() });
    store.guild = Some(GUILD);
    store.apply(Event::Account(AccountEvent::Proto { bytes: Some(Msg::default().encode()), partial: false }));
    {
        let mut people = store.people.lock();
        people.upsert_user(UserInfo {
            id: ME,
            username: "moi.fictif".into(),
            global_name: Some("Moi Fictif".into()),
            discriminator: None,
            avatar: crate::state::model::Avatar::default_for(ME),
            banner: None,
            accent: None,
            flags: None,
            bot: false,
            system: false,
            look: Default::default(),
        });
        people.upsert_member(GUILD, ME, crate::state::people::MemberInfo { nick: Some("Pseudo".into()), ..Default::default() });
        let full = crate::state::profiles::Profile {
            user: ProfileText { bio: Some("Bio du compte.".into()), pronouns: Some("iel".into()), theme: Some([0x112233, 0x445566]), ..Default::default() },
            guild: Some(ProfileText { bio: Some("Bio du serveur.".into()), ..Default::default() }),
            ..Default::default()
        };
        people.profiles_mut().set(ME, Some(GUILD), ProfileState::Ready(Box::new(full.clone())));
        people.profiles_mut().set(ME, None, ProfileState::Ready(Box::new(crate::state::profiles::Profile { guild: None, ..full })));
    }
    store.take_commands();
    let images = crate::images::Images::disabled(ctx.clone());
    let mut app = App::with_context(ctx, super::super::tests::offline_boot(Box::new(Offline), store, images));
    app.theme = Tokens::default();
    app.lang = Lang::Fr;
    app
}

fn walk(s: &egui::Shape, f: &mut impl FnMut(&egui::epaint::TextShape)) {
    match s {
        egui::Shape::Text(t) => f(t),
        egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, f)),
        _ => {}
    }
}

/// Une image : la carte de mon compte et l'édition rapide ; rend les textes
/// peints et la position (centre) de chacun. `switch` : « Changer de compte » choisi.
pub(in crate::ui) fn frame_with(app: &mut App, events: Vec<egui::Event>, switch: &mut bool) -> Vec<(String, egui::Pos2)> {
    frame_at(app, events, switch, PANEL)
}

/// [`frame_with`], le panneau utilisateur étant en `panel`.
fn frame_at(app: &mut App, events: Vec<egui::Event>, switch: &mut bool, panel: egui::Rect) -> Vec<(String, egui::Pos2)> {
    let ctx = app.ctx.clone();
    // Le temps avance (une image, un dixième de seconde) : les fondus d'ouverture se terminent.
    let input = egui::RawInput { time: Some(ctx.cumulative_frame_nr() as f64 / 10.0), screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 1600.0))), events, ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| {
        super::super::profile::install(ui.ctx(), &app.store.people);
        *switch |= app.account_card_ui(ui, panel);
        app.profile_editor_ui(ui);
        app.store.people.lock().end_frame();
    });
    out.textures_delta.clear();
    let mut texts = Vec::new();
    for c in &out.shapes {
        walk(&c.shape, &mut |t| texts.push((t.galley.job.text.clone(), t.pos + t.galley.mesh_bounds.center().to_vec2())));
    }
    texts
}

pub(in crate::ui) fn frame(app: &mut App, events: Vec<egui::Event>) -> Vec<(String, egui::Pos2)> {
    frame_with(app, events, &mut false)
}

pub(in crate::ui) fn has(texts: &[(String, egui::Pos2)], want: &str) -> bool {
    texts.iter().any(|(t, _)| t.contains(want))
}

/// Clic sur le texte `want` (appui à une image, relâchement à la suivante) ;
/// vrai si « Changer de compte » en est sorti.
pub(in crate::ui) fn click(app: &mut App, want: &str) -> bool {
    // Un contenu qui vient de changer de taille est replacé à l'image suivante.
    frame(app, Vec::new());
    let texts = frame(app, Vec::new());
    let pos = texts.iter().find(|(t, _)| t == want).map(|(_, p)| *p).unwrap_or_else(|| panic!("« {want} » absent de {:?}", texts.iter().map(|(t, _)| t).collect::<Vec<_>>()));
    click_at(app, pos)
}

pub(in crate::ui) fn click_at(app: &mut App, pos: egui::Pos2) -> bool {
    let button = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    let mut switch = false;
    frame_with(app, vec![egui::Event::PointerMoved(pos)], &mut switch);
    frame_with(app, vec![button(true)], &mut switch);
    frame_with(app, vec![button(false)], &mut switch);
    switch
}

pub(in crate::ui) fn escape() -> Vec<egui::Event> {
    vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]
}

/// Commandes du compte parties depuis la dernière lecture.
pub(in crate::ui) fn sent(app: &mut App) -> Vec<AccountCmd> {
    app.store
        .take_commands()
        .into_iter()
        .filter_map(|c| match c {
            Cmd::Account(a) => Some(a),
            _ => None,
        })
        .collect()
}

#[test]
fn card_shows_my_profile_and_its_actions() {
    let mut app = app();
    assert!(frame(&mut app, Vec::new()).is_empty(), "fermée : rien n'est dessiné");
    app.open_account_card();
    // Première image : mesure (rien n'est peint) ; la suivante montre la carte.
    frame(&mut app, Vec::new());
    let texts = frame(&mut app, Vec::new());
    for want in ["Pseudo", "moi.fictif", "iel", "Bio du serveur.", "Modifier le profil", "Statut", "Définir un statut personnalisé", "Changer de compte", "Copier l'identifiant"] {
        assert!(has(&texts, want), "« {want} » absent de {texts:?}");
    }
    // Des lignes de menu, plus de formulaire : ni titres de section, ni champ, ni boutons d'enregistrement.
    for gone in ["STATUT", "Effacer après", "Enregistrer", "Retirer le statut", "Quoi de neuf"] {
        assert!(!has(&texts, gone), "« {gone} » encore dans la carte : {texts:?}");
    }
    // Les statuts sont dans le sous-menu, avec leur description.
    assert!(!has(&texts, "Ne pas déranger"));
    click(&mut app, "Statut");
    frame(&mut app, Vec::new());
    let texts = frame(&mut app, Vec::new());
    for want in ["En ligne", "Inactif", "Ne pas déranger", "Invisible", "Vous ne recevrez aucune notification sur le bureau."] {
        assert!(has(&texts, want), "« {want} » absent de {texts:?}");
    }
    // Anglais : mêmes actions, traduites.
    app.lang = Lang::En;
    let texts = frame(&mut app, Vec::new());
    for want in ["Edit Profile", "Set Custom Status", "Do Not Disturb", "You will not receive any desktop notifications.", "Switch Accounts", "Copy user ID"] {
        assert!(has(&texts, want), "« {want} » absent de {texts:?}");
    }
}

#[test]
fn status_is_changed_from_its_submenu() {
    let mut app = app();
    app.open_account_card();
    frame(&mut app, Vec::new());
    assert!(sent(&mut app).is_empty(), "ouvrir la carte n'écrit rien");
    click(&mut app, "Statut");
    assert!(sent(&mut app).is_empty(), "ouvrir le sous-menu non plus");
    click(&mut app, "Ne pas déranger");
    let cmds = sent(&mut app);
    assert!(matches!(cmds.as_slice(), [AccountCmd::PatchProto { .. }]), "{cmds:?}");
    assert_eq!(app.own_status_code(), Some("dnd"), "optimiste");
    assert!(app.account_card_open(), "la carte reste ouverte");
    assert!(app.quick.sub.is_none(), "le sous-menu se referme sur le choix");
    // La ligne porte maintenant le statut choisi ; écriture en cours : un
    // deuxième statut n'est pas proposé (lignes du sous-menu inactives).
    click(&mut app, "Ne pas déranger");
    assert!(app.quick.sub.is_some(), "sous-menu rouvert par sa ligne");
    click(&mut app, "Invisible");
    assert!(sent(&mut app).is_empty());
}

#[test]
fn custom_status_is_set_in_its_window_and_cleared_from_the_card() {
    let mut app = app();
    app.open_account_card();
    frame(&mut app, Vec::new());
    click(&mut app, "Définir un statut personnalisé");
    assert!(!app.account_card_open() && app.custom_status_open(), "la fenêtre remplace la carte");
    frame(&mut app, Vec::new());
    let texts = frame(&mut app, Vec::new());
    for want in ["Définir un statut personnalisé", "Effacer après", "Annuler", "Enregistrer"] {
        assert!(has(&texts, want), "« {want} » absent de {texts:?}");
    }
    assert!(!has(&texts, "Retirer le statut"), "rien à retirer : {texts:?}");
    // Rien de saisi : « Enregistrer » est inactif.
    click(&mut app, "Enregistrer");
    assert!(sent(&mut app).is_empty() && app.custom_status_open());
    app.params.status.text = "  En pause ".into();
    app.params.status.emoji = "☕".into();
    app.params.status.expiry = 2;
    click(&mut app, "Enregistrer");
    assert!(!app.custom_status_open(), "enregistré : la fenêtre se ferme");
    let cmds = sent(&mut app);
    let [AccountCmd::PatchProto { id, .. }] = cmds.as_slice() else { panic!("{cmds:?}") };
    let saved = app.saved_custom_status().unwrap_or_default();
    assert_eq!((saved.text.as_str(), saved.emoji_name.as_str()), ("En pause", "☕"));
    assert!(saved.expires_at_ms > 0, "effacement dans une heure");
    // Ma présence montrée suit mes réglages.
    let mut p = None;
    app.own_presence_shown(&mut p);
    let custom = p.as_ref().and_then(Presence::custom).cloned().unwrap_or_else(|| panic!("{p:?}"));
    assert_eq!(custom.state.as_deref(), Some("En pause"));
    app.store.apply(Event::Account(AccountEvent::ProtoWritten { id: *id, result: Ok(crate::state::usersettings::ProtoReply { bytes: None, out_of_date: false }) }));
    // La carte propose maintenant de le modifier, et une croix le retire d'un clic.
    app.open_account_card();
    frame(&mut app, Vec::new());
    frame(&mut app, Vec::new());
    let texts = frame(&mut app, Vec::new());
    assert!(has(&texts, "Modifier le statut personnalisé") && has(&texts, "En pause"), "{texts:?}");
    let cross = app.ctx.read_response(clear_button_id()).map(|r| r.rect.center()).unwrap_or_else(|| panic!("croix absente"));
    click_at(&mut app, cross);
    assert!(matches!(sent(&mut app).as_slice(), [AccountCmd::PatchProto { .. }]));
    assert_eq!(app.saved_custom_status(), None);
    assert!(app.account_card_open() && !app.custom_status_open(), "la croix n'ouvre pas la fenêtre");
}

/// « Annuler » ou Échap ferment la fenêtre sans rien écrire, et le brouillon
/// abandonné ne revient pas à l'ouverture suivante.
#[test]
fn cancelling_the_custom_status_window_writes_nothing() {
    let mut app = app();
    app.open_custom_status();
    frame(&mut app, Vec::new());
    frame(&mut app, Vec::new());
    app.params.status.text = "Brouillon".into();
    click(&mut app, "Annuler");
    assert!(!app.custom_status_open() && sent(&mut app).is_empty());
    app.open_custom_status();
    frame(&mut app, Vec::new());
    frame(&mut app, Vec::new());
    assert_eq!(app.params.status.text, "", "brouillon abandonné oublié");
    frame(&mut app, escape());
    assert!(!app.custom_status_open() && sent(&mut app).is_empty());
    // Fenêtre fermée au repos : rien ne tourne.
    for _ in 0..4 {
        frame(&mut app, Vec::new());
    }
    assert!(!app.ctx.has_requested_repaint());
}

#[test]
fn edit_switch_and_copy() {
    let mut app = app();
    app.open_account_card();
    frame(&mut app, Vec::new());
    // « Changer de compte » ouvre son sous-menu ; « Gérer les comptes » demande le sélecteur.
    assert!(!click(&mut app, "Changer de compte"));
    assert!(app.account_card_open());
    assert!(click(&mut app, "Gérer les comptes"), "le sélecteur de compte est demandé");
    assert!(!app.account_card_open());

    app.open_account_card();
    frame(&mut app, Vec::new());
    assert!(!click(&mut app, "Modifier le profil"));
    assert!(!app.account_card_open() && app.profile_editor_open(), "l'édition rapide remplace la carte");
    app.quick.editor = None;

    app.open_account_card();
    frame(&mut app, Vec::new());
    click(&mut app, "Copier l'identifiant");
    assert!(app.toast.is_some() && app.account_card_open());
}

#[test]
fn escape_or_a_click_outside_closes_the_card() {
    let mut app = app();
    app.open_account_card();
    frame(&mut app, Vec::new());
    frame(&mut app, escape());
    assert!(!app.account_card_open());
    // Sous-menu ouvert : Échap le ferme d'abord, la carte ensuite.
    app.open_account_card();
    frame(&mut app, Vec::new());
    click(&mut app, "Statut");
    assert!(app.quick.sub.is_some());
    frame(&mut app, escape());
    assert!(app.quick.sub.is_none() && app.account_card_open());
    frame(&mut app, escape());
    assert!(!app.account_card_open());
    app.open_account_card();
    frame(&mut app, Vec::new());
    click_at(&mut app, egui::pos2(900.0, 100.0));
    assert!(!app.account_card_open());
    // Rien ne tourne au repos : aucune image n'est redemandée.
    app.open_account_card();
    for _ in 0..4 {
        frame(&mut app, Vec::new());
    }
    assert!(!app.ctx.has_requested_repaint(), "carte ouverte au repos");
    // Sous-menu ouvert au repos (pointeur sur sa ligne) : rien non plus.
    click(&mut app, "Statut");
    for _ in 0..12 {
        frame(&mut app, Vec::new());
    }
    assert!(app.quick.sub.is_some());
    assert!(!app.ctx.has_requested_repaint(), "sous-menu ouvert au repos");
}

/// Le sous-menu part à droite de la carte ; sans place à droite, à gauche :
/// il reste entier dans la fenêtre, sans recouvrir la carte.
#[test]
fn submenu_stays_inside_the_window() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 1600.0));
    for panel_left in [72.0, 700.0] {
        let mut app = app();
        let panel = egui::Rect::from_min_max(egui::pos2(panel_left, 1548.0), egui::pos2(panel_left + 240.0, 1600.0));
        app.open_status_submenu();
        for _ in 0..4 {
            frame_at(&mut app, Vec::new(), &mut false, panel);
        }
        let rect = app.ctx.memory(|m| m.area_rect(sub_id(SubKind::Status))).unwrap_or_else(|| panic!("sous-menu absent"));
        assert!(screen.contains_rect(rect), "{rect:?} hors de {screen:?}");
        let card = app.ctx.memory(|m| m.area_rect(Id::unique("ferrocord.mon-compte"))).unwrap_or_else(|| panic!("carte absente"));
        assert!(!rect.intersects(card), "le sous-menu {rect:?} recouvre la carte {card:?}");
    }
}

fn over(bg: Color32, top: Color32) -> Color32 {
    let a = f32::from(top.a()) / 255.0;
    let f = |b: u8, t: u8| (f32::from(t) + f32::from(b) * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(f(bg.r(), top.r()), f(bg.g(), top.g()), f(bg.b(), top.b()))
}

/// Lignes de la carte lisibles sur le fond de leur bloc, au repos et
/// survolées : dans chaque préréglage, et sur un profil à thème foncé, clair
/// ou vif (en haut comme en bas du dégradé).
#[test]
fn rows_are_readable_on_every_card_background() {
    use crate::theme::{contrast_ratio, Preset, MIN_TEXT};
    let check = |what: &str, t: &Tokens, bg: Color32| {
        let block = over(bg, block_fill(t));
        assert_ne!(block, bg, "{what} : le bloc ne se détache pas du fond");
        let (_, ink, _) = menu::line_colors(t, false, true);
        assert!(contrast_ratio(ink, block) >= MIN_TEXT, "{what} : {:.2}", contrast_ratio(ink, block));
        let (fill, ink, _) = menu::line_colors(t, true, true);
        assert!(contrast_ratio(ink, fill) >= MIN_TEXT, "{what}, survol : {:.2}", contrast_ratio(ink, fill));
    };
    for p in Preset::ALL {
        let t = p.tokens();
        check(p.key(), &t, t.colors.bg1);
        for pair in [[0x5b2a86, 0x2d1b4e], [0xf6e7ff, 0xffd9ec], [0x112233, 0x445566], [0xffffff, 0x000000], [0xff7a00, 0x00c2ff], [0x808080, 0x808080]] {
            let th = super::super::cosmetics::Themed::new(&t, pair);
            for y in [0.0, 100_000.0] {
                check(&format!("{} thème {pair:06x?} y={y}", p.key()), &th.tokens, th.at(0.0, y));
            }
        }
    }
}

#[test]
fn own_presence_follows_my_settings() {
    let mut app = app();
    let mut p = None;
    app.own_presence_shown(&mut p);
    assert_eq!(p, None, "statut inconnu : rien n'est inventé");
    app.store.account.write(Key::Status, SValue::Str("invisible".into())).unwrap_or_else(|e| panic!("{e:?}"));
    app.own_presence_shown(&mut p);
    assert_eq!(p.map(|p| p.status), Some(Status::Offline));
}
