//! Sous-ensemble de MLS 1.0 (RFC 9420) dont DAVE a besoin : groupe à un
//! membre, propositions externes Add/Remove, validation (Commit par
//! référence, avec ou sans UpdatePath), Welcome avec l'arbre en ligne,
//! exportateur et authentifiant d'époque. Messages de contrôle en clair
//! (PublicMessage) seulement, comme l'exige DAVE ; pas de PSK, pas de
//! messages applicatifs MLS.

pub mod msg;
pub mod tree;

use std::sync::Arc;

use super::Error;
use super::codec::{R, Reader, build};
use super::crypto::{self, HpkeKey, NH, SigKey};
use msg::{CT_COMMIT, CT_PROPOSAL, KeyPackage, LeafNode, LeafSource, Proposal, SENDER_EXTERNAL, SENDER_MEMBER, SUITE, VERSION, WF_PUBLIC};
use tree::{Node, ParentNode, Tree, common_ancestor, in_subtree, leaf_node, left, right};

/// Proposition reçue de la passerelle, en attente d'un Commit.
#[derive(Clone, Debug)]
pub struct Queued {
    pub reference: Vec<u8>,
    pub proposal: Proposal,
}

pub struct CommitOut {
    /// MLSMessage (PublicMessage) du Commit.
    pub message: Vec<u8>,
    /// Welcome (sans enveloppe MLSMessage, comme libdave), s'il y a des ajouts.
    pub welcome: Option<Vec<u8>>,
    pub next: Group,
}

struct Schedule {
    joiner: Vec<u8>,
    welcome: Vec<u8>,
    init: Vec<u8>,
    exporter: Vec<u8>,
    membership: Vec<u8>,
    confirm: Vec<u8>,
    authentication: Vec<u8>,
}

fn from_epoch_secret(epoch: &[u8], joiner: Vec<u8>, welcome: Vec<u8>) -> Schedule {
    Schedule {
        joiner,
        welcome,
        init: crypto::derive_secret(epoch, "init"),
        exporter: crypto::derive_secret(epoch, "exporter"),
        membership: crypto::derive_secret(epoch, "membership"),
        confirm: crypto::derive_secret(epoch, "confirm"),
        authentication: crypto::derive_secret(epoch, "authentication"),
    }
}

fn from_joiner(joiner: Vec<u8>, ctx: &[u8]) -> Schedule {
    // Pas de PSK : psk_secret = Nh zéros.
    from_joiner_psk(joiner, &[0u8; NH], ctx)
}

fn from_joiner_psk(joiner: Vec<u8>, psk: &[u8], ctx: &[u8]) -> Schedule {
    let member = crypto::extract(&joiner, psk);
    let welcome = crypto::derive_secret(&member, "welcome");
    let epoch = crypto::expand_with_label(&member, "epoch", ctx, NH);
    from_epoch_secret(&epoch, joiner, welcome)
}

fn schedule(init_prev: &[u8], commit_secret: &[u8], ctx: &[u8]) -> Schedule {
    let pre = crypto::extract(init_prev, commit_secret);
    from_joiner(crypto::expand_with_label(&pre, "joiner", ctx, NH), ctx)
}

fn context_bytes(gid: &[u8], epoch: u64, tree_hash: &[u8], confirmed: &[u8], exts: &[u8]) -> Vec<u8> {
    build(|w| {
        w.u16(VERSION).u16(SUITE).vec(gid).u64(epoch).vec(tree_hash).vec(confirmed).raw(exts);
    })
}

fn interim_of(confirmed: &[u8], tag: &[u8]) -> Vec<u8> {
    let mut m = confirmed.to_vec();
    m.extend(build(|w| {
        w.vec(tag);
    }));
    crypto::hash(&m)
}

pub fn export_with_root(root: &[u8], context: &[u8], len: usize) -> Vec<u8> {
    crypto::expand_with_label(root, "exported", &crypto::hash(context), len)
}

fn welcome_key_nonce(welcome_secret: &[u8]) -> (Vec<u8>, Vec<u8>) {
    (
        crypto::expand_with_label(welcome_secret, "key", &[], crypto::KEY_LEN),
        crypto::expand_with_label(welcome_secret, "nonce", &[], crypto::NONCE_LEN),
    )
}

/// Enfant de `p` du côté de `x`.
fn child_toward(p: usize, x: usize) -> usize {
    if in_subtree(x, left(p)) { left(p) } else { right(p) }
}

/// Pose les hachages parents d'un chemin fraîchement écrit, de la racine
/// vers la feuille ; rend celui que la feuille doit porter.
fn set_parent_hashes(t: &mut Tree, x: usize, fdp: &[(usize, usize)]) -> R<Vec<u8>> {
    let mut leaf_ph = Vec::new();
    for i in (0..fdp.len()).rev() {
        let p = fdp[i].0;
        let h = t.parent_hash_of(p, child_toward(p, x)).ok_or(Error("hachage parent"))?;
        if i == 0 {
            leaf_ph = h;
        } else if let Some(Some(Node::Parent(pn))) = t.nodes.get_mut(fdp[i - 1].0) {
            pn.parent_hash = h;
        }
    }
    Ok(leaf_ph)
}

struct PathNode {
    enc: Vec<u8>,
    cts: Vec<(Vec<u8>, Vec<u8>)>,
}

struct ParsedCommit<'a> {
    content: &'a [u8],
    sender: u32,
    refs: Vec<Vec<u8>>,
    path: Option<(LeafNode, Vec<PathNode>)>,
    signature: Vec<u8>,
    tag: Vec<u8>,
    membership: Vec<u8>,
}

/// Lit un MLSMessage PublicMessage jusqu'au contenu ; rend le lecteur placé
/// au début de FramedContent.
fn open_public(msg: &[u8]) -> R<Reader<'_>> {
    let mut r = Reader::new(msg);
    if r.u16()? != VERSION || r.u16()? != WF_PUBLIC {
        return Err(Error("message MLS inattendu"));
    }
    Ok(r)
}

fn parse_commit(msg: &[u8]) -> R<(Vec<u8>, u64, ParsedCommit<'_>)> {
    let mut r = open_public(msg)?;
    let c = read_commit(&mut r)?;
    r.end()?;
    Ok(c)
}

/// Lit un PublicMessage de Commit (après version et format) ; laisse le
/// lecteur juste après.
fn read_commit<'a>(r: &mut Reader<'a>) -> R<(Vec<u8>, u64, ParsedCommit<'a>)> {
    let start = r.pos();
    let gid = r.vec()?.to_vec();
    let epoch = r.u64()?;
    if r.u8()? != SENDER_MEMBER {
        return Err(Error("Commit d'un non-membre"));
    }
    let sender = r.u32()?;
    r.vec()?;
    if r.u8()? != CT_COMMIT {
        return Err(Error("pas un Commit"));
    }
    let mut props = r.sub()?;
    let mut refs = Vec::new();
    while !props.is_empty() {
        // DAVE : propositions uniquement par référence.
        if props.u8()? != 2 {
            return Err(Error("proposition en ligne refusée"));
        }
        refs.push(props.vec()?.to_vec());
    }
    let path = if r.present()? {
        let leaf = LeafNode::read(r)?;
        let mut nodes = r.sub()?;
        let mut out = Vec::new();
        while !nodes.is_empty() {
            let enc = nodes.vec()?.to_vec();
            let mut cts = nodes.sub()?;
            let mut list = Vec::new();
            while !cts.is_empty() {
                list.push((cts.vec()?.to_vec(), cts.vec()?.to_vec()));
            }
            out.push(PathNode { enc, cts: list });
        }
        Some((leaf, out))
    } else {
        None
    };
    let content = r.since(start);
    let signature = r.vec()?.to_vec();
    let tag = r.vec()?.to_vec();
    let membership = r.vec()?.to_vec();
    Ok((gid, epoch, ParsedCommit { content, sender, refs, path, signature, tag, membership }))
}

/// Applique les propositions (retraits puis ajouts, RFC 9420, 12.3).
fn apply(t: &mut Tree, props: &[&Proposal]) -> R<(Vec<(u32, KeyPackage)>, bool)> {
    let mut removed = false;
    for p in props {
        if let Proposal::Remove(i) = p {
            t.remove_leaf(*i)?;
            removed = true;
        }
    }
    let mut joiners = Vec::new();
    for p in props {
        if let Proposal::Add(kp) = p {
            let i = t.add_leaf(kp.leaf.clone());
            joiners.push((i, (**kp).clone()));
        }
    }
    Ok((joiners, removed))
}

#[derive(Clone)]
pub struct Group {
    pub group_id: Vec<u8>,
    pub epoch: u64,
    pub tree: Tree,
    exts: Vec<u8>,
    tree_hash: Vec<u8>,
    confirmed: Vec<u8>,
    interim: Vec<u8>,
    init_secret: Vec<u8>,
    exporter_secret: Vec<u8>,
    membership_key: Vec<u8>,
    pub authenticator: Vec<u8>,
    pub me: u32,
    keys: Vec<(usize, Arc<HpkeKey>)>,
    sig: Arc<SigKey>,
}

impl Group {
    /// Groupe à un seul membre (RFC 9420, 11), époque 0.
    pub fn create(gid: Vec<u8>, leaf: LeafNode, leaf_key: Arc<HpkeKey>, sig: Arc<SigKey>, exts: Vec<u8>) -> R<Self> {
        let tree = Tree::with_leaf(leaf);
        let tree_hash = tree.tree_hash();
        let s =from_epoch_secret(&crypto::random_secret()?, Vec::new(), Vec::new());
        let tag = crypto::mac(&s.confirm, &[]);
        Ok(Group {
            group_id: gid,
            epoch: 0,
            tree,
            exts,
            tree_hash,
            confirmed: Vec::new(),
            interim: interim_of(&[], &tag),
            init_secret: s.init,
            exporter_secret: s.exporter,
            membership_key: s.membership,
            authenticator: s.authentication,
            me: 0,
            keys: vec![(0, leaf_key)],
            sig,
        })
    }

    pub fn context(&self) -> Vec<u8> {
        context_bytes(&self.group_id, self.epoch, &self.tree_hash, &self.confirmed, &self.exts)
    }

    pub fn extensions(&self) -> &[u8] {
        &self.exts
    }

    /// MLS-Exporter (RFC 9420, 8.5).
    #[cfg(test)]
    pub fn export(&self, label: &str, context: &[u8], len: usize) -> Vec<u8> {
        export_with_root(&self.export_root(label), context, len)
    }

    /// Première moitié de l'exportateur, propre à l'étiquette : gardée seule
    /// pour dériver plus tard la clé de chaque expéditeur sans garder le groupe.
    pub fn export_root(&self, label: &str) -> Vec<u8> {
        crypto::derive_secret(&self.exporter_secret, label)
    }

    /// Membres : (identifiant Discord, clé publique de signature).
    pub fn roster(&self) -> Vec<(u64, Vec<u8>)> {
        self.tree.leaves().filter_map(|(_, l)| Some((l.user_id()?, l.sig.clone()))).collect()
    }

    fn successor(&self, tree: Tree, confirmed: Vec<u8>, tag: &[u8], s: Schedule, keys: Vec<(usize, Arc<HpkeKey>)>) -> Group {
        let tree_hash = tree.tree_hash();
        Group {
            group_id: self.group_id.clone(),
            epoch: self.epoch.wrapping_add(1),
            interim: interim_of(&confirmed, tag),
            tree,
            exts: self.exts.clone(),
            tree_hash,
            confirmed,
            init_secret: s.init,
            exporter_secret: s.exporter,
            membership_key: s.membership,
            authenticator: s.authentication,
            me: self.me,
            keys,
            sig: self.sig.clone(),
        }
    }

    /// Ne garde que les clés privées encore portées par l'arbre.
    fn live_keys(tree: &Tree, keys: impl IntoIterator<Item = (usize, Arc<HpkeKey>)>) -> Vec<(usize, Arc<HpkeKey>)> {
        keys.into_iter()
            .filter(|(x, k)| matches!(tree.nodes.get(*x), Some(Some(n)) if n.enc() == k.public()))
            .collect()
    }

    /// Déballe et vérifie une proposition de l'expéditeur externe ; lit un
    /// seul MLSMessage dans `r` (l'op 27 les met bout à bout).
    pub fn unwrap_proposal(&self, r: &mut Reader) -> R<Queued> {
        if r.u16()? != VERSION || r.u16()? != WF_PUBLIC {
            return Err(Error("message MLS inattendu"));
        }
        let start = r.pos();
        if r.vec()? != self.group_id.as_slice() {
            return Err(Error("proposition d'un autre groupe"));
        }
        if r.u64()? != self.epoch {
            return Err(Error("proposition d'une autre époque"));
        }
        if r.u8()? != SENDER_EXTERNAL {
            return Err(Error("proposition d'un expéditeur non externe"));
        }
        let idx = r.u32()? as usize;
        r.vec()?;
        if r.u8()? != CT_PROPOSAL {
            return Err(Error("pas une proposition"));
        }
        let proposal = Proposal::read(r)?;
        let content = r.since(start);
        let signature = r.vec()?.to_vec();
        let senders = msg::external_senders(&self.exts)?;
        let sender = senders.get(idx).ok_or(Error("expéditeur externe inconnu"))?;
        let tbs = build(|w| {
            w.u16(VERSION).u16(WF_PUBLIC).raw(content);
        });
        if !crypto::verify_with_label(&sender.sig, "FramedContentTBS", &tbs, &signature) {
            return Err(Error("signature de proposition invalide"));
        }
        match &proposal {
            Proposal::Add(kp) if !kp.verify() => return Err(Error("paquet de clés invalide")),
            Proposal::Remove(i) if self.tree.leaf(*i).is_none() => return Err(Error("retrait d'un inconnu")),
            _ => {}
        }
        let authenticated = build(|w| {
            w.u16(WF_PUBLIC).raw(content).vec(&signature);
        });
        Ok(Queued { reference: crypto::ref_hash("MLS 1.0 Proposal Reference", &authenticated), proposal })
    }

    /// Produit un Commit couvrant toutes les propositions en attente.
    pub fn commit(&self, queue: &[Queued]) -> R<CommitOut> {
        let props: Vec<&Proposal> = queue.iter().map(|q| &q.proposal).collect();
        let mut t = self.tree.clone();
        let (joiners, removed) = apply(&mut t, &props)?;
        if t.leaf(self.me).is_none() {
            return Err(Error("le Commit nous retirerait"));
        }
        let me_x = leaf_node(self.me);
        let mut keys: Vec<(usize, Arc<HpkeKey>)> = Vec::new();
        let mut secrets: Vec<(usize, Vec<u8>)> = Vec::new();
        let (commit_secret, path) = if queue.is_empty() || removed {
            let fdp = t.filtered_direct_path(self.me);
            t.blank_path(self.me);
            let mut ps = crypto::random_secret()?;
            for &(p, _) in &fdp {
                let k = HpkeKey::derive(&crypto::derive_secret(&ps, "node"))?;
                t.nodes[p] = Some(Node::Parent(ParentNode { enc: k.public().to_vec(), parent_hash: Vec::new(), unmerged: Vec::new() }));
                keys.push((p, Arc::new(k)));
                secrets.push((p, ps.clone()));
                ps = crypto::derive_secret(&ps, "path");
            }
            let leaf_ph = set_parent_hashes(&mut t, me_x, &fdp)?;
            let leaf_key = HpkeKey::generate()?;
            let mut leaf = self.tree.leaf(self.me).cloned().ok_or(Error("feuille absente"))?;
            leaf.enc = leaf_key.public().to_vec();
            leaf.source = LeafSource::Commit { parent_hash: leaf_ph };
            leaf.sign(&self.sig, &self.group_id, self.me)?;
            t.nodes[me_x] = Some(Node::Leaf(leaf.clone()));
            keys.push((me_x, Arc::new(leaf_key)));
            let prov = context_bytes(&self.group_id, self.epoch.wrapping_add(1), &t.tree_hash(), &self.confirmed, &self.exts);
            let joiner_nodes: Vec<usize> = joiners.iter().map(|(i, _)| leaf_node(*i)).collect();
            let mut nodes = Vec::new();
            for (i, &(_, c)) in fdp.iter().enumerate() {
                let mut cts = Vec::new();
                for y in t.resolution(c) {
                    if joiner_nodes.contains(&y) {
                        continue;
                    }
                    let pk = t.nodes[y].as_ref().map(Node::enc).ok_or(Error("résolution"))?;
                    cts.push(crypto::encrypt_with_label(pk, "UpdatePathNode", &prov, &secrets[i].1)?);
                }
                nodes.push(build(|w| {
                    w.vec(keys[i].1.public()).nested(|w| {
                        for c in &cts {
                            w.raw(c);
                        }
                    });
                }));
            }
            let path = build(|w| {
                leaf.write(w);
                w.nested(|w| {
                    for n in &nodes {
                        w.raw(n);
                    }
                });
            });
            (ps, Some(path))
        } else {
            keys = Self::live_keys(&t, self.keys.iter().cloned());
            (vec![0u8; NH], None)
        };

        let content = build(|w| {
            w.vec(&self.group_id).u64(self.epoch).u8(SENDER_MEMBER).u32(self.me).vec(&[]).u8(CT_COMMIT);
            w.nested(|w| {
                for q in queue {
                    w.u8(2).vec(&q.reference);
                }
            });
            match &path {
                Some(p) => {
                    w.u8(1).raw(p);
                }
                None => {
                    w.u8(0);
                }
            }
        });
        let tbs = build(|w| {
            w.u16(VERSION).u16(WF_PUBLIC).raw(&content).raw(&self.context());
        });
        let signature = self.sig.sign_with_label("FramedContentTBS", &tbs)?;
        let mut cth = self.interim.clone();
        cth.extend(build(|w| {
            w.u16(WF_PUBLIC).raw(&content).vec(&signature);
        }));
        let confirmed = crypto::hash(&cth);
        let ctx = context_bytes(&self.group_id, self.epoch.wrapping_add(1), &t.tree_hash(), &confirmed, &self.exts);
        let s = schedule(&self.init_secret, &commit_secret, &ctx);
        let tag = crypto::mac(&s.confirm, &confirmed);
        let auth = build(|w| {
            w.vec(&signature).vec(&tag);
        });
        let mut tbm = tbs.clone();
        tbm.extend_from_slice(&auth);
        let membership = crypto::mac(&self.membership_key, &tbm);
        let message = build(|w| {
            w.u16(VERSION).u16(WF_PUBLIC).raw(&content).raw(&auth).vec(&membership);
        });

        let welcome = if joiners.is_empty() {
            None
        } else {
            let gi_tbs = build(|w| {
                w.raw(&ctx);
                w.nested(|w| {
                    w.u16(msg::EXT_RATCHET_TREE).vec(&t.write());
                });
                w.vec(&tag).u32(self.me);
            });
            let gi_sig = self.sig.sign_with_label("GroupInfoTBS", &gi_tbs)?;
            let mut gi = gi_tbs;
            gi.extend(build(|w| {
                w.vec(&gi_sig);
            }));
            let (key, nonce) = welcome_key_nonce(&s.welcome);
            let egi = crypto::aead_seal(&key, &nonce, &[], &gi)?;
            let mut entries = Vec::new();
            for (i, kp) in &joiners {
                let anc = common_ancestor(me_x, leaf_node(*i));
                let ps = secrets.iter().find(|(p, _)| *p == anc).map(|(_, s)| s);
                let gs = build(|w| {
                    w.vec(&s.joiner);
                    match ps {
                        Some(ps) => {
                            w.u8(1).vec(ps);
                        }
                        None => {
                            w.u8(0);
                        }
                    }
                    w.nested(|_| {});
                });
                let enc = crypto::encrypt_with_label(&kp.init_key, "Welcome", &egi, &gs)?;
                entries.push(build(|w| {
                    w.vec(&kp.reference()).raw(&enc);
                }));
            }
            Some(build(|w| {
                w.u16(SUITE).nested(|w| {
                    for e in &entries {
                        w.raw(e);
                    }
                });
                w.vec(&egi);
            }))
        };
        let next = self.successor(t, confirmed, &tag, s, keys);
        Ok(CommitOut { message, welcome, next })
    }

    /// Traite le Commit d'un autre membre. `queue` : propositions en attente,
    /// que le Commit doit couvrir exactement (règle DAVE).
    pub fn process_commit(&self, msg: &[u8], queue: &[Queued]) -> R<Group> {
        let (gid, epoch, c) = parse_commit(msg)?;
        if gid != self.group_id || epoch != self.epoch {
            return Err(Error("Commit d'un autre groupe ou d'une autre époque"));
        }
        let mut props = Vec::new();
        for (n, r) in c.refs.iter().enumerate() {
            if c.refs[..n].contains(r) {
                return Err(Error("référence répétée"));
            }
            props.push(&queue.iter().find(|q| &q.reference == r).ok_or(Error("référence inconnue"))?.proposal);
        }
        if props.len() != queue.len() {
            return Err(Error("le Commit ne couvre pas toutes les propositions"));
        }
        let old_leaf = self.tree.leaf(c.sender).ok_or(Error("auteur inconnu"))?;
        let tbs = build(|w| {
            w.u16(VERSION).u16(WF_PUBLIC).raw(c.content).raw(&self.context());
        });
        let auth = build(|w| {
            w.vec(&c.signature).vec(&c.tag);
        });
        let mut tbm = tbs.clone();
        tbm.extend_from_slice(&auth);
        if !crypto::ct_eq(&crypto::mac(&self.membership_key, &tbm), &c.membership) {
            return Err(Error("étiquette d'appartenance invalide"));
        }
        if !crypto::verify_with_label(&old_leaf.sig, "FramedContentTBS", &tbs, &c.signature) {
            return Err(Error("signature du Commit invalide"));
        }

        let mut t = self.tree.clone();
        let (joiners, removed) = apply(&mut t, &props)?;
        if t.leaf(self.me).is_none() {
            return Err(Error("retiré du groupe"));
        }
        if t.leaf(c.sender).is_none() {
            return Err(Error("l'auteur se retire lui-même"));
        }
        let me_x = leaf_node(self.me);
        let sender_x = leaf_node(c.sender);
        let (commit_secret, keys) = match c.path {
            None if props.is_empty() || removed => return Err(Error("chemin requis absent")),
            None => (vec![0u8; NH], Self::live_keys(&t, self.keys.iter().cloned())),
            Some((leaf, nodes)) => {
                if leaf.sig != old_leaf.sig || leaf.identity != old_leaf.identity {
                    return Err(Error("le Commit change l'identité de son auteur"));
                }
                if !matches!(leaf.source, LeafSource::Commit { .. }) || !leaf.verify(&self.group_id, c.sender) {
                    return Err(Error("feuille du chemin invalide"));
                }
                let fdp = t.filtered_direct_path(c.sender);
                if fdp.len() != nodes.len() {
                    return Err(Error("longueur du chemin"));
                }
                let old_keys = Self::live_keys(&t, self.keys.iter().cloned());
                let old_tree = t.clone();
                t.blank_path(c.sender);
                for (&(p, _), n) in fdp.iter().zip(&nodes) {
                    t.nodes[p] = Some(Node::Parent(ParentNode { enc: n.enc.clone(), parent_hash: Vec::new(), unmerged: Vec::new() }));
                }
                let expected = set_parent_hashes(&mut t, sender_x, &fdp)?;
                if leaf.parent_hash() != Some(expected.as_slice()) {
                    return Err(Error("hachage parent du chemin invalide"));
                }
                t.nodes[sender_x] = Some(Node::Leaf(leaf));
                let prov = context_bytes(&self.group_id, self.epoch.wrapping_add(1), &t.tree_hash(), &self.confirmed, &self.exts);
                let joiner_nodes: Vec<usize> = joiners.iter().map(|(i, _)| leaf_node(*i)).collect();
                let i = fdp.iter().position(|&(_, cp)| in_subtree(me_x, cp)).ok_or(Error("hors du chemin"))?;
                let res: Vec<usize> = old_tree.resolution(fdp[i].1).into_iter().filter(|y| !joiner_nodes.contains(y)).collect();
                let (j, key) = res
                    .iter()
                    .enumerate()
                    .find_map(|(j, y)| old_keys.iter().find(|(x, _)| x == y).map(|(_, k)| (j, k.clone())))
                    .ok_or(Error("aucune clé pour le chemin"))?;
                let (kem, ct) = nodes[i].cts.get(j).ok_or(Error("chiffré manquant"))?;
                let mut ps = crypto::decrypt_with_label(&key, "UpdatePathNode", &prov, kem, ct)?;
                let mut fresh = Vec::new();
                for (m, n) in nodes.iter().enumerate().skip(i) {
                    let k = HpkeKey::derive(&crypto::derive_secret(&ps, "node"))?;
                    if k.public() != n.enc.as_slice() {
                        return Err(Error("clé du chemin incohérente"));
                    }
                    fresh.push((fdp[m].0, Arc::new(k)));
                    ps = crypto::derive_secret(&ps, "path");
                }
                (ps, Self::live_keys(&t, old_keys.into_iter().chain(fresh)))
            }
        };
        let mut seen: Vec<u64> = Vec::new();
        for (_, l) in t.leaves() {
            let uid = l.user_id().ok_or(Error("identité invalide"))?;
            if seen.contains(&uid) {
                return Err(Error("identité en double"));
            }
            seen.push(uid);
        }
        let mut cth = self.interim.clone();
        cth.extend(build(|w| {
            w.u16(WF_PUBLIC).raw(c.content).vec(&c.signature);
        }));
        let confirmed = crypto::hash(&cth);
        let ctx = context_bytes(&self.group_id, self.epoch.wrapping_add(1), &t.tree_hash(), &confirmed, &self.exts);
        let s = schedule(&self.init_secret, &commit_secret, &ctx);
        if !crypto::ct_eq(&crypto::mac(&s.confirm, &confirmed), &c.tag) {
            return Err(Error("étiquette de confirmation invalide"));
        }
        Ok(self.successor(t, confirmed, &c.tag, s, keys))
    }

    /// Rejoint un groupe par un Welcome (RFC 9420, 12.4.3.1). `tree` : arbre
    /// fourni à part, sinon l'extension ratchet_tree du GroupInfo.
    pub fn join(
        welcome: &[u8],
        kp: &KeyPackage,
        init_key: &HpkeKey,
        leaf_key: Arc<HpkeKey>,
        sig: Arc<SigKey>,
        tree: Option<&[u8]>,
    ) -> R<Self> {
        let mut r = Reader::new(welcome);
        if r.u16()? != SUITE {
            return Err(Error("suite du Welcome"));
        }
        let my_ref = kp.reference();
        let mut list = r.sub()?;
        let mut mine = None;
        while !list.is_empty() {
            let reference = list.vec()?;
            let kem = list.vec()?;
            let ct = list.vec()?;
            if reference == my_ref.as_slice() {
                mine = Some((kem, ct));
            }
        }
        let egi = r.vec()?;
        r.end()?;
        let (kem, ct) = mine.ok_or(Error("Welcome destiné à un autre"))?;
        let gs = crypto::decrypt_with_label(init_key, "Welcome", egi, kem, ct)?;
        let mut g = Reader::new(&gs);
        let joiner = g.vec()?.to_vec();
        let path_secret = if g.present()? { Some(g.vec()?.to_vec()) } else { None };
        if !g.sub()?.is_empty() {
            return Err(Error("PSK non prises en charge"));
        }
        g.end()?;

        let member = crypto::extract(&joiner, &[0u8; NH]);
        let (key, nonce) = welcome_key_nonce(&crypto::derive_secret(&member, "welcome"));
        let gi = crypto::aead_open(&key, &nonce, &[], egi)?;
        let mut r = Reader::new(&gi);
        let ctx_start = r.pos();
        if r.u16()? != VERSION || r.u16()? != SUITE {
            return Err(Error("contexte : version ou suite"));
        }
        let gid = r.vec()?.to_vec();
        let epoch = r.u64()?;
        let tree_hash = r.vec()?.to_vec();
        let confirmed = r.vec()?.to_vec();
        let ext_start = r.pos();
        r.vec()?;
        let exts = r.since(ext_start).to_vec();
        let ctx = r.since(ctx_start).to_vec();
        let gi_ext_start = r.pos();
        r.vec()?;
        let gi_exts = r.since(gi_ext_start).to_vec();
        let tag = r.vec()?.to_vec();
        let signer = r.u32()?;
        let tbs_end = r.pos();
        let signature = r.vec()?.to_vec();
        r.end()?;

        let inline = msg::find_extension(&gi_exts, msg::EXT_RATCHET_TREE)?;
        let t = Tree::read(tree.or(inline.as_deref()).ok_or(Error("arbre absent du Welcome"))?)?;
        let signer_leaf = t.leaf(signer).ok_or(Error("signataire absent"))?;
        if !crypto::verify_with_label(&signer_leaf.sig, "GroupInfoTBS", &gi[..tbs_end], &signature) {
            return Err(Error("signature du GroupInfo invalide"));
        }
        if t.tree_hash() != tree_hash {
            return Err(Error("hachage de l'arbre"));
        }
        if !t.structure_valid() || !t.parent_hashes_valid() {
            return Err(Error("arbre invalide"));
        }
        if !t.leaves().all(|(i, l)| l.verify(&gid, i)) {
            return Err(Error("feuille mal signée"));
        }
        let me = t.leaves().find(|(_, l)| **l == kp.leaf).map(|(i, _)| i).ok_or(Error("notre feuille est absente"))?;
        if leaf_key.public() != kp.leaf.enc.as_slice() {
            return Err(Error("clé de feuille"));
        }
        let s = from_joiner(joiner, &ctx);
        if !crypto::ct_eq(&crypto::mac(&s.confirm, &confirmed), &tag) {
            return Err(Error("étiquette de confirmation du Welcome"));
        }
        let me_x = leaf_node(me);
        let mut keys = vec![(me_x, leaf_key)];
        if let Some(mut ps) = path_secret {
            let anc = common_ancestor(me_x, leaf_node(signer));
            let fdp = t.filtered_direct_path(signer);
            let start = fdp.iter().position(|&(p, _)| p == anc).ok_or(Error("secret de chemin mal placé"))?;
            for &(p, _) in &fdp[start..] {
                let k = HpkeKey::derive(&crypto::derive_secret(&ps, "node"))?;
                if t.nodes[p].as_ref().map(Node::enc) != Some(k.public()) {
                    return Err(Error("secret de chemin incohérent"));
                }
                keys.push((p, Arc::new(k)));
                ps = crypto::derive_secret(&ps, "path");
            }
        }
        Ok(Group {
            group_id: gid,
            epoch,
            interim: interim_of(&confirmed, &tag),
            tree: t,
            exts,
            tree_hash,
            confirmed,
            init_secret: s.init,
            exporter_secret: s.exporter,
            membership_key: s.membership,
            authenticator: s.authentication,
            me,
            keys,
            sig,
        })
    }
}

/// Longueur du MLSMessage de Commit en tête de `buf` (l'op 28 y colle le Welcome).
#[cfg(test)]
pub fn commit_len(buf: &[u8]) -> R<usize> {
    let mut r = open_public(buf)?;
    read_commit(&mut r)?;
    Ok(r.pos())
}

#[cfg(test)]
pub(crate) mod tests;
