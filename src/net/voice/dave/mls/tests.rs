//! Vecteurs d'interopérabilité MLS (github.com/mlswg/mls-implementations,
//! suite 2 seulement, dans tests/fixtures/dave/) et scénarios de groupe.

use std::sync::Arc;

use serde_json::Value;

use super::super::codec::{Reader, build};
use super::super::crypto::{self, HpkeKey, NH, SigKey};
use super::msg::{CT_PROPOSAL, ExternalSender, KeyPackage, LeafNode, LeafSource, Proposal, SENDER_EXTERNAL, VERSION, WF_PUBLIC, default_capabilities};
use super::*;

pub fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn h(v: &Value, k: &str) -> Vec<u8> {
    hex(v[k].as_str().unwrap())
}

fn fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/dave/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn vecteurs_crypto_de_base() {
    for v in fixture("mls-crypto-basics.json").as_array().unwrap() {
        let e = &v["expand_with_label"];
        assert_eq!(
            crypto::expand_with_label(&h(e, "secret"), e["label"].as_str().unwrap(), &h(e, "context"), e["length"].as_u64().unwrap() as usize),
            h(e, "out")
        );
        let d = &v["derive_secret"];
        assert_eq!(crypto::derive_secret(&h(d, "secret"), d["label"].as_str().unwrap()), h(d, "out"));
        let t = &v["derive_tree_secret"];
        let generation = t["generation"].as_u64().unwrap() as u32;
        assert_eq!(
            crypto::expand_with_label(&h(t, "secret"), t["label"].as_str().unwrap(), &generation.to_be_bytes(), t["length"].as_u64().unwrap() as usize),
            h(t, "out")
        );
        let r = &v["ref_hash"];
        assert_eq!(crypto::ref_hash(r["label"].as_str().unwrap(), &h(r, "value")), h(r, "out"));
        let s = &v["sign_with_label"];
        assert!(crypto::verify_with_label(&h(s, "pub"), s["label"].as_str().unwrap(), &h(s, "content"), &h(s, "signature")));
        let key = SigKey::from_raw(&h(s, "priv"), &h(s, "pub")).unwrap();
        let sig = key.sign_with_label("x", b"y").unwrap();
        assert!(crypto::verify_with_label(&h(s, "pub"), "x", b"y", &sig));
        let c = &v["encrypt_with_label"];
        let sk = HpkeKey::from_scalar(&h(c, "priv")).unwrap();
        assert_eq!(sk.public(), h(c, "pub").as_slice());
        let pt = crypto::decrypt_with_label(&sk, c["label"].as_str().unwrap(), &h(c, "context"), &h(c, "kem_output"), &h(c, "ciphertext")).unwrap();
        assert_eq!(pt, h(c, "plaintext"));
    }
}

#[test]
fn vecteurs_calendrier_de_cles() {
    for v in fixture("mls-key-schedule.json").as_array().unwrap() {
        let mut init = h(v, "initial_init_secret");
        for e in v["epochs"].as_array().unwrap() {
            let ctx = h(e, "group_context");
            let pre = crypto::extract(&init, &h(e, "commit_secret"));
            let joiner = crypto::expand_with_label(&pre, "joiner", &ctx, NH);
            assert_eq!(joiner, h(e, "joiner_secret"));
            let s = from_joiner_psk(joiner, &h(e, "psk_secret"), &ctx);
            assert_eq!(s.welcome, h(e, "welcome_secret"));
            assert_eq!(s.exporter, h(e, "exporter_secret"));
            assert_eq!(s.membership, h(e, "membership_key"));
            assert_eq!(s.confirm, h(e, "confirmation_key"));
            assert_eq!(s.authentication, h(e, "epoch_authenticator"));
            assert_eq!(s.init, h(e, "init_secret"));
            let x = &e["exporter"];
            // L'étiquette de ces vecteurs s'emploie telle quelle (texte hexadécimal).
            let root = crypto::expand_with_label_bytes(&s.exporter, x["label"].as_str().unwrap().as_bytes(), &[], NH);
            assert_eq!(export_with_root(&root, &h(x, "context"), x["length"].as_u64().unwrap() as usize), h(x, "secret"));
            init = s.init;
        }
    }
}

/// Welcome des vecteurs : déchiffrement des secrets et du GroupInfo, signature.
#[test]
fn vecteur_welcome() {
    for v in fixture("mls-welcome.json").as_array().unwrap() {
        let kp = KeyPackage::read(&mut Reader::new(&h(v, "key_package")[4..])).unwrap();
        assert!(kp.verify());
        let init = HpkeKey::from_scalar(&h(v, "init_priv")).unwrap();
        assert_eq!(init.public(), kp.init_key.as_slice());
        let welcome = h(v, "welcome");
        let mut r = Reader::new(&welcome[4..]);
        assert_eq!(r.u16().unwrap(), msg::SUITE);
        let mut list = r.sub().unwrap();
        let reference = list.vec().unwrap();
        assert_eq!(reference, kp.reference().as_slice());
        let (kem, ct) = (list.vec().unwrap(), list.vec().unwrap());
        let egi = r.vec().unwrap();
        let gs = crypto::decrypt_with_label(&init, "Welcome", egi, kem, ct).unwrap();
        let joiner = Reader::new(&gs).vec().unwrap().to_vec();
        let member = crypto::extract(&joiner, &[0u8; NH]);
        let (key, nonce) = welcome_key_nonce(&crypto::derive_secret(&member, "welcome"));
        let gi = crypto::aead_open(&key, &nonce, &[], egi).unwrap();
        let mut g = Reader::new(&gi);
        let start = g.pos();
        g.u16().unwrap();
        g.u16().unwrap();
        g.vec().unwrap();
        g.u64().unwrap();
        g.vec().unwrap();
        let confirmed = g.vec().unwrap().to_vec();
        g.vec().unwrap();
        let ctx = g.since(start).to_vec();
        g.vec().unwrap();
        let tag = g.vec().unwrap().to_vec();
        g.u32().unwrap();
        let tbs_end = g.pos();
        let signature = g.vec().unwrap();
        assert!(crypto::verify_with_label(&h(v, "signer_pub"), "GroupInfoTBS", &gi[..tbs_end], signature));
        let s = from_joiner(joiner, &ctx);
        assert_eq!(crypto::mac(&s.confirm, &confirmed), tag);
    }
}

/// Client passif rejoignant par Welcome : l'authentifiant d'époque doit
/// correspondre (arbre en ligne et arbre fourni à part).
#[test]
fn vecteurs_client_passif_welcome() {
    let cases = fixture("mls-passive-welcome.json");
    assert!(!cases.as_array().unwrap().is_empty());
    for v in cases.as_array().unwrap() {
        let kp = KeyPackage::read(&mut Reader::new(&h(v, "key_package")[4..])).unwrap();
        let init = HpkeKey::from_scalar(&h(v, "init_priv")).unwrap();
        let leaf = Arc::new(HpkeKey::from_scalar(&h(v, "encryption_priv")).unwrap());
        let sig = Arc::new(SigKey::from_raw(&h(v, "signature_priv"), &kp.leaf.sig).unwrap());
        let tree = v["ratchet_tree"].as_str().map(hex);
        let g = Group::join(&h(v, "welcome")[4..], &kp, &init, leaf, sig, tree.as_deref());
        let g = g.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(g.authenticator, h(v, "initial_epoch_authenticator"));
    }
}

// ---------------------------------------------------------------- scénarios

/// Passerelle simulée : l'expéditeur externe qui signe les propositions.
pub struct ExtSim {
    key: SigKey,
    pub sender: ExternalSender,
}

impl ExtSim {
    pub fn new() -> Self {
        let key = SigKey::generate().unwrap();
        let sender = ExternalSender { sig: key.public().to_vec(), identity: b"voice-gateway".to_vec() };
        ExtSim { key, sender }
    }

    pub fn encoded_sender(&self) -> Vec<u8> {
        build(|w| self.sender.write(w))
    }

    /// MLSMessage d'une proposition externe.
    pub fn propose(&self, gid: &[u8], epoch: u64, p: &Proposal) -> Vec<u8> {
        let content = build(|w| {
            w.vec(gid).u64(epoch).u8(SENDER_EXTERNAL).u32(0).vec(&[]).u8(CT_PROPOSAL);
            p.write(w);
        });
        let tbs = build(|w| {
            w.u16(VERSION).u16(WF_PUBLIC).raw(&content);
        });
        let sig = self.key.sign_with_label("FramedContentTBS", &tbs).unwrap();
        build(|w| {
            w.u16(VERSION).u16(WF_PUBLIC).raw(&content).vec(&sig);
        })
    }
}

pub struct Member {
    sig: Arc<SigKey>,
    leaf_key: Arc<HpkeKey>,
    init: HpkeKey,
    pub kp: KeyPackage,
}

impl Member {
    pub fn new(uid: u64) -> Self {
        let sig = Arc::new(SigKey::generate().unwrap());
        let leaf_key = Arc::new(HpkeKey::generate().unwrap());
        let mut leaf = LeafNode {
            enc: leaf_key.public().to_vec(),
            sig: sig.public().to_vec(),
            identity: uid.to_be_bytes().to_vec(),
            caps: default_capabilities(),
            source: LeafSource::KeyPackage { not_before: 0, not_after: u64::MAX },
            exts: vec![0],
            signature: Vec::new(),
        };
        leaf.sign(&sig, &[], 0).unwrap();
        let init = HpkeKey::generate().unwrap();
        let kp = KeyPackage::create(init.public(), leaf, &sig).unwrap();
        Member { sig, leaf_key, init, kp }
    }

    fn create(&self, gid: &[u8], ext: &ExtSim) -> Group {
        Group::create(gid.to_vec(), self.kp.leaf.clone(), self.leaf_key.clone(), self.sig.clone(), ext.sender.group_extensions()).unwrap()
    }

    fn join(&self, welcome: &[u8]) -> Group {
        Group::join(welcome, &self.kp, &self.init, self.leaf_key.clone(), self.sig.clone(), None).unwrap()
    }
}

fn queue(g: &Group, msgs: &[Vec<u8>]) -> Vec<Queued> {
    msgs.iter().map(|m| g.unwrap_proposal(&mut Reader::new(m)).unwrap()).collect()
}

fn same(groups: &[&Group]) {
    for g in groups {
        assert_eq!(g.authenticator, groups[0].authenticator);
        assert_eq!(g.epoch, groups[0].epoch);
        assert_eq!(g.export("Discord Secure Frames v0", &7u64.to_le_bytes(), 16), groups[0].export("Discord Secure Frames v0", &7u64.to_le_bytes(), 16));
        assert_eq!(g.tree.tree_hash(), groups[0].tree.tree_hash());
    }
}

#[test]
fn groupe_de_deux_puis_trois_ajout_retrait() {
    let gid = 99u64.to_be_bytes();
    let ext = ExtSim::new();
    let (a, b, c) = (Member::new(1), Member::new(2), Member::new(3));

    // A crée son groupe local ; la passerelle propose d'ajouter B.
    let ga0 = a.create(&gid, &ext);
    let add_b = ext.propose(&gid, 0, &Proposal::Add(Box::new(b.kp.clone())));
    let q = queue(&ga0, std::slice::from_ref(&add_b));
    let out = ga0.commit(&q).unwrap();
    let ga1 = out.next;
    let gb1 = b.join(&out.welcome.unwrap());
    assert_eq!(gb1.me, 1);
    same(&[&ga1, &gb1]);
    assert_eq!(ga1.roster().len(), 2);

    // Époque 1 -> 2 : ajout de C, validé par B.
    let add_c = ext.propose(&gid, 1, &Proposal::Add(Box::new(c.kp.clone())));
    let qb = queue(&gb1, std::slice::from_ref(&add_c));
    let out = gb1.commit(&qb).unwrap();
    let qa = queue(&ga1, std::slice::from_ref(&add_c));
    let ga2 = ga1.process_commit(&out.message, &qa).unwrap();
    let gc2 = c.join(&out.welcome.unwrap());
    let gb2 = out.next;
    same(&[&ga2, &gb2, &gc2]);
    assert_eq!(ga2.roster().len(), 3);

    // Commit altéré : refusé.
    let rm_b = ext.propose(&gid, 2, &Proposal::Remove(1));
    let qc = queue(&gc2, std::slice::from_ref(&rm_b));
    let out = gc2.commit(&qc).unwrap();
    let mut bad = out.message.clone();
    let n = bad.len();
    bad[n - 40] ^= 1;
    let qa = queue(&ga2, std::slice::from_ref(&rm_b));
    assert!(ga2.process_commit(&bad, &qa).is_err());
    // Commit qui ne couvre pas la file : refusé.
    assert!(ga2.process_commit(&out.message, &[]).is_err());

    // Retrait de B (chemin obligatoire), validé par C et suivi par A.
    let ga3 = ga2.process_commit(&out.message, &qa).unwrap();
    let gc3 = out.next;
    same(&[&ga3, &gc3]);
    assert_eq!(ga3.roster().iter().map(|(u, _)| *u).collect::<Vec<_>>(), vec![1, 3]);
    assert_ne!(ga3.authenticator, ga2.authenticator);
    // B retiré ne peut pas suivre.
    let qb = queue(&gb2, std::slice::from_ref(&rm_b));
    assert!(gb2.process_commit(&out.message, &qb).is_err());

    // Époque 3 -> 4 : B revient avec un nouveau paquet, A valide.
    let b2 = Member::new(2);
    let add_b2 = ext.propose(&gid, 3, &Proposal::Add(Box::new(b2.kp.clone())));
    let out = ga3.commit(&queue(&ga3, std::slice::from_ref(&add_b2))).unwrap();
    let gc4 = gc3.process_commit(&out.message, &queue(&gc3, std::slice::from_ref(&add_b2))).unwrap();
    let gb4 = b2.join(&out.welcome.unwrap());
    same(&[&out.next, &gc4, &gb4]);
}

#[test]
fn proposition_mal_signee_ou_hors_epoque() {
    let gid = 5u64.to_be_bytes();
    let ext = ExtSim::new();
    let other = ExtSim::new();
    let a = Member::new(1);
    let g = a.create(&gid, &ext);
    let p = Proposal::Add(Box::new(Member::new(2).kp));
    assert!(g.unwrap_proposal(&mut Reader::new(&other.propose(&gid, 0, &p))).is_err());
    assert!(g.unwrap_proposal(&mut Reader::new(&ext.propose(&gid, 1, &p))).is_err());
    assert!(g.unwrap_proposal(&mut Reader::new(&ext.propose(&6u64.to_be_bytes(), 0, &p))).is_err());
    assert!(g.unwrap_proposal(&mut Reader::new(&ext.propose(&gid, 0, &p))).is_ok());
    // Retrait d'une feuille vide.
    assert!(g.unwrap_proposal(&mut Reader::new(&ext.propose(&gid, 0, &Proposal::Remove(3)))).is_err());
}

#[test]
fn grand_groupe_et_arbre_tronque() {
    let gid = 8u64.to_be_bytes();
    let ext = ExtSim::new();
    let a = Member::new(1);
    let others: Vec<Member> = (2..=6).map(Member::new).collect();
    let g0 = a.create(&gid, &ext);
    let msgs: Vec<Vec<u8>> = others.iter().map(|m| ext.propose(&gid, 0, &Proposal::Add(Box::new(m.kp.clone())))).collect();
    let out = g0.commit(&queue(&g0, &msgs)).unwrap();
    let welcome = out.welcome.unwrap();
    let mut groups: Vec<Group> = others.iter().map(|m| m.join(&welcome)).collect();
    groups.push(out.next);
    same(&groups.iter().collect::<Vec<_>>());
    assert_eq!(groups[0].tree.n_leaves(), 8);
    // Retirer les feuilles 4 et 5 : l'arbre redescend à 4 feuilles.
    let rms = [ext.propose(&gid, 1, &Proposal::Remove(4)), ext.propose(&gid, 1, &Proposal::Remove(5))];
    let committer = groups.pop().unwrap();
    let out = committer.commit(&queue(&committer, &rms)).unwrap();
    let mut next = vec![out.next];
    for g in &groups[..3] {
        next.push(g.process_commit(&out.message, &queue(g, &rms)).unwrap());
    }
    same(&next.iter().collect::<Vec<_>>());
    assert_eq!(next[0].tree.n_leaves(), 4);

    // Retrait et ajout dans le même Commit : le Welcome porte un secret de chemin.
    let newcomer = Member::new(7);
    let props = [ext.propose(&gid, 2, &Proposal::Remove(2)), ext.propose(&gid, 2, &Proposal::Add(Box::new(newcomer.kp.clone())))];
    let out = next[0].commit(&queue(&next[0], &props)).unwrap();
    let g1 = next[1].process_commit(&out.message, &queue(&next[1], &props)).unwrap();
    let g3 = next[3].process_commit(&out.message, &queue(&next[3], &props)).unwrap();
    assert!(next[2].process_commit(&out.message, &queue(&next[2], &props)).is_err());
    let g7 = newcomer.join(&out.welcome.unwrap());
    assert_eq!(g7.me, 2);
    assert!(g7.keys.len() > 1, "le secret de chemin donne des clés de nœuds parents");
    same(&[&out.next, &g1, &g3, &g7]);
}
