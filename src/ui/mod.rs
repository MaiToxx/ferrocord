//! Interface egui. L'interface est redessinée uniquement quand quelque chose
//! change (entrée utilisateur, événement réseau, thème ou langue modifiés) :
//! 0 % de CPU au repos.
//!
//! Elle ne lit que le magasin `state::Store` et parle au reste par une
//! petite interface de commandes ([`Backend`]) ; elle ne fait ni réseau ni
//! disque ni appel au coffre : le thème passe par `theme::ThemeLink`, les
//! réglages par `settings::SettingsLink` et les jetons par
//! `account::AccountLink`, qui travaillent chacun dans leur fil. Ses textes
//! viennent de `i18n`, dans la langue courante, sauf ceux des écrans de
//! comptes et de migration (`accounts.rs`, `login.rs`), encore en français.

mod accounts;
// Activité de jeu et présence enrichie (lot C3).
mod activity;
mod game_card;
// Rich Presence locale : réglage et applications branchées (lot R).
mod rpc;
mod appearance;
mod attachments;
mod channels;
// « Modifier le salon » (phase 3, lot A2).
mod channel_settings;
mod chat;
mod chat_head;
mod embeds;
mod complete;
mod composer;
mod desk;
mod composer_emoji;
mod emoji;
mod emoji_picker;
// Onglet GIF du sélecteur d'expressions (lot G).
mod gif_picker;
mod image_ops;
pub mod fonts;
// Fenêtre de l'installation et de la désinstallation (lot distrib).
#[cfg(windows)]
pub mod setup;
mod sticker_hints;
mod stickers;
mod friends;
mod friends_active;
// Navigation : sélecteur rapide, boîte de réception, raccourcis (lot navigation).
mod inbox;
mod inbox_cards;
mod keys;
// Liens Discord, instance unique, démarrage et mises à jour (lot C6).
mod links;
/// Personnalisation du client (lot PERSO).
mod look;
// Partage de la largeur entre les colonnes, fenêtre étroite (phase 10, lot V2).
mod layout;
mod quick_switcher;
mod groups;
mod login;
mod markdown;
mod members;
mod message;
mod msg_actions;
/// MP du compte système de Discord : étiquette « OFFICIEL », bandeau (lot « messages »).
mod official;
/// Carte d'une notification du compte système de Discord (lot « messages »).
mod safety_card;
mod pins;
// Page « Plugins » et publication de l'état des plugins (lot P0).
mod plugins;
mod prefs;
mod probe;
mod profile;
mod profile_full;
mod profile_parts;
// Carte de mon compte et édition rapide du profil.
mod profile_edit;
mod profile_edit_board;
mod profile_edit_card;
mod profile_edit_left;
mod quick_profile;
mod cosmetics;
mod rail;
mod rail_dnd;
mod rail_voice;
mod guild_create;
// Fenêtre « Paramètres du serveur » (phase 3).
pub mod guild_settings;
// Sanctions sur un membre : dialogues de modération (lots A4 + A5).
mod moderation;
mod invites_manage;
mod threads;
mod forum;
// Commandes, composants et fenêtres des applications (lot T11).
mod commands;
mod components;
mod modal;
mod invites;
mod polls;
mod system_msgs;
mod reactions;
mod settings;
mod reads;
// Recherche de messages et saut vers un message (lot T12).
mod search;
// Distance d'un cran de molette (réglage « Vitesse de défilement »).
pub mod scroll;
#[cfg(test)]
mod state_table;
mod upload;
mod motion;
mod util;
mod viewer;
mod vlist;
mod words;
// Événements programmés, accueil des nouveaux membres (lot A7).
mod events;
mod welcome;
// « Parcourir les salons » (lot fin-texte-restes).
mod browse;
// Vocal des serveurs (phase 4, lot V4).
mod voice;
// Appels privés, conférences, tableau de sons (lot V6).
mod call;
mod call_band;
mod soundboard;
mod stage;
// Vue d'appel, vidéo et directs (phase 5, lot M4).
mod video;
// Sélecteur de partage : écrans, fenêtres, caméras (phase 5, lot M3).
mod share_picker;
// Accessibilité, sélection des messages au clavier, bandeau du mode streamer (lot C5).
mod a11y;
mod msg_select;
mod streamer_banner;
// Bandeau du rapport de plantage local (lot E5).
mod crash_banner;
mod news_banner;
mod deck;
mod global_keys;
// Thèmes partageables : fichiers `.ferrotheme` (lot E5).
mod theme_share;
// Mica et transparence de Windows 11 (lot E5).
mod mica;
// Messages vocaux reçus : carte et lecteur (phase 6).
mod voice_note;
// Connexion par QR code depuis l'application mobile (phase 6, lot C2).
mod qr_login;
// Réveils décidés par la passerelle : ce que l'interface montre (lot C8).
mod interest;
// Mises en page des messages gardées d'une image à l'autre (lot C8).
mod text_cache;
// Disposition mobile : écrans, onglets, gestes (phase 7, lot P2).
mod mobile;
// Historique hors ligne : chargement, affichage avant READY, enregistrement (lot H).
mod offline;
// Journal des messages supprimés et modifiés (plugin, lot E1).
mod journal;
mod keywords;
mod export;
mod local_search;
// Indicateur de chargement redessiné 30 fois par seconde, pas à chaque image (lot C9).
mod spinner;
// Glyphes courants rastérisés pendant l'attente du premier salon (lot C9).
mod warm;
// Lot M : lecteur vidéo et audio du fil.
pub mod player;
// Fondations visuelles (lot V1) : composants partagés, lignes de menu, barre de titre.
mod widgets;
mod menu;
// Boutons et champs du gabarit pour les pages de formulaire (lot V).
mod form;
// Lot U : menu complet d'un utilisateur, demandes de message.
mod user_menu;
mod user_panel;
mod requests;
// Fonctions non documentées (lot ND) : effets de profil, quêtes.
mod effects;
mod effects_picker;
mod shop;
mod discovery;
mod summaries;
mod verification;
mod app_directory;
mod saved;
mod suggestions;
mod quests;
mod dm_profile;
mod status_emoji;
mod titlebar;
mod gallery;
/// Fenêtre des notifications d'un serveur (lot N).
mod notif_settings;
/// Lot FF : messages masqués des personnes bloquées ou ignorées.
mod age_gate;
mod hidden;
/// Lot FF : clic sur une mention de salon ou de rôle.
mod mentions;
/// Lot FF : aperçu de la mise en forme pendant la frappe.
mod composer_fmt;
/// Lot FF : sons par événement et leur page de réglages.
mod sounds;
/// Lot FF : menu « + » de la zone de saisie.
mod composer_plus;
/// Lot FF : menus contextuels de la saisie et des liens, long collage en fichier.
mod composer_menu;
// Plugins « discret et saisie » (lot E3).
mod stealth;
mod drafts;
mod scheduled;
mod translate;
/// Lot E2 : onglets de salons, vue scindée et fenêtres détachées.
mod tabs;
/// Lot E2 : palette de commandes et son registre d'actions.
mod palette;
/// Lot E2 : taille du texte par salon.
mod chat_zoom;
mod panes;
/// Lot FF2 : fenêtres de la seconde passe sur les manques fonctionnels.
mod ff2;
pub use guild_settings::Page as GuildPage;

use crate::account::{AccountKey, AccountLink, Survey, Token, TokenError, WipeCheck};
use crate::images::Images;
use crate::settings::{Settings, SettingsLink, SettingsUpdate};
use crate::i18n::{self, Lang, Text};
use crate::state::{Cmd, Effect, Event, Notice, Problem, Store};
use crate::theme::{Clock, ThemeLink, Tokens, Update, CHANNELS_WIDTH};
use accounts::{Confirm, Current, Phase, Storing};
use appearance::Appearance;
use eframe::egui::{self, Id};
use prefs::Prefs;
use std::{
    path::PathBuf,
    borrow::Cow,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

/// Heure murale (remplaçable dans les tests).
pub type WallClock = Arc<dyn Fn() -> SystemTime + Send + Sync>;

/// Durée d'affichage d'une notification.
const TOAST: Duration = Duration::from_secs(4);

/// Réveil de l'interface, appelable depuis n'importe quel fil.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Ce que l'interface attend du réseau : une connexion (réelle, ou simulée
/// par l'aperçu hors ligne). Une seule à la fois : `connect` remplace la
/// précédente.
pub trait Backend {
    /// Fonction à appeler quand des événements arrivent.
    fn set_wake(&mut self, wake: Wake);
    fn connect(&mut self, token: Token);
    /// Ferme la connexion ; aucun de ses événements n'arrive plus ensuite.
    fn disconnect(&mut self);
    fn is_connected(&self) -> bool;
    /// Événements arrivés depuis le dernier appel.
    fn poll(&mut self) -> Vec<Event>;
    fn send(&mut self, cmds: Vec<Cmd>);
    /// Ce que l'interface montre de la liste des membres : les mises à jour
    /// de liste hors de la vue ne la réveillent pas.
    fn set_list_watch(&mut self, _watch: Arc<crate::state::members::ListWatch>) {}
    /// Ce que l'interface montre du reste (salon ouvert, salons calmes) : les
    /// événements sans effet à l'écran ne la réveillent pas (lot C8).
    fn set_interest(&mut self, _interest: Arc<crate::state::interest::Interest>) {}
    /// Nouvelle tentative de connexion par QR code (lot C2) ; `None` si ce
    /// réseau ne sait pas le faire (aperçu hors ligne).
    fn remote_auth(&mut self) -> Option<crate::state::remote_auth::RemoteLogin> {
        None
    }
}

/// Tout ce que l'application reçoit au démarrage.
pub struct Boot {
    pub backend: Box<dyn Backend>,
    pub store: Store,
    /// Lien vers `theme.toml` et résultat de sa première lecture (faite
    /// avant la fenêtre, pour que la première image ait le bon thème).
    pub theme: (ThemeLink, Option<Update>),
    /// Lien vers `settings.toml` et résultat de sa première lecture.
    pub settings: (SettingsLink, Option<SettingsUpdate>),
    /// Fil des comptes (coffre et migration).
    pub accounts: AccountLink,
    /// Jeton de `DISCORD_TOKEN` : session éphémère, jamais enregistrée.
    /// `Some(Err)` : la variable ne contient pas un jeton (refusé, signalé).
    pub env_token: Option<Result<Token, TokenError>>,
    /// Ancien fichier de jeton en clair, à migrer vers le coffre.
    pub legacy: Option<PathBuf>,
    /// Pipeline d'images, créé avec le même contexte egui.
    pub images: Images,
}

/// Ce que dit une notification : une valeur typée ou un texte constant
/// (remis en mots si la langue change pendant qu'ils sont affichés), ou un
/// texte déjà fait (venu d'ailleurs, laissé tel quel).
#[derive(Clone, Debug, PartialEq)]
enum ToastText {
    Notice(Notice),
    Const(Text),
    Text(String),
}

impl From<Text> for ToastText {
    fn from(t: Text) -> Self {
        ToastText::Const(t)
    }
}

impl From<Notice> for ToastText {
    fn from(n: Notice) -> Self {
        ToastText::Notice(n)
    }
}

impl From<String> for ToastText {
    fn from(s: String) -> Self {
        ToastText::Text(s)
    }
}

/// Textes des écrans de comptes et de migration : encore en français, en dur.
impl From<&'static str> for ToastText {
    fn from(s: &'static str) -> Self {
        ToastText::Text(s.to_string())
    }
}

/// Notification affichée : sa source, son texte rendu une fois (sans
/// allocation pour un texte constant), son heure.
struct Toast {
    source: ToastText,
    text: Cow<'static, str>,
    at: Instant,
}

pub struct App {
    ctx: egui::Context,
    backend: Box<dyn Backend>,
    store: Store,
    /// Avatars et icônes (chargés hors du fil d'interface).
    images: Images,
    /// Jetons prêts à dessiner (densité appliquée).
    theme: Tokens,
    look: Appearance,
    /// Fenêtre Paramètres (roue dentée, Ctrl+,).
    params: settings::SettingsWindow,
    prefs: Prefs,
    accounts: AccountLink,

    /// Où en est l'ouverture d'une session.
    phase: Phase,
    /// Début de l'étape en cours (temps écoulé affiché).
    phase_since: Instant,
    /// Session ouverte (ou en cours d'ouverture).
    current: Option<Current>,
    env_token: Option<Token>,
    legacy: Option<PathBuf>,
    /// Jeton du fichier de jeton en clair proposé pour une session (migration
    /// impossible, ou jeton du coffre refusé alors que le fichier en a un autre).
    legacy_token: Option<Token>,
    /// `legacy_token` est proposé après le refus du jeton du coffre
    /// (« Essayer le jeton du fichier ») : enregistré après READY.
    legacy_newer: bool,
    /// Jeton du fichier quand le coffre en avait un autre (migration non faite).
    file_token: Option<Token>,
    /// Le fichier de jeton en clair est encore sur le disque et reste à
    /// supprimer par l'utilisateur : pourquoi (avertissement durable).
    legacy_file: Option<String>,
    /// Suppression de l'ancien fichier en cours.
    wiping_legacy: bool,
    /// Étape 2 de la migration à faire après READY du compte importé : le
    /// jeton que fichier et coffre partagent.
    finish_pending: Option<Token>,
    /// READY obtenu pour l'étape 2 : elle attend l'heure de Discord (B1).
    finish_ready: bool,
    /// Étape 2 en cours sur le fil des comptes.
    finishing: bool,
    /// `DISCORD_TOKEN` abîmé : signalé à part, il n'empêche rien.
    env_warning: Option<String>,
    /// Import du fichier refusé (compte importé retiré) : « Importer » est proposé.
    legacy_declined: bool,
    /// « Importer » demandé, réponse attendue.
    importing: bool,
    /// Ce que le coffre contient du jeton du fichier (confirmation de suppression).
    wipe_check: Option<WipeCheck>,
    /// Lecture du coffre en cours avant cette confirmation.
    checking_wipe: bool,
    /// État relu (coffre et fichier) que décrit la question de suppression.
    wipe_survey: Option<Survey>,
    /// État relu que décrit la question de retrait (ou de déconnexion) de ce compte.
    remove_survey: Option<(AccountKey, Survey)>,
    /// Relecture en cours pour cette question.
    surveying: Option<AccountKey>,
    /// Le fichier a été vidé mais pas supprimé : vestige sans jeton.
    legacy_vestige: bool,
    /// Ce processus a écrit le jeton du fichier dans le coffre : pas d'étape 2.
    wrote_copy: bool,
    /// Confirmations refusées parce que l'état avait changé (question reposée).
    stale_count: usize,
    /// Jeton saisi d'un compte déjà enregistré avec un autre jeton : question en attente.
    replacing: Option<accounts::Replacing>,
    /// Dernier jeton enregistré refusé par Discord (« Essayer le jeton du fichier » peut le remplacer).
    refused: Option<(AccountKey, Token)>,
    /// Heure murale du démarrage (première image), en secondes.
    start_secs: Option<u64>,
    /// Heure murale (instant de la copie, délai de l'étape 2).
    clock: WallClock,
    /// Valeur d'avant qu'une écriture ratée n'a pas pu remettre, gardée en
    /// mémoire pour « Recopier le jeton d'avant ».
    recovery: Option<(AccountKey, Token)>,
    /// Ce que l'écriture ratée a laissé dans cette entrée (relu) : « Recopier »
    /// n'écrit que par-dessus ceci (ou une entrée vide).
    recovery_left: Option<Token>,
    /// Comptes dont la suppression du coffre est en cours.
    forgetting: Vec<AccountKey>,
    /// Jetons saisis, validés par READY, en cours d'écriture dans le coffre.
    storing: Vec<Storing>,
    /// Action irréversible attendant son deuxième clic.
    confirm: Option<Confirm>,
    /// Sélecteur de compte ouvert (au-dessus du pied utilisateur).
    menu_open: bool,
    /// Carte de mon compte et édition rapide du profil.
    quick: quick_profile::QuickUi,
    /// Langue des textes rendus (magasin, notification, erreur de connexion) ;
    /// comparée à `i18n::language()` à chaque image : une lecture atomique.
    lang: Lang,

    token_input: String,
    remember: bool,
    /// Erreur affichée par l'écran de connexion, et sa source (remise en
    /// mots si la langue change).
    login_error: Option<String>,
    login_problem: Option<Problem>,

    /// Zone de saisie : texte en cours, brouillons, autocomplétion.
    input: composer::Composer,
    /// Pièces jointes en attente, par salon, et fil des ajouts (lot T8).
    uploads: upload::UploadUi,
    toast: Option<Toast>,
    /// Pages Rôles et Membres de « Paramètres du serveur » : brouillons, dialogues (lot A3).
    roles_admin: guild_settings::roles::RolesUi,
    scroll_to_bottom: bool,
    /// Liste de messages virtualisée (positions et hauteurs par salon).
    list: chat::MessageList,
    /// Réponse, modification, sélecteur, épingles (lots T5/T6).
    actions: msg_actions::ActionsUi,
    /// Sélecteur d'emojis : récents, teinte, recherche (lot T4).
    emojis: emoji_picker::EmojiUi,
    /// Liste des membres (panneau de droite).
    member_view: members::MembersView,
    /// Zone du salon à cette image (interface principale) : les notifications
    /// s'y placent, jamais sous le panneau des salons ni sous le sélecteur de compte.
    chat_rect: Option<egui::Rect>,
    /// Page Amis (onglet, recherche, confirmations).
    friends: friends::FriendsPage,
    /// Panneau des membres d'un MP de groupe.
    groups: groups::GroupPanel,
    /// Demandes de message (lot U).
    requests: requests::Requests,
    /// Scènes des effets de profil affichés (lot ND).
    profile_fx: effects::Stage,
    /// Page des quêtes (lot ND).
    quests: quests::QuestsPage,
    discover: discovery::DiscoverPage,
    /// Salon dont le panneau « Résumés » est ouvert (lot ND, `ui/summaries.rs`).
    summaries_open: Option<u64>,
    /// Fenêtre « Règles et vérification » (lot ND, `ui/verification.rs`).
    verify: Option<verification::VerifyWindow>,
    /// Notifications de bureau, pastille, zone de notification (lot S5).
    desk: desk::DeskUi,
    /// Fils et forums (lot T10).
    threads: threads::ThreadsUi,

    /// Réglages de cet ordinateur (accessibilité, mode streamer, journal).
    local: settings::local::LocalUi,
    /// Page « Voix et vidéo » et son service audio (lot V5).
    voice_cfg: settings::voice::VoiceUi,
    /// Sélecteur de partage et son service de capture (lot M3).
    share: share_picker::ShareUi,
    /// Rail (dossiers, glisser-déposer) et fenêtres des serveurs (lot S7 + A1).
    servers: guild_create::ServersUi,
    /// Actions de modération, sélection de messages, pages (lots A4 + A5).
    moderation: moderation::ModerationUi,
    /// Champ de recherche et panneau des résultats (lot T12).
    search: search::SearchUi,
    /// Raccourcis, historique, sélecteur rapide, boîte de réception (lot navigation).
    nav: keys::NavUi,
    /// Commandes slash et fenêtres des applications (lot T11).
    cmds: commands::CommandsUi,
    /// Sondages, annonces, invitations (lot T13).
    polls_ui: polls::PollsUi,
    /// Fenêtre « Paramètres du serveur » (phase 3).
    guild_settings: guild_settings::GuildSettingsUi,
    /// « Modifier le salon », création, clonage, suppression, glisser-déposer (lot A2).
    chan_ui: channel_settings::ChannelUi,
    /// Connexion par QR code (lot C2).
    qr: qr_login::QrLogin,
    /// Événements programmés, accueil des nouveaux membres (lot A7).
    events_ui: events::EventsUi,
    /// Vocal des serveurs (phase 4, lot V4).
    voice: voice::VoiceUi,
    /// Appels, conférences, tableau de sons (lot V6).
    calls_ui: call::CallsUi,
    /// Vue d'appel, vidéo et directs (phase 5, lot M4).
    video_ui: video::VideoUi,
    /// Installation, liens et mises à jour (lot C6).
    install: links::InstallUi,
    /// Ce que la passerelle sait de l'écran pour ne pas réveiller l'interface (lot C8).
    interest: interest::Watch,
    /// Disposition mobile (phase 7, lot P2).
    mobile: mobile::MobileUi,
    /// Historique hors ligne (lot H).
    offline: offline::OfflineUi,
    /// Journal des messages supprimés et modifiés, fichiers des plugins locaux (lot E1).
    journal: journal::JournalUi,
    keywords: keywords::KeywordsUi,
    local_search: local_search::LocalSearchUi,
    export: export::ExportUi,
    stealth: stealth::StealthUi,
    drafts: drafts::DraftsUi,
    scheduled: scheduled::ScheduledUi,
    translate: translate::TranslateUi,
    /// Glyphes courants préparés avant le premier salon (lot C9).
    warm: warm::Warm,
    /// Planche des composants, pour l'aperçu hors ligne (lot V1).
    gallery: Option<gallery::Gallery>,
    /// Place des colonnes et du panneau utilisateur (phase 10, lot V2).
    place: layout::Place,
    /// Fond personnalisé, polices choisies, thèmes enregistrés (lot PERSO).
    fx: look::LookFx,
    /// Rapport de plantage local (lot E5).
    crash: crash_banner::CrashUi,
    news: news_banner::NewsUi,
    global_keys: global_keys::GlobalKeys,
    deck: deck::DeckUi,
    /// Mica et transparence de Windows 11 (lot E5).
    mica: mica::MicaUi,
    /// Onglets de salons (plugin, lot E2).
    tabs: tabs::TabsUi,
    /// Palette de commandes (plugin, lot E2).
    palette: palette::Palette,
    /// Zoom par salon (plugin, lot E2).
    zoom: chat_zoom::ZoomUi,
    /// Vue scindée et fenêtres détachées (plugins, lot E2).
    panes: panes::PanesUi,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, boot: Boot) -> Self {
        // Segoe UI (vrai gras, italique) lue dans un fil à part.
        fonts::install(&cc.egui_ctx);
        titlebar::attach(cc);
        Self::with_context(cc.egui_ctx.clone(), boot)
    }

    /// Comme [`App::new`], sans fenêtre (tests de bout en bout).
    pub fn with_context(ctx: egui::Context, boot: Boot) -> Self {
        let Boot { mut backend, mut store, theme, settings, accounts, env_token, legacy, images } = boot;
        let wake_ctx = ctx.clone();
        backend.set_wake(Arc::new(move || wake_ctx.request_repaint()));
        backend.set_list_watch(store.members.watch());
        // Distance d'un cran de molette avant même la lecture de `local.toml`.
        scroll::apply(&ctx, None, crate::state::local::LocalPrefs::default().scroll_speed);
        let interest = interest::Watch::default();
        backend.set_interest(interest.shared());
        let (link, first) = theme;
        let wake_ctx = ctx.clone();
        link.set_wake(move || wake_ctx.request_repaint());
        let wake_ctx = ctx.clone();
        accounts.set_wake(move || wake_ctx.request_repaint());
        let wake_ctx = ctx.clone();
        i18n::set_wake(move || wake_ctx.request_repaint());
        let lang = i18n::language();

        let (mut look, notice) = Appearance::new(link, first);
        let (prefs, prefs_notice) = Prefs::new(settings.0, settings.1);
        let wake_ctx = ctx.clone();
        prefs.set_wake(move || wake_ctx.request_repaint());
        let params = settings::SettingsWindow::reopened(prefs.now.appearance_open);
        let theme = look.edited.scaled();
        look.request_zoom(&ctx, theme.layout.zoom);
        theme.apply(&ctx);
        fonts::set_family(&ctx, theme.typography.family);
        settings::shell::remember(&ctx, &theme);
        store.set_clock12(theme.layout.clock == Clock::H12);
        store.set_language(lang);

        let mut app = App {
            ctx,
            backend,
            store,
            images,
            theme,
            look,
            params,
            prefs,
            accounts,
            phase: Phase::Starting,
            phase_since: Instant::now(),
            current: None,
            env_token: None,
            legacy,
            legacy_token: None,
            legacy_newer: false,
            file_token: None,
            legacy_file: None,
            wiping_legacy: false,
            finish_pending: None,
            finish_ready: false,
            finishing: false,
            env_warning: None,
            legacy_declined: false,
            importing: false,
            wipe_check: None,
            checking_wipe: false,
            wipe_survey: None,
            remove_survey: None,
            surveying: None,
            legacy_vestige: false,
            wrote_copy: false,
            stale_count: 0,
            replacing: None,
            refused: None,
            start_secs: None,
            clock: Arc::new(SystemTime::now),
            recovery: None,
            recovery_left: None,
            forgetting: Vec::new(),
            storing: Vec::new(),
            confirm: None,
            menu_open: false,
            quick: Default::default(),
            lang,
            token_input: String::new(),
            remember: true,
            login_error: None,
            login_problem: None,
            input: Default::default(),
            uploads: Default::default(),
            toast: None,
            roles_admin: Default::default(),
            scroll_to_bottom: false,
            list: Default::default(),
            actions: Default::default(),
            emojis: Default::default(),
            member_view: Default::default(),
            chat_rect: None,
            friends: Default::default(),
            groups: Default::default(),
            requests: Default::default(),
            profile_fx: Default::default(),
            quests: Default::default(),
            discover: Default::default(),
            summaries_open: None,
            verify: None,
            desk: Default::default(),
            threads: Default::default(),

            local: Default::default(),
            voice_cfg: Default::default(),
            share: Default::default(),
            servers: Default::default(),
            moderation: Default::default(),
            search: Default::default(),
            nav: Default::default(),
            cmds: Default::default(),
            polls_ui: Default::default(),
            guild_settings: Default::default(),
            chan_ui: Default::default(),
            qr: Default::default(),
            events_ui: Default::default(),
            voice: Default::default(),
            calls_ui: Default::default(),
            video_ui: Default::default(),
            install: Default::default(),
            interest,
            mobile: Default::default(),
            offline: Default::default(),
            journal: Default::default(),
            keywords: Default::default(),
            local_search: Default::default(),
            export: Default::default(),
            stealth: Default::default(),
            drafts: Default::default(),
            scheduled: Default::default(),
            translate: Default::default(),
            warm: Default::default(),
            gallery: None,
            place: Default::default(),
            fx: Default::default(),
            crash: Default::default(),
            news: Default::default(),
            global_keys: Default::default(),
            deck: Default::default(),
            mica: Default::default(),
            tabs: Default::default(),
            palette: Default::default(),
            zoom: Default::default(),
            panes: Default::default(),
        };
        for n in [notice, prefs_notice].into_iter().flatten() {
            app.notify(n);
        }
        app.poll_theme();
        app.begin(env_token);
        app
    }

    /// Remplace l'heure murale (tests : délai de l'étape 2). À appeler
    /// avant la première image.
    pub fn set_clock(&mut self, clock: WallClock) {
        self.clock = clock;
    }

    /// Heure murale du démarrage du processus (retenue à la première image).
    fn process_start(&mut self) -> u64 {
        let now = self.now_secs();
        *self.start_secs.get_or_insert(now)
    }

    /// Heure murale, en secondes depuis 1970.
    fn now_secs(&self) -> u64 {
        (self.clock)().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    }

    /// Remplace le thème sans l'enregistrer (aperçu sur un préréglage).
    pub fn set_theme(&mut self, t: Tokens) {
        self.look.replace(t);
        self.sync_theme();
    }

    /// Ouvre le panneau Apparence.
    pub fn open_appearance(&mut self) {
        self.show_settings(settings::Page::Appearance);
    }

    /// Ouvre la fenêtre Paramètres sur une page nommée (aperçu, tests) ;
    /// `false` si le nom est inconnu.
    pub fn open_settings(&mut self, page: &str) -> bool {
        use settings::Page::*;
        let p = match page {
            "compte" => MyAccount,
            "profil" => Profile,
            "statut" => Status,
            "confidentialite" => Privacy,
            "applications" => Apps,
            "appareils" => Devices,
            "comptes-lies" => Connections,
            "facturation" => Billing,
            "apparence" => Appearance,
            "texte" => TextImages,
            "notifications" => Notifications,
            "langue" => Language,
            "dossiers" => Folders,
            "ferrocord" => Ferrocord,
            "accessibilite" => Accessibility,
            "streamer" => Streamer,
            "avances" => Advanced,
            "donnees" => DataPrivacy,
            "activite" => Activity,
            "a-propos" => About,
            "raccourcis" => Keybinds,
            "voix" => Voice,
            // « Voix et vidéo », amenée sur la section « Vidéo » (lot restes).
            "voix-video" => {
                self.open_video_settings();
                return true;
            }
            "plugins" => Plugins,
            // Fenêtre de réglages d'un plugin, sur un exemple des quatre formes (lot P0).
            "plugins-reglages" => {
                self.show_settings(Plugins);
                self.plugins_preview_settings();
                return true;
            }
            // Liste des pages (disposition mobile).
            "menu" => {
                self.params.show_menu();
                return true;
            }
            _ => return false,
        };
        self.show_settings(p);
        true
    }

    /// Ouvre le sélecteur de compte (aperçu).
    pub fn open_account_menu(&mut self) {
        self.menu_open = true;
    }

    /// Une image sans fenêtre : logique puis interface (tests de bout en bout).
    pub fn step(&mut self) {
        let ctx = self.ctx.clone();
        self.logic_pass();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| self.ui_pass(ui));
        out.textures_delta.clear();
    }

    /// Ouvre un salon comme un clic dans la liste (`guild` : `None` pour
    /// un MP) ; sert aux tests de bout en bout.
    pub fn open(&mut self, guild: Option<u64>, channel: u64) {
        match guild {
            Some(g) => self.store.guild = Some(g),
            None => self.store.select_dms(),
        }
        self.open_channel(channel);
        self.flush_commands();
    }

    /// Envoie un message dans un salon, comme la zone de saisie (tests de
    /// bout en bout) ; `false` si l'envoi est refusé d'avance.
    pub fn send(&mut self, channel: u64, text: &str) -> bool {
        let ok = self.store.send_message(channel, text.into());
        self.flush_commands();
        ok
    }

    /// « Réessayer » sur un message non envoyé (tests de bout en bout) ;
    /// `false` s'il n'existe pas (ou plus) dans cette session.
    pub fn retry_unsent(&mut self, channel: u64, nonce: u64) -> bool {
        let ok = self.store.retry_unsent(channel, nonce);
        self.flush_commands();
        ok
    }

    pub fn settings(&self) -> &Settings {
        &self.prefs.now
    }

    /// Message affiché sur l'écran de connexion.
    pub fn login_error(&self) -> Option<&str> {
        self.login_error.as_deref()
    }

    /// Notification affichée en ce moment.
    pub fn notice(&self) -> Option<&str> {
        self.toast.as_ref().map(|t| t.text.as_ref())
    }

    /// Avertissement durable : ancien fichier de jeton en clair encore présent.
    pub fn legacy_notice(&self) -> Option<&str> {
        self.legacy_file.as_deref()
    }

    /// « Essayer le jeton du fichier » est proposé.
    pub fn offers_file_token(&self) -> bool {
        self.legacy_newer && self.legacy_token.is_some()
    }

    /// Session avec le jeton du fichier de jeton en clair (clic sur sa ligne).
    pub fn try_file_token(&mut self) {
        self.open_legacy();
    }

    /// « Oui » à la question de retrait de `key` (posée par
    /// [`App::ask_remove`]) : `false` si elle n'est pas prête, ou si le
    /// retrait est refusé (étape 2 de la migration en cours), expliqué.
    pub fn remove_account(&mut self, key: AccountKey) -> bool {
        if self.confirm != Some(Confirm::Remove(key.clone())) {
            return false;
        }
        self.confirm_remove(key)
    }

    /// « Supprimer le fichier de jeton en clair », confirmé.
    pub fn delete_legacy_file(&mut self) {
        self.confirmed(Confirm::WipeLegacy);
    }

    /// « Non » : la question en attente est abandonnée.
    pub fn cancel_confirmation(&mut self) {
        self.confirm = None;
    }

    /// Premier clic sur « Supprimer le fichier de jeton en clair » : le
    /// coffre est lu pour la question de confirmation.
    pub fn ask_delete_legacy_file(&mut self) {
        self.ask_wipe();
    }

    /// Question de confirmation de la suppression du fichier (`None` tant
    /// que le coffre n'a pas été lu).
    pub fn delete_legacy_question(&self) -> Option<String> {
        self.wipe_question()
    }

    /// Question de confirmation du retrait d'un compte (`None` tant que
    /// coffre et fichier n'ont pas été relus).
    pub fn remove_account_question(&self, key: &AccountKey) -> Option<String> {
        self.remove_question(key)
    }

    /// `DISCORD_TOKEN` abîmé (ignoré).
    pub fn env_warning(&self) -> Option<&str> {
        self.env_warning.as_deref()
    }

    /// Le même compte enregistré deux fois : celui que le sélecteur propose de retirer.
    pub fn duplicate_account(&self) -> Option<AccountKey> {
        self.twin_to_remove()
    }

    /// « Importer » est proposé (import refusé auparavant).
    pub fn offers_import(&self) -> bool {
        self.legacy_declined && self.legacy_file.is_some()
    }

    /// « Importer » : le fichier de jeton en clair est importé de nouveau.
    pub fn import_legacy_file(&mut self) {
        self.import_legacy();
    }

    /// « Recopier le jeton d'avant » est proposé.
    pub fn offers_recopy(&self) -> bool {
        self.recovery.is_some()
    }

    /// « Recopier le jeton d'avant ».
    pub fn recopy_previous_token(&mut self) {
        self.recopy_previous();
    }

    /// L'étape 2 de la migration est programmée (après READY) ou en cours.
    pub fn step_two(&self) -> (bool, bool) {
        (self.finish_pending.is_some(), self.finishing)
    }

    /// Clic sur le × d'un compte : coffre et fichier sont relus, puis la
    /// ligne pose la question.
    pub fn ask_remove_account(&mut self, key: AccountKey) {
        self.ask_remove(key);
    }

    /// « Se déconnecter » : la question est posée (compte enregistré) ou la
    /// session est fermée tout de suite.
    pub fn ask_logout(&mut self) {
        self.request_logout();
    }

    /// Question de confirmation de la déconnexion (`None` tant que coffre et
    /// fichier n'ont pas été relus).
    pub fn logout_confirmation(&self) -> Option<String> {
        self.logout_question()
    }

    /// « Oui » à la question de déconnexion (ou fermeture directe si rien
    /// n'est effacé). Sans question prête, rien n'est fait.
    pub fn logout(&mut self) {
        if self.logout_needs_confirm() && self.confirm != Some(Confirm::Logout) {
            return;
        }
        self.confirmed(Confirm::Logout);
    }

    /// Question « remplacer le jeton enregistré » (jeton saisi d'un compte
    /// déjà enregistré avec un autre jeton) ; `None` : pas de question.
    pub fn replace_token_question(&self) -> Option<String> {
        self.replace_question()
    }

    /// Réponse à cette question (« Non » : le coffre garde son jeton).
    pub fn answer_replace_token(&mut self, yes: bool) {
        self.answer_replace(yes);
    }

    /// Une étape est en cours (ouverture des comptes, lecture d'un jeton, connexion).
    pub fn is_busy(&self) -> bool {
        self.phase != Phase::Login && !(self.phase == Phase::Session && self.store.me.is_some())
    }

    /// Bouton « Annuler » de l'étape en cours.
    pub fn cancel(&mut self) {
        self.cancel_busy();
    }

    /// Transmet au réseau les commandes accumulées par le magasin.
    fn flush_commands(&mut self) {
        let cmds = self.store.take_commands();
        self.journal_outgoing(&cmds);
        for c in &cmds {
            if let Cmd::Voice(v) = c {
                self.voice_link_command(v);
            }
        }
        if !cmds.is_empty() {
            self.backend.send(cmds);
        }
    }

    fn h24(&self) -> bool {
        self.theme.layout.clock == Clock::H24
    }

    fn word(&self, source: &ToastText) -> Cow<'static, str> {
        match source {
            ToastText::Notice(n) => words::notice(self.lang, n, self.h24()).into(),
            ToastText::Const(t) => t.get(self.lang).into(),
            ToastText::Text(s) => s.clone().into(),
        }
    }

    fn notify(&mut self, msg: impl Into<ToastText>) {
        let source = msg.into();
        let text = self.word(&source);
        self.local_log(&text);
        self.toast = Some(Toast { source, text, at: Instant::now() });
    }

    /// Langue changée (`i18n::set_language`) : tout ce qui a été rendu une
    /// fois est refait une fois (messages du magasin, notification, erreur
    /// de connexion, hauteurs de la liste), puis l'image est redessinée.
    fn sync_language(&mut self) {
        let lang = i18n::language();
        if lang == self.lang {
            return;
        }
        self.lang = lang;
        self.store.set_language(lang);
        if let Some(toast) = &self.toast {
            let text = self.word(&toast.source);
            if let Some(toast) = &mut self.toast {
                toast.text = text;
            }
        }
        if let Some(p) = &self.login_problem {
            self.login_error = Some(words::problem(lang, p, self.h24()));
        }
        self.ctx.request_repaint();
    }

    fn poll_events(&mut self) {
        for e in self.backend.poll() {
            // Export (lot E1) : une page d'historique demandée pour lui ne va pas au magasin.
            if self.export_before(&e) {
                continue;
            }
            let ready = matches!(e, Event::Ready { .. });
            let pending = if let Event::MessageCreate(m) = &e { Some(self.desk_before(m)) } else { None };
            let watched = if let Event::MessageCreate(m) = &e { self.keywords_before(m) } else { None };
            // Journal (lot E1) : une suppression gardée ne retire pas le message.
            self.local_search_before(&e);
            let (kept, page) = self.journal_before(&e);
            let eff = if kept { None } else { self.store.apply(e) };
            if let Some(c) = page {
                self.journal_after(c);
            }
            let alerted = pending.as_ref().is_some_and(desk::Pending::alerts);
            if let Some(p) = pending {
                self.desk_after(p);
            }
            if let Some(w) = watched {
                self.keywords_after(w, alerted);
            }
            if ready {
                self.on_ready();
                self.local_log(crate::i18n::table::s2::t::LOG_READY.get(self.lang));
            }
            self.handle(eff);
            self.flush_commands();
        }
        self.thread_failures();
        self.interactions_tick();
        self.lot13_notices();
        // Fin d'une exclusion temporaire : permissions recalculées une fois,
        // et réveil prévu pour ce moment-là (rien n'est recalculé entre-temps).
        let eff = self.store.expire_timeouts(self.store.now());
        self.handle(eff);
        if let Some(end) = self.store.next_timeout_end() {
            let wait = u64::try_from(end.saturating_sub(self.store.now())).unwrap_or(0);
            self.ctx.request_repaint_after(Duration::from_millis(wait));
        }
    }

    fn handle(&mut self, eff: Option<Effect>) {
        match eff {
            Some(Effect::Toast(n)) => self.notify(n),
            Some(Effect::Fatal { problem, bad_token }) => self.session_refused(problem, bad_token),
            Some(Effect::ScrollToBottom) => self.scroll_to_bottom = true,
            None => {}
        }
    }

    /// Nouvelles du fil de thème (fichier modifié, import, erreur).
    fn poll_theme(&mut self) {
        if let Some(err) = self.look.poll() {
            self.notify(err);
        }
        self.sync_theme();
    }

    /// Recalcule et applique les jetons si le thème édité a changé, et
    /// demande son enregistrement.
    fn sync_theme(&mut self) {
        // Zoom changé au clavier (Ctrl + / Ctrl -) : il devient un réglage du thème.
        self.look.adopt_zoom(&self.ctx);
        let scaled = settings::local::saturated(self.look.edited.scaled(), self.local.prefs.saturation);
        // Disposition mobile : cibles tactiles et textes lisibles (phase 7).
        let scaled = if self.mobile_active() { mobile::touch_tokens(scaled) } else { scaled };
        // Fond personnalisé (dégradé, image) : les aplats de code le laissent transparaître.
        let scaled = if self.look_paints_nothing() { scaled } else { look::soften(scaled) };
        if scaled != self.theme {
            if scaled.layout.clock != self.theme.layout.clock {
                // Les balises <t:…> déjà rendues le sont une fois de plus, pas à chaque image.
                self.store.set_clock12(scaled.layout.clock == Clock::H12);
            }
            self.theme = scaled;
            self.look.request_zoom(&self.ctx, self.theme.layout.zoom);
            self.theme.apply(&self.ctx);
            fonts::set_family(&self.ctx, self.theme.typography.family);
            settings::shell::remember(&self.ctx, &self.theme);
            // Le nouveau style ne vaut qu'à partir de l'image suivante.
            self.ctx.request_repaint();
        }
        self.look.persist();
    }

    /// Nouvelles du fil des réglages, puis enregistrement de ce qui a changé
    /// (fenêtre, panneau, dernier salon…).
    fn sync_prefs(&mut self) {
        let (notice, reloaded) = self.prefs.poll();
        if let Some(n) = notice {
            self.notify(n);
        }
        // Fichier modifié au dehors (autre instance, éditeur) : sa langue
        // s'applique ; sinon le choix en cours (panneau Apparence) y est écrit.
        if reloaded && self.prefs.now.language != i18n::choice() {
            i18n::set_language(self.prefs.now.language);
        }
        self.track_window();
        self.track_place();
        // Fenêtre ouverte sur Apparence : rouverte ainsi au lancement suivant.
        self.prefs.now.appearance_open = self.params.open && self.params.page() == settings::Page::Appearance;
        self.prefs.now.language = i18n::choice();
        self.prefs.persist();
    }

    fn open_channel(&mut self, id: u64) {
        self.store.select_channel(id);
        self.scroll_to_bottom = true;
    }

    fn toast_ui(&mut self, ui: &mut egui::Ui) {
        // Bande réservée sous la liste de messages à cette image, s'il y en a une.
        let slot = self.list.toast_slot.take();
        let chat = self.chat_rect.take();
        let Some(toast) = &self.toast else { return };
        let age = toast.at.elapsed();
        if age > TOAST {
            self.toast = None;
            // La bande de la liste disparaît à l'image suivante.
            ui.ctx().request_repaint();
            return;
        }
        let t = &self.theme;
        let area = egui::Area::new(Id::unique("toast"));
        let area = match slot {
            // Centrée dans sa bande : elle ne recouvre aucun message.
            Some(r) => area.pivot(egui::Align2::CENTER_CENTER).fixed_pos(r.center()),
            // Pas de bande : en bas de la zone du salon (à droite des panneaux).
            None => match chat {
                Some(r) => area.pivot(egui::Align2::CENTER_BOTTOM).fixed_pos(r.center_bottom() + t.toast_offset()),
                None => area.anchor(egui::Align2::CENTER_BOTTOM, t.toast_offset()),
            },
        };
        area.show(ui.ctx(), |ui| {
            if let Some(r) = slot.or(chat) {
                ui.set_max_width((r.width() - 2.0 * t.spacing.chat_pad - t.toast_margin().sum().x).max(1.0));
            }
            t.float_frame().corner_radius(t.r_toast()).inner_margin(t.toast_margin()).show(ui, |ui| {
                ui.label(egui::RichText::new(toast.text.as_ref()).color(t.colors.text));
            });
        });
        ui.ctx().request_repaint_after(TOAST.saturating_sub(age));
    }

    fn logic_pass(&mut self) {
        self.images.begin_frame();
        self.poll_accounts();
        self.qr_tick();
        self.sync_language();
        self.poll_events();
        self.offline_tick();
        self.journal_tick();
        self.keywords_tick();
        self.local_search_tick();
        self.export_tick();
        self.drafts_tick();
        self.scheduled_tick();
        self.translate_tick();
        self.voice_link_tick();
        self.relations_tick();
        self.local_tick();
        self.look_tick();
        self.audio_settings_tick();
        self.poll_theme();
        self.desk_tick();
        self.keys_tick();
        self.global_keys_tick();
        self.deck_tick();
        crate::voice::set_extra_codecs(self.local.prefs.audio.extra_codecs);
        // Quêtes « jouer » : battement tant que le jeu détecté tourne, et
        // réveil pour le suivant (aucun réveil sans quête en cours).
        if let Some(wait) = self.store.quests_tick(self.local.prefs.quest_progress) {
            self.ctx.request_repaint_after(std::time::Duration::from_millis(wait.clamp(1_000, crate::state::quests::BEAT_EVERY_MS) as u64));
        }
        // Journal de débogage : polices de nom que Ferrocord ne connaît pas.
        for id in fonts::take_unknown() {
            self.local_log(&format!("police de nom inconnue : font_id {id}"));
        }
        self.install_tick();
        self.crash_tick();
        self.news_tick();
        self.mica_tick();
    }

    fn ui_pass(&mut self, ui: &mut egui::Ui) {
        // Quand jouer les animations, d'après les réglages (lot A6).
        motion::set(ui.ctx(), motion::Motion::from_settings(&self.store.account, self.local.reduced_motion()).with_level(self.local.motion_level()));
        motion::set_typists(ui.ctx(), self.store.channel.unwrap_or(0), self.store.typing.all(self.store.now()));
        // Ce que les messages montrent, d'après « Texte et images » (lot restes).
        attachments::set_shown(ui.ctx(), attachments::Shown::from_settings(&self.store.account));
        // Raccourcis avec Ctrl ou Alt (Ctrl+, Ctrl+K…), avant les champs de texte (`keys.rs`).
        self.shortcuts_early(ui);
        // Fichier `.ferrotheme` déposé (lot E5) : pris avant les pièces jointes.
        self.theme_share_drop(ui.ctx());
        // Barre de titre intégrée : panneau du haut, avant tout le reste (lot V1).
        self.titlebar_ui(ui);
        // Image agrandie : par-dessus tout, et elle prend Échap avant le reste.
        viewer::show(ui, &self.theme, self.lang, &self.images);
        player::show_full(ui, &self.theme, self.lang);

        // Fenêtre Paramètres : une fenêtre flottante, dessinée au-dessus des
        // panneaux quel que soit l'ordre des appels ; ses réglages valent dès cette image.
        // Disposition mobile ou bureau, avant les fenêtres qui en dépendent (phase 7).
        self.mobile_decide(ui.ctx());
        self.settings_ui(ui);
        // Planche des fondations visuelles (aperçu hors ligne), à la place de l'interface.
        if self.gallery_ui(ui) {
            self.sync_theme();
            return;
        }
        // Rapport d'un plantage précédent (lot E5) : bandeau du haut.
        self.crash_banner(ui);
        // Notes de la version tout juste installée.
        self.news_banner(ui);
        // Tant que READY n'est pas arrivé, l'écran de connexion dit où on en est.
        if self.backend.is_connected() && self.store.me.is_some() && self.mobile_active() {
            self.mobile_ui(ui);
        } else if self.backend.is_connected() && self.store.me.is_some() {
            self.main_ui(ui);
        } else {
            self.login_ui(ui);
        }

        // Les réglages du panneau s'appliquent dès cette image (langue comprise).
        self.sync_theme();
        self.sync_prefs();
        self.sync_language();
        self.export_ui(ui.ctx());
        // Aperçu d'un thème à importer (lot E5).
        self.theme_share_ui(ui.ctx());
        self.scheduled_ui(ui.ctx());
        self.toast_ui(ui);
        // Liens demandés à egui pendant l'image : ouverts par nous (lot C6).
        self.route_links(ui.ctx());
        self.flush_commands();
        // Rejoindre, quitter, muet… décidés à cette image : l'appel suit tout de suite.
        self.voice_link_tick();
        // Sons du tableau, messages vocaux (lecture, enregistrement) : phase 6.
        self.voice_notes_tick(ui.ctx());
        // Plugins vocaux (lot E4) : enregistrement local, incrustation.
        self.voice_plugins_tick(ui.ctx());
        // Lecteur vidéo et audio du fil (lot M).
        player::tick(ui.ctx(), self.media_link());
        self.flush_commands();
        // Ce qui a été lu à cette image devient « affiché » (réveils des présences).
        self.store.people.lock().end_frame();
        // Contour de l'élément qui a le clavier, avec les jetons du thème (lot C5).
        a11y::focus_ring(ui.ctx(), &self.theme);
        // Emojis et autocollants demandés pendant l'image (lot T4).
        emoji::flush(ui.ctx(), &self.images, &self.theme);
        self.images.end_frame();
        // Effets de profil : la scène d'un profil fermé est libérée (lot ND).
        self.profile_fx.end_frame();
        // Avant le premier salon : glyphes courants préparés, une police par image (lot C9).
        self.warm.step(ui.ctx(), &self.theme, self.store.channel.is_some());
        // Ce que la passerelle doit savoir de l'écran pour les prochains événements.
        self.publish_interest();
        self.panes_publish();
    }
}

impl eframe::App for App {
    /// Disposition mobile hors Android : la souris touche comme un doigt (phase 7).
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        // Toutes les fenêtres partagent l'atlas des glyphes : même borne partout (lot « ram »).
        fonts::limit_atlas(raw);
        // Fenêtre secondaire (incrustation « qui parle », lot E4) : ni doigt
        // simulé ni barre de titre.
        if raw.viewport_id != egui::ViewportId::ROOT {
            return;
        }
        self.mobile_input(raw);
        titlebar::reserve(&self.theme, raw);
    }

    /// La fenêtre principale peint tout son fond ; l'incrustation « qui
    /// parle » (lot E4) et la fenêtre sous Mica (lot E5), elles, doivent
    /// partir d'un fond transparent.
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        voice::overlay_clear_color().or_else(|| self.mica_clear_color()).unwrap_or_else(|| visuals.panel_fill.to_normalized_gamma_f32())
    }

    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.logic_pass();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        fonts::note_atlas(ui.ctx());
        self.ui_pass(ui);
    }

    /// Historique hors ligne : dernier instantané écrit avant la fermeture (lot H).
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.offline_exit();
        self.local_search_exit();
        self.e3_exit();
        self.journal_exit();
        self.tabs_exit();
        self.zoom_exit();
        self.panes_exit();
    }
}

impl App {
    fn main_ui(&mut self, ui: &mut egui::Ui) {
        profile::install(ui.ctx(), &self.store.people);
        cosmetics::install(ui.ctx(), &self.store);
        // Lien de message cliqué, saut abandonné (lot T12).
        self.search_tick(ui.ctx());
        // Onglets (lot E2) : l'onglet actif suit l'endroit affiché.
        self.tabs_sync();
        self.zoom_tick();
        self.panes_begin();
        // Mode streamer actif : bandeau en haut, au-dessus de tout (lot C5).
        self.streamer_banner(ui);
        // Message programmé arrivé à échéance : il attend un clic (lot E3).
        self.scheduled_banner(ui);
        let t = &self.theme;
        // Rail et colonne des salons : en voile quand Mica est derrière (lot E5).
        let (rail_frame, channels_frame, chat_frame) = (
            egui::Frame::new().fill(self.mica_fill(t.colors.bg0)).inner_margin(t.rail_margin()),
            egui::Frame::new().fill(self.mica_fill(t.colors.bg1)),
            egui::Frame::new().fill(t.colors.bg2),
        );
        // Filets des panneaux : dessinés plus bas avec le jeton `panel_border` (lot V1).
        // Barre des serveurs masquée par l'utilisateur (lot PERSO) : une colonne de largeur nulle.
        let rail_rect = if self.local.prefs.look.hide_rail {
            let r = ui.available_rect_before_wrap();
            r.with_max_x(r.left())
        } else {
            egui::Panel::left("rail")
            .exact_size(t.rail_panel_width())
            .resizable(false)
            .show_separator_line(false)
            .frame(rail_frame)
            .show(ui, |ui| {
                look::backdrop(ui, look::Zone::Side);
                self.guild_rail(ui)
            })
            .response
            .rect
        };

        // Largeur changée dans le panneau ou le fichier : imposée une image,
        // puis le panneau redevient redimensionnable à la souris.
        let t = &self.theme;
        let width = t.layout.channels_width;
        // MP à deux : le panneau de profil tient la place de la liste des membres (lot R).
        let members = self.dm_profile_width().or((t.members_shown() && self.has_member_list()).then_some(t.layout.members_width));
        let fit = layout::fit(ui.available_width() + rail_rect.width(), rail_rect.width(), width, members);
        self.place.fit = fit;
        let mut panel = egui::Panel::left("channels").show_separator_line(false).frame(channels_frame);
        panel = if let Some(cap) = fit.channels_cap {
            // Fenêtre étroite : largeur imposée sans toucher au réglage, qui
            // revient dès que la place revient.
            self.look.channels_shown = cap;
            panel.exact_size(cap).resizable(false)
        } else if self.look.channels_shown != width {
            self.look.channels_shown = width;
            panel.exact_size(width)
        } else {
            panel.default_size(width).size_range(CHANNELS_WIDTH.min..=CHANNELS_WIDTH.max)
        };
        // Poignée de redimensionnement du panneau (identifiant calculé comme egui le fait).
        let handle = ui.scope_id().with("channels").with("__resize");
        let channels_rect = panel
            .show(ui, |ui| {
                look::backdrop(ui, look::Zone::Side);
                self.channel_list(ui)
            })
            .response
            .rect;
        let shown = channels_rect.width();
        if fit.channels_cap.is_none() && ui.ctx().is_being_dragged(handle) {
            self.look.adopt_channels_width(shown);
        }

        self.members_ui(ui);
        if self.group_panel_shown() {
            let t = &self.theme;
            let frame = egui::Frame::new().fill(t.colors.bg1).inner_margin(t.side_panel_margin());
            egui::Panel::right("group_panel").frame(frame).default_size(t.layout.channels_width).show(ui, |ui| self.group_panel(ui));
        }
        // Accueil des messages privés sans conversation choisie : la page Amis.
        let home = self.store.guild.is_none() && self.store.channel.is_none();
        let chat = egui::CentralPanel::default().frame(chat_frame).show(ui, |ui| {
            look::backdrop(ui, look::Zone::Chat);
            // Barre d'onglets (lot E2) : rien avec un seul onglet ou le plugin coupé.
            self.tabs_bar(ui);
            if home && self.quests_shown() { self.quests_page(ui) } else if home && self.requests_shown() { self.requests_page(ui) } else if home { self.friends_page(ui) } else { self.chat_area(ui) }
        });
        self.chat_rect = Some(chat.response.rect);
        // Le filet du rail s'arrête à la carte du panneau utilisateur, qui passe dessous.
        let rail_line = self.place.card.map_or(rail_rect, |c| rail_rect.with_max_y(c.top()));
        self.theme.paint_panel_borders(ui.painter(), rail_line, channels_rect, ui.ctx().content_rect());
        // Par-dessus le reste, après les zones qui ont pu l'ouvrir à cette image.
        self.profile_ui(ui);
        self.badge_window(ui.ctx());
        self.mentions_ui(ui.ctx());
        // Édition rapide du profil, par-dessus la carte qui l'a ouverte.
        self.profile_editor_ui(ui);
        self.servers_ui(ui);
        self.guild_settings_ui(ui);
        // « Modifier le salon » et ses fenêtres, au-dessus de « Paramètres du serveur ».
        self.channel_admin_ui(ui);
        self.moderation_ui(ui);
        // Événements, accueil, paramètres du serveur (lot A7).
        self.admin_events_ui(ui);
        // Sélecteur rapide, boîte de réception, raccourcis (lot navigation).
        self.nav_ui(ui);
        // Appel entrant et tableau de sons (lot V6).
        self.calls_overlay(ui);
        // Fenêtres détachées, plein écran, textures vidéo (phase 5).
        self.video_frame_end(ui);
        // Sélecteur de partage (lot M3), au-dessus de tout.
        self.share_picker_ui(ui);
        // Discussions dans leur propre fenêtre (plugin, lot E2).
        self.detached_ui();
    }
}

#[cfg(test)]
mod tests {
    use super::appearance::Appearance;
    use crate::theme::{parse, export, Preset, ThemeLink, WatchConfig, CHANNELS_WIDTH, ZOOM};
    use eframe::egui;
    use std::{
        fs,
        path::PathBuf,
        time::{Duration, Instant},
    };

    /// Textes affichés par l'interface et le magasin, pour le test des glyphes.
    const SOURCES: &[(&str, &str)] = &[
        ("ui/appearance.rs", include_str!("appearance.rs")),
        ("ui/channels.rs", include_str!("channels.rs")),
        ("ui/chat.rs", include_str!("chat.rs")),
        ("ui/composer.rs", include_str!("composer.rs")),
        ("ui/desk.rs", include_str!("desk.rs")),
        ("i18n/notify.rs", include_str!("../i18n/notify.rs")),
        ("ui/friends.rs", include_str!("friends.rs")),
        ("ui/groups.rs", include_str!("groups.rs")),
        ("ui/user_menu.rs", include_str!("user_menu.rs")),
        ("ui/requests.rs", include_str!("requests.rs")),
        ("ui/quests.rs", include_str!("quests.rs")),
        ("ui/effects_picker.rs", include_str!("effects_picker.rs")),
        ("ui/profile_edit_left.rs", include_str!("profile_edit_left.rs")),
        ("ui/profile_edit_card.rs", include_str!("profile_edit_card.rs")),
        ("ui/profile_edit_board.rs", include_str!("profile_edit_board.rs")),
        ("ui/shop.rs", include_str!("shop.rs")),
        ("ui/suggestions.rs", include_str!("suggestions.rs")),
        ("ui/saved.rs", include_str!("saved.rs")),
        ("ui/app_directory.rs", include_str!("app_directory.rs")),
        ("ui/verification.rs", include_str!("verification.rs")),
        ("ui/summaries.rs", include_str!("summaries.rs")),
        ("ui/discovery.rs", include_str!("discovery.rs")),
        ("i18n/nd.rs", include_str!("../i18n/nd.rs")),
        ("i18n/people.rs", include_str!("../i18n/people.rs")),
        ("ui/reads.rs", include_str!("reads.rs")),
        ("ui/login.rs", include_str!("login.rs")),
        ("ui/members.rs", include_str!("members.rs")),
        ("state/members.rs", include_str!("../state/members.rs")),
        ("ui/message.rs", include_str!("message.rs")),
        ("ui/msg_actions.rs", include_str!("msg_actions.rs")),
        ("ui/pins.rs", include_str!("pins.rs")),
        ("ui/search.rs", include_str!("search.rs")),
        ("i18n/search.rs", include_str!("../i18n/search.rs")),
        ("state/social.rs", include_str!("../state/social.rs")),
        ("ui/profile.rs", include_str!("profile.rs")),
        ("ui/profile_full.rs", include_str!("profile_full.rs")),
        ("i18n/profile.rs", include_str!("../i18n/profile.rs")),
        ("state/profiles.rs", include_str!("../state/profiles.rs")),
        ("state/people.rs", include_str!("../state/people.rs")),
        ("ui/mod.rs", include_str!("mod.rs")),
        ("ui/rail.rs", include_str!("rail.rs")),
        ("ui/guild_settings/roles.rs", include_str!("guild_settings/roles.rs")),
        ("ui/guild_settings/members.rs", include_str!("guild_settings/members.rs")),
        ("ui/guild_settings/member_roles.rs", include_str!("guild_settings/member_roles.rs")),
        ("i18n/guilds.rs", include_str!("../i18n/guilds.rs")),
        ("i18n/admin.rs", include_str!("../i18n/admin.rs")),
        ("ui/keys.rs", include_str!("keys.rs")),
        ("ui/inbox.rs", include_str!("inbox.rs")),
        ("ui/inbox_cards.rs", include_str!("inbox_cards.rs")),
        ("ui/quick_switcher.rs", include_str!("quick_switcher.rs")),
        ("ui/util.rs", include_str!("util.rs")),
        ("ui/accounts.rs", include_str!("accounts.rs")),
        ("ui/prefs.rs", include_str!("prefs.rs")),
        ("account/link.rs", include_str!("../account/link.rs")),
        ("account/migrate.rs", include_str!("../account/migrate.rs")),
        ("account/vault.rs", include_str!("../account/vault.rs")),
        ("settings.rs", include_str!("../settings.rs")),
        ("theme/watch.rs", include_str!("../theme/watch.rs")),
        ("ui/words.rs", include_str!("words.rs")),
        ("ui/settings/mod.rs", include_str!("settings/mod.rs")),
        ("ui/settings/account.rs", include_str!("settings/account.rs")),
        ("ui/settings/synced.rs", include_str!("settings/synced.rs")),
        ("ui/settings/safety.rs", include_str!("settings/safety.rs")),
        ("ui/settings/menu.rs", include_str!("settings/menu.rs")),
        ("i18n/settings.rs", include_str!("../i18n/settings.rs")),
        ("i18n/mod.rs", include_str!("../i18n/mod.rs")),
        ("i18n/table.rs", include_str!("../i18n/table.rs")),
        ("i18n/time.rs", include_str!("../i18n/time.rs")),
        ("i18n/tokens.rs", include_str!("../i18n/tokens.rs")),
        ("i18n/verbatim.rs", include_str!("../i18n/verbatim.rs")),
        ("state/notice.rs", include_str!("../state/notice.rs")),
        ("theme/file.rs", include_str!("../theme/file.rs")),
        ("state/event.rs", include_str!("../state/event.rs")),
        ("state/model.rs", include_str!("../state/model.rs")),
        ("state/store.rs", include_str!("../state/store.rs")),
        ("theme/mod.rs", include_str!("../theme/mod.rs")),
        ("theme/presets.rs", include_str!("../theme/presets.rs")),
        ("net/gateway.rs", include_str!("../net/gateway.rs")),
        // Lot V6.
        ("i18n/calls.rs", include_str!("../i18n/calls.rs")),
        ("ui/call.rs", include_str!("call.rs")),
        ("ui/stage.rs", include_str!("stage.rs")),
    ];

    /// Un passage d'egui sans fenêtre (les textures produites sont jetées).
    fn pass(ctx: &egui::Context, f: impl FnMut(&mut egui::Ui)) {
        let mut out = ctx.run_ui(egui::RawInput::default(), f);
        out.textures_delta.clear();
    }

    /// Caractères non ASCII des chaînes littérales (hors commentaires).
    fn literal_chars(src: &str) -> Vec<char> {
        let mut out = Vec::new();
        for line in src.lines().filter(|l| !l.trim_start().starts_with("//")) {
            let (mut inside, mut escaped) = (false, false);
            for c in line.chars() {
                match c {
                    '\\' if inside && !escaped => escaped = true,
                    '"' if !escaped => inside = !inside,
                    _ => {
                        if inside && !c.is_ascii() {
                            out.push(c);
                        }
                        escaped = false;
                    }
                }
            }
        }
        out
    }

    /// Aucun carré vide : chaque caractère affiché existe dans les polices
    /// d'egui (qui n'ont pas de pictogrammes : ceux-ci sont dessinés).
    #[test]
    fn every_displayed_character_has_a_glyph() {
        let ctx = egui::Context::default();
        pass(&ctx, |_| {});
        let font = egui::FontId::default();
        let mut missing = Vec::new();
        for (file, src) in SOURCES {
            for c in literal_chars(src) {
                if !ctx.fonts_mut(|f| f.has_glyph(&font, c)) {
                    missing.push(format!("{file} : {c:?} (U+{:04X})", u32::from(c)));
                }
            }
        }
        // Tous les textes traduits, dans les deux langues, échappements
        // (`\u{a0}`…) compris : espaces insécables, guillemets « » et “ ”.
        let mut seen = std::collections::HashSet::new();
        for (key, lang, text) in crate::i18n::tests::every_string() {
            for c in text.chars().filter(|c| !c.is_ascii()) {
                if seen.insert((c, lang)) && !ctx.fonts_mut(|f| f.has_glyph(&font, c)) {
                    missing.push(format!("{key} ({lang:?}) : {c:?} (U+{:04X})", u32::from(c)));
                }
            }
        }
        for c in ['\u{a0}', '«', '»', '“', '”', '…'] {
            assert!(seen.iter().any(|&(s, _)| s == c), "{c:?} absent des textes : le test ne le vérifie pas");
        }
        assert!(missing.is_empty(), "glyphes absents : {missing:#?}");
        // Le test voit bien les caractères à vérifier, et un pictogramme manque bien.
        let warning = '\u{26A0}';
        assert_eq!(literal_chars(&format!("let s = \"é{warning}\"; // {warning}")), vec!['é', warning]);
        assert!(!ctx.fonts_mut(|f| f.has_glyph(&font, warning)));
    }

    /// L'identifiant de la poignée du panneau des salons est bien celui d'egui.
    #[test]
    fn channels_resize_handle_id_matches_egui() {
        let ctx = egui::Context::default();
        let mut handle = None;
        let mut seen = false;
        for _ in 0..2 {
            pass(&ctx, |ui| {
                let id = ui.scope_id().with("channels").with("__resize");
                seen = ui.ctx().read_response(id).is_some();
                handle = Some(id);
                egui::Panel::left("channels").resizable(true).show(ui, |_| {});
            });
        }
        assert!(seen, "poignée {handle:?} introuvable");
    }

    /// De même pour la liste des membres, à droite (lot V2).
    #[test]
    fn members_resize_handle_id_matches_egui() {
        let ctx = egui::Context::default();
        let mut seen = false;
        for _ in 0..2 {
            pass(&ctx, |ui| {
                let id = ui.scope_id().with("members").with("__resize");
                seen = ui.ctx().read_response(id).is_some();
                egui::Panel::right("members").resizable(true).show(ui, |_| {});
            });
        }
        assert!(seen);
    }

    /// Source d'images qui note chaque demande (appel de `url`, fait sur le
    /// fil d'interface au moment où l'image est demandée : sans course avec
    /// l'ordonnanceur) et chaque téléchargement, et sert un PNG.
    struct Recorder(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

    impl crate::images::Source for Recorder {
        fn url(&self, image: &crate::state::model::ImageRef, size: u32) -> String {
            let url = format!("note://{image:?}/{size}");
            if let Ok(mut v) = self.0.lock() {
                v.push(format!("demande {url}"));
            }
            url
        }
        fn fetch(&self, url: String) -> crate::images::FetchFuture {
            if let Ok(mut v) = self.0.lock() {
                v.push(format!("téléchargement {url}"));
            }
            let img = image::RgbaImage::from_pixel(64, 64, image::Rgba([90, 120, 200, 255]));
            let mut out = std::io::Cursor::new(Vec::new());
            let png = image::DynamicImage::ImageRgba8(img).write_to(&mut out, image::ImageFormat::Png).map(|()| out.into_inner());
            Box::pin(async move { png.map_err(|e| crate::net::cdn::FetchError::Refused(e.to_string())) })
        }
    }

    /// Connexion factice : toujours connectée, rien n'est envoyé ni reçu.
    pub(super) struct Offline;

    impl super::Backend for Offline {
        fn set_wake(&mut self, _: crate::net::Wake) {}
        fn connect(&mut self, _: crate::account::Token) {}
        fn disconnect(&mut self) {}
        fn is_connected(&self) -> bool {
            true
        }
        fn poll(&mut self) -> Vec<crate::state::Event> {
            Vec::new()
        }
        fn send(&mut self, _: Vec<crate::state::Cmd>) {}
    }

    /// Démarrage sans fichier ni coffre réel : thème et réglages en mémoire,
    /// coffre en mémoire vide, aucun ancien fichier, aucun `DISCORD_TOKEN`.
    pub(super) fn offline_boot(backend: Box<dyn super::Backend>, store: crate::state::Store, images: crate::images::Images) -> super::Boot {
        super::Boot {
            backend,
            store,
            theme: ThemeLink::open(None, false, WatchConfig::default()),
            settings: crate::settings::SettingsLink::open(None, false, WatchConfig::default()),
            accounts: crate::account::AccountLink::start(std::sync::Arc::new(crate::account::MemoryVault::new())),
            env_token: None,
            legacy: None,
            images,
        }
    }

    /// Application dessinable sans fenêtre : moi, un serveur avec icône, un
    /// MP avec avatar, liste des MP affichée ; images servies par `Recorder`.
    struct Harness {
        _rt: tokio::runtime::Runtime,
        app: super::App,
        urls: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    fn harness(theme: crate::theme::Tokens) -> Harness {
        use crate::images::{Config, Images};
        use crate::state::model::{Avatar, Dm, Guild, ImageHash, ImageRef, User};
        use std::sync::{Arc, Mutex};
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap_or_else(|e| panic!("{e}"));
        let urls = Arc::new(Mutex::new(Vec::new()));
        let ctx = egui::Context::default();
        let images = Images::start(ctx.clone(), rt.handle(), Arc::new(Recorder(urls.clone())), None, Config::default(), Arc::new(|| {}));
        let mut store = crate::state::Store::new();
        let me = User { avatar: Avatar::of_user(1, Some(ImageHash::synthetic(1)), None), ..User::new(1, "Moi".into()) };
        let mut guild = Guild::new(9, "Alpha".into(), Vec::new());
        guild.icon = Some(ImageRef::GuildIcon { guild_id: 9, hash: ImageHash::synthetic(9) });
        let dm = Dm { avatar: Avatar::of_user(7, Some(ImageHash::synthetic(7)), None), ..Dm::new(50, "Alice".into(), 50) };
        store.apply(crate::state::Event::Ready { me, guilds: vec![guild], dms: vec![dm] });
        store.select_dms();
        let mut app = super::App::with_context(ctx, offline_boot(Box::new(Offline), store, images));
        app.theme = theme;
        Harness { _rt: rt, app, urls }
    }

    impl Harness {
        /// Une image complète de l'interface principale.
        fn frame(&mut self) {
            let ctx = self.app.ctx.clone();
            self.app.images.begin_frame();
            pass(&ctx, |ui| self.app.main_ui(ui));
            self.app.images.end_frame();
        }

        /// Images dessinées jusqu'à ce que plus rien ne soit en attente.
        fn settle(&mut self) {
            let deadline = std::time::Instant::now() + Duration::from_secs(60);
            loop {
                self.frame();
                let s = self.app.images.stats();
                if s.pending == 0 && s.textures + s.failed > 0 {
                    return;
                }
                assert!(std::time::Instant::now() < deadline, "{s:?}");
                std::thread::sleep(Duration::from_millis(2));
            }
        }

        /// Images de ce genre demandées par l'interface.
        fn requested(&self, kind: &str) -> usize {
            self.count("demande ", kind)
        }

        /// Images de ce genre téléchargées.
        fn downloaded(&self, kind: &str) -> usize {
            self.count("téléchargement ", kind)
        }

        fn count(&self, what: &str, kind: &str) -> usize {
            self.urls.lock().map(|v| v.iter().filter(|u| u.starts_with(what) && u.contains(kind)).count()).unwrap_or(0)
        }
    }

    /// Avatars masqués : ni les messages, ni la liste des MP, ni le pied
    /// n'en demandent (aucun téléchargement), mais les icônes de serveur du
    /// rail restent. Messages sur une ligne : pas d'avatar de message.
    /// Affichés : chacun des trois endroits demande le sien.
    #[test]
    fn hidden_avatars_are_never_downloaded() {
        use crate::state::model::{Avatar, ImageHash, Message};
        use crate::theme::{Avatars, MessageMode, Tokens};

        let m = Message {
            id: 5 << 22,
            channel_id: 1,
            guild_id: None,
            author_id: 3,
            author: "Bob".into(),
            author_avatar: Avatar::of_user(3, Some(ImageHash::synthetic(3)), None),
            content: "bonjour".into(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            mentions: Vec::new(),
            mention_everyone: false,
            edited: false,
            system: None,
            meta: Default::default(),
        };
        let avatar_of = |id: u64| format!("Avatar {{ user_id: {id},");
        let mut hidden = Tokens::default();
        hidden.layout.avatars = Avatars::Hidden;
        let mut h = harness(hidden.clone());
        for _ in 0..3 {
            h.app.images.begin_frame();
            pass(&h.app.ctx.clone(), |ui| super::message::message_ui(ui, &hidden, crate::i18n::Lang::Fr, &h.app.images, &m, &crate::state::markdown::Doc::default(), false, 0));
            h.app.images.end_frame();
        }
        h.settle();
        assert_eq!(h.requested("Avatar {"), 0, "{:?}", h.urls.lock().map(|v| v.clone()));
        assert_eq!(h.requested("GuildIcon {"), 1, "témoin : l'icône du serveur est demandée");
        assert_eq!(h.downloaded("Avatar {"), 0, "rien de téléchargé");

        let mut compact = Tokens::default();
        compact.layout.messages = MessageMode::Compact;
        let mut h = harness(compact.clone());
        h.app.images.begin_frame();
        pass(&h.app.ctx.clone(), |ui| super::message::message_ui(ui, &compact, crate::i18n::Lang::Fr, &h.app.images, &m, &crate::state::markdown::Doc::default(), false, 0));
        h.app.images.end_frame();
        h.settle();
        assert_eq!(h.requested(&avatar_of(3)), 0, "une ligne : pas d'avatar de message");

        // Témoin : affichés, ils sont demandés aux trois endroits.
        let shown = Tokens::default();
        let mut h = harness(shown.clone());
        h.app.images.begin_frame();
        pass(&h.app.ctx.clone(), |ui| super::message::message_ui(ui, &shown, crate::i18n::Lang::Fr, &h.app.images, &m, &crate::state::markdown::Doc::default(), false, 0));
        h.app.images.end_frame();
        h.settle();
        assert_eq!(h.requested(&avatar_of(3)), 1, "avatar du message");
        assert_eq!(h.requested(&avatar_of(7)), 1, "avatar de la liste des MP");
        assert_eq!(h.requested(&avatar_of(1)), 1, "mon avatar, dans le pied");
        // Toujours affichés, ces deux-là sont aussi téléchargés (le dessin attend qu'ils soient prêts).
        assert_eq!((h.downloaded(&avatar_of(7)), h.downloaded(&avatar_of(1))), (1, 1));
    }

    /// Bouton du rail élargi par son texte : l'icône est demandée et
    /// cherchée à la même taille (une seule texture, rien en attente).
    #[test]
    fn rail_icon_is_requested_at_one_size() {
        let mut t = crate::theme::Tokens::default();
        t.typography.rail = t.layout.rail_button * 2.0;
        let mut h = harness(t);
        // Trois initiales larges : le bouton dépasse le côté prévu.
        h.app.store.guilds[0].name = "Wwww Mmmm Wwww".into();
        h.settle();
        for _ in 0..5 {
            h.frame();
        }
        let s = h.app.images.stats();
        // L'icône du serveur, plus mon avatar et celui du MP (affichés par défaut).
        assert_eq!((s.textures, s.pending, s.failed), (3, 0, 0), "{s:?}");
        assert_eq!(h.requested("GuildIcon {"), 1);
        // Celle que le dessin a chargée est bien celle que le rail cherche (fond et anneau justes).
        let icon = h.app.store.guilds[0].icon.unwrap_or_else(|| panic!("icône"));
        assert!(h.app.images.peek(icon, h.app.theme.layout.rail_button));
    }

    /// Application de l'écran de connexion : comptes `accounts` dans les
    /// réglages et dans un coffre en mémoire, aucune connexion possible.
    fn login_app(accounts: &[(u64, &str)], env: Option<Result<crate::account::Token, crate::account::TokenError>>) -> (super::App, std::sync::Arc<crate::account::MemoryVault>) {
        use crate::account::{AccountKey, AccountLink, MemoryVault, Token};
        use crate::settings::{Account, Settings, SettingsLink, SettingsUpdate};
        let vault = std::sync::Arc::new(MemoryVault::with(accounts.iter().map(|(id, _)| (AccountKey::of_user(*id), Token::from(format!("faux-{id}"))))));
        let mut s = Settings::default();
        for (id, name) in accounts {
            s.accounts.push(Account { user_id: Some(*id), name: Some((*name).into()), ..Account::new(AccountKey::of_user(*id)) });
        }
        let ctx = egui::Context::default();
        let boot = super::Boot {
            backend: Box::new(Offline),
            store: crate::state::Store::new(),
            theme: ThemeLink::open(None, false, WatchConfig::default()),
            settings: (SettingsLink::open(None, false, WatchConfig::default()).0, Some(SettingsUpdate::Loaded(s))),
            accounts: AccountLink::start(vault.clone()),
            env_token: env,
            legacy: None,
            images: crate::images::Images::disabled(ctx.clone()),
        };
        let mut app = super::App::with_context(ctx, boot);
        let end = Instant::now() + Duration::from_secs(5);
        while app.phase == super::Phase::Starting {
            assert!(Instant::now() < end, "démarrage");
            let _ = frame(&mut app, Vec::new());
            std::thread::sleep(Duration::from_millis(2));
        }
        (app, vault)
    }

    /// Une image complète (logique et interface) avec ces événements, sur
    /// un écran de 1100 × 720 points, arbre d'accessibilité compris.
    pub(super) fn frame(app: &mut super::App, events: Vec<egui::Event>) -> egui::FullOutput {
        let ctx = app.ctx.clone();
        ctx.enable_accesskit();
        app.logic_pass();
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 720.0))), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| app.ui_pass(ui));
        out.textures_delta.clear();
        out
    }

    /// Rectangle de l'élément dont le nom accessible contient `label`.
    pub(super) fn find(out: &egui::FullOutput, label: &str) -> Option<egui::Rect> {
        let update = out.platform_output.accesskit_update.as_ref()?;
        let (_, node) = update.nodes.iter().find(|(_, n)| n.label().is_some_and(|l| l.contains(label)))?;
        let b = node.bounds()?;
        Some(egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32)))
    }

    /// Clic sur l'élément nommé `label` (introuvable : échec du test).
    pub(super) fn click(app: &mut super::App, label: &str) {
        let mut out = frame(app, Vec::new());
        // Mise en page stable (la carte est centrée d'après l'image d'avant).
        for _ in 0..3 {
            out = frame(app, Vec::new());
        }
        let at = find(&out, label).unwrap_or_else(|| panic!("« {label} » introuvable")).center();
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(app, vec![egui::Event::PointerMoved(at), button(true)]);
        frame(app, vec![button(false)]);
        frame(app, Vec::new());
    }

    /// Retirer un compte demande un deuxième clic (« Oui ») ; « Non » annule.
    #[test]
    fn removing_an_account_needs_confirmation() {
        use crate::account::AccountKey;
        let (mut app, vault) = login_app(&[(7, "Alice Fictive")], None);
        let key = AccountKey::of_user(7);
        click(&mut app, "Retirer ce compte");
        assert_eq!(app.confirm, Some(super::Confirm::Remove(key.clone())), "premier clic : question seulement");
        assert!(app.forgetting.is_empty() && vault.peek(&key).is_some(), "rien d'effacé");
        click(&mut app, "Non");
        assert_eq!(app.confirm, None);
        click(&mut app, "Retirer ce compte");
        click(&mut app, "Oui");
        let end = Instant::now() + Duration::from_secs(5);
        while !app.prefs.now.accounts.is_empty() {
            assert!(Instant::now() < end, "compte retiré");
            let _ = frame(&mut app, Vec::new());
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(vault.peek(&key), None, "jeton effacé du coffre après confirmation");
    }

    /// Le champ du jeton est vidé après Connexion, état egui compris
    /// (l'historique d'annulation en garde des copies). Un jeton abîmé est
    /// refusé avec la raison, sans ouvrir de session.
    #[test]
    fn token_field_is_wiped_after_submit() {
        let (mut app, _) = login_app(&[], None);
        let id = egui::Id::unique(super::accounts::TOKEN_FIELD);
        app.token_input = "MTk4.Gx 7Qz.TEST".into();
        app.submit_token();
        assert!(app.login_error.as_deref().is_some_and(|e| e.contains("espaces")), "{:?}", app.login_error);
        assert!(app.current.is_none(), "pas de session avec un jeton abîmé");
        // Saisie normale : un état de champ existe (curseur, annulation).
        app.token_input = "\u{feff}MTk4.Gx7Qz.TEST\n".into();
        egui::text_edit::TextEditState::default().store(&app.ctx, id);
        app.submit_token();
        assert!(app.token_input.is_empty());
        assert!(egui::text_edit::TextEditState::load(&app.ctx, id).is_none(), "état du champ effacé");
        assert!(app.current.is_some(), "session ouverte");
    }

    /// Images jusqu'à ce que l'écran ne redemande rien avant 5 s pendant dix
    /// images de suite (4 s d'attente au plus) ; rend la dernière. Le travail
    /// fini des autres fils (polices du système, comptes, réglages) est alors
    /// passé ; une animation, elle, n'y arrive jamais.
    pub(super) fn settle(app: &mut super::App) -> egui::FullOutput {
        let rest = |out: &egui::FullOutput| out.viewport_output.values().all(|v| v.repaint_delay >= Duration::from_secs(5));
        let start = std::time::Instant::now();
        let mut calm = 0;
        loop {
            let out = frame(app, Vec::new());
            calm = if rest(&out) { calm + 1 } else { 0 };
            if calm >= 10 || start.elapsed() > Duration::from_secs(4) {
                return out;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Écran « connexion en cours » : aucune animation ; prochain redessin
    /// au premier palier du temps écoulé (10 s).
    #[test]
    fn busy_screen_does_not_animate() {
        let (mut app, _) = login_app(&[], None);
        app.add_account(crate::account::Token::from("MTk4.Gx7Qz.TEST"), false);
        let delay = |out: &egui::FullOutput| out.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay).unwrap_or_default();
        let rest = |out: &egui::FullOutput| delay(out) >= Duration::from_secs(5);
        let mut out = frame(&mut app, Vec::new());
        // L'apparition de l'écran (fondu de 160 ms, lot ANIM) est finie après ces images.
        for _ in 0..30 {
            out = frame(&mut app, Vec::new());
        }
        // Cause de l'instabilité : du travail fini arrive d'autres fils à un
        // moment qui dépend de la charge de la machine. Les polices du système
        // (chargées par un fil) relancent la préparation des glyphes, une
        // police par image (`warm.rs`) ; les fils des comptes et des réglages
        // réveillent l'interface à chaque réponse. Un nombre fixe d'images ne
        // dit donc pas que tout est passé : on attend que l'écran se taise dix
        // images de suite, ce qu'une animation ne ferait jamais, sans
        // dépasser le palier de 10 s.
        let _ = out;
        let out = settle(&mut app);
        assert!(app.is_busy());
        assert!(rest(&out), "jamais au repos, redessin dans {:?} : {:?}", delay(&out), app.ctx.repaint_causes());
        // Annuler revient au formulaire.
        app.cancel();
        assert!(!app.is_busy() && app.current.is_none());
    }

    /// `DISCORD_TOKEN` abîmé : ignoré, avec la raison sur l'écran de connexion.
    #[test]
    fn damaged_env_token_is_reported() {
        let (app, _) = login_app(&[], Some(Err(crate::account::TokenError::Invalid("le jeton contient des espaces"))));
        assert!(app.env_token.is_none() && app.current.is_none());
        assert!(app.login_error.is_none(), "n'empêche rien : {:?}", app.login_error);
        assert!(app.env_warning().is_some_and(|e| e.contains("DISCORD_TOKEN") && e.contains("espaces")), "{:?}", app.env_warning());
    }

    /// Taille et place de la fenêtre retenues en points logiques (sans le
    /// zoom de l'interface) ; maximisée, celles d'avant sont gardées.
    #[test]
    fn window_geometry_is_tracked_without_zoom() {
        let mut h = harness(crate::theme::Tokens::default());
        let ctx = h.app.ctx.clone();
        let input = |maximized: bool, zoom: f32| {
            ctx.set_zoom_factor(zoom);
            let mut raw = egui::RawInput::default();
            let vp = raw.viewports.entry(egui::ViewportId::ROOT).or_default();
            vp.native_pixels_per_point = Some(1.5);
            // Rectangles en points d'egui : divisés par le zoom.
            vp.inner_rect = Some(egui::Rect::from_min_size(egui::pos2(110.0, 230.0) / zoom, egui::vec2(800.0, 600.0) / zoom));
            vp.outer_rect = Some(egui::Rect::from_min_size(egui::pos2(100.0, 200.0) / zoom, egui::vec2(820.0, 640.0) / zoom));
            vp.maximized = Some(maximized);
            for _ in 0..2 {
                let mut out = ctx.run_ui(raw.clone(), |_| {});
                out.textures_delta.clear();
            }
        };
        for zoom in [1.0, 2.0] {
            input(false, zoom);
            h.app.track_window();
            let w = h.app.prefs.now.window;
            assert_eq!((w.size, w.position, w.maximized), (Some([800.0, 600.0]), Some([100.0, 200.0]), false), "zoom {zoom}");
        }
        input(true, 1.0);
        h.app.track_window();
        let w = h.app.prefs.now.window;
        assert_eq!((w.size, w.position, w.maximized), (Some([800.0, 600.0]), Some([100.0, 200.0]), true));
    }

    fn offline() -> Appearance {
        let (link, first) = ThemeLink::open(None, false, WatchConfig::default());
        Appearance::new(link, first).0
    }

    /// Zoom au clavier hors bornes : ramené dans les bornes, pour le thème
    /// comme pour egui ; et le fichier exporté se relit.
    #[test]
    fn keyboard_zoom_is_clamped() {
        let ctx = egui::Context::default();
        let mut look = offline();
        for (asked, expected) in [(ZOOM.max + ZOOM.max, ZOOM.max), (ZOOM.min * ZOOM.min, ZOOM.min)] {
            ctx.set_zoom_factor(asked);
            pass(&ctx, |_| {});
            assert_eq!(ctx.zoom_factor(), asked);
            look.adopt_zoom(&ctx);
            assert_eq!(look.edited.layout.zoom, expected);
            pass(&ctx, |_| {});
            assert_eq!(ctx.zoom_factor(), expected, "egui reçoit la valeur bornée");
            look.adopt_zoom(&ctx);
            assert_eq!(look.edited.layout.zoom, expected);
            assert!(parse(&export(&look.edited)).is_ok());
        }
    }

    /// Un zoom demandé mais pas encore effectif n'est pas pris pour un zoom au clavier.
    #[test]
    fn pending_zoom_is_not_adopted() {
        let ctx = egui::Context::default();
        pass(&ctx, |_| {});
        let mut look = offline();
        let target = ZOOM.max;
        look.edited.layout.zoom = target;
        look.request_zoom(&ctx, target);
        look.adopt_zoom(&ctx);
        assert_eq!(look.edited.layout.zoom, target);
        pass(&ctx, |_| {});
        look.adopt_zoom(&ctx);
        assert_eq!(look.edited.layout.zoom, target);
    }

    #[test]
    fn dragged_channels_width_is_clamped() {
        let mut look = offline();
        look.adopt_members_width(f32::INFINITY);
        assert_eq!(look.edited.layout.members_width, crate::theme::MEMBERS_WIDTH.min);
        look.adopt_members_width(300.4);
        assert_eq!(look.edited.layout.members_width, 300.0);
        for (w, expected) in [(CHANNELS_WIDTH.max + CHANNELS_WIDTH.max, CHANNELS_WIDTH.max), (-CHANNELS_WIDTH.max, CHANNELS_WIDTH.min), (f32::NAN, CHANNELS_WIDTH.min)] {
            look.adopt_channels_width(w);
            assert_eq!(look.edited.layout.channels_width, expected);
            assert!(parse(&export(&look.edited)).is_ok());
        }
    }

    /// Fichier refusé : le panneau n'enregistre rien et le dit, ligne comprise.
    #[test]
    fn refused_file_blocks_saving() {
        let dir = std::env::temp_dir().join(format!("ferrocord-ui-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{dir:?} : {e}"));
        let path: PathBuf = dir.join("theme.toml");
        let broken = "base = \"clair\"\n[couleurs\n";
        fs::write(&path, broken).unwrap_or_else(|e| panic!("{path:?} : {e}"));
        let cfg = WatchConfig { poll: Duration::from_millis(20), debounce: Duration::from_millis(30) };
        let (link, first) = ThemeLink::open(Some(path.clone()), true, cfg);
        let (mut look, notice) = Appearance::new(link, first);
        let notice = notice.unwrap_or_default();
        assert!(notice.contains("theme.toml contient une erreur ligne 2 : corrigez-le ou cliquez sur Réinitialiser le fichier"), "{notice}");
        look.edited = Preset::Minuit.tokens();
        look.persist();
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(fs::read_to_string(&path).unwrap_or_default(), broken);
        drop(look);
        assert_eq!(fs::read_to_string(&path).unwrap_or_default(), broken, "ni à la fermeture");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Vue d'appel (phase 5) : une texture seulement pour une tuile affichée
    /// qui reçoit des images, rendue quand la tuile disparaît ; aucun réveil hors champ.
    #[test]
    fn call_view_uploads_only_visible_tiles() {
        use crate::state::video::{test_pattern, Tile};
        use crate::state::voice::{VoiceEvent, VoiceState};
        use std::sync::atomic::{AtomicUsize, Ordering};
        let mut h = harness(Preset::Rouille.tokens());
        let joined = |user_id: u64, self_video: bool| crate::state::Event::Voice(VoiceEvent::StateUpdate { guild_id: None, user_id, state: Some(VoiceState { user_id, channel_id: 50, self_video, ..Default::default() }), session_id: None });
        h.app.store.apply(joined(1, false));
        h.app.store.apply(joined(7, true));
        h.app.show_call(None, 50);
        let sink = h.app.video_sink();
        h.frame();
        let wakes = std::sync::Arc::new(AtomicUsize::new(0));
        let w = wakes.clone();
        sink.set_wake(std::sync::Arc::new(move || {
            w.fetch_add(1, Ordering::Relaxed);
        }));
        assert!(sink.is_visible(&Tile::Camera(7)), "caméra d'Alice affichée");
        assert!(!sink.is_visible(&Tile::Camera(1)), "ma tuile sans caméra : aucune image attendue");
        assert_ne!(sink.wanted_quality(&Tile::Camera(7)), (0, 0));
        assert_eq!(h.app.video_textures(), 0, "rien reçu, rien téléversé");
        sink.push(Tile::Camera(7), &test_pattern(64, 36, 1, 7));
        sink.push(Tile::Camera(1), &test_pattern(64, 36, 1, 1));
        assert_eq!(wakes.load(Ordering::Relaxed), 1, "seule la tuile affichée réveille");
        h.frame();
        assert_eq!(h.app.video_textures(), 1);
        // Autre écran que l'appel : plus de tuile, plus de texture.
        h.app.store.channel = None;
        h.frame();
        assert_eq!(h.app.video_textures(), 0);
        assert!(!sink.is_visible(&Tile::Camera(7)));
        sink.push(Tile::Camera(7), &test_pattern(64, 36, 2, 7));
        assert_eq!(wakes.load(Ordering::Relaxed), 1, "hors champ : aucun réveil");
    }
}
