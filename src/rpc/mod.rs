//! Rich Presence locale (lot R) : le serveur IPC que les jeux et logiciels
//! appellent pour afficher leur activité (« Joue à … », détails, images,
//! durée, groupe, boutons).
//!
//! Format public, sources (lues le 14 novembre 2026) :
//! - documentation Discord RPC (docs.discord.com/developers/topics/rpc) :
//!   charge utile `{ cmd, args, nonce }`, réponse `{ cmd, data, evt, nonce }`,
//!   READY (`v`, `config` : `cdn_host`, `api_endpoint`, `environment`,
//!   `user`) envoyé dans une trame FRAME après la poignée de main,
//!   SET_ACTIVITY (`pid`, `activity`, types 0, 2, 3 et 5 seulement),
//!   erreurs (`evt: "ERROR"`, `data: { code, message }`). Les numéros 4000
//!   (charge utile invalide) et 4002 (commande invalide) viennent de la table
//!   « RPC Error Codes » des versions antérieures de cette page ;
//! - bibliothèque officielle discord-rpc (github.com/discord/discord-rpc) :
//!   `src/connection_win.cpp` (tubes `\\?\pipe\discord-ipc-0` à `-9`,
//!   essayés dans l'ordre), `src/rpc_connection.h` (en-tête de deux u32,
//!   opcode puis longueur, petit-boutiste d'après l'exemple de la
//!   documentation ; opcodes 0 HANDSHAKE, 1 FRAME, 2 CLOSE, 3 PING, 4 PONG ;
//!   trame de 64 Kio au plus), `src/serialization.cpp` (poignée de main
//!   `{ "v", "client_id" }` ; activité : `state`, `details`, `timestamps`,
//!   `assets`, `party` `id`/`size`, `secrets`, `instance`) ;
//! - les horodatages du SDK sont en secondes (exemple `time(nullptr)` de la
//!   documentation), ceux de la passerelle en millisecondes : une valeur
//!   trop petite pour des millisecondes est convertie.
//!
//! Seule l'activité est acceptée : toute autre commande (AUTHORIZE,
//! AUTHENTICATE, SUBSCRIBE, invitations…) reçoit une erreur 4002 et rien
//! n'est fait pour le compte. Les secrets de groupe (rejoindre, regarder)
//! sont ignorés : nous ne savons pas répondre aux demandes qu'ils ouvrent.
//!
//! Rien ne part chez Discord d'ici : le serveur signale au magasin chaque
//! branchement, activité et départ ([`RpcEvent`]) ; l'activité choisie part
//! dans l'op 3 de la passerelle (`net::games`), au rythme de son limiteur.

mod conn;
mod proto;
#[cfg(windows)]
mod pipe;

pub use conn::session;
pub use proto::{activity_json, encode, Frame, OP_CLOSE, OP_FRAME, OP_HANDSHAKE, OP_PING, OP_PONG};

use crate::state::rpc::RpcEvent;
use std::sync::Arc;
use std::time::Duration;

/// Préfixe des vrais tubes ; le numéro (0 à 9) suit.
pub const PIPE_PREFIX: &str = "discord-ipc-";
/// Nombre de tubes essayés (discord-rpc : 0 à 9).
pub const PIPES: u8 = 10;
/// Serveur disponible sur ce système (tubes nommés de Windows).
pub const SUPPORTED: bool = cfg!(windows);
/// Trame plus longue : connexion fermée (une activité tient en quelques Kio).
pub const MAX_FRAME: usize = 64 * 1024;
/// Attente maximale de la poignée de main.
pub const HANDSHAKE_WAIT: Duration = Duration::from_secs(10);
/// Écart minimal entre deux activités d'une même application remontées au
/// magasin : la dernière reçue part à la fin de l'écart. L'op 3 a son
/// propre limiteur (5 par 20 s, `net::games`).
pub const ACTIVITY_GAP: Duration = Duration::from_secs(1);

/// Où vont les événements du serveur.
pub type Sink = Arc<dyn Fn(RpcEvent) + Send + Sync>;
/// Nom du programme d'un processus (d'après son `pid`), lu hors du fil courant.
pub type ProcName = fn(u32) -> Option<String>;

/// Nom du programme (`C:\Jeux\Mon Jeu.exe` → « Mon Jeu »), d'après la
/// liste des processus de cet ordinateur.
pub fn process_name(pid: u32) -> Option<String> {
    let d = crate::procs::details(pid)?;
    let file = d.path.rsplit(['/', '\\']).next()?;
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    let name: String = stem.chars().filter(|c| !c.is_control()).take(crate::state::rpc::MAX_TEXT).collect();
    (!name.trim().is_empty()).then_some(name)
}

/// Serveur tant que le canal vit : `Some(préfixe)` l'ouvre sur le premier
/// tube libre, `None` le ferme (applications débranchées).
#[cfg(windows)]
pub async fn serve(rx: tokio::sync::watch::Receiver<Option<String>>, sink: Sink) {
    pipe::serve(rx, sink, process_name).await;
}

/// Ailleurs que sous Windows : pas de serveur (les sockets Unix de Linux et
/// macOS viendront avec le lot C7) ; le magasin apprend qu'aucun tube n'est ouvert.
#[cfg(not(windows))]
pub async fn serve(mut rx: tokio::sync::watch::Receiver<Option<String>>, sink: Sink) {
    loop {
        if rx.borrow_and_update().is_some() {
            sink(RpcEvent::Listening(None));
        }
        if rx.changed().await.is_err() {
            return;
        }
    }
}
