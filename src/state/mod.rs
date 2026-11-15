//! Magasin central : le réseau y verse des `Event`, l'interface le lit et
//! émet des `Cmd`. Ne dépend ni d'egui ni de `net/`.

// Administration des serveurs (phase 3).
pub mod admin;
mod compose;
pub mod cosmetics;
pub mod crop;
pub mod emoji;
mod event;
pub mod guild_folders;
mod image;
pub mod local;
pub mod audio_prefs;
// Réglages vidéo et caméra de la page « Voix et vidéo » (lot restes).
pub mod video_prefs;
pub mod look_prefs;
// « Parcourir les salons » (lot fin-texte-restes).
pub mod browse;
/// Liste des salons : catégories repliées, salons muets masqués (phase 10, lot S).
pub mod chanlist;
// Navigation, boîte de réception, raccourcis (lot navigation).
pub mod inbox;
// Réveils de l'interface décidés par la passerelle (lot C8).
pub mod interest;
pub mod keys;
pub mod nav;
pub mod interactions;
pub mod markdown;
mod media;
pub mod members;
pub mod notify;
pub mod model;
mod notice;
pub mod people;
pub mod perms;
pub mod profiles;
pub mod reads;
pub mod relations;
pub mod servers;
pub mod search;
pub mod social;
pub mod store;
pub mod threads;
pub mod typing;
pub mod upload;
pub mod usersettings;
// Sondages, messages système, annonces, invitations (lot T13).
pub mod announce;
pub mod invites;
pub mod polls;
pub mod system_msgs;
// Vocal des serveurs (phase 4, lot V4).
pub mod voice;
/// Lien vers l'orchestrateur du vocal (lot V7).
pub mod call_rec;
pub mod voice_link;
pub mod voice_msg;
// Appels privés, conférences, tableau de sons (lot V6).
pub mod calls;
pub mod stage;
// Vidéo et directs (phase 5, lot M4).
pub mod stream;
pub mod video;
// Câblage de la vidéo (phase 5, lot M5).
pub mod video_link;
// Sélecteur de partage (phase 5, lot M3).
pub mod share;
// Activité de jeu détectée et présence enrichie (lot C3).
pub mod activity;
// Rich Presence locale : applications branchées sur le tube (lot R).
pub mod rpc;
// Connexion par QR code depuis l'application mobile (phase 6, lot C2).
pub mod remote_auth;
// Disposition mobile (phase 7, lot P2).
pub mod mobile;
// Identité annoncée à Discord (décision du 14 novembre 2026).
pub mod identity_prefs;
// Historique hors ligne (lot H).
pub mod offline;
// Sélecteur de GIF, favoris et fréquence du compte (lot G).
pub mod frecency;
/// Commandes intégrées de la saisie et `@silent` (lot FF).
pub mod builtin;
/// Usage local des emojis (lot FF).
pub mod emoji_usage;
pub mod gifs;
// Fonctions non documentées (lot ND) : effets de profil, quêtes.
pub mod effects;
pub mod quests;
pub mod tags;
// Widgets de jeux du profil (éditeur de profil).
pub mod widgets;
pub mod shop;
// Journal des messages supprimés et modifiés (plugin, lot E1).
pub mod journal;
/// Brouillons persistants (plugin du lot E3).
pub mod drafts;
/// Messages programmés, à envoi confirmé (plugin du lot E3).
pub mod scheduled;
/// Traduction d'un message (plugin du lot E3).
pub mod translate;
// Mots-clés surveillés (plugin, lot E1).
pub mod keywords;
// Onglets, vue scindée, fenêtres détachées, zoom par salon (lot E2).
pub mod windows;
// Index de la recherche locale (plugin, lot E1).
pub mod local_index;
// Export d'une conversation (plugin, lot E1).
pub mod export;
pub mod inventory;
pub mod discovery;
pub mod summaries;
pub mod verification;
pub mod appdir;
pub mod safety;
pub mod saved;
pub mod suggestions;

pub use event::{Cmd, DmKnown, Event, HISTORY_PAGE};
pub use notice::{Cause, Notice, Problem, ProfileRefusal, Status, SystemKind};
pub use people::PeopleHub;
pub use perms::{Permissions, Verdict};
pub use store::{Effect, SendBlock, Store, Unsent, UnsentState};
