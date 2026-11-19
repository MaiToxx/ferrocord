//! Aperçu hors ligne de la vraie interface, pour vérifier l'apparence.
//!
//! Le magasin est rempli à partir des jeux de données de `tests/fixtures/` :
//! aucune session réseau, aucun jeton lu (ni fichier, ni `DISCORD_TOKEN`),
//! rien d'écrit dans le dossier de configuration.
//!
//! ```text
//! cargo run --example preview -- [--theme <préréglage|fichier.toml>] [--view <vue>]
//!                                [--panel] [--hover-none] [--pointer x,y] [--notice]
//!                                [--lang fr|en] [--screenshot <capture.png>] [--audit [--only section.clé,…]]
//! ```
//!
//! - `--theme` : nom d'un préréglage (rouille, minuit, clair, contraste,
//!   compact, aere) ou chemin d'un `theme.toml`, relu à chaud et jamais réécrit ;
//! - `--view` : `guild` salon d'un serveur (par défaut), `dm` conversation
//!   privée, `login` écran de connexion ; vues de permissions (READY avec
//!   rôles) : `masque` (salons et catégorie masqués, sur #général),
//!   `lecture-seule` (#annonces), `sans-historique` (#ancien), `inconnu`
//!   (Alpha sans mon membre : tout affiché, #modération compris), `exclusion`
//!   (exclusion temporaire en cours) ; vues des comptes (coffre en mémoire,
//!   jetons factices) : `comptes` (écran de connexion avec trois comptes
//!   enregistrés), `selecteur` (dernier compte rouvert à son dernier salon,
//!   sélecteur de compte ouvert), `connexion` (connexion en cours) ; vues
//!   du fichier de jeton en clair (fichier factice, dossier temporaire) :
//!   `migration` (1er lancement : copié, gardé jusqu'au lancement suivant),
//!   `suppression` (question de suppression, coffre lu), `doublon` et
//!   `doublon-retrait` (même compte enregistré deux fois), `refus-import`
//!   (import refusé auparavant : « Importer » / « Supprimer ») ; vues
//!   sociales (READY avec relations et états de lecture) : `amis` (page Amis),
//!   avec `--scene menu-utilisateur`, `demandes` ou `pseudo-ami` : lot U),
//!   `groupe` (MP de groupe, panneau des membres ouvert) ;
//! - `--panel` : fenêtre Paramètres ouverte sur Apparence ;
//! - `--settings <page>` : fenêtre Paramètres ouverte sur cette page (`compte`,
//!   `profil`, `statut`, `confidentialite`, `applications`, `appareils`,
//!   `comptes-lies`, `facturation`, `apparence`, `texte`, `notifications`,
//!   `langue`, `dossiers`, `ferrocord`, `raccourcis`, `voix` : périphériques inventés et test du micro, `plugins`, `plugins-reglages` : fenêtre de réglages d'un plugin), réponses du compte fictives ;
//! - `--hover-none` : la souris est ignorée (aucun survol sur la capture) ;
//! - `--pointer x,y` : pointeur immobile à cette position (en points), pour un survol ;
//! - `--notice` : une notification (sur l'écran de connexion : une erreur) au départ ;
//! - `--profile` : carte de profil d'Alice ouverte (vues `guild` et `dm`) ;
//! - `--profile-full` : profil complet d'Alice ouvert (onglet « À propos ») ;
//! - `--lang fr|en` : langue de l'interface (par défaut : celle du système) ;
//! - `--screenshot` : attend que l'image soit stable, l'enregistre en PNG et quitte ;
//! - `--at <ms>` : avec `--screenshot`, capture à cet instant après le lancement sans
//!   attendre une image stable (animations ; scènes `ecriture`, `ecriture-deux`,
//!   `ecriture-plusieurs` : indicateur d'écriture et pastille qui écrit ; `transitions` :
//!   à 1 s, un message qui mentionne arrive et la carte du compte s'ouvre) ;
//! - `--audit` : modifie chaque jeton à tour de rôle et affiche ceux qui ne
//!   changent aucun pixel de la vue choisie, puis quitte ; `--only` limite
//!   l'audit à quelques jetons ;
//! - `--set section.clé=valeur` (répétable) : change un jeton du préréglage,
//!   par exemple `--set mise_en_page.avatar_shape=carres` ;
//! - `--no-images` : avatars et icônes jamais chargés (remplaçants seuls) ;
//! - `--scene <nom>` : situation de la liste de messages (vue `guild`) :
//!   `nouveaux` (ligne « Nouveaux messages » à l'ouverture de #général),
//!   `bouton` (vue remontée puis messages arrivés : bouton du bas),
//!   `present` (fenêtre du cache remontée : « Revenir au présent »),
//!   `non-envoyes` (comme `present`, avec des messages non envoyés au-dessus
//!   de la saisie : les deux bandes ensemble), `saisie` (zone de saisie :
//!   « X et Y écrivent… », mode lent, autocomplétion de « @al », brouillon
//!   dans #hors-sujet), `saisie-longue` (même chose, avec un long texte :
//!   compteur de caractères et hauteur bornée), `fichiers` (plateau de trois
//!   pièces jointes, un envoi en cours, un envoi en échec), `fichier-details`
//!   (fenêtre des détails d'une pièce jointe), `parcourir` (fenêtre
//!   « Parcourir les salons »), `recherche`
//!   (champ de recherche et panneau des résultats, mots surlignés), `emojis` (vrai gras, emojis
//!   en image dans le texte, en grand, en réaction et dans un statut, emoji
//!   du serveur, autocollants, sélecteur d'emojis de la saisie ouvert ; les
//!   images des emojis sont générées : l'aperçu ne contacte aucun réseau),
//!   `gifs` (sélecteur de GIF : favoris, tendances, catégories), `gifs-recherche`
//!   (résultats de « chat »), `gifs-favoris` (favoris du compte ; lot G),
//!   `super-reaction` (sélecteur de réaction d'un compte Nitro, super réaction active),
//!   `selecteur` (sélecteur rapide ouvert sur « gen »), `boite` (boîte de réception : salons non lus en cartes),
//!   `boite-cartes` (premières cartes repliées), `boite-programmes` (onglet « Programmés »),
//!   `boite-mentions` (onglet « Mentions » de la boîte de
//!   réception, mentions du jeu d'essai), `raccourcis` (fenêtre des raccourcis) ;
//!   `applications` (boutons, listes, composants v2, « … réfléchit », échec
//!   d'une commande, formulaire de /sondage), `modale` (fenêtre d'un robot) ;
//!   `sondages` (sondage, résultat, épingle, annonce suivie, invitation) ;
//!   `parametres-serveur`, `parametres-serveur-webhooks`, `parametres-serveur-integrations`,
//!   `parametres-serveur-boosts` (fenêtre « Paramètres du serveur » d'Alpha, lot A8 + A9) ;
//!   `salons`, `salons-permissions`, `salons-creer`, `salons-serveur`
//!   (« Modifier le salon », création, page Salons du serveur, vue `guild`) ;
//!   `evenements` (liste des événements d'Alpha), `evenement-creer` (fenêtre
//!   de création), `bienvenue` (écran de bienvenue), `integration` (questions
//!   de l'intégration), `parametres-integration` et `parametres-bienvenue`
//!   (pages des paramètres du serveur) : lot A7 ;
//!   `appel` (vue d'appel : 4 personnes dont 2 en caméra, mires animées),
//!   `appel-direct` (direct d'Ève regardé et mis en avant), `appel-partage`
//!   (barre de contrôle pendant un partage d'écran) : phase 5, lot M4 ;
//!   `partage` (sélecteur de partage, onglet Applications), `partage-ecrans`,
//!   `partage-appareils`, `partage-camera` (aperçu avant d'allumer la caméra),
//!   `partage-qualite` (menu de l'engrenage) : sources et images inventées ;
//!   `appel-mp` (appel d'un MP en bandeau au-dessus de la conversation),
//!   `appel-sortant` (le correspondant n'a pas encore décroché),
//!   `appel-groupe` (quatre personnes dont une qui sonne), `appel-mp-partage`
//!   (bandeau agrandi par un partage d'écran) : lot « appel » ;
//!   `entete` (en-tête de 48 px : sujet tronqué, Fils, cloche, épingles,
//!   membres, recherche à loupe), `entete-sujet` (sujet affiché en entier),
//!   `menu-serveur` (menu du serveur ouvert, lignes de menu du lot V1),
//!   `entete-fils` et `entete-archives` (panneau « Fils »), `entete-cloche`
//!   (menu des notifications, salon muet), `categories` (catégorie repliée
//!   qui garde son salon non lu, salon muet éteint, icônes de fin de ligne),
//!   `categories-muets` (« Masquer les salons muets »), `salon-vocal`
//!   (« Modifier le salon » d'un salon vocal) : phase 10, lot S ;
//!   avec `--view login`, `qr` (connexion par QR code : QR et compte à
//!   rebours) et `qr-lu` (QR lu, confirmation attendue) : lot C2 ;
//!   avec `--view connexion`, `hors-ligne` (historique hors ligne d'une
//!   session précédente affiché avant READY, temps affiché) : lot H ;
//!   `mon-compte` (carte de mon compte ouverte au-dessus du panneau
//!   utilisateur ; `mon-compte-sobre` : profil sans thème, `mon-compte-clair` :
//!   thème de profil clair, `mon-compte-statut` : sous-menu des statuts,
//!   `mon-compte-statut-perso` : fenêtre du statut personnalisé), `profil-edition` (édition rapide du profil, compte Nitro,
//!   bannière choisie), `profil-edition-serveur` (onglet « Profil de ce
//!   serveur »), `profil-edition-sans-nitro`, `profil-edition-style`
//!   (sélecteur « Style du pseudonyme » : chaque police dans sa forme) : lot « profil rapide » ;
//! - `--size l,h` : taille de la fenêtre en points (une fenêtre étroite, par exemple) ;
//! - `--mobile <écran>` (phase 7, avec `--size 390,844`) : disposition mobile
//!   imposée, sur `salons`, `discussion`, `membres`, `messages`,
//!   `notifications` ou `vous` ; `appui-long` : discussion, puis appui long
//!   simulé sur un message (son menu s'ouvre) ; se combine avec `--settings`
//!   (Paramètres en plein écran) et `--scene appel` (vue d'appel) ;
//! - `--scene apparence-editeur` (éditeur de thème de la page Apparence), `theme-degrade`,
//!   `fond-image`, `densites`, `densites-spacieuse`, `sans-rail` (lot PERSO) ;
//! - `--scene quetes` et `boutique` (lot ND, vue `amis`) : page des quêtes, boutique
//!   en consultation ; `--scene effet` et
//!   `effet-fixe` (avec `--profile` ou `--profile-full`) : effet de profil d'Alice,
//!   animé ou en image fixe ; le choix de l'effet se voit dans `profil-edition` ;
//! - `--scene profil-carte` (lot profil ; avec `--profile-full` : le profil complet) : carte
//!   de profil d'Alice comme la capture de référence (thème violet, effet trouvé par le
//!   catalogue, bulle de statut, badges, amis en commun, comptes connectés) ; avec
//!   `--pointer x,y` sur un badge : son infobulle détaillée ;
//! - `--scene panneau` et `panneau-menu` (lot profil) : panneau utilisateur du bas (ma plaque
//!   en fond, ma décoration, mon nom stylé, manette, micro coupé) ; le menu du chevron du micro ouvert ;
//! - `--scene jeu-detecte` et `jeu-detecte-appel` (lot « activité ») : un jeu fictif détecté, son encart
//!   au-dessus du panneau utilisateur, la manette sous mon nom ; dans un vocal, le bouton de partage ;
//! - `--scene parures` (lot P ; vues `guild` et `dm`, avec ou sans `--profile`
//!   ou `--profile-full`) : décorations d'avatar, plaques, tags de serveur,
//!   styles de nom, rôles en dégradé, bannière et avatar animés d'Alice, avec
//!   « réduire les animations » (images fixes) ; `--scene parures-animees` :
//!   la même, animations permises (deux captures successives diffèrent) ;
//! - `--scene polices-nom` (lot polices) : planche des polices de nom, chacune sur un nom
//!   uni, avec un effet, et sur un nom mêlant latin, emoji, cyrillique et chinois ;
//! - `--sons <dossier>` (lot « sons ») : écrit chaque son d'interface en WAV dans ce dossier
//!   (un fichier par événement, au nom accepté par le dossier des sons personnels), sans
//!   ouvrir de fenêtre ni rien jouer ;
//! - `--scene mp-systeme` et `transfert` (lot « messages » ; avec `--at 4000` pour laisser
//!   les images arriver) : MP du compte officiel (bandeau à la place de la saisie, deux
//!   cartes de notification), messages transférés avec image, fichier, embed et autocollant ;
//! - `--bench` : banc de mesure des budgets (voir `bench/mod.rs`) sur un
//!   magasin synthétique ; réglages `--bench-guilds N` (100), `--bench-channels N`
//!   (50), `--bench-messages N` (500), `--bench-deep N` (2000), `--bench-frames N` (images par phase,
//!   300), `--bench-quiet S` (observation au repos, 10 s), `--bench-out
//!   rapport.json`, `--bench-label nom` ; la fenêtre se ferme seule ;
//! - `--bench-cpu <secondes>` : banc du processeur (lot « cpu », voir
//!   `bench/cpu.rs`) : démarrage d'un compte simulé, processeur consommé et
//!   délai jusqu'au calme, puis repos sans et sous trafic pendant `secondes` ;
//! - `--angle` (lot « gl ») : pose les deux bibliothèques d'ANGLE à côté de l'aperçu,
//!   qui dessine alors par Direct3D 11 comme l'application installée (elles y restent ;
//!   `FERROCORD_RENDU=opengl` revient au pilote OpenGL) ; le rapport des bancs nomme le
//!   rendu (`rendu`) et les bibliothèques graphiques chargées (`pilotes`) ;
//! - `--bench-long <visites>` : banc d'usage prolongé (lot « long », voir
//!   `bench/long.rs`) : cinq tours de `visites` salons ouverts à la suite
//!   (500 pour la mesure de référence), deux heures d'événements simulés et 25
//!   reconnexions par tour ; relevé du tas vivant, de la mémoire privée, des
//!   handles et des fils après chaque tour, verdicts « rien ne s'accumule »
//!   (code de sortie 1 sinon) ;
//! - `--bench-atlas-libre` (avec `--bench`) : sans la borne de l'atlas des
//!   polices, pour comparer les temps d'image avec et sans elle dans le même
//!   exécutable ;
//! - `--bench-mem <scénario>` : mémoire du processus après 3 s d'une fenêtre
//!   `vide`, `vide-sans-polices`, `vide-logiciel` (rendu OpenGL non
//!   accéléré), `connexion` (écran de connexion), `app-sans-images` ou `app`
//!   (magasin du banc), puis fermeture ; `demarrage` (lot « ram ») rejoue le
//!   démarrage d'un compte simulé (READY de 100 serveurs, 10 000 jeux
//!   détectables, un salon affiché) et donne, étape par étape, les octets
//!   vivants du tas, la mémoire privée et sa répartition (modules, piles, tas,
//!   reste) ; `demarrage-demi` et `demarrage-double` changent la taille du
//!   compte, `demarrage-parties` pèse chaque partie du magasin ; code de
//!   sortie 1 si le tas vivant dépasse le seuil ; `FERROCORD_BANC_TRACE=<octets>`
//!   nomme par pile d'appels les blocs survivants d'au moins cette taille ;
//!
//! Avatars et icônes viennent d'une source locale (images générées par le
//! code) : l'aperçu ne contacte aucun réseau et n'écrit aucun cache disque.

#[path = "bench/mod.rs"]
mod bench;

// Allocateur compteur (lot « ram ») : celui du système, plus des compteurs. Dans l'aperçu seulement.
#[global_allocator]
static HEAP: bench::alloc::Counting = bench::alloc::Counting;

use eframe::egui;
use ferrocord::images::{self, FetchFuture, Images, Source};
use ferrocord::net::cdn::FetchError;
use ferrocord::net::{decode_history, Decoder};
use ferrocord::state::model::{ImageRef, Message};
use ferrocord::account::{AccountKey, AccountLink, MemoryVault, Token};
use ferrocord::settings::{Account, Place, Settings, SettingsLink, SettingsUpdate};
use ferrocord::state::model::ImageHash;
use ferrocord::i18n::Choice;
use ferrocord::state::{Cmd, Event, PeopleHub, Problem, Store, HISTORY_PAGE};
use ferrocord::state::social::{Pin, Reactor, SendKind, SocialCmd, SocialEvent};
use ferrocord::state::upload::{Failure, UploadCmd, UploadEvent};
use ferrocord::state::search::{SearchCmd, SearchEvent};
use ferrocord::theme::{FieldMut, Preset, ThemeLink, Tokens, WatchConfig};
use ferrocord::ui::{App, Backend, Boot, Wake};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

/// Source d'images de l'aperçu : chaque image est dessinée par le code
/// (dégradé, silhouette ou losange), encodée tour à tour en PNG, JPEG ou
/// WebP pour passer par les vrais décodeurs.
struct Generated;

impl Source for Generated {
    fn url(&self, image: &ImageRef, size: u32) -> String {
        let (kind, id) = match image {
            ImageRef::Avatar { user_id, .. } => ("avatar", *user_id),
            ImageRef::GuildIcon { guild_id, .. } => ("icone", *guild_id),
            ImageRef::GroupIcon { channel_id, .. } => ("groupe", *channel_id),
            ImageRef::Media { owner, .. } => ("media", *owner),
            ImageRef::Banner { user_id, .. } => ("banniere", *user_id),
            ImageRef::MemberAvatar { guild_id, user_id, .. } => ("avatar", user_id ^ guild_id),
            ImageRef::MemberBanner { guild_id, user_id, .. } => ("banniere", user_id ^ guild_id),
            ImageRef::BadgeIcon { hash } => ("badge", hash.to_string().bytes().fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(u64::from(b)))),
            ImageRef::Twemoji(u) => ("emoji", u64::from(u.index) * 8 + u64::from(u.tone)),
            ImageRef::CustomEmoji { id } => ("emoji", *id),
            ImageRef::Sticker { id, .. } | ImageRef::LottieSticker { id } => ("autocollant", *id),
            ImageRef::RoleIcon { role_id, .. } => ("icone", *role_id),
            ImageRef::EventCover { event_id, .. } => ("couverture", *event_id),
            ImageRef::Decoration { asset } => ("decoration", name_seed(&asset.name())),
            ImageRef::Nameplate { asset } => ("plaque", name_seed(&asset.name())),
            ImageRef::TagBadge { guild_id, .. } => ("badge", *guild_id),
            ImageRef::AppIcon { app_id, .. } => ("couverture", *app_id),
        };
        format!("apercu://{kind}/{id}/{size}")
    }

    /// Image jointe : un dégradé à la taille demandée, différent selon l'adresse.
    fn media_url(&self, url: &str, width: u32, height: u32) -> String {
        let id = url.bytes().fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(u64::from(b)));
        format!("apercu://media/{id}/{width}x{height}")
    }

    /// Emoji animé (lot A6) : un GIF généré.
    fn anim_url(&self, image: &ImageRef, size: u32) -> Option<String> {
        match image {
            ImageRef::CustomEmoji { id } => Some(format!("apercu://emoji-anime/{id}/{size}")),
            // Lot P : bannière, avatar et décoration animés (empreinte `a_`).
            ImageRef::Banner { user_id, hash } if hash.animated => Some(format!("apercu://banniere-animee/{user_id}/{size}")),
            ImageRef::MemberBanner { guild_id, user_id, hash } if hash.animated => Some(format!("apercu://banniere-animee/{}/{size}", user_id ^ guild_id)),
            ImageRef::Avatar { user_id, hash } if hash.animated => Some(format!("apercu://avatar-anime/{user_id}/{size}")),
            ImageRef::MemberAvatar { guild_id, user_id, hash } if hash.animated => Some(format!("apercu://avatar-anime/{}/{size}", user_id ^ guild_id)),
            ImageRef::Decoration { asset } if asset.animated() => Some(format!("apercu://decoration-animee/{}/{size}", name_seed(&asset.name()))),
            _ => None,
        }
    }

    fn fetch(&self, url: String) -> FetchFuture {
        Box::pin(async move { generate(&url).ok_or(FetchError::NotFound) })
    }
}

/// Scène `parures` (lot P) : les données fictives portent décorations,
/// plaques, tags, styles de nom et rôles en dégradé.
static PARURES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn parures() -> bool {
    PARURES.load(std::sync::atomic::Ordering::Relaxed)
}

/// Scène `amis` (lot « amis ») : la page Amis et la colonne des MP garnies
/// comme l'accueil du client (tags de serveur, style de nom, décoration,
/// plaque, jeu, partage d'écran, statuts personnalisés, surnom avec « 🩶 »).
static AMIS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn amis() -> bool {
    AMIS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Scènes `vocal-*` (lot « vocal ») : des amis sont en vocal sur un serveur commun.
static VOCAL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Scène `mp-chat` : la scène `amis`, ouverte sur la conversation avec Alice.
static MPCHAT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Scène « mp-activite » : Alice sans bannière ni thème, seulement sa couleur d'accent.
static SANS_BANNIERE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Scène `mp-systeme` : le MP du compte officiel s'ajoute à ceux de la scène `amis`.
static MPSYSTEME: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// MP du compte officiel (scène `mp-systeme`).
const DM_SYSTEME: u64 = 4_000_000_000_000_000_090;

/// Le compte officiel, tel que la passerelle le décrit (`system`).
fn compte_officiel() -> Value {
    serde_json::json!({ "id": ferrocord::state::store::official::DISCORD_SYSTEM_USER.to_string(), "username": "discord", "global_name": "Discord", "discriminator": "0", "avatar": null, "system": true })
}

/// Message d'aperçu, `minutes` avant maintenant ; `more` complète ou remplace ses champs.
fn message_apercu(channel: u64, minutes: i64, author: Value, content: &str, more: Value) -> Vec<Event> {
    let now = chrono::Utc::now().timestamp_millis();
    let mut d = serde_json::json!({
        "id": (((now - minutes * 60_000 - 1_420_070_400_000) as u64) << 22).to_string(), "channel_id": channel.to_string(), "author": author,
        "content": content, "timestamp": "2025-01-15T10:06:00.000000+00:00", "edited_timestamp": null, "tts": false,
        "mention_everyone": false, "mentions": [], "mention_roles": [], "attachments": [], "embeds": [], "components": [], "pinned": false, "type": 0, "flags": 0
    });
    if let (Some(d), Some(more)) = (d.as_object_mut(), more.as_object()) {
        d.extend(more.clone());
    }
    packet(&serde_json::json!({ "t": "MESSAGE_CREATE", "d": d }))
}

/// Scène `mp-systeme` : un message du compte officiel, puis deux notifications
/// (l'une d'aujourd'hui avec « En savoir plus », l'autre plus ancienne et sans lien).
fn mp_systeme_events() -> Vec<Event> {
    use serde_json::json;
    let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
    let field = |name: &str, value: String| json!({ "name": name, "value": value, "inline": false });
    let notice = |header: &str, body: &str, days: f64, link: Option<&str>| {
        let mut fields = vec![
            field("client_version_message", "Mets à jour ton application pour voir ce message.".into()),
            field("classification_id", "9000000000000000001".into()),
            field("body", body.into()),
            field("icon_type", "megaphone".into()),
            field("header", header.into()),
            field("timestamp", format!("{:.3}", now - days * 86_400.0)),
            field("theme", "default".into()),
            field("ctas", if link.is_some() { "learn_more_link" } else { "" }.into()),
        ];
        if let Some(link) = link {
            fields.push(field("learn_more_link", link.into()));
        }
        json!({ "type": "safety_system_notification", "fields": fields })
    };
    let mut out = message_apercu(DM_SYSTEME, 3 * 24 * 60, compte_officiel(), "Bienvenue ! Ce fil rassemble les annonces qui concernent ton compte d'aperçu.", json!({}));
    let embeds = json!([
        notice("Le règlement de la communauté évolue", "Une nouvelle version entre en vigueur le mois prochain. Rien ne change pour ton compte d'ici là.", 2.0, None),
        notice("Tes réglages de confidentialité ont été vérifiés", "Tout est en ordre. Tu peux les revoir à tout moment depuis les paramètres.", 0.0, Some("https://support.discord.com/hc/articles/360036118732")),
    ]);
    out.extend(message_apercu(DM_SYSTEME, 20, compte_officiel(), "", json!({ "embeds": embeds })));
    out
}

/// Scène `transfert` : Alice transfère un message avec fichier, embed et autocollant, puis un autre avec image.
fn transfert_events() -> Vec<Event> {
    use serde_json::json;
    let alice = json!({ "id": "1000000000000000002", "username": "alice_fictive", "global_name": "Alice", "discriminator": "0", "avatar": "a11ce0000000000000000000000000a1" });
    let image = json!({ "id": "6000000000000000071", "filename": "croquis.png", "size": 184_000, "content_type": "image/png", "width": 960, "height": 600,
        "url": "https://cdn.discordapp.com/attachments/1/6000000000000000071/croquis.png", "proxy_url": "https://media.discordapp.net/attachments/1/6000000000000000071/croquis.png" });
    let snapshot = |message: Value| json!({ "message_reference": { "type": 1, "channel_id": GENERAL.to_string(), "message_id": "5000000000000000070" }, "message_snapshots": [{ "message": message }] });
    let mut out = message_apercu(DM, 12, alice.clone(), "", snapshot(json!({
        "content": "La version avec **les trois îles**, à valider avant samedi.", "type": 0, "flags": 0, "timestamp": "2025-01-14T18:00:00.000000+00:00",
        "edited_timestamp": null, "mentions": [], "mention_roles": [], "attachments": [image], "embeds": []
    })));
    out.extend(message_apercu(DM, 13, alice, "", snapshot(json!({
        "content": "", "type": 0, "flags": 0, "timestamp": "2025-01-14T18:05:00.000000+00:00", "edited_timestamp": null, "mentions": [], "mention_roles": [],
        "attachments": [{ "id": "6000000000000000072", "filename": "legende.pdf", "size": 420_000, "content_type": "application/pdf",
            "url": "https://cdn.discordapp.com/attachments/1/6000000000000000072/legende.pdf", "proxy_url": "https://media.discordapp.net/attachments/1/6000000000000000072/legende.pdf" }],
        "embeds": [{ "type": "rich", "title": "Carnet de route", "description": "Étape 4 : traversée du col, puis descente vers le lac.", "color": 0x3ba55d }],
        "sticker_items": [{ "id": "7000000000000000031", "name": "Bravo", "format_type": 1 }]
    }))));
    out
}

/// Scène `mp-chat` : échange avec Alice, aujourd'hui et hier ; une réponse à l'un de mes
/// messages qui me mentionne (fond ambre), une autre sans mention (aucun fond).
fn mp_chat_events() -> Vec<Event> {
    use serde_json::json;
    let now = chrono::Utc::now().timestamp_millis();
    let alice = json!({ "id": "1000000000000000002", "username": "alice_fictive", "global_name": "Alice", "discriminator": "0", "avatar": "a11ce0000000000000000000000000a1" });
    let me = json!({ "id": ME.to_string(), "username": "moi_fictif", "global_name": "Moi", "discriminator": "0", "avatar": null });
    // (minutes avant maintenant, d'Alice ?, texte, réponse au message n° …, avec mention ?)
    let lines: [(i64, bool, &str, Option<usize>, bool); 9] = [
        (26 * 60, true, "On regarde ça demain ?", None, false),
        (26 * 60 - 1, false, "Oui, demain après-midi", None, false),
        (14, true, "Tu veux voc un peu ?", None, false),
        (13, false, "Là je suis avec mon frère", None, false),
        (12, true, "Okk, pas de soucis !", None, false),
        (11, false, "Après demande à Ethan s'il veut se faire une partie", None, false),
        (9, true, "Un peu quand même, et aussi dans les fonctionnalités", Some(5), false),
        (8, true, "Je te le jure, tu le montres à n'importe qui, il ne fait pas gaffe", Some(3), true),
        (7, false, "Oui je m'en doute en vrai", None, false),
    ];
    let id = |minutes: i64| (((now - minutes * 60_000 - 1_420_070_400_000) as u64) << 22).to_string();
    let mut out = Vec::new();
    for (minutes, from_alice, text, reply, ping) in lines {
        let mut d = json!({
            "id": id(minutes), "channel_id": DM.to_string(), "author": if from_alice { alice.clone() } else { me.clone() },
            "content": text, "timestamp": "2025-01-15T10:06:00.000000+00:00", "edited_timestamp": null, "tts": false,
            "mention_everyone": false, "mentions": if ping { json!([me.clone()]) } else { json!([]) }, "mention_roles": [],
            "attachments": [], "embeds": [], "components": [], "pinned": false, "type": 0, "flags": 0
        });
        if let Some(n) = reply {
            let (to_minutes, to_alice, to_text, ..) = lines[n];
            d["type"] = 19.into();
            d["message_reference"] = json!({ "type": 0, "message_id": id(to_minutes), "channel_id": DM.to_string() });
            d["referenced_message"] = json!({
                "id": id(to_minutes), "channel_id": DM.to_string(), "author": if to_alice { alice.clone() } else { me.clone() },
                "content": to_text, "timestamp": "2025-01-15T10:00:00.000000+00:00", "edited_timestamp": null, "tts": false,
                "mention_everyone": false, "mentions": [], "mention_roles": [], "attachments": [], "embeds": [], "pinned": false, "type": 0, "flags": 0
            });
        }
        out.extend(packet(&json!({ "t": "MESSAGE_CREATE", "d": d })));
    }
    out
}

/// Amis ajoutés par la scène `amis` : identifiant, nom d'utilisateur, nom affiché.
const AMIS_FRIENDS: [(u64, &str, &str); 4] = [
    (1_000_000_000_000_000_011, "nox_fictif", "Nox"),
    (1_000_000_000_000_000_012, "ewa_fictif", "Ewa"),
    (1_000_000_000_000_000_013, "bjorn_fictif", "bjørn"),
    (1_000_000_000_000_000_014, "paprika_fictif", "paprika"),
];

/// Scène `amis` : READY avec plus d'amis, un MP par ami et un surnom d'ami.
fn amis_ready(v: &mut Value) {
    use serde_json::json;
    let d = &mut v["d"];
    // « ❤ » sans sélecteur d'emoji : le client le dessine en texte, à la couleur du nom.
    d["relationships"][1]["nickname"] = "Ma Lune \u{2764} 🩶".into();
    let bob = d["relationships"][1]["user"].clone();
    let mut dms = vec![json!({ "id": "4000000000000000019", "type": 1, "last_message_id": "5000000000000000090", "recipients": [bob] })];
    if MPSYSTEME.load(std::sync::atomic::Ordering::Relaxed) {
        dms.push(json!({ "id": DM_SYSTEME.to_string(), "type": 1, "last_message_id": null, "recipients": [compte_officiel()] }));
    }
    let mut friends = Vec::new();
    for (i, (id, username, name)) in AMIS_FRIENDS.iter().enumerate() {
        let user = json!({ "id": id.to_string(), "username": username, "global_name": name, "discriminator": "0", "avatar": null });
        friends.push(json!({ "id": id.to_string(), "type": 1, "nickname": null, "user_ignored": false, "user": user }));
        dms.push(json!({ "id": (4_000_000_000_000_000_010u64 + i as u64).to_string(), "type": 1, "last_message_id": (5_000_000_000_000_000_080u64 - i as u64).to_string(), "recipients": [user] }));
    }
    if let Some(list) = d["relationships"].as_array_mut() {
        list.extend(friends);
    }
    if let Some(list) = d["private_channels"].as_array_mut() {
        list.extend(dms);
    }
}

/// Scène `amis` : présences et parures des amis, et un partage d'écran.
fn amis_events() -> Vec<Event> {
    use serde_json::json;
    let plate = |n: u32, palette: &str| json!({ "nameplate": { "sku_id": "7000000000000009002", "asset": format!("nameplates/fictives/plaque{n}/"), "label": "", "palette": palette } });
    let tag = |text: &str| json!({ "identity_guild_id": ALPHA.to_string(), "identity_enabled": true, "tag": text, "badge": "7a9000000000000000000000000000b9" });
    let presence = |id: u64, username: &str, name: Value, status: &str, extra: Value, activities: Value| {
        let mut user = json!({ "id": id.to_string(), "username": username, "global_name": name, "discriminator": "0" });
        if let (Some(user), Some(extra)) = (user.as_object_mut(), extra.as_object()) {
            user.extend(extra.clone());
        }
        json!({ "t": "PRESENCE_UPDATE", "d": { "user": user, "status": status, "client_status": { "desktop": status }, "activities": activities } })
    };
    let [nox, ewa, bjorn, paprika] = AMIS_FRIENDS;
    let custom = |text: &str| json!({ "type": 4, "name": "Custom Status", "state": text });
    let mp_chat = MPCHAT.load(std::sync::atomic::Ordering::Relaxed);
    // Scène `mp-chat` : Alice porte aussi un style de nom (police et dégradé), Nox un statut en plus de son jeu.
    let alice_look = if mp_chat {
        json!({ "avatar": "a11ce0000000000000000000000000a1", "primary_guild": tag("CAT"), "display_name_styles": { "font_id": 3, "effect_id": 2, "colors": [0xc77dff, 0x7b5cf0] } })
    } else {
        json!({ "avatar": "a11ce0000000000000000000000000a1", "primary_guild": tag("NOVA"), "avatar_decoration_data": { "asset": "a_dec0000000000000000000000000a11c", "sku_id": "7000000000000009001" } })
    };
    let nox_does = if mp_chat {
        json!([{ "type": 0, "name": "Honkai: Star Rail", "created_at": 1_700_000_000_000u64 }, custom("Je suis là")])
    } else {
        json!([{ "type": 0, "name": "Honkai: Star Rail", "created_at": 1_700_000_000_000u64 }])
    };
    let packets = [
        // Alice : tag, décoration, partage d'écran et statut personnalisé.
        presence(
            1_000_000_000_000_000_002,
            "alice_fictive",
            "Alice".into(),
            "online",
            alice_look,
            json!([custom("Je suis là")]),
        ),
        // Bob (surnom « Ma Lune 🩶 ») : style de nom, plaque, tag, statut à caractères rares.
        presence(
            1_000_000_000_000_000_003,
            "bob_fictif",
            Value::Null,
            "dnd",
            json!({ "display_name_styles": { "font_id": 15, "effect_id": 1, "colors": [0xe6_e0f5] }, "collectibles": plate(3, "violet"), "primary_guild": tag("ᓚᘏᗢ") }),
            json!([custom("Le temps file... ๑ ⋆｡°☆")]),
        ),
        presence(nox.0, nox.1, nox.2.into(), "online", json!({ "display_name_styles": { "font_id": 3, "effect_id": 2, "colors": [0xff7a59, 0x8a5cf0] } }), nox_does),
        presence(ewa.0, ewa.1, ewa.2.into(), "idle", json!({ "primary_guild": tag("blend") }), json!([{ "type": 4, "name": "Custom Status", "state": "Dodo", "emoji": { "name": "🌙" } }])),
        presence(bjorn.0, bjorn.1, bjorn.2.into(), "online", json!({}), json!([custom("𐙚 ⋆｡° 𖦹 ๑")])),
        presence(paprika.0, paprika.1, paprika.2.into(), "offline", json!({ "collectibles": plate(7, "crimson") }), json!([])),
        json!({ "t": "VOICE_STATE_UPDATE", "d": {
            "guild_id": ALPHA.to_string(), "channel_id": "3000000000000000013", "user_id": "1000000000000000002",
            "member": { "user": { "id": "1000000000000000002", "username": "alice_fictive", "global_name": "Alice", "avatar": "a11ce0000000000000000000000000a1", "discriminator": "0" }, "nick": null, "roles": [], "joined_at": "2024-06-01T12:00:00.000000+00:00", "deaf": false, "mute": false, "flags": 0 },
            "session_id": "session-vocale-alice", "deaf": false, "mute": false, "self_deaf": false, "self_mute": false, "self_video": false, "self_stream": true, "suppress": false, "request_to_speak_timestamp": null
        } }),
    ];
    packets.iter().flat_map(packet).collect()
}

/// Scène `vocal-amis` (lot « vocal ») : Nox et Ewa rejoignent le salon vocal d'Alpha où
/// Alice partage déjà son écran ; bjørn joue avec une présence enrichie depuis cinq heures.
fn vocal_amis_events() -> Vec<Event> {
    use serde_json::json;
    let [nox, ewa, bjorn, _] = AMIS_FRIENDS;
    let voice = |(id, username, name): (u64, &str, &str)| {
        json!({ "t": "VOICE_STATE_UPDATE", "d": {
            "guild_id": ALPHA.to_string(), "channel_id": "3000000000000000014", "user_id": id.to_string(),
            "member": { "user": { "id": id.to_string(), "username": username, "global_name": name, "avatar": null, "discriminator": "0" }, "nick": null, "roles": [], "joined_at": "2024-06-01T12:00:00.000000+00:00", "deaf": false, "mute": false, "flags": 0 },
            "session_id": format!("session-vocale-{username}"), "deaf": false, "mute": false, "self_deaf": false, "self_mute": false, "self_video": false, "self_stream": false, "suppress": false, "request_to_speak_timestamp": null
        } })
    };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let started = now.saturating_sub((5 * 3600 + 7 * 60 + 2) * 1000);
    let playing = json!({ "t": "PRESENCE_UPDATE", "d": {
        "user": { "id": bjorn.0.to_string(), "username": bjorn.1, "global_name": bjorn.2, "discriminator": "0" }, "status": "online", "client_status": { "desktop": "online" },
        "activities": [{ "type": 0, "name": "Forge Lunaire", "application_id": "7000000000000004001", "details": "Exploration des Marais d'Ambre, troisième expédition", "state": "En solo", "created_at": started, "timestamps": { "start": started } }]
    } });
    [voice(nox), voice(ewa), playing].iter().flat_map(packet).collect()
}

/// Scènes `profil-edition*` (lot « profil rapide ») : le compte fictif a
/// Nitro (bannière, couleurs de thème et profil de serveur modifiables).
static NITRO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Couleurs de thème de mon profil fictif : 0 = foncées (violet et bleu-vert),
/// 1 = aucune (scène `mon-compte-sobre`), 2 = claires (`mon-compte-clair`).
static MY_THEME: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

fn name_seed(s: &str) -> u64 {
    s.bytes().fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(u64::from(b)))
}

/// Parures fictives du membre `n` de la liste : une sur deux en porte, à tour de rôle.
fn member_cosmetics(n: usize, user: &mut Value) {
    use serde_json::json;
    const PALETTES: [&str; 6] = ["cobalt", "crimson", "forest", "bubble_gum", "lemon", "violet"];
    const TAGS: [&str; 3] = ["RUST", "FR", "DEV"];
    const STYLES: [[u32; 2]; 3] = [[0xff7a59, 0x8a5cf0], [0x23a55a, 0x00a8fc], [0xf0b232, 0xf23f43]];
    match n % 6 {
        0 => user["avatar_decoration_data"] = json!({ "asset": format!("a_dec000000000000000000000000{:04x}", n), "sku_id": "7000000000000009001" }),
        1 => user["collectibles"] = json!({ "nameplate": { "sku_id": "7000000000000009002", "asset": format!("nameplates/fictives/plaque{n}/"), "label": "", "palette": PALETTES[(n / 6) % PALETTES.len()] } }),
        2 => user["primary_guild"] = json!({ "identity_guild_id": ALPHA.to_string(), "identity_enabled": true, "tag": TAGS[(n / 6) % 3], "badge": "7a9000000000000000000000000000b9" }),
        3 => user["display_name_styles"] = json!({ "font_id": 11, "effect_id": 2, "colors": STYLES[(n / 6) % 3] }),
        4 => {
            user["avatar_decoration_data"] = json!({ "asset": format!("dec100000000000000000000000000{:02x}", n), "sku_id": "7000000000000009003" });
            user["collectibles"] = json!({ "nameplate": { "sku_id": "7000000000000009002", "asset": format!("nameplates/fictives/plaque{n}/"), "label": "", "palette": PALETTES[(n / 6 + 3) % PALETTES.len()] } });
            user["primary_guild"] = json!({ "identity_guild_id": ALPHA.to_string(), "identity_enabled": true, "tag": "ÉTÉ", "badge": Value::Null });
        }
        _ => {}
    }
}

/// Décoration d'avatar générée : couronne de perles sur fond transparent ;
/// `turn` (0 à 1) les fait tourner (images de l'animation).
fn decoration_frame(seed: u64, side: u32, turn: f32) -> image::RgbaImage {
    let hue = [(250u8, 200u8, 80u8), (120, 220, 255), (255, 140, 200), (150, 255, 170)][(seed % 4) as usize];
    image::RgbaImage::from_fn(side, side, |x, y| {
        let (u, v) = (x as f32 / side as f32 - 0.5, y as f32 / side as f32 - 0.5);
        let (r, a) = ((u * u + v * v).sqrt(), v.atan2(u) / std::f32::consts::TAU + turn);
        let ring = (r - 0.455).abs() < 0.012;
        let pearl = (r - 0.455).abs() < 0.04 && ((a * 10.0).rem_euclid(1.0) - 0.5).abs() < 0.22;
        if pearl {
            image::Rgba([hue.0, hue.1, hue.2, 255])
        } else if ring {
            image::Rgba([255, 255, 255, 230])
        } else {
            image::Rgba([0, 0, 0, 0])
        }
    })
}

fn png(img: image::DynamicImage) -> Option<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

fn gif(frames: impl Iterator<Item = image::RgbaImage>, ms: u32) -> Option<Vec<u8>> {
    use image::{codecs::gif::{GifEncoder, Repeat}, Delay, Frame};
    let mut out = Vec::new();
    {
        let mut enc = GifEncoder::new_with_speed(&mut out, 30);
        enc.set_repeat(Repeat::Infinite).ok()?;
        enc.encode_frames(frames.map(|img| Frame::from_parts(img, 0, 0, Delay::from_numer_denom_ms(ms, 1)))).ok()?;
    }
    Some(out)
}

/// Bannière générée aux proportions de Discord (17:6) : dégradé, cadre clair
/// au bord et disque au centre, pour juger du recadrage (le disque doit
/// rester rond et centré, le cadre être rogné également des deux côtés).
/// `phase` (0 à 1) déplace une bande claire (images de l'animation).
fn banner_frame(seed: u64, w: u32, phase: Option<f32>) -> image::RgbaImage {
    let h = (w * 6 / 17).max(1);
    let hue = |shift: u32| -> [f32; 3] {
        let x = ((seed >> shift) % 360) as f32 / 60.0;
        let f = 1.0 - (x % 2.0 - 1.0).abs();
        match x as u32 {
            0 => [1.0, f, 0.0],
            1 => [f, 1.0, 0.0],
            2 => [0.0, 1.0, f],
            3 => [0.0, f, 1.0],
            4 => [f, 0.0, 1.0],
            _ => [1.0, 0.0, f],
        }
    };
    let (a, b) = (hue(7), hue(23));
    image::RgbaImage::from_fn(w, h, |x, y| {
        let (u, v) = (x as f32 / w as f32, y as f32 / h as f32);
        let k = (u + v) / 2.0;
        let frame = x < 4 || y < 4 || x + 4 >= w || y + 4 >= h;
        let disc = (((x as f32 - w as f32 / 2.0).powi(2) + (y as f32 - h as f32 / 2.0).powi(2)).sqrt()) < h as f32 * 0.3;
        let band = phase.is_some_and(|p| ((u - p).rem_euclid(1.0) - 0.5).abs() > 0.44);
        let px = |i: usize| {
            let c = (a[i] * (1.0 - k) + b[i] * k) * 0.7 + 0.1;
            ((if frame || disc || band { 0.4 + c * 0.6 } else { c }).clamp(0.0, 1.0) * 255.0) as u8
        };
        image::Rgba([px(0), px(1), px(2), 255])
    })
}

/// Images générées du lot P ; `None` : une autre sorte d'image.
fn generate_cosmetic(kind: &str, id: u64, size: &str) -> Option<Option<Vec<u8>>> {
    let side = size.parse::<u32>().ok().map(|s| s.clamp(1, 1024));
    let seed = id.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    Some(match kind {
        "decoration" => png(image::DynamicImage::ImageRgba8(decoration_frame(id, side?, 0.0))),
        "decoration-animee" => gif((0..10).map(|i| decoration_frame(id, side.unwrap_or(96), i as f32 / 100.0)), 80),
        // Plaque : taille fixe, motif rayé qui se fond vers la gauche.
        "plaque" => png(image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(448, 84, |x, y| {
            let u = x as f32 / 448.0;
            let stripe = ((x + y * 2) / 14) % 2 == 0;
            let alpha = ((u - 0.35).max(0.0) / 0.65 * if stripe { 150.0 } else { 60.0 }) as u8;
            let c = [(255u8, 255u8, 255u8), (255, 230, 150), (170, 220, 255)][(id % 3) as usize];
            image::Rgba([c.0, c.1, c.2, alpha])
        }))),
        "banniere" => png(image::DynamicImage::ImageRgba8(banner_frame(seed, side?, None))),
        "banniere-animee" => gif((0..12).map(|i| banner_frame(seed, side.unwrap_or(256), Some(i as f32 / 12.0))), 90),
        // Avatar animé : un disque qui bat sur un fond uni.
        "avatar-anime" => gif(
            (0..8u32).map(|i| {
                let s = side.unwrap_or(96);
                let r = 0.22 + 0.1 * (i as f32 / 8.0 * std::f32::consts::TAU).sin();
                image::RgbaImage::from_fn(s, s, |x, y| {
                    let (u, v) = (x as f32 / s as f32 - 0.5, y as f32 / s as f32 - 0.5);
                    if (u * u + v * v).sqrt() < r { image::Rgba([255, 235, 160, 255]) } else { image::Rgba([70, 90, 200, 255]) }
                })
            }),
            110,
        ),
        _ => return None,
    })
}

/// Scènes `effet` et `effet-fixe` (lot ND) : Alice porte un effet de profil.
static EFFET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Scène `profil-carte` (lot profil) : profil d'Alice paré comme la capture
/// de référence.
static CARTE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Scènes `panneau*` (lot profil) : mon propre compte porte des parures.
static PANNEAU: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn carte() -> bool {
    CARTE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Image d'un effet de profil généré : des gouttes claires qui tombent sur
/// fond transparent ; `phase` (0 à 1) les fait descendre.
fn effect_frame(seed: u64, w: u32, h: u32, phase: f32) -> image::RgbaImage {
    let mut img = image::RgbaImage::new(w, h);
    let tint = [(190u8, 225u8, 255u8), (255, 190, 120)][(seed % 2) as usize];
    for i in 0..48u64 {
        let r = (i + seed).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let x = (r >> 8) % u64::from(w);
        let y = (((r >> 24) % 1000) as f32 / 1000.0 + phase).rem_euclid(1.0) * h as f32;
        let len = 10 + (r >> 40) % 22;
        for d in 0..len {
            let yy = y as i64 - d as i64;
            if yy >= 0 && (yy as u32) < h {
                let alpha = (220 - d * 220 / len) as u8;
                for dx in 0..2u32 {
                    if (x as u32 + dx) < w {
                        img.put_pixel(x as u32 + dx, yy as u32, image::Rgba([tint.0, tint.1, tint.2, alpha]));
                    }
                }
            }
        }
    }
    img
}

/// APNG (les effets de profil de Discord en sont) : `frames` images de `ms` millisecondes.
fn apng(w: u32, h: u32, frames: u32, ms: u16, frame: impl Fn(u32) -> image::RgbaImage) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        enc.set_animated(frames, 0).ok()?;
        enc.set_frame_delay(ms, 1000).ok()?;
        let mut writer = enc.write_header().ok()?;
        for i in 0..frames {
            writer.write_image_data(frame(i).as_raw()).ok()?;
        }
        writer.finish().ok()?;
    }
    Some(out)
}

/// Fichiers du CDN demandés tels quels (lot ND) : couches et vignettes
/// d'effets de profil, images de quêtes.
fn generate_asset(path: &str) -> Option<Vec<u8>> {
    let seed = name_seed(path);
    if path.starts_with("profile_effects/") {
        let tint = u64::from(path.contains("braises"));
        return if path.ends_with("apercu.png") {
            // Vignette : fond sombre, pour que l'effet se lise seul.
            let mut img = image::RgbaImage::from_pixel(92, 180, image::Rgba([30, 32, 40, 255]));
            image::imageops::overlay(&mut img, &effect_frame(tint, 92, 180, 0.3), 0, 0);
            png(image::DynamicImage::ImageRgba8(img))
        } else if path.ends_with("reduit.png") {
            png(image::DynamicImage::ImageRgba8(effect_frame(tint, 450, 880, 0.3)))
        } else {
            apng(450, 880, 12, 80, |i| effect_frame(tint, 450, 880, i as f32 / 12.0))
        };
    }
    // Image de quête. La troisième quête du jeu d'essai ne répond que sous
    // l'autre racine du CDN : l'aperçu montre le repli d'une adresse à la suivante.
    if path.starts_with("quests/8000000000000000003/") {
        return None;
    }
    let file = path.rsplit('/').next().unwrap_or(path);
    let stem = file.split('.').next().unwrap_or(file);
    let last = stem.bytes().last().filter(|_| stem.bytes().all(|b| b.is_ascii_digit())).map(|b| b - b'0');
    let (a, b) = ((seed >> 5) as u8, (seed >> 17) as u8);
    let mix = |p: u8, q: u8, t: f32| (f32::from(p) * (1.0 - t) + f32::from(q) * t) as u8;
    if file.contains("hero") || file.contains("questbar") || matches!(last, Some(1 | 2)) {
        // Image d'ambiance : ciel en dégradé, astre, deux lignes de crêtes.
        let (w, h) = (960u32, 540u32);
        let sun = ((seed >> 9) % 600 + 180) as f32;
        return png(image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(w, h, |x, y| {
            let (fx, fy) = (x as f32, y as f32);
            let t = fy / h as f32;
            let mut c = [mix(a / 2 + 40, 250 - b / 3, t * 0.7), mix(b / 2 + 30, 150 + a / 4, t), mix(200 - a / 2, a / 2 + 60, t)];
            let d = ((fx - sun).powi(2) + (fy - 150.0).powi(2)).sqrt();
            if d < 150.0 {
                let glow = (1.0 - d / 150.0).powi(2) * if d < 46.0 { 1.0 } else { 0.5 };
                c = [mix(c[0], 255, glow), mix(c[1], 240, glow), mix(c[2], 200, glow)];
            }
            let far = 300.0 + 40.0 * (fx / 70.0 + a as f32).sin() + 22.0 * (fx / 23.0).sin();
            let near = 400.0 + 55.0 * (fx / 110.0 + b as f32).sin() + 14.0 * (fx / 17.0).cos();
            if fy > near {
                c = [18, 20, 30];
            } else if fy > far {
                c = [mix(c[0], 40, 0.7), mix(c[1], 44, 0.7), mix(c[2], 70, 0.7)];
            }
            image::Rgb(c)
        })));
    }
    if file.contains("wordmark") || matches!(last, Some(7..=9)) {
        // Logotype : blocs de « lettres » sur fond transparent, clairs (fond
        // sombre) ou foncés (version pour fond clair).
        let dark_ink = path.contains("light/") || file.contains("clair") || last == Some(8);
        let ink = if dark_ink { image::Rgba([24, 26, 34, 255]) } else { image::Rgba([255, 255, 255, 255]) };
        let (w, h) = (360u32, 96u32);
        let letters = 5 + (seed % 3) as u32;
        return png(image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(w, h, |x, y| {
            let cell = w / letters;
            let (i, cx) = (x / cell, x % cell);
            let shape = (seed >> (i * 3)) % 4;
            let inside = cx > 6 && cx < cell - 6 && y > 10 && y < h - 10;
            let hole = match shape {
                0 => cx > 18 && cx < cell - 18 && y > 26 && y < h - 26,
                1 => cx > 18 && y > 26 && y < 44,
                2 => cx > 18 && cx < cell - 18 && y > 26,
                _ => cx > 18 && y < h - 30 && y > 40,
            };
            if inside && !hole { ink } else { image::Rgba([0, 0, 0, 0]) }
        })));
    }
    // Vignette (jeu, récompense) : fond en dégradé, losange clair au centre.
    let (w, h) = (256u32, 256u32);
    png(image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(w, h, |x, y| {
        let t = (x + y) as f32 / (w + h) as f32;
        let (u, v) = ((x as f32 / w as f32 - 0.5).abs(), (y as f32 / h as f32 - 0.5).abs());
        if u + v < 0.26 {
            image::Rgb([mix(255, a, 0.25), mix(240, b, 0.25), mix(250, a, 0.1)])
        } else if u + v < 0.31 {
            image::Rgb([30, 32, 44])
        } else {
            image::Rgb([mix(a, 40, t), mix(90, b, t), mix(b, a, t) / 2 + 60])
        }
    })))
}

fn generate(url: &str) -> Option<Vec<u8>> {
    if let Some(path) = url.strip_prefix("https://cdn.discordapp.com/assets/") {
        return generate_asset(path);
    }
    // Autre racine des images de quêtes (`/quests/…`).
    if let Some(path) = url.strip_prefix("https://cdn.discordapp.com/quests/") {
        return generate_asset(&format!("quests/_/{path}"));
    }
    let mut parts = url.strip_prefix("apercu://")?.split('/');
    let (kind, id, size) = (parts.next()?, parts.next()?.parse::<u64>().ok()?, parts.next()?);
    if kind == "emoji-anime" {
        return generate_anim(id, size.parse::<u32>().ok()?.clamp(1, 512));
    }
    if let Some(done) = generate_cosmetic(kind, id, size) {
        return done;
    }
    let (w, h) = match size.split_once('x') {
        Some((w, h)) => (w.parse::<u32>().ok()?.clamp(1, 4096), h.parse::<u32>().ok()?.clamp(1, 4096)),
        None => (size.parse::<u32>().ok()?, size.parse::<u32>().ok()?),
    };
    let seed = id.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let hue = |shift: u32| -> [f32; 3] {
        let h = ((seed >> shift) % 360) as f32 / 60.0;
        let x = 1.0 - (h % 2.0 - 1.0).abs();
        match h as u32 {
            0 => [1.0, x, 0.0],
            1 => [x, 1.0, 0.0],
            2 => [0.0, 1.0, x],
            3 => [0.0, x, 1.0],
            4 => [x, 0.0, 1.0],
            _ => [1.0, 0.0, x],
        }
    };
    let (a, b) = (hue(7), hue(23));
    // Bannière : proportions de Discord (5:2).
    // Couverture d'événement : 16:9 (lot A7).
    let h = match kind {
        "banniere" => (w * 2 / 5).max(1),
        "couverture" => (w * 9 / 16).max(1),
        _ => h,
    };
    let (sw, sh) = (w as f32, h as f32);
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        let (u, v) = (x as f32 / sw, y as f32 / sh);
        let t = (u + v) / 2.0;
        let mut c = [0.0f32; 3];
        for i in 0..3 {
            c[i] = (a[i] * (1.0 - t) + b[i] * t) * 0.75 + 0.1;
        }
        let light = match kind {
            // Silhouette : tête et épaules.
            "avatar" => {
                let head = ((u - 0.5).powi(2) + (v - 0.4).powi(2)).sqrt() < 0.18;
                let body = ((u - 0.5).powi(2) / 0.1 + (v - 1.0).powi(2) / 0.12) < 1.0;
                head || body
            }
            // Emoji : un disque.
            "emoji" => ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt() < 0.4,
            // Losange.
            _ => (u - 0.5).abs() + (v - 0.5).abs() < 0.3,
        };
        let px = |c: f32| ((if light { 0.35 + c * 0.65 } else { c }).clamp(0.0, 1.0) * 255.0) as u8;
        image::Rgb([px(c[0]), px(c[1]), px(c[2])])
    });
    let format = match id % 3 {
        0 => image::ImageFormat::Png,
        1 => image::ImageFormat::Jpeg,
        _ => image::ImageFormat::WebP,
    };
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img).write_to(&mut out, format).ok()?;
    Some(out.into_inner())
}

/// Emoji animé de l'aperçu : un disque qui fait le tour de l'image, huit
/// images de 120 ms.
fn generate_anim(id: u64, side: u32) -> Option<Vec<u8>> {
    use image::{codecs::gif::GifEncoder, Delay, Frame, Rgba, RgbaImage};
    let hue = (id % 6) as u8;
    let frames = (0..8u32).map(|i| {
        let angle = i as f32 / 8.0 * std::f32::consts::TAU;
        let (cx, cy) = (0.5 + 0.25 * angle.cos(), 0.5 + 0.25 * angle.sin());
        let img = RgbaImage::from_fn(side, side, |x, y| {
            let (u, v) = (x as f32 / side as f32, y as f32 / side as f32);
            if ((u - cx).powi(2) + (v - cy).powi(2)).sqrt() < 0.2 { Rgba([250, 200 - hue * 20, 60 + hue * 30, 255]) } else { Rgba([0, 0, 0, 0]) }
        });
        Frame::from_parts(img, 0, 0, Delay::from_numer_denom_ms(120, 1))
    });
    let mut out = Vec::new();
    GifEncoder::new(&mut out).encode_frames(frames).ok()?;
    Some(out)
}

const ALPHA: u64 = 2_000_000_000_000_000_001;
const GENERAL: u64 = 3_000_000_000_000_000_011;
const ANNONCES: u64 = 3_000_000_000_000_000_013;
const HORS_SUJET: u64 = 3_000_000_000_000_000_012;
const DISCUSSIONS: u64 = 3_000_000_000_000_000_010;
const MODERATION: u64 = 3_000_000_000_000_000_016;
const ANCIEN: u64 = 3_000_000_000_000_000_021;
const GAMMA_SALON: u64 = 3_000_000_000_000_000_041;
const GAMMA: u64 = 2_000_000_000_000_000_003;
const DM: u64 = 4_000_000_000_000_000_001;
const DM_GROUP: u64 = 4_000_000_000_000_000_002;

fn fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path} : {e}"))
}

/// Tables des utilisateurs, membres et présences de l'aperçu : communes à
/// tous les paquets décodés, remises au magasin par `Event::PeopleHub`.
fn people() -> PeopleHub {
    static PEOPLE: OnceLock<PeopleHub> = OnceLock::new();
    PEOPLE.get_or_init(PeopleHub::default).clone()
}

/// Événements d'un paquet de passerelle isolé, éventuellement retouché.
fn packet(v: &Value) -> Vec<Event> {
    Decoder::with_people(people()).decode(v["t"].as_str().unwrap_or_default(), &v["d"].to_string())
}

/// Présence d'Alice portant l'utilisateur complet (`rest_user`).
fn alice_presence() -> Value {
    let mut alice = fixture("presence_update");
    alice["d"]["user"] = fixture("rest_user");
    alice
}

/// Activité (lot C3) : Alice écoute Spotify (1:15 sur 3:30) et joue depuis
/// 47 min (images, boutons) ; de mon côté, liste des jeux chargée et un jeu
/// en cours.
fn activity_store(s: &mut Store) {
    use ferrocord::state::activity::{ActivityEvent, GameIndex, RunningGame};
    let now = chrono::Utc::now().timestamp_millis();
    let mut p = fixture("presence_update_rich");
    p["d"]["user"] = fixture("rest_user");
    if SANS_BANNIERE.load(std::sync::atomic::Ordering::Relaxed) {
        p["d"]["user"]["banner"] = serde_json::Value::Null;
    }
    p["d"]["activities"][0]["timestamps"] = serde_json::json!({ "start": (now - 75_000).to_string(), "end": (now + 135_000).to_string() });
    p["d"]["activities"][1]["timestamps"] = serde_json::json!({ "start": now - 47 * 60_000 });
    for e in packet(&p) {
        s.apply(e);
    }
    let apps = ferrocord::net::decode_detectable_list(&fixture("rest_games_detectable").to_string()).unwrap_or_default();
    s.apply(Event::Activity(ActivityEvent::Detectable(std::sync::Arc::new(GameIndex::new(apps, now)))));
    s.apply(Event::Activity(ActivityEvent::Running(vec![RunningGame { app_id: Some(6_000_000_000_000_000_020), name: "PLAYERUNKNOWN'S BATTLEGROUNDS".into(), start_ms: now - 12 * 60_000 }])));
    // Rich Presence locale (lot R) : tube 1 (le 0 au client officiel), deux applications branchées.
    use ferrocord::state::rpc::{RpcActivity, RpcEvent};
    let rpc = |e| Event::Activity(ActivityEvent::Rpc(e));
    s.apply(rpc(RpcEvent::Listening(Some(1))));
    s.apply(rpc(RpcEvent::Connected { conn: 1, app_id: 6_000_000_000_000_000_040, since_ms: now - 25 * 60_000 }));
    s.apply(rpc(RpcEvent::Connected { conn: 2, app_id: 6_000_000_000_000_000_041, since_ms: now - 3 * 60_000 }));
    let act = RpcActivity { name: "Donjon des Brumes".into(), details: Some("Étage 4".into()), state: Some("En groupe".into()), start_ms: Some(now - 25 * 60_000), party_size: Some((2, 4)), ..Default::default() };
    s.apply(rpc(RpcEvent::Activity { conn: 1, process: Some("Donjon".into()), activity: Some(Box::new(act)) }));
    s.apply(rpc(RpcEvent::Activity { conn: 2, process: Some("Lecteur".into()), activity: None }));
}

/// Lot « activité » : un jeu fictif de la liste, en cours depuis douze minutes.
fn detected_game_store(s: &mut Store) {
    use ferrocord::state::activity::{ActivityEvent, DetectableApp, GameIndex, RunningGame};
    const ASTRAL: u64 = 6_000_000_000_000_000_060;
    let now = chrono::Utc::now().timestamp_millis();
    let icon = ferrocord::state::model::ImageHash::parse("a57a1d21f7000000000000000000c0de");
    let apps = vec![DetectableApp { id: ASTRAL, name: "Astral Drift".into(), exes: vec!["astraldrift.exe".into()], icon }];
    s.apply(Event::Activity(ActivityEvent::Detectable(std::sync::Arc::new(GameIndex::new(apps, now)))));
    s.apply(Event::Activity(ActivityEvent::Running(vec![RunningGame { app_id: Some(ASTRAL), name: "Astral Drift".into(), start_ms: now - 12 * 60_000 - 20_000 }])));
}

/// Profils et présences : Alice complète (bannière, couleur, badges, rôles,
/// boost, statut personnalisé, activités), statuts variés sur les messages,
/// dans la liste des MP et dans le pied.
fn profile_events() -> Vec<Event> {
    let mut out = Vec::new();
    let mut alice = alice_presence();
    if carte() {
        // En ligne depuis un téléphone seulement : pastille et badge « mobile ».
        alice["d"]["client_status"] = serde_json::json!({ "mobile": "online" });
        alice["d"]["activities"][0]["state"] = "Entre deux parties, je reviens vite. Laissez un message !".into();
    }
    if parures() {
        // Parures d'Alice (jeu `rest_user_cosmetics`) : avatar et bannière animés, décoration, plaque, tag, style du nom.
        let dressed = fixture("rest_user_cosmetics");
        for key in ["avatar", "banner", "avatar_decoration_data", "collectibles", "primary_guild", "display_name_styles"] {
            alice["d"]["user"][key] = dressed["user"][key].clone();
        }
    }
    out.extend(packet(&alice));
    for (id, status) in [(ME, "online"), (1_000_000_000_000_000_003, "idle"), (1_000_000_000_000_000_004, "dnd"), (1_000_000_000_000_000_005, "offline")] {
        let mut p = serde_json::json!({ "t": "PRESENCE_UPDATE", "d": { "user": { "id": id.to_string() }, "status": status, "client_status": { "desktop": status }, "activities": [] } });
        if id == ME && PANNEAU.load(std::sync::atomic::Ordering::Relaxed) {
            // Mes parures (jeu `rest_user_cosmetics`) : décoration, plaque, style du nom.
            let dressed = fixture("rest_user_cosmetics");
            p["d"]["user"]["username"] = "moi.fictif".into();
            p["d"]["user"]["global_name"] = "Moi Fictif".into();
            for key in ["avatar_decoration_data", "collectibles", "display_name_styles"] {
                p["d"]["user"][key] = dressed["user"][key].clone();
            }
        }
        out.extend(packet(&p));
    }
    let mut role = fixture("guild_role_create");
    role["d"]["role"]["id"] = "2000000000000001001".into();
    role["d"]["role"]["name"] = "Modération".into();
    role["d"]["role"]["color"] = 15158332.into();
    role["d"]["role"]["position"] = 4.into();
    if parures() {
        // Rôle holographique (valeurs imposées par Discord, Role Colors Object).
        role["d"]["role"]["color"] = 11127295.into();
        role["d"]["role"]["colors"] = serde_json::json!({ "primary_color": 11127295, "secondary_color": 16759788, "tertiary_color": 16761760 });
    }
    out.extend(packet(&role));
    out.extend(packet(&fixture("guild_role_create")));
    let mut member = serde_json::json!({ "t": "GUILD_MEMBER_UPDATE", "d": fixture("rest_guild_member") });
    member["d"]["guild_id"] = ALPHA.to_string().into();
    member["d"]["roles"] = serde_json::json!(["2000000000000001001", "2000000000000001003"]);
    out.extend(packet(&member));
    out
}

/// Réponses fictives aux demandes de la fenêtre Paramètres (aucun réseau).
fn account_reply(c: ferrocord::state::usersettings::AccountCmd) -> Vec<Event> {
    use ferrocord::state::usersettings::{AccountCmd as C, AccountEvent as E, AuthSession, AuthorizedApp, LinkedAccount, Profile, ProtoReply};
    let ev = match c {
        C::LoadProto => E::Proto { bytes: ferrocord::state::usersettings::b64::decode(fixture("rest_settings_proto")["settings"].as_str().unwrap_or_default()), partial: false },
        C::PatchProto { id, .. } => E::ProtoWritten { id, result: Ok(ProtoReply { bytes: None, out_of_date: false }) },
        C::PatchGuild { id, guild, .. } => E::GuildWritten { id, guild, result: Ok(None) },
        C::LoadProfile => E::Profile(Ok(Profile {
            id: 1_000_000_000_000_000_001,
            username: "moi.fictif".into(),
            global_name: Some("Moi Fictif".into()),
            email: Some("moi@example.invalid".into()),
            phone: Some("+33100000000".into()),
            verified: true,
            mfa: true,
            premium: if NITRO.load(std::sync::atomic::Ordering::Relaxed) { 2 } else { 0 },
            bio: "Profil d'aperçu, données fictives.".into(),
            pronouns: "il".into(),
            accent: Some(0x5865f2),
        })),
        C::PatchProfile { id, .. } => E::ProfileWritten { id, result: Ok(()) },
        C::SetNick { guild, .. } => E::NickWritten { guild, result: Ok(()) },
        C::PatchGuildProfile { id, guild, .. } => E::GuildProfileWritten { id, guild, result: Ok(()) },
        C::LoadSessions => E::Sessions(Ok(vec![
            AuthSession { id_hash: "a".into(), last_used_ms: Some(1_763_000_000_000), os: Some("Windows".into()), platform: Some("Ferrocord".into()), location: Some("Paris, France".into()) },
            AuthSession { id_hash: "b".into(), last_used_ms: Some(1_762_000_000_000), os: Some("Android".into()), platform: Some("Discord Android".into()), location: Some("Lyon, France".into()) },
        ])),
        C::LogoutSession(hash) => E::SessionLoggedOut { hash, result: Ok(()) },
        C::LoadConnections => E::Connections(Ok(vec![
            LinkedAccount { kind: "github".into(), name: "compte-fictif".into(), verified: true, visible: true, revoked: false },
            LinkedAccount { kind: "steam".into(), name: "Joueur Fictif".into(), verified: true, visible: false, revoked: false },
        ])),
        C::LoadApps => E::Apps(Ok(vec![AuthorizedApp { id: 7_000_000_000_000_000_001, name: "Robot Fictif".into(), scopes: vec!["identify".into(), "guilds".into()] }])),
        C::RevokeApp(id) => E::AppRevoked { id, result: Ok(()) },
        C::Extra(x) => {
            use ferrocord::state::usersettings::extra::{Consents, ExtraCmd, ExtraEvent};
            E::Extra(match x {
                ExtraCmd::LoadConsents => ExtraEvent::Consents(Ok(Consents { usage_statistics: Some(false), personalization: Some(true) })),
                ExtraCmd::SetConsent { kind, .. } => ExtraEvent::ConsentWritten { kind, result: Ok(Consents::default()) },
                ExtraCmd::SetImage { target, data } => ExtraEvent::ImageWritten { target, removed: data.is_none(), result: Ok(()) },
            })
        }
    };
    vec![Event::Account(ev)]
}

/// Réponses fictives de la fenêtre « Paramètres du serveur » (aucun réseau) :
/// lectures servies par les jeux d'essai ; les écritures restent sans réponse.
fn admin_reply(c: &ferrocord::state::admin::guild::AdminCmd) -> Option<ferrocord::state::admin::guild::AdminEvent> {
    use ferrocord::state::admin::guild::AdminCmd as C;
    let body = match c {
        C::Load { .. } => fixture("rest_guild_get").to_string(),
        C::Vanity { .. } => r#"{ "code": "alpha", "uses": 42 }"#.to_string(),
        C::Webhooks { .. } => fixture("rest_guild_webhooks").to_string(),
        C::Integrations { .. } => fixture("rest_guild_integrations").to_string(),
        C::CommandPerms { .. } => fixture("rest_command_permissions").to_string(),
        // Widget, modèle, élagage (lot V) : lectures seulement.
        C::Extra(ferrocord::state::admin::extras::ExtraCmd::Widget { .. }) => fixture("rest_guild_widget").to_string(),
        C::Extra(ferrocord::state::admin::extras::ExtraCmd::Templates { .. }) => fixture("rest_guild_templates").to_string(),
        C::Extra(ferrocord::state::admin::extras::ExtraCmd::PruneCount { .. }) => r#"{ "pruned": 12 }"#.to_string(),
        _ => return None,
    };
    ferrocord::net::decode_admin(c, &body)
}

/// Réponses fictives de la modération (lots A4 + A5) : les jeux d'essai
/// `rest_*` passés par les vrais décodeurs ; écritures acceptées.
fn moderation_reply(c: ferrocord::state::admin::moderation::ModCmd) -> Vec<Event> {
    use ferrocord::state::admin::automod::Execution;
    use ferrocord::state::admin::moderation::{ModCmd as C, ModEvent as E};
    let body = |name: &str| serde_json::to_string(&fixture(name)).unwrap_or_default();
    let ev = match c {
        C::Kick { guild_id, user_id, .. } => E::Kicked { guild_id, user_id },
        C::Ban { guild_id, user_id, .. } => E::Banned { guild_id, user_id },
        C::Unban { guild_id, user_id, .. } => E::Unbanned { guild_id, user_id },
        C::Timeout { guild_id, user_id, until_ms, .. } => E::TimedOut { guild_id, user_id, until_ms },
        C::Bans { guild_id, after } => E::Bans { guild_id, after, bans: if after.is_none() { ferrocord::net::decode_bans(&body("rest_guild_bans")).unwrap_or_default() } else { Vec::new() } },
        C::BulkBan { guild_id, user_ids, .. } => E::BulkBanned { guild_id, banned: user_ids, failed: Vec::new() },
        C::BulkDelete { channel_id, ids, .. } => E::BulkDeleted { channel_id, deleted: ids.len(), failed: 0 },
        C::Audit { guild_id, filter, before } => E::Audit { guild_id, filter, before, entries: if before.is_none() { ferrocord::net::decode_audit(&body("rest_audit_log")).unwrap_or_default() } else { Vec::new() } },
        C::Rules { guild_id } => {
            let rules = ferrocord::net::decode_rules(&body("rest_automod_rules")).unwrap_or_default();
            let alert = Execution { guild_id, rule_id: rules.first().map_or(0, |r| r.id), trigger: 1, action: 1, user_id: 1_000_000_000_000_000_003, channel_id: Some(GENERAL), content: "achète des crédits ici".into(), matched: "crédits".into() };
            return vec![Event::Moderation(E::Rules { guild_id, rules }), Event::Moderation(E::Execution(alert))];
        }
        C::SaveRule { guild_id, mut rule } => {
            if rule.id == 0 {
                rule.id = 6_000_000_000_000_000_999;
            }
            E::RuleSaved { guild_id, rule, mine: true }
        }
        C::DeleteRule { guild_id, rule_id } => E::RuleDeleted { guild_id, rule_id },
        C::Safety { guild_id } | C::SetSafety { guild_id, .. } | C::Incidents { guild_id, .. } => {
            let mut safety = ferrocord::net::decode_safety(&body("rest_guild_safety")).unwrap_or_default();
            // Pause du jeu d'essai (2025) déjà passée : une pause en cours, pour la montrer.
            safety.invites_disabled_until = Some(chrono_now_ms() + 3 * 3_600_000);
            E::Safety { guild_id, safety, mine: true }
        }
    };
    vec![Event::Moderation(ev)]
}

/// Heure de la machine (ms Unix), pour les dates fictives de l'aperçu.
fn chrono_now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0))
}

/// Serveur communautaire du lot A7 et lieux (salons vocaux) d'Alpha.
const COMMUNITY_GUILD: u64 = 2_000_000_000_000_000_501;

/// Événements, serveur communautaire, salons vocaux d'Alpha (lot A7).
fn admin_store(s: &mut Store) {
    use ferrocord::state::admin::community::{CommunityEvent, Venue};
    for e in packet(&fixture("guild_create_community")) {
        s.apply(e);
    }
    for (id, name, stage) in [(3_000_000_000_000_000_901, "Salon vocal", false), (3_000_000_000_000_000_902, "Scène", true)] {
        s.apply(Event::Community(CommunityEvent::VenueUpsert { guild_id: ALPHA, venue: Venue { id, name: name.into(), stage, position: 10 } }));
    }
    for e in packet(&fixture("guild_scheduled_event_create")) {
        s.apply(e);
    }
}

/// Serveur « Club de conférences » du lot V6 et ses salons vocaux.
const CLUB: u64 = 2_000_000_000_000_000_601;
const CLUB_SCENE: u64 = 3_000_000_000_000_000_601;
const CLUB_VOCAL: u64 = 3_000_000_000_000_000_602;

/// Appels, conférences, sons (lot V6) : jeux d'essai passés par le vrai décodeur.
fn calls_store(s: &mut Store, scene: Option<Scene>) {
    use ferrocord::state::voice::{VoiceEvent, VoiceState};
    let me = |channel: u64, suppress: bool| Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(CLUB), user_id: ME, state: Some(VoiceState { guild_id: Some(CLUB), channel_id: channel, user_id: ME, suppress, ..Default::default() }), session_id: None });
    match scene {
        Some(Scene::Sonnerie) => {
            for e in packet(&fixture("call_create")) {
                s.apply(e);
            }
        }
        Some(Scene::Conference) => {
            for e in packet(&fixture("guild_create_stage")) {
                s.apply(e);
            }
            s.apply(me(CLUB_SCENE, true));
        }
        Some(Scene::Sons) => {
            for e in packet(&fixture("guild_create_stage")) {
                s.apply(e);
            }
            s.apply(me(CLUB_VOCAL, false));
            for e in packet(&fixture("voice_channel_effect_send")) {
                s.apply(e);
            }
        }
        _ => {}
    }
}

/// Scènes `appel-mp`, `appel-sortant`, `appel-groupe` et `appel-mp-partage`
/// (lot « appel ») : l'appel d'un MP en bandeau au-dessus de la conversation.
/// À deux avec Alice qui parle ; Alice qui n'a pas encore décroché ; groupe à
/// quatre (Bob micro coupé, Ève qui parle, Alice qui sonne) ; Alice qui
/// partage son écran (mire). L'historique porte des messages d'appel.
fn dm_call_store(s: &mut Store, scene: Scene) {
    use ferrocord::state::calls::{Call, CallEvent};
    use ferrocord::state::voice::{VoiceEvent, VoiceState};
    const ALICE: u64 = 1_000_000_000_000_000_002;
    const BOB: u64 = 1_000_000_000_000_000_003;
    const EVE_MP: u64 = 1_000_000_000_000_000_004;
    let now = chrono::Utc::now().timestamp_millis();
    let flake = |ms: i64| (u64::try_from(ms - 1_420_070_400_000).unwrap_or(0)) << 22;
    let channel = if scene == Scene::AppelGroupe { DM_GROUP } else { DM };
    let state = |user_id: u64| VoiceState { channel_id: channel, user_id, ..Default::default() };
    let (present, ringing, speaker) = match scene {
        Scene::AppelSortant => (vec![state(ME)], vec![ALICE], None),
        Scene::AppelGroupe => (vec![state(ME), VoiceState { self_mute: true, ..state(BOB) }, state(EVE_MP)], vec![ALICE], Some(EVE_MP)),
        Scene::AppelMpPartage => (vec![state(ME), VoiceState { self_stream: true, ..state(ALICE) }], Vec::new(), None),
        _ => (vec![state(ME), state(ALICE)], Vec::new(), Some(ALICE)),
    };
    // Historique : appels passés (durées en mots, appel manqué), puis celui en cours.
    let started = now - 95_000;
    let me = serde_json::json!({ "id": ME.to_string(), "username": "moi.fictif", "global_name": "Moi Fictif", "discriminator": "0", "avatar": null, "public_flags": 0 });
    let alice = fixture("message_create_dm")["d"]["author"].clone();
    let iso = |ms: i64| chrono::DateTime::from_timestamp_millis(ms).map(|d| d.to_rfc3339()).unwrap_or_default();
    let call = |author: &Value, start: i64, lasted: Option<i64>, joined: bool| {
        let mut people = vec![author["id"].clone()];
        if joined {
            people.push(serde_json::json!(ME.to_string()));
        }
        serde_json::json!({
            "id": flake(start).to_string(), "channel_id": channel.to_string(), "author": author, "content": "", "timestamp": iso(start),
            "edited_timestamp": null, "tts": false, "mention_everyone": false, "mentions": [], "mention_roles": [], "attachments": [], "embeds": [],
            "components": [], "pinned": false, "type": 3, "flags": 0,
            "call": { "participants": people, "ended_timestamp": lasted.map(|d| iso(start + d)) }
        })
    };
    const MIN: i64 = 60_000;
    let text = serde_json::json!({
        "id": flake(now - 20 * MIN).to_string(), "channel_id": channel.to_string(), "author": alice, "content": "On s'appelle pour relire le plan ?", "timestamp": iso(now - 20 * MIN),
        "edited_timestamp": null, "tts": false, "mention_everyone": false, "mentions": [], "mention_roles": [], "attachments": [], "embeds": [],
        "components": [], "pinned": false, "type": 0, "flags": 0
    });
    // Du plus récent au plus ancien, comme la réponse de Discord.
    let history = vec![
        call(&me, started, None, true),
        call(&alice, now - 12 * MIN, Some(4 * MIN + 20_000), true),
        call(&alice, now - 16 * MIN, Some(25_000), false),
        text,
        call(&me, now - 3 * 60 * MIN, Some(75_000), true),
        call(&alice, now - 26 * 60 * MIN, Some(125 * MIN), true),
        call(&me, now - 27 * 60 * MIN, Some(8_000), true),
    ];
    let messages = decode_history(&Value::Array(history).to_string()).unwrap_or_default();
    s.select_dms();
    s.select_channel(channel);
    s.apply(Event::History { channel_id: channel, before: None, messages });
    s.voice_join_call(channel);
    s.apply(Event::Calls(CallEvent::Upsert { call: Call { channel_id: channel, message_id: Some(flake(started)), region: None, ringing, unavailable: false }, voice_states: Some(present) }));
    s.apply(Event::Voice(VoiceEvent::Connected));
    if let Some(user_id) = speaker {
        s.apply(Event::Voice(VoiceEvent::Speaking { user_id, speaking: true }));
    }
    if scene == Scene::AppelMpPartage {
        use ferrocord::state::stream::{stream_key, StreamEvent};
        let key = stream_key(None, DM, ALICE);
        s.video_watch(&key);
        s.apply(Event::Stream(StreamEvent::Create { key: key.clone(), rtc_server_id: None, rtc_channel_id: None, region: None, viewers: vec![ME], paused: false }));
        s.apply(Event::Stream(StreamEvent::Connected { key }));
    }
    s.take_commands();
}

/// Réponses fictives du lot V6 : listes des jeux d'essai, actions acceptées.
fn calls_reply(c: Cmd) -> Vec<Event> {
    use ferrocord::state::admin::expressions::{ExprCmd, Kind};
    use ferrocord::state::calls::{CallCmd, CallEvent};
    use ferrocord::state::stage::StageEvent;
    match c {
        Cmd::Calls(c) => {
            let (action, key) = c.action();
            let mut v = Vec::new();
            match c {
                CallCmd::DefaultSounds => {
                    let list = ferrocord::net::decode_default_sounds(&fixture("rest_soundboard_default_sounds").to_string()).unwrap_or_default();
                    v.push(Event::Calls(CallEvent::DefaultSounds(list)));
                }
                CallCmd::GuildSounds { guild_id } => {
                    let cmd = ExprCmd::List { guild_id, kind: Kind::Sound };
                    v.extend(ferrocord::net::decode_expressions(&cmd, &fixture("rest_guild_soundboard_sounds").to_string()).map(Event::Expressions));
                }
                _ => {}
            }
            v.push(Event::Calls(CallEvent::Done { action, key }));
            v
        }
        Cmd::Stage(c) => vec![Event::Stage(StageEvent::Done { busy: c.busy() })],
        _ => Vec::new(),
    }
}

/// Réponses fictives aux événements programmés (aucun réseau).
fn events_reply(c: ferrocord::state::admin::events::EventCmd) -> ferrocord::state::admin::events::EventEvent {
    use ferrocord::state::admin::events::{EventCmd as C, EventEvent as E, Failure};
    match c {
        C::Load { guild_id } => {
            let events = ferrocord::net::decode_scheduled_events(&fixture("rest_guild_scheduled_events").to_string()).into_iter().map(|mut e| {
                e.guild_id = guild_id;
                e
            });
            E::Loaded { guild_id, events: events.collect(), mine: Some(vec![5_000_000_000_000_000_001]) }
        }
        C::Interest { guild_id, event_id, on } => E::Interest { guild_id, event_id, on },
        // Ni création ni modification dans l'aperçu.
        other => {
            let (action, key) = other.action();
            E::Failed { action, key, failure: Failure::Problem(ferrocord::state::Problem::MfaRequired) }
        }
    }
}

/// Réponses fictives à l'accueil des nouveaux membres (aucun réseau).
fn community_reply(c: ferrocord::state::admin::community::CommunityCmd) -> ferrocord::state::admin::community::CommunityEvent {
    use ferrocord::state::admin::community::{CommunityCmd as C, CommunityEvent as E};
    use ferrocord::state::admin::events::Failure;
    match c {
        C::LoadWelcome { guild_id } => match ferrocord::net::decode_welcome(&fixture("rest_welcome_screen").to_string()) {
            Some(screen) => E::Welcome { guild_id, screen },
            None => E::Failed { action: ferrocord::state::admin::community::CommunityAction::Welcome, guild_id, failure: Failure::NotFound },
        },
        C::LoadOnboarding { guild_id } => match ferrocord::net::decode_onboarding(&fixture("rest_guild_onboarding").to_string()) {
            Some(onboarding) => E::Onboarding { guild_id, onboarding },
            None => E::Failed { action: ferrocord::state::admin::community::CommunityAction::Onboarding, guild_id, failure: Failure::NotFound },
        },
        C::Respond { guild_id, options, .. } => E::Responded { guild_id, options },
        other => {
            let (action, guild_id) = other.action();
            E::Failed { action, guild_id, failure: Failure::Problem(ferrocord::state::Problem::MfaRequired) }
        }
    }
}

/// Réponses fictives aux actions sur les serveurs (aucun réseau).
fn server_reply(c: ferrocord::state::servers::ServerCmd) -> ferrocord::state::servers::ServerEvent {
    use ferrocord::state::servers::{Failure, Invite, ServerAction, ServerCmd as C, ServerEvent as E, TemplateInfo};
    let invite = |code: &str, channel: &str, inviter: &str, uses: u32, max_uses: u32, expires: Option<i64>| Invite {
        code: code.into(),
        guild_id: Some(ALPHA),
        channel_id: Some(GENERAL),
        channel_name: channel.into(),
        inviter: Some(inviter.into()),
        uses,
        max_uses,
        max_age: 604_800,
        temporary: false,
        expires_at_ms: expires,
    };
    let now = chrono::Utc::now().timestamp_millis();
    match c {
        C::Invites { guild_id } => E::Invites {
            guild_id,
            invites: vec![invite("apercu1", "général", "Alice", 3, 10, Some(now + 5 * 86_400_000)), invite("apercu2", "hors-sujet", "Bob", 0, 0, None)],
        },
        C::CreateInvite { .. } | C::CreateGroupInvite { .. } => E::InviteCreated(invite("nouveau7", "général", "Moi Fictif", 0, 0, Some(now + 7 * 86_400_000))),
        C::Template { code } => E::Template(TemplateInfo {
            code,
            name: "Club de lecture".into(),
            description: "Un modèle fictif".into(),
            usage_count: 42,
            guild_name: "Lecteurs".into(),
            channels: vec!["général".into(), "lectures".into()],
        }),
        C::Revoke { guild_id, code } => E::Revoked { guild_id, code },
        // Ni création ni adhésion dans l'aperçu.
        C::Join { .. } => E::Failed { action: ServerAction::Join, failure: Failure::Captcha },
        C::Create { .. } | C::CreateFromTemplate { .. } => E::Failed { action: ServerAction::Create, failure: Failure::Captcha },
        C::Leave { .. } => E::Failed { action: ServerAction::Leave, failure: Failure::Captcha },
    }
}

/// Ce que la passerelle enverrait à l'ouverture. Vues de permissions : READY
/// avec rôles, dérogations et mon membre (sans GUILD_CREATE qui le remplacerait).
fn session_events(view: View) -> Vec<Event> {
    let names: &[&str] = match view {
        // Scène `mp-chat` : la session sociale de la page Amis, ouverte sur un MP.
        View::Dm if MPCHAT.load(std::sync::atomic::Ordering::Relaxed) => &["ready_user_social", "ready_supplemental"],
        View::Guild | View::Dm | View::Login | View::Accounts | View::Selector | View::Connecting | View::Removing | View::OldFile | View::Migration | View::WipeQuestion | View::Twin | View::TwinRemove | View::Declined | View::Emptied | View::UnknownContent | View::Replacing | View::RecentWipe | View::FileTry => &["ready_user_classic", "guild_create_alpha", "guild_create_beta", "channel_create_dm"],
        View::Timeout => &["ready_user_roles", "guild_member_update_timeout", "channel_create_dm"],
        View::Friends | View::Group => &["ready_user_social", "ready_supplemental", "channel_create_message_request"],
        _ => &["ready_user_roles", "channel_create_dm"],
    };
    // Décodeur à état : mon identifiant lu dans READY sert aux paquets suivants.
    let mut dec = Decoder::with_people(people());
    let mut out = Vec::new();
    for name in names {
        let mut v = fixture(name);
        if name.starts_with("ready_user") {
            // Réglages synchronisés fictifs (pages de la fenêtre Paramètres).
            v["d"]["user_settings_proto"] = fixture("rest_settings_proto")["settings"].clone();
        }
        if amis() && *name == "ready_user_social" {
            amis_ready(&mut v);
        }
        // Scène `vocal-amis` : Alpha reçoit un salon vocal, où des amis se retrouvent.
        if VOCAL.load(std::sync::atomic::Ordering::Relaxed)
            && *name == "ready_user_social"
            && let Some(list) = v["d"]["guilds"][0]["channels"].as_array_mut()
        {
            list.push(serde_json::json!({ "id": "3000000000000000014", "type": 2, "name": "Veillée du Phare", "position": 2, "parent_id": "3000000000000000010", "permission_overwrites": [], "bitrate": 64000, "user_limit": 0, "rtc_region": null, "flags": 0 }));
        }
        if view == View::Unknown && *name == "ready_user_roles" {
            // Mon membre retiré d'Alpha : ses permissions deviennent inconnues
            // et #modération, masqué dans la vue `masque`, reste affiché.
            v["d"]["guilds"][0]["members"] = Value::Array(Vec::new());
        }
        out.extend(dec.decode(v["t"].as_str().unwrap_or_default(), &v["d"].to_string()));
    }
    out
}

/// Salon ouvert par chaque vue de permissions.
fn perms_target(view: View) -> Option<(u64, u64)> {
    match view {
        View::Hidden | View::Timeout | View::Unknown => Some((ALPHA, GENERAL)),
        View::ReadOnly => Some((ALPHA, ANNONCES)),
        View::NoHistory => Some((ALPHA, ANCIEN)),
        View::Guild | View::Dm | View::Login | View::Accounts | View::Selector | View::Connecting | View::Removing | View::OldFile | View::Migration | View::WipeQuestion | View::Twin | View::TwinRemove | View::Declined | View::Emptied | View::UnknownContent | View::Replacing | View::RecentWipe | View::FileTry => None,
        View::Friends | View::Group => None,
    }
}

/// Vues de permissions : une mention dans un salon masqué (elle ne doit
/// marquer ni le salon ni le serveur) et un message en direct dans #ancien.
fn perms_live_events() -> Vec<Event> {
    let mut out = Vec::new();
    let mut hidden = fixture("message_create_mention");
    hidden["d"]["channel_id"] = MODERATION.to_string().into();
    out.extend(packet(&hidden));
    let mut live = fixture("message_create_simple");
    live["d"]["id"] = "5000000000000000400".into();
    live["d"]["channel_id"] = ANCIEN.to_string().into();
    live["d"]["content"] = "Nouveau message, visible malgré l'historique fermé".into();
    out.extend(packet(&live));
    out
}

/// Messages arrivés après l'ouverture de #général.
fn live_events() -> Vec<Event> {
    let mut out = Vec::new();
    for name in ["message_create_simple", "message_create_attachment", "message_create_embed", "message_create_media"] {
        out.extend(packet(&fixture(name)));
    }
    // Suite du message d'Alice : montre un groupe de plusieurs messages.
    let mut follow = fixture("message_create_embed");
    follow["d"]["id"] = "5000000000000000310".into();
    follow["d"]["content"] = "Et la suite, dans le même groupe : https://exemple.invalid/suite".into();
    follow["d"]["embeds"] = Value::Array(Vec::new());
    out.extend(packet(&follow));
    for name in ["message_create_nick", "message_create_system", "message_create_mention", "message_create_dm", "message_update_partial"] {
        out.extend(packet(&fixture(name)));
    }
    // Texte mis en forme (markdown), avec un rôle coloré à mentionner.
    let mut role = fixture("guild_role_create");
    role["d"]["role"]["id"] = "2000000000000001009".into();
    role["d"]["role"]["name"] = "Modération".into();
    role["d"]["role"]["color"] = 0x3498db.into();
    out.extend(packet(&role));
    out.extend(packet(&fixture("message_create_markdown")));
    // Réponse avec réactions, transfert épinglé, emojis du serveur (lots T5/T6).
    for name in ["message_create_reply", "message_create_forward", "message_reaction_add", "guild_emojis_update"] {
        out.extend(packet(&fixture(name)));
    }
    // Non-lu sans mention sur un autre serveur (anneau gris dans le rail).
    let mut other = fixture("message_create_simple");
    other["d"]["id"] = "5000000000000000900".into();
    other["d"]["channel_id"] = GAMMA_SALON.to_string().into();
    other["d"]["guild_id"] = GAMMA.to_string().into();
    out.extend(packet(&other));
    out
}

/// Serveurs et salons inventés de la scène `boite` (boîte de réception).
const MAQUETTES: u64 = 2_000_000_000_000_000_071;
const MAQUETTES_VITRINE: u64 = 3_000_000_000_000_000_712;
const MAQUETTES_ROBOT: u64 = 3_000_000_000_000_000_713;
const MAQUETTES_ENTRAIDE: u64 = 3_000_000_000_000_000_714;
const CERCLE: u64 = 2_000_000_000_000_000_072;
const CERCLE_LECTURES: u64 = 3_000_000_000_000_000_722;

/// Paquet MESSAGE_CREATE inventé de la scène `boite` : `minutes` avant
/// maintenant (l'identifiant porte l'heure), auteur (identifiant, nom, robot).
fn boite_message(minutes: u64, channel: u64, guild: u64, author: (u64, &str, bool), content: &str, more: Value) -> Value {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let id = (now.saturating_sub(ferrocord::state::model::DISCORD_EPOCH + minutes * 60_000)) << 22 | (minutes & 0xfff);
    let mut v = fixture("message_create_simple");
    let d = &mut v["d"];
    d["id"] = id.to_string().into();
    d["nonce"] = Value::Null;
    d["channel_id"] = channel.to_string().into();
    d["guild_id"] = guild.to_string().into();
    d["author"] = serde_json::json!({ "id": author.0.to_string(), "username": author.1.to_lowercase().replace(' ', "_"), "global_name": author.1, "discriminator": "0", "avatar": null, "bot": author.2, "public_flags": 0 });
    d["content"] = content.into();
    if let Some(more) = more.as_object() {
        for (k, x) in more {
            d[k.as_str()] = x.clone();
        }
    }
    v
}

/// Scène `boite` : deux serveurs inventés (dont un au nom très long), des
/// salons non lus déjà en mémoire (texte mis en forme, embed d'un robot,
/// image jointe) et un salon non lu sans historique en mémoire.
fn boite_events() -> Vec<Event> {
    use ferrocord::state::model::{Channel, ChannelKind, Guild};
    use serde_json::json;
    let chan = |id: u64, name: &str, kind, position, parent| Channel { id, name: name.into(), kind, position, parent, overwrites: Some(Vec::new()), slowmode: 0 };
    let atelier = Guild::new(
        MAQUETTES,
        "Atelier des Maquettistes Ferroviaires et des Dioramas Miniatures du Grand Ouest".into(),
        vec![
            chan(3_000_000_000_000_000_711, "Vie de l'atelier", ChannelKind::Category, 0, None),
            chan(MAQUETTES_ROBOT, "journal-du-dépôt", ChannelKind::Text, 0, Some(3_000_000_000_000_000_711)),
            chan(MAQUETTES_VITRINE, "vitrine-des-réalisations-terminées", ChannelKind::Text, 1, Some(3_000_000_000_000_000_711)),
            chan(MAQUETTES_ENTRAIDE, "entraide", ChannelKind::Text, 2, Some(3_000_000_000_000_000_711)),
        ],
    );
    let cercle = Guild::new(CERCLE, "Cercle des Lecteurs Nocturnes".into(), vec![chan(3_000_000_000_000_000_721, "Salons", ChannelKind::Category, 0, None), chan(CERCLE_LECTURES, "lectures-du-mois", ChannelKind::Text, 0, Some(3_000_000_000_000_000_721))]);
    let mut out = vec![Event::GuildCreate(atelier), Event::GuildCreate(cercle)];
    // Historique (vide) en mémoire : les messages reçus ensuite y sont gardés. « entraide » n'en a pas.
    for c in [MAQUETTES_VITRINE, MAQUETTES_ROBOT, CERCLE_LECTURES] {
        out.push(Event::History { channel_id: c, before: None, messages: Vec::new() });
    }
    let (maelle, octave, robot) = ((1_000_000_000_000_000_071, "Maëlle Tisserand", false), (1_000_000_000_000_000_072, "Octave Brindille", false), (1_000_000_000_000_000_073, "Aiguilleur", true));
    let embed = json!({ "embeds": [{ "type": "rich", "title": "Dépôt mis à jour : version 2.4", "description": "**Nouveautés**\n- signaux lumineux à trois feux\n- plaque tournante motorisée\n\nMerci à tous les testeurs 🎉", "color": 0x2ecc71, "fields": [{ "name": "Voies", "value": "12", "inline": true }, { "name": "Aiguillages", "value": "27", "inline": true }], "footer": { "text": "Journal automatique du dépôt" } }] });
    let image = json!({ "attachments": [{ "id": "7000000000000000071", "filename": "gare-miniature.png", "size": 48211, "url": "https://cdn.discordapp.com/attachments/3000000000000000712/7000000000000000071/gare-miniature.png?ex=00000000&is=00000000&hm=00", "proxy_url": "https://media.discordapp.net/attachments/3000000000000000712/7000000000000000071/gare-miniature.png?ex=00000000&is=00000000&hm=00", "content_type": "image/png", "width": 640, "height": 360 }] });
    let none = json!({});
    for v in [
        boite_message(52, MAQUETTES_VITRINE, MAQUETTES, maelle, "J'ai terminé la **gare de triage** ce week-end, avec l'éclairage intérieur.", image),
        boite_message(51, MAQUETTES_VITRINE, MAQUETTES, maelle, "Il reste le ballast à patiner, *mais ça avance* <:ferris:6000000000000000001>", none.clone()),
        boite_message(38, MAQUETTES_VITRINE, MAQUETTES, octave, "Superbe ! Tu as utilisé quelle échelle ?\n> avec l'éclairage intérieur\nÇa change tout.", none.clone()),
        boite_message(31, MAQUETTES_VITRINE, MAQUETTES, maelle, "Du `HO`, comme d'habitude.", none.clone()),
        boite_message(30, MAQUETTES_VITRINE, MAQUETTES, octave, "Je note. Je posterai mon pont tournant demain.", none.clone()),
        boite_message(24, MAQUETTES_ROBOT, MAQUETTES, robot, "", embed),
        boite_message(19, MAQUETTES_ENTRAIDE, MAQUETTES, octave, "Quelqu'un a un plan de câblage ?", none.clone()),
        boite_message(12, CERCLE_LECTURES, CERCLE, octave, "Prochaine lecture : __Les Villes invisibles__. Rendez-vous <t:1893456000:R> ||(surprise)||", none),
    ] {
        out.extend(packet(&v));
    }
    out
}

/// Mentions récentes ajoutées à celles du jeu d'essai (scène `boite-mentions`) :
/// serveur au nom très long, @everyone, texte mis en forme, emoji du serveur.
fn boite_mentions() -> Vec<Message> {
    use serde_json::json;
    let me = json!({ "mentions": [{ "id": "1000000000000000001", "username": "moi_fictif", "global_name": "Moi Fictif", "discriminator": "0", "avatar": null, "public_flags": 0 }] });
    let list = [
        boite_message(64, MAQUETTES_VITRINE, MAQUETTES, (1_000_000_000_000_000_073, "Aiguilleur", true), "@everyone\n**Mise à jour du règlement** : les photos vont dans <#3000000000000000712> <:ferris:6000000000000000001>", json!({ "mention_everyone": true })),
        boite_message(95, CERCLE_LECTURES, CERCLE, (1_000_000_000_000_000_072, "Octave Brindille", false), "<@1000000000000000001> tu présentes le livre *jeudi* ?", me),
    ];
    decode_history(&Value::Array(list.iter().map(|v| v["d"].clone()).collect()).to_string()).unwrap_or_default()
}

/// Page du jeu d'essai d'un salon : le décodage n'est fait que pour les
/// salons qui en ont une (le banc change de salon à chaque image, et ce
/// décodage, propre à l'aperçu, faussait son temps d'image).
fn fixture_history(channel_id: u64) -> Vec<Message> {
    if [ANNONCES, GAMMA_SALON, GENERAL].contains(&channel_id) { history().remove(&channel_id).unwrap_or_default() } else { Vec::new() }
}

fn history() -> HashMap<u64, Vec<Message>> {
    let body = fixture("rest_history").to_string();
    let mut h = HashMap::new();
    let page = decode_history(&body).unwrap_or_default();
    // Mêmes messages dans les salons ouverts par les vues de permissions.
    for c in [ANNONCES, GAMMA_SALON] {
        h.insert(c, page.clone());
    }
    h.insert(GENERAL, page);
    h
}

/// Système de `--settings ferrocord` (lot C6) : registre et mise à jour
/// inventés ; ni registre, ni réseau, ni navigateur.
struct PreviewInstall {
    sent: bool,
}

impl ferrocord::install::Installer for PreviewInstall {
    fn load(&mut self) {}
    fn set_autostart(&mut self, _: ferrocord::install::Autostart) {}
    fn set_protocol(&mut self, _: bool) {}
    fn configure_updates(&mut self, _: String) {}
    fn check_updates(&mut self) {}
    fn download(&mut self, _: ferrocord::install::update::Manifest) {}
    fn confirm_started(&mut self) {}
    fn open_external(&mut self, _: &str) {}
    fn poll(&mut self) -> Vec<ferrocord::install::InstallEvent> {
        use ferrocord::install::{update, Autostart, InstallEvent, Protocol};
        if std::mem::replace(&mut self.sent, true) {
            return Vec::new();
        }
        let m = update::Manifest { version: update::Version(0, 2, 0), url: "https://exemple.invalid/ferrocord.exe".into(), sha256: [0; 32], size: 21 << 20, notes: "Démarrage avec Windows, liens Discord, licences complètes.".into() };
        vec![
            InstallEvent::Registry { autostart: Autostart::On { minimized: true }, protocol: Protocol::Other(r#""C:\Users\moi\AppData\Local\Discord\Update.exe" --processStart Discord.exe"#.into()) },
            InstallEvent::Update(update::Status::Available(m)),
        ]
    }
}
/// Service audio de `--settings voix` : périphériques inventés, test du
/// micro en cours avec un niveau fixe ; aucun périphérique réel n'est ouvert.
struct PreviewAudio {
    sent: bool,
}

impl ferrocord::state::audio_prefs::AudioService for PreviewAudio {
    fn list_devices(&mut self) {
        self.sent = false;
    }
    fn start_test(&mut self, _: &ferrocord::state::audio_prefs::AudioPrefs) {}
    fn update_test(&mut self, _: &ferrocord::state::audio_prefs::AudioPrefs) {}
    fn stop_test(&mut self) {}
    fn testing(&self) -> bool {
        true
    }
    fn meter(&self) -> Option<ferrocord::state::audio_prefs::Meter> {
        Some(ferrocord::state::audio_prefs::Meter { level_db: -24.0, threshold_db: -45.0, open: true })
    }
    fn poll(&mut self) -> Vec<ferrocord::state::audio_prefs::AudioEvent> {
        use ferrocord::audio::io::DeviceInfo;
        if std::mem::replace(&mut self.sent, true) {
            return Vec::new();
        }
        let dev = |id: &str, name: &str, is_default| DeviceInfo { id: id.into(), name: name.into(), is_default };
        vec![ferrocord::state::audio_prefs::AudioEvent::Devices {
            inputs: vec![dev("wasapi:micro-1", "Micro du casque (USB)", true), dev("wasapi:micro-2", "Réseau de microphones (Realtek)", false)],
            outputs: vec![dev("wasapi:sortie-1", "Casque (USB)", true), dev("wasapi:sortie-2", "Haut-parleurs (Realtek)", false)],
        }]
    }
}

/// Service de capture des scènes `partage…` : écrans, fenêtres et caméras
/// inventés, images synthétiques (bureau en dégradé, fenêtres en blocs) ;
/// rien n'est capturé.
#[derive(Default)]
struct PreviewCapture {
    queue: Vec<ferrocord::state::share::ShareEvent>,
}

/// Image BGRA de `w` × `h` : dégradé de teinte `seed`, avec des fenêtres claires.
fn synthetic_capture(seed: u32, w: u32, h: u32) -> ferrocord::video::Frame {
    let mut px = Vec::with_capacity((4 * w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let in_window = |(x0, y0, x1, y1): (u32, u32, u32, u32)| x * 100 >= x0 * w && x * 100 < x1 * w && y * 100 >= y0 * h && y * 100 < y1 * h;
            let bar = y * 100 >= 94 * h;
            let (r, g, b) = if bar {
                (30, 32, 40)
            } else if in_window((8, 10, 55, 70)) {
                if y * 100 < 16 * h { (60, 64, 80) } else { (236, 238, 244) }
            } else if in_window((48 + seed % 3 * 4, 30, 92, 88)) {
                if y * 100 < 36 * h { (50, 54, 70) } else { (210, 220, 236) }
            } else {
                ((40 + seed * 70 + x * 80 / w) as u8 as u32, (70 + y * 120 / h) as u8 as u32, (150 + seed * 35) as u8 as u32)
            };
            px.extend_from_slice(&[b as u8, g as u8, r as u8, 255]);
        }
    }
    ferrocord::video::Frame { width: w, height: h, pts_us: 0, data: ferrocord::video::FrameData::Bgra(px) }
}

impl ferrocord::state::share::CaptureService for PreviewCapture {
    fn watch(&mut self, tab: ferrocord::state::share::ShareTab) {
        use ferrocord::video::capture::{Source, SourceKind};
        let src = |id: &str, name: &str, kind| Source { id: id.into(), name: name.into(), kind };
        let lists = ferrocord::state::share::SourceLists {
            screens: vec![src("screen:1", "Moniteur 27 pouces", SourceKind::Screen), src("screen:2", "Écran intégré", SourceKind::Screen)],
            windows: vec![
                src("window:11", "Bloc-notes — liste des courses.txt", SourceKind::Window),
                src("window:12", "Navigateur — Documentation de Rust", SourceKind::Window),
                src("window:13", "Lecteur multimédia", SourceKind::Window),
                src("window:14", "Explorateur de fichiers", SourceKind::Window),
                src("window:15", "Tableur — budget du club de lecture.ods", SourceKind::Window),
                src("window:16", "Les Cavernes de Sel — partie en cours", SourceKind::Window),
                src("window:17", "Terminal", SourceKind::Window),
            ],
            cameras: vec![src("camera:usb", "Webcam HD (USB)", SourceKind::Camera), src("camera:int", "Caméra intégrée", SourceKind::Camera)],
        };
        let shown: Vec<Source> = lists.of(tab).iter().filter(|s| ferrocord::state::share::wants_thumbnail(s.kind)).cloned().collect();
        self.queue.push(ferrocord::state::share::ShareEvent::Sources(lists));
        for (i, s) in shown.iter().enumerate() {
            // La dernière fenêtre n'a pas d'image (réduite pendant la capture).
            let frame = (s.kind == SourceKind::Screen || i + 1 < shown.len()).then(|| synthetic_capture(i as u32, 480, 270));
            self.queue.push(ferrocord::state::share::ShareEvent::Thumbnail { id: s.id.clone(), frame });
        }
    }
    fn preview(&mut self, s: &ferrocord::video::capture::Source) {
        self.queue.push(ferrocord::state::share::ShareEvent::Preview { id: s.id.clone(), frame: Ok(synthetic_capture(4, 960, 540)) });
    }
    fn cancel(&mut self) {
        self.queue.clear();
    }
    fn poll(&mut self) -> Vec<ferrocord::state::share::ShareEvent> {
        std::mem::take(&mut self.queue)
    }
}

/// Pièce jointe vidéo des scènes `Lecteur` (lot M).
const PLAYER_VIDEO: u64 = 7_000_000_000_000_000_900;

/// Situations de la liste de messages montrées par `--scene`.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Scene {
    /// Jeu d'essai des glyphes (lot « caractères ») : planche `glyphes`.
    Glyphes,
    /// Polices de nom (lot polices) : planche `polices-nom`.
    PolicesNom,
    /// Carte de mon compte (lot « profil rapide ») : ouverte au-dessus du
    /// panneau utilisateur, statut personnalisé en place.
    MonCompte,
    /// Carte de mon compte avec badges (« +N ») et activité en cours (lot « profils »).
    MonCompteJeu,
    /// MP dont le correspondant, sans bannière, joue (carte d'activité et ses boutons).
    MpActivite,
    /// Carte de mon compte, profil sans couleurs de thème.
    MonCompteSobre,
    /// Carte de mon compte, profil à thème clair.
    MonCompteClair,
    /// Carte de mon compte, sous-menu des statuts ouvert.
    MonCompteStatut,
    /// Fenêtre du statut personnalisé.
    MonCompteStatutPerso,
    /// Édition rapide du profil : modale par-dessus la discussion, onglet
    /// « Profil principal », compte Nitro, bio modifiée (non enregistrée).
    ProfilEdition,
    /// `ProfilEdition`, onglet « Profil de ce serveur ».
    ProfilEditionServeur,
    /// `ProfilEdition` sans Nitro : champs réservés marqués.
    ProfilEditionSansNitro,
    /// `ProfilEdition`, sélecteur « Style du pseudonyme » ouvert (polices de nom).
    ProfilEditionStyle,
    /// Vocal (phase 4, lot V4) : « Serveur Vocal », salon à 4 personnes dont 2
    /// qui parlent, panneau de connexion « Connecté », micro coupé.
    Vocal,
    /// `Vocal` et la fenêtre des codes de vérification DAVE (lot V7).
    VocalCodes,
    /// `Vocal` et un plugin vocal (lot E4, `App::voice_e4_preview`) :
    /// `enregistrement` (0), `enregistrement-demarrer` (1), `incrustation` (2),
    /// `rnnoise` (3).
    VoixE4(u8),
    /// Vue d'appel (phase 5, lot M4) : salon de « Serveur Vocal », 4 personnes
    /// dont 2 en caméra (Dana et moi), mires animées à la place des images.
    Appel,
    /// `Appel`, direct d'Ève regardé et mis en avant, bande des autres tuiles.
    AppelDirect,
    /// `Appel`, barre de contrôle pendant un partage d'écran (bandeau « Arrêter »).
    AppelPartage,
    /// Ligne « Nouveaux messages » à l'ouverture de #général.
    Nouveaux,
    /// Vue remontée, puis des messages arrivent : bouton « nouveaux messages ».
    Bouton,
    /// Fenêtre du cache remontée loin : bouton « Revenir au présent ».
    Present,
    /// `Present` et des messages non envoyés au-dessus de la saisie.
    NonEnvoyes,
    /// Zone de saisie : « Alice et Bob écrivent… », mode lent en cours, texte
    /// « Salut @al » et ses propositions, brouillon dans #hors-sujet.
    Saisie,
    /// Indicateur d'écriture animé (lot ANIM) : une, deux ou plusieurs personnes, pastille qui écrit.
    Ecriture(u8),
    /// Message qui arrive, mention qui pulse, carte qui apparaît (lot ANIM).
    Transitions,
    /// Comme `Saisie`, avec un long texte sur plusieurs lignes (compteur de
    /// caractères, hauteur bornée) au lieu de la liste de propositions.
    SaisieLongue,
    /// Comme `Saisie`, avec une réponse en préparation (bande collée à la carte, lot V3).
    Reponse,
    /// Envoi de fichiers (lot T8) : trois pièces jointes dans le plateau (image,
    /// texte, fichier trop gros), un envoi en cours et un envoi en échec.
    Fichiers,
    /// Fenêtre « Détails de la pièce jointe » ouverte sur la première du plateau.
    FichierDetails,
    /// Fenêtre « Parcourir les salons » du serveur Alpha.
    Parcourir,
    /// Message vocal reçu (phase 6) : carte avec onde et durée, bouton micro dans la saisie.
    MessagesVocaux,
    /// Alerte AutoMod dans le fil, avec son détail et ses actions (lot V).
    AlerteAutomod,
    /// Messages d'une personne bloquée repliés dans le fil (lot FF, F1).
    Bloques,
    /// Menu du « + » de la saisie (lot FF).
    MenuPlus,
    /// Fenêtre d'une mention de rôle (lot FF, F3).
    MentionRole,
    /// Écran d'entrée d'un salon à limite d'âge (lot FF, F8).
    LimiteAge,
    /// Lecteur du fil (lot M) : vidéo en pause à 0:30 avec ses commandes, son
    /// au repos, vidéo WebM (carte « Ouvrir »).
    Lecteur,
    /// `Lecteur`, vidéo agrandie à toute la fenêtre.
    LecteurPlein,
    /// Vidéos jointes au repos (lot VID) : affiche d'un MP4, WebM.
    Videos,
    /// Vidéos des liens (lot VID) : GIF en vidéo, lien direct sans vignette,
    /// site de vidéos (bouton de lecture vers le navigateur).
    VideosLiens,
    /// Fenêtre de confirmation d'un lien externe (lot VID).
    LienExterne,
    /// Parures des profils (lot P) : décorations, plaques, tags, styles de nom,
    /// rôles en dégradé, avatar et bannière animés d'Alice ; animations
    /// réduites (rien ne bouge sans le pointeur : capture stable).
    Parures,
    /// Comme `Parures`, animations en marche (bannière, avatar et décoration
    /// d'un profil ouvert bougent).
    ParuresAnimees,
    /// Sélecteur rapide (Ctrl+K) ouvert sur « gen » (lot navigation).
    Selecteur,
    /// Boîte de réception : salons non lus en cartes (lot navigation, lot « boîte »).
    Boite,
    /// Boîte de réception, premières cartes repliées (image jointe, embed d'un robot).
    BoiteCartes,
    /// Boîte de réception, onglet « Programmés » (plugin activé, deux messages).
    BoiteProgrammes,
    /// Boîte de réception, onglet « Mentions ».
    BoiteMentions,
    /// Fenêtre « Raccourcis clavier » (Ctrl+/, lot navigation).
    Raccourcis,
    /// Fenêtre des notifications d'un serveur, avec deux exceptions (lot N).
    NotifsServeur,
    /// Onglet « Ajouter » de la page Amis (lot FF2 ; vue `amis`).
    AmisAjouter,
    /// Boîte de réception sur l'onglet « Pour vous » (lot N).
    PourVous,
    /// Journal des messages (lot E1) : un message supprimé gardé, un message
    /// modifié dont les versions précédentes sont dépliées.
    Journal,
    /// Lot E2 : barre d'onglets (`onglets`), palette de commandes (`palette`),
    /// vue scindée (`scinde`), fenêtre détachée (`detache`), zoom d'un salon (`zoom`).
    Fenetres(&'static str),
    /// Mots-clés surveillés (lot E1) : mots surlignés dans le fil et lignes
    /// de la boîte de réception (`false`), ou la liste dans les paramètres (`true`).
    MotsCles(bool),
    /// Recherche locale (lot E1) : mode « Local », résultats trouvés sur place.
    RechercheLocale,
    /// Export d'une conversation (lot E1) : la fenêtre (`false`), ou la lecture de l'historique (`true`).
    Export(bool),
    /// Bandeau du rapport de plantage local (lot E5).
    Plantage,
    /// Thèmes partageables (lot E5) : fenêtre d'import d'un thème fourni.
    ThemeImport,
    /// Mica (`false`) ou acrylique (`true`) de Windows 11 derrière le rail et
    /// les salons (lot E5). La fenêtre est créée transparente ; une capture
    /// `--screenshot` ne montre pas la matière du système, seulement le voile.
    Mica(bool),
    /// Plugins « discret et saisie » (lot E3) : 0 mode discret (médias floutés,
    /// « Marquer comme lu »), 1 bandeau d'un message programmé échu, 2 liste
    /// des messages programmés, 3 fenêtre « Programmer », 4 message traduit,
    /// 5 réglages de la traduction.
    E3(u8),
    /// En-tête de la discussion (phase 10, lot S) : sujet tronqué, icônes
    /// Fils, cloche, épingles, membres, recherche à loupe ; fils sous #général.
    Entete,
    /// `Entete`, sujet affiché en entier.
    EnteteSujet,
    /// `Entete`, panneau « Fils » ouvert sur les fils actifs.
    EnteteFils,
    /// `Entete`, panneau « Fils » sur les fils archivés (une page reçue).
    EnteteArchives,
    /// `Entete`, menu des notifications de la cloche (salon muet).
    EnteteCloche,
    /// Menu du serveur ouvert sous l'en-tête de la colonne des salons (lignes de menu du lot V1).
    MenuServeur,
    /// Lot U (vue `amis`) : menu complet d'un utilisateur ouvert sur un ami.
    MenuUtilisateur,
    /// Catégories de contacts : deux catégories garnies (page Amis et colonne des MP).
    CategoriesAmis,
    /// Lot U (vue `amis`) : page « Demandes de message », dossier de spam.
    Demandes,
    /// Lot ND (vue `amis`) : onglet « En attente » avec les suggestions d'amis.
    Suggestions,
    /// Lot ND : boîte de réception, onglet « Enregistrés ».
    Enregistres,
    /// Lot ND : page Confidentialité, section « État du compte » lue.
    EtatCompte,
    /// Lot ND (vue `amis`) : onglet « Applications » de la page « Découvrir ».
    Applis,
    /// Lot ND (vue serveur) : fenêtre « Règles et vérification », demandes listées.
    Verification,
    /// Lot ND (vue serveur) : panneau « Résumés du salon » de #général.
    Resumes,
    /// Lot ND (vue `amis`) : page « Découvrir des serveurs ».
    Decouverte,
    /// Lot ND (vue `amis`) : onglet « Inventaire » de la boutique.
    Inventaire,
    /// Lot ND (vue `amis`) : boutique en consultation.
    Boutique,
    /// Lot ND (vue `amis`) : page des quêtes.
    Quetes,
    /// Lot ND (avec `--profile` ou `--profile-full`) : Alice porte un effet de profil animé.
    Effet,
    /// Comme `Effet`, « réduire les animations » activé (image fixe).
    EffetFixe,
    /// Lot profil : carte de profil d'Alice, parée comme la capture de référence.
    ProfilCarte,
    /// Lot profil : panneau utilisateur paré, micro coupé.
    Panneau,
    /// Comme `Panneau`, menu du chevron du micro ouvert.
    PanneauMenu,
    /// Lot « amis » : page Amis et colonne des MP garnies comme l'accueil du client.
    Amis,
    /// Lot « vocal » : page Amis de la scène `amis`, avec des amis en vocal sur un serveur
    /// commun (ligne des MP, ligne d'ami, carte vocale de la colonne « En ligne ») et une
    /// activité enrichie (carte en deux parties).
    VocalAmis,
    /// Lot « vocal » : rail de la scène `serveurs` avec des amis en vocal et en partage dans un
    /// serveur sans icône du dossier déplié (pastille, infobulle à droite, acronyme, capsule).
    VocalRail,
    /// Lot « vocal » : salon vocal occupé (haut-parleur vert, boutons de survol), membres
    /// rangés comme dans le client, étiquette EN DIRECT et carte d'aperçu d'un direct.
    VocalSalon,
    /// Lot « chatmp » : conversation privée comparée au client (auteur au nom stylé et à
    /// tag, messages d'aujourd'hui et d'hier, réponse qui me mentionne, colonne des MP
    /// de la scène `amis` avec une activité doublée d'un statut).
    MpChat,
    /// Lot « messages » : MP du compte officiel (bandeau à la place de la saisie, cartes de notification).
    MpSysteme,
    /// Lot « messages » : messages transférés avec image, fichier, embed et autocollant.
    Transfert,
    /// Lot U (vue `amis`) : fenêtre « Pseudo d'ami ».
    PseudoAmi,
    /// Liste des salons (lot S) : catégorie repliée qui garde son salon non
    /// lu, salon muet éteint, icônes de fin de ligne du salon ouvert.
    Categories,
    /// `Categories` avec « Masquer les salons muets ».
    CategoriesMuets,
    /// « Modifier le salon » d'un salon vocal : débit, limite, région, qualité vidéo.
    SalonVocal,
    /// Fils (lot T10) : fils sous #général, résumé « N messages » sous le message de départ.
    Fils,
    /// Un fil ouvert : en-tête (parent, rejoindre, archiver) et ses messages.
    Fil,
    /// Le même fil, avec la fenêtre « Membres du fil » (lot FF2).
    FilMembres,
    /// Vue d'un salon forum : publications, étiquettes, tri.
    Forum,
    /// Forum en galerie de cartes (lot R).
    Galerie,
    /// Sondages, messages système, annonce suivie, invitation (lot T13).
    Sondages,
    /// Emojis et autocollants (lot T4), sélecteur de la saisie ouvert.
    Emojis,
    /// Suggestions d'autocollants au-dessus de la saisie pour le mot écrit (0.1.11).
    SuggestionsAutocollants,
    /// Sélecteur de GIF (lot G) : tuiles des favoris, des tendances et des catégories.
    Gifs,
    /// Sélecteur de GIF : résultats de la recherche « chat ».
    GifsRecherche,
    /// Sélecteur de GIF : favoris du compte.
    GifsFavoris,
    /// Sélecteur de réaction d'un compte Nitro, interrupteur « Super réaction » actif (lot G).
    SuperReaction,
    /// Rail des serveurs (lot S7) : dossier replié avec mentions, dossier déplié.
    Serveurs,
    /// `Serveurs` et la fenêtre « Ajouter un serveur ».
    ServeursAjouter,
    /// `Serveurs` et « Inviter des personnes », lien créé.
    ServeursInviter,
    /// `Serveurs` et la liste des invitations d'Alpha.
    ServeursInvitations,
    /// Recherche (lot T12) : « rendez-vous » tapé, panneau des résultats (jeu
    /// `rest_search_messages` passé par le vrai décodeur), mots surlignés.
    Recherche,
    /// Paramètres du serveur (lot A6) : page Emojis d'Alpha (niveau 1, un emoji animé).
    ParametresEmojis,
    /// Page Autocollants, formulaire d'envoi rempli.
    ParametresAutocollants,
    /// Page Sons (liste du jeu d'essai).
    ParametresSons,
    /// Applications (lot T11) : composants, réponse différée, échec, formulaire de commande.
    Applications,
    /// Liste des événements d'Alpha (lot A7).
    Evenements,
    /// Fenêtre de création d'un événement (lot A7).
    EvenementCreer,
    /// Écran de bienvenue d'un serveur communautaire (lot A7).
    Bienvenue,
    /// Questions de l'intégration (« Salons et rôles », lot A7).
    Integration,
    /// Page « Intégration » des paramètres du serveur (lot A7).
    ParametresIntegration,
    /// Page « Écran de bienvenue » des paramètres du serveur (lot A7).
    ParametresBienvenue,
    /// `Applications` avec la fenêtre modale d'un robot ouverte.
    Modale,
    /// Fenêtre « Paramètres du serveur » d'Alpha (lot A8 + A9), sur une page.
    ParametresServeur(ferrocord::ui::guild_settings::Page),
    /// « Modifier le salon » sur #général, vue d'ensemble (lot A2).
    Salons,
    /// « Modifier le salon », page Permissions (rôle Modération, refus pour @everyone).
    SalonsPermissions,
    /// « Créer un salon » dans la catégorie Discussions.
    SalonsCreer,
    /// « Paramètres du serveur », page Salons.
    SalonsServeur,
    /// Paramètres du serveur (lot A3) : page Rôles du serveur « Atelier » (je le possède).
    Roles,
    /// Éditeur du rôle Animateurs, onglet Affichage (dégradé, icône).
    Role,
    /// Éditeur du rôle Modérateurs, onglet Permissions.
    RolePermissions,
    /// Page Membres.
    Membres,
    /// Dialogue des rôles et du pseudo d'un membre.
    MembreRoles,
    /// Paramètres du serveur Alpha sur une page de la modération (lots A4 + A5) :
    /// Sécurité, AutoMod, journal d'audit, bannissements.
    Moderation(ferrocord::ui::guild_settings::Page),
    /// Fenêtre « Bannir Alice » ouverte sur #général (lot A4).
    ModerationMembre,
    /// Appel entrant d'Alice (lot V6) : fenêtre « Accepter / Refuser » et bandeau de l'appel dans son MP.
    Sonnerie,
    /// Appel d'un MP en bandeau au-dessus de la conversation (lot « appel ») : à deux, Alice parle.
    AppelMp,
    /// Appel sortant : Alice n'a pas encore décroché (avatar estompé, points d'attente).
    AppelSortant,
    /// Appel de groupe à quatre : Bob micro coupé, Ève parle, Alice sonne encore.
    AppelGroupe,
    /// Appel d'un MP où Alice partage son écran : le bandeau s'agrandit.
    AppelMpPartage,
    /// Salon de conférence en cours (lot V6) : sujet, orateurs, main levée, auditeurs, gestes du modérateur.
    Conference,
    /// Salon vocal où je suis (lot V6) : statut, son joué par Alice, tableau de sons ouvert.
    Sons,
    /// Sélecteur de partage (lots M3 et « partage ») : barre d'onglets, grille de
    /// vignettes, préréglage de qualité. 0 : `partage` (Applications) ;
    /// 1 : `partage-ecrans` ; 2 : `partage-appareils` ; 3 : `partage-camera`
    /// (aperçu avant d'allumer la caméra) ; 4 : `partage-qualite` (menu de l'engrenage).
    Partage(u8),
    /// Accessibilité (lot C5) : mode streamer actif (bandeau), dernier
    /// message de #général choisi au clavier.
    Accessibilite,
    /// Personnalisation (lot PERSO) : page Apparence avec l'éditeur de thème et des thèmes enregistrés.
    ApparenceEditeur,
    /// Dégradé « Crépuscule » sur les fonds.
    ThemeDegrade,
    /// Image de fond (générée : aucun fichier lu) derrière toute l'application, floutée.
    FondImage,
    /// Densité compacte (`false`) ou spacieuse (`true`).
    Densites(bool),
    /// Barre des serveurs masquée, point de couleur devant les noms.
    SansRail,
    /// Activité (lot C3) : carte de profil d'Alice (Spotify, jeu avec images
    /// et boutons) ; avec `--settings activite`, jeux détectés et programme ajouté.
    Activite,
    /// Rich Presence locale (lot R) : ma carte de profil, avec l'activité reçue d'un jeu par le tube local.
    RichPresence,
    /// Lot « activité » : un jeu (fictif) détecté, encart au-dessus du panneau utilisateur.
    JeuDetecte,
    /// Comme `JeuDetecte`, dans un salon vocal : le bouton de partage de l'encart est là.
    JeuDetecteAppel,
    /// Connexion par QR code (lot C2, avec `--view login`) : QR affiché, compte à rebours.
    Qr,
    /// `Qr` après lecture sur le téléphone : « Connexion de Alice : confirmez ».
    QrLu,
    /// Avec `--view connexion` : l'historique hors ligne d'une session
    /// précédente s'affiche avant READY, qui n'arrive jamais (lot H).
    HorsLigne,
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Guild,
    Dm,
    Login,
    /// Salons masqués (#modération, catégorie Équipe), sur #général.
    Hidden,
    /// #annonces en lecture seule.
    ReadOnly,
    /// #ancien sans READ_MESSAGE_HISTORY.
    NoHistory,
    /// Alpha, dont mon membre est inconnu : tout affiché, #modération compris.
    Unknown,
    /// Exclusion temporaire dans Alpha.
    Timeout,
    /// Écran de connexion avec des comptes enregistrés.
    Accounts,
    /// Dernier compte rouvert (coffre en mémoire), sélecteur de compte ouvert.
    Selector,
    /// Dernier compte en cours de connexion (READY n'arrive jamais).
    Connecting,
    /// Écran de connexion, retrait d'un compte en attente de confirmation.
    Removing,
    /// Écran de connexion, ancien fichier de jeton en clair encore présent
    /// (fichier factice dans un dossier temporaire).
    OldFile,
    /// 1er lancement de la migration : jeton copié dans le coffre, fichier
    /// gardé jusqu'au lancement suivant ; sélecteur ouvert.
    Migration,
    /// Question de suppression du fichier, coffre lu (« seule copie ») ; sélecteur ouvert.
    WipeQuestion,
    /// Même compte enregistré deux fois ; sélecteur ouvert.
    Twin,
    /// Même chose, retrait du doublon en attente de confirmation.
    TwinRemove,
    /// Import refusé auparavant : avertissement avec « Importer » et « Supprimer ».
    Declined,
    /// Fichier vidé (que des zéros) mais pas supprimé : vestige sans jeton ; sélecteur ouvert.
    Emptied,
    /// Fichier au contenu illisible : question de suppression « contenu inconnu » ; sélecteur ouvert.
    UnknownContent,
    /// Jeton saisi d'un compte déjà enregistré avec un autre jeton : question de remplacement ; sélecteur ouvert.
    Replacing,
    /// Question de suppression moins de 24 h après la copie dans le coffre ; sélecteur ouvert.
    RecentWipe,
    /// Jeton du coffre refusé, « Essayer le jeton du fichier » accepté : question de remplacement ; sélecteur ouvert.
    FileTry,
    /// Page Amis (accueil des messages privés), présences du READY_SUPPLEMENTAL.
    Friends,
    /// MP de groupe, panneau des membres ouvert.
    Group,
}

impl View {
    /// Vue qui part sans session : le magasin est vide au départ.
    fn starts_offline(self) -> bool {
        matches!(self, View::Login | View::Accounts | View::Selector | View::Connecting | View::Removing | View::OldFile | View::Migration | View::WipeQuestion | View::Twin | View::TwinRemove | View::Declined | View::Emptied | View::UnknownContent | View::Replacing | View::RecentWipe | View::FileTry)
    }

    /// Vue qui montre des comptes enregistrés.
    fn has_accounts(self) -> bool {
        matches!(self, View::Accounts | View::Selector | View::Connecting | View::Removing | View::OldFile | View::Twin | View::TwinRemove | View::Replacing)
    }
}

const ME: u64 = 1_000_000_000_000_000_001;
const SECOND: u64 = 1_000_000_000_000_000_077;

/// Réglages d'exemple : trois comptes (dont un repris de l'ancien fichier,
/// pas encore vérifié) ; le premier est rouvert par les vues `selecteur` et `connexion`.
fn sample_settings(view: View) -> Settings {
    let me = Account {
        key: AccountKey::of_user(ME),
        user_id: Some(ME),
        name: Some("Moi Fictif".into()),
        avatar: Some(ImageHash::synthetic(ME)),
        place: Some(Place { guild: Some(ALPHA), channel: Some(GENERAL) }),
    };
    let second = Account {
        key: AccountKey::of_user(SECOND),
        user_id: Some(SECOND),
        name: Some("Compte secondaire".into()),
        avatar: Some(ImageHash::synthetic(SECOND)),
        place: None,
    };
    let last = matches!(view, View::Selector | View::Connecting | View::Twin | View::TwinRemove).then(|| me.key.clone());
    // Vues `doublon` : le compte importé est le même compte que le premier.
    let imported = match view {
        View::Twin | View::TwinRemove => Account { key: AccountKey::imported(), ..me.clone() },
        _ => Account::new(AccountKey::imported()),
    };
    Settings { last_account: last, accounts: vec![me, second, imported], ..Settings::default() }
}

/// Coffre en mémoire avec un jeton factice par compte d'exemple.
fn sample_vault() -> MemoryVault {
    MemoryVault::with([
        (AccountKey::of_user(ME), Token::from("faux.jeton.apercu-1")),
        (AccountKey::of_user(SECOND), Token::from("faux.jeton.apercu-2")),
        (AccountKey::imported(), Token::from("faux.jeton.apercu-3")),
    ])
}

/// Magasin dans l'état d'une session ouverte (sur #général, ou sur la
/// première conversation privée, ou sur le salon d'une vue de permissions).
/// Renvoie aussi l'historique complet des salons servis page par page (scène `present`).
fn filled_store(view: View, scene: Option<Scene>) -> (Store, HashMap<u64, Vec<Message>>) {
    if scene == Some(Scene::NonEnvoyes) {
        let (mut s, pages) = filled_store(view, Some(Scene::Present));
        s.apply(Event::SendError { channel_id: GENERAL, nonce: 1, content: "Quelqu'un a les notes de la réunion ?".into(), error: Problem::Network("connection reset".into()), forbidden: false });
        s.apply(Event::SendUnconfirmed { channel_id: GENERAL, nonce: 2, content: "Je les mets dans le dossier partagé".into() });
        return (s, pages);
    }
    let mut s = Store::new();
    s.apply(Event::PeopleHub(people()));
    if view.starts_offline() {
        return (s, HashMap::new());
    }
    for e in session_events(view) {
        s.apply(e);
    }
    if let Some((guild, channel)) = perms_target(view) {
        s.select_guild(guild);
        s.select_channel(channel);
        let cmds = s.take_commands();
        let mut hist = history();
        for c in cmds {
            if let Cmd::History { channel_id, before: None } = c {
                let messages = hist.remove(&channel_id).unwrap_or_default();
                s.apply(Event::History { channel_id, before: None, messages });
            }
        }
        for e in perms_live_events() {
            s.apply(e);
        }
        return (s, HashMap::new());
    }
    s.select_guild(ALPHA);
    s.select_channel(GENERAL);
    s.take_commands();
    let mut hist = history();
    // Messages plus anciens que ceux des jeux de données : 40 pour pouvoir
    // défiler (`bouton`), 600 pour remonter la fenêtre du cache (`present`).
    let older_count = match scene {
        Some(Scene::Bouton) => 40,
        Some(Scene::Present) => 600,
        _ => 0,
    };
    let mut deep = Vec::new();
    if let Some(page) = hist.get_mut(&GENERAL)
        && older_count > 0
    {
        let first = page.first().map_or(0, |m| m.id);
        let ms = ferrocord::state::model::snowflake_ms(first);
        let older = (0..older_count).map(|i| bench::message(i, GENERAL, ALPHA, ms - (older_count - i) as i64 * 600_000)).filter(|m| m.id < first);
        deep = older.collect();
        deep.extend(page.iter().cloned());
        *page = deep[deep.len().saturating_sub(HISTORY_PAGE)..].to_vec();
    }
    // Historique (vide) de la conversation privée : le message en direct s'y ajoute.
    hist.insert(DM, Vec::new());
    if scene == Some(Scene::MpSysteme) {
        hist.insert(DM_SYSTEME, Vec::new());
    }
    for (channel_id, messages) in hist.drain() {
        s.apply(Event::History { channel_id, before: None, messages });
    }
    if scene == Some(Scene::Present) {
        // Lecture de l'historique comme en remontant la liste, page par page,
        // la vue en haut : la fenêtre glisse et quitte le présent.
        while !s.chans.get(&GENERAL).is_some_and(|c| c.newer) {
            let Some(front) = s.chans.get(&GENERAL).and_then(|c| c.msgs.front()).map(|m| m.id) else { break };
            s.set_viewing(GENERAL, Some((front, front)));
            if !s.load_older(GENERAL) {
                break;
            }
            s.take_commands();
            let messages: Vec<Message> = deep.iter().rev().filter(|m| m.id < front).take(HISTORY_PAGE).cloned().collect();
            s.apply(Event::History { channel_id: GENERAL, before: Some(front), messages });
        }
        s.set_viewing(GENERAL, None);
    }
    // Ligne « Nouveaux messages » : les messages arrivent pendant qu'un autre salon est ouvert.
    if scene == Some(Scene::Nouveaux) {
        s.select_channel(ANNONCES);
    }
    if scene != Some(Scene::Bouton) {
        for e in live_events() {
            s.apply(e);
        }
        for e in profile_events() {
            s.apply(e);
        }
        if matches!(scene, Some(Scene::Amis | Scene::MpChat | Scene::MpActivite | Scene::VocalAmis | Scene::MpSysteme | Scene::Transfert)) {
            for e in amis_events() {
                s.apply(e);
            }
        }
        if scene == Some(Scene::VocalAmis) {
            for e in vocal_amis_events() {
                s.apply(e);
            }
        }
        let extra = match scene {
            Some(Scene::MpSysteme) => mp_systeme_events(),
            Some(Scene::Transfert) => transfert_events(),
            _ => Vec::new(),
        };
        for e in extra {
            s.apply(e);
        }
        if matches!(scene, Some(Scene::MpChat | Scene::MpActivite)) {
            for e in mp_chat_events() {
                s.apply(e);
            }
        }
    }
    if matches!(scene, Some(Scene::Activite | Scene::RichPresence | Scene::Panneau | Scene::PanneauMenu | Scene::MonCompteJeu | Scene::MpActivite)) {
        activity_store(&mut s);
    }
    if matches!(scene, Some(Scene::JeuDetecte | Scene::JeuDetecteAppel)) {
        detected_game_store(&mut s);
    }
    // Boîte de réception : serveurs inventés, salons non lus avec et sans messages en mémoire.
    if matches!(scene, Some(Scene::Boite | Scene::BoiteMentions | Scene::BoiteCartes)) {
        for e in boite_events() {
            s.apply(e);
        }
    }
    if scene == Some(Scene::Nouveaux) {
        s.select_channel(GENERAL);
    }
    if scene == Some(Scene::Bloques) {
        use ferrocord::state::relations::{RelKind, Relation, RelationsEvent};
        s.apply(Event::Relations(RelationsEvent::RelationAdd(Relation { id: 1_000_000_000_000_000_003, kind: RelKind::Blocked, nickname: None, ignored: false, stranger: false, user: None })));
    }
    if scene == Some(Scene::LimiteAge) {
        s.apply(Event::Threads(ferrocord::state::threads::ThreadEvent::AgeGates { guild_id: ALPHA, channels: vec![GENERAL] }));
    }
    if scene == Some(Scene::MessagesVocaux) {
        for e in packet(&fixture("message_create_voice")) {
            s.apply(e);
        }
    }
    if scene == Some(Scene::AlerteAutomod) {
        for e in packet(&fixture("message_create_automod")) {
            s.apply(e);
        }
    }
    if matches!(scene, Some(Scene::Lecteur | Scene::LecteurPlein)) {
        use serde_json::json;
        let mut v = fixture("message_create_voice");
        v["d"]["flags"] = json!(0);
        v["d"]["content"] = json!("La démo d'hier, et la musique du générique.");
        let file = |id: u64, name: &str, mime: &str, size: u64, dims: Option<(u32, u32)>| {
            let url = format!("https://cdn.discordapp.com/attachments/{GENERAL}/{id}/{name}?ex=00000000&is=00000000&hm=00");
            let mut a = json!({ "id": id.to_string(), "filename": name, "size": size, "url": url, "proxy_url": url, "content_type": mime });
            if let Some((w, h)) = dims {
                a["width"] = json!(w);
                a["height"] = json!(h);
            }
            a
        };
        v["d"]["attachments"] = json!([
            file(PLAYER_VIDEO, "demo-ferrocord.mp4", "video/mp4", 18_400_000, Some((1280, 720))),
            file(PLAYER_VIDEO + 1, "generique.mp3", "audio/mpeg", 4_100_000, None),
            file(PLAYER_VIDEO + 2, "ancien-format.webm", "video/webm", 2_300_000, Some((640, 360))),
        ]);
        for e in packet(&v) {
            s.apply(e);
        }
    }
    if matches!(scene, Some(Scene::Videos | Scene::VideosLiens)) {
        use serde_json::json;
        let mut v = fixture("message_create_voice");
        v["d"]["flags"] = json!(0);
        v["d"]["content"] = json!("Le clip et sa version WebM.");
        let file = |id: u64, name: &str, mime: &str, size: u64, dims: (u32, u32)| {
            let path = format!("attachments/{GENERAL}/{id}/{name}?ex=00000000&is=00000000&hm=00");
            json!({ "id": id.to_string(), "filename": name, "size": size, "url": format!("https://cdn.discordapp.com/{path}"), "proxy_url": format!("https://media.discordapp.net/{path}"), "content_type": mime, "width": dims.0, "height": dims.1 })
        };
        if scene == Some(Scene::Videos) {
            v["d"]["attachments"] = json!([file(PLAYER_VIDEO + 10, "clip-de-la-soiree.mp4", "video/mp4", 12_400_000, (1280, 720)), file(PLAYER_VIDEO + 11, "capture.webm", "video/webm", 2_300_000, (640, 360))]);
        }
        let ext = "https://images-ext-1.discordapp.net/external/fictif/https";
        if scene == Some(Scene::VideosLiens) {
            v["d"]["content"] = json!("Trois liens.");
            v["d"]["attachments"] = json!([]);
        }
        v["d"]["embeds"] = if scene == Some(Scene::Videos) { json!([]) } else { json!([
            { "type": "gifv", "url": "https://gif.exemple.invalid/vue/chat", "provider": { "name": "GifFictif" },
              "thumbnail": { "url": "https://gif.exemple.invalid/chat.png", "proxy_url": format!("{ext}/gif.exemple.invalid/chat.png"), "width": 320, "height": 180 },
              "video": { "url": "https://gif.exemple.invalid/chat.mp4", "proxy_url": format!("{ext}/gif.exemple.invalid/chat.mp4"), "width": 320, "height": 180 } },
            { "type": "video", "url": "https://fichiers.exemple.invalid/demo.mp4",
              "video": { "url": "https://fichiers.exemple.invalid/demo.mp4", "proxy_url": format!("{ext}/fichiers.exemple.invalid/demo.mp4"), "width": 480, "height": 270 } },
            { "type": "video", "url": "https://video.exemple.invalid/regarder?v=fictif", "title": "Ferrocord en dix minutes", "color": 16711680,
              "provider": { "name": "VidéoFictive" }, "author": { "name": "Chaîne fictive" },
              "thumbnail": { "url": "https://video.exemple.invalid/vi/fictif.jpg", "proxy_url": format!("{ext}/video.exemple.invalid/vi/fictif.jpg"), "width": 480, "height": 270 },
              "video": { "url": "https://video.exemple.invalid/embed/fictif", "width": 1280, "height": 720 } }
        ]) };
        for e in packet(&v) {
            s.apply(e);
        }
    }
    if matches!(scene, Some(Scene::Saisie | Scene::SaisieLongue | Scene::Reponse)) {
        saisie_store(&mut s);
    }
    if let Some(Scene::Ecriture(n)) = scene {
        let names = ["Alice", "bob_fictif", "Chloé", "David"];
        for (i, name) in names.iter().enumerate().take(usize::from(n)) {
            s.apply(Event::TypingStart { channel_id: GENERAL, user_id: 1_000_000_000_000_000_002 + i as u64, name: Some((*name).into()) });
        }
    }
    if matches!(scene, Some(Scene::Fils | Scene::Fil | Scene::FilMembres | Scene::Forum | Scene::Galerie)) {
        threads_store(&mut s, scene);
    }
    if matches!(scene, Some(Scene::Entete | Scene::EnteteSujet | Scene::EnteteFils | Scene::EnteteArchives | Scene::EnteteCloche)) {
        threads_store(&mut s, scene);
        head_store(&mut s, scene);
    }
    if matches!(scene, Some(Scene::Categories | Scene::CategoriesMuets)) {
        categories_store(&mut s, scene == Some(Scene::CategoriesMuets));
    }
    if scene == Some(Scene::SalonVocal) {
        voice_store(&mut s);
    }
    if scene == Some(Scene::Sondages) {
        polls_store(&mut s);
    }
    if matches!(scene, Some(Scene::ParametresEmojis | Scene::ParametresAutocollants | Scene::ParametresSons)) {
        expressions_store(&mut s);
    }
    if matches!(scene, Some(Scene::Emojis | Scene::SuggestionsAutocollants | Scene::Gifs | Scene::GifsRecherche | Scene::GifsFavoris | Scene::SuperReaction)) {
        emojis_store(&mut s);
        if scene == Some(Scene::SuperReaction) {
            // Profil chargé avec Nitro : la super réaction est proposée.
            use ferrocord::state::usersettings::{AccountEvent, Profile};
            let profile = Profile { id: ME, username: "moi.fictif".into(), global_name: Some("Moi Fictif".into()), email: None, phone: None, verified: true, mfa: true, premium: 2, bio: String::new(), pronouns: String::new(), accent: None };
            s.apply(Event::Account(AccountEvent::Profile(Ok(profile))));
        }
    }
    if matches!(scene, Some(Scene::Vocal | Scene::VocalCodes | Scene::VoixE4(_) | Scene::JeuDetecteAppel | Scene::VocalSalon)) {
        voice_store(&mut s);
        if scene == Some(Scene::VocalSalon) {
            vocal_salon_store(&mut s);
        }
        if scene == Some(Scene::VocalCodes) {
            voice_codes_store(&mut s);
        }
    }
    if matches!(scene, Some(Scene::Appel | Scene::AppelDirect | Scene::AppelPartage)) {
        voice_store(&mut s);
        call_video_store(&mut s, scene);
    }
    if matches!(scene, Some(Scene::Roles | Scene::Role | Scene::RolePermissions | Scene::Membres | Scene::MembreRoles)) {
        roles_store(&mut s);
    }
    if matches!(scene, Some(Scene::Serveurs | Scene::ServeursAjouter | Scene::ServeursInviter | Scene::ServeursInvitations | Scene::VocalRail)) {
        servers_store(&mut s);
    }
    if scene == Some(Scene::VocalRail) {
        vocal_rail_store(&mut s);
    }
    if matches!(scene, Some(Scene::Salons | Scene::SalonsPermissions | Scene::SalonsCreer | Scene::SalonsServeur)) {
        salons_store(&mut s);
    }
    if matches!(scene, Some(Scene::Applications | Scene::Modale)) {
        apps_store(&mut s, scene);
    }
    if matches!(scene, Some(Scene::Evenements | Scene::EvenementCreer | Scene::Bienvenue | Scene::Integration | Scene::ParametresIntegration | Scene::ParametresBienvenue)) {
        admin_store(&mut s);
    }
    if let Some(sc @ (Scene::AppelMp | Scene::AppelSortant | Scene::AppelGroupe | Scene::AppelMpPartage)) = scene {
        dm_call_store(&mut s, sc);
    }
    if matches!(scene, Some(Scene::Sonnerie | Scene::Conference | Scene::Sons)) {
        calls_store(&mut s, scene);
    }
    // Fenêtre « Paramètres du serveur » (phase 3) : droits inconnus, rien n'est
    // proposé ; l'aperçu me rend propriétaire d'Alpha et du serveur communautaire.
    if matches!(
        scene,
        Some(Scene::ParametresServeur(_) | Scene::SalonsServeur | Scene::Moderation(_) | Scene::ParametresEmojis | Scene::ParametresAutocollants | Scene::ParametresSons | Scene::ParametresIntegration | Scene::ParametresBienvenue)
    ) {
        for g in s.guilds.iter_mut().filter(|g| g.id == ALPHA || g.id == COMMUNITY_GUILD) {
            g.owner_id = Some(ME);
        }
    }
    s.take_commands();
    if matches!(scene, Some(Scene::Fichiers | Scene::FichierDetails)) {
        // Deux envois déjà partis (leurs commandes vont au faux réseau) : l'un
        // progresse, l'autre échoue.
        s.select_channel(GENERAL);
        s.take_commands();
        let photo = |name: &str, n: u64| ferrocord::state::upload::OutFile {
            source: ferrocord::state::upload::Source::Memory(Arc::from(vec![0u8; 16])),
            name: name.into(),
            size: n,
            description: String::new(),
            spoiler: false,
        };
        let _ = s.send_files(GENERAL, "Les photos de la sortie".into(), vec![photo("sortie-1.jpg", 3_400_000), photo("sortie-2.jpg", 2_900_000)], None);
        let _ = s.send_files(GENERAL, "Le compte rendu (échec)".into(), vec![photo("compte-rendu.pdf", 900_000)], None);
    }
    if view == View::Dm {
        s.select_dms();
        s.select_channel(if scene == Some(Scene::MpSysteme) { DM_SYSTEME } else { DM });
        s.take_commands();
    }
    if view == View::Friends {
        s.select_dms();
    }
    if view == View::Group {
        s.select_dms();
        s.select_channel(DM_GROUP);
        s.take_commands();
        s.apply(Event::History { channel_id: DM_GROUP, before: None, messages: Vec::new() });
    }
    let pages = if scene == Some(Scene::Present) { HashMap::from([(GENERAL, deep)]) } else { HashMap::new() };
    // Lot ND : dans les scènes d'édition du profil, deux serveurs ont un tag que le compte peut afficher.
    if NITRO.load(std::sync::atomic::Ordering::Relaxed) {
        for (g, tag) in s.guilds.iter_mut().zip(["ALFA", "BÊTA"]) {
            g.tag = ferrocord::state::cosmetics::GuildTag::new(g.id, tag, None);
        }
    }
    (s, pages)
}

/// Scènes `applications` et `modale` (lot T11) : session de passerelle,
/// index des commandes (`rest_application_command_index`), messages à
/// composants v1 (éphémère) et v2, réponse différée en cours, commande
/// restée sans réponse ; `modale` reçoit en plus INTERACTION_MODAL_CREATE.
fn apps_store(s: &mut Store, scene: Option<Scene>) {
    use ferrocord::state::interactions::{InteractionEvent as I, Scope, SessionId};
    s.apply(Event::Interactions(I::Session(SessionId("session-apercu".into()))));
    if let Some(index) = ferrocord::net::decode_command_index(&fixture("rest_application_command_index").to_string()) {
        s.apply(Event::Interactions(I::Index { scope: Scope::Guild(ALPHA), index }));
    }
    s.apply(Event::Interactions(I::Index { scope: Scope::User, index: Default::default() }));
    s.select_channel(GENERAL);
    s.take_commands();
    for name in ["message_create_components", "message_create_components_v2"] {
        for e in packet(&fixture(name)) {
            s.apply(e);
        }
    }
    // Réponse différée : drapeau LOADING, sans texte ni composants.
    let mut wait = fixture("message_create_components");
    wait["d"]["id"] = "5000000000000000920".into();
    wait["d"]["flags"] = 128.into();
    wait["d"]["content"] = "".into();
    wait["d"]["components"] = serde_json::json!([]);
    wait["d"]["interaction_metadata"]["name"] = "meteo".into();
    for e in packet(&wait) {
        s.apply(e);
    }
    // Commande partie, puis INTERACTION_FAILURE (délai dépassé).
    let role = s.app_commands(GENERAL, ferrocord::state::interactions::CommandKind::ChatInput).iter().find(|c| c.cmd.name == "role").map(|c| c.cmd.id);
    if let Some(id) = role
        && s.run_command(GENERAL, id, vec!["retirer".into()], Vec::new(), None)
    {
        for c in s.take_commands() {
            if let Cmd::Interactions(ferrocord::state::interactions::InteractionCmd::Send(o)) = c {
                s.apply(Event::Interactions(I::Sent { nonce: o.nonce }));
                s.apply(Event::Interactions(I::Failed { nonce: o.nonce, id: 0, reason: 2 }));
            }
        }
    }
    if scene == Some(Scene::Modale) {
        // La fenêtre ne s'ouvre qu'en réponse à une interaction partie d'ici.
        let mut modal = fixture("interaction_modal_create");
        if let Some(id) = role
            && s.run_command(GENERAL, id, vec!["retirer".into()], Vec::new(), None)
        {
            for c in s.take_commands() {
                if let Cmd::Interactions(ferrocord::state::interactions::InteractionCmd::Send(o)) = c {
                    modal["d"]["nonce"] = o.nonce.to_string().into();
                }
            }
        }
        for e in packet(&modal) {
            s.apply(e);
        }
    }
}

/// Salon forum ajouté à Alpha par les scènes des fils.
const FORUM: u64 = 3_000_000_000_000_000_051;

/// Scènes `fils`, `fil` et `forum` (lot T10) : un forum dans Alpha, des
/// fils sous #général (dont un né d'un message de l'historique) et des
/// publications, arrivés par la passerelle (CHANNEL_CREATE, THREAD_LIST_SYNC).
fn threads_store(s: &mut Store, scene: Option<Scene>) {
    let starter = s.chans.get(&GENERAL).and_then(|c| c.msgs.iter().rev().nth(3)).map_or(GENERAL + 1, |m| m.id);
    let mut forum = fixture("guild_create_forum")["d"]["channels"][1].clone();
    forum["id"] = FORUM.to_string().into();
    forum["guild_id"] = ALPHA.to_string().into();
    forum["position"] = 9.into();
    forum["permission_overwrites"] = Value::Array(Vec::new());
    for e in packet(&serde_json::json!({ "t": "CHANNEL_CREATE", "d": forum })) {
        s.apply(e);
    }
    let meta = |archived: bool| serde_json::json!({ "archived": archived, "auto_archive_duration": 1440, "archive_timestamp": "2026-11-10T10:00:00+00:00", "locked": false });
    let th = |id: u64, parent: u64, name: &str, count: u32, tags: &[&str], owner: u64| {
        serde_json::json!({ "id": id.to_string(), "type": 11, "guild_id": ALPHA.to_string(), "parent_id": parent.to_string(), "name": name, "owner_id": owner.to_string(),
            "message_count": count, "member_count": 3, "applied_tags": tags, "flags": if name.starts_with("Règles") { 2 } else { 0 }, "thread_metadata": meta(false) })
    };
    let alice = 1_000_000_000_000_000_002;
    let threads = serde_json::json!([
        th(starter, GENERAL, "Organisation de la sortie", 12, &[], alice),
        th(starter + 7, GENERAL, "Idées pour le logo", 4, &[], ME),
        th(FORUM + 101, FORUM, "Règles du forum", 1, &[], ME),
        th(FORUM + 102, FORUM, "Comment configurer le thème ?", 6, &["6000000000000000900"], alice),
        th(FORUM + 103, FORUM, "Résolu : l'aperçu ne démarre pas", 3, &["6000000000000000901"], alice),
        th(FORUM + 104, FORUM, "Suggestion : dossiers de serveurs", 18, &["6000000000000000900"], ME),
    ]);
    let members = serde_json::json!([ { "id": starter.to_string(), "user_id": ME.to_string(), "join_timestamp": "2026-11-10T10:00:00+00:00", "flags": 0 },
        { "id": (FORUM + 104).to_string(), "user_id": ME.to_string(), "join_timestamp": "2026-11-10T10:00:00+00:00", "flags": 0 } ]);
    let sync = serde_json::json!({ "t": "THREAD_LIST_SYNC", "d": { "guild_id": ALPHA.to_string(), "threads": threads, "members": members } });
    for e in packet(&sync) {
        s.apply(e);
    }
    match scene {
        Some(Scene::Fil | Scene::FilMembres) => {
            s.select_channel(starter);
            s.take_commands();
            let ms = ferrocord::state::model::snowflake_ms(starter);
            let messages = (0..6).map(|i| bench::message(i, starter, ALPHA, ms + (i as i64 + 1) * 60_000)).collect();
            s.apply(Event::History { channel_id: starter, before: None, messages });
        }
        Some(Scene::Forum | Scene::Galerie) => s.select_channel(FORUM),
        _ => {}
    }
}

/// Scènes `entete*` (phase 10, lot S) : #général reçoit un sujet (long, avec
/// un lien) par CHANNEL_UPDATE, passé par le vrai décodeur ; `entete-cloche` :
/// le salon est muet.
fn head_store(s: &mut Store, scene: Option<Scene>) {
    let mut update = fixture("channel_update");
    update["d"]["id"] = GENERAL.to_string().into();
    update["d"]["name"] = "général".into();
    update["d"]["position"] = 0.into();
    update["d"]["parent_id"] = DISCUSSIONS.to_string().into();
    update["d"]["topic"] = "Discussions générales du serveur. Soyez courtois, lisez les règles avant de poster : https://example.org/regles\n\nLes annonces passent par **#annonces** ; le hors-sujet a son salon.".into();
    for e in packet(&update) {
        s.apply(e);
    }
    if scene == Some(Scene::EnteteCloche) {
        use ferrocord::state::usersettings::notif::{ChannelNotif, GuildNotif, Level, Mute};
        use ferrocord::state::usersettings::AccountEvent;
        let mut g = GuildNotif::new(Some(ALPHA));
        g.channels.insert(GENERAL, ChannelNotif { mute: Mute { on: true, until_ms: None }, level: Level::Mentions, flags: 0 });
        s.apply(Event::Account(AccountEvent::GuildSettingsUpdate(g)));
    }
    s.select_channel(GENERAL);
}

/// Fil archivé renvoyé par le faux réseau à l'onglet « Archivés » (scène `entete-archives`).
fn archived_reply(parent_id: u64, before: Option<i64>) -> Event {
    use ferrocord::state::threads::{Thread, ThreadEvent, ThreadKind};
    let th = |id: u64, name: &str, messages: u32, locked: bool| Thread {
        id,
        guild_id: ALPHA,
        parent_id,
        name: name.into(),
        kind: ThreadKind::Public,
        owner_id: Some(ME),
        archived: true,
        locked,
        auto_archive: 1440,
        archived_at: Some(1_760_000_000_000 + id as i64 % 1000),
        created_at: 1_750_000_000_000,
        message_count: messages,
        member_count: 4,
        applied_tags: Vec::new(),
        last_message_id: None,
        slowmode: 0,
        pinned: false,
        joined: false,
    };
    let threads = vec![th(parent_id + 901, "Bilan de la réunion d'octobre", 42, false), th(parent_id + 902, "Ancien sondage sur le nom du projet", 17, true)];
    Event::Threads(ThreadEvent::Archived { parent_id, before, threads, joined: vec![parent_id + 901], more: false })
}

/// Scènes `categories*` (phase 10, lot S) : Alpha reçoit une catégorie
/// « Projets » (repliée, avec un salon non lu qui reste visible) et un salon
/// muet ; réglages du compte arrivés comme une mise à jour de la passerelle.
fn categories_store(s: &mut Store, hide_muted: bool) {
    use ferrocord::state::usersettings::notif::{ChannelNotif, GuildNotif, Level, Mute};
    use ferrocord::state::usersettings::AccountEvent;
    const PROJETS: u64 = 3_000_000_000_000_000_030;
    let chan = |id: u64, kind: u8, name: &str, position: i32, parent: Option<u64>| {
        serde_json::json!({ "t": "CHANNEL_CREATE", "d": { "id": id.to_string(), "type": kind, "guild_id": ALPHA.to_string(), "name": name, "position": position,
            "parent_id": parent.map(|p| p.to_string()), "permission_overwrites": [], "last_message_id": null } })
    };
    let packets = [
        chan(PROJETS, 4, "Projets", 5, None),
        chan(PROJETS + 1, 0, "design", 6, Some(PROJETS)),
        chan(PROJETS + 2, 0, "code", 7, Some(PROJETS)),
        chan(PROJETS + 3, 0, "idées-en-vrac", 8, Some(PROJETS)),
        chan(DISCUSSIONS + 5, 0, "mèmes", 3, Some(DISCUSSIONS)),
    ];
    for p in &packets {
        for e in packet(p) {
            s.apply(e);
        }
    }
    // Un message dans #code : il reste visible dans sa catégorie repliée.
    let mut m = fixture("message_create_simple");
    m["d"]["id"] = "5000000000000000990".into();
    m["d"]["channel_id"] = (PROJETS + 2).to_string().into();
    for e in packet(&m) {
        s.apply(e);
    }
    let mut g = GuildNotif::new(Some(ALPHA));
    g.collapsed.insert(PROJETS);
    g.hide_muted = hide_muted;
    g.channels.insert(DISCUSSIONS + 5, ChannelNotif { mute: Mute { on: true, until_ms: None }, level: Level::Inherit, flags: 0 });
    s.apply(Event::Account(AccountEvent::GuildSettingsUpdate(g)));
    for g in s.guilds.iter_mut().filter(|g| g.id == ALPHA) {
        g.owner_id = Some(ME);
    }
    s.select_channel(GENERAL);
}

/// Scènes `serveurs*` (lot S7 + A1) : quatre serveurs de plus, deux dossiers
/// dans les réglages synchronisés (« Jeux », replié, avec des mentions ;
/// « Travail », déplié), arrivés comme une mise à jour des réglages.
/// Serveur « Atelier » des scènes du lot A3 : je le possède, niveau de boost 2,
/// rôles variés (dégradé, rôle de bot, rôle des boosters) et six membres connus.
const ATELIER: u64 = 2_000_000_000_000_000_201;
const R_ADMIN: u64 = 2_000_000_000_000_000_211;
const R_MODO: u64 = 2_000_000_000_000_000_212;
const R_ANIM: u64 = 2_000_000_000_000_000_213;
const R_BOOST: u64 = 2_000_000_000_000_000_214;
const R_BOT: u64 = 2_000_000_000_000_000_215;
const R_MEMBRE: u64 = 2_000_000_000_000_000_216;
const CAMILLE: u64 = 1_000_000_000_000_000_301;

fn roles_store(s: &mut Store) {
    use serde_json::json;
    let role = |id: u64, name: &str, color: u32, position: i32, permissions: &str| {
        json!({ "id": id.to_string(), "name": name, "color": color, "position": position, "permissions": permissions, "hoist": false, "mentionable": false, "managed": false, "icon": null, "unicode_emoji": null, "flags": 0 })
    };
    let mut anim = role(R_ANIM, "Animateurs", 0x2ecc71, 4, "1110517509");
    anim["colors"] = json!({ "primary_color": 0x2ecc71, "secondary_color": 0x1abc9c, "tertiary_color": null });
    anim["hoist"] = true.into();
    anim["unicode_emoji"] = "*".into();
    let mut boost = role(R_BOOST, "Server Booster", 0xf47fff, 3, "0");
    boost["managed"] = true.into();
    boost["tags"] = json!({ "premium_subscriber": null });
    let mut bot = role(R_BOT, "Musique", 0, 2, "36703232");
    bot["managed"] = true.into();
    bot["tags"] = json!({ "bot_id": "1000000000000000399" });
    let roles = vec![
        role(ATELIER, "@everyone", 0, 0, "1071698660929"),
        role(R_ADMIN, "Admin", 0xe74c3c, 6, "8"),
        role(R_MODO, "Modérateurs", 0x3498db, 5, "1099780063238"),
        anim,
        boost,
        bot,
        role(R_MEMBRE, "Membres", 0x95a5a6, 1, "0"),
    ];
    let people = [
        (CAMILLE, "camille", "Camille", vec![R_MODO, R_MEMBRE], "2023-02-11T09:00:00+00:00", Some("Cam")),
        (CAMILLE + 1, "dorian", "Dorian", vec![R_ANIM, R_BOOST, R_MEMBRE], "2023-06-02T09:00:00+00:00", None),
        (CAMILLE + 2, "elise", "Élise", vec![R_ANIM, R_MEMBRE], "2024-01-20T09:00:00+00:00", None),
        (CAMILLE + 3, "farid", "Farid", vec![R_MEMBRE], "2024-09-05T09:00:00+00:00", None),
        (CAMILLE + 4, "gaelle", "Gaëlle", vec![], "2025-03-14T09:00:00+00:00", None),
        (1_000_000_000_000_000_399, "musique", "Musique", vec![R_BOT], "2023-01-01T09:00:00+00:00", None),
        (ME, "moi", "Moi", vec![R_ADMIN], "2022-12-01T09:00:00+00:00", None),
    ];
    let members: Vec<Value> = people
        .iter()
        .map(|(id, user, name, roles, joined, nick)| {
            json!({ "user": { "id": id.to_string(), "username": user, "global_name": name, "avatar": null, "discriminator": "0" }, "roles": roles.iter().map(u64::to_string).collect::<Vec<_>>(), "joined_at": joined, "nick": nick, "deaf": false, "mute": false })
        })
        .collect();
    let mut g = fixture("guild_create_beta");
    g["d"]["id"] = ATELIER.to_string().into();
    g["d"]["name"] = "Atelier".into();
    g["d"]["owner_id"] = ME.to_string().into();
    g["d"]["premium_tier"] = 2.into();
    g["d"]["roles"] = roles.into();
    g["d"]["members"] = members.into();
    g["d"]["member_count"] = 7.into();
    g["d"]["channels"][0]["id"] = 3_000_000_000_000_000_201u64.to_string().into();
    for e in packet(&g) {
        s.apply(e);
    }
}

fn servers_store(s: &mut Store) {
    use ferrocord::state::usersettings::proto::{wrapper, Field, Msg, Value as P};
    let names = ["Jeux de société", "Rétro gaming", "Club photo", "Travail"];
    let ids: Vec<u64> = (0..4).map(|i| 2_000_000_000_000_000_101 + i).collect();
    for (i, (id, name)) in ids.iter().zip(names).enumerate() {
        let mut g = fixture("guild_create_beta");
        g["d"]["id"] = id.to_string().into();
        g["d"]["name"] = name.into();
        g["d"]["channels"][0]["id"] = (3_000_000_000_000_000_101 + i as u64).to_string().into();
        for e in packet(&g) {
            s.apply(e);
        }
    }
    // Deux mentions dans le dossier « Jeux » : la pastille les cumule.
    for (n, i) in [0u64, 1].iter().enumerate() {
        let mut m = fixture("message_create_simple");
        m["d"]["id"] = (5_000_000_000_000_009_000 + n as u64).to_string().into();
        m["d"]["channel_id"] = (3_000_000_000_000_000_101 + i).to_string().into();
        m["d"]["guild_id"] = ids[*i as usize].to_string().into();
        m["d"]["mentions"] = serde_json::json!([{ "id": ME.to_string(), "username": "moi" }]);
        for e in packet(&m) {
            s.apply(e);
        }
    }
    let folder = |guilds: &[u64], id: u64, name: &str, color: Option<u64>| {
        let mut f = Msg::default();
        f.set(1, P::Len(guilds.iter().flat_map(|g| g.to_le_bytes()).collect()));
        f.set(2, P::Len(wrapper(P::Varint(id)).encode()));
        f.set(3, P::Len(wrapper(P::Len(name.as_bytes().to_vec())).encode()));
        if let Some(c) = color {
            f.set(4, P::Len(wrapper(P::Varint(c)).encode()));
        }
        Field { num: 1, value: P::Len(f.encode()) }
    };
    let lone = |g: u64| {
        let mut f = Msg::default();
        f.set(1, P::Len(g.to_le_bytes().to_vec()));
        Field { num: 1, value: P::Len(f.encode()) }
    };
    let mut gf = Msg::default();
    gf.fields.push(lone(ALPHA));
    gf.fields.push(folder(&ids[..3], 11, "Jeux", Some(0xe6_7e_22)));
    gf.fields.push(folder(&[2_000_000_000_000_000_002, ids[3]], 12, "Travail", None));
    let mut root = Msg::default();
    root.set(14, P::Len(gf.encode()));
    let ev = ferrocord::state::usersettings::AccountEvent::Proto { bytes: Some(root.encode()), partial: true };
    s.apply(Event::Account(ev));
}

/// Scène `vocal-rail` (lot « vocal ») : le rail de la scène `serveurs`, où le serveur sans
/// icône « Projet.cool » (dans le dossier déplié) accueille trois amis en vocal, dont une
/// qui partage son écran, et une inconnue qui ne compte pas ; « Rétro gaming » (dossier
/// replié) n'a que des inconnus en vocal : pas de pastille.
fn vocal_rail_store(s: &mut Store) {
    use ferrocord::state::model::Avatar;
    use ferrocord::state::relations::{Person, RelKind, Relation, RelationsEvent};
    use serde_json::json;
    for (id, name, username) in [(1_000_000_000_000_000_002u64, "Alice", "alice_fictive"), (1_000_000_000_000_000_005, "Dana", "dana_fictive"), (1_000_000_000_000_000_006, "Ève", "eve_fictive")] {
        let user = Person { id, name: name.into(), username: username.into(), avatar: Avatar::default_for(id) };
        s.apply(Event::Relations(RelationsEvent::RelationAdd(Relation { id, kind: RelKind::Friend, nickname: None, ignored: false, stranger: false, user: Some(user) })));
    }
    let state = |user: u64, channel: u64, stream: bool| json!({ "user_id": user.to_string(), "channel_id": channel.to_string(), "session_id": format!("s-{user}"), "deaf": false, "mute": false, "self_deaf": false, "self_mute": false, "self_video": false, "self_stream": stream, "suppress": false, "request_to_speak_timestamp": null });
    let voice = |id: u64, name: &str| json!({ "id": id.to_string(), "type": 2, "name": name, "position": 1, "parent_id": null, "permission_overwrites": [], "bitrate": 64000, "user_limit": 0, "rtc_region": null, "flags": 0 });
    for (guild, name, text, vocal, states) in [
        (2_000_000_000_000_000_104u64, "Projet.cool", 3_000_000_000_000_000_104u64, 3_000_000_000_000_000_204u64, vec![(1_000_000_000_000_000_002u64, false), (1_000_000_000_000_000_005, false), (1_000_000_000_000_000_006, true), (1_000_000_000_000_000_007, false)]),
        (2_000_000_000_000_000_102, "Rétro gaming", 3_000_000_000_000_000_102, 3_000_000_000_000_000_202, vec![(1_000_000_000_000_000_007, true)]),
    ] {
        let mut g = fixture("guild_create_beta");
        g["d"]["id"] = guild.to_string().into();
        g["d"]["name"] = name.into();
        g["d"]["channels"][0]["id"] = text.to_string().into();
        if let Some(list) = g["d"]["channels"].as_array_mut() {
            list.push(voice(vocal, "Atelier"));
        }
        g["d"]["voice_states"] = states.iter().map(|(u, live)| state(*u, vocal, *live)).collect::<Vec<_>>().into();
        for e in packet(&g) {
            s.apply(e);
        }
    }
}

/// Scènes `salons*` (lot A2) : un rôle Modération, #général refusé en
/// écriture à @everyone et ouvert à Modération.
fn salons_store(s: &mut Store) {
    use ferrocord::state::model::{Overwrite, OverwriteKind, Role};
    use ferrocord::state::Permissions as P;
    const MODERATION_ROLE: u64 = 2_000_000_000_000_009_001;
    s.apply(Event::RoleUpsert { guild_id: ALPHA, role: Role { id: MODERATION_ROLE, name: "Modération".into(), color: 0x2e_cc_71, position: 1, permissions: P::NONE, mentionable: false } });
    let channel = s.guilds.iter().find(|g| g.id == ALPHA).and_then(|g| g.channels.iter().find(|c| c.id == GENERAL).cloned());
    if let Some(mut c) = channel {
        c.overwrites = Some(vec![
            Overwrite { id: ALPHA, kind: OverwriteKind::Role, allow: P::NONE, deny: P::SEND_MESSAGES | P::MENTION_EVERYONE },
            Overwrite { id: MODERATION_ROLE, kind: OverwriteKind::Role, allow: P::SEND_MESSAGES | P::MANAGE_MESSAGES, deny: P::NONE },
        ]);
        s.apply(Event::ChannelUpsert { guild_id: ALPHA, channel: c });
    }
}

/// Scène `vocal` (phase 4) : serveur du jeu `guild_create_voice` passé par
/// le vrai décodeur, mon arrivée dans « Salon vocal », connexion établie
/// (ping 38 ms), Alice et Dana qui parlent, micro coupé.
fn voice_store(s: &mut Store) {
    use ferrocord::state::voice::VoiceEvent;
    const VOCAL: u64 = 2_000_000_000_000_000_004;
    for name in ["guild_create_voice", "voice_state_update"] {
        for e in packet(&fixture(name)) {
            s.apply(e);
        }
    }
    s.apply(Event::Voice(VoiceEvent::Connected));
    s.apply(Event::Voice(VoiceEvent::Ping(38)));
    for user_id in [1_000_000_000_000_000_002, 1_000_000_000_000_000_005] {
        s.apply(Event::Voice(VoiceEvent::Speaking { user_id, speaking: true }));
    }
    s.voice_set_mute(true);
    s.select_guild(VOCAL);
    s.select_channel(3_000_000_000_000_000_050);
    s.take_commands();
    s.apply(Event::History { channel_id: 3_000_000_000_000_000_050, before: None, messages: Vec::new() });
}

/// Scène `vocal-salon` (lot « vocal ») : la scène `vocal`, où Ève diffuse un jeu depuis
/// vingt-cinq minutes (étiquette EN DIRECT, carte d'aperçu au survol) et Dana porte un tag
/// de serveur ; les membres sont rangés comme dans le client.
fn vocal_salon_store(s: &mut Store) {
    use serde_json::json;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let started = now.saturating_sub((25 * 60 + 3) * 1000);
    let packets = [
        json!({ "t": "PRESENCE_UPDATE", "d": {
            "guild_id": VOICE_GUILD.to_string(), "user": { "id": "1000000000000000006", "username": "eve_fictive", "global_name": "Ève", "discriminator": "0" }, "status": "online", "client_status": { "desktop": "online" },
            "activities": [{ "type": 0, "name": "Forge Lunaire", "application_id": "7000000000000004001", "created_at": started, "timestamps": { "start": started } }]
        } }),
        json!({ "t": "PRESENCE_UPDATE", "d": {
            "guild_id": VOICE_GUILD.to_string(), "status": "online", "client_status": { "desktop": "online" }, "activities": [],
            "user": { "id": "1000000000000000005", "username": "dana_fictive", "global_name": "Dana", "discriminator": "0", "primary_guild": { "identity_guild_id": VOICE_GUILD.to_string(), "identity_enabled": true, "tag": "LUNE", "badge": "7a9000000000000000000000000000b9" } }
        } }),
    ];
    for e in packets.iter().flat_map(packet) {
        s.apply(e);
    }
}

/// Serveur et salon vocaux des jeux d'essai (`guild_create_voice`).
const VOICE_GUILD: u64 = 2_000_000_000_000_000_004;
const VOICE_ROOM: u64 = 3_000_000_000_000_000_051;
const EVE: u64 = 1_000_000_000_000_000_006;

/// Scènes `appel*` (phase 5) : ma caméra allumée, direct d'Ève regardé et
/// mis en avant, ou mon partage en cours ; directs passés par le vrai décodeur.
fn call_video_store(s: &mut Store, scene: Option<Scene>) {
    use ferrocord::state::stream::{stream_key, StreamEvent};
    use ferrocord::state::video::Tile;
    s.video_set_camera(true);
    match scene {
        Some(Scene::AppelDirect) => {
            let key = stream_key(Some(VOICE_GUILD), VOICE_ROOM, EVE);
            s.video_watch(&key);
            for name in ["stream_create", "stream_update"] {
                for e in packet(&fixture(name)) {
                    s.apply(e);
                }
            }
            s.apply(Event::Stream(StreamEvent::Connected { key: key.clone() }));
            s.video_focus(Some(Tile::Stream(key)));
        }
        Some(Scene::AppelPartage) => {
            s.video_start_share("ecran-1", "Écran 1 (2560 × 1440)", ferrocord::video::Quality { width: 1920, height: 1080, fps: 30 }, true);
        }
        _ => {}
    }
    s.take_commands();
}

/// Pousse des mires animées dans les tuiles à vidéo, à 30 images/s, comme le
/// câblage (M5) le fera avec les images décodées. Pour une capture, la mire
/// s'arrête après 2 s : l'image devient stable.
fn spawn_test_patterns(sink: ferrocord::state::video::VideoSink, stop: bool) {
    use ferrocord::state::video::{test_pattern, Tile};
    let tiles = [
        (Tile::Camera(1_000_000_000_000_000_005), (640, 360)),
        (Tile::Camera(ME), (640, 360)),
        (Tile::Stream(ferrocord::state::stream::stream_key(Some(VOICE_GUILD), VOICE_ROOM, EVE)), (1280, 720)),
        (Tile::Stream(ferrocord::state::stream::stream_key(Some(VOICE_GUILD), VOICE_ROOM, ME)), (1280, 720)),
        (Tile::Stream(ferrocord::state::stream::stream_key(None, DM, 1_000_000_000_000_000_002)), (1280, 720)),
    ];
    std::thread::spawn(move || {
        let started = Instant::now();
        let mut step = 0u32;
        while !stop || started.elapsed() < Duration::from_secs(2) {
            for (i, (tile, (w, h))) in tiles.iter().enumerate() {
                sink.push(tile.clone(), test_pattern(*w, *h, step, i as u64 + 1));
            }
            step = step.wrapping_add(1);
            std::thread::sleep(Duration::from_millis(33));
        }
    });
}

/// Scène `vocal-codes` (lot V7) : codes de vérification DAVE reçus (fictifs).
fn voice_codes_store(s: &mut Store) {
    use ferrocord::state::voice::{VoiceCodes, VoiceEvent};
    let code = |seed: u64, groups: usize| (0..groups).map(|i| format!("{:05}", (seed * 7919 + i as u64 * 104_729) % 100_000)).collect::<Vec<_>>().join(" ");
    s.apply(Event::Voice(VoiceEvent::Codes(VoiceCodes {
        call: Some(code(1, 6)),
        users: vec![(1_000_000_000_000_000_002, Some(code(2, 9))), (1_000_000_000_000_000_005, Some(code(5, 9)))],
    })));
}

/// Réponses fictives aux actions sur les salons (aucun réseau ; rien n'est créé ni supprimé).
fn chan_reply(c: ferrocord::state::admin::channels::ChanCmd) -> Event {
    use ferrocord::state::admin::channels::{ChanAction, ChanCmd as C, ChanEvent as E, ChanFailure, Details, Region};
    Event::ChannelAdmin(match c {
        // Salon vocal du jeu `guild_create_voice` (scène `salon-vocal`).
        C::Load { channel_id } if channel_id == VOICE_ROOM => E::Details(Details {
            id: channel_id,
            guild_id: Some(VOICE_GUILD),
            kind: 2,
            name: "Salon vocal".into(),
            bitrate: Some(64_000),
            user_limit: Some(8),
            rtc_region: Some("rotterdam".into()),
            video_quality: Some(1),
            ..Details::default()
        }),
        C::Regions => E::Regions(
            [("brazil", "Brazil"), ("rotterdam", "Rotterdam"), ("us-east", "US East"), ("japan", "Japan")].into_iter().map(|(id, name)| Region { id: id.into(), name: name.into() }).collect(),
        ),
        C::Load { channel_id } => E::Details(Details {
            id: channel_id,
            guild_id: Some(ALPHA),
            kind: 0,
            name: "général".into(),
            topic: "Discussions générales du serveur. Soyez courtois.".into(),
            slowmode: 10,
            auto_archive: Some(1440),
            ..Details::default()
        }),
        C::Modify { channel_id, .. } => E::Saved { channel_id },
        C::Positions { guild_id, .. } => E::Moved { guild_id },
        C::Create { .. } => E::Failed { action: ChanAction::Create, failure: ChanFailure::Problem(ferrocord::state::Problem::MfaRequired) },
        C::Delete { .. } => E::Failed { action: ChanAction::Delete, failure: ChanFailure::Problem(ferrocord::state::Problem::MfaRequired) },
    })
}

/// Scène `sondages` (lot T13) : un sondage à choix multiples avec mon vote,
/// son résultat, un message épinglé, une annonce venue d'un salon suivi
/// avec une invitation, et une réponse de commande.
fn polls_store(s: &mut Store) {
    let mut pinned = fixture("message_create_simple");
    pinned["d"]["id"] = "5000000000000000405".into();
    pinned["d"]["type"] = 6.into();
    pinned["d"]["content"] = "".into();
    pinned["d"]["message_reference"] = serde_json::json!({ "type": 0, "channel_id": GENERAL.to_string(), "message_id": "5000000000000000400" });
    let mut command = fixture("message_create_simple");
    command["d"]["id"] = "5000000000000000406".into();
    command["d"]["type"] = 20.into();
    command["d"]["content"] = "Le lancer de dé donne 4.".into();
    command["d"]["interaction_metadata"] = serde_json::json!({ "id": "5000000000000000407", "type": 2, "name": "dé", "user": { "id": "1000000000000000003", "username": "bob_fictif", "global_name": "Bob" } });
    let mut events = Vec::new();
    for name in ["message_create_poll", "message_create_poll_result"] {
        events.extend(packet(&fixture(name)));
    }
    events.extend(packet(&pinned));
    events.extend(packet(&fixture("message_create_crosspost")));
    events.extend(packet(&command));
    for e in events {
        s.apply(e);
    }
    s.select_channel(GENERAL);
}

/// Scène `emojis` : messages avec emojis (texte, en grand, réactions),
/// autocollants, emoji et autocollants du serveur.
/// Scènes `parametres-*` (lot A6) : Alpha au niveau de boost 1, deux
/// autocollants dans la liste du serveur ; les listes REST (auteurs, sons)
/// viennent des jeux d'essai quand la page les demande.
fn expressions_store(s: &mut Store) {
    use ferrocord::state::emoji::{GuildSticker, StickerEvent, StickerFormat};
    // Niveau de boost : `Guild::premium_tier`, seule source.
    if let Some(g) = s.guilds.iter_mut().find(|g| g.id == ALPHA) {
        g.premium_tier = 1;
    }
    let stickers = vec![
        GuildSticker { id: 7_000_000_000_000_000_701, name: "Salut".into(), format: StickerFormat::Png, available: true, tags: String::new() },
        GuildSticker { id: 7_000_000_000_000_000_702, name: "Danse".into(), format: StickerFormat::Gif, available: true, tags: String::new() },
    ];
    s.apply(Event::Stickers(StickerEvent::Guild { guild_id: ALPHA, stickers }));
}

/// GIF synthétiques de l'aperçu (lot G) : le jeu d'essai, complété pour remplir la grille.
fn preview_gifs(seed: &str) -> Vec<ferrocord::state::gifs::Gif> {
    let mut list = ferrocord::net::decode_gif_list(&fixture("rest_gifs_search").to_string()).unwrap_or_default();
    let sizes = [(220, 165), (220, 290), (220, 140), (220, 180), (220, 124), (220, 250), (220, 220), (220, 150)];
    for (i, (width, height)) in sizes.into_iter().cycle().take(14).enumerate() {
        list.push(ferrocord::state::gifs::Gif {
            id: format!("90000000000000001{i:02}"),
            url: format!("https://tenor.com/view/{seed}-synthetique-{i}"),
            src: format!("https://media.tenor.com/{seed}{i:02}AAAAM/{seed}.gif"),
            gif_src: format!("https://media.tenor.com/{seed}{i:02}AAAAC/{seed}.gif"),
            width,
            height,
        });
    }
    list
}

/// Réponses de l'aperçu aux commandes du lot G : catégories et GIF du jeu
/// d'essai, favoris et fréquence synthétiques, écritures acceptées.
fn gifs_reply(cmd: ferrocord::state::gifs::GifCmd) -> Vec<Event> {
    use ferrocord::state::frecency::{synthetic, FavGif};
    use ferrocord::state::gifs::{Category, GifCmd, GifEvent};
    use ferrocord::state::usersettings::ProtoReply;
    let ev = match cmd {
        GifCmd::Categories { .. } => {
            let (mut cats, now) = ferrocord::net::decode_gif_trending(&fixture("rest_gifs_trending").to_string()).unwrap_or_default();
            for name in ["oui", "non", "bisous", "danse", "au revoir", "surprise", "bonjour", "dodo"] {
                cats.push(Category { name: name.into(), src: format!("https://media.tenor.com/categorie{}AAAAM/c.gif", name.len() * 7 + cats.len()) });
            }
            GifEvent::Categories(Ok((cats, now.into_iter().next().map(|g| g.src))))
        }
        GifCmd::Trending { round, .. } => GifEvent::Results { round, gifs: Ok(preview_gifs("tendance")) },
        GifCmd::Search { round, query, .. } => GifEvent::Results { round, gifs: Ok(if query == "rien" { Vec::new() } else { preview_gifs("recherche") }) },
        GifCmd::Select { .. } => return Vec::new(),
        GifCmd::LoadFrecency => {
            let fav = |n: u32, w: u32, h: u32, video: bool| FavGif { url: format!("https://tenor.com/view/favori-synthetique-{n}"), src: format!("https://media.tenor.com/favori{n:02}AAAAC/favori.gif"), width: w, height: h, order: n, video };
            let gifs = [fav(1, 220, 124, false), fav(2, 220, 220, false), fav(3, 220, 165, true), fav(4, 220, 260, false), fav(5, 220, 140, false)];
            let frequent = [("joy", 900), ("7000000000000000501", 700), ("heart", 650), ("thumbsup", 600), ("fire", 500), ("eyes", 420), ("tada", 400), ("sob", 380), ("pray", 300), ("thinking", 250), ("100", 200), ("wave", 150)];
            GifEvent::Frecency { bytes: Some(synthetic(&gifs, &["7000000000000000501", "crab", "sparkles", "coffee", "rocket"], &frequent, 1)), partial: false }
        }
        GifCmd::PatchFrecency { id, .. } => GifEvent::FrecencyWritten { id, result: Ok(ProtoReply { bytes: None, out_of_date: false }) },
        GifCmd::SuperReact { channel_id, message_id, emoji, add } => {
            let echo = SocialEvent::Reaction { channel_id, message_id, user_id: ME, emoji: emoji.clone(), burst: true, add };
            return vec![Event::Social(echo), Event::Gifs(GifEvent::SuperReactDone { message_id, emoji, error: None })];
        }
    };
    vec![Event::Gifs(ev)]
}

fn emojis_store(s: &mut Store) {
    use ferrocord::state::emoji::{GuildSticker, StickerEvent, StickerFormat};
    use ferrocord::state::model::Emoji;
    if let Some(g) = s.guilds.iter_mut().find(|g| g.id == ALPHA) {
        g.emojis.push(Emoji { id: 7_000_000_000_000_000_501, name: "ferris".into(), animated: false, available: true, roles: Vec::new() });
    }
    let stickers = vec![
        GuildSticker { id: 7_000_000_000_000_000_601, name: "Coucou".into(), format: StickerFormat::Png, available: true, tags: "danser".into() },
        GuildSticker { id: 7_000_000_000_000_000_602, name: "Danse".into(), format: StickerFormat::Lottie, available: true, tags: String::new() },
    ];
    s.apply(Event::Stickers(StickerEvent::Guild { guild_id: ALPHA, stickers }));
    let texts = [
        ("5000000000000000950", "**Vrai gras**, *italique*, ***les deux*** et des emojis 🔥 👍🏽 ❤️ <:ferris:7000000000000000501> dans le texte, même `🔥` en code.", Value::Null),
        ("5000000000000000951", "🎉🦀🚀", Value::Null),
        ("5000000000000000952", "", serde_json::json!([{ "id": "7000000000000000601", "name": "Coucou", "format_type": 1 }, { "id": "7000000000000000602", "name": "Danse", "format_type": 3 }])),
    ];
    for (id, content, stickers) in texts {
        let mut m = fixture("message_create_simple");
        m["d"]["id"] = id.into();
        m["d"]["content"] = content.into();
        m["d"]["sticker_items"] = stickers;
        if id.ends_with("950") {
            m["d"]["reactions"] = serde_json::json!([
                { "emoji": { "id": null, "name": "🔥" }, "count": 3, "count_details": { "normal": 3, "burst": 0 }, "me": true, "me_burst": false },
                { "emoji": { "id": "7000000000000000501", "name": "ferris" }, "count": 1, "count_details": { "normal": 1, "burst": 0 }, "me": false, "me_burst": false }
            ]);
        }
        for e in packet(&m) {
            s.apply(e);
        }
    }
}

/// Scène `saisie` : mode lent de 30 s sur #général, deux personnes qui
/// écrivent, un emoji du serveur.
fn saisie_store(s: &mut Store) {
    use ferrocord::state::model::Emoji;
    if let Some(g) = s.guilds.iter_mut().find(|g| g.id == ALPHA) {
        if let Some(c) = g.channels.iter_mut().find(|c| c.id == GENERAL) {
            c.slowmode = 30;
        }
        g.emojis.push(Emoji { id: 7_000_000_000_000_000_501, name: "ferris".into(), animated: false, available: true, roles: Vec::new() });
    }
    s.apply(Event::TypingStart { channel_id: GENERAL, user_id: 1_000_000_000_000_000_002, name: Some("Alice".into()) });
    s.apply(Event::TypingStart { channel_id: GENERAL, user_id: 1_000_000_000_000_000_003, name: Some("bob_fictif".into()) });
}

/// Connexion simulée : répond aux demandes d'historique avec les jeux de
/// données et renvoie les messages envoyés comme s'ils étaient publiés.
struct Offline {
    connected: bool,
    queue: Vec<Event>,
    wake: Option<Wake>,
    next_id: u64,
    view: View,
    /// Événements versés par le banc de mesure (messages en direct).
    inject: Option<Arc<Mutex<Vec<Event>>>>,
    /// Historique complet de quelques salons (ordre croissant), servi page
    /// par page comme le ferait Discord (`before`, `after`, dernière page).
    pages: HashMap<u64, Vec<Message>>,
    /// Pages servies (vers le passé, vers le présent), comptées pour le banc.
    served: Option<bench::Served>,
    /// Banc d'usage prolongé : page d'un salon fabriquée à la demande
    /// (salon, `before`), pour tout salon absent de `pages`.
    make: Option<MakePage>,
}

/// Fabrique d'une page d'historique : salon, `before`.
type MakePage = fn(u64, Option<u64>) -> Vec<Message>;

/// Filtre des réveils publié par l'interface (`Backend::set_interest`), lu par le banc.
static INTEREST: OnceLock<Arc<ferrocord::state::interest::Interest>> = OnceLock::new();

/// Historique hors ligne de l'aperçu (lot H) : l'instantané passe par le vrai
/// chiffrement et le vrai décodage, en mémoire (aucun fichier, aucune clé DPAPI).
struct PreviewOffline(Option<ferrocord::state::offline::Snapshot>, Vec<ferrocord::offline::OfflineEvent>);

impl ferrocord::offline::OfflineDisk for PreviewOffline {
    fn load(&mut self, user: u64) {
        let at = Instant::now();
        let snapshot = self.0.clone().and_then(|mut s| {
            s.me.id = user;
            let (_, key) = ferrocord::offline::crypt::Key::random()?;
            let sealed = ferrocord::offline::crypt::seal(&key, user, s.encode())?;
            ferrocord::state::offline::Snapshot::decode(&ferrocord::offline::crypt::open(&key, user, &sealed)?)
        });
        self.1.push(ferrocord::offline::OfflineEvent::Loaded { user, snapshot: snapshot.map(|s| Box::new(s.prepare())), took: at.elapsed() });
    }
    fn save(&mut self, _: u64, _: ferrocord::state::offline::Snapshot) {}
    fn purge(&mut self, _: u64) {}
    fn keep_only(&mut self, _: Vec<u64>) {}
    fn purge_all(&mut self) {}
    fn flush(&mut self, _: Duration) {}
    fn poll(&mut self) -> Vec<ferrocord::offline::OfflineEvent> {
        std::mem::take(&mut self.1)
    }
}

/// Page de `all` (ordre croissant) demandée avec `before` ou `after`, ou la dernière.
fn serve_page(all: &[Message], before: Option<u64>, after: Option<u64>) -> Vec<Message> {
    match (before, after) {
        (_, Some(a)) => all.iter().filter(|m| m.id > a).take(HISTORY_PAGE).cloned().collect(),
        (Some(b), None) => all.iter().rev().filter(|m| m.id < b).take(HISTORY_PAGE).cloned().collect(),
        (None, None) => all[all.len().saturating_sub(HISTORY_PAGE)..].to_vec(),
    }
}

impl Offline {
    fn push(&mut self, e: Event) {
        self.queue.push(e);
        if let Some(w) = &self.wake {
            w();
        }
    }
}

impl Backend for Offline {
    fn set_wake(&mut self, wake: Wake) {
        self.wake = Some(wake);
    }
    // Banc : les MESSAGE_UPDATE injectés au repos passent par le même filtre que la passerelle (lot C9).
    fn set_interest(&mut self, interest: Arc<ferrocord::state::interest::Interest>) {
        let _ = INTEREST.set(interest);
    }
    fn connect(&mut self, token: Token) {
        self.connected = true;
        if self.view == View::Connecting {
            return;
        }
        self.push(Event::PeopleHub(people()));
        // Vue `essai-fichier` : Discord refuse le jeton du coffre.
        if self.view == View::FileTry && token.expose() == "faux.jeton.coffre-5" {
            self.connected = false;
            self.push(Event::Fatal { problem: Problem::BadToken, bad_token: true });
            return;
        }
        for e in session_events(self.view) {
            self.push(e);
        }
    }
    fn disconnect(&mut self) {
        self.connected = false;
    }
    fn is_connected(&self) -> bool {
        self.connected
    }
    fn poll(&mut self) -> Vec<Event> {
        if let Some(q) = &self.inject
            && let Ok(mut q) = q.lock()
        {
            self.queue.append(&mut q);
        }
        std::mem::take(&mut self.queue)
    }
    fn send(&mut self, cmds: Vec<Cmd>) {
        for cmd in cmds {
            match cmd {
                // Vue `connexion` : pas de réseau, l'historique hors ligne reste seul (lot H).
                Cmd::History { channel_id, before } if self.view == View::Connecting => {
                    self.push(Event::HistoryError { channel_id, before, after: None, error: Problem::Network("aperçu hors ligne".into()) });
                }
                Cmd::History { channel_id, before } => {
                    if let (Some(s), Some(_)) = (&self.served, before) {
                        s.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                    let messages = match self.pages.get(&channel_id) {
                        Some(all) => serve_page(all, before, None),
                        None if self.make.is_some() => self.make.map(|f| f(channel_id, before)).unwrap_or_default(),
                        None if before.is_none() => fixture_history(channel_id),
                        None => Vec::new(),
                    };
                    self.push(Event::History { channel_id, before, messages });
                }
                Cmd::HistoryAfter { channel_id, after } => {
                    if let Some(s) = &self.served {
                        s.1.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                    let messages = self.pages.get(&channel_id).map(|all| serve_page(all, None, Some(after))).unwrap_or_default();
                    self.push(Event::HistoryAfter { channel_id, after, messages });
                }
                Cmd::Account(c) => {
                    for e in account_reply(c) {
                        self.push(e);
                    }
                }
                // Écritures sociales et accusés : sans effet dans l'aperçu.
                Cmd::Relations(_) => {}
                // Fils : sans effet dans l'aperçu.
                // Panneau « Fils » de l'en-tête (phase 10, lot S) : une page de fils archivés.
                Cmd::Threads(ferrocord::state::threads::ThreadCmd::Archived { parent_id, before }) => self.push(archived_reply(parent_id, before)),
                Cmd::Threads(_) => {}
                // Serveurs : réponses fictives (lot A1).
                Cmd::Servers(c) => self.push(Event::Servers(server_reply(c))),
                // Modération : jeux d'essai par les vrais décodeurs (lots A4 + A5).
                Cmd::Moderation(c) => {
                    for e in moderation_reply(c) {
                        self.push(e);
                    }
                }
                // Mentions récentes : la page du jeu d'essai, une seule fois ; retrait accepté.
                Cmd::Inbox(ferrocord::state::inbox::InboxCmd::Load { round, before, .. }) => {
                    let body = serde_json::to_string(&fixture("rest_recent_mentions")).unwrap_or_default();
                    let mut messages = if before.is_none() { ferrocord::net::decode_mentions(&body).unwrap_or_default() } else { Vec::new() };
                    // Mentions inventées de la scène `boite-mentions` (serveur au nom très long).
                    if before.is_none() {
                        messages.extend(boite_mentions());
                    }
                    self.push(Event::Inbox(ferrocord::state::inbox::InboxEvent::Page { round, before, messages }));
                }
                Cmd::Inbox(ferrocord::state::inbox::InboxCmd::Delete { message_id }) => self.push(Event::Inbox(ferrocord::state::inbox::InboxEvent::Deleted { message_id })),
                // Index des commandes : jeu `rest_application_command_index` pour
                // le serveur, vide ailleurs ; une interaction est acceptée, et
                // une commande reste sans réponse de l'application.
                Cmd::Interactions(c) => {
                    use ferrocord::state::interactions::{InteractionCmd as C, InteractionEvent as I, OutKind, Scope};
                    match c {
                        C::Index { scope } => {
                            let index = match scope {
                                Scope::Guild(_) => ferrocord::net::decode_command_index(&fixture("rest_application_command_index").to_string()).unwrap_or_default(),
                                _ => Default::default(),
                            };
                            self.push(Event::Interactions(I::Index { scope, index }));
                        }
                        C::Send(o) => {
                            self.push(Event::Interactions(I::Sent { nonce: o.nonce }));
                            match o.kind {
                                OutKind::Command { .. } => self.push(Event::Interactions(I::Failed { nonce: o.nonce, id: 0, reason: 2 })),
                                OutKind::Autocomplete { .. } => self.push(Event::Interactions(I::Autocomplete { nonce: o.nonce, choices: Vec::new() })),
                                _ => self.push(Event::Interactions(I::Succeeded { nonce: o.nonce, id: 0 })),
                            }
                        }
                    }
                }
                // Lot T13 : seule la carte d'invitation reçoit une réponse.
                Cmd::Invites(ferrocord::state::invites::InviteCmd::Get { code }) => {
                    let invite = ferrocord::net::decode_invite(&code, &fixture("rest_invite").to_string()).ok_or(ferrocord::state::Problem::Http(500));
                    self.push(Event::Invites(ferrocord::state::invites::InviteEvent::Loaded { code, invite }));
                }
                Cmd::Polls(_) | Cmd::Announce(_) | Cmd::Roles(_) => {}
                // Appels, conférences, sons (lot V6).
                c @ (Cmd::Calls(_) | Cmd::Stage(_)) => {
                    for e in calls_reply(c) {
                        self.push(e);
                    }
                }
                // Paramètres du serveur (lot A8 + A9) : jeux d'essai par le vrai décodeur.
                Cmd::GuildAdmin(c) => {
                    if let Some(e) = admin_reply(&c) {
                        self.push(Event::GuildAdmin(e));
                    }
                }
                // Salons (lot A2) : réponses fictives.
                Cmd::ChannelAdmin(c) => self.push(chan_reply(c)),
                // Événements et accueil (lot A7) : réponses tirées des jeux d'essai.
                Cmd::GuildEvents(c) => self.push(Event::GuildEvents(events_reply(c))),
                Cmd::Community(c) => self.push(Event::Community(community_reply(c))),
                // Expressions (lot A6) : listes des jeux d'essai, écritures acceptées.
                Cmd::Expressions(c) => {
                    use ferrocord::state::admin::expressions::{ExprCmd, Kind};
                    let body = match &c {
                        ExprCmd::List { kind: Kind::Emoji, .. } => fixture("rest_guild_emojis").to_string(),
                        ExprCmd::List { kind: Kind::Sticker, .. } => fixture("rest_guild_stickers").to_string(),
                        ExprCmd::List { kind: Kind::Sound, .. } => fixture("rest_guild_soundboard_sounds").to_string(),
                        _ => String::new(),
                    };
                    if let Some(e) = ferrocord::net::decode_expressions(&c, &body) {
                        self.push(Event::Expressions(e));
                    }
                }
                // Vocal (phase 4) : modération acceptée ; rien d'autre ne répond hors ligne.
                Cmd::Voice(ferrocord::state::voice::VoiceCmd::Moderate { guild_id, user_id, action, .. }) => {
                    self.push(Event::Voice(ferrocord::state::voice::VoiceEvent::Moderated { guild_id, user_id, action }));
                }
                Cmd::Voice(_) => {}
                // Vidéo (phase 5) : rien ne répond hors ligne.
                Cmd::Video(_) => {}
                // Activité (lot C3) : rien ne répond hors ligne.
                Cmd::Activity(_) => {}
                // GIF, favoris du compte, super réaction (lot G) : jeux d'essai synthétiques.
                Cmd::Gifs(c) => {
                    for e in gifs_reply(c) {
                        self.push(e);
                    }
                }
                // Effets de profil et quêtes (lot ND) : jeux d'essai synthétiques, passés par les vrais décodeurs.
                Cmd::Effects(c) => {
                    use ferrocord::state::effects::{EffectCmd, EffectEvent};
                    let e = match c {
                        // Comme Discord : le produit n'est trouvé que par son SKU (lot profil).
                        EffectCmd::Config { id, sku } => {
                            let product = |name: &str| ferrocord::net::decode_effect_product(&fixture(name).to_string(), id, sku).filter(|c| c.sku_id == sku);
                            let owned = || ferrocord::net::decode_owned_effects(&fixture("nd/rest_collectibles_purchases").to_string()).unwrap_or_default().into_iter().find(|c| c.id == id && c.sku_id == sku);
                            EffectEvent::Config { id, result: Ok(product("nd/rest_collectibles_product").or_else(|| product("profil/rest_collectibles_product_effect")).or_else(owned)) }
                        }
                        EffectCmd::Catalog => EffectEvent::Catalog(Ok(ferrocord::net::decode_effect_catalog(&fixture("profil/rest_collectibles_categories_effects").to_string()).unwrap_or_default())),
                        EffectCmd::Owned => {
                            self.push(Event::Effects(EffectEvent::OwnedLooks(ferrocord::net::decode_owned_looks(&fixture("nd/rest_collectibles_purchases").to_string()))));
                            EffectEvent::Owned(Ok(ferrocord::net::decode_owned_effects(&fixture("nd/rest_collectibles_purchases").to_string()).unwrap_or_default()))
                        }
                    };
                    self.push(Event::Effects(e));
                }
                // Choix du tag de serveur (lot ND) : accepté.
                Cmd::Tags(ferrocord::state::tags::TagCmd::Set { guild }) => self.push(Event::Tags(ferrocord::state::tags::TagEvent::Set { guild, result: Ok(()) })),
                Cmd::Tags(ferrocord::state::tags::TagCmd::Wear { slot, sku }) => self.push(Event::Tags(ferrocord::state::tags::TagEvent::Worn { slot, sku, result: Ok(()) })),
                Cmd::Tags(ferrocord::state::tags::TagCmd::Style { style }) => self.push(Event::Tags(ferrocord::state::tags::TagEvent::Styled { style, result: Ok(()) })),
                // Widgets de jeux du profil : jeux d'essai synthétiques ; l'écriture réussit.
                Cmd::Widgets(c) => {
                    use ferrocord::state::widgets::{WidgetCmd, WidgetEvent};
                    let e = match c {
                        WidgetCmd::Games { ids } => WidgetEvent::Games { ids, result: Ok(ferrocord::net::decode_games(&fixture("nd/rest_games").to_string()).unwrap_or_default()) },
                        WidgetCmd::Suggested => WidgetEvent::Suggested(Ok(ferrocord::net::decode_suggested(&fixture("nd/rest_widgets_suggested").to_string()).unwrap_or_default())),
                        WidgetCmd::Save { widgets } => WidgetEvent::Saved(Ok(widgets)),
                    };
                    self.push(Event::Widgets(e));
                }
                // Boutique en consultation (lot ND) : jeu d'essai synthétique.
                Cmd::Shop(_) => self.push(Event::Shop(ferrocord::state::shop::ShopEvent::Loaded(Ok(ferrocord::net::decode_shop(&fixture("nd/rest_collectibles_categories").to_string()).unwrap_or_default())))),
                // Suggestions d'amis (lot ND) : jeu d'essai synthétique ; « Ignorer » ne répond rien.
                Cmd::Suggestions(ferrocord::state::suggestions::SuggestionCmd::List) => self.push(Event::Suggestions(ferrocord::state::suggestions::SuggestionEvent::List(Ok(ferrocord::net::decode_suggestions(&fixture("nd/rest_friend_suggestions").to_string()).unwrap_or_default())))),
                Cmd::Suggestions(_) => {}
                // Messages enregistrés (lot ND) : jeu d'essai synthétique ; les écritures réussissent.
                Cmd::Saved(c) => {
                    use ferrocord::state::saved::{SavedCmd, SavedEvent};
                    self.push(Event::Saved(match c {
                        SavedCmd::List => SavedEvent::List(ferrocord::net::decode_saved_messages(&fixture("nd/rest_saved_messages").to_string()).ok_or(ferrocord::state::Problem::Forbidden)),
                        SavedCmd::Save { due_ms, .. } => SavedEvent::Saved { reminder: due_ms.is_some(), result: Ok(()) },
                        SavedCmd::Remove { channel_id, message_id } => SavedEvent::Removed { channel_id, message_id, result: Ok(()) },
                    }));
                }
                // Centre de sécurité (lot ND) : jeu d'essai synthétique.
                Cmd::SafetyHub(_) => self.push(Event::SafetyHub(ferrocord::state::safety::SafetyEvent::Loaded(ferrocord::net::decode_safety_hub(&fixture("nd/rest_safety_hub").to_string()).ok_or(ferrocord::state::Problem::Forbidden)))),
                // Répertoire d'applications (lot ND) : jeux d'essai synthétiques.
                Cmd::AppDir(ferrocord::state::appdir::AppDirCmd::Categories { .. }) => self.push(Event::AppDir(ferrocord::state::appdir::AppDirEvent::Categories(Ok(ferrocord::net::decode_discovery_categories(&fixture("nd/rest_app_directory_categories").to_string()).unwrap_or_default())))),
                Cmd::AppDir(ferrocord::state::appdir::AppDirCmd::Search { seq, .. }) => self.push(Event::AppDir(ferrocord::state::appdir::AppDirEvent::Results { seq, result: Ok(ferrocord::net::decode_app_directory(&fixture("nd/rest_app_directory_search").to_string()).unwrap_or_default()) })),
                // Vérification des membres (lot ND) : jeux d'essai synthétiques ; l'envoi et les décisions réussissent.
                Cmd::Verification(c) => {
                    use ferrocord::state::verification::{Gate, Status, VerifyCmd, VerifyEvent};
                    self.push(Event::Verification(match c {
                        VerifyCmd::Load { guild_id } => VerifyEvent::Loaded { guild_id, result: Ok(Gate { form: ferrocord::net::decode_member_verification(&fixture("nd/rest_member_verification").to_string()).flatten(), status: Status::Unknown }) },
                        VerifyCmd::Submit { guild_id, .. } => VerifyEvent::Submitted { guild_id, result: Ok(Status::Submitted) },
                        VerifyCmd::Requests { guild_id } => VerifyEvent::Requests { guild_id, result: Ok(ferrocord::net::decode_join_requests(&fixture("nd/rest_join_requests").to_string()).unwrap_or_default()) },
                        VerifyCmd::Action { guild_id, request_id, .. } => VerifyEvent::Actioned { guild_id, request_id, result: Ok(()) },
                    }));
                }
                // Résumés de salon (lot ND) : jeu d'essai synthétique.
                Cmd::Summaries(ferrocord::state::summaries::SummaryCmd::Load { channel_id }) => self.push(Event::Summaries(ferrocord::state::summaries::SummaryEvent::Loaded { channel_id, result: Ok(ferrocord::net::decode_summaries(&fixture("nd/rest_channel_summaries").to_string()).unwrap_or_default()) })),
                // Découverte de serveurs (lot ND) : jeux d'essai synthétiques, quelle que soit la recherche.
                Cmd::Discovery(ferrocord::state::discovery::DiscoveryCmd::Categories { .. }) => self.push(Event::Discovery(ferrocord::state::discovery::DiscoveryEvent::Categories(Ok(ferrocord::net::decode_discovery_categories(&fixture("nd/rest_discovery_categories").to_string()).unwrap_or_default())))),
                Cmd::Discovery(ferrocord::state::discovery::DiscoveryCmd::Search { seq, .. }) => self.push(Event::Discovery(ferrocord::state::discovery::DiscoveryEvent::Results { seq, result: Ok(ferrocord::net::decode_discoverable(&fixture("nd/rest_discoverable_guilds").to_string()).unwrap_or_default()) })),
                // Inventaire (lot ND) : jeu d'essai synthétique, le même pour les deux listes.
                Cmd::Inventory(_) => {
                    let mine = ferrocord::net::decode_entitlements(&fixture("nd/rest_entitlements").to_string()).unwrap_or_default();
                    let giftable = mine.iter().take(1).cloned().collect();
                    self.push(Event::Inventory(ferrocord::state::inventory::InventoryEvent::Loaded(Ok(ferrocord::state::inventory::Owned { mine, giftable }))));
                }
                Cmd::Quests(c) => {
                    use ferrocord::state::quests::{QuestCmd, QuestEvent};
                    match c {
                        QuestCmd::List => {
                            // Les deux jeux d'essai : formes documentées, puis les autres formes connues des images.
                            let (mut list, blocked) = ferrocord::net::decode_quests(&fixture("nd/rest_quests_assets").to_string()).unwrap_or_default();
                            list.extend(ferrocord::net::decode_quests(&fixture("nd/rest_quests").to_string()).unwrap_or_default().0);
                            self.push(Event::Quests(QuestEvent::List(Ok((list, blocked)))));
                            self.push(Event::Quests(QuestEvent::Orbs(1400)));
                        }
                        QuestCmd::Enroll { id } => {
                            if let Some((_, status)) = ferrocord::net::decode_quest_status(&fixture("nd/rest_quest_status").to_string()) {
                                self.push(Event::Quests(QuestEvent::Enrolled { id, result: Ok(status) }));
                            }
                        }
                        QuestCmd::Claim { id } => self.push(Event::Quests(QuestEvent::Claimed { id, result: Ok(None) })),
                        QuestCmd::Code { id } => self.push(Event::Quests(QuestEvent::Code { id, result: Ok("FICTIF-0000-CODE".into()) })),
                    }
                }
                Cmd::Send { channel_id, content, .. } => {
                    self.next_id += 1;
                    let mut m = fixture("message_create_simple");
                    m["d"]["id"] = self.next_id.to_string().into();
                    m["d"]["channel_id"] = channel_id.to_string().into();
                    m["d"]["content"] = content.into();
                    m["d"]["author"]["id"] = "1000000000000000001".into();
                    m["d"]["author"]["global_name"] = "Moi Fictif".into();
                    for e in packet(&m) {
                        self.push(e);
                    }
                }
                Cmd::Social(c) => {
                    for e in social_answer(c, &mut self.next_id) {
                        self.push(e);
                    }
                }
                Cmd::Typing { .. } => {}
                // « Envoyer un message » : MP créé comme le ferait Discord (CHANNEL_CREATE), puis ouvert.
                Cmd::OpenDm { user_id } => {
                    self.next_id += 1;
                    let mut c = fixture("channel_create_dm");
                    c["d"]["id"] = self.next_id.to_string().into();
                    c["d"]["recipients"] = serde_json::json!([{ "id": user_id.to_string(), "username": "utilisateur_fictif" }]);
                    for e in packet(&c) {
                        self.push(e);
                    }
                    self.push(Event::DmOpened { channel_id: self.next_id });
                }
                // Profil complet et note : jeux `rest_user_profile` et
                // `rest_user_note` pour Alice, passés par le vrai décodeur ;
                // les autres gardent ce que les paquets ont appris.
                Cmd::Profile(c) => {
                    use ferrocord::state::profiles::ProfileCmd as P;
                    const ALICE: u64 = 1_000_000_000_000_000_002;
                    match c {
                        P::Fetch { user_id: ALICE, guild_id } => {
                            let mut v = fixture("rest_user_profile");
                            if EFFET.load(std::sync::atomic::Ordering::Relaxed) {
                                v["user_profile"]["profile_effect"] = serde_json::json!({ "id": "6000000000000000001", "expires_at": null });
                            }
                            if SANS_BANNIERE.load(std::sync::atomic::Ordering::Relaxed) {
                                for part in ["user", "user_profile"] {
                                    v[part]["banner"] = serde_json::Value::Null;
                                }
                                v["user_profile"]["theme_colors"] = serde_json::Value::Null;
                            }
                            if carte() {
                                // L'identifiant de l'effet seul (son SKU diffère) : il passe par le catalogue.
                                v["user_profile"]["profile_effect"] = serde_json::json!({ "id": "6000000000000000301", "expires_at": null });
                                v["user_profile"]["theme_colors"] = serde_json::json!([0x7b_3fc4, 0x2a_1250]);
                                v["premium_since"] = "2023-01-10T10:00:00+00:00".into();
                                v["premium_guild_since"] = "2025-05-02T12:00:00+00:00".into();
                                v["badges"] = serde_json::json!([
                                    { "id": "premium_tenure_36_month_v2", "description": "Abonné depuis le 10 janv. 2023", "icon": "2ba85e8026a8614b640c2837bcdfe21b" },
                                    { "id": "guild_booster_lvl5", "description": "Booste des serveurs depuis le 2 mai 2025", "icon": "011940fd013da3f7fb926e4a1cd2e618" },
                                    { "id": "hypesquad_house_2", "description": "HypeSquad Brilliance", "icon": "3aa41de486fa12454c3761e8e223442e" },
                                    { "id": "quest_completed", "description": "A terminé une quête", "icon": "7d9ae358c8c5e118768335dbe68b4fb8" }
                                ]);
                            }
                            if parures() {
                                // Bannière et avatar animés, comme dans l'objet `user` paré.
                                let dressed = fixture("rest_user_cosmetics");
                                v["user"]["avatar"] = dressed["user"]["avatar"].clone();
                                v["user"]["banner"] = dressed["user"]["banner"].clone();
                                v["user_profile"]["banner"] = dressed["user"]["banner"].clone();
                                // Le membre joint au profil porte l'avatar, la bannière et
                                // la décoration de serveur du jeu d'essai.
                                for key in ["avatar", "banner", "avatar_decoration_data"] {
                                    v["guild_member"][key] = dressed["member"][key].clone();
                                }
                                v["guild_member"]["user"] = dressed["user"].clone();
                            }
                            ferrocord::net::decode_profile(&people(), ALICE, guild_id, &v.to_string());
                        }
                        P::Note { user_id: ALICE } => ferrocord::net::decode_note(&people(), ALICE, &fixture("rest_user_note").to_string()),
                        P::Note { user_id } => ferrocord::net::decode_note(&people(), user_id, r#"{ "note": "" }"#),
                        P::SetNote { user_id, .. } => people().lock().profiles_mut().note_written(user_id, ferrocord::state::profiles::NoteWrite::Done),
                        // Mon propre profil complet (carte de mon compte, édition rapide).
                        P::Fetch { user_id: ME, guild_id } => {
                            let theme = match MY_THEME.load(std::sync::atomic::Ordering::Relaxed) {
                                1 => serde_json::Value::Null,
                                2 => serde_json::json!([0xf6e7ff, 0xffd9ec]),
                                // Éditeur de profil : le thème violet de la capture de référence.
                                3 => serde_json::json!([0x1b0b33, 0x8200e0]),
                                _ => serde_json::json!([0x3b2a6b, 0x12343b]),
                            };
                            // Scène « mon-compte-jeu » : huit badges, pour la capsule repliée à six et son « +2 ».
                            let icons = ["2ba85e8026a8614b640c2837bcdfe21b", "011940fd013da3f7fb926e4a1cd2e618", "3aa41de486fa12454c3761e8e223442e", "7d9ae358c8c5e118768335dbe68b4fb8"];
                            let many = MY_THEME.load(std::sync::atomic::Ordering::Relaxed) == 4;
                            let badges: Vec<serde_json::Value> = (0..if many { 8 } else { 0 }).map(|i| serde_json::json!({ "id": format!("badge_fictif_{i}"), "description": format!("Badge fictif n° {}", i + 1), "icon": icons[i % icons.len()] })).collect();
                            let v = serde_json::json!({
                                "user": { "id": ME.to_string(), "username": "moi.fictif", "global_name": "Moi Fictif", "discriminator": "0", "avatar": null, "banner": null, "accent_color": 0x5865f2, "bio": "Profil d'aperçu, données fictives." },
                                "user_profile": { "bio": "Profil d'aperçu, données fictives.", "pronouns": "il", "banner": null, "accent_color": 0x5865f2, "theme_colors": theme },
                                "guild_member_profile": { "guild_id": ALPHA.to_string(), "pronouns": "", "bio": "Ici, je m'occupe des salons.", "banner": null, "accent_color": null, "theme_colors": null },
                                "badges": badges, "guild_badges": [], "mutual_guilds": [], "connected_accounts": [{ "type": "twitch", "id": "1", "name": "moi_fictif", "verified": true }, { "type": "youtube", "id": "UCfictif", "name": "Moi Fictif", "verified": false }],
                                // Widgets de jeux (le dernier du jeu d'essai, de type inconnu, est laissé de côté).
                                "widgets": fixture("nd/rest_widgets").as_array().map(|w| w[..4].to_vec())
                            });
                            ferrocord::net::decode_profile(&people(), ME, guild_id, &v.to_string());
                        }
                        P::Fetch { user_id, guild_id } => people().lock().profiles_mut().set(user_id, guild_id, ferrocord::state::profiles::ProfileState::Unavailable),
                    }
                }
                // Envoi de fichiers : le premier avance, celui dont le texte dit « échec » échoue.
                Cmd::Upload(UploadCmd::Send { channel_id, nonce, content, files, .. }) => {
                    let total: u64 = files.iter().map(|f| f.size).sum();
                    if content.contains("échec") {
                        self.push(Event::Upload(UploadEvent::Failed { channel_id, nonce, failure: Failure::Problem(Problem::Network("connexion interrompue".into())) }));
                    } else {
                        self.push(Event::Upload(UploadEvent::Progress { nonce, sent: total * 42 / 100, total }));
                    }
                }
                Cmd::Upload(UploadCmd::Cancel { nonce }) => {
                    self.push(Event::Upload(UploadEvent::Cancelled { channel_id: GENERAL, nonce }));
                }
                Cmd::Subscribe(sub) => {
                    // Banc : la liste est fabriquée et décodée hors du fil d'interface,
                    // comme le fait le fil réseau de l'application ; sinon ce faux
                    // serveur comptait dans le temps d'image du changement de salon.
                    if let (Some(_), Some(q)) = (&self.served, &self.inject) {
                        let (q, wake) = (q.clone(), self.wake.clone());
                        std::thread::spawn(move || {
                            let events = member_list(&sub);
                            if let Ok(mut q) = q.lock() {
                                q.extend(events);
                            }
                            if let Some(w) = wake {
                                w();
                            }
                        });
                    } else {
                        for e in member_list(&sub) {
                            self.push(e);
                        }
                    }
                }
                // Recherche (lot T12) : toujours le même jeu de résultats ; saut : page autour du message.
                Cmd::Search(SearchCmd::Query { seq, .. }) => {
                    if let Some(e) = ferrocord::net::decode_search(&fixture("rest_search_messages").to_string(), seq) {
                        self.push(e);
                    }
                }
                Cmd::Search(SearchCmd::Around { channel_id, around }) => {
                    let all = self.pages.get(&channel_id).cloned().unwrap_or_else(|| fixture_history(channel_id));
                    let at = all.partition_point(|m| m.id < around);
                    let messages = all[at.saturating_sub(HISTORY_PAGE / 2)..(at + HISTORY_PAGE / 2).min(all.len())].to_vec();
                    self.push(Event::Search(SearchEvent::Around { channel_id, around, messages }));
                }
            }
        }
    }
}

/// Réponses simulées aux actions sur les messages (lots T5/T6), comme si
/// Discord les acceptait toutes.
fn social_answer(cmd: SocialCmd, next_id: &mut u64) -> Vec<Event> {
    let ev = |e: SocialEvent| vec![Event::Social(e)];
    // Message connu de l'aperçu (historique ou réponse des jeux de données).
    let known = |id: u64| -> Option<Value> {
        let key = id.to_string();
        let page = fixture("rest_history");
        let from_page = page.as_array().and_then(|a| a.iter().find(|m| m["id"].as_str() == Some(key.as_str())).cloned());
        from_page.or_else(|| [fixture("message_create_simple"), fixture("message_create_reply")].into_iter().map(|p| p["d"].clone()).find(|m| m["id"].as_str() == Some(key.as_str())))
    };
    match cmd {
        SocialCmd::React { channel_id, message_id, emoji, add } => ev(SocialEvent::ReactDone { channel_id, message_id, emoji, add, error: None }),
        SocialCmd::Reactors { message_id, emoji, burst, .. } => {
            let users = vec![Reactor { id: 1_000_000_000_000_000_002, name: "Alice".into() }, Reactor { id: 1_000_000_000_000_000_004, name: "eve_fictive".into() }];
            ev(SocialEvent::Reactors { message_id, emoji, burst, users: Ok(users) })
        }
        SocialCmd::Edit { channel_id, message_id, content } => vec![Event::MessageUpdate { channel_id, id: message_id, content }],
        SocialCmd::Delete { channel_id, message_id } => vec![Event::MessageDelete { channel_id, id: message_id }],
        SocialCmd::Pin { channel_id, message_id, pin } => ev(SocialEvent::Pinned { channel_id, message_id, pinned: pin }),
        SocialCmd::SuppressEmbeds { channel_id, message_id, .. } => ev(SocialEvent::EmbedsSuppressed { channel_id, message_id }),
        SocialCmd::Pins { channel_id, before } => {
            let body = fixture("rest_pins");
            let items = body["items"].as_array().cloned().unwrap_or_default();
            let messages: Vec<Value> = items.iter().map(|i| i["message"].clone()).collect();
            let decoded = decode_history(&Value::Array(messages).to_string()).unwrap_or_default();
            let page = items.iter().zip(decoded).map(|(i, message)| Pin { pinned_at: i["pinned_at"].as_str().unwrap_or_default().into(), message }).collect();
            ev(SocialEvent::Pins { channel_id, before, pins: Ok((page, false)) })
        }
        SocialCmd::Send { channel_id, content, nonce, kind } => {
            *next_id += 1;
            let mut m = fixture("message_create_simple");
            let d = &mut m["d"];
            d["id"] = next_id.to_string().into();
            d["channel_id"] = channel_id.to_string().into();
            d["content"] = content.into();
            d["nonce"] = nonce.to_string().into();
            d["author"]["id"] = "1000000000000000001".into();
            d["author"]["global_name"] = "Moi Fictif".into();
            match kind {
                SendKind::Reply { message_id, .. } => {
                    d["type"] = 19.into();
                    d["message_reference"] = serde_json::json!({ "type": 0, "message_id": message_id.to_string() });
                    if let Some(r) = known(message_id) {
                        d["referenced_message"] = r;
                    }
                }
                SendKind::Forward { channel_id: from, message_id, .. } => {
                    d["content"] = "".into();
                    d["flags"] = 16384.into();
                    d["message_reference"] = serde_json::json!({ "type": 1, "message_id": message_id.to_string(), "channel_id": from.to_string() });
                    let snapshot = known(message_id).unwrap_or_default();
                    d["message_snapshots"] = serde_json::json!([{ "message": snapshot }]);
                }
                SendKind::Sticker { sticker_id } => {
                    let name = std::mem::take(&mut d["content"]);
                    d["content"] = "".into();
                    d["sticker_items"] = serde_json::json!([{ "id": sticker_id.to_string(), "name": name, "format_type": 1 }]);
                }
                // Un message vocal part par la file des envois de fichiers, pas par ici.
                SendKind::Voice { .. } => {}
            }
            packet(&m)
        }
    }
}

/// Liste des membres fictive d'un serveur, comme la passerelle la
/// renverrait à une souscription : un rôle affiché à part (créé au passage,
/// coloré), puis en ligne, puis hors ligne ; une SYNC par plage demandée.
fn member_list(sub: &ferrocord::state::members::Subscription) -> Vec<Event> {
    use serde_json::json;
    const ROLE: &str = "2000000000000001010";
    let Some((_, ranges)) = &sub.channel else { return Vec::new() };
    let mut out = packet(&json!({ "t": "GUILD_ROLE_CREATE", "d": { "guild_id": sub.guild_id.to_string(), "role": {
        "id": ROLE, "name": "Modération", "permissions": "0", "position": 2, "color": 0x2ecc71, "hoist": true,
        "colors": { "primary_color": 0x2ecc71, "secondary_color": if parures() { json!(0x00a8fc) } else { Value::Null }, "tertiary_color": null } } } }));
    let names = ["Alice", "Bob", "Carol", "Ève", "Gaspard", "Héloïse", "Inès", "Jules", "Karim", "Léa", "Mathis", "Noémie", "Oscar", "Pauline"];
    let activities = [
        json!([{ "type": 0, "name": "Échecs" }]),
        json!([{ "type": 4, "name": "Custom Status", "state": "De retour dans 5 min" }]),
        json!([{ "type": 2, "name": "Radio fictive" }]),
        json!([]),
        json!([{ "type": 3, "name": "Un documentaire" }]),
        json!([]),
    ];
    let member = |n: usize, status: &str, roles: Vec<&str>| {
        let id = format!("{}", 1_000_000_000_000_001_000u64 + n as u64);
        let name = format!("{} {}", names[n % names.len()], n / names.len() + 1);
        let mut v = json!({ "member": { "user": { "id": id, "username": name.to_lowercase(), "global_name": name, "avatar": if n.is_multiple_of(3) { Value::Null } else { "a11ce0000000000000000000000000a1".into() }, "bot": n == 7 },
            "roles": roles, "presence": { "status": status, "activities": if status == "offline" { json!([]) } else { activities[n % activities.len()].clone() } } } });
        if parures() {
            member_cosmetics(n, &mut v["member"]["user"]);
        }
        v
    };
    let (mods, online, offline) = (2usize, 40usize, 260usize);
    let mut items = vec![json!({ "group": { "id": ROLE } })];
    items.extend((0..mods).map(|n| member(n, "online", vec![ROLE])));
    items.push(json!({ "group": { "id": "online" } }));
    items.extend((mods..mods + online).map(|n| member(n, ["online", "idle", "dnd", "online"][n % 4], Vec::new())));
    items.push(json!({ "group": { "id": "offline" } }));
    items.extend((mods + online..mods + online + offline).map(|n| member(n, "offline", Vec::new())));
    let ops: Vec<Value> = ranges
        .iter()
        .map(|&(lo, hi)| {
            let slice: Vec<Value> = items.iter().skip(lo as usize).take((hi - lo + 1) as usize).cloned().collect();
            json!({ "op": "SYNC", "range": [lo, hi], "items": slice })
        })
        .collect();
    let d = json!({ "guild_id": sub.guild_id.to_string(), "id": "everyone", "member_count": mods + online + offline, "online_count": mods + online,
        "groups": [{ "id": ROLE, "count": mods }, { "id": "online", "count": online }, { "id": "offline", "count": offline }], "ops": ops });
    // Par les tables partagées : les parures des membres listés y sont lues.
    out.extend(Decoder::with_people(people()).decode("GUILD_MEMBER_LIST_UPDATE", &d.to_string()));
    out
}

struct Args {
    theme: Option<String>,
    view: View,
    panel: bool,
    hover_none: bool,
    screenshot: Option<PathBuf>,
    /// Capture prise à cet instant après le lancement, sans attendre une image stable (animations).
    at: Option<Duration>,
    audit: bool,
    /// Audit limité à ces jetons (`section.clé`).
    only: Vec<String>,
    /// Pointeur immobile à cette position (en points), pour montrer un survol.
    pointer: Option<egui::Pos2>,
    /// Notification (ou erreur de connexion sur l'écran de connexion) affichée au départ.
    notice: bool,
    /// Jetons changés (`section.clé=valeur`).
    set: Vec<String>,
    no_images: bool,
    /// Planche des fondations visuelles (lot V1) : icones, composants, fenetre.
    gallery: Option<String>,
    /// Dossier où écrire les sons d'interface en WAV (rien n'est joué, aucune fenêtre).
    sons: Option<PathBuf>,
    /// Position retenue de la fenêtre (comme dans `settings.toml`) ; la
    /// position obtenue est écrite sur la sortie d'erreur.
    position: Option<[f32; 2]>,
    /// Taille de la fenêtre (en points), pour voir une fenêtre étroite.
    size: Option<[f32; 2]>,
    /// Banc de mesure (`--bench`) et ses réglages.
    bench: Option<bench::BenchCfg>,
    /// Scénario de mesure de la mémoire (`--bench-mem`).
    bench_mem: Option<String>,
    /// Banc du processeur (`--bench-cpu <secondes de repos>`).
    bench_cpu: Option<u64>,
    /// Pose les bibliothèques d'ANGLE à côté de l'aperçu avant d'ouvrir la fenêtre (`--angle`).
    angle: bool,
    /// Banc d'usage prolongé (`--bench-long <salons visités par tour>`).
    bench_long: Option<usize>,
    scene: Option<Scene>,
    /// Carte de profil d'Alice ouverte au départ (`--profile`).
    profile: bool,
    /// Profil complet d'Alice ouvert au départ (`--profile-full`).
    profile_full: bool,
    /// Fenêtre Paramètres ouverte sur cette page (`--settings`).
    settings_page: Option<String>,
    avatar_image: bool,
    /// Disposition mobile imposée, sur cet écran (`--mobile`, phase 7).
    mobile: Option<String>,
}

fn args() -> Result<Args, String> {
    let mut a = Args {
        theme: None,
        view: View::Guild,
        panel: false,
        hover_none: false,
        screenshot: None,
        at: None,
        audit: false,
        only: Vec::new(),
        pointer: None,
        notice: false,
        set: Vec::new(),
        no_images: false,
        gallery: None,
        sons: None,
        position: None,
        size: None,
        bench: None,
        bench_mem: None,
        bench_cpu: None,
        angle: false,
        bench_long: None,
        scene: None,
        profile: false,
        profile_full: false,
        settings_page: None,
        avatar_image: false,
        mobile: None,
    };
    let num = |v: Option<String>, opt: &str| v.and_then(|s| s.parse::<usize>().ok()).ok_or(format!("{opt} attend un nombre"));
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--theme" => a.theme = Some(it.next().ok_or("--theme attend un préréglage ou un fichier")?),
            "--view" => {
                a.view = match it.next().as_deref() {
                    Some("guild") => View::Guild,
                    Some("dm") => View::Dm,
                    Some("login") => View::Login,
                    Some("masque") => View::Hidden,
                    Some("lecture-seule") => View::ReadOnly,
                    Some("sans-historique") => View::NoHistory,
                    Some("inconnu") => View::Unknown,
                    Some("exclusion") => View::Timeout,
                    Some("comptes") => View::Accounts,
                    Some("selecteur") => View::Selector,
                    Some("connexion") => View::Connecting,
                    Some("retrait") => View::Removing,
                    Some("ancien-fichier") => View::OldFile,
                    Some("migration") => View::Migration,
                    Some("suppression") => View::WipeQuestion,
                    Some("doublon") => View::Twin,
                    Some("doublon-retrait") => View::TwinRemove,
                    Some("refus-import") => View::Declined,
                    Some("fichier-vide") => View::Emptied,
                    Some("contenu-inconnu") => View::UnknownContent,
                    Some("remplacement") => View::Replacing,
                    Some("suppression-recente") => View::RecentWipe,
                    Some("essai-fichier") => View::FileTry,
                    Some("amis") => View::Friends,
                    Some("groupe") => View::Group,
                    _ => {
                        return Err("--view attend guild, dm, login, masque, lecture-seule, sans-historique, inconnu, exclusion, comptes, selecteur, connexion, retrait, ancien-fichier, migration, suppression, doublon, doublon-retrait, refus-import, fichier-vide, contenu-inconnu, remplacement, suppression-recente, essai-fichier, amis ou groupe".into());
                    }
                }
            }
            "--panel" => a.panel = true,
            "--settings" => a.settings_page = Some(it.next().ok_or("--settings attend une page (compte, profil, statut, confidentialite, applications, appareils, comptes-lies, facturation, apparence, texte, notifications, langue, dossiers, ferrocord, accessibilite, streamer, avances, donnees, activite, a-propos, raccourcis, voix, plugins, plugins-reglages)")?),
            "--avatar-image" => a.avatar_image = true,
            "--mobile" => a.mobile = Some(it.next().ok_or("--mobile attend salons, discussion, membres, messages, notifications, vous ou appui-long")?),
            "--hover-none" => a.hover_none = true,
            "--screenshot" => a.screenshot = Some(it.next().ok_or("--screenshot attend un fichier .png")?.into()),
            "--audit" => a.audit = true,
            "--only" => a.only = it.next().ok_or("--only attend des jetons séparés par des virgules")?.split(',').map(str::to_string).collect(),
            "--pointer" => {
                let v = it.next().ok_or("--pointer attend x,y")?;
                let (x, y) = v.split_once(',').ok_or("--pointer attend x,y")?;
                let n = |s: &str| s.trim().parse::<f32>().map_err(|_| format!("--pointer : nombre invalide « {s} »"));
                a.pointer = Some(egui::pos2(n(x)?, n(y)?));
            }
            "--notice" => a.notice = true,
            "--at" => a.at = Some(Duration::from_millis(num(it.next(), &arg)? as u64)),
            "--profile" => a.profile = true,
            "--profile-full" => a.profile_full = true,
            "--set" => a.set.push(it.next().ok_or("--set attend section.clé=valeur")?),
            "--no-images" => a.no_images = true,
            "--gallery" => a.gallery = Some(it.next().ok_or("--gallery attend icones, composants ou fenetre")?),
            "--sons" => a.sons = Some(it.next().ok_or("--sons attend un dossier")?.into()),
            "--position" => {
                let v = it.next().ok_or("--position attend x,y")?;
                let (x, y) = v.split_once(',').ok_or("--position attend x,y")?;
                let n = |s: &str| s.trim().parse::<f32>().map_err(|_| format!("--position : nombre invalide « {s} »"));
                a.position = Some([n(x)?, n(y)?]);
            }
            "--size" => {
                let v = it.next().ok_or("--size attend largeur,hauteur")?;
                let (w, h) = v.split_once(',').ok_or("--size attend largeur,hauteur")?;
                let n = |s: &str| s.trim().parse::<f32>().map_err(|_| format!("--size : nombre invalide « {s} »"));
                a.size = Some([n(w)?, n(h)?]);
            }
            "--bench" => a.bench = Some(a.bench.take().unwrap_or_default()),
            "--bench-guilds" => a.bench.get_or_insert_with(Default::default).guilds = num(it.next(), &arg)?,
            "--bench-channels" => a.bench.get_or_insert_with(Default::default).channels = num(it.next(), &arg)?.max(10),
            "--bench-messages" => a.bench.get_or_insert_with(Default::default).messages = num(it.next(), &arg)?,
            "--bench-deep" => a.bench.get_or_insert_with(Default::default).deep = num(it.next(), &arg)?.max(HISTORY_PAGE),
            "--bench-frames" => a.bench.get_or_insert_with(Default::default).frames = num(it.next(), &arg)?.max(10),
            "--bench-quiet" => a.bench.get_or_insert_with(Default::default).quiet = num(it.next(), &arg)? as u64,
            "--bench-out" => a.bench.get_or_insert_with(Default::default).out = Some(it.next().ok_or("--bench-out attend un fichier")?.into()),
            "--bench-label" => a.bench.get_or_insert_with(Default::default).label = it.next().ok_or("--bench-label attend un nom")?,
            "--scene" => {
                a.scene = Some(match it.next().as_deref() {
                    Some("nouveaux") => Scene::Nouveaux,
                    Some("bouton") => Scene::Bouton,
                    Some("present") => Scene::Present,
                    Some("non-envoyes") => Scene::NonEnvoyes,
                    Some("saisie") => Scene::Saisie,
                    Some("ecriture") => Scene::Ecriture(1),
                    Some("ecriture-deux") => Scene::Ecriture(2),
                    Some("ecriture-plusieurs") => Scene::Ecriture(4),
                    Some("transitions") => Scene::Transitions,
                    Some("saisie-longue") => Scene::SaisieLongue,
                    Some("reponse") => Scene::Reponse,
                    Some("fichiers") => Scene::Fichiers,
                    Some("fichier-details") => Scene::FichierDetails,
                    Some("parcourir") => Scene::Parcourir,
                    Some("messages-vocaux") => Scene::MessagesVocaux,
                    Some("entete") => Scene::Entete,
                    Some("entete-sujet") => Scene::EnteteSujet,
                    Some("entete-fils") => Scene::EnteteFils,
                    Some("entete-archives") => Scene::EnteteArchives,
                    Some("entete-cloche") => Scene::EnteteCloche,
                    Some("menu-serveur") => Scene::MenuServeur,
                    Some("menu-utilisateur") => Scene::MenuUtilisateur,
                    Some("categories-amis") => Scene::CategoriesAmis,
                    Some("demandes") => Scene::Demandes,
                    Some("quetes") => Scene::Quetes,
                    Some("boutique") => Scene::Boutique,
                    Some("inventaire") => Scene::Inventaire,
                    Some("suggestions") => Scene::Suggestions,
                    Some("enregistres") => Scene::Enregistres,
                    Some("etat-compte") => Scene::EtatCompte,
                    Some("repertoire") => Scene::Applis,
                    Some("verification") => Scene::Verification,
                    Some("resumes") => Scene::Resumes,
                    Some("decouverte") => Scene::Decouverte,
                    Some("effet") => Scene::Effet,
                    Some("effet-fixe") => Scene::EffetFixe,
                    Some("profil-carte") => Scene::ProfilCarte,
                    Some("panneau") => Scene::Panneau,
                    Some("jeu-detecte") => Scene::JeuDetecte,
                    Some("jeu-detecte-appel") => Scene::JeuDetecteAppel,
                    Some("panneau-menu") => Scene::PanneauMenu,
                    Some("amis") => Scene::Amis,
                    Some("vocal-amis") => Scene::VocalAmis,
                    Some("vocal-rail") => Scene::VocalRail,
                    Some("vocal-salon") => Scene::VocalSalon,
                    Some("mp-chat") => Scene::MpChat,
                    Some("mp-systeme") => Scene::MpSysteme,
                    Some("transfert") => Scene::Transfert,
                    Some("pseudo-ami") => Scene::PseudoAmi,
                    Some("categories") => Scene::Categories,
                    Some("categories-muets") => Scene::CategoriesMuets,
                    Some("salon-vocal") => Scene::SalonVocal,
                    Some("lecteur") => Scene::Lecteur,
                    Some("lecteur-plein") => Scene::LecteurPlein,
                    Some("videos") => Scene::Videos,
                    Some("videos-liens") => Scene::VideosLiens,
                    Some("lien-externe") => Scene::LienExterne,
                    Some("fils") => Scene::Fils,
                    Some("fil") => Scene::Fil,
                    Some("fil-membres") => Scene::FilMembres,
                    Some("forum") => Scene::Forum,
                    Some("galerie") => Scene::Galerie,
                    Some("emojis") => Scene::Emojis,
                    Some("suggestions-autocollants") => Scene::SuggestionsAutocollants,
                    Some("gifs") => Scene::Gifs,
                    Some("gifs-recherche") => Scene::GifsRecherche,
                    Some("gifs-favoris") => Scene::GifsFavoris,
                    Some("super-reaction") => Scene::SuperReaction,
                    Some("parures") => Scene::Parures,
                    Some("parures-animees") => Scene::ParuresAnimees,
                    Some("serveurs") => Scene::Serveurs,
                    Some("serveurs-ajouter") => Scene::ServeursAjouter,
                    Some("serveurs-inviter") => Scene::ServeursInviter,
                    Some("serveurs-invitations") => Scene::ServeursInvitations,
                    Some("parametres-serveur") => Scene::ParametresServeur(ferrocord::ui::guild_settings::Page::Overview),
                    Some("parametres-serveur-webhooks") => Scene::ParametresServeur(ferrocord::ui::guild_settings::Page::Webhooks),
                    Some("parametres-serveur-integrations") => Scene::ParametresServeur(ferrocord::ui::guild_settings::Page::Integrations),
                    Some("parametres-serveur-boosts") => Scene::ParametresServeur(ferrocord::ui::guild_settings::Page::Boosts),
                    Some("parametres-serveur-widget") => Scene::ParametresServeur(ferrocord::ui::guild_settings::Page::Widget),
                    Some("parametres-serveur-modele") => Scene::ParametresServeur(ferrocord::ui::guild_settings::Page::Template),
                    Some("parametres-serveur-elagage") => Scene::ParametresServeur(ferrocord::ui::guild_settings::Page::Prune),
                    Some("alerte-automod") => Scene::AlerteAutomod,
                    Some("bloques") => Scene::Bloques,
                    Some("menu-plus") => Scene::MenuPlus,
                    Some("mention-role") => Scene::MentionRole,
                    Some("limite-age") => Scene::LimiteAge,
                    Some("recherche") => Scene::Recherche,
                    Some("selecteur") => Scene::Selecteur,
                    Some("boite") => Scene::Boite,
                    Some("boite-mentions") => Scene::BoiteMentions,
                    Some("boite-programmes") => Scene::BoiteProgrammes,
                    Some("boite-cartes") => Scene::BoiteCartes,
                    Some("raccourcis") => Scene::Raccourcis,
                    Some("notifs-serveur") => Scene::NotifsServeur,
                    Some("amis-ajouter") => Scene::AmisAjouter,
                    Some("pour-vous") => Scene::PourVous,
                    Some("journal") => Scene::Journal,
                    Some("onglets") => Scene::Fenetres("onglets"),
                    Some("palette") => Scene::Fenetres("palette"),
                    Some("scinde") => Scene::Fenetres("scinde"),
                    Some("detache") => Scene::Fenetres("detache"),
                    Some("zoom") => Scene::Fenetres("zoom"),
                    Some("mots-cles") => Scene::MotsCles(false),
                    Some("mots-cles-reglages") => Scene::MotsCles(true),
                    Some("recherche-locale") => Scene::RechercheLocale,
                    Some("export") => Scene::Export(false),
                    Some("export-lecture") => Scene::Export(true),
                    Some("plantage") => Scene::Plantage,
                    Some("theme-import") => Scene::ThemeImport,
                    Some("mica") => Scene::Mica(false),
                    Some("mica-acrylique") => Scene::Mica(true),
                    Some("discret") => Scene::E3(0),
                    Some("programme") => Scene::E3(1),
                    Some("programmes") => Scene::E3(2),
                    Some("programmer") => Scene::E3(3),
                    Some("traduction") => Scene::E3(4),
                    Some("traduction-reglages") => Scene::E3(5),
                    Some("applications") => Scene::Applications,
                    Some("modale") => Scene::Modale,
                    Some("sondages") => Scene::Sondages,
                    Some("salons") => Scene::Salons,
                    Some("salons-permissions") => Scene::SalonsPermissions,
                    Some("salons-creer") => Scene::SalonsCreer,
                    Some("salons-serveur") => Scene::SalonsServeur,
                    Some("roles") => Scene::Roles,
                    Some("role") => Scene::Role,
                    Some("role-permissions") => Scene::RolePermissions,
                    Some("membres") => Scene::Membres,
                    Some("membre-roles") => Scene::MembreRoles,
                    Some("moderation-securite") => Scene::Moderation(ferrocord::ui::guild_settings::Page::Safety),
                    Some("moderation-automod") => Scene::Moderation(ferrocord::ui::guild_settings::Page::AutoMod),
                    Some("moderation-journal") => Scene::Moderation(ferrocord::ui::guild_settings::Page::AuditLog),
                    Some("moderation-bannissements") => Scene::Moderation(ferrocord::ui::guild_settings::Page::Bans),
                    Some("moderation-membre") => Scene::ModerationMembre,
                    Some("evenements") => Scene::Evenements,
                    Some("evenement-creer") => Scene::EvenementCreer,
                    Some("bienvenue") => Scene::Bienvenue,
                    Some("integration") => Scene::Integration,
                    Some("parametres-integration") => Scene::ParametresIntegration,
                    Some("parametres-bienvenue") => Scene::ParametresBienvenue,
                    Some("parametres-emojis") => Scene::ParametresEmojis,
                    Some("parametres-autocollants") => Scene::ParametresAutocollants,
                    Some("parametres-sons") => Scene::ParametresSons,
                    Some("vocal") => Scene::Vocal,
                    Some("vocal-codes") => Scene::VocalCodes,
                    Some("enregistrement") => Scene::VoixE4(0),
                    Some("enregistrement-demarrer") => Scene::VoixE4(1),
                    Some("incrustation") => Scene::VoixE4(2),
                    Some("rnnoise") => Scene::VoixE4(3),
                    Some("appel") => Scene::Appel,
                    Some("appel-direct") => Scene::AppelDirect,
                    Some("appel-partage") => Scene::AppelPartage,
                    Some("sonnerie") => Scene::Sonnerie,
                    Some("appel-mp") => Scene::AppelMp,
                    Some("appel-sortant") => Scene::AppelSortant,
                    Some("appel-groupe") => Scene::AppelGroupe,
                    Some("appel-mp-partage") => Scene::AppelMpPartage,
                    Some("conference") => Scene::Conference,
                    Some("sons") => Scene::Sons,
                    Some("partage") => Scene::Partage(0),
                    Some("partage-ecrans") => Scene::Partage(1),
                    Some("partage-appareils") => Scene::Partage(2),
                    Some("partage-camera") => Scene::Partage(3),
                    Some("partage-qualite") => Scene::Partage(4),
                    Some("accessibilite") => Scene::Accessibilite,
                    Some("apparence-editeur") => Scene::ApparenceEditeur,
                    Some("theme-degrade") => Scene::ThemeDegrade,
                    Some("fond-image") => Scene::FondImage,
                    Some("densites") => Scene::Densites(false),
                    Some("densites-spacieuse") => Scene::Densites(true),
                    Some("sans-rail") => Scene::SansRail,
                    Some("activite") => Scene::Activite,
                    Some("rich-presence") => Scene::RichPresence,
                    Some("qr") => Scene::Qr,
                    Some("qr-lu") => Scene::QrLu,
                    Some("mon-compte") => Scene::MonCompte,
                    Some("mon-compte-jeu") => Scene::MonCompteJeu,
                    Some("mp-activite") => Scene::MpActivite,
                    Some("mon-compte-sobre") => Scene::MonCompteSobre,
                    Some("mon-compte-clair") => Scene::MonCompteClair,
                    Some("mon-compte-statut") => Scene::MonCompteStatut,
                    Some("mon-compte-statut-perso") => Scene::MonCompteStatutPerso,
                    Some("profil-edition") => Scene::ProfilEdition,
                    Some("profil-edition-serveur") => Scene::ProfilEditionServeur,
                    Some("profil-edition-sans-nitro") => Scene::ProfilEditionSansNitro,
                    Some("profil-edition-style") => Scene::ProfilEditionStyle,
                    Some("hors-ligne") => Scene::HorsLigne,
                    Some("glyphes") => Scene::Glyphes,
                    Some("polices-nom") => Scene::PolicesNom,
                    _ => return Err("--scene : nom de scène inconnu (la liste est dans la correspondance juste au-dessus, examples/preview.rs)".into()),
                });
                // La page des demandes vit dans l'accueil des messages privés : la scène suffit à l'ouvrir.
                if a.scene == Some(Scene::Demandes) {
                    a.view = View::Friends;
                }
                if a.scene == Some(Scene::VocalAmis) {
                    VOCAL.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                if matches!(a.scene, Some(Scene::Amis | Scene::VocalAmis)) {
                    AMIS.store(true, std::sync::atomic::Ordering::Relaxed);
                    a.view = View::Friends;
                }
                if a.scene == Some(Scene::MpActivite) {
                    SANS_BANNIERE.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                if matches!(a.scene, Some(Scene::MpChat | Scene::MpActivite | Scene::MpSysteme | Scene::Transfert)) {
                    MPSYSTEME.store(a.scene == Some(Scene::MpSysteme), std::sync::atomic::Ordering::Relaxed);
                    AMIS.store(true, std::sync::atomic::Ordering::Relaxed);
                    MPCHAT.store(true, std::sync::atomic::Ordering::Relaxed);
                    a.view = View::Dm;
                }
                if matches!(a.scene, Some(Scene::Effet | Scene::EffetFixe)) {
                    EFFET.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                if matches!(a.scene, Some(Scene::Panneau | Scene::PanneauMenu)) {
                    PANNEAU.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                if a.scene == Some(Scene::ProfilCarte) {
                    CARTE.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                if matches!(a.scene, Some(Scene::Parures | Scene::ParuresAnimees | Scene::ProfilCarte)) {
                    PARURES.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                MY_THEME.store(match a.scene {
                    Some(Scene::MonCompteSobre) => 1,
                    Some(Scene::MonCompteClair) => 2,
                    Some(Scene::ProfilEdition | Scene::ProfilEditionStyle) => 3,
                    Some(Scene::MonCompteJeu) => 4,
                    _ => 0,
                }, std::sync::atomic::Ordering::Relaxed);
                if matches!(a.scene, Some(Scene::ProfilEdition | Scene::ProfilEditionServeur | Scene::ProfilEditionStyle)) {
                    NITRO.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
            "--bench-cpu" => a.bench_cpu = Some(num(it.next(), &arg)? as u64),
            "--angle" => a.angle = true,
            "--bench-long" => a.bench_long = Some(num(it.next(), &arg)?.max(20)),
            "--bench-atlas-libre" => a.bench.get_or_insert_with(Default::default).free_atlas = true,
            "--bench-mem" => a.bench_mem = Some(it.next().ok_or("--bench-mem attend vide, vide-sans-polices, vide-logiciel, connexion, app-sans-images ou app")?),
            // Langue de l'interface, choisie avant de construire quoi que ce soit.
            "--lang" => ferrocord::i18n::set_language(match it.next().as_deref() {
                Some("fr") => Choice::Fr,
                Some("en") => Choice::En,
                _ => return Err("--lang attend fr ou en".into()),
            }),
            other => return Err(format!("option inconnue : {other}")),
        }
    }
    Ok(a)
}

/// Jetons modifiés un à un pour l'audit.
struct Audit {
    base: Tokens,
    /// (rang du jeton dans `fields_mut`, `section.clé`)
    keys: Vec<(usize, String)>,
    /// 0 = image de référence, puis un jeton modifié par étape.
    step: usize,
    reference: Option<Arc<egui::ColorImage>>,
    quiet: Vec<String>,
}

/// `base` avec le jeton numéro `index` nettement changé.
fn perturbed(base: &Tokens, index: usize) -> Tokens {
    fn invert(c: egui::Color32) -> egui::Color32 {
        // Opaque : un voile presque transparent changé ainsi se voit.
        egui::Color32::from_rgb(u8::MAX - c.r(), u8::MAX - c.g(), u8::MAX - c.b())
    }
    let mut t = base.clone();
    if let Some(f) = t.fields_mut().into_iter().nth(index) {
        match f.value {
            FieldMut::Color(c) => *c = invert(*c),
            FieldMut::Palette(p) => p.iter_mut().for_each(|c| *c = invert(*c)),
            FieldMut::Number(v, b) => *v = if *v - b.min < b.max - *v { b.max } else { b.min },
            FieldMut::Choice(c) => {
                let n = c.options().len();
                c.set_index((c.index() + 1) % n);
            }
        }
    }
    t
}

/// Enveloppe qui attend une image stable, la capture puis ferme la fenêtre.
struct Preview {
    app: App,
    /// Ouvrir le sélecteur de compte dès que la session est prête.
    open_menu: bool,
    /// Vue montrée (question posée une fois le sélecteur ouvert).
    view: View,
    /// Runtime des images : gardé tant que la fenêtre vit.
    _rt: tokio::runtime::Runtime,
    shot: Option<PathBuf>,
    /// Instant de la capture (`--at`), sans attendre une image stable.
    at: Option<Duration>,
    hover_none: bool,
    pointer: Option<egui::Pos2>,
    /// La position simulée a déjà été injectée.
    pointer_sent: bool,
    audit: Option<Audit>,
    started: Instant,
    settle: Duration,
    slot: Arc<Mutex<Option<Arc<egui::ColorImage>>>>,
    previous: Option<Arc<egui::ColorImage>>,
    waiting: bool,
    done: bool,
    /// Écrire la position obtenue de la fenêtre (`--position`).
    report_position: bool,
    /// Dossier temporaire de la vue `ancien-fichier`, supprimé à la fin.
    _old_file: Option<OldFile>,
    scene: Option<Scene>,
    /// File d'événements de la connexion simulée (messages arrivés en cours de scène).
    inject: Arc<Mutex<Vec<Event>>>,
    /// Messages de la scène déjà versés ; pointeur rendu après le défilement.
    injected: bool,
    pointer_released: bool,
    /// Appui long simulé sur un message (`--mobile appui-long`) : déjà envoyé.
    long_press: Option<bool>,
    /// Début de l'image en cours (scène `hors-ligne` : coût de l'image où l'historique s'affiche, lot C9).
    frame_at: Option<Instant>,
}

/// Ancien fichier de jeton factice, dans un dossier temporaire. Vue
/// `ancien-fichier` : son « jeton » contient des espaces, il n'est donc pas
/// migré et reste signalé ; autres vues : un jeton factice.
struct OldFile(PathBuf, Option<std::fs::File>);

impl OldFile {
    fn create(content: &[u8], hold: bool) -> Option<Self> {
        let dir = std::env::temp_dir().join(format!("ferrocord-apercu-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::write(dir.join("token"), content).ok()?;
        // Vue `fichier-vide` : un lecteur garde le fichier ouvert sans partager la
        // suppression (Windows) : les zéros sont là, la suppression échoue.
        let held = if hold { hold_open(&dir.join("token")) } else { None };
        Some(Self(dir, held))
    }

    fn path(&self) -> PathBuf {
        self.0.join("token")
    }
}

/// Ouvre le fichier en lecture, en partageant lecture et écriture, pas la suppression.
#[cfg(windows)]
fn hold_open(path: &std::path::Path) -> Option<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().read(true).share_mode(0x1 | 0x2).open(path).ok()
}

#[cfg(not(windows))]
fn hold_open(_: &std::path::Path) -> Option<std::fs::File> {
    None
}

/// Contenu du fichier factice de chaque vue qui en a un.
fn old_file_content(view: View) -> Option<&'static [u8]> {
    match view {
        View::OldFile => Some(b"faux ancien jeton\n"),
        View::Migration | View::Declined => Some(b"faux.jeton.apercu-9\n"),
        View::WipeQuestion => Some(b"faux.jeton.ancien-8\n"),
        View::RecentWipe => Some(b"faux.jeton.apercu-3\n"),
        View::FileTry => Some(b"faux.jeton.fichier-6\n"),
        View::Twin | View::TwinRemove => Some(b"faux.jeton.apercu-3\n"),
        View::Emptied => Some(&[0u8; 20]),
        View::UnknownContent => Some(&[0xff, 0xfe, 0x41, 0x00]),
        _ => None,
    }
}

impl Drop for OldFile {
    fn drop(&mut self) {
        self.1 = None;
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Délai minimal avant la première capture (lecture du thème, mise en page).
const SETTLE: Duration = Duration::from_millis(700);
/// Scène `transitions` : instant du déclenchement (à comparer à `--at`).
const TRANSITIONS_AT: Duration = Duration::from_millis(1000);
/// Durée du défilement vers le haut des scènes `bouton` et `present`.
const SCROLL_UP: Duration = Duration::from_millis(500);
/// Délai après chaque jeton modifié pendant l'audit.
const AUDIT_SETTLE: Duration = Duration::from_millis(120);
/// Abandon : on garde la dernière capture même si elle bouge encore.
const GIVE_UP: Duration = Duration::from_secs(15);

impl Preview {
    /// Image stable reçue : capture enregistrée, ou étape suivante de l'audit.
    fn stable(&mut self, ctx: &egui::Context, img: Arc<egui::ColorImage>) {
        if let Some(a) = self.audit.as_mut() {
            match &a.reference {
                None => a.reference = Some(img),
                Some(r) => {
                    if r.size == img.size && r.pixels == img.pixels {
                        a.quiet.push(a.keys[a.step - 1].1.clone());
                    }
                }
            }
            a.step += 1;
            if a.step <= a.keys.len() {
                let t = perturbed(&a.base, a.keys[a.step - 1].0);
                self.app.set_theme(t);
                self.started = Instant::now();
                self.settle = AUDIT_SETTLE;
                self.previous = None;
                return;
            }
            println!("jetons sans effet visible dans cette vue ({} sur {}) :", a.quiet.len(), a.keys.len());
            for k in &a.quiet {
                println!("  {k}");
            }
            self.done = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if let Some(path) = &self.shot {
            match save_png(path, &img) {
                Ok(()) => eprintln!("capture : {} ({}x{})", path.display(), img.size[0], img.size[1]),
                Err(e) => eprintln!("capture impossible ({}) : {e}", path.display()),
            }
        }
        self.done = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

impl eframe::App for Preview {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        eframe::App::clear_color(&self.app, visuals)
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        // Fenêtre secondaire (incrustation « qui parle ») : rien à simuler.
        if raw.viewport_id != egui::ViewportId::ROOT {
            return;
        }
        if self.hover_none || self.pointer.is_some() {
            // La vraie souris est ignorée : hors de la fenêtre, ou immobile à
            // `pointer`. La position simulée n'est donnée qu'une fois (egui la
            // garde) : la répéter à chaque image en provoquerait une autre.
            raw.events.retain(|e| !matches!(e, egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. } | egui::Event::MouseMoved(_)));
            if !self.pointer_sent {
                self.pointer_sent = true;
                raw.events.push(self.pointer.map_or(egui::Event::PointerGone, egui::Event::PointerMoved));
            }
        }
        // Scènes : la vue remonte (molette sur la discussion), puis le pointeur s'en va.
        if self.scene == Some(Scene::Bouton) {
            if self.started.elapsed() < SCROLL_UP {
                raw.events.push(egui::Event::PointerMoved(raw.screen_rect.map_or(egui::Pos2::ZERO, |r| egui::pos2(r.max.x * 0.65, r.max.y * 0.5))));
                raw.events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 800.0),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Wheel,
                    modifiers: egui::Modifiers::NONE,
                });
            } else if !self.pointer_released {
                self.pointer_released = true;
                raw.events.push(egui::Event::PointerGone);
            }
        }
        // Appui long sur un message (phase 7) : la vraie souris est ignorée,
        // le bouton reste enfoncé sur la discussion jusqu'à la capture.
        if let Some(sent) = self.long_press {
            raw.events.retain(|e| !matches!(e, egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. } | egui::Event::MouseMoved(_) | egui::Event::PointerGone));
            if !sent && self.started.elapsed() > SETTLE / 2 {
                self.long_press = Some(true);
                let at = raw.screen_rect.map_or(egui::Pos2::ZERO, |r| egui::pos2(r.center().x, r.max.y - 260.0));
                raw.events.push(egui::Event::PointerMoved(at));
                raw.events.push(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
            }
        }
        // Disposition mobile : la souris touche comme un doigt.
        eframe::App::raw_input_hook(&mut self.app, ctx, raw);
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.frame_at = Some(Instant::now());
        eframe::App::logic(&mut self.app, ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Clic sur « Essayer le jeton du fichier » dès qu'il est proposé.
        if self.view == View::FileTry && self.app.offers_file_token() && self.app.login_error().is_some() {
            self.app.try_file_token();
        }
        if self.scene == Some(Scene::Bouton) && !self.injected && self.started.elapsed() > SCROLL_UP {
            self.injected = true;
            if let Ok(mut q) = self.inject.lock() {
                q.extend(live_events());
            }
        }
        // Transitions : à 1 s, un message qui nous mentionne arrive et la carte du compte s'ouvre.
        if self.scene == Some(Scene::Transitions) && !self.injected && self.started.elapsed() > TRANSITIONS_AT {
            self.injected = true;
            if let Ok(mut q) = self.inject.lock() {
                let mut v = fixture("message_create_mention");
                v["d"]["id"] = "9000000000000000001".into();
                v["d"]["channel_id"] = GENERAL.to_string().into();
                q.extend(packet(&v));
            }
            self.app.open_account_menu();
            ui.ctx().request_repaint();
        }
        if matches!(self.scene, Some(Scene::Qr | Scene::QrLu)) && !self.injected {
            self.injected = true;
            self.app.preview_qr_login(self.scene == Some(Scene::QrLu));
        }
        eframe::App::ui(&mut self.app, ui, frame);
        if self.scene == Some(Scene::HorsLigne)
            && !self.injected
            && let Some(d) = self.app.offline_shown_after()
        {
            self.injected = true;
            eprintln!("historique hors ligne affiché : {} ms après l'ouverture de la session, {} ms après la création de l'application", d.as_millis(), self.started.elapsed().as_millis());
            // Recul C9 : l'image du premier salon (caches froids) mesurée comme dans FERROCORD_PERF.
            if let Some(at) = self.frame_at {
                eprintln!("image du premier salon : {:.2} ms (logique et interface)", at.elapsed().as_secs_f64() * 1000.0);
            }
        } else if self.scene == Some(Scene::HorsLigne) && self.injected {
            // Les deux images suivantes (la liste des messages, puis celle des membres).
            static NEXT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
            if let Some(at) = self.frame_at
                && NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 2
            {
                eprintln!("  image suivante : {:.2} ms", at.elapsed().as_secs_f64() * 1000.0);
            }
        }
        // Sélecteur de partage : la caméra choisie dès que la liste est là (son aperçu), ou le menu de l'engrenage.
        if !self.injected && (self.scene == Some(Scene::Partage(3)) && self.app.share_select("camera:usb") || self.scene == Some(Scene::Partage(4)) && self.started.elapsed() > Duration::from_millis(400)) {
            if self.scene == Some(Scene::Partage(4)) {
                self.app.open_share_settings();
            }
            self.injected = true;
            ui.ctx().request_repaint();
        }
        if self.report_position
            && let Some(r) = ui.ctx().input(|i| i.viewport().outer_rect)
        {
            self.report_position = false;
            eprintln!("position de la fenêtre (points) : {:.0},{:.0}", r.min.x, r.min.y);
        }
        if self.open_menu && self.app.store().me.is_some() {
            self.open_menu = false;
            self.app.open_account_menu();
            match self.view {
                // Premier clic sur « Supprimer le fichier » : le coffre est lu pour la question.
                View::WipeQuestion | View::RecentWipe => self.app.ask_delete_legacy_file(),
                // Premier clic sur « Retirer » du doublon.
                View::TwinRemove => {
                    if let Some(k) = self.app.duplicate_account() {
                        self.app.ask_remove_account(k);
                    }
                }
                _ => {}
            }
            ui.ctx().request_repaint();
        }
        if (self.shot.is_none() && self.audit.is_none()) || self.done {
            return;
        }
        let ctx = ui.ctx().clone();
        ctx.request_repaint_after(Duration::from_millis(50));
        let arrived = self.slot.lock().ok().and_then(|mut s| s.take());
        if let Some(img) = arrived {
            self.waiting = false;
            let stable = self.previous.as_ref().is_some_and(|p| p.size == img.size && p.pixels == img.pixels);
            if stable || self.at.is_some() || self.started.elapsed() > GIVE_UP {
                return self.stable(&ctx, img);
            }
            self.previous = Some(img);
        }
        if !self.waiting && self.started.elapsed() > self.at.unwrap_or(self.settle) {
            let slot = self.slot.clone();
            ctx.request_screenshot(move |img| {
                if let Ok(mut s) = slot.lock() {
                    *s = Some(img);
                }
            });
            self.waiting = true;
        }
    }
}

/// Jetons de `--set`, vérifiés une première fois sur Rouille.
fn overrides(set: &[String]) -> Result<Vec<(String, String, String)>, String> {
    let mut out = Vec::new();
    for s in set {
        let (path, value) = s.split_once('=').ok_or_else(|| format!("--set {s} : section.clé=valeur attendu"))?;
        let (section, key) = path.split_once('.').ok_or_else(|| format!("--set {s} : section.clé=valeur attendu"))?;
        out.push((section.trim().to_string(), key.trim().to_string(), value.trim().to_string()));
    }
    ferrocord::theme::parse(&overrides_toml(Preset::Rouille, &out)).map_err(|e| format!("--set : {e}"))?;
    Ok(out)
}

/// Fichier de thème partiel : le préréglage, puis les jetons changés.
fn overrides_toml(base: Preset, set: &[(String, String, String)]) -> String {
    let mut text = format!("base = \"{}\"\n", base.key());
    // Une seule table par section (TOML refuse les doublons).
    let mut sections: Vec<&str> = set.iter().map(|(s, _, _)| s.as_str()).collect();
    sections.dedup();
    sections.sort_unstable();
    sections.dedup();
    for section in sections {
        text.push_str(&format!("[{section}]\n"));
        for (_, key, value) in set.iter().filter(|(s, _, _)| s == section) {
            let value = if value.parse::<f64>().is_ok() || value.starts_with('"') { value.clone() } else { format!("\"{value}\"") };
            text.push_str(&format!("{key} = {value}\n"));
        }
    }
    text
}

fn with_overrides(p: Preset, set: &[(String, String, String)]) -> Tokens {
    ferrocord::theme::parse(&overrides_toml(p, set)).unwrap_or_else(|_| p.tokens())
}

fn save_png(path: &PathBuf, img: &egui::ColorImage) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let [w, h] = img.size;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    let data: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    writer.write_image_data(&data).map_err(|e| e.to_string())
}

fn main() -> eframe::Result {
    ferrocord::panics::install();
    // L'aperçu ne lit ni n'écrit les réglages de l'utilisateur : tout va dans
    // un dossier temporaire à lui (constaté : `installation.json` et
    // `local.toml` de la vraie installation étaient réécrits à chaque lancement).
    ferrocord::platform::use_private_dirs(std::env::temp_dir().join("ferrocord-apercu"));
    let args = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    if let Some(dir) = args.sons.as_deref() {
        return match write_sounds(dir) {
            Ok(n) => {
                println!("{n} sons écrits dans {}", dir.display());
                Ok(())
            }
            Err(e) => {
                eprintln!("--sons : {e}");
                std::process::exit(1);
            }
        };
    }
    // Lot « gl » : l'application ne pose ANGLE que dans son dossier d'installation ; l'aperçu,
    // sur demande, à côté de lui. Sans cela (ou avec FERROCORD_RENDU=opengl) : pilote OpenGL.
    if args.angle
        && let Some(dir) = ferrocord::rendu::angle::exe_dir()
        && let Err(e) = ferrocord::rendu::angle::install(&dir)
    {
        eprintln!("--angle : {e}");
    }
    if let Some(scenario) = args.bench_mem.as_deref() {
        return bench::run_mem(scenario);
    }
    if let Some(quiet) = args.bench_cpu {
        return bench::cpu::run(quiet);
    }
    if let Some(visits) = args.bench_long {
        return bench::long::run(bench::long::LongCfg { visits });
    }
    if let Some(cfg) = args.bench {
        return bench::run(cfg);
    }
    // Préréglage : aucun fichier. Fichier : lu et surveillé, jamais réécrit.
    let (theme, preset) = match args.theme.as_deref() {
        None => (ThemeLink::open(None, false, WatchConfig::default()), Some(Preset::DEFAULT)),
        Some(name) => match Preset::from_name(name) {
            Some(p) => (ThemeLink::open(None, false, WatchConfig::default()), Some(p)),
            None => (ThemeLink::open(Some(PathBuf::from(name)), false, WatchConfig::default()), None),
        },
    };
    let set = match overrides(&args.set) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    // Runtime des images (source locale, aucun réseau).
    let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("runtime : {e}");
            std::process::exit(1);
        }
    };
    // Même lancement que l'application (position retenue appliquée seulement
    // si elle tombe sur un écran présent).
    let window = ferrocord::settings::Window { position: args.position, size: args.size, ..Default::default() };
    // Barre de titre intégrée si le thème la demande (lot V1) ; `--set mise_en_page.titlebar=windows` rend celle de Windows.
    let first_theme = match (&preset, &theme.1) {
        (Some(p), _) => Some(with_overrides(*p, &set)),
        (None, Some(ferrocord::theme::Update::Loaded(t))) => Some(t.clone()),
        _ => None,
    };
    let mut options = ferrocord::with_titlebar(ferrocord::native_options_for(&window), first_theme.as_ref());
    options.viewport = options.viewport.with_title("Ferrocord — aperçu hors ligne");
    // Mica (lot E5) : fenêtre transparente, comme l'application quand le plugin est activé.
    options = ferrocord::with_backdrop(options, matches!(args.scene, Some(Scene::Mica(_))));
    ferrocord::launch(
        "Ferrocord aperçu",
        options,
        window,
        Box::new(move |cc| {
            let connected = !args.view.starts_offline();
            let mut queue = Vec::new();
            if args.notice {
                queue.push(if connected {
                    Event::Error(Problem::TooManyRequests)
                } else {
                    Event::Fatal { problem: Problem::Refused { code: 4004 }, bad_token: false }
                });
            }
            let inject: Arc<Mutex<Vec<Event>>> = Arc::default();
            let (store, pages) = filled_store(args.view, args.scene);
            let backend = Offline { connected, queue, wake: None, next_id: 5_000_000_000_000_001_000, view: args.view, inject: Some(inject.clone()), pages, served: None, make: None };
            let images = if args.no_images {
                Images::disabled(cc.egui_ctx.clone())
            } else {
                let wake_ctx = cc.egui_ctx.clone();
                let wake: Wake = Arc::new(move || wake_ctx.request_repaint());
                Images::start(cc.egui_ctx.clone(), rt.handle(), Arc::new(Generated), None, images::Config::default(), wake)
            };
            // Réglages et coffre en mémoire : aucun fichier lu ni écrit, aucun
            // `DISCORD_TOKEN`. Ancien fichier de jeton : seulement pour la vue
            // `ancien-fichier`, un fichier factice dans un dossier temporaire.
            let (settings_link, _) = SettingsLink::open(None, false, WatchConfig::default());
            let old_file = old_file_content(args.view).and_then(|c| OldFile::create(c, args.view == View::Emptied));
            let (settings_first, vault) = match args.view {
                _ if args.view.has_accounts() => (Some(sample_settings(args.view)), sample_vault()),
                // Le coffre a déjà un autre jeton pour le compte importé.
                View::WipeQuestion | View::Emptied | View::UnknownContent => (None, MemoryVault::with([(AccountKey::imported(), Token::from("faux.jeton.apercu-3"))])),
                // Même jeton dans le fichier et le coffre, copie faite il y a une heure.
                // Compte importé déjà connu ; le coffre a un autre jeton que le fichier (refusé par Discord).
                View::FileTry => {
                    let me = Account { user_id: Some(ME), name: Some("Moi Fictif".into()), avatar: Some(ImageHash::synthetic(ME)), ..Account::new(AccountKey::imported()) };
                    (Some(Settings { last_account: Some(AccountKey::imported()), accounts: vec![me], ..Settings::default() }), MemoryVault::with([(AccountKey::imported(), Token::from("faux.jeton.coffre-5"))]))
                }
                View::RecentWipe => {
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                    (Some(Settings { vault_copy_since: Some(now.saturating_sub(3600)), ..Settings::default() }), MemoryVault::with([(AccountKey::imported(), Token::from("faux.jeton.apercu-3"))]))
                }
                // Import refusé auparavant : taille et date de ce fichier-là.
                View::Declined => {
                    let declined_import = old_file.as_ref().and_then(|f| ferrocord::account::migrate::file_mark(&f.path()));
                    (Some(Settings { declined_import, ..Settings::default() }), MemoryVault::new())
                }
                _ => (None, MemoryVault::new()),
            };
            let settings_first = settings_first.map(SettingsUpdate::Loaded);
            let boot = Boot {
                backend: Box::new(backend),
                store,
                theme,
                settings: (settings_link, settings_first),
                accounts: AccountLink::start(Arc::new(vault)),
                env_token: None,
                legacy: old_file.as_ref().map(OldFile::path),
                images,
            };
            let mut app = App::new(cc, boot);
            // Historique hors ligne (lot H) : instantané d'une session précédente, en mémoire.
            if args.scene == Some(Scene::HorsLigne) {
                app.attach_offline(Box::new(PreviewOffline(ferrocord::state::offline::Snapshot::capture(&filled_store(View::Guild, None).0), Vec::new())));
            }
            if args.view == View::Removing {
                app.ask_remove_account(AccountKey::of_user(SECOND));
            }
            if args.view == View::Group {
                app.show_group_members(DM_GROUP);
            }
            // Premier clic sur « Supprimer le fichier » (écran de connexion) : fichier et coffre relus.
            if args.view == View::UnknownContent {
                app.ask_delete_legacy_file();
            }
            // Jeton saisi (« Se souvenir de moi ») pour le premier compte, qui en a déjà un autre.
            if args.view == View::Replacing {
                app.add_account(Token::from("faux.jeton.nouveau-4"), true);
            }
            let base = preset.map(|p| with_overrides(p, &set));
            if let Some(t) = &base {
                app.set_theme(t.clone());
            }
            if let Some(p) = &args.settings_page
                && !app.open_settings(p)
            {
                return Err(format!("--settings : page inconnue « {p} »").into());
            }
            if args.settings_page.as_deref() == Some("ferrocord") {
                let mut p = app.local_prefs();
                p.update_manifest = "https://exemple.invalid/ferrocord/manifeste.json".into();
                app.set_local_prefs(p);
                app.attach_install(Box::new(PreviewInstall { sent: false }), None, &Default::default());
            }
            if args.settings_page.as_deref() == Some("plugins") {
                // Lot P0 : deux plugins coupés, pour voir les deux états d'une carte.
                app.set_plugin(ferrocord::plugins::Id::RichPresence, false);
                app.set_plugin(ferrocord::plugins::Id::AutoTheme, false);
            }
            if args.settings_page.as_deref() == Some("voix") {
                // Lot V5 : seuil réglé à la main (curseur et trait sur le vumètre).
                let mut p = app.local_prefs();
                (p.audio.auto_threshold, p.audio.threshold_db) = (false, -45);
                app.set_local_prefs(p);
                app.attach_audio(Box::new(PreviewAudio { sent: false }));
            }
            if args.avatar_image {
                // Image synthétique (dégradé) dans le recadrage de la page Profils.
                let img = image::RgbaImage::from_fn(800, 500, |x, y| image::Rgba([(x * 255 / 800) as u8, (y * 255 / 500) as u8, 160, 255]));
                let mut png = std::io::Cursor::new(Vec::new());
                image::DynamicImage::ImageRgba8(img).write_to(&mut png, image::ImageFormat::Png)?;
                app.open_settings("profil");
                app.stage_avatar_image(png.into_inner());
            }
            if args.scene == Some(Scene::Glyphes) {
                app.open_gallery("glyphes");
            }
            if args.scene == Some(Scene::PolicesNom) {
                app.open_gallery("polices-nom");
            }
            if let Some(page) = &args.gallery
                && !app.open_gallery(page)
            {
                return Err(format!("--gallery : page inconnue « {page} »").into());
            }
            if args.panel {
                app.open_appearance();
            }
            if matches!(args.scene, Some(Scene::Saisie | Scene::SaisieLongue | Scene::Reponse)) {
                app.open(Some(ALPHA), HORS_SUJET);
                app.set_composer_text("Brouillon pour plus tard");
                app.open(Some(ALPHA), GENERAL);
                // Envoyé à l'instant : le mode lent compte 30 s.
                app.send(GENERAL, "Je regarde ça tout de suite");
                if args.scene == Some(Scene::SaisieLongue) {
                    let line = "Compte rendu de la réunion, point par point, avec tous les détails utiles. ";
                    let text: Vec<String> = (0..26).map(|i| format!("{}. {line}", i + 1)).collect();
                    app.set_composer_text(&text.join("\n"));
                } else {
                    app.set_composer_text(if args.scene == Some(Scene::Reponse) { "Bien vu" } else { "Salut @al" });
                    if args.scene == Some(Scene::Reponse) {
                        app.preview_reply("bob_fictif");
                    }
                }
            }
            if args.scene == Some(Scene::Parcourir) {
                app.show_browse(ALPHA);
            }
            if matches!(args.scene, Some(Scene::Fichiers | Scene::FichierDetails)) {
                app.open(Some(ALPHA), GENERAL);
                app.attach_bytes("vacances.png", generate("apercu://media/3/640x400").unwrap_or_default());
                app.attach_bytes("notes.txt", b"Liste des courses".to_vec());
                // Plus gros que la limite sans Nitro : signalé en rouge.
                app.attach_bytes("video-longue.mp4", vec![0u8; 12 << 20]);
                app.set_composer_text("Voici les fichiers");
                if args.scene == Some(Scene::FichierDetails) {
                    app.show_attachment_details(GENERAL, 0);
                }
            }
            if args.scene == Some(Scene::Applications) {
                app.open(Some(ALPHA), GENERAL);
                if app.open_command("sondage") {
                    app.set_command_field(0, "Pizza ou pâtes ?");
                }
            }
            // Paramètres du serveur (lot A6).
            {
                use ferrocord::ui::guild_settings::Page;
                match args.scene {
                    Some(Scene::ParametresEmojis) => {
                        app.show_guild_settings(ALPHA, Page::Emojis);
                    }
                    Some(Scene::ParametresAutocollants) => {
                        app.show_guild_settings(ALPHA, Page::Stickers);
                        if let Some(img) = generate("apercu://autocollant/7/640") {
                            app.pick_expression_bytes(Page::Stickers, "coucou.jpg", img);
                        }
                    }
                    Some(Scene::ParametresSons) => {
                        app.show_guild_settings(ALPHA, Page::Sounds);
                    }
                    _ => {}
                }
            }
            // Vocal (phase 4) : micro coupé retenu dans `local.toml`, Ève rendue muette pour moi.
            if matches!(args.scene, Some(Scene::Vocal | Scene::VocalCodes)) {
                let mut p = app.local_prefs();
                p.voice.mute = true;
                p.voice.set_user(1_000_000_000_000_000_006, ferrocord::state::voice::UserVoice { volume: 100, muted: true });
                app.set_local_prefs(p);
            }
            // Plugins vocaux (lot E4).
            if matches!(args.scene, Some(Scene::Panneau | Scene::PanneauMenu)) {
                app.user_panel_preview(args.scene == Some(Scene::PanneauMenu));
            }
            // Jeu détecté dans un vocal : le sélecteur de partage est relié, son bouton se montre.
            if args.scene == Some(Scene::JeuDetecteAppel) {
                app.enable_share_picker();
            }
            if let Some(Scene::VoixE4(n)) = args.scene {
                app.voice_e4_preview(n);
            }
            if matches!(args.scene, Some(Scene::Effet | Scene::EffetFixe)) {
                let mut p = app.local_prefs();
                p.motion = if args.scene == Some(Scene::EffetFixe) { ferrocord::state::local::Motion::On } else { ferrocord::state::local::Motion::Off };
                app.set_local_prefs(p);
            }
            if matches!(args.scene, Some(Scene::Parures | Scene::ParuresAnimees)) {
                let mut p = app.local_prefs();
                // `On` : « réduire les animations » activé.
                p.motion = if args.scene == Some(Scene::Parures) { ferrocord::state::local::Motion::On } else { ferrocord::state::local::Motion::Off };
                app.set_local_prefs(p);
            }
            if args.scene == Some(Scene::SuggestionsAutocollants) {
                app.open(Some(ALPHA), GENERAL);
                app.set_composer_text("dan");
            }
            if args.scene == Some(Scene::Emojis) {
                app.open(Some(ALPHA), GENERAL);
                app.set_composer_text("Le sélecteur :fire: :ferris: ");
                app.open_emoji_picker();
            }
            if matches!(args.scene, Some(Scene::Gifs | Scene::GifsRecherche | Scene::GifsFavoris)) {
                app.open(Some(ALPHA), GENERAL);
                app.show_gifs(args.scene == Some(Scene::GifsFavoris), (args.scene == Some(Scene::GifsRecherche)).then_some("chat"));
            }
            if args.scene == Some(Scene::SuperReaction) {
                app.open(Some(ALPHA), GENERAL);
                app.open_reaction_picker(5_000_000_000_000_000_950, eframe::egui::pos2(500.0, 120.0), true);
            }
            match args.scene {
                Some(Scene::Serveurs | Scene::VocalRail) => {
                    app.unfold_rail_folders();
                }
                Some(Scene::ServeursAjouter) => {
                    app.show_server_dialog("choisir");
                }
                Some(Scene::ServeursInviter) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.show_invites(ALPHA, false);
                    app.generate_invite();
                }
                Some(Scene::ServeursInvitations) => {
                    app.show_invites(ALPHA, true);
                }
                Some(Scene::ParametresServeur(page)) => {
                    app.open(Some(ALPHA), GENERAL);
                    if !app.show_guild_settings(ALPHA, page) {
                        eprintln!("Paramètres du serveur : page {page:?} non permise");
                    }
                }
                Some(Scene::Salons) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.show_channel_settings(GENERAL, "vue");
                }
                Some(Scene::EnteteSujet) => {
                    app.show_head_popup("sujet");
                }
                Some(Scene::EnteteFils) => {
                    app.show_head_popup("fils");
                }
                Some(Scene::EnteteArchives) => {
                    app.show_head_popup("archives");
                }
                Some(Scene::EnteteCloche) => {
                    app.show_head_popup("cloche");
                }
                Some(Scene::MenuServeur) => {
                    app.show_head_popup("serveur");
                }
                Some(Scene::CategoriesAmis) => {
                    let mut p = app.local_prefs();
                    p.friend_groups.create("Jeux du soir");
                    p.friend_groups.create("Travail");
                    p.friend_groups.set("Jeux du soir", AMIS_FRIENDS[0].0, true);
                    p.friend_groups.set("Jeux du soir", AMIS_FRIENDS[2].0, true);
                    p.friend_groups.set("Travail", AMIS_FRIENDS[1].0, true);
                    // Alice a un MP ouvert : il passe sous l'en-tête de sa catégorie.
                    p.friend_groups.set("Jeux du soir", 1_000_000_000_000_000_002, true);
                    app.set_local_prefs(p);
                }
                Some(Scene::MenuUtilisateur) => {
                    app.show_user_menu(1_000_000_000_000_000_002, "Alice", egui::pos2(620.0, 150.0));
                }
                Some(Scene::Decouverte) => {
                    app.show_discovery();
                }
                Some(Scene::Resumes) => {
                    app.show_summaries(GENERAL);
                }
                Some(Scene::Verification) => {
                    app.show_verification(ALPHA);
                    app.show_join_requests();
                }
                Some(Scene::Applis) => {
                    app.show_app_directory();
                }
                Some(Scene::EtatCompte) => {
                    app.open_settings("confidentialite");
                    app.check_account_standing();
                }
                Some(Scene::Enregistres) => {
                    app.show_inbox_tab("enregistres");
                }
                Some(Scene::Suggestions) => {
                    app.show_friend_suggestions();
                }
                Some(Scene::Inventaire) => {
                    app.show_inventory();
                }
                Some(Scene::Boutique) => {
                    app.show_shop();
                }
                Some(Scene::Quetes) => {
                    app.show_quests(false);
                }
                Some(Scene::Demandes) => {
                    app.show_requests(true);
                }
                Some(Scene::MenuPlus) => {
                    app.open_plus_menu();
                }
                Some(Scene::MentionRole) => {
                    app.preview_role_mention(2_000_000_000_000_001_001, "Modération");
                }
                Some(Scene::Galerie) => {
                    app.show_forum_gallery(true);
                }
                Some(Scene::PseudoAmi) => {
                    app.show_friend_nick(1_000_000_000_000_000_003);
                }
                Some(Scene::SalonVocal) => {
                    app.show_channel_settings(VOICE_ROOM, "vue");
                }
                Some(Scene::SalonsPermissions) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.show_channel_settings(GENERAL, "permissions");
                }
                Some(Scene::SalonsCreer) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.show_create_channel(ALPHA, Some(DISCUSSIONS), false);
                }
                Some(Scene::SalonsServeur) => {
                    app.open(Some(ALPHA), GENERAL);
                    if !app.show_guild_settings(ALPHA, ferrocord::ui::guild_settings::Page::Channels) {
                        eprintln!("Paramètres du serveur : page Salons non permise");
                    }
                }
                Some(Scene::Roles) => {
                    app.show_guild_settings(ATELIER, ferrocord::ui::guild_settings::Page::Roles);
                }
                Some(Scene::Role) => {
                    app.show_role_editor(ATELIER, R_ANIM, "affichage");
                }
                Some(Scene::RolePermissions) => {
                    app.show_role_editor(ATELIER, R_MODO, "permissions");
                }
                Some(Scene::Membres) => {
                    app.show_guild_settings(ATELIER, ferrocord::ui::guild_settings::Page::Members);
                }
                Some(Scene::MembreRoles) => {
                    app.show_member_roles(ATELIER, CAMILLE);
                }
                Some(Scene::Moderation(page)) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.show_guild_settings(ALPHA, page);
                }
                Some(Scene::ModerationMembre) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.show_member_action(1_000_000_000_000_000_002, "Alice", "bannir");
                }
                _ => {}
            }
            // Événements et accueil des nouveaux membres (lot A7).
            match args.scene {
                Some(Scene::Evenements) => app.show_events(ALPHA),
                Some(Scene::EvenementCreer) => app.show_event_editor(ALPHA, "Tournoi d'échecs"),
                Some(Scene::Bienvenue) => app.show_arrival(COMMUNITY_GUILD, false),
                Some(Scene::Integration) => app.show_arrival(COMMUNITY_GUILD, true),
                Some(Scene::ParametresIntegration) => {
                    app.show_guild_settings(COMMUNITY_GUILD, ferrocord::ui::GuildPage::Onboarding);
                }
                Some(Scene::ParametresBienvenue) => {
                    app.show_guild_settings(COMMUNITY_GUILD, ferrocord::ui::GuildPage::Welcome);
                }
                _ => {}
            }
            if args.scene == Some(Scene::VocalCodes) {
                app.show_voice_codes();
            }
            if args.scene == Some(Scene::LienExterne) {
                app.open_link("https://boutique.exemple.invalid/produits/clavier-mecanique?couleur=noir&reference=ABC-1234567890&suivi=campagne-de-novembre");
            }
            if matches!(args.scene, Some(Scene::Lecteur | Scene::LecteurPlein)) {
                use ferrocord::video::play::{Handle, Info, Phase, Picture};
                // Mire fixe à la place d'une image décodée : dégradé et bandes.
                let (w, h) = (640u32, 360u32);
                let pixels = (0..w * h).flat_map(|i| {
                    let (x, y) = (i % w, i / w);
                    let band = if (x / 80) % 2 == 0 { 40 } else { 0 };
                    [(x * 200 / w) as u8 + band, (y * 160 / h) as u8 + 40, 150 - (x * 100 / w) as u8, 255]
                });
                let info = Info { duration_us: 95_000_000, video: Some((1280, 720)), rotation: 0, audio: true };
                let handle = Handle::fixed(Phase::Paused, info, 30_000_000, Some(Picture { width: w, height: h, pixels: pixels.collect() }));
                ferrocord::ui::player::preview(&cc.egui_ctx, PLAYER_VIDEO, handle, true, args.scene == Some(Scene::LecteurPlein));
            }
            // Vue d'appel (phase 5) : mires animées poussées comme le ferait le décodeur.
            if matches!(args.scene, Some(Scene::Appel | Scene::AppelDirect | Scene::AppelPartage)) {
                app.enable_share_picker();
                app.show_call(Some(VOICE_GUILD), VOICE_ROOM);
                spawn_test_patterns(app.video_sink(), args.screenshot.is_some());
            }
            // Appels, conférences, sons (lot V6).
            match args.scene {
                Some(Scene::Sonnerie) => app.open(None, DM),
                Some(Scene::AppelMp | Scene::AppelSortant) => {
                    app.enable_share_picker();
                    app.open(None, DM);
                }
                Some(Scene::AppelGroupe) => {
                    app.enable_share_picker();
                    app.open(None, DM_GROUP);
                }
                Some(Scene::AppelMpPartage) => {
                    app.enable_share_picker();
                    app.open(None, DM);
                    spawn_test_patterns(app.video_sink(), args.screenshot.is_some());
                }
                Some(Scene::Conference) => app.open_voice_room(CLUB, CLUB_SCENE),
                Some(Scene::Sons) => {
                    app.open_voice_room(CLUB, CLUB_VOCAL);
                    app.show_soundboard(CLUB_VOCAL);
                }
                _ => {}
            }
            // Sélecteur de partage (lot M3) : sources et images synthétiques.
            if let Some(Scene::Partage(n)) = args.scene {
                use ferrocord::state::share::ShareTab;
                app.open(Some(ALPHA), GENERAL);
                app.attach_capture(Box::new(PreviewCapture::default()));
                app.open_share_picker(match n {
                    1 => ShareTab::Screens,
                    2 | 3 => ShareTab::Cameras,
                    _ => ShareTab::Windows,
                });
            }
            if args.scene == Some(Scene::Recherche) {
                app.open(Some(ALPHA), GENERAL);
                app.search("rendez-vous");
            }
            // Navigation (lot navigation) : sélecteur rapide, boîte de réception, raccourcis.
            match args.scene {
                Some(Scene::Selecteur) => {
                    app.open(Some(ALPHA), HORS_SUJET);
                    app.open(Some(ALPHA), GENERAL);
                    app.open_switcher();
                    app.switcher_type("gen");
                }
                Some(Scene::Boite) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.toggle_inbox();
                }
                Some(Scene::BoiteCartes) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.toggle_inbox();
                    for c in [DM, GAMMA_SALON, HORS_SUJET] {
                        app.inbox_fold(c);
                    }
                }
                Some(Scene::BoiteProgrammes) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.scheduled_preview(false, false);
                    app.show_inbox_tab("programmes");
                }
                Some(Scene::BoiteMentions) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.show_inbox_tab("mentions");
                }
                Some(Scene::MonCompte | Scene::MonCompteSobre | Scene::MonCompteClair | Scene::MonCompteJeu) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.open_account_card();
                }
                Some(Scene::MonCompteStatut) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.open_status_submenu();
                }
                Some(Scene::MonCompteStatutPerso) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.open_custom_status();
                }
                Some(Scene::ProfilEdition | Scene::ProfilEditionServeur | Scene::ProfilEditionSansNitro | Scene::ProfilEditionStyle) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.open_profile_editor(args.scene == Some(Scene::ProfilEditionServeur));
                    if args.scene == Some(Scene::ProfilEditionStyle) {
                        app.profile_editor_style(16);
                    }
                    if args.scene == Some(Scene::ProfilEdition) {
                        // Bannière synthétique choisie (dégradé) : aperçu en direct, recadrage proposé.
                        let img = image::RgbaImage::from_fn(680, 240, |x, y| image::Rgba([(40 + x * 120 / 680) as u8, (60 + y * 90 / 240) as u8, 170, 255]));
                        let mut png = std::io::Cursor::new(Vec::new());
                        image::DynamicImage::ImageRgba8(img).write_to(&mut png, image::ImageFormat::Png)?;
                        app.stage_profile_image(true, png.into_inner());
                    }
                }
                Some(Scene::RichPresence) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.open_profile(ME, Some(ALPHA), egui::pos2(330.0, 90.0));
                }
                Some(Scene::Activite) => {
                    app.open(Some(ALPHA), GENERAL);
                    let mut p = app.local_prefs();
                    p.games = ferrocord::state::activity::ManualGame::from_path(r"D:\Outils\Editeur de cartes.exe").into_iter().collect();
                    app.set_local_prefs(p);
                    if args.settings_page.is_none() {
                        app.open_profile(1_000_000_000_000_000_002, Some(ALPHA), egui::pos2(330.0, 90.0));
                    }
                }
                Some(Scene::Raccourcis) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.open_shortcuts();
                }
                Some(Scene::AmisAjouter) => {
                    app.show_add_friend("", "");
                }
                Some(Scene::FilMembres) => {
                    if let Some(c) = app.store().channel {
                        app.thread_members_preview(c, vec![(1_000_000_000_000_000_002, "Alice".into()), (1_000_000_000_000_000_003, "bob_fictif".into()), (1_000_000_000_000_000_004, "Chloé".into())]);
                    }
                }
                Some(Scene::NotifsServeur) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.show_notif_settings(ALPHA, "a");
                    app.notif_settings_add(HORS_SUJET);
                    app.notif_settings_add(ANNONCES);
                }
                Some(Scene::PourVous) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.show_inbox_tab("pour-vous");
                }
                Some(Scene::Fenetres(which)) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.windows_preview(which, &[(Some(ALPHA), HORS_SUJET), (Some(ALPHA), ANNONCES), (None, DM_GROUP)]);
                }
                Some(Scene::Journal) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.journal_preview();
                }
                Some(Scene::Export(reading)) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.export_preview(reading);
                }
                Some(Scene::Mica(acrylic)) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.mica_preview(acrylic);
                }
                Some(Scene::ThemeImport) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.theme_share_preview();
                }
                Some(Scene::Plantage) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.crash_preview();
                }
                Some(Scene::E3(n)) => {
                    app.open(Some(ALPHA), GENERAL);
                    match n {
                        0 => app.stealth_preview(),
                        1 => app.scheduled_preview(false, false),
                        2 => app.scheduled_preview(true, false),
                        3 => app.scheduled_preview(false, true),
                        4 => app.translate_preview(),
                        _ => app.translate_preview_settings(),
                    }
                }
                Some(Scene::RechercheLocale) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.local_search_preview("rendez");
                }
                Some(Scene::MotsCles(settings)) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.keywords_preview(&["réunion", "citation"]);
                    if settings {
                        app.open_settings("notifications");
                    } else {
                        app.show_inbox_tab("pour-vous");
                    }
                }
                Some(Scene::ApparenceEditeur) => {
                    app.open(Some(ALPHA), GENERAL);
                    app.look_library(vec!["Forêt en hiver".into(), "Néon".into()], vec!["Travail".into(), "Soirée".into()]);
                    app.open_appearance();
                }
                Some(Scene::ThemeDegrade) => {
                    app.open(Some(ALPHA), GENERAL);
                    let mut p = app.local_prefs();
                    p.look.gradient.on = true;
                    p.look.gradient.strength = 45;
                    app.set_local_prefs(p);
                }
                Some(Scene::FondImage) => {
                    app.open(Some(ALPHA), GENERAL);
                    let mut p = app.local_prefs();
                    p.look.backdrop = ferrocord::state::look_prefs::Backdrop { path: "aperçu".into(), scope: ferrocord::state::look_prefs::Scope::App, opacity: 45, blur: 6, dim: 35 };
                    app.set_local_prefs(p);
                    // Paysage synthétique : ciel en dégradé, soleil, collines.
                    let (w, h) = (640usize, 360usize);
                    let mut image = egui::ColorImage::filled([w, h], egui::Color32::BLACK);
                    for y in 0..h {
                        for x in 0..w {
                            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
                            let sun = (1.0 - ((fx - 0.7).powi(2) + (fy - 0.35).powi(2)).sqrt() * 4.0).clamp(0.0, 1.0);
                            let hill = fy > 0.72 + 0.08 * (fx * 9.0).sin() + 0.04 * (fx * 23.0).cos();
                            let sky = [40.0 + 180.0 * fy + 120.0 * sun, 30.0 + 90.0 * fy + 110.0 * sun, 110.0 + 60.0 * (1.0 - fy) + 20.0 * sun];
                            let c = if hill { [18.0, 40.0 + 30.0 * fx, 52.0] } else { sky };
                            image[(x, y)] = egui::Color32::from_rgb(c[0].min(255.0) as u8, c[1].min(255.0) as u8, c[2].min(255.0) as u8);
                        }
                    }
                    app.look_backdrop_image(image);
                }
                Some(Scene::SansRail) => {
                    app.open(Some(ALPHA), GENERAL);
                    let mut p = app.local_prefs();
                    p.look.hide_rail = true;
                    p.look.name_color = ferrocord::state::look_prefs::NameColor::Dot;
                    app.set_local_prefs(p);
                }
                Some(Scene::Densites(airy)) => {
                    app.open(Some(ALPHA), GENERAL);
                    let mut t = ferrocord::theme::Preset::DEFAULT.tokens();
                    t.layout.density = if airy { ferrocord::theme::Density::Airy } else { ferrocord::theme::Density::Compact };
                    app.set_theme(t);
                }
                Some(Scene::Accessibilite) => {
                    app.open(Some(ALPHA), GENERAL);
                    let mut p = app.local_prefs();
                    p.streamer.on = true;
                    app.set_local_prefs(p);
                    app.select_last();
                }
                _ => {}
            }
            // Disposition mobile imposée, sur l'écran demandé (phase 7).
            if let Some(screen) = args.mobile.as_deref() {
                let mut p = app.local_prefs();
                p.mobile.layout = ferrocord::state::mobile::Layout::Mobile;
                app.set_local_prefs(p);
                if args.scene.is_none() && args.view == View::Guild {
                    app.open(Some(ALPHA), GENERAL);
                }
                let screen = if screen == "appui-long" { "discussion" } else { screen };
                if !app.mobile_show(screen) {
                    eprintln!("--mobile : écran inconnu « {screen} »");
                }
            }
            if args.scene == Some(Scene::ProfilCarte) && !args.profile_full {
                app.open_profile(1_000_000_000_000_000_002, None, egui::pos2(330.0, 40.0));
            } else if args.profile_full {
                app.open_full_profile(1_000_000_000_000_000_002, Some(ALPHA));
            } else if args.profile {
                app.open_profile(1_000_000_000_000_000_002, Some(ALPHA), egui::pos2(330.0, 90.0));
            }
            let audit = args.audit.then(|| {
                // Thème lu dans un fichier : l'audit part de ce thème-là.
                let from_file = || std::fs::read_to_string(args.theme.as_deref()?).ok().and_then(|s| ferrocord::theme::parse(&s).ok());
                let mut base = base.clone().or_else(from_file).unwrap_or_default();
                let keys = base
                    .fields_mut()
                    .iter()
                    .map(|f| format!("{}.{}", f.section, f.key))
                    .enumerate()
                    .filter(|(_, k)| args.only.is_empty() || args.only.contains(k))
                    .collect();
                Audit { base, keys, step: 0, reference: None, quiet: Vec::new() }
            });
            Ok(Box::new(Preview {
                app,
                open_menu: matches!(args.view, View::Selector | View::Migration | View::WipeQuestion | View::RecentWipe | View::Twin | View::TwinRemove),
                view: args.view,
                _rt: rt,
                shot: args.screenshot,
                at: args.at,
                hover_none: args.hover_none || args.audit,
                pointer: args.pointer,
                pointer_sent: false,
                audit,
                started: Instant::now(),
                // Appui long : le menu s'ouvre 0,8 s après l'appui (phase 7).
                // Parures : bannières et décorations synthétiques sont longues à
                // produire sans optimisation ; la capture les attend.
                settle: if args.mobile.as_deref() == Some("appui-long") {
                    SETTLE + Duration::from_millis(1500)
                } else if matches!(args.scene, Some(Scene::Parures | Scene::ParuresAnimees | Scene::Effet | Scene::EffetFixe | Scene::ProfilCarte)) {
                    SETTLE + Duration::from_millis(3500)
                } else {
                    SETTLE
                },
                slot: Arc::default(),
                previous: None,
                waiting: false,
                done: false,
                report_position: args.position.is_some(),
                _old_file: old_file,
                scene: args.scene,
                inject,
                injected: false,
                pointer_released: false,
                long_press: (args.mobile.as_deref() == Some("appui-long")).then_some(false),
                frame_at: None,
            }))
        }),
    )
}

/// Écrit chaque son d'interface intégré en WAV dans `dir` (créé au besoin) :
/// pour les écouter sans lancer Ferrocord. Rien n'est joué.
fn write_sounds(dir: &std::path::Path) -> std::io::Result<usize> {
    use ferrocord::audio::sons;
    std::fs::create_dir_all(dir)?;
    for kind in ferrocord::state::notify::Kind::ALL {
        let name = sons::perso::noms(kind)[0];
        std::fs::write(dir.join(format!("{name}.wav")), sons::synth::wav(&sons::integre(kind)))?;
    }
    Ok(ferrocord::state::notify::Kind::ALL.len())
}
