//! Sessions DAVE face à une passerelle vocale simulée (expéditeur externe,
//! choix du premier Commit, transitions), en mémoire puis sur une vraie
//! WebSocket 127.0.0.1 avec le format binaire des ops.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use super::mls::msg::Proposal;
use super::mls::tests::ExtSim;
use super::*;

/// Message de la passerelle vers un client.
#[derive(Clone, Debug)]
pub enum ToClient {
    Json(Value),
    Bin(u8, Vec<u8>),
}

/// Proposition en vol : (ajouté, retiré).
type Pending = (Option<u64>, Option<u64>);

/// Passerelle vocale réduite à ce que DAVE lui demande (protocol.md).
pub struct Gateway {
    ext: ExtSim,
    gid: [u8; 8],
    connected: Vec<u64>,
    established: bool,
    epoch: u64,
    members: Vec<u64>,
    /// Paquets de clés reçus, en attente d'ajout.
    kps: HashMap<u64, Vec<u8>>,
    /// Propositions d'ajout en vol avant l'établissement : (auteur, message).
    initial: Vec<(u64, Vec<u8>)>,
    /// Propositions envoyées à chaque client pendant l'époque : (ajouté ?, message).
    queued: HashMap<u64, Vec<Pending>>,
    commit_taken: bool,
    tid: u16,
    waiting: HashSet<u64>,
    ready: HashSet<u64>,
    pub invalid_reports: usize,
}

impl Gateway {
    pub fn new(channel: u64) -> Self {
        Gateway {
            ext: ExtSim::new(),
            gid: channel.to_be_bytes(),
            connected: Vec::new(),
            established: false,
            epoch: 0,
            members: Vec::new(),
            kps: HashMap::new(),
            initial: Vec::new(),
            queued: HashMap::new(),
            commit_taken: false,
            tid: 0,
            waiting: HashSet::new(),
            ready: HashSet::new(),
            invalid_reports: 0,
        }
    }

    pub fn connect(&mut self, uid: u64) -> Vec<(u64, ToClient)> {
        self.connected.push(uid);
        let ids: Vec<String> = self.connected.iter().map(u64::to_string).collect();
        let mut out: Vec<(u64, ToClient)> =
            self.connected.iter().map(|&c| (c, ToClient::Json(json!({ "op": 11, "d": { "user_ids": ids } })))).collect();
        out.push((uid, ToClient::Json(json!({ "op": 4, "d": { "dave_protocol_version": 1, "mode": "aead_aes256_gcm_rtpsize" } }))));
        out.push((uid, ToClient::Bin(25, self.ext.encoded_sender())));
        out
    }

    /// Déconnexion : retrait proposé avec l'indice de feuille du partant.
    pub fn disconnect(&mut self, uid: u64, leaf: u32) -> Vec<(u64, ToClient)> {
        self.connected.retain(|&c| c != uid);
        self.members.retain(|&c| c != uid);
        let mut out: Vec<(u64, ToClient)> =
            self.connected.iter().map(|&c| (c, ToClient::Json(json!({ "op": 13, "d": { "user_id": uid.to_string() } })))).collect();
        let msg = self.ext.propose(&self.gid, self.epoch, &Proposal::Remove(leaf));
        for &m in &self.members {
            self.queued.entry(m).or_default().push((None, Some(uid)));
            out.push((m, proposals(std::slice::from_ref(&msg))));
        }
        out
    }

    /// Op venu d'un client, déjà découpé comme sur le fil.
    pub fn handle_client(&mut self, uid: u64, msg: &Outgoing) -> Vec<(u64, ToClient)> {
        let mut out = Vec::new();
        match msg {
            Outgoing::Binary(26, kp) => {
                self.kps.insert(uid, kp.clone());
                let kp_parsed = KeyPackage::read(&mut Reader::new(kp)).expect("paquet de clés valide");
                let msg = self.ext.propose(&self.gid, self.epoch, &Proposal::Add(Box::new(kp_parsed)));
                if self.established {
                    for &m in &self.members {
                        self.queued.entry(m).or_default().push((Some(uid), None));
                        out.push((m, proposals(std::slice::from_ref(&msg))));
                    }
                } else {
                    for (author, m) in &self.initial {
                        self.queued.entry(uid).or_default().push((Some(*author), None));
                        out.push((uid, proposals(std::slice::from_ref(m))));
                    }
                    for (author, _) in self.initial.clone() {
                        self.queued.entry(author).or_default().push((Some(uid), None));
                        out.push((author, proposals(std::slice::from_ref(&msg))));
                    }
                    self.initial.push((uid, msg));
                }
            }
            Outgoing::Binary(28, payload) => {
                if self.commit_taken {
                    return out;
                }
                self.commit_taken = true;
                let n = mls::commit_len(payload).expect("Commit lisible");
                let (commit, welcome) = payload.split_at(n);
                self.tid += 1;
                let mut tid_commit = self.tid.to_be_bytes().to_vec();
                tid_commit.extend_from_slice(commit);
                let queue = self.queued.remove(&uid).unwrap_or_default();
                let added: Vec<u64> = queue.iter().filter_map(|(a, _)| *a).collect();
                let removed: Vec<u64> = queue.iter().filter_map(|(_, r)| *r).collect();
                let mut recipients = self.members.clone();
                if !recipients.contains(&uid) {
                    recipients.push(uid);
                }
                for &m in &recipients {
                    out.push((m, ToClient::Bin(29, tid_commit.clone())));
                }
                for &a in &added {
                    let mut w = self.tid.to_be_bytes().to_vec();
                    w.extend_from_slice(welcome);
                    out.push((a, ToClient::Bin(30, w)));
                    self.kps.remove(&a);
                }
                let mut members: Vec<u64> = recipients.into_iter().filter(|m| !removed.contains(m)).collect();
                members.extend(added);
                self.members = members;
                self.established = true;
                self.epoch += 1;
                self.queued.clear();
                self.initial.clear();
                self.waiting = self.members.iter().copied().collect();
                self.ready.clear();
            }
            Outgoing::Json(v) => match v["op"].as_u64() {
                Some(23) => {
                    self.ready.insert(uid);
                    if self.waiting.is_subset(&self.ready) && !self.waiting.is_empty() {
                        self.waiting.clear();
                        self.commit_taken = false;
                        let tid = v["d"]["transition_id"].clone();
                        for &m in &self.members {
                            out.push((m, ToClient::Json(json!({ "op": 22, "d": { "transition_id": tid } }))));
                        }
                    }
                }
                Some(31) => self.invalid_reports += 1,
                _ => {}
            },
            _ => {}
        }
        out
    }
}

fn proposals(msgs: &[Vec<u8>]) -> ToClient {
    let mut p = vec![0u8];
    let body: Vec<u8> = msgs.concat();
    p.extend(codec::build(|w| {
        w.vec(&body);
    }));
    ToClient::Bin(27, p)
}

/// Trame binaire serveur -> client : numéro de séquence, opcode, charge.
pub fn encode_server(seq: u16, op: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = seq.to_be_bytes().to_vec();
    v.push(op);
    v.extend_from_slice(payload);
    v
}

pub fn decode_server(frame: &[u8]) -> Option<(u16, u8, &[u8])> {
    let (head, payload) = frame.split_at_checked(3)?;
    Some((u16::from_be_bytes([head[0], head[1]]), head[2], payload))
}

/// Client -> serveur : opcode puis charge, sans séquence.
pub fn encode_client(op: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = vec![op];
    v.extend_from_slice(payload);
    v
}

/// Banc en mémoire : passerelle + sessions, messages passés par le format du fil.
struct Bench {
    gw: Gateway,
    sessions: Vec<(u64, DaveSession)>,
    seq: u16,
}

impl Bench {
    fn new() -> Self {
        Bench { gw: Gateway::new(4242), sessions: Vec::new(), seq: 0 }
    }

    fn s(&mut self, uid: u64) -> &mut DaveSession {
        &mut self.sessions.iter_mut().find(|(u, _)| *u == uid).unwrap().1
    }

    fn pump(&mut self, mut pending: Vec<(u64, ToClient)>) {
        let mut guard = 0;
        while !pending.is_empty() {
            guard += 1;
            assert!(guard < 1000, "boucle de messages");
            let mut next = Vec::new();
            for (uid, msg) in pending {
                let Some(i) = self.sessions.iter().position(|(u, _)| *u == uid) else { continue };
                let mut out = Vec::new();
                match msg {
                    ToClient::Json(v) => {
                        let op = v["op"].as_u64().unwrap() as u8;
                        if op == 11 {
                            let ids: Vec<u64> = v["d"]["user_ids"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().parse().unwrap()).collect();
                            self.sessions[i].1.on_users(&ids);
                        } else {
                            self.sessions[i].1.on_op(op, Some(&v), None, &mut out);
                        }
                    }
                    ToClient::Bin(op, payload) => {
                        self.seq += 1;
                        let wire = encode_server(self.seq, op, &payload);
                        let (_, op, payload) = decode_server(&wire).unwrap();
                        self.sessions[i].1.on_op(op, None, Some(payload), &mut out);
                    }
                }
                for o in out {
                    if let Outgoing::Binary(op, p) = &o {
                        let wire = encode_client(*op, p);
                        assert_eq!(wire[0], *op);
                    }
                    next.extend(self.gw.handle_client(uid, &o));
                }
            }
            pending = next;
        }
    }

    fn connect(&mut self, uid: u64) {
        self.sessions.push((uid, DaveSession::new(uid, 4242).unwrap()));
        let msgs = self.gw.connect(uid);
        self.pump(msgs);
    }

    /// Rend la session partie (elle garde ses anciennes clés).
    fn disconnect(&mut self, uid: u64) -> DaveSession {
        let leaf = self.s(uid).group.as_ref().unwrap().me;
        let i = self.sessions.iter().position(|(u, _)| *u == uid).unwrap();
        let gone = self.sessions.remove(i).1;
        let msgs = self.gw.disconnect(uid, leaf);
        self.pump(msgs);
        gone
    }

    fn talk(&mut self, from: u64, to: u64, opus: &[u8]) -> Option<Vec<u8>> {
        let frame = self.s(from).encrypt(opus)?;
        assert_ne!(frame.as_slice(), opus, "trame partie en clair");
        self.s(to).decrypt(from, &frame)
    }
}

#[test]
fn deux_puis_trois_membres_ajout_retrait() {
    let mut b = Bench::new();
    b.connect(1);
    assert_eq!(b.s(1).protocol_version(), 1);
    assert!(!b.s(1).is_encrypting(), "seul, pas encore de groupe établi");
    assert_eq!(b.s(1).encrypt(b"x"), None, "rien ne part en clair sous DAVE");

    b.connect(2);
    assert!(b.s(1).is_encrypting() && b.s(2).is_encrypting());
    assert_eq!(b.s(1).epoch(), 1);
    assert_eq!(b.talk(1, 2, b"bonjour").as_deref(), Some(&b"bonjour"[..]));
    assert_eq!(b.talk(2, 1, b"salut").as_deref(), Some(&b"salut"[..]));
    let code_12 = b.s(1).verification_code(2).unwrap();
    assert_eq!(code_12.len(), 45);
    assert_eq!(Some(code_12), b.s(2).verification_code(1));
    let call = b.s(1).verification_code(1).unwrap();
    assert_eq!(call.len(), 30);
    assert_eq!(Some(call.clone()), b.s(2).call_code());

    // Une trame de l'époque 1 encore en vol quand C arrive.
    let in_flight = b.s(1).encrypt(b"en vol").unwrap();
    b.connect(3);
    assert_eq!(b.s(3).epoch(), 2);
    assert_eq!(b.s(1).epoch(), 2);
    assert_eq!(b.s(2).decrypt(1, &in_flight).as_deref(), Some(&b"en vol"[..]));
    for (x, y) in [(1, 3), (3, 2), (2, 1)] {
        assert_eq!(b.talk(x, y, b"a trois").as_deref(), Some(&b"a trois"[..]), "{x} -> {y}");
    }
    assert_ne!(b.s(3).call_code(), Some(call));
    assert_eq!(b.s(3).call_code(), b.s(1).call_code());

    // Trame altérée et silence du SFU.
    let mut f = b.s(1).encrypt(b"intact").unwrap();
    f[1] ^= 4;
    assert_eq!(b.s(2).decrypt(1, &f), None);
    assert_eq!(b.s(2).decrypt(1, &frame::SILENCE).as_deref(), Some(&frame::SILENCE[..]));

    // B part : A et C passent à l'époque 3.
    let mut gone = b.disconnect(2);
    assert_eq!(b.s(1).epoch(), 3);
    assert_eq!(b.talk(1, 3, b"sans B").as_deref(), Some(&b"sans B"[..]));
    let after = b.s(1).encrypt(b"secret").unwrap();
    assert_eq!(gone.decrypt(1, &after), None, "l'ancien membre ne lit plus");
    assert_eq!(b.gw.invalid_reports, 0);
}

#[test]
fn passage_au_chiffrement_de_transport_seul() {
    let mut b = Bench::new();
    b.connect(1);
    b.connect(2);
    let mut out = Vec::new();
    for uid in [1, 2] {
        b.s(uid).on_op(21, Some(&json!({ "op": 21, "d": { "transition_id": 9, "protocol_version": 0 } })), None, &mut out);
        assert_eq!(out.pop(), Some(Outgoing::Json(json!({ "op": 23, "d": { "transition_id": 9 } }))));
        // Prêt à recevoir en clair avant même l'exécution.
        assert_eq!(b.s(uid).decrypt(3, b"opus clair").as_deref(), Some(&b"opus clair"[..]));
    }
    // Encore chiffré tant que la transition n'est pas exécutée.
    assert_ne!(b.s(1).encrypt(b"x").unwrap(), b"x");
    for uid in [1, 2] {
        b.s(uid).on_op(22, Some(&json!({ "transition_id": 9 })), None, &mut out);
    }
    assert_eq!(b.s(1).protocol_version(), 0);
    assert_eq!(b.s(1).encrypt(b"x").as_deref(), Some(&b"x"[..]));
    assert_eq!(b.s(2).decrypt(1, b"x").as_deref(), Some(&b"x"[..]));
}

#[test]
fn commit_illisible_signale_et_relance() {
    let mut b = Bench::new();
    b.connect(1);
    b.connect(2);
    let mut out = Vec::new();
    let mut bogus = 7u16.to_be_bytes().to_vec();
    bogus.extend_from_slice(&[0, 1, 0, 1, 1, 2, 3]);
    b.s(1).on_op(29, None, Some(&bogus), &mut out);
    assert_eq!(out[0], Outgoing::Json(json!({ "op": 31, "d": { "transition_id": 7 } })));
    assert!(matches!(out[1], Outgoing::Binary(26, _)), "nouveau paquet de clés");
    assert_eq!(b.s(1).epoch(), 0);
    // Un Welcome illisible aussi.
    let mut out = Vec::new();
    b.s(1).on_op(30, None, Some(&[0, 8, 0, 2, 0, 0]), &mut out);
    assert_eq!(out[0], Outgoing::Json(json!({ "op": 31, "d": { "transition_id": 8 } })));
}

#[test]
fn ajout_d_un_inconnu_refuse() {
    let mut b = Bench::new();
    b.connect(1);
    b.connect(2);
    // La passerelle propose un utilisateur jamais annoncé (op 11) : pas de Commit.
    let intruder = super::mls::tests::Member::new(666);
    let msg = b.gw.ext.propose(&b.gw.gid, 1, &Proposal::Add(Box::new(intruder.kp.clone())));
    let ToClient::Bin(op, payload) = proposals(&[msg]) else { unreachable!() };
    let mut out = Vec::new();
    b.s(1).on_op(op, None, Some(&payload), &mut out);
    assert!(out.is_empty());
}

#[test]
fn ops_json_complets_ou_reduits_a_d() {
    let mut s = DaveSession::new(1, 2).unwrap();
    let mut out = Vec::new();
    s.on_op(4, Some(&json!({ "dave_protocol_version": 1 })), None, &mut out);
    assert!(matches!(out.as_slice(), [Outgoing::Binary(26, _)]));
    assert_eq!(s.max_protocol_version(), 1);
    // Op binaire tronqué : ignoré sans panique.
    s.on_op(27, None, Some(&[0, 0x40]), &mut out);
    s.on_op(29, None, Some(&[1]), &mut out);
    s.on_op(25, None, Some(&[]), &mut out);
}

// ---------------------------------------------------------------- WebSocket 127.0.0.1

/// Bout en bout sur une vraie WebSocket locale : la passerelle simulée parle
/// le format du fil (texte JSON, binaire séquence + opcode) à deux clients
/// qui n'ont que `FrameCrypto` ; un tiers relaie une trame Opus chiffrée.
#[test]
fn bout_en_bout_websocket_locale() {
    use futures_util::{SinkExt, StreamExt};
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;
    use tokio_tungstenite::tungstenite::Message;

    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    rt.block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let gw = Arc::new(StdMutex::new(Gateway::new(4242)));
        let routes: Routes = Arc::default();

        type Routes = Arc<StdMutex<HashMap<u64, mpsc::UnboundedSender<Message>>>>;
        fn route(routes: &Routes, msgs: Vec<(u64, ToClient)>, seq: &StdMutex<u16>) {
            let r = routes.lock().unwrap();
            let mut seq = seq.lock().unwrap();
            for (uid, m) in msgs {
                let Some(tx) = r.get(&uid) else { continue };
                let msg = match m {
                    ToClient::Json(v) => Message::Text(v.to_string().into()),
                    ToClient::Bin(op, p) => {
                        *seq += 1;
                        Message::Binary(encode_server(*seq, op, &p).into())
                    }
                };
                let _ = tx.send(msg);
            }
        }

        // Serveur : une tâche par client, la passerelle partagée.
        {
            let (gw, routes) = (gw.clone(), routes.clone());
            tokio::spawn(async move {
                let seq_shared = Arc::new(StdMutex::new(0u16));
                loop {
                    let Ok((sock, _)) = listener.accept().await else { return };
                    let (gw, routes, seq) = (gw.clone(), routes.clone(), seq_shared.clone());
                    tokio::spawn(async move {
                        let ws = tokio_tungstenite::accept_async(sock).await.unwrap();
                        let (mut tx, mut rx) = ws.split();
                        // Premier message : identifiant de l'utilisateur.
                        let Some(Ok(Message::Text(hello))) = rx.next().await else { return };
                        let uid: u64 = hello.parse().unwrap();
                        let (otx, mut orx) = mpsc::unbounded_channel::<Message>();
                        routes.lock().unwrap().insert(uid, otx);
                        tokio::spawn(async move {
                            while let Some(m) = orx.recv().await {
                                if tx.send(m).await.is_err() {
                                    return;
                                }
                            }
                        });
                        let msgs = gw.lock().unwrap().connect(uid);
                        route(&routes, msgs, &seq);
                        while let Some(Ok(m)) = rx.next().await {
                            let out = match m {
                                Message::Text(t) => Outgoing::Json(serde_json::from_str(&t).unwrap()),
                                Message::Binary(b) => Outgoing::Binary(b[0], b[1..].to_vec()),
                                _ => continue,
                            };
                            let msgs = gw.lock().unwrap().handle_client(uid, &out);
                            route(&routes, msgs, &seq);
                        }
                    });
                }
            });
        }

        // Client : ne connaît que le contrat FrameCrypto et le format du fil.
        async fn client(addr: std::net::SocketAddr, uid: u64, want_epoch: u64) -> DaveSession {
            let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}")).await.unwrap();
            let (mut tx, mut rx) = ws.split();
            tx.send(Message::Text(uid.to_string().into())).await.unwrap();
            let mut dave = DaveSession::new(uid, 4242).unwrap();
            while dave.epoch() < want_epoch || !dave.is_encrypting() {
                let m = tokio::time::timeout(std::time::Duration::from_secs(20), rx.next()).await.expect("délai").unwrap().unwrap();
                let mut out = Vec::new();
                match m {
                    Message::Text(t) => {
                        let v: Value = serde_json::from_str(&t).unwrap();
                        let op = v["op"].as_u64().unwrap() as u8;
                        if op == 11 {
                            let ids: Vec<u64> = v["d"]["user_ids"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().parse().unwrap()).collect();
                            dave.on_users(&ids);
                        } else {
                            dave.on_op(op, Some(&v), None, &mut out);
                        }
                    }
                    Message::Binary(b) => {
                        let (_seq, op, payload) = decode_server(&b).unwrap();
                        dave.on_op(op, None, Some(payload), &mut out);
                    }
                    _ => {}
                }
                for o in out {
                    let m = match o {
                        Outgoing::Json(v) => Message::Text(v.to_string().into()),
                        Outgoing::Binary(op, p) => Message::Binary(encode_client(op, &p).into()),
                    };
                    tx.send(m).await.unwrap();
                }
            }
            // Laisse le socket ouvert le temps du test.
            tokio::spawn(async move {
                let _keep = tx;
                while rx.next().await.is_some() {}
            });
            dave
        }

        let a = tokio::spawn(client(addr, 11, 1));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let b = tokio::spawn(client(addr, 22, 1));
        let (mut a, mut b) = (a.await.unwrap(), b.await.unwrap());
        assert_eq!(a.call_code(), b.call_code());
        let frame = a.encrypt(b"opus sur le fil").unwrap();
        assert_eq!(b.decrypt(11, &frame).as_deref(), Some(&b"opus sur le fil"[..]));
        assert_eq!(gw.lock().unwrap().invalid_reports, 0);
    });
}

/// Image H.264 synthétique : SPS, PPS, IDR puis une tranche, corps sans code de début émulé.
fn h264_frame(seed: u8, idr_len: usize) -> Vec<u8> {
    let body = |len: usize, s: u8| (0..len).map(move |i| (i as u8).wrapping_mul(29).wrapping_add(s) | 1);
    let mut f = vec![0, 0, 0, 1, 0x67, 0x42, 0xc0, 0x1f];
    f.extend_from_slice(&[0, 0, 0, 1, 0x68, 0xce, 0x3c, 0x80]);
    // En-tête de tranche : first_mb 0, slice_type 7, pps 0 (1 0001000 1).
    f.extend_from_slice(&[0, 0, 1, 0x65, 0b1000_1000, 0b1000_0000 | 1]);
    f.extend(body(idr_len, seed));
    f.extend_from_slice(&[0, 0, 0, 1, 0x41, 0b1000_1000, 0b1000_0001]);
    f.extend(body(700, seed ^ 0x55));
    f
}

#[test]
fn video_chiffree_entre_deux_sessions() {
    use crate::net::voice::video::{h264, vp8};
    use crate::video::Codec;
    let mut b = Bench::new();
    b.connect(1);
    b.connect(2);
    assert!(b.s(1).is_encrypting());
    for i in 0..40u8 {
        let frame = h264_frame(i, 20_000 + usize::from(i) * 97);
        let enc = b.s(1).encrypt_video(Codec::H264, &frame).unwrap();
        // NAL toujours visibles (4 codes de début, aucun émulé), SPS et PPS en clair.
        let nals = h264::nal_units(&enc);
        assert_eq!(nals.len(), 4, "image {i}");
        assert_eq!(nals[0], &[0x67, 0x42, 0xc0, 0x1f]);
        assert_eq!(nals[1], &[0x68, 0xce, 0x3c, 0x80]);
        assert_eq!(&nals[2][..3], &[0x65, 0b1000_1000, 0b1000_0001], "en-tête de tranche en clair");
        assert!(h264::is_keyframe(&enc));
        assert_ne!(&nals[2][3..100], &h264::nal_units(&frame)[2][3..100], "IDR chiffrée");
        // Mise en paquets, réassemblage, déchiffrement par l'autre membre.
        let back = h264::depacketize(h264::packetize(&enc, 1100).iter().map(Vec::as_slice)).unwrap();
        assert_eq!(back, enc);
        let plain = b.s(2).decrypt_video(1, Codec::H264, &back).unwrap();
        // Les codes de début courts sont rendus longs avant chiffrement.
        let mut want = frame.clone();
        want.splice(16..19, [0, 0, 0, 1]);
        assert_eq!(plain, want);
    }
    // VP8 : 10 octets en clair pour une image clé.
    let mut key = vec![0x10u8; 5000];
    key[0] = 0x50;
    let enc = b.s(2).encrypt_video(Codec::Vp8, &key).unwrap();
    assert_eq!(&enc[..10], &key[..10]);
    assert_ne!(&enc[10..20], &key[10..20]);
    let back = vp8::depacketize(vp8::packetize(&enc, 1100).iter().map(Vec::as_slice)).unwrap();
    assert_eq!(b.s(1).decrypt_video(2, Codec::Vp8, &back).unwrap(), key);
    // L'audio continue sur le même compteur.
    assert_eq!(b.talk(1, 2, b"et la voix").as_deref(), Some(&b"et la voix"[..]));
    // Une image altérée est refusée.
    let mut bad = b.s(1).encrypt_video(Codec::H264, &h264_frame(1, 3000)).unwrap();
    let n = bad.len();
    bad[n / 2] ^= 0x10;
    assert_eq!(b.s(2).decrypt_video(1, Codec::H264, &bad), None);
}
