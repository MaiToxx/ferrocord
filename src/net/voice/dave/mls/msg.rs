//! Structures MLS utilisées par DAVE (RFC 9420) : nœuds feuilles, paquets de
//! clés, propositions, expéditeur externe. Les champs que DAVE n'interprète
//! pas (capacités, extensions) sont gardés tels quels pour réencoder à
//! l'identique ce qui est signé et haché.

use super::super::Error;
use super::super::codec::{R, Reader, Writer, build};
use super::super::crypto;

pub const VERSION: u16 = 1;
pub const SUITE: u16 = 2;

pub const WF_PUBLIC: u16 = 1;

pub const EXT_RATCHET_TREE: u16 = 2;
pub const EXT_EXTERNAL_SENDERS: u16 = 5;

pub const PROP_ADD: u16 = 1;
pub const PROP_REMOVE: u16 = 3;

pub const CT_PROPOSAL: u8 = 2;
pub const CT_COMMIT: u8 = 3;

pub const SENDER_MEMBER: u8 = 1;
pub const SENDER_EXTERNAL: u8 = 2;

const CRED_BASIC: u16 = 1;

/// Lit une `Credential` ; DAVE n'admet que le type basique.
fn read_credential(r: &mut Reader) -> R<Vec<u8>> {
    if r.u16()? != CRED_BASIC {
        return Err(Error("identité non basique"));
    }
    Ok(r.vec()?.to_vec())
}

fn write_credential(w: &mut Writer, identity: &[u8]) {
    w.u16(CRED_BASIC).vec(identity);
}

/// Identifiant Discord d'une identité basique : 8 octets gros-boutistes.
pub fn user_id_of(identity: &[u8]) -> Option<u64> {
    let a: [u8; 8] = identity.try_into().ok()?;
    Some(u64::from_be_bytes(a))
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LeafSource {
    KeyPackage { not_before: u64, not_after: u64 },
    Update,
    Commit { parent_hash: Vec<u8> },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LeafNode {
    pub enc: Vec<u8>,
    pub sig: Vec<u8>,
    pub identity: Vec<u8>,
    /// `Capabilities` encodée.
    pub caps: Vec<u8>,
    pub source: LeafSource,
    /// `Extension extensions<V>` encodé (longueur comprise).
    pub exts: Vec<u8>,
    pub signature: Vec<u8>,
}

/// Capacités annoncées par Ferrocord : MLS 1.0, suite 2, identité basique.
pub fn default_capabilities() -> Vec<u8> {
    build(|w| {
        w.nested(|w| {
            w.u16(VERSION);
        });
        w.nested(|w| {
            w.u16(SUITE);
        });
        w.nested(|_| {});
        w.nested(|_| {});
        w.nested(|w| {
            w.u16(CRED_BASIC);
        });
    })
}

impl LeafNode {
    pub fn read(r: &mut Reader) -> R<Self> {
        let enc = r.vec()?.to_vec();
        let sig = r.vec()?.to_vec();
        let identity = read_credential(r)?;
        let start = r.pos();
        for _ in 0..5 {
            r.vec()?;
        }
        let caps = r.since(start).to_vec();
        let source = match r.u8()? {
            1 => LeafSource::KeyPackage { not_before: r.u64()?, not_after: r.u64()? },
            2 => LeafSource::Update,
            3 => LeafSource::Commit { parent_hash: r.vec()?.to_vec() },
            _ => return Err(Error("source de feuille inconnue")),
        };
        let start = r.pos();
        r.vec()?;
        let exts = r.since(start).to_vec();
        let signature = r.vec()?.to_vec();
        Ok(LeafNode { enc, sig, identity, caps, source, exts, signature })
    }

    fn write_body(&self, w: &mut Writer) {
        w.vec(&self.enc).vec(&self.sig);
        write_credential(w, &self.identity);
        w.raw(&self.caps);
        match &self.source {
            LeafSource::KeyPackage { not_before, not_after } => {
                w.u8(1).u64(*not_before).u64(*not_after);
            }
            LeafSource::Update => {
                w.u8(2);
            }
            LeafSource::Commit { parent_hash } => {
                w.u8(3).vec(parent_hash);
            }
        }
        w.raw(&self.exts);
    }

    pub fn write(&self, w: &mut Writer) {
        self.write_body(w);
        w.vec(&self.signature);
    }

    fn tbs(&self, group_id: &[u8], leaf_index: u32) -> Vec<u8> {
        build(|w| {
            self.write_body(w);
            if !matches!(self.source, LeafSource::KeyPackage { .. }) {
                w.vec(group_id).u32(leaf_index);
            }
        })
    }

    pub fn sign(&mut self, key: &crypto::SigKey, group_id: &[u8], leaf_index: u32) -> R<()> {
        self.signature = key.sign_with_label("LeafNodeTBS", &self.tbs(group_id, leaf_index))?;
        Ok(())
    }

    pub fn verify(&self, group_id: &[u8], leaf_index: u32) -> bool {
        crypto::verify_with_label(&self.sig, "LeafNodeTBS", &self.tbs(group_id, leaf_index), &self.signature)
    }

    pub fn user_id(&self) -> Option<u64> {
        user_id_of(&self.identity)
    }

    pub fn parent_hash(&self) -> Option<&[u8]> {
        match &self.source {
            LeafSource::Commit { parent_hash } => Some(parent_hash),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct KeyPackage {
    pub init_key: Vec<u8>,
    pub leaf: LeafNode,
    /// Encodage complet (pour la référence).
    pub raw: Vec<u8>,
    tbs_len: usize,
    signature: Vec<u8>,
}

impl KeyPackage {
    pub fn read(r: &mut Reader) -> R<Self> {
        let start = r.pos();
        if r.u16()? != VERSION || r.u16()? != SUITE {
            return Err(Error("paquet de clés : version ou suite"));
        }
        let init_key = r.vec()?.to_vec();
        let leaf = LeafNode::read(r)?;
        r.vec()?;
        let tbs_len = r.pos() - start;
        let signature = r.vec()?.to_vec();
        let raw = r.since(start).to_vec();
        Ok(KeyPackage { init_key, leaf, raw, tbs_len, signature })
    }

    /// Construit et signe un paquet de clés pour la feuille donnée.
    pub fn create(init_key: &[u8], leaf: LeafNode, key: &crypto::SigKey) -> R<Self> {
        let tbs = build(|w| {
            w.u16(VERSION).u16(SUITE).vec(init_key);
            leaf.write(w);
            w.nested(|_| {});
        });
        let signature = key.sign_with_label("KeyPackageTBS", &tbs)?;
        let mut raw = tbs.clone();
        let mut w = Writer::new();
        w.vec(&signature);
        raw.extend_from_slice(&w.buf);
        Ok(KeyPackage { init_key: init_key.to_vec(), leaf, raw, tbs_len: tbs.len(), signature })
    }

    /// Validation RFC 9420, 10.1 (sauf la durée de vie, que DAVE ignore).
    pub fn verify(&self) -> bool {
        self.init_key != self.leaf.enc
            && matches!(self.leaf.source, LeafSource::KeyPackage { .. })
            && crypto::verify_with_label(&self.leaf.sig, "KeyPackageTBS", &self.raw[..self.tbs_len], &self.signature)
            && self.leaf.verify(&[], 0)
    }

    pub fn reference(&self) -> Vec<u8> {
        crypto::ref_hash("MLS 1.0 KeyPackage Reference", &self.raw)
    }
}

#[derive(Clone, Debug)]
pub enum Proposal {
    Add(Box<KeyPackage>),
    Remove(u32),
}

impl Proposal {
    pub fn read(r: &mut Reader) -> R<Self> {
        match r.u16()? {
            PROP_ADD => Ok(Proposal::Add(Box::new(KeyPackage::read(r)?))),
            PROP_REMOVE => Ok(Proposal::Remove(r.u32()?)),
            _ => Err(Error("proposition non admise par DAVE")),
        }
    }

    #[cfg(test)]
    pub fn write(&self, w: &mut Writer) {
        match self {
            Proposal::Add(kp) => {
                w.u16(PROP_ADD).raw(&kp.raw);
            }
            Proposal::Remove(i) => {
                w.u16(PROP_REMOVE).u32(*i);
            }
        }
    }
}

/// `ExternalSender` (RFC 9420, 12.1.8.1), tel que l'envoie l'op 25.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExternalSender {
    pub sig: Vec<u8>,
    pub identity: Vec<u8>,
}

impl ExternalSender {
    pub fn read(r: &mut Reader) -> R<Self> {
        let sig = r.vec()?.to_vec();
        let identity = read_credential(r)?;
        Ok(ExternalSender { sig, identity })
    }

    pub fn write(&self, w: &mut Writer) {
        w.vec(&self.sig);
        write_credential(w, &self.identity);
    }

    /// Liste d'extensions du groupe DAVE : le seul expéditeur externe.
    pub fn group_extensions(&self) -> Vec<u8> {
        build(|w| {
            w.nested(|w| {
                w.u16(EXT_EXTERNAL_SENDERS).nested(|w| {
                    w.nested(|w| self.write(w));
                });
            });
        })
    }
}

/// Extrait les expéditeurs externes d'une liste d'extensions encodée.
pub fn external_senders(exts: &[u8]) -> R<Vec<ExternalSender>> {
    let mut r = Reader::new(exts);
    let mut list = r.sub()?;
    let mut out = Vec::new();
    while !list.is_empty() {
        let ty = list.u16()?;
        let data = list.vec()?;
        if ty == EXT_EXTERNAL_SENDERS {
            let mut d = Reader::new(data);
            let mut senders = d.sub()?;
            while !senders.is_empty() {
                out.push(ExternalSender::read(&mut senders)?);
            }
        }
    }
    Ok(out)
}

/// Cherche une extension dans une liste encodée.
pub fn find_extension(exts: &[u8], ty: u16) -> R<Option<Vec<u8>>> {
    let mut r = Reader::new(exts);
    let mut list = r.sub()?;
    while !list.is_empty() {
        let t = list.u16()?;
        let data = list.vec()?;
        if t == ty {
            return Ok(Some(data.to_vec()));
        }
    }
    Ok(None)
}
