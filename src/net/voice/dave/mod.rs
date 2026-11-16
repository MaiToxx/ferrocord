//! Chiffrement de bout en bout DAVE (protocole 1) : https://daveprotocol.com,
//! https://github.com/discord/dave-protocol (protocol.md). Implémente
//! [`FrameCrypto`] : ops 21 à 31 de la passerelle vocale, groupe MLS
//! (sous-ensemble maison sur aws-lc-rs, voir `mls`), chiffrement des trames
//! Opus et codes de vérification.
//!
//! La clé d'identité (signature ECDSA P-256) est créée pour la session et
//! ne quitte jamais la mémoire : DAVE l'autorise (clé « éphémère »).
//!
//! Ops que le transport doit transmettre à [`FrameCrypto::on_op`] : 21 à
//! 30, et aussi 4 (`dave_protocol_version` de select_protocol_ack), 11 et
//! 13 s'il ne passe pas par `on_users`. Le JSON peut être l'objet complet
//! `{op, d}` ou seulement `d`.

mod codec;
mod crypto;
mod frame;
mod mls;
mod verify;
mod video;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod robust;

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{Value, json};

use super::e2ee::{FrameCrypto, Outgoing};
use codec::Reader;
use crypto::{HpkeKey, SigKey};
use frame::{Decryptor, Encryptor, EpochKeys};
use mls::msg::{ExternalSender, KeyPackage, LeafNode, LeafSource, Proposal, default_capabilities};
use mls::{Group, Queued};

/// Version de DAVE prise en charge.
pub const PROTOCOL_VERSION: u16 = 1;

/// Erreur interne (jamais montrée telle quelle : sert aux journaux et aux tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error(pub &'static str);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// Commits que nous avons proposés pendant l'époque : la passerelle peut
/// retenir n'importe lequel.
const MAX_OUTBOUND: usize = 4;

/// Paquet de clés en cours et ses secrets.
struct Join {
    leaf_key: Arc<HpkeKey>,
    init_key: HpkeKey,
    kp: KeyPackage,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Passthrough {
    Forever,
    Until(Instant),
}

/// Ce qu'il faut pour les codes de vérification, hors de la session.
#[derive(Clone, Debug, Default)]
pub struct CodeInputs {
    /// Code de l'appel (30 chiffres), s'il y a un groupe établi.
    pub call: Option<String>,
    me: (u64, Vec<u8>),
    /// Membres du groupe (moi exclu) et leur clé d'identité.
    peers: Vec<(u64, Vec<u8>)>,
}

impl CodeInputs {
    pub fn users(&self) -> impl Iterator<Item = u64> + '_ {
        self.peers.iter().map(|(u, _)| *u)
    }

    /// Code de paire (45 chiffres) avec `user_id` : un scrypt (16 Mio,
    /// quelques dizaines de ms), à calculer hors du fil d'interface.
    pub fn pairwise(&self, user_id: u64) -> Option<String> {
        let (_, key) = self.peers.iter().find(|(u, _)| *u == user_id)?;
        let fp = verify::pairwise_fingerprint(key, user_id, &self.me.1, self.me.0)?;
        verify::displayable_code(&fp, 45, 5)
    }
}

pub struct DaveSession {
    user_id: u64,
    group_id: Vec<u8>,
    sig: Arc<SigKey>,
    external: Option<ExternalSender>,
    /// Version annoncée par la passerelle pour le prochain groupe.
    target: u16,
    /// Version en vigueur pour l'envoi (0 : trames en clair).
    protocol: u16,
    join: Option<Join>,
    /// Groupe local à un membre, en attente d'être établi.
    pending: Option<Group>,
    group: Option<Group>,
    queue: Vec<Queued>,
    outbound: VecDeque<(Vec<u8>, Group)>,
    recognized: HashSet<u64>,
    /// Transitions annoncées : identifiant -> version.
    transitions: HashMap<u16, u16>,
    /// Notre clé d'envoi de la prochaine époque, par transition.
    send_bases: HashMap<u16, Vec<u8>>,
    encryptor: Encryptor,
    decryptor: Decryptor,
    passthrough: Passthrough,
    /// Codes de paire déjà calculés (scrypt est coûteux) : (clé du pair, code).
    codes: Mutex<HashMap<u64, (Vec<u8>, String)>>,
}

fn data(json: Option<&Value>) -> Option<&Value> {
    let v = json?;
    Some(v.get("d").filter(|d| d.is_object()).unwrap_or(v))
}

fn num(v: &Value, key: &str) -> Option<u64> {
    let f = v.get(key)?;
    f.as_u64().or_else(|| f.as_str()?.parse().ok())
}

fn ready(tid: u16) -> Outgoing {
    Outgoing::Json(json!({ "op": 23, "d": { "transition_id": tid } }))
}

impl DaveSession {
    /// `channel_id` sert d'identifiant de groupe MLS (gros-boutiste), comme
    /// dans les clients officiels.
    pub fn new(user_id: u64, channel_id: u64) -> Result<Self, Error> {
        Ok(DaveSession {
            user_id,
            group_id: channel_id.to_be_bytes().to_vec(),
            sig: Arc::new(SigKey::generate()?),
            external: None,
            target: 0,
            protocol: 0,
            join: None,
            pending: None,
            group: None,
            queue: Vec::new(),
            outbound: VecDeque::new(),
            recognized: HashSet::new(),
            transitions: HashMap::new(),
            send_bases: HashMap::new(),
            encryptor: Encryptor::default(),
            decryptor: Decryptor::default(),
            passthrough: Passthrough::Forever,
            codes: Mutex::new(HashMap::new()),
        })
    }

    /// Version de DAVE en vigueur pour nos trames (0 : chiffrement de transport seul).
    pub fn protocol_version(&self) -> u16 {
        self.protocol
    }

    /// Le groupe MLS est établi et nos trames partent chiffrées.
    pub fn is_encrypting(&self) -> bool {
        self.protocol > 0 && self.encryptor.has_key()
    }

    /// Époque MLS courante (0 hors groupe établi).
    pub fn epoch(&self) -> u64 {
        self.group.as_ref().map_or(0, |g| g.epoch)
    }

    /// Code de l'appel (authentifiant d'époque, 30 chiffres), identique chez tous les membres.
    pub fn call_code(&self) -> Option<String> {
        verify::displayable_code(&self.group.as_ref()?.authenticator, 30, 5)
    }

    /// Clés publiques du groupe, copiées pour calculer les codes de paire
    /// ailleurs (scrypt coûteux) sans tenir la session pendant ce temps.
    pub fn code_inputs(&self) -> CodeInputs {
        let peers = self.group.as_ref().map(|g| g.roster().into_iter().filter(|(u, _)| *u != self.user_id).collect()).unwrap_or_default();
        CodeInputs { call: self.call_code(), me: (self.user_id, self.sig.public().to_vec()), peers }
    }

    fn new_join(&self) -> Result<Join, Error> {
        let leaf_key = Arc::new(HpkeKey::generate()?);
        let mut leaf = LeafNode {
            enc: leaf_key.public().to_vec(),
            sig: self.sig.public().to_vec(),
            identity: self.user_id.to_be_bytes().to_vec(),
            caps: default_capabilities(),
            // La passerelle exige la durée de vie maximale.
            source: LeafSource::KeyPackage { not_before: 0, not_after: u64::MAX },
            exts: vec![0],
            signature: Vec::new(),
        };
        leaf.sign(&self.sig, &[], 0)?;
        let init_key = HpkeKey::generate()?;
        let kp = KeyPackage::create(init_key.public(), leaf, &self.sig)?;
        Ok(Join { leaf_key, init_key, kp })
    }

    fn create_pending(&mut self) {
        self.pending = None;
        self.queue.clear();
        self.outbound.clear();
        let (Some(ext), Some(join)) = (&self.external, &self.join) else { return };
        self.pending =
            Group::create(self.group_id.clone(), join.kp.leaf.clone(), join.leaf_key.clone(), self.sig.clone(), ext.group_extensions()).ok();
    }

    /// Repart d'un groupe vide et envoie un nouveau paquet de clés (op 26).
    fn reset(&mut self, out: &mut Vec<Outgoing>) {
        self.group = None;
        self.codes_clear();
        self.decryptor.expire_all(Instant::now());
        self.join = self.new_join().ok();
        self.create_pending();
        if let Some(j) = &self.join {
            out.push(Outgoing::Binary(26, j.kp.raw.clone()));
        }
    }

    fn codes_clear(&self) {
        if let Ok(mut c) = self.codes.lock() {
            c.clear();
        }
    }

    fn recognized(&self, uid: Option<u64>) -> bool {
        uid.is_some_and(|u| u == self.user_id || self.recognized.contains(&u))
    }

    fn execute(&mut self, tid: u16) {
        let now = Instant::now();
        let version = self.transitions.remove(&tid).unwrap_or(self.target);
        if tid == 0 && self.group.is_none() {
            // Remise à zéro du membre seul : on passe à notre groupe local.
            if let Some(p) = self.pending.take() {
                self.adopt(p, 0);
            }
        }
        self.protocol = version;
        if version == 0 {
            self.encryptor.set_base(None);
            self.passthrough = Passthrough::Forever;
            return;
        }
        if let Some(b) = self.send_bases.remove(&tid) {
            self.encryptor.set_base(Some(b));
        }
        if self.passthrough == Passthrough::Forever {
            self.passthrough = Passthrough::Until(now + frame::EXPIRY);
        }
    }

    /// Nouveau groupe établi : clés de réception tout de suite, clé d'envoi
    /// à l'exécution de la transition.
    fn adopt(&mut self, g: Group, tid: u16) {
        let root = g.export_root(frame::EXPORT_LABEL);
        let members = g.roster().into_iter().map(|(u, _)| u).collect();
        self.decryptor.install(EpochKeys::new(root.clone(), members), Instant::now());
        self.send_bases.insert(tid, frame::sender_base(&root, self.user_id));
        self.group = Some(g);
        self.pending = None;
        self.join = None;
        self.queue.clear();
        self.outbound.clear();
        self.codes_clear();
    }

    fn established(&mut self, g: Group, tid: u16, out: &mut Vec<Outgoing>) {
        self.adopt(g, tid);
        if tid == 0 {
            self.execute(0);
        } else {
            out.push(ready(tid));
        }
    }

    /// Commit ou Welcome illisible : on le signale (op 31) et on repart.
    fn recover(&mut self, tid: u16, out: &mut Vec<Outgoing>) {
        out.push(Outgoing::Json(json!({ "op": 31, "d": { "transition_id": tid } })));
        self.reset(out);
    }

    fn on_external_sender(&mut self, payload: &[u8], out: &mut Vec<Outgoing>) -> Result<(), Error> {
        let mut r = Reader::new(payload);
        let ext = ExternalSender::read(&mut r)?;
        r.end()?;
        if self.group.is_some() || (self.pending.is_some() && self.external.as_ref() == Some(&ext)) {
            return Ok(());
        }
        self.external = Some(ext);
        if self.join.is_none() {
            // Op 4 non transmis : l'expéditeur externe suffit à savoir que DAVE est actif.
            if self.target == 0 {
                self.target = PROTOCOL_VERSION;
            }
            self.reset(out);
        } else {
            self.create_pending();
        }
        Ok(())
    }

    fn on_proposals(&mut self, payload: &[u8], out: &mut Vec<Outgoing>) -> Result<(), Error> {
        let Some(base) = self.pending.as_ref().or(self.group.as_ref()) else { return Ok(()) };
        let mut r = Reader::new(payload);
        let revoke = r.u8()?;
        let mut list = r.sub()?;
        r.end()?;
        let mut queue = self.queue.clone();
        match revoke {
            0 => {
                while !list.is_empty() {
                    let q = base.unwrap_proposal(&mut list)?;
                    if queue.iter().any(|x| x.reference == q.reference) {
                        return Err(Error("proposition en double"));
                    }
                    if let Proposal::Add(kp) = &q.proposal
                        && !self.recognized(kp.leaf.user_id())
                    {
                        return Err(Error("ajout d'un inconnu"));
                    }
                    queue.push(q);
                }
            }
            1 => {
                while !list.is_empty() {
                    let reference = list.vec()?;
                    let i = queue.iter().position(|q| q.reference == reference).ok_or(Error("révocation inconnue"))?;
                    queue.remove(i);
                }
            }
            _ => return Err(Error("opération inconnue")),
        }
        if queue.is_empty() {
            self.queue = queue;
            return Ok(());
        }
        let c = base.commit(&queue)?;
        self.queue = queue;
        let mut payload = c.message.clone();
        payload.extend(c.welcome.unwrap_or_default());
        if self.outbound.len() >= MAX_OUTBOUND {
            self.outbound.pop_front();
        }
        self.outbound.push_back((c.message, c.next));
        out.push(Outgoing::Binary(28, payload));
        Ok(())
    }

    fn on_commit(&mut self, payload: &[u8], out: &mut Vec<Outgoing>) -> Result<(), Error> {
        let mut r = Reader::new(payload);
        let tid = r.u16()?;
        let msg = r.rest();
        let result = if let Some((_, next)) = self.outbound.iter().find(|(m, _)| m.as_slice() == msg) {
            Ok(next.clone())
        } else if let Some(g) = &self.group {
            g.process_commit(msg, &self.queue)
        } else {
            // Pas encore de groupe établi et Commit d'un autre : notre Welcome suivra.
            return Ok(());
        };
        match result {
            Ok(g) => self.established(g, tid, out),
            Err(_) => self.recover(tid, out),
        }
        Ok(())
    }

    fn on_welcome(&mut self, payload: &[u8], out: &mut Vec<Outgoing>) -> Result<(), Error> {
        let mut r = Reader::new(payload);
        let tid = r.u16()?;
        let welcome = r.rest();
        if self.group.is_some() {
            return Ok(());
        }
        let Some(join) = &self.join else { return Ok(()) };
        let joined = Group::join(welcome, &join.kp, &join.init_key, join.leaf_key.clone(), self.sig.clone(), None).and_then(|g| {
            self.check_welcome(&g)?;
            Ok(g)
        });
        match joined {
            Ok(g) => self.established(g, tid, out),
            Err(_) => self.recover(tid, out),
        }
        Ok(())
    }

    /// Vérifications propres à DAVE d'un groupe rejoint (protocol.md, « Invalid Groups »).
    fn check_welcome(&self, g: &Group) -> Result<(), Error> {
        if g.group_id != self.group_id {
            return Err(Error("groupe inattendu"));
        }
        let senders = mls::msg::external_senders(g.extensions())?;
        if senders.len() != 1 || Some(&senders[0]) != self.external.as_ref() {
            return Err(Error("expéditeur externe inattendu"));
        }
        let mut seen = HashSet::new();
        for (_, leaf) in g.tree.leaves() {
            let uid = leaf.user_id();
            if !self.recognized(uid) || !seen.insert(uid) {
                return Err(Error("membre inattendu"));
            }
        }
        Ok(())
    }

    fn handle(&mut self, op: u8, json: Option<&Value>, binary: Option<&[u8]>, out: &mut Vec<Outgoing>) -> Result<(), Error> {
        let d = data(json);
        let field = |k: &str| d.and_then(|d| num(d, k));
        let tid = || field("transition_id").and_then(|t| u16::try_from(t).ok());
        match op {
            4 => {
                let v = field("dave_protocol_version").unwrap_or(0) as u16;
                self.target = v;
                self.protocol = v;
                if v > 0 {
                    self.reset(out);
                }
            }
            11 => {
                let ids = d.and_then(|d| d.get("user_ids")).and_then(Value::as_array);
                for id in ids.into_iter().flatten() {
                    if let Some(u) = id.as_u64().or_else(|| id.as_str()?.parse().ok()) {
                        self.recognized.insert(u);
                    }
                }
            }
            13 => {
                if let Some(u) = field("user_id") {
                    self.recognized.remove(&u);
                }
            }
            21 => {
                let tid = tid().ok_or(Error("transition sans identifiant"))?;
                let version = field("protocol_version").unwrap_or(0) as u16;
                self.transitions.insert(tid, version);
                if version == 0 {
                    self.passthrough = Passthrough::Forever;
                }
                if tid == 0 {
                    self.execute(0);
                } else {
                    out.push(ready(tid));
                }
            }
            22 => self.execute(tid().ok_or(Error("transition sans identifiant"))?),
            24 => {
                self.target = field("protocol_version").unwrap_or(u64::from(self.target)) as u16;
                if field("epoch") == Some(1) {
                    self.reset(out);
                }
            }
            25 => self.on_external_sender(binary.ok_or(Error("op 25 sans charge"))?, out)?,
            27 => self.on_proposals(binary.ok_or(Error("op 27 sans charge"))?, out)?,
            29 => self.on_commit(binary.ok_or(Error("op 29 sans charge"))?, out)?,
            30 => self.on_welcome(binary.ok_or(Error("op 30 sans charge"))?, out)?,
            _ => {}
        }
        Ok(())
    }
}

impl FrameCrypto for DaveSession {
    fn max_protocol_version(&self) -> u16 {
        PROTOCOL_VERSION
    }

    fn on_op(&mut self, op: u8, json: Option<&Value>, binary: Option<&[u8]>, out: &mut Vec<Outgoing>) {
        // Un message malformé est ignoré, comme libdave : la passerelle
        // reprendra par une nouvelle époque si le groupe diverge.
        let _ = self.handle(op, json, binary, out);
    }

    fn on_users(&mut self, connected: &[u64]) {
        self.recognized = connected.iter().copied().collect();
    }

    fn encrypt(&mut self, opus: &[u8]) -> Option<Vec<u8>> {
        if self.protocol == 0 {
            return Some(opus.to_vec());
        }
        // Pas encore de clé : rien ne part plutôt que de l'audio en clair.
        self.encryptor.encrypt(opus)
    }

    fn decrypt(&mut self, user_id: u64, frame: &[u8]) -> Option<Vec<u8>> {
        if frame == frame::SILENCE {
            return Some(frame.to_vec());
        }
        self.decrypt_media(user_id, frame)
    }

    /// Un seul chiffreur (un seul compteur de nonce) pour l'audio et la
    /// vidéo, comme le déchiffreur unique par expéditeur : le rejeu reste
    /// détecté quel que soit le média.
    fn encrypt_video(&mut self, codec: crate::video::Codec, frame: &[u8]) -> Option<Vec<u8>> {
        if self.protocol == 0 {
            return Some(frame.to_vec());
        }
        let (data, clear, h26x) = video::layout(codec, frame)?;
        self.encryptor.encrypt_ranges(&data, &clear, h26x)
    }

    fn decrypt_video(&mut self, user_id: u64, _codec: crate::video::Codec, frame: &[u8]) -> Option<Vec<u8>> {
        self.decrypt_media(user_id, frame)
    }

    /// Code de paire (45 chiffres) avec `user_id` ; pour notre propre
    /// identifiant, code de l'appel (30 chiffres). Le premier calcul d'un
    /// code de paire coûte un scrypt (16 Mio, quelques dizaines de ms) :
    /// à appeler hors du fil d'interface ; ensuite il est en cache.
    fn verification_code(&self, user_id: u64) -> Option<String> {
        self.verification_code_inner(user_id)
    }
}

impl DaveSession {
    fn decrypt_media(&mut self, user_id: u64, frame: &[u8]) -> Option<Vec<u8>> {
        let now = Instant::now();
        match frame::parse(frame) {
            Some(p) => self.decryptor.decrypt(user_id, &p, now),
            None => {
                let open = match self.passthrough {
                    Passthrough::Forever => true,
                    Passthrough::Until(t) => now < t,
                };
                open.then(|| frame.to_vec())
            }
        }
    }

    fn verification_code_inner(&self, user_id: u64) -> Option<String> {
        if user_id == self.user_id {
            return self.call_code();
        }
        let g = self.group.as_ref()?;
        let (_, key) = g.roster().into_iter().find(|(u, _)| *u == user_id)?;
        if let Ok(c) = self.codes.lock()
            && let Some((k, code)) = c.get(&user_id)
            && *k == key
        {
            return Some(code.clone());
        }
        let fp = verify::pairwise_fingerprint(&key, user_id, self.sig.public(), self.user_id)?;
        let code = verify::displayable_code(&fp, 45, 5)?;
        if let Ok(mut c) = self.codes.lock() {
            c.insert(user_id, (key, code.clone()));
        }
        Some(code)
    }
}
