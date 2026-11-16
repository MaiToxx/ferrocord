//! Réactions, réponses, transferts, épingles, modification et suppression
//! côté magasin (lots T5/T6). Module enfant de `store` : il touche au cache
//! des salons comme le reste du magasin, sans élargir ce qui en est public.

use super::{ChanState, Effect, Store, MAX_UNSENT_PER_CHANNEL};
use crate::state::event::Cmd;
use crate::state::model::{ChannelKind, Message};
use crate::state::notice::{Notice, Problem};
use crate::state::perms::{Permissions, Verdict};
use crate::state::social::{Emoji, GuildEmoji, Pin, Reaction, Reactor, ReplyState, SendKind, SocialCmd, SocialEvent};
use std::collections::{HashMap, HashSet, VecDeque};

/// Listes de qui a réagi gardées en mémoire, au plus (les plus anciennes partent).
pub const MAX_REACTOR_LISTS: usize = 64;
/// Salons dont la liste d'épingles reste en mémoire, au plus.
pub const MAX_PIN_LISTS: usize = 8;
/// Envois particuliers (réponse, transfert) retenus pour un nouvel essai, au plus.
const MAX_SEND_KINDS: usize = 4 * MAX_UNSENT_PER_CHANNEL;
/// Longueur de l'extrait d'une réponse (caractères).
pub const EXCERPT_CHARS: usize = 120;
/// Source gardée pour l'extrait mis en forme (caractères, balises comprises).
pub const EXCERPT_SOURCE_CHARS: usize = 4 * EXCERPT_CHARS;
/// Longueur maximale d'un message (documentation de Create Message).
pub const MAX_CONTENT: usize = 2000;

/// Liste de qui a réagi.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reactors {
    Loading,
    Loaded(Vec<Reactor>),
    Failed(Problem),
}

/// Épingles d'un salon (chargées à la demande, page par page).
#[derive(Clone, Debug, Default)]
pub struct PinList {
    pub items: Vec<Pin>,
    pub loading: bool,
    pub more: bool,
    pub error: Option<Problem>,
    /// Une page a été reçue au moins.
    pub loaded: bool,
}

/// Ce que l'utilisateur peut faire sur les messages d'un salon (calculé par
/// image à partir du cache des permissions : aucune recherche par message).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rights {
    pub me: u64,
    pub dm: bool,
    pub send: bool,
    /// Ajouter une nouvelle réaction (un emoji encore absent du message).
    pub react: bool,
    /// Réagir avec un emoji déjà présent (l'historique suffit).
    pub react_existing: bool,
    /// Supprimer les messages des autres.
    pub manage: bool,
    pub pin: bool,
    /// Créer un fil depuis un message (lot T10).
    pub thread: bool,
    /// MP du compte système de Discord : lecture seule, ni barre de survol ni transfert.
    pub official: bool,
}

#[derive(Default)]
pub struct Social {
    /// Réactions envoyées sans réponse : un nouveau clic sur la même attend.
    inflight: HashSet<(u64, Emoji)>,
    reactors: HashMap<(u64, Emoji, bool), Reactors>,
    /// Ordre d'arrivée des listes de `reactors` (éviction des plus anciennes).
    reactors_order: VecDeque<(u64, Emoji, bool)>,
    pins: HashMap<u64, PinList>,
    pins_order: VecDeque<u64>,
    emojis: HashMap<u64, Vec<GuildEmoji>>,
    /// Envois particuliers par nonce : un nouvel essai garde la référence.
    sends: HashMap<u64, SendKind>,
    /// Suppressions demandées, pas encore confirmées.
    deleting: HashSet<u64>,
    /// Modification refusée : (salon, message, texte) rendu à l'interface.
    edit_failed: Option<(u64, u64, String)>,
    /// Réactions reçues sur mes messages, pas encore notifiées (F11).
    fresh_reactions: Vec<ReactionNotice>,
    /// Messages `/tts` reçus dans le salon affiché, pas encore lus à voix haute (F27).
    pub(in crate::state) fresh_tts: Vec<String>,
}

/// Messages `/tts` en attente de lecture, au plus ; chacun coupé à ce nombre de caractères.
pub(in crate::state) const MAX_TTS: usize = 4;
pub(in crate::state) const MAX_TTS_CHARS: usize = 300;

/// Réaction de quelqu'un d'autre à l'un de mes messages (F11).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReactionNotice {
    pub guild_id: Option<u64>,
    pub channel_id: u64,
    pub message_id: u64,
    pub user_id: u64,
    pub emoji: Emoji,
    /// Nom porté par l'événement (serveur) ; absent en MP.
    pub name: Option<String>,
}

/// Réactions gardées en attente de notification, au plus.
const MAX_REACTION_NOTICES: usize = 8;

impl Store {
    /// Réactions à mes messages arrivées depuis le dernier appel (F11).
    pub fn take_reactions(&mut self) -> Vec<ReactionNotice> {
        std::mem::take(&mut self.social.fresh_reactions)
    }

    /// Messages `/tts` à lire à voix haute depuis le dernier appel (F27).
    pub fn take_tts(&mut self) -> Vec<String> {
        std::mem::take(&mut self.social.fresh_tts)
    }
}

impl Social {
    /// Nouvelle session (READY) : ce qui dépend du serveur est oublié ; les
    /// emojis suivent le READY.
    pub(super) fn reset(&mut self) {
        *self = Social::default();
    }
}

/// Texte d'un extrait : une seule ligne, coupé à `EXCERPT_CHARS`.
pub fn excerpt(text: &str) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(EXCERPT_CHARS) {
        Some((i, _)) => format!("{}…", &flat[..i]),
        None => flat,
    }
}

/// Applique une réaction à un message. `mine` : la mienne (idempotente :
/// l'écho de la passerelle d'une réaction déjà comptée ne compte pas deux fois).
fn react_on(m: &mut Message, emoji: &Emoji, burst: bool, mine: bool, add: bool) -> bool {
    let list = &mut m.meta.reactions;
    let pos = list.iter().position(|r| r.emoji.same(emoji));
    if !add && pos.is_none() {
        return false;
    }
    let i = match pos {
        Some(i) => i,
        None => {
            list.push(Reaction { emoji: emoji.clone(), normal: 0, burst: 0, me: false, me_burst: false });
            list.len() - 1
        }
    };
    let Some(r) = list.get_mut(i) else { return false };
    let (count, me) = if burst { (&mut r.burst, &mut r.me_burst) } else { (&mut r.normal, &mut r.me) };
    let changed = match (mine, add) {
        (true, true) if *me => false,
        (true, false) if !*me => false,
        (true, a) => {
            *me = a;
            *count = if a { count.saturating_add(1) } else { count.saturating_sub(1) };
            true
        }
        (false, true) => {
            *count = count.saturating_add(1);
            true
        }
        (false, false) => {
            let before = *count;
            *count = count.saturating_sub(1);
            before != *count
        }
    };
    if r.normal == 0 && r.burst == 0 {
        list.remove(i);
    }
    changed
}

impl ChanState {
    /// Message de la fenêtre ou de la file du retour au présent.
    fn message_mut(&mut self, id: u64) -> Option<(&mut Message, bool)> {
        match self.msgs.binary_search_by_key(&id, |m| m.id) {
            Ok(i) => self.msgs.get_mut(i).map(|m| (m, true)),
            Err(_) => self.live.iter_mut().find(|m| m.id == id).map(|m| (m, false)),
        }
    }

    /// Modifie un message en mémoire ; la version du cache change s'il est
    /// dans la fenêtre et que `f` dit l'avoir changé.
    fn change(&mut self, id: u64, f: impl FnOnce(&mut Message) -> bool) {
        if let Some((m, in_window)) = self.message_mut(id)
            && f(m)
            && in_window
        {
            self.rev.bump();
        }
    }
}

impl Store {
    pub(super) fn change_message(&mut self, channel_id: u64, id: u64, f: impl FnOnce(&mut Message) -> bool) {
        if let Some(cs) = self.chans.get_mut(&channel_id) {
            cs.change(id, f);
        }
    }

    /// Message en mémoire (fenêtre ou file du retour au présent).
    pub fn find_message(&self, channel_id: u64, id: u64) -> Option<&Message> {
        let cs = self.chans.get(&channel_id)?;
        match cs.msgs.binary_search_by_key(&id, |m| m.id) {
            Ok(i) => cs.msgs.get(i),
            Err(_) => cs.live.iter().find(|m| m.id == id),
        }
    }

    /// Mon dernier message modifiable du salon (flèche haut dans une saisie vide).
    pub fn last_own_message(&self, channel_id: u64) -> Option<&Message> {
        let me = self.me.as_ref()?.id;
        let cs = self.chans.get(&channel_id)?;
        if cs.newer {
            return None;
        }
        cs.msgs.iter().rev().take(super::HISTORY_PAGE).find(|m| m.author_id == me && m.system.is_none() && m.meta.forward.is_none())
    }

    /// Texte simple d'un message (balises mises en mots, sans markdown),
    /// pour les lignes qui ne passent pas par la mise en forme.
    fn plain_text(&self, m: &Message) -> String {
        if m.system.is_some() && m.content.is_empty() {
            return self.render_content(m);
        }
        crate::state::markdown::parse_message(self, m, self.lang, !self.clock12).plain()
    }

    /// Rend l'extrait de la réponse en texte simple, une fois, à la réception
    /// (pour les usages sans mise en forme). Sa source reste, bornée, avec le
    /// message transféré : l'interface les met en forme avec le message
    /// (`markdown::parse_message`, une fois).
    pub(super) fn render_meta(&self, m: &mut Message) {
        let Some(raw) = m.meta.reply_raw.as_mut() else { return };
        // Assez de source pour une ligne d'extrait, balises comprises.
        if let Some((i, _)) = raw.0.char_indices().nth(EXCERPT_SOURCE_CHARS) {
            raw.0.truncate(i);
        }
        let tmp = Message {
            id: m.id,
            channel_id: m.channel_id,
            guild_id: m.guild_id,
            author_id: 0,
            author: String::new(),
            author_avatar: m.author_avatar,
            content: raw.0.clone(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            mentions: raw.1.clone(),
            mention_everyone: false,
            edited: false,
            system: None,
            meta: Default::default(),
        };
        let text = excerpt(&self.plain_text(&tmp));
        if let Some(r) = m.meta.reply.as_mut()
            && let ReplyState::Loaded { excerpt, .. } = &mut r.state
        {
            *excerpt = text;
        }
    }

    // ------------------------------------------------------------------ droits

    /// Droits de l'utilisateur dans ce salon. `Inconnu` vaut permis (Discord
    /// répondra) ; dans un MP, seuls mes propres messages se suppriment.
    pub fn rights(&self, channel_id: u64) -> Rights {
        let me = self.me.as_ref().map_or(0, |u| u.id);
        let dm = !self.chan_guild.contains_key(&channel_id);
        let ok = |p| self.can(channel_id, p) != Verdict::Refuse;
        let history = ok(Permissions::READ_MESSAGE_HISTORY);
        // MP du compte système de Discord : lecture seule, comme dans le client.
        if dm && self.is_official_dm(channel_id) {
            return Rights { me, dm, official: true, ..Rights::default() };
        }
        Rights {
            me,
            dm,
            send: self.send_block(channel_id).is_none(),
            react: history && ok(Permissions::ADD_REACTIONS),
            react_existing: history,
            manage: !dm && ok(Permissions::MANAGE_MESSAGES),
            pin: dm || ok(Permissions::PIN_MESSAGES),
            thread: self.can_create_thread(channel_id),
            official: false,
        }
    }

    // ------------------------------------------------------------------ réactions

    /// Ajoute ma réaction si je ne l'ai pas, la retire sinon (mise à jour
    /// immédiate, défaite si Discord refuse). `false` si rien ne part.
    pub fn toggle_reaction(&mut self, channel_id: u64, message_id: u64, emoji: Emoji) -> bool {
        let Some(m) = self.find_message(channel_id, message_id) else { return false };
        let existing = m.meta.reactions.iter().find(|r| r.emoji.same(&emoji));
        let add = !existing.is_some_and(|r| r.me);
        let rights = self.rights(channel_id);
        let allowed = if existing.is_some() { rights.react_existing } else { rights.react };
        if !allowed || !self.social.inflight.insert((message_id, emoji.clone())) {
            return false;
        }
        self.change_message(channel_id, message_id, |m| react_on(m, &emoji, false, true, add));
        self.outbox.push(Cmd::Social(SocialCmd::React { channel_id, message_id, emoji, add }));
        true
    }

    /// Ajoute ma réaction (sélecteur) ; rien si je l'ai déjà.
    pub fn add_reaction(&mut self, channel_id: u64, message_id: u64, emoji: Emoji) -> bool {
        let mine = self.find_message(channel_id, message_id).is_some_and(|m| m.meta.reactions.iter().any(|r| r.emoji.same(&emoji) && r.me));
        !mine && self.toggle_reaction(channel_id, message_id, emoji)
    }

    pub fn reaction_pending(&self, message_id: u64, emoji: &Emoji) -> bool {
        self.social.inflight.iter().any(|(m, e)| *m == message_id && e.same(emoji))
    }

    /// Liste de qui a réagi, si elle a été demandée.
    pub fn reactors(&self, message_id: u64, emoji: &Emoji, burst: bool) -> Option<&Reactors> {
        self.social.reactors.get(&(message_id, emoji.clone(), burst))
    }

    /// Demande la liste de qui a réagi, une seule fois (gardée ensuite).
    pub fn request_reactors(&mut self, channel_id: u64, message_id: u64, emoji: &Emoji, burst: bool) -> bool {
        let key = (message_id, emoji.clone(), burst);
        if self.social.reactors.contains_key(&key) || self.can(channel_id, Permissions::READ_MESSAGE_HISTORY) == Verdict::Refuse {
            return false;
        }
        self.store_reactors(key, Reactors::Loading);
        self.outbox.push(Cmd::Social(SocialCmd::Reactors { channel_id, message_id, emoji: emoji.clone(), burst }));
        true
    }

    fn store_reactors(&mut self, key: (u64, Emoji, bool), v: Reactors) {
        let s = &mut self.social;
        if s.reactors.insert(key.clone(), v).is_none() {
            s.reactors_order.push_back(key);
            while s.reactors_order.len() > super::social::MAX_REACTOR_LISTS {
                if let Some(old) = s.reactors_order.pop_front() {
                    s.reactors.remove(&old);
                }
            }
        }
    }

    /// Les listes de ce message ne valent plus (réaction ajoutée ou retirée).
    fn forget_reactors(&mut self, message_id: u64) {
        let s = &mut self.social;
        s.reactors.retain(|(m, _, _), v| *m != message_id || *v == Reactors::Loading);
        let kept = &s.reactors;
        s.reactors_order.retain(|k| kept.contains_key(k));
    }

    // ------------------------------------------------------------------ messages

    /// Modifie le texte d'un de mes messages. `false` si rien ne part (pas le
    /// mien, vide, trop long ou inchangé).
    pub fn edit_message(&mut self, channel_id: u64, message_id: u64, content: String) -> bool {
        let me = self.me.as_ref().map(|u| u.id);
        let Some(m) = self.find_message(channel_id, message_id) else { return false };
        let content = content.trim().to_string();
        if Some(m.author_id) != me || m.system.is_some() || content.is_empty() || content.chars().count() > MAX_CONTENT {
            return false;
        }
        let raw = self.chans.get(&channel_id).and_then(|cs| cs.stamped.get(&message_id)).unwrap_or(&m.content);
        if *raw == content {
            return false;
        }
        self.outbox.push(Cmd::Social(SocialCmd::Edit { channel_id, message_id, content }));
        true
    }

    /// Texte d'origine d'un message (avant rendu des balises), pour l'éditer.
    pub fn raw_content(&self, channel_id: u64, message_id: u64) -> Option<String> {
        let cs = self.chans.get(&channel_id)?;
        cs.stamped.get(&message_id).cloned().or_else(|| self.find_message(channel_id, message_id).map(|m| m.content.clone()))
    }

    /// Modification refusée : le texte revient à l'interface (une fois).
    pub fn take_edit_failure(&mut self) -> Option<(u64, u64, String)> {
        self.social.edit_failed.take()
    }

    /// Supprime un message (le mien, ou celui d'un autre avec « gérer les messages »).
    pub fn delete_message(&mut self, channel_id: u64, message_id: u64) -> bool {
        let rights = self.rights(channel_id);
        let Some(m) = self.find_message(channel_id, message_id) else { return false };
        if !(m.author_id == rights.me || rights.manage) || !self.social.deleting.insert(message_id) {
            return false;
        }
        self.outbox.push(Cmd::Social(SocialCmd::Delete { channel_id, message_id }));
        true
    }

    pub fn is_deleting(&self, message_id: u64) -> bool {
        self.social.deleting.contains(&message_id)
    }

    pub fn set_pinned(&mut self, channel_id: u64, message_id: u64, pin: bool) -> bool {
        if !self.rights(channel_id).pin || self.find_message(channel_id, message_id).is_none_or(|m| m.system.is_some()) {
            return false;
        }
        self.outbox.push(Cmd::Social(SocialCmd::Pin { channel_id, message_id, pin }));
        true
    }

    /// « Supprimer les intégrations » est-il proposé pour ce message ?
    pub fn can_suppress_embeds(&self, m: &Message) -> bool {
        let r = self.rights(m.channel_id);
        !m.embeds.is_empty() && m.system.is_none() && m.meta.flags & super::super::social::SUPPRESS_EMBEDS == 0 && (m.author_id == r.me || r.manage)
    }

    /// Retire les intégrations d'un message (le mien, ou avec « gérer les messages »).
    pub fn suppress_embeds(&mut self, channel_id: u64, message_id: u64) -> bool {
        let Some(m) = self.find_message(channel_id, message_id) else { return false };
        if !self.can_suppress_embeds(m) {
            return false;
        }
        let flags = m.meta.flags | super::super::social::SUPPRESS_EMBEDS;
        self.outbox.push(Cmd::Social(SocialCmd::SuppressEmbeds { channel_id, message_id, flags }));
        true
    }

    /// Envoie une réponse ; `mention` : mentionner l'auteur du message.
    pub fn send_reply(&mut self, channel_id: u64, content: String, message_id: u64, mention: bool) -> bool {
        self.send_kind(channel_id, content, SendKind::Reply { message_id, mention })
    }

    /// Transfère le message `message_id` du salon `from` dans `channel_id`.
    /// `label` : texte montré si l'envoi échoue (un transfert n'a pas de texte).
    pub fn forward(&mut self, channel_id: u64, from: u64, message_id: u64, label: String) -> bool {
        if !self.channel_exists(channel_id) {
            return false;
        }
        let guild_id = self.chan_guild.get(&from).copied();
        self.send_kind(channel_id, label, SendKind::Forward { channel_id: from, message_id, guild_id })
    }

    /// Envoie l'autocollant `sticker_id` ; `label` : texte montré si l'envoi échoue.
    pub fn send_sticker(&mut self, channel_id: u64, sticker_id: u64, label: String) -> bool {
        self.send_kind(channel_id, label, SendKind::Sticker { sticker_id })
    }

    fn send_kind(&mut self, channel_id: u64, content: String, kind: SendKind) -> bool {
        if self.send_block(channel_id).is_some() {
            return false;
        }
        let nonce = self.next_nonce();
        let s = &mut self.social;
        if s.sends.len() >= MAX_SEND_KINDS
            && let Some(oldest) = s.sends.keys().min().copied()
        {
            s.sends.remove(&oldest);
        }
        s.sends.insert(nonce, kind.clone());
        self.outbox.push(Cmd::Social(SocialCmd::Send { channel_id, content, nonce, kind }));
        self.return_to_present(channel_id);
        true
    }

    /// Commande d'un nouvel essai : la même réponse ou le même transfert.
    pub(super) fn resend(&self, channel_id: u64, content: String, nonce: u64) -> Cmd {
        match self.social.sends.get(&nonce) {
            Some(kind) => Cmd::Social(SocialCmd::Send { channel_id, content, nonce, kind: kind.clone() }),
            None => Cmd::Send { channel_id, content, nonce },
        }
    }

    // ------------------------------------------------------------------ épingles

    /// Liste des épingles du salon (chargée par `load_pins`).
    pub fn pins(&self, channel_id: u64) -> Option<&PinList> {
        self.social.pins.get(&channel_id)
    }

    /// Demande la première page (`more` = false, seulement si rien n'est en
    /// mémoire) ou la suivante.
    pub fn load_pins(&mut self, channel_id: u64, more: bool) -> bool {
        if self.can(channel_id, Permissions::READ_MESSAGE_HISTORY) == Verdict::Refuse {
            return false;
        }
        if !self.social.pins.contains_key(&channel_id) {
            self.social.pins_order.push_back(channel_id);
            while self.social.pins_order.len() > MAX_PIN_LISTS {
                if let Some(old) = self.social.pins_order.pop_front() {
                    self.social.pins.remove(&old);
                }
            }
        }
        let list = self.social.pins.entry(channel_id).or_default();
        if list.loading || (more && !list.more) || (!more && list.loaded && list.error.is_none()) {
            return false;
        }
        let before = if more { list.items.last().map(|p| p.pinned_at.clone()) } else { None };
        if !more {
            list.items.clear();
        }
        list.loading = true;
        list.error = None;
        self.outbox.push(Cmd::Social(SocialCmd::Pins { channel_id, before }));
        true
    }

    // ------------------------------------------------------------------ emojis

    pub fn guild_emojis(&self, guild_id: u64) -> &[GuildEmoji] {
        self.social.emojis.get(&guild_id).map_or(&[], Vec::as_slice)
    }

    /// Serveur d'un salon (aucun pour un MP).
    pub fn guild_of(&self, channel_id: u64) -> Option<u64> {
        self.chan_guild.get(&channel_id).copied()
    }

    /// Salons où un transfert peut aller : salons textuels visibles où j'écris, et MP.
    pub fn forward_targets(&self) -> Vec<(u64, Option<u64>, String)> {
        let mut out = Vec::new();
        for g in &self.guilds {
            for c in &g.channels {
                if c.kind != ChannelKind::Category && !self.is_hidden(c.id) && self.send_block(c.id).is_none() {
                    out.push((c.id, Some(g.id), c.name.clone()));
                }
            }
        }
        out.extend(self.dms.iter().map(|d| (d.id, None, d.name.clone())));
        out
    }

    // ------------------------------------------------------------------ événements

    pub(super) fn apply_social(&mut self, ev: SocialEvent) -> Option<Effect> {
        let me = self.me.as_ref().map_or(0, |u| u.id);
        match ev {
            SocialEvent::Reaction { channel_id, message_id, user_id, emoji, burst, add } => {
                self.change_message(channel_id, message_id, |m| react_on(m, &emoji, burst, user_id == me, add));
                self.forget_reactors(message_id);
            }
            SocialEvent::ReactedTo { guild_id, channel_id, message_id, user_id, author_id, emoji, name } => {
                let blocked = self.relations.relation(user_id).is_some_and(|r| r.kind == super::super::relations::RelKind::Blocked);
                if author_id == me && user_id != me && !blocked && self.social.fresh_reactions.len() < MAX_REACTION_NOTICES {
                    self.social.fresh_reactions.push(ReactionNotice { guild_id, channel_id, message_id, user_id, emoji, name });
                }
            }
            SocialEvent::ReactionRemoveAll { channel_id, message_id } => {
                self.change_message(channel_id, message_id, |m| !std::mem::take(&mut m.meta.reactions).is_empty());
                self.forget_reactors(message_id);
            }
            SocialEvent::ReactionRemoveEmoji { channel_id, message_id, emoji } => {
                self.change_message(channel_id, message_id, |m| {
                    let before = m.meta.reactions.len();
                    m.meta.reactions.retain(|r| !r.emoji.same(&emoji));
                    before != m.meta.reactions.len()
                });
                self.forget_reactors(message_id);
            }
            SocialEvent::ReactDone { channel_id, message_id, emoji, add, error } => {
                self.social.inflight.retain(|(m, e)| !(*m == message_id && e.same(&emoji)));
                if let Some(error) = error {
                    // Refusé : la mise à jour immédiate est défaite.
                    self.change_message(channel_id, message_id, |m| react_on(m, &emoji, false, true, !add));
                    return Some(Effect::Toast(Notice::Error(error)));
                }
                self.forget_reactors(message_id);
            }
            SocialEvent::Reactors { message_id, emoji, burst, users } => {
                let key = (message_id, emoji, burst);
                // Liste oubliée entre-temps (réaction ajoutée) : la réponse est périmée.
                if self.social.reactors.contains_key(&key) {
                    self.store_reactors(key, users.map_or_else(Reactors::Failed, Reactors::Loaded));
                }
            }
            SocialEvent::EditFailed { channel_id, message_id, content, error } => {
                self.social.edit_failed = Some((channel_id, message_id, content));
                return Some(Effect::Toast(Notice::Error(error)));
            }
            SocialEvent::DeleteFailed { message_id, error, .. } => {
                self.social.deleting.remove(&message_id);
                return Some(Effect::Toast(Notice::Error(error)));
            }
            SocialEvent::Pinned { channel_id, message_id, pinned } => {
                self.change_message(channel_id, message_id, |m| std::mem::replace(&mut m.meta.pinned, pinned) != pinned);
                if let Some(list) = self.social.pins.get_mut(&channel_id)
                    && !pinned
                {
                    list.items.retain(|p| p.message.id != message_id);
                }
            }
            SocialEvent::PinFailed { error, .. } => return Some(Effect::Toast(Notice::Error(error))),
            SocialEvent::EmbedsSuppressed { channel_id, message_id } => {
                self.change_message(channel_id, message_id, |m| {
                    m.meta.flags |= super::super::social::SUPPRESS_EMBEDS;
                    !std::mem::take(&mut m.embeds).is_empty()
                });
            }
            SocialEvent::Pins { channel_id, before, pins } => {
                let rendered = pins.map(|(page, more)| {
                    let page: Vec<Pin> = page
                        .into_iter()
                        .map(|mut p| {
                            p.message.content = self.plain_text(&p.message);
                            self.render_meta(&mut p.message);
                            p
                        })
                        .collect();
                    (page, more)
                });
                let list = self.social.pins.get_mut(&channel_id)?;
                // Réponse à une demande qui n'est plus celle en cours : ignorée.
                if !list.loading || before.as_deref() != (if before.is_some() { list.items.last().map(|p| p.pinned_at.as_str()) } else { None }) {
                    return None;
                }
                list.loading = false;
                list.loaded = true;
                match rendered {
                    Ok((page, more)) => {
                        list.items.extend(page);
                        list.more = more;
                    }
                    Err(e) => list.error = Some(e),
                }
            }
            SocialEvent::PinsChanged { channel_id } => {
                // Relue à la prochaine ouverture du panneau (ou tout de suite s'il est ouvert).
                if let Some(list) = self.social.pins.get_mut(&channel_id)
                    && !list.loading
                {
                    list.loaded = false;
                    list.more = false;
                }
            }
            SocialEvent::GuildEmojis { guild_id, emojis } => {
                self.social.emojis.insert(guild_id, emojis);
            }
        }
        None
    }

    /// Message supprimé : il n'est plus en cours de suppression.
    pub(super) fn social_deleted(&mut self, message_id: u64) {
        self.social.deleting.remove(&message_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::event::Event;
    use crate::state::model::{Avatar, Channel, Guild, Member, Role, User};
    use crate::state::social::{Forward, ReplyRef};

    const G: u64 = 9;
    const C: u64 = 100;
    const ME: u64 = 1;

    fn msg(id: u64, author_id: u64, content: &str) -> Message {
        Message {
            id,
            channel_id: C,
            guild_id: Some(G),
            author_id,
            author: format!("u{author_id}"),
            author_avatar: Avatar::default_for(author_id),
            content: content.into(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            mentions: Vec::new(),
            mention_everyone: false,
            edited: false,
            system: None,
            meta: Default::default(),
        }
    }

    /// Serveur où @everyone a `perms` dans #100.
    fn store(perms: Permissions) -> Store {
        let mut s = Store::new();
        let mut g = Guild::new(G, "G".into(), vec![Channel { id: C, name: "c".into(), kind: ChannelKind::Text, position: 0, parent: None, overwrites: Some(Vec::new()), slowmode: 0 }]);
        g.roles = vec![Role { id: G, name: "@everyone".into(), color: 0, position: 0, permissions: perms, mentionable: false }];
        g.me = Some(Member::default());
        g.owner_id = Some(999);
        s.apply(Event::Ready { me: User::new(ME, "Moi".into()), guilds: vec![g], dms: Vec::new() });
        s.select_channel(C);
        s.take_commands();
        s.apply(Event::History { channel_id: C, before: None, messages: vec![msg(10, 2, "a"), msg(11, ME, "b <@2>"), msg(12, 2, "c")] });
        s
    }

    fn all() -> Permissions {
        Permissions::VIEW_CHANNEL
            | Permissions::SEND_MESSAGES
            | Permissions::READ_MESSAGE_HISTORY
            | Permissions::ADD_REACTIONS
            | Permissions::MANAGE_MESSAGES
            | Permissions::PIN_MESSAGES
    }

    fn reactions(s: &Store, id: u64) -> Vec<(String, u32, u32, bool)> {
        s.find_message(C, id).map(|m| m.meta.reactions.iter().map(|r| (r.emoji.display(), r.normal, r.burst, r.me)).collect()).unwrap_or_default()
    }

    fn thumbs() -> Emoji {
        Emoji::Unicode("👍".into())
    }

    #[test]
    fn optimistic_reaction_is_kept_on_echo_and_undone_on_refusal() {
        let mut s = store(all());
        let rev = s.chans[&C].rev;
        assert!(s.toggle_reaction(C, 10, thumbs()));
        assert_ne!(s.chans[&C].rev, rev, "hauteur à refaire");
        assert_eq!(reactions(&s, 10), vec![("👍".into(), 1, 0, true)]);
        assert_eq!(s.take_commands(), vec![Cmd::Social(SocialCmd::React { channel_id: C, message_id: 10, emoji: thumbs(), add: true })]);
        // Second clic pendant l'envoi : rien ne part.
        assert!(!s.toggle_reaction(C, 10, thumbs()));
        assert!(s.reaction_pending(10, &thumbs()));
        // Écho de la passerelle : pas compté deux fois.
        s.apply(Event::Social(SocialEvent::Reaction { channel_id: C, message_id: 10, user_id: ME, emoji: thumbs(), burst: false, add: true }));
        s.apply(Event::Social(SocialEvent::ReactDone { channel_id: C, message_id: 10, emoji: thumbs(), add: true, error: None }));
        assert_eq!(reactions(&s, 10), vec![("👍".into(), 1, 0, true)]);
        // Quelqu'un d'autre réagit, puis je retire la mienne, refusée : elle revient.
        s.apply(Event::Social(SocialEvent::Reaction { channel_id: C, message_id: 10, user_id: 5, emoji: thumbs(), burst: false, add: true }));
        assert!(s.toggle_reaction(C, 10, thumbs()));
        assert_eq!(reactions(&s, 10), vec![("👍".into(), 1, 0, false)]);
        let eff = s.apply(Event::Social(SocialEvent::ReactDone { channel_id: C, message_id: 10, emoji: thumbs(), add: false, error: Some(Problem::Forbidden) }));
        assert_eq!(eff, Some(Effect::Toast(Notice::Error(Problem::Forbidden))));
        assert_eq!(reactions(&s, 10), vec![("👍".into(), 2, 0, true)]);
        // Super-réaction d'un autre, puis tout retiré.
        s.apply(Event::Social(SocialEvent::Reaction { channel_id: C, message_id: 10, user_id: 5, emoji: thumbs(), burst: true, add: true }));
        assert_eq!(reactions(&s, 10), vec![("👍".into(), 2, 1, true)]);
        s.apply(Event::Social(SocialEvent::ReactionRemoveEmoji { channel_id: C, message_id: 10, emoji: thumbs() }));
        assert!(reactions(&s, 10).is_empty());
        s.apply(Event::Social(SocialEvent::Reaction { channel_id: C, message_id: 10, user_id: 5, emoji: thumbs(), burst: false, add: true }));
        s.apply(Event::Social(SocialEvent::ReactionRemoveAll { channel_id: C, message_id: 10 }));
        assert!(reactions(&s, 10).is_empty());
        // Retrait d'une réaction inconnue : rien.
        s.apply(Event::Social(SocialEvent::Reaction { channel_id: C, message_id: 10, user_id: 5, emoji: thumbs(), burst: false, add: false }));
        assert!(reactions(&s, 10).is_empty());
    }

    #[test]
    fn a_reaction_to_my_message_is_kept_for_a_notification() {
        let mut s = store(all());
        let ev = |user_id, author_id| Event::Social(SocialEvent::ReactedTo { guild_id: None, channel_id: C, message_id: 10, user_id, author_id, emoji: thumbs(), name: Some("eve".into()) });
        s.apply(ev(5, 77));
        s.apply(ev(ME, ME));
        assert!(s.take_reactions().is_empty(), "message d'un autre, ou ma propre réaction");
        s.apply(ev(5, ME));
        let got = s.take_reactions();
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].user_id, got[0].name.as_deref()), (5, Some("eve")));
        assert!(s.take_reactions().is_empty(), "notifiée une seule fois");
    }

    #[test]
    fn new_reaction_needs_add_reactions_existing_one_does_not() {
        let mut s = store(Permissions::VIEW_CHANNEL | Permissions::READ_MESSAGE_HISTORY | Permissions::SEND_MESSAGES);
        assert!(!s.toggle_reaction(C, 10, thumbs()));
        s.apply(Event::Social(SocialEvent::Reaction { channel_id: C, message_id: 10, user_id: 5, emoji: thumbs(), burst: false, add: true }));
        assert!(s.toggle_reaction(C, 10, thumbs()));
        let r = s.rights(C);
        assert!(!r.react && r.react_existing && !r.manage && !r.pin && r.send);
    }

    #[test]
    fn reactors_are_fetched_once_and_bounded() {
        let mut s = store(all());
        assert!(s.request_reactors(C, 10, &thumbs(), false));
        assert!(!s.request_reactors(C, 10, &thumbs(), false));
        assert_eq!(s.take_commands().len(), 1);
        assert_eq!(s.reactors(10, &thumbs(), false), Some(&Reactors::Loading));
        let users = vec![Reactor { id: 5, name: "eve".into() }];
        s.apply(Event::Social(SocialEvent::Reactors { message_id: 10, emoji: thumbs(), burst: false, users: Ok(users.clone()) }));
        assert_eq!(s.reactors(10, &thumbs(), false), Some(&Reactors::Loaded(users)));
        // Une réaction de plus : la liste est redemandée au prochain survol.
        s.apply(Event::Social(SocialEvent::Reaction { channel_id: C, message_id: 10, user_id: 6, emoji: thumbs(), burst: false, add: true }));
        assert!(s.reactors(10, &thumbs(), false).is_none());
        for i in 0..(MAX_REACTOR_LISTS as u64 + 10) {
            s.request_reactors(C, 1000 + i, &thumbs(), false);
        }
        assert!(s.social.reactors.len() <= MAX_REACTOR_LISTS && s.social.reactors_order.len() == s.social.reactors.len());
    }

    #[test]
    fn edit_delete_pin_rules() {
        let mut s = store(Permissions::VIEW_CHANNEL | Permissions::READ_MESSAGE_HISTORY | Permissions::SEND_MESSAGES);
        // Pas le mien, vide, inchangé : rien.
        assert!(!s.edit_message(C, 10, "x".into()));
        assert!(!s.edit_message(C, 11, "  ".into()));
        assert!(!s.edit_message(C, 11, "b <@2>".into()), "texte d'origine inchangé");
        assert!(!s.edit_message(C, 11, "x".repeat(MAX_CONTENT + 1)));
        assert!(s.edit_message(C, 11, " nouveau ".into()));
        assert_eq!(s.take_commands(), vec![Cmd::Social(SocialCmd::Edit { channel_id: C, message_id: 11, content: "nouveau".into() })]);
        assert_eq!(s.raw_content(C, 11).as_deref(), Some("b <@2>"));
        // Supprimer : le mien seulement sans « gérer les messages », une fois.
        assert!(!s.delete_message(C, 10));
        assert!(s.delete_message(C, 11));
        assert!(!s.delete_message(C, 11) && s.is_deleting(11));
        s.apply(Event::Social(SocialEvent::DeleteFailed { channel_id: C, message_id: 11, error: Problem::Forbidden }));
        assert!(!s.is_deleting(11));
        assert!(!s.set_pinned(C, 10, true));
        assert_eq!(s.last_own_message(C).map(|m| m.id), Some(11));

        let mut s = store(all());
        assert!(s.delete_message(C, 10));
        assert!(s.set_pinned(C, 10, true));
        s.apply(Event::Social(SocialEvent::Pinned { channel_id: C, message_id: 10, pinned: true }));
        assert!(s.find_message(C, 10).is_some_and(|m| m.meta.pinned));
        s.apply(Event::MessageDelete { channel_id: C, id: 10 });
        assert!(!s.is_deleting(10));
        let eff = s.apply(Event::Social(SocialEvent::EditFailed { channel_id: C, message_id: 11, content: "texte".into(), error: Problem::Forbidden }));
        assert!(eff.is_some());
        assert_eq!(s.take_edit_failure(), Some((C, 11, "texte".into())));
        assert_eq!(s.take_edit_failure(), None);
    }

    #[test]
    fn reply_retry_keeps_the_reference() {
        let mut s = store(all());
        assert!(s.send_reply(C, "oui".into(), 10, false));
        let nonce = match s.take_commands().as_slice() {
            [Cmd::Social(SocialCmd::Send { nonce, kind: SendKind::Reply { message_id: 10, mention: false }, .. })] => *nonce,
            other => panic!("{other:?}"),
        };
        s.apply(Event::SendError { channel_id: C, nonce, content: "oui".into(), error: Problem::Forbidden, forbidden: false });
        assert!(s.retry_unsent(C, nonce));
        assert_eq!(
            s.take_commands(),
            vec![Cmd::Social(SocialCmd::Send { channel_id: C, content: "oui".into(), nonce, kind: SendKind::Reply { message_id: 10, mention: false } })]
        );
        assert!(s.forward(C, C, 12, "c".into()));
        assert!(matches!(s.take_commands().as_slice(), [Cmd::Social(SocialCmd::Send { kind: SendKind::Forward { channel_id: C, message_id: 12, guild_id: Some(G) }, .. })]));
        assert!(!s.forward(4242, C, 12, "c".into()), "salon inconnu");
    }

    #[test]
    fn reply_excerpt_and_forward_are_rendered_once() {
        let mut s = store(all());
        let mut m = msg(20, 2, "réponse");
        m.meta.reply = Some(Box::new(ReplyRef { message_id: Some(11), state: ReplyState::Loaded { author_id: ME, author: "Moi".into(), excerpt: String::new() } }));
        m.meta.reply_raw = Some(Box::new((format!("salut <@3>\n{}", "long ".repeat(50)), vec![(3, "eve".into())])));
        m.meta.forward = Some(Box::new(Forward { content: "vu <@3>".into(), mentions: vec![(3, "eve".into())], ..Default::default() }));
        s.apply(Event::MessageCreate(m));
        let m = s.find_message(C, 20).unwrap_or_else(|| panic!("message"));
        let Some(ReplyState::Loaded { excerpt, .. }) = m.meta.reply.as_ref().map(|r| &r.state) else { panic!("réponse") };
        assert!(excerpt.starts_with("salut @eve long") && excerpt.ends_with('…') && excerpt.chars().count() == EXCERPT_CHARS + 1, "{excerpt}");
        // Sources gardées (bornées) pour la mise en forme avec le message.
        assert!(m.meta.reply_raw.as_ref().is_some_and(|r| r.0.starts_with("salut <@3>") && r.0.chars().count() <= EXCERPT_SOURCE_CHARS));
        let f = m.meta.forward.as_ref().unwrap_or_else(|| panic!("transfert"));
        assert_eq!((f.content.as_str(), f.mentions.len()), ("vu <@3>", 1));
        let doc = crate::state::markdown::parse_message(&s, m, crate::i18n::Lang::Fr, true);
        assert_eq!(doc.forward.as_ref().map(|d| d.plain()).as_deref(), Some("vu @eve"));
        assert!(doc.reply.as_ref().is_some_and(|d| d.plain().starts_with("salut @eve")));
        // Embeds : mis en forme avec le message, spoilers à la suite des siens.
        let mut e = msg(21, 2, "||m||");
        e.embeds = vec![crate::state::media::Embed {
            description: Some("**gras** ||d||".into()),
            fields: vec![crate::state::media::EmbedField { name: "n".into(), value: "||v||".into(), inline: false }],
            ..Default::default()
        }];
        let doc = crate::state::markdown::parse_message(&s, &e, crate::i18n::Lang::Fr, true);
        let spoiler = |d: &crate::state::markdown::Doc| d.blocks.iter().flat_map(|b| b.spans.iter()).find_map(|s| s.style.spoiler);
        assert_eq!(doc.embeds.iter().map(|(i, slot, d)| (*i, *slot, spoiler(d))).collect::<Vec<_>>(), vec![(0, 0, Some(1)), (0, 1, Some(2))]);
        assert!(doc.embeds[0].2.blocks[0].spans[0].style.bold);
    }

    #[test]
    fn pins_pages_and_staleness() {
        let mut s = store(all());
        assert!(s.load_pins(C, false));
        assert!(!s.load_pins(C, false), "déjà en cours");
        assert_eq!(s.take_commands(), vec![Cmd::Social(SocialCmd::Pins { channel_id: C, before: None })]);
        let pin = |id: u64, at: &str| Pin { pinned_at: at.into(), message: msg(id, 2, "<@3>") };
        s.apply(Event::Social(SocialEvent::Pins { channel_id: C, before: None, pins: Ok((vec![pin(30, "t2"), pin(31, "t1")], true)) }));
        let list = s.pins(C).unwrap_or_else(|| panic!("liste"));
        assert_eq!((list.items.len(), list.more, list.loading), (2, true, false));
        assert_eq!(list.items[0].message.content, "@utilisateur", "rendu à la réception");
        assert!(!s.load_pins(C, false), "déjà chargée");
        assert!(s.load_pins(C, true));
        assert_eq!(s.take_commands(), vec![Cmd::Social(SocialCmd::Pins { channel_id: C, before: Some("t1".into()) })]);
        // Réponse périmée (autre repère) : ignorée.
        s.apply(Event::Social(SocialEvent::Pins { channel_id: C, before: Some("t9".into()), pins: Ok((vec![pin(40, "t0")], false)) }));
        assert_eq!(s.pins(C).map(|l| l.items.len()), Some(2));
        s.apply(Event::Social(SocialEvent::Pins { channel_id: C, before: Some("t1".into()), pins: Err(Problem::Forbidden) }));
        assert_eq!(s.pins(C).and_then(|l| l.error.clone()), Some(Problem::Forbidden));
        // Désépinglé : retiré ; CHANNEL_PINS_UPDATE : relue à la demande.
        s.apply(Event::Social(SocialEvent::Pinned { channel_id: C, message_id: 30, pinned: false }));
        assert_eq!(s.pins(C).map(|l| l.items.len()), Some(1));
        s.apply(Event::Social(SocialEvent::PinsChanged { channel_id: C }));
        assert!(s.load_pins(C, false));
        for c in 0..(MAX_PIN_LISTS as u64 + 3) {
            s.load_pins(5000 + c, false);
        }
        assert!(s.social.pins.len() <= MAX_PIN_LISTS);
    }

    #[test]
    fn emojis_and_ready_reset() {
        let mut s = store(all());
        s.apply(Event::Social(SocialEvent::GuildEmojis { guild_id: G, emojis: vec![GuildEmoji { id: 1, name: "chat".into(), animated: false, available: true }] }));
        assert_eq!(s.guild_emojis(G).len(), 1);
        assert_eq!(s.guild_of(C), Some(G));
        s.apply(Event::Ready { me: User::new(ME, "Moi".into()), guilds: Vec::new(), dms: Vec::new() });
        assert!(s.guild_emojis(G).is_empty());
    }

    #[test]
    fn excerpt_is_one_line() {
        assert_eq!(excerpt("a\n\nb   c"), "a b c");
        assert_eq!(excerpt(""), "");
    }
}
