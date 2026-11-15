//! Raccordement entre l'interface et le réseau : runtime tokio, connexion,
//! réception des événements et envoi des commandes. `ui/` ne connaît que le
//! trait `ui::Backend`, que `Session` implémente. Les jetons enregistrés
//! sont l'affaire de `account` (coffre), pas de la session.

use crate::account::{Token, TokenError};
use crate::net::{self, Connection, Endpoints};
use crate::state::{Cmd, Event};
use crate::ui::{Backend, Wake};
use std::sync::Arc;

pub struct Session {
    rt: tokio::runtime::Runtime,
    conn: Option<Connection>,
    wake: Option<Wake>,
    endpoints: Endpoints,
    watch: Arc<crate::state::members::ListWatch>,
    interest: Arc<crate::state::interest::Interest>,
}

impl Session {
    /// Session vers Discord (adresses de production, non réglables), avec
    /// l'identité de Ferrocord.
    pub fn new() -> Self {
        Self::with_endpoints(Endpoints::default())
    }

    /// Session vers Discord avec l'identité choisie pour ce lancement
    /// (`net::identity::Config::for_app`).
    pub fn with_identity(identity: net::identity::Config) -> Self {
        Self::with_endpoints(Endpoints { identity, ..Endpoints::default() })
    }

    /// Session vers d'autres adresses : tests seulement (faux serveur local).
    pub fn with_endpoints(endpoints: Endpoints) -> Self {
        // Petit runtime : 2 threads suffisent largement pour un client.
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime tokio");
        Self { rt, conn: None, wake: None, endpoints, watch: Arc::default(), interest: Arc::default() }
    }

    /// Runtime partagé avec le chargement des images.
    pub fn handle(&self) -> &tokio::runtime::Handle {
        self.rt.handle()
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend for Session {
    fn set_wake(&mut self, wake: Wake) {
        self.wake = Some(wake);
    }

    /// Ouvre une connexion (la précédente est fermée d'abord) ; le réveil est
    /// appelé à chaque événement reçu.
    fn connect(&mut self, token: Token) {
        self.conn = None;
        let wake = self.wake.clone().unwrap_or_else(|| Arc::new(|| {}));
        // Le mode (bureau ou Ferrocord) est relu : un changement du réglage
        // vaut pour cette connexion ; versions et machine restent celles du lancement.
        let endpoints = Endpoints { identity: self.endpoints.identity.reread_mode(), ..self.endpoints.clone() };
        self.conn = Some(net::connect_filtered(self.rt.handle(), token, endpoints, wake, self.watch.clone(), self.interest.clone()));
    }

    fn set_list_watch(&mut self, watch: Arc<crate::state::members::ListWatch>) {
        self.watch = watch;
    }

    fn set_interest(&mut self, interest: Arc<crate::state::interest::Interest>) {
        self.interest = interest;
    }

    /// Ferme la connexion : ses tâches sont arrêtées et les événements
    /// qu'elle n'a pas encore livrés sont jetés avec elle.
    fn disconnect(&mut self) {
        self.conn = None;
    }

    fn is_connected(&self) -> bool {
        self.conn.is_some()
    }

    fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        if let Some(c) = &self.conn {
            while let Ok(e) = c.events.try_recv() {
                events.push(e);
            }
        }
        events
    }

    /// Les commandes sont perdues s'il n'y a pas de connexion.
    fn send(&mut self, cmds: Vec<Cmd>) {
        for cmd in cmds {
            if let Some(c) = &self.conn {
                let _ = c.cmd.send(cmd);
            }
        }
    }

    /// Passerelle d'authentification à distance de Discord.
    fn remote_auth(&mut self) -> Option<crate::state::remote_auth::RemoteLogin> {
        let ep = net::remote_auth::RemoteAuthEndpoints { identity: self.endpoints.identity.reread_mode(), ..Default::default() };
        // Session de test (faux serveur) : jamais la vraie passerelle.
        if ep.api != self.endpoints.api {
            return None;
        }
        let wake = self.wake.clone().unwrap_or_else(|| Arc::new(|| {}));
        Some(net::remote_auth::start(self.rt.handle(), ep, wake))
    }
}

/// Jeton de `DISCORD_TOKEN` : session éphémère, jamais enregistrée ni oubliée.
/// `None` si la variable est absente ou vide ; `Some(Err)` si son contenu
/// n'est pas un jeton (caractère invisible, espace interne…).
pub fn env_token() -> Option<Result<Token, TokenError>> {
    let text = std::env::var("DISCORD_TOKEN").ok()?;
    match Token::parse(&text) {
        Err(TokenError::Empty) => None,
        other => Some(other),
    }
}
