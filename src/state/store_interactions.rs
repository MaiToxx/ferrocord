//! Commandes et composants des applications (lot T11), côté magasin :
//! module enfant de `store` pour lire le serveur d'un salon et toucher au
//! cache des messages (réponse éphémère ignorée, réponse différée résolue).

use super::super::event::Cmd;
use super::super::interactions::*;
use super::{now_ms, Effect, Store};

/// Commande d'un index, avec son application.
#[derive(Clone, Copy, Debug)]
pub struct CommandRef<'a> {
    pub cmd: &'a Command,
    pub app: Option<&'a AppInfo>,
}

impl CommandRef<'_> {
    pub fn app_name(&self) -> &str {
        self.app.map_or("", |a| a.name.as_str())
    }
}

impl Store {
    /// Serveur d'un salon (fil compris) ; `None` : MP ou groupe.
    pub fn interaction_guild(&self, channel_id: u64) -> Option<u64> {
        self.chan_guild.get(&channel_id).copied().or_else(|| self.threads.get(channel_id).map(|t| t.guild_id))
    }

    /// Index qui valent pour ce salon : celui du serveur (ou du MP) et celui du compte.
    fn scopes(&self, channel_id: u64) -> [Scope; 2] {
        match self.interaction_guild(channel_id) {
            Some(g) => [Scope::Guild(g), Scope::User],
            None => [Scope::Channel(channel_id), Scope::User],
        }
    }

    /// Demande les index de ce salon s'ils manquent ou sont périmés (liste
    /// des commandes ouverte, menu « Applications » ouvert : un geste).
    pub fn want_commands(&mut self, channel_id: u64) {
        let now = now_ms();
        for scope in self.scopes(channel_id) {
            if self.interactions.wants(scope, now) {
                self.interactions.mark_loading(scope, now);
                self.push_command(Cmd::Interactions(InteractionCmd::Index { scope }));
            }
        }
    }

    /// Un index de ce salon est encore attendu (et aucun n'est là).
    pub fn commands_loading(&self, channel_id: u64) -> bool {
        let scopes = self.scopes(channel_id);
        scopes.iter().any(|s| self.interactions.loading(*s)) && scopes.iter().all(|s| self.interactions.index(*s).is_none())
    }

    /// Commandes de ce type utilisables ici, sans doublon (une application
    /// installée sur le serveur et sur le compte n'apparaît qu'une fois).
    pub fn app_commands(&self, channel_id: u64, kind: CommandKind) -> Vec<CommandRef<'_>> {
        let mut out: Vec<CommandRef<'_>> = Vec::new();
        for scope in self.scopes(channel_id) {
            let Some(index) = self.interactions.index(scope) else { continue };
            for cmd in index.commands.iter().filter(|c| c.kind == kind) {
                if out.iter().any(|r| r.cmd.id == cmd.id) {
                    continue;
                }
                out.push(CommandRef { cmd, app: index.apps.iter().find(|a| a.id == cmd.app_id) });
            }
        }
        out
    }

    fn command_by_id(&self, channel_id: u64, id: u64) -> Option<CommandRef<'_>> {
        [CommandKind::ChatInput, CommandKind::User, CommandKind::Message].into_iter().flat_map(|k| self.app_commands(channel_id, k)).find(|r| r.cmd.id == id)
    }

    fn outgoing(&mut self, channel_id: u64, application_id: u64, kind: OutKind) -> Option<Outgoing> {
        let session = self.interactions.session()?.clone();
        let nonce = self.interactions.next_nonce(now_ms());
        Some(Outgoing { nonce, session, application_id, guild_id: self.interaction_guild(channel_id), channel_id, kind, files: Vec::new() })
    }

    fn launch(&mut self, out: Outgoing, app_name: String, what: Option<What>) {
        if let Some(what) = what {
            let p = Pending { nonce: out.nonce, channel_id: out.channel_id, app_name, what, id: None, since: now_ms(), state: PendingState::Sending };
            self.interactions.track(p);
        }
        self.push_command(Cmd::Interactions(InteractionCmd::Send(Box::new(out))));
    }

    /// Lance une commande slash (`path` : sous-commande) ou de menu
    /// contextuel (`target_id`). `false` : rien n'est parti (commande
    /// inconnue, session pas encore ouverte).
    pub fn run_command(&mut self, channel_id: u64, command_id: u64, path: Vec<String>, options: Vec<Filled>, target_id: Option<u64>) -> bool {
        self.run_command_with_files(channel_id, command_id, path, options, target_id, Vec::new())
    }

    /// `run_command` avec les fichiers des options pièce jointe
    /// (`Value::Attachment(n)` désigne `files[n]`).
    pub fn run_command_with_files(&mut self, channel_id: u64, command_id: u64, path: Vec<String>, options: Vec<Filled>, target_id: Option<u64>, files: Vec<crate::state::upload::OutFile>) -> bool {
        let Some(r) = self.command_by_id(channel_id, command_id) else { return false };
        let (cmd, app_name) = (r.cmd.clone(), r.app_name().to_string());
        let shown = std::iter::once(cmd.name.as_str()).chain(path.iter().map(String::as_str)).collect::<Vec<_>>().join(" ");
        let kind = OutKind::Command { id: cmd.id, version: cmd.version, name: cmd.name.clone(), kind: cmd.kind, path, options, target_id };
        let Some(mut out) = self.outgoing(channel_id, cmd.app_id, kind) else { return false };
        out.files = files;
        self.launch(out, app_name, Some(What::Command(shown)));
        true
    }

    /// Demande au bot ses propositions pour l'option `focused`, en cours de
    /// frappe. Les anciennes propositions sont oubliées.
    pub fn autocomplete(&mut self, channel_id: u64, command_id: u64, path: Vec<String>, options: Vec<Filled>, focused: Filled) -> bool {
        let Some(r) = self.command_by_id(channel_id, command_id) else { return false };
        let cmd = r.cmd.clone();
        let kind = OutKind::Autocomplete { id: cmd.id, version: cmd.version, name: cmd.name.clone(), path, options, focused };
        let Some(out) = self.outgoing(channel_id, cmd.app_id, kind) else { return false };
        self.interactions.forget_choices();
        self.interactions.ask_choices(out.nonce);
        self.launch(out, String::new(), None);
        true
    }

    /// Clic sur un bouton ou choix dans un menu d'un message. Refusé si ce
    /// composant attend déjà sa réponse (un seul envoi par geste).
    pub fn press_component(&mut self, channel_id: u64, message_id: u64, component_type: u8, custom_id: &str, values: Option<Vec<String>>) -> bool {
        if self.interactions.component_busy(message_id, custom_id) {
            return false;
        }
        let Some(m) = self.find_message(channel_id, message_id) else { return false };
        let app = m.meta.app.as_deref();
        let flags = app.map_or(0, |a| a.flags);
        // Message d'un bot hors interaction : l'application est celle de son auteur.
        let application_id = app.and_then(|a| a.application_id).unwrap_or(m.author_id);
        let app_name = m.author.clone();
        let kind = OutKind::Component { message_id, message_flags: flags, component_type, custom_id: custom_id.to_string(), values };
        let Some(out) = self.outgoing(channel_id, application_id, kind) else { return false };
        self.launch(out, app_name, Some(What::Component { message_id, custom_id: custom_id.to_string() }));
        true
    }

    /// Fenêtre modale à montrer (prise par l'interface).
    pub fn take_modal(&mut self) -> Option<Box<Modal>> {
        self.interactions.modal.take()
    }

    /// Réponse à une fenêtre modale.
    pub fn submit_modal(&mut self, modal: &Modal, values: Vec<ModalValue>) -> bool {
        let kind = OutKind::ModalSubmit { interaction_id: modal.id, custom_id: modal.custom_id.clone(), values };
        let Some(out) = self.outgoing(modal.channel_id, modal.app_id, kind) else { return false };
        self.launch(out, modal.app_name.clone(), Some(What::Modal(modal.title.clone())));
        true
    }

    /// « Ignorer le message » d'une réponse éphémère : elle n'existe que
    /// chez moi, elle disparaît de l'affichage (rien n'est envoyé).
    pub fn dismiss_ephemeral(&mut self, channel_id: u64, message_id: u64) {
        let Some(cs) = self.chans.get_mut(&channel_id) else { return };
        let before = cs.msgs.len();
        cs.msgs.retain(|m| m.id != message_id || !m.meta.app.as_ref().is_some_and(|a| a.ephemeral()));
        if cs.msgs.len() != before {
            cs.stamped.remove(&message_id);
            cs.rev.bump();
        }
    }

    pub fn dismiss_interaction(&mut self, nonce: u64) {
        self.interactions.dismiss(nonce);
    }

    /// Délais des interactions : renvoie la prochaine échéance.
    pub fn expire_interactions(&mut self) -> Option<i64> {
        self.interactions.expire(now_ms())
    }

    pub(super) fn apply_interactions(&mut self, e: InteractionEvent) -> Option<Effect> {
        if let InteractionEvent::MessageApp { channel_id, id, app, edited } = e {
            let cs = self.chans.get_mut(&channel_id)?;
            let mut changed = false;
            for slot in cs.msgs.iter_mut().filter(|m| m.id == id) {
                // Réponse différée enfin donnée : ce n'est pas une modification.
                let was_loading = slot.meta.app.as_ref().is_some_and(|a| a.loading());
                if was_loading && !edited {
                    slot.edited = false;
                }
                if let Some(a) = app.as_deref().filter(|a| a.v2()) {
                    let mut shown = Vec::new();
                    Component::media_ids(&a.components, &mut shown);
                    slot.attachments.retain(|x| !shown.contains(&x.id));
                }
                slot.meta.app = app.clone();
                changed = true;
            }
            if changed {
                cs.rev.bump();
            }
            return None;
        }
        self.interactions.apply(e, now_ms());
        None
    }
}
