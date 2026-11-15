//! Tests de l'éditeur de profil : logique du brouillon, ordre des
//! écritures, refus affiché sur place, édition dans la carte, barre
//! d'enregistrement, éléments verrouillés, widgets ; fenêtre dessinée en mémoire.

use super::super::quick_profile::tests::{app, click, escape, frame, has, sent, GUILD, ME};
use super::*;
use crate::i18n::Lang;
use crate::state::cosmetics::GuildTag;
use crate::state::quests::Refused;
use crate::state::tags::{TagCmd, TagEvent};
use crate::state::usersettings::extra::{ExtraCmd, ExtraEvent};
use crate::state::usersettings::{AccountCmd, AccountEvent};
use crate::state::widgets::{WidgetCmd, WidgetEvent, WidgetGame};
use crate::state::{Cmd, Event, ProfileRefusal};

fn account(app: &mut App, e: AccountEvent) {
    let eff = app.store.apply(Event::Account(e));
    app.handle(eff);
}

fn event(app: &mut App, e: Event) {
    let eff = app.store.apply(e);
    app.handle(eff);
}

/// Profil du compte reçu (`GET /users/@me`) ; `premium` : 2 pour Nitro.
fn with_profile(app: &mut App, premium: u8) {
    let p = Profile { id: ME, username: "moi.fictif".into(), global_name: Some("Moi Fictif".into()), premium, bio: "Bio du compte.".into(), pronouns: "iel".into(), accent: Some(0x5865f2), ..Default::default() };
    account(app, AccountEvent::Profile(Ok(p)));
}

/// Éditeur ouvert et dessiné une fois (bases lues).
fn opened(premium: u8, guild: bool) -> App {
    let mut app = app();
    with_profile(&mut app, premium);
    app.open_profile_editor(guild);
    // Première image : mesure (rien n'est peint) ; la suivante place la fenêtre.
    frame(&mut app, Vec::new());
    frame(&mut app, Vec::new());
    assert!(sent(&mut app).is_empty(), "profil déjà connu : rien n'est demandé");
    app
}

fn editor(app: &mut App) -> &mut Editor {
    app.quick.editor.as_mut().unwrap_or_else(|| panic!("éditeur fermé"))
}

fn main_draft(app: &mut App) -> &mut ProfileDraft {
    &mut editor(app).main.as_mut().unwrap_or_else(|| panic!("profil du compte absent")).draft
}

fn guild_draft(app: &mut App) -> &mut GuildDraft {
    &mut editor(app).guild.as_mut().unwrap_or_else(|| panic!("profil du serveur absent")).1
}

/// Image de 8 × 4 mise dans le brouillon, sans passer par le fil de décodage.
fn stage(app: &mut App, target: ImageTarget) {
    let image = Arc::new(image::DynamicImage::new_rgba8(8, 4));
    let texture = app.ctx.load_texture("essai", egui::ColorImage::filled([8, 4], egui::Color32::WHITE), egui::TextureOptions::LINEAR);
    editor(app).images.insert(target, ImageEdit::New(Picked { image, texture, crop: Crop::default() }));
}

/// Mon profil complet porte deux widgets de jeux.
fn with_widgets(app: &mut App) -> Vec<GameWidget> {
    let mut current = GameWidget::new(WidgetKind::Current);
    current.id = Some(70);
    current.games = vec![WidgetGame::new(501), WidgetGame::new(502)];
    let mut favorite = GameWidget::new(WidgetKind::Favorite);
    favorite.games = vec![WidgetGame { game_id: 501, comment: Some("Mon préféré.".into()), tags: vec!["expert".into()] }];
    let list = vec![current, favorite];
    let mut people = app.store.people.lock();
    for guild in [None, Some(GUILD)] {
        people.profiles_mut().set_widgets(ME, guild, &list);
    }
    list
}

#[test]
fn guild_draft_sends_only_what_changed() {
    let text = ProfileText { bio: Some("bio".into()), pronouns: None, theme: Some([0x010203, 0x040506]), ..Default::default() };
    let base = GuildDraft::of(Some("Pseudo"), Some(&text));
    let mut d = base.clone();
    assert_eq!(d.edit(&base), (None, GuildProfileEdit::default()));
    d.nick = "  ".into();
    d.pronouns = "elle".into();
    d.theme = None;
    let (nick, edit) = d.edit(&base);
    assert_eq!(nick, Some(None), "pseudo vidé : retiré");
    assert_eq!(edit, GuildProfileEdit { bio: None, pronouns: Some("elle".into()), theme: Some(None) });
    d.nick = " Nouveau ".into();
    assert_eq!(d.edit(&base).0, Some(Some("Nouveau".into())));
    // Profil du serveur jamais rempli : tout est vide, rien ne part.
    let empty = GuildDraft::of(None, None);
    assert_eq!(empty.edit(&GuildDraft::default()), (None, GuildProfileEdit::default()));
}

#[test]
fn theme_colors_are_part_of_the_account_draft() {
    let base = Profile { bio: "bio".into(), ..Default::default() };
    let known = Some([0x112233, 0x445566]);
    let mut d = ProfileDraft::of(&base, known);
    assert!(d.edit(&base, known).is_empty());
    d.theme = Some([[0xff, 0, 0], [0, 0, 0xff]]);
    assert_eq!(d.edit(&base, known).theme, Some(Some([0xff0000, 0x0000ff])));
    d.theme = None;
    assert_eq!(d.edit(&base, known), ProfileEdit { theme: Some(None), ..Default::default() });
}

#[test]
fn a_new_base_moves_only_untouched_fields() {
    let old = ProfileDraft { global_name: "A".into(), bio: "bio".into(), ..Default::default() };
    let new = ProfileDraft { global_name: "B".into(), bio: "autre".into(), theme: Some([[1, 2, 3], [4, 5, 6]]), ..Default::default() };
    let mut d = ProfileDraft { bio: "en cours de saisie".into(), ..old.clone() };
    rebase(&mut d, &old, &new);
    assert_eq!((d.global_name.as_str(), d.bio.as_str()), ("B", "en cours de saisie"));
    // Couleurs du thème arrivées après l'ouverture : adoptées, jamais « retirées ».
    assert_eq!(d.theme, new.theme);
}

/// Les trois colonnes sont là ; la barre d'enregistrement n'apparaît qu'avec
/// un changement, que la carte montre à l'image même.
#[test]
fn window_shows_columns_card_and_save_bar() {
    let mut app = opened(2, false);
    with_widgets(&mut app);
    let texts = frame(&mut app, Vec::new());
    for want in ["Profil principal", "Plaque nominative", "Avatar et décoration", "Style du pseudonyme", "Thème et bannière", "Effet de profil", "Moi Fictif", "moi.fictif", "iel", "Envoyer un message", "Bio", "Bio du compte.", "Membre depuis", "Tableau", "Liste de souhaits", "Tes widgets", "Jeux du moment", "Jeu favori", "Mon préféré.", "Ajoute jusqu'à 5 jeux", "Définir un statut"] {
        assert!(has(&texts, want), "« {want} » absent de {texts:?}");
    }
    for absent in ["Attention, des modifications", "Enregistrer", "Réinitialiser", "Réservé à Nitro"] {
        assert!(!has(&texts, absent), "« {absent} » ne devrait pas être là : {texts:?}");
    }
    // La carte suit le brouillon, à l'image même ; la barre apparaît.
    main_draft(&mut app).bio = "Nouvelle bio.".into();
    main_draft(&mut app).global_name = "Autre Nom".into();
    let texts = frame(&mut app, Vec::new());
    for want in ["Attention, des modifications n'ont pas été enregistrées", "Réinitialiser", "Enregistrer", "Nouvelle bio.", "13/190", "Autre Nom"] {
        assert!(has(&texts, want), "« {want} » absent de {texts:?}");
    }
    // « Réinitialiser » rend le profil enregistré et retire la barre.
    click(&mut app, "Réinitialiser");
    let texts = frame(&mut app, Vec::new());
    assert!(has(&texts, "Bio du compte.") && !has(&texts, "Enregistrer"), "{texts:?}");
    assert!(sent(&mut app).is_empty(), "réinitialiser n'envoie rien");
    app.lang = Lang::En;
    let texts = frame(&mut app, Vec::new());
    for want in ["Main Profile", "Nameplate", "Display Name Style", "Board", "Wishlist", "Member Since"] {
        assert!(has(&texts, want), "« {want} » absent de {texts:?}");
    }
}

/// On écrit dans le profil lui-même : un clic dans la bio y met le curseur,
/// la frappe change le brouillon et fait apparaître la barre.
#[test]
fn fields_are_edited_in_place() {
    let mut app = opened(2, false);
    click(&mut app, "Bio du compte.");
    frame(&mut app, vec![egui::Event::Text("!".into())]);
    let bio = main_draft(&mut app).bio.clone();
    assert!(bio.contains('!') && bio.chars().count() == 15, "{bio:?}");
    let texts = frame(&mut app, Vec::new());
    assert!(has(&texts, "15/190") && has(&texts, "Enregistrer"), "{texts:?}");
    // Profil d'un serveur : le nom est le pseudo, la bio celle du serveur.
    let mut app = opened(2, true);
    let texts = frame(&mut app, Vec::new());
    assert!(has(&texts, "Pseudo") && has(&texts, "Bio du serveur.") && !has(&texts, "Plaque nominative") && !has(&texts, "Tes widgets"), "{texts:?}");
    click(&mut app, "Bio du serveur.");
    frame(&mut app, vec![egui::Event::Text("?".into())]);
    assert!(guild_draft(&mut app).bio.contains('?'));
    assert_eq!(main_draft(&mut app).bio, "Bio du compte.", "la bio du compte n'a pas bougé");
}

/// Ce que Discord réserve à Nitro est montré verrouillé, avec la raison et
/// le renvoi vers Discord ; rien ne s'achète, rien ne part.
#[test]
fn nitro_only_settings_are_locked_with_the_reason() {
    let mut app = opened(0, false);
    for picker in [Picker::Theme, Picker::Style, Picker::Banner] {
        editor(&mut app).picker = Some(picker);
        frame(&mut app, Vec::new());
        let texts = frame(&mut app, Vec::new());
        assert!(has(&texts, "Discord réserve ce réglage aux abonnés Nitro") && has(&texts, "Voir dans Discord"), "{picker:?} : {texts:?}");
        assert!(!has(&texts, "Thème personnalisé") && !has(&texts, "Changer…"), "{picker:?} : {texts:?}");
        // La bannière reste retirable sans Nitro (lot R), et sa couleur réglable.
        let remove = crate::i18n::table::s2::t::REMOVE_IMAGE.get(Lang::Fr);
        assert_eq!(has(&texts, remove), picker == Picker::Banner, "{picker:?} : {texts:?}");
    }
    assert!(editor(&mut app).steps().is_empty() && sent(&mut app).is_empty());
    // Avec Nitro : les réglages, sans verrou.
    let mut app = opened(2, false);
    editor(&mut app).picker = Some(Picker::Theme);
    frame(&mut app, Vec::new());
    let texts = frame(&mut app, Vec::new());
    assert!(has(&texts, "Thème personnalisé") && has(&texts, "Principale") && !has(&texts, "abonnés Nitro"), "{texts:?}");
    editor(&mut app).picker = Some(Picker::Style);
    frame(&mut app, Vec::new());
    let texts = frame(&mut app, Vec::new());
    for want in ["Aucun style", "Dégradé", "Néon"] {
        assert!(has(&texts, want), "« {want} » absent de {texts:?}");
    }
    click(&mut app, "Dégradé");
    let style = editor(&mut app).style.clone().flatten().unwrap_or_else(|| panic!("style attendu"));
    assert_eq!((style.effect_id, style.colors.len()), (2, 2), "un dégradé a deux couleurs");
    // Échap ferme le sélecteur, pas l'éditeur (qui garde son brouillon).
    frame(&mut app, escape());
    frame(&mut app, Vec::new());
    assert!(app.profile_editor_open() && editor(&mut app).picker.is_none());
    assert_eq!(editor(&mut app).steps(), vec![Step::Style(Some(style))]);
}

#[test]
fn save_sends_one_write_at_a_time_and_keeps_the_window() {
    let mut app = opened(2, false);
    app.store.guilds[0].tag = GuildTag::new(GUILD, "TAG", None);
    main_draft(&mut app).global_name = "Autre Nom".into();
    main_draft(&mut app).theme = None;
    editor(&mut app).images.insert(ImageTarget { kind: ImageKind::Banner, guild: None }, ImageEdit::Remove);
    editor(&mut app).on_guild = true;
    guild_draft(&mut app).nick = "Surnom".into();
    guild_draft(&mut app).bio = "Ici.".into();
    editor(&mut app).tag = Some(Some(GUILD));
    let style = StyleEdit { font_id: 11, effect_id: 1, colors: vec![0xff0000] };
    editor(&mut app).style = Some(Some(style.clone()));
    click(&mut app, "Enregistrer");
    // Profil du compte d'abord, seul.
    let cmds = sent(&mut app);
    let [AccountCmd::PatchProfile { id, edit }] = cmds.as_slice() else { panic!("{cmds:?}") };
    assert_eq!(edit, &ProfileEdit { global_name: Some(Some("Autre Nom".into())), theme: Some(None), ..Default::default() });
    assert!(has(&frame(&mut app, Vec::new()), "Enregistrement…"));
    assert!(app.store.take_commands().is_empty(), "une écriture à la fois");
    account(&mut app, AccountEvent::ProfileWritten { id: *id, result: Ok(()) });
    frame(&mut app, Vec::new());
    assert_eq!(sent(&mut app), vec![AccountCmd::SetNick { guild: GUILD, nick: Some("Surnom".into()) }]);
    account(&mut app, AccountEvent::NickWritten { guild: GUILD, result: Ok(()) });
    frame(&mut app, Vec::new());
    let cmds = sent(&mut app);
    let [AccountCmd::PatchGuildProfile { id, guild: GUILD, edit }] = cmds.as_slice() else { panic!("{cmds:?}") };
    assert_eq!(edit, &GuildProfileEdit { bio: Some("Ici.".into()), ..Default::default() });
    account(&mut app, AccountEvent::GuildProfileWritten { id: *id, guild: GUILD, result: Ok(()) });
    frame(&mut app, Vec::new());
    let target = ImageTarget { kind: ImageKind::Banner, guild: None };
    assert_eq!(sent(&mut app), vec![AccountCmd::Extra(ExtraCmd::SetImage { target, data: None })]);
    account(&mut app, AccountEvent::Extra(ExtraEvent::ImageWritten { target, removed: true, result: Ok(()) }));
    frame(&mut app, Vec::new());
    // Puis le tag de serveur et le style du nom, par leurs requêtes à eux.
    assert_eq!(app.store.take_commands(), vec![Cmd::Tags(TagCmd::Set { guild: Some(GUILD) })]);
    event(&mut app, Event::Tags(TagEvent::Set { guild: Some(GUILD), result: Ok(()) }));
    frame(&mut app, Vec::new());
    assert_eq!(app.store.take_commands(), vec![Cmd::Tags(TagCmd::Style { style: Some(style.clone()) })]);
    assert!(app.toast.is_none(), "encore une réponse attendue");
    event(&mut app, Event::Tags(TagEvent::Styled { style: Some(style), result: Ok(()) }));
    let texts = frame(&mut app, Vec::new());
    // Tout est passé : la fenêtre reste, comme dans Discord, sans rien à enregistrer.
    assert!(app.profile_editor_open() && editor(&mut app).steps().is_empty());
    assert!(!has(&texts, "Attention, des modifications") && !has(&texts, "Enregistrer"), "{texts:?}");
    assert_eq!(app.toast.as_ref().map(|t| t.text.as_ref()), Some("Profil enregistré."));
    assert!(app.store.take_commands().is_empty());
    // Ce que Ferrocord garde de mon profil a suivi, sans nouvelle demande.
    assert_eq!(app.store.own_tag().map(|t| t.guild_id), Some(GUILD));
    assert!(app.store.own_name_style().is_some());
    let people = app.store.people.lock();
    let kept = people.profiles().profile(ME, Some(GUILD)).and_then(ProfileState::profile).cloned().unwrap_or_default();
    assert_eq!(kept.user.theme, None);
    assert_eq!(kept.guild.and_then(|g| g.bio).as_deref(), Some("Ici."));
}

#[test]
fn refusal_is_shown_in_place_and_keeps_the_draft() {
    let mut app = opened(2, false);
    main_draft(&mut app).global_name = "Autre Nom".into();
    main_draft(&mut app).bio = "Nouvelle bio.".into();
    editor(&mut app).images.insert(ImageTarget { kind: ImageKind::Avatar, guild: None }, ImageEdit::Remove);
    click(&mut app, "Enregistrer");
    let cmds = sent(&mut app);
    let [AccountCmd::PatchProfile { id, .. }] = cmds.as_slice() else { panic!("{cmds:?}") };
    account(&mut app, AccountEvent::ProfileWritten { id: *id, result: Err(Problem::Profile(ProfileRefusal::NameRateLimit)) });
    let texts = frame(&mut app, Vec::new());
    assert!(has(&texts, "Non enregistré\u{a0}: Vous changez de nom trop souvent"), "{texts:?}");
    assert!(sent(&mut app).is_empty(), "la suite n'est pas envoyée");
    assert!(app.profile_editor_open());
    assert_eq!(main_draft(&mut app).global_name, "Autre Nom", "brouillon gardé");
    assert!(has(&texts, "Enregistrer") && has(&texts, "Réinitialiser"));
    // En anglais, le même refus, dans la langue de l'interface.
    app.lang = Lang::En;
    assert!(has(&frame(&mut app, Vec::new()), "Not saved: You are changing your name too often"));
    // Nouvel essai : tout repart (le profil a été rétabli par le refus).
    app.lang = Lang::Fr;
    click(&mut app, "Enregistrer");
    assert!(matches!(sent(&mut app).as_slice(), [AccountCmd::PatchProfile { .. }]));
}

/// Un captcha arrête l'enregistrement et le dit ; la suite ne part pas.
#[test]
fn captcha_stops_the_save_and_says_so() {
    let mut app = opened(2, false);
    app.store.guilds[0].tag = GuildTag::new(GUILD, "TAG", None);
    let list = with_widgets(&mut app);
    frame(&mut app, Vec::new());
    editor(&mut app).tag = Some(Some(GUILD));
    let Some(draft) = editor(&mut app).widgets_mut() else { panic!("widgets connus") };
    draft[0].games.remove(1);
    let wanted: Vec<GameWidget> = editor(&mut app).widgets.clone().unwrap_or_default();
    assert_eq!(editor(&mut app).steps(), vec![Step::Tag(Some(GUILD)), Step::Widgets(wanted.clone())]);
    app.store.take_commands();
    click(&mut app, "Enregistrer");
    assert_eq!(app.store.take_commands(), vec![Cmd::Tags(TagCmd::Set { guild: Some(GUILD) })]);
    event(&mut app, Event::Tags(TagEvent::Set { guild: Some(GUILD), result: Err(Refused::Captcha) }));
    let texts = frame(&mut app, Vec::new());
    assert!(has(&texts, "Discord demande une vérification (captcha)"), "{texts:?}");
    assert!(app.store.take_commands().is_empty(), "les widgets ne partent pas");
    assert_eq!(editor(&mut app).steps().len(), 2, "brouillon gardé");
    // Sans le tag, les widgets s'enregistrent : la réponse de Discord devient la base.
    editor(&mut app).tag = None;
    click(&mut app, "Enregistrer");
    assert_eq!(app.store.take_commands(), vec![Cmd::Widgets(WidgetCmd::Save { widgets: wanted.clone() })]);
    event(&mut app, Event::Widgets(WidgetEvent::Saved(Ok(wanted.clone()))));
    frame(&mut app, Vec::new());
    assert!(editor(&mut app).steps().is_empty() && editor(&mut app).widgets.is_none());
    assert_eq!(editor(&mut app).worn.widgets.as_ref(), Some(&wanted));
    assert_ne!(wanted, list);
}

/// Widgets : « + » ouvre le choix d'un jeu (suggestions demandées à ce
/// moment), un jeu choisi entre dans le brouillon ; un widget qui n'est pas
/// compris les rend tous non modifiables.
#[test]
fn widgets_are_edited_in_the_draft_only() {
    let mut app = opened(2, false);
    with_widgets(&mut app);
    frame(&mut app, Vec::new());
    assert_eq!(app.store.take_commands(), vec![Cmd::Widgets(WidgetCmd::Games { ids: vec![501, 502] })], "noms des jeux demandés une fois");
    event(&mut app, Event::Widgets(WidgetEvent::Games { ids: vec![501, 502], result: Ok(vec![(501, crate::state::widgets::Game { name: "Course Fictive".into(), icon: None, cover: None })]) }));
    let texts = frame(&mut app, Vec::new());
    assert!(has(&texts, "Course Fictive") && has(&texts, "Jeu inconnu"), "{texts:?}");
    editor(&mut app).picker = Some(Picker::Game(WidgetKind::Played));
    frame(&mut app, Vec::new());
    assert_eq!(app.store.take_commands(), vec![Cmd::Widgets(WidgetCmd::Suggested)]);
    event(&mut app, Event::Widgets(WidgetEvent::Suggested(Ok(crate::state::widgets::Suggested { games: vec![501], wishlist: Vec::new() }))));
    frame(&mut app, Vec::new());
    let texts = frame(&mut app, Vec::new());
    assert!(has(&texts, "Suggestions de Discord") && has(&texts, "Rechercher un jeu"), "{texts:?}");
    // Le jeu suggéré est sur la carte et dans la liste : c'est la ligne de la liste (la dernière peinte) qu'on clique.
    let at = texts.iter().rev().find(|(t, _)| t == "Course Fictive").map(|(_, p)| *p).unwrap_or_else(|| panic!("suggestion absente"));
    super::super::quick_profile::tests::click_at(&mut app, at);
    let draft = editor(&mut app).widgets.clone().unwrap_or_default();
    let played = draft.iter().find(|w| w.kind == WidgetKind::Played).unwrap_or_else(|| panic!("widget créé"));
    assert_eq!(played.games, vec![WidgetGame::new(501)]);
    assert!(editor(&mut app).picker.is_none() && app.store.take_commands().is_empty(), "rien ne part avant « Enregistrer »");
    // Un widget reçu que Ferrocord ne comprend pas : lecture seule.
    let mut app = opened(2, false);
    with_widgets(&mut app);
    for guild in [None, Some(GUILD)] {
        let known = app.store.people.lock().profiles().profile(ME, guild).cloned();
        if let Some(ProfileState::Ready(p)) = known {
            app.store.people.lock().profiles_mut().set(ME, guild, ProfileState::Ready(Box::new(crate::state::profiles::Profile { widgets_opaque: true, ..*p })));
        }
    }
    frame(&mut app, Vec::new());
    let texts = frame(&mut app, Vec::new());
    assert!(has(&texts, "n'est pas pris en charge"), "{texts:?}");
    assert!(editor(&mut app).widgets_mut().is_none());
}

#[test]
fn picked_image_is_cropped_encoded_then_sent() {
    let mut app = opened(2, false);
    let target = ImageTarget { kind: ImageKind::Avatar, guild: None };
    stage(&mut app, target);
    assert_eq!(editor(&mut app).steps(), vec![Step::Image(target)]);
    // La carte montre l'image choisie : la zone gardée, carrée, au milieu.
    let swap = editor(&mut app).swap(ImageKind::Avatar, 1.0).unwrap_or_else(|| panic!("image de remplacement absente"));
    assert_eq!(swap.uv, egui::Rect::from_min_max(egui::pos2(0.25, 0.0), egui::pos2(0.75, 1.0)));
    // Le sélecteur de l'avatar propose le recadrage.
    editor(&mut app).picker = Some(Picker::Avatar);
    frame(&mut app, Vec::new());
    let texts = frame(&mut app, Vec::new());
    assert!(has(&texts, "Nouvelle image, envoyée à l'enregistrement.") && has(&texts, "Garder l'actuelle"), "{texts:?}");
    editor(&mut app).picker = None;
    click(&mut app, "Enregistrer");
    assert!(sent(&mut app).is_empty(), "codage en cours : rien n'est encore parti");
    // Le fil de codage rend l'image (attendu ici sans dormir : on le relit).
    let mut cmds = Vec::new();
    for _ in 0..2000 {
        frame(&mut app, Vec::new());
        cmds = sent(&mut app);
        if !cmds.is_empty() {
            break;
        }
        std::thread::yield_now();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let [AccountCmd::Extra(ExtraCmd::SetImage { target: t, data: Some(data) })] = cmds.as_slice() else { panic!("{cmds:?}") };
    assert!(*t == target && data.starts_with("data:image/png;base64,"));
    account(&mut app, AccountEvent::Extra(ExtraEvent::ImageWritten { target, removed: false, result: Err(Problem::Profile(ProfileRefusal::ImageTooLarge)) }));
    let texts = frame(&mut app, Vec::new());
    assert!(has(&texts, "Image trop lourde"), "{texts:?}");
    assert!(editor(&mut app).images.contains_key(&target), "image gardée dans le brouillon");
}

#[test]
fn closing_shakes_the_bar_when_there_are_unsaved_changes() {
    let mut app = opened(2, false);
    frame(&mut app, escape());
    assert!(!app.profile_editor_open(), "Échap ferme un éditeur sans changement");
    // Avec un brouillon : la fenêtre reste, la barre tremble (et passe en rouge).
    let mut app = opened(2, false);
    main_draft(&mut app).bio = "x".into();
    frame(&mut app, Vec::new());
    frame(&mut app, escape());
    assert!(app.profile_editor_open() && editor(&mut app).warned, "un brouillon ne se jette pas d'une touche");
    assert!(app.ctx.has_requested_repaint(), "le tremblement est animé");
    let soon = app.ctx.input(|i| i.time) + 0.1;
    assert!(editor(&mut app).shake_offset(soon).is_some());
    // Il ne dure pas : au repos ensuite.
    for _ in 0..8 {
        frame(&mut app, Vec::new());
    }
    assert!(!app.ctx.has_requested_repaint());
    click(&mut app, "Réinitialiser");
    assert!(!editor(&mut app).warned && sent(&mut app).is_empty());
    frame(&mut app, escape());
    assert!(!app.profile_editor_open(), "réinitialisé : Échap ferme");
    // « Plus d'options », dans la liste des profils : la page Profils des paramètres.
    let mut app = opened(2, false);
    click(&mut app, "Profil principal");
    click(&mut app, "Plus d'options");
    assert!(!app.profile_editor_open() && app.params.open && app.params.page() == Page::Profile);
    // Rien ne tourne au repos, fenêtre ouverte.
    let mut app = opened(2, false);
    for _ in 0..4 {
        frame(&mut app, Vec::new());
    }
    assert!(!app.ctx.has_requested_repaint());
}

#[test]
fn opening_asks_what_is_missing_once() {
    let mut app = app();
    app.store.people.lock().profiles_mut().clear();
    app.open_profile_editor(true);
    frame(&mut app, Vec::new());
    let texts = frame(&mut app, Vec::new());
    assert!(has(&texts, "Chargement du profil de ce serveur…"), "{texts:?}");
    let all = app.store.take_commands();
    assert_eq!(
        all,
        vec![Cmd::Profile(crate::state::profiles::ProfileCmd::Fetch { user_id: ME, guild_id: Some(GUILD) }), Cmd::Effects(crate::state::effects::EffectCmd::Owned), Cmd::Account(AccountCmd::LoadProfile)],
        "une demande de chaque, à l'ouverture seulement"
    );
    // Hors d'un serveur : le profil principal, et pas de profil de serveur affiché.
    let mut app = super::super::quick_profile::tests::app();
    app.store.guild = None;
    with_profile(&mut app, 0);
    app.open_profile_editor(true);
    frame(&mut app, Vec::new());
    let texts = frame(&mut app, Vec::new());
    assert!(has(&texts, "Profil principal") && has(&texts, "Plaque nominative") && !editor(&mut app).on_guild);
}

/// Les colonnes ne se chevauchent à aucune largeur : la colonne des widgets
/// passe sous le profil, celle de gauche se replie.
#[test]
fn columns_never_overlap() {
    let t = crate::theme::Tokens::default();
    let mut e = Editor::new(None, false);
    for (width, open, beside) in [(1500.0, true, true), (1000.0, true, false), (700.0, false, false), (420.0, false, false)] {
        for bar in [false, true] {
            let win = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(width, 800.0));
            let lay = Layout::of(&t, win, &e, bar);
            assert_eq!((lay.left_open, lay.right.is_some(), lay.board_below), (open, beside, !beside), "largeur {width}");
            let center = lay.center.unwrap_or_else(|| panic!("profil absent à {width}"));
            assert!(lay.left.right() <= center.left() && center.right() <= win.right(), "largeur {width}");
            assert!(lay.card_x >= center.left() && lay.card_x + lay.card_w <= center.right() + 0.5, "carte hors de sa zone à {width}");
            if let Some(right) = lay.right {
                assert!(lay.card_x + lay.card_w < right.left() && right.right() <= win.right(), "largeur {width}");
            }
            assert_eq!(lay.bar.is_some(), bar, "la barre reste tant que le profil est là : largeur {width}");
            if let Some(b) = lay.bar {
                assert!(b.left() >= lay.left.right() && b.right() <= win.right() && lay.bottom > b.height(), "largeur {width}");
            }
        }
    }
    // Dépliée de force dans une fenêtre étroite : elle la prend toute, le profil cède la place.
    e.left_open = Some(true);
    let win = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(480.0, 800.0));
    let lay = Layout::of(&t, win, &e, true);
    assert!(lay.center.is_none() && lay.right.is_none() && lay.bar.is_none() && lay.left == win);
    // Profil d'un serveur : pas de colonne de widgets, le profil se centre.
    e.left_open = None;
    e.here = Some(1);
    e.on_guild = true;
    let win = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1500.0, 800.0));
    let lay = Layout::of(&t, win, &e, false);
    assert!(lay.right.is_none() && !lay.board_below);
    let mid = (lay.left.right() + win.right()) / 2.0;
    assert!((lay.card_x + lay.card_w / 2.0 - mid).abs() <= 1.0);
}

/// Sélecteur de style : les polices viennent de la table du rendu
/// (`fonts::NAMES`) ; prisme et « gummy », lus mais sans valeur d'envoi
/// documentée, ne sont pas proposés, et un style qui en porte un n'est
/// modifiable qu'après le choix d'un effet proposé.
#[test]
fn style_picker_offers_the_font_table_and_only_writable_effects() {
    let mut app = opened(2, false);
    editor(&mut app).picker = Some(Picker::Style);
    frame(&mut app, Vec::new());
    click(&mut app, "Dégradé");
    frame(&mut app, Vec::new());
    let texts = frame(&mut app, Vec::new());
    for f in crate::ui::fonts::NAMES.iter() {
        assert_eq!(has(&texts, f.label), f.offered, "« {} » dans {texts:?}", f.label);
    }
    assert!(!has(&texts, "Prisme") && !has(&texts, "Gummy"), "{texts:?}");
    click(&mut app, "Sakura");
    assert_eq!(editor(&mut app).style.clone().flatten().map(|s| (s.font_id, s.effect_id)), Some((3, 2)));
    // Style porté avec l'effet « prisme » et une police retirée : montrés, le reste inactif.
    let prism = StyleEdit { font_id: 1, effect_id: 7, colors: vec![0xff0000, 0x0000ff] };
    editor(&mut app).style = Some(Some(prism.clone()));
    frame(&mut app, Vec::new());
    let texts = frame(&mut app, Vec::new());
    assert!(has(&texts, "Prisme") && has(&texts, "Bangers") && has(&texts, "ne sait pas l'enregistrer"), "{texts:?}");
    click(&mut app, "Sakura");
    assert_eq!(editor(&mut app).style.clone().flatten(), Some(prism), "inactif tant que l'effet n'est pas enregistrable");
    click(&mut app, "Néon");
    click(&mut app, "Sakura");
    assert_eq!(editor(&mut app).style.clone().flatten().map(|s| (s.font_id, s.effect_id, s.colors.len())), Some((3, 3, 1)));
}
