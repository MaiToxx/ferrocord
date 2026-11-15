//! Décompression du flux de la passerelle (`compress=zlib-stream`).
//!
//! Discord compresse toute la connexion avec un seul contexte zlib : chaque
//! paquet se termine par un vidage synchrone (`00 00 FF FF`), et un paquet
//! peut arriver en plusieurs messages WebSocket. Le contexte vit donc aussi
//! longtemps que la connexion et n'est jamais réutilisé pour la suivante.
//!
//! Pourquoi zlib plutôt que zstd : `flate2` (moteur `miniz_oxide`, Rust pur)
//! est déjà dans l'exécutable pour le PNG, la compression ne coûte donc
//! aucune nouvelle bibliothèque ; un décodeur zstd en Rust pur (`ruzstd`)
//! ajouterait du code, décode moins vite, et devrait être alimenté par blocs
//! complets d'une trame zstd qui ne se termine jamais.

use flate2::{Decompress, FlushDecompress, Status};

/// Fin d'un paquet complet (vidage synchrone zlib).
const SUFFIX: [u8; 4] = [0x00, 0x00, 0xFF, 0xFF];
/// Capacité gardée entre deux paquets : au-delà (après un READY de plusieurs
/// Mo), le tampon est rendu au système pour ne pas occuper la mémoire en continu.
pub const KEEP_CAPACITY: usize = 256 << 10;
/// Pas d'agrandissement minimal du tampon de sortie.
const MIN_GROW: usize = 32 << 10;

#[derive(Debug, PartialEq, Eq)]
pub enum InflateError {
    /// Données illisibles (flux corrompu, ou fin de flux inattendue).
    Corrupt,
    /// Le paquet décompressé dépasserait la taille maximale (bombe de décompression).
    TooLarge,
}

impl std::fmt::Display for InflateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            // Texte technique (journaux) : l'interface affiche la `Cause` correspondante.
            InflateError::Corrupt => "corrupt compressed stream",
            InflateError::TooLarge => "decompressed packet too large",
        })
    }
}

impl std::error::Error for InflateError {}

/// Contexte de décompression d'une connexion.
pub struct Inflater {
    z: Decompress,
    /// Paquet en cours de décompression.
    out: Vec<u8>,
    /// Derniers octets compressés reçus : le suffixe peut être à cheval sur deux messages.
    tail: [u8; 4],
    tail_len: usize,
    max: usize,
    /// Plus grande capacité atteinte par le tampon (mesure de la mémoire de pointe).
    peak: usize,
    /// Une erreur rend le contexte inutilisable : la suite du flux en dépend.
    broken: bool,
}

impl Inflater {
    /// `max` : taille maximale d'un paquet décompressé, en octets.
    pub fn new(max: usize) -> Self {
        Self { z: Decompress::new(true), out: Vec::new(), tail: [0; 4], tail_len: 0, max, peak: 0, broken: false }
    }

    /// Un paquet est-il commencé mais pas terminé ?
    pub fn pending(&self) -> bool {
        self.tail_len > 0 && !self.ends_with_suffix()
    }

    #[cfg(test)]
    pub fn peak_capacity(&self) -> usize {
        self.peak
    }

    #[cfg(test)]
    pub fn capacity(&self) -> usize {
        self.out.capacity()
    }

    fn ends_with_suffix(&self) -> bool {
        self.tail_len == 4 && self.tail == SUFFIX
    }

    fn push_tail(&mut self, data: &[u8]) {
        let take = data.len().min(4);
        for &b in &data[data.len() - take..] {
            self.tail.copy_within(1.., 0);
            self.tail[3] = b;
        }
        self.tail_len = (self.tail_len + take).min(4);
    }

    /// Ajoute un message WebSocket. Renvoie le paquet décompressé s'il est
    /// complet ; il reste valable jusqu'au prochain appel.
    pub fn feed(&mut self, data: &[u8]) -> Result<Option<&[u8]>, InflateError> {
        if self.broken {
            return Err(InflateError::Corrupt);
        }
        if self.ends_with_suffix() {
            self.start_next();
        }
        if let Err(e) = self.inflate(data) {
            self.broken = true;
            self.out = Vec::new();
            return Err(e);
        }
        self.push_tail(data);
        Ok(self.ends_with_suffix().then_some(self.out.as_slice()))
    }

    /// Emporte le paquet complet que `feed` vient de rendre, sans le copier
    /// (pour le décoder ailleurs) ; vide si aucun paquet n'est complet.
    pub fn take_packet(&mut self) -> Vec<u8> {
        if !self.ends_with_suffix() {
            return Vec::new();
        }
        self.tail_len = 0;
        std::mem::take(&mut self.out)
    }

    /// Oublie le paquet précédent, en rendant la mémoire d'un très gros paquet.
    fn start_next(&mut self) {
        self.out.clear();
        if self.out.capacity() > KEEP_CAPACITY {
            self.out = Vec::new();
        }
        self.tail_len = 0;
    }

    fn inflate(&mut self, mut input: &[u8]) -> Result<(), InflateError> {
        loop {
            if self.out.len() == self.out.capacity() {
                let room = self.max.saturating_sub(self.out.len());
                if room == 0 {
                    input = self.probe_at_limit(input)?;
                    if input.is_empty() {
                        return Ok(());
                    }
                    continue;
                }
                self.out.reserve_exact(self.out.len().max(MIN_GROW).min(room));
                self.peak = self.peak.max(self.out.capacity());
            }
            let (in0, out0) = (self.z.total_in(), self.z.total_out());
            let status = self.z.decompress_vec(input, &mut self.out, FlushDecompress::Sync).map_err(|_| InflateError::Corrupt)?;
            let consumed = usize::try_from(self.z.total_in() - in0).map_err(|_| InflateError::Corrupt)?;
            let produced = self.z.total_out() - out0;
            input = input.get(consumed..).ok_or(InflateError::Corrupt)?;
            if status == Status::StreamEnd {
                // Discord ne termine jamais le flux : une fin est une anomalie.
                return Err(InflateError::Corrupt);
            }
            let full = self.out.len() == self.out.capacity();
            if input.is_empty() && !full {
                return Ok(());
            }
            if consumed == 0 && produced == 0 && !full {
                return Err(InflateError::Corrupt);
            }
        }
    }

    /// Tampon plein à la limite : le reste de l'entrée ne doit plus rien
    /// produire (il peut encore contenir le vidage final, sans données).
    fn probe_at_limit<'a>(&mut self, input: &'a [u8]) -> Result<&'a [u8], InflateError> {
        let mut probe = [0u8; 1];
        let (in0, out0) = (self.z.total_in(), self.z.total_out());
        let status = self.z.decompress(input, &mut probe, FlushDecompress::Sync).map_err(|_| InflateError::Corrupt)?;
        if self.z.total_out() != out0 {
            return Err(InflateError::TooLarge);
        }
        if status == Status::StreamEnd {
            return Err(InflateError::Corrupt);
        }
        let consumed = usize::try_from(self.z.total_in() - in0).map_err(|_| InflateError::Corrupt)?;
        let rest = input.get(consumed..).ok_or(InflateError::Corrupt)?;
        if consumed == 0 && !rest.is_empty() {
            return Err(InflateError::Corrupt);
        }
        Ok(rest)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use flate2::{Compress, Compression, FlushCompress};

    /// Compresseur de flux, comme celui de Discord (et du faux serveur).
    pub(crate) struct Deflater(Compress);

    impl Deflater {
        pub(crate) fn new() -> Self {
            Self(Compress::new(Compression::default(), true))
        }

        pub(crate) fn packet(&mut self, data: &[u8]) -> Vec<u8> {
            let mut out = Vec::with_capacity(data.len() / 2 + 64);
            let mut input = data;
            loop {
                if out.len() == out.capacity() {
                    out.reserve(out.capacity().max(64));
                }
                let before = self.0.total_in();
                self.0.compress_vec(input, &mut out, FlushCompress::Sync).unwrap_or_else(|e| panic!("{e}"));
                let used = (self.0.total_in() - before) as usize;
                input = &input[used..];
                if input.is_empty() && out.len() < out.capacity() {
                    break;
                }
            }
            assert!(out.ends_with(&SUFFIX));
            out
        }
    }

    fn feed_all(inf: &mut Inflater, chunks: &[&[u8]]) -> Vec<Option<Vec<u8>>> {
        chunks.iter().map(|c| inf.feed(c).map(|o| o.map(<[u8]>::to_vec)).unwrap_or_else(|e| panic!("{e}"))).collect()
    }

    #[test]
    fn round_trip_keeps_context_between_packets() {
        let mut d = Deflater::new();
        let mut inf = Inflater::new(1 << 20);
        for i in 0..50 {
            let text = format!(r#"{{"op":0,"t":"MESSAGE_CREATE","s":{i},"d":{{"content":"bonjour bonjour {i}"}}}}"#);
            let c = d.packet(text.as_bytes());
            assert_eq!(inf.feed(&c), Ok(Some(text.as_bytes())));
            assert!(!inf.pending());
        }
        // Le contexte partagé rend les paquets répétés minuscules.
        let again = d.packet(br#"{"op":0,"t":"MESSAGE_CREATE","s":49,"d":{"content":"bonjour bonjour 49"}}"#);
        assert!(again.len() < 30, "{}", again.len());
    }

    #[test]
    fn packet_split_in_many_frames() {
        let text: String = (0..2000).map(|i| format!("{i:05} ")).collect();
        // Coupé partout, y compris au milieu du suffixe.
        for cut in [1, 2, 3, 7, 100] {
            let mut d = Deflater::new();
            let c = d.packet(text.as_bytes());
            let mut inf = Inflater::new(1 << 20);
            let chunks: Vec<&[u8]> = c.chunks(cut).collect();
            let results = feed_all(&mut inf, &chunks);
            let (last, rest) = results.split_last().unwrap_or_else(|| panic!("vide"));
            assert!(rest.iter().all(Option::is_none), "paquet rendu avant la fin (coupe {cut})");
            assert_eq!(last.as_deref(), Some(text.as_bytes()));
        }
        // Suffixe à cheval : « …00 00 » puis « FF FF ».
        let mut d = Deflater::new();
        let c = d.packet(text.as_bytes());
        let mut inf = Inflater::new(1 << 20);
        let (a, b) = c.split_at(c.len() - 2);
        assert_eq!(inf.feed(a), Ok(None));
        assert!(inf.pending());
        assert_eq!(inf.feed(b), Ok(Some(text.as_bytes())));
        assert!(!inf.pending());
    }

    #[test]
    fn corrupt_stream_is_an_error_and_stays_broken() {
        let mut d = Deflater::new();
        let mut inf = Inflater::new(1 << 20);
        let ok = d.packet(b"premier paquet");
        assert!(inf.feed(&ok).is_ok());
        let mut bad = d.packet(b"second paquet, abime en route");
        for b in bad.iter_mut().take(6) {
            *b ^= 0xA5;
        }
        assert_eq!(inf.feed(&bad), Err(InflateError::Corrupt));
        // La suite du flux dépend de ce qui a été perdu : plus rien n'est lu.
        let next = d.packet(b"troisieme");
        assert_eq!(inf.feed(&next), Err(InflateError::Corrupt));
        // Données qui ne sont pas du zlib du tout.
        let mut fresh = Inflater::new(1 << 20);
        assert_eq!(fresh.feed(b"{\"op\":10}\x00\x00\xff\xff"), Err(InflateError::Corrupt));
    }

    #[test]
    fn end_of_stream_is_corrupt() {
        let mut c = Compress::new(Compression::default(), true);
        let mut out = Vec::with_capacity(256);
        c.compress_vec(b"fin", &mut out, FlushCompress::Finish).unwrap_or_else(|e| panic!("{e}"));
        let mut inf = Inflater::new(1 << 20);
        assert_eq!(inf.feed(&out), Err(InflateError::Corrupt));
    }

    #[test]
    fn decompression_bomb_is_refused_without_allocating_it() {
        let zeros = vec![0u8; 2 << 20];
        let mut d = Deflater::new();
        let c = d.packet(&zeros);
        assert!(c.len() < 20 << 10, "{}", c.len());
        let max = 512 << 10;
        let mut inf = Inflater::new(max);
        assert_eq!(inf.feed(&c), Err(InflateError::TooLarge));
        assert!(inf.peak_capacity() <= max, "{} > {max}", inf.peak_capacity());
        // Exactement à la limite : accepté.
        let mut d = Deflater::new();
        let c = d.packet(&zeros[..max]);
        let mut inf = Inflater::new(max);
        assert_eq!(inf.feed(&c).map(|o| o.map(<[u8]>::len)), Ok(Some(max)));
    }

    #[test]
    fn big_packet_memory_is_released() {
        let mut d = Deflater::new();
        let mut inf = Inflater::new(64 << 20);
        let big: Vec<u8> = (0..1u32 << 20).map(|i| (i % 251) as u8).collect();
        assert_eq!(inf.feed(&d.packet(&big)).map(|o| o.map(<[u8]>::len)), Ok(Some(big.len())));
        assert!(inf.capacity() >= big.len());
        assert!(inf.feed(&d.packet(b"petit")).is_ok());
        assert!(inf.capacity() <= KEEP_CAPACITY, "{}", inf.capacity());
    }

    /// READY synthétique (forme de `tests/fixtures/ready_user_classic.json`) :
    /// `guilds` serveurs de 30 salons et 20 rôles, identifiants pseudo-aléatoires.
    pub(crate) fn synthetic_ready(guilds: usize) -> String {
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut id = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            1_000_000_000_000_000_000 + x % 400_000_000_000_000_000
        };
        let mut g = Vec::with_capacity(guilds);
        for i in 0..guilds {
            let gid = id();
            let channels: Vec<String> = (0..30)
                .map(|c| {
                    format!(
                        r#"{{"id":"{}","type":0,"name":"salon-{c}-{i}","position":{c},"parent_id":"{}","topic":"Sujet du salon {c} du serveur {i}","last_message_id":"{}","permission_overwrites":[{{"id":"{}","type":0,"allow":"0","deny":"1024"}}],"rate_limit_per_user":0,"nsfw":false}}"#,
                        id(),
                        id(),
                        id(),
                        id()
                    )
                })
                .collect();
            let roles: Vec<String> = (0..20)
                .map(|r| {
                    format!(
                        r#"{{"id":"{}","name":"Rôle {r}","permissions":"{}","position":{r},"color":{},"hoist":false,"managed":false,"mentionable":true}}"#,
                        id(),
                        id() % 1_000_000_000,
                        id() % 16_777_216
                    )
                })
                .collect();
            g.push(format!(
                r#"{{"id":"{gid}","name":"Serveur fictif {i}","icon":null,"owner_id":"{}","features":["COMMUNITY","NEWS"],"channels":[{}],"roles":[{}],"members":[{{"user":{{"id":"1000000000000000001"}},"roles":[],"joined_at":"2020-01-01T00:00:00+00:00"}}]}}"#,
                id(),
                channels.join(","),
                roles.join(",")
            ));
        }
        format!(
            r#"{{"op":0,"t":"READY","s":1,"d":{{"v":10,"user":{{"id":"1000000000000000001","username":"moi_fictif","global_name":"Moi"}},"session_id":"session-fictive","resume_gateway_url":"wss://gateway.discord.gg","guilds":[{}],"private_channels":[],"users":[]}}}}"#,
            g.join(",")
        )
    }

    /// Mesures pour le rapport : `cargo test --release --lib -- --ignored --nocapture measure_ready`.
    #[test]
    #[ignore]
    fn measure_ready() {
        for guilds in [100, 400, 1500] {
            let ready = synthetic_ready(guilds);
            let mut d = Deflater::new();
            let c = d.packet(ready.as_bytes());
            let mut inf = Inflater::new(256 << 20);
            let t = std::time::Instant::now();
            let n = inf.feed(&c).map(|o| o.map(<[u8]>::len)).unwrap_or_else(|e| panic!("{e}"));
            let dt = t.elapsed();
            let t = std::time::Instant::now();
            let parsed = serde_json::from_slice::<serde::de::IgnoredAny>(ready.as_bytes()).is_ok();
            let parse = t.elapsed();
            eprintln!(
                "READY {guilds} serveurs : {} octets en clair, {} compressés ({:.1} %), décompression {dt:?}, \
                 tampon de pointe {} octets (lecture JSON seule : {parse:?})",
                ready.len(),
                c.len(),
                c.len() as f64 * 100.0 / ready.len() as f64,
                inf.peak_capacity()
            );
            assert!(parsed);
            assert_eq!(n, Some(ready.len()));
        }
    }
}
