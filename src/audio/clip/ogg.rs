//! Conteneur Ogg (RFC 3533) et flux Opus dedans (RFC 7845), écrits à la
//! main : une page est un en-tête de 27 octets, une table de segments et
//! des données ; un paquet est la suite de ses segments (255 = il continue).
//! Seul le premier flux logique est lu (les messages vocaux n'en ont qu'un).

use super::ClipError;

const CAPTURE: &[u8; 4] = b"OggS";
const CONTINUED: u8 = 0x01;
const BOS: u8 = 0x02;
const EOS: u8 = 0x04;
/// Segments par page, au plus.
const MAX_SEGMENTS: usize = 255;

/// CRC de l'Ogg : polynôme 0x04C11DB7, sans réflexion, départ à 0.
fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut r = (i as u32) << 24;
            for _ in 0..8 {
                r = if r & 0x8000_0000 != 0 { (r << 1) ^ 0x04C1_1DB7 } else { r << 1 };
            }
            *e = r;
        }
        t
    });
    data.iter().fold(0u32, |crc, b| (crc << 8) ^ table[(((crc >> 24) as u8) ^ b) as usize])
}

/// Paquets du premier flux logique, dans l'ordre. Une page abîmée (CRC faux,
/// longueur impossible) arrête la lecture : ce qui précède est rendu.
pub fn packets(data: &[u8]) -> Result<Vec<Vec<u8>>, ClipError> {
    let mut out = Vec::new();
    let mut partial: Vec<u8> = Vec::new();
    let mut serial = None;
    let mut pos = 0;
    while pos + 27 <= data.len() {
        if &data[pos..pos + 4] != CAPTURE {
            break;
        }
        let n_seg = usize::from(data[pos + 26]);
        let table_end = pos + 27 + n_seg;
        let Some(table) = data.get(pos + 27..table_end) else { break };
        let body_len: usize = table.iter().map(|b| usize::from(*b)).sum();
        let Some(body) = data.get(table_end..table_end + body_len) else { break };
        let mut page = data[pos..table_end + body_len].to_vec();
        let stored = u32::from_le_bytes([page[22], page[23], page[24], page[25]]);
        page[22..26].fill(0);
        if crc32(&page) != stored {
            break;
        }
        let this = u32::from_le_bytes([data[pos + 14], data[pos + 15], data[pos + 16], data[pos + 17]]);
        let flags = data[pos + 5];
        pos = table_end + body_len;
        if *serial.get_or_insert(this) != this {
            continue;
        }
        if flags & CONTINUED == 0 {
            partial.clear();
        }
        let mut at = 0;
        for &len in table {
            let len = usize::from(len);
            partial.extend_from_slice(&body[at..at + len]);
            at += len;
            if len < 255 {
                out.push(std::mem::take(&mut partial));
            }
        }
    }
    if out.is_empty() {
        return Err(ClipError::Format);
    }
    Ok(out)
}

/// Écrit des pages Ogg d'un flux logique.
pub struct Writer {
    serial: u32,
    seq: u32,
    out: Vec<u8>,
}

impl Writer {
    pub fn new(serial: u32) -> Self {
        Writer { serial, seq: 0, out: Vec::new() }
    }

    /// Une page avec ces paquets entiers (chacun doit tenir dans la page).
    fn page(&mut self, packets: &[&[u8]], granule: u64, flags: u8) {
        let mut table = Vec::new();
        for p in packets {
            table.extend(std::iter::repeat_n(255u8, p.len() / 255));
            table.push((p.len() % 255) as u8);
        }
        let start = self.out.len();
        self.out.extend_from_slice(CAPTURE);
        self.out.push(0);
        self.out.push(flags);
        self.out.extend_from_slice(&granule.to_le_bytes());
        self.out.extend_from_slice(&self.serial.to_le_bytes());
        self.out.extend_from_slice(&self.seq.to_le_bytes());
        self.out.extend_from_slice(&[0; 4]);
        self.out.push(table.len() as u8);
        self.out.extend_from_slice(&table);
        for p in packets {
            self.out.extend_from_slice(p);
        }
        let crc = crc32(&self.out[start..]);
        self.out[start + 22..start + 26].copy_from_slice(&crc.to_le_bytes());
        self.seq += 1;
    }

    pub fn finish(self) -> Vec<u8> {
        self.out
    }
}

/// Segments pris par un paquet.
fn segments(len: usize) -> usize {
    len / 255 + 1
}

/// Un flux Opus lu dans un fichier Ogg.
#[derive(Debug)]
pub struct OpusStream {
    pub channels: u8,
    /// Échantillons (à 48 kHz) à jeter au début.
    pub pre_skip: u16,
    pub packets: Vec<Vec<u8>>,
}

/// Lit un fichier Ogg/Opus (famille de voies 0 : mono ou stéréo).
pub fn read_opus(data: &[u8]) -> Result<OpusStream, ClipError> {
    let mut packets = packets(data)?.into_iter();
    let head = packets.next().ok_or(ClipError::Format)?;
    if head.len() < 19 || &head[..8] != b"OpusHead" || head[8] & 0xF0 != 0 {
        return Err(ClipError::Format);
    }
    let channels = head[9];
    if head[18] != 0 || !(1..=2).contains(&channels) {
        return Err(ClipError::Unsupported);
    }
    let pre_skip = u16::from_le_bytes([head[10], head[11]]);
    let tags = packets.next().ok_or(ClipError::Format)?;
    if !tags.starts_with(b"OpusTags") {
        return Err(ClipError::Format);
    }
    Ok(OpusStream { channels, pre_skip, packets: packets.collect() })
}

/// Paquets par page, au plus (une page par seconde de trames de 20 ms).
const PAGE_PACKETS: usize = 50;

/// Flux Ogg/Opus écrit au fil de l'eau (enregistrement d'un appel, lot E4) :
/// chaque page part dans `out` dès qu'elle est pleine, rien ne s'accumule.
/// Mêmes pages que [`write_opus`] pour les mêmes paquets.
pub struct OpusFile<W: std::io::Write> {
    out: W,
    pages: Writer,
    pending: Vec<Vec<u8>>,
    segs: usize,
    granule: u64,
    frame: u64,
}

impl<W: std::io::Write> OpusFile<W> {
    /// Écrit les en-têtes ; chaque paquet portera `frame` échantillons à 48 kHz.
    pub fn create(mut out: W, channels: u8, pre_skip: u16, frame: u64, serial: u32) -> std::io::Result<Self> {
        let mut pages = Writer::new(serial);
        head_pages(&mut pages, channels, pre_skip);
        out.write_all(&pages.out)?;
        pages.out.clear();
        Ok(OpusFile { out, pages, pending: Vec::with_capacity(PAGE_PACKETS), segs: 0, granule: u64::from(pre_skip), frame })
    }

    pub fn push(&mut self, packet: &[u8]) -> std::io::Result<()> {
        if self.pending.len() == PAGE_PACKETS || self.segs + segments(packet.len()) > MAX_SEGMENTS {
            self.flush(0)?;
        }
        self.segs += segments(packet.len());
        self.pending.push(packet.to_vec());
        Ok(())
    }

    fn flush(&mut self, flags: u8) -> std::io::Result<()> {
        self.granule += self.frame * self.pending.len() as u64;
        let page: Vec<&[u8]> = self.pending.iter().map(Vec::as_slice).collect();
        self.pages.page(&page, self.granule, flags);
        self.pending.clear();
        self.segs = 0;
        self.out.write_all(&self.pages.out)?;
        self.pages.out.clear();
        Ok(())
    }

    /// Dernière page (marquée fin de flux) ; rend la sortie.
    pub fn finish(mut self) -> std::io::Result<W> {
        self.flush(EOS)?;
        Ok(self.out)
    }
}

/// Les deux pages d'en-tête d'un flux Opus (RFC 7845).
fn head_pages(w: &mut Writer, channels: u8, pre_skip: u16) {
    let mut head = b"OpusHead".to_vec();
    head.push(1);
    head.push(channels);
    head.extend_from_slice(&pre_skip.to_le_bytes());
    head.extend_from_slice(&48_000u32.to_le_bytes());
    head.extend_from_slice(&0i16.to_le_bytes());
    head.push(0);
    w.page(&[&head], 0, BOS);
    let vendor = b"ferrocord";
    let mut tags = b"OpusTags".to_vec();
    tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    tags.extend_from_slice(vendor);
    tags.extend_from_slice(&0u32.to_le_bytes());
    w.page(&[&tags], 0, 0);
}

/// Fichier Ogg/Opus de ces paquets ; chacun porte `frame` échantillons à 48 kHz.
pub fn write_opus(packets: &[Vec<u8>], channels: u8, pre_skip: u16, frame: u64, serial: u32) -> Vec<u8> {
    let mut w = Writer::new(serial);
    head_pages(&mut w, channels, pre_skip);
    // Une page par seconde environ, sans dépasser 255 segments.
    let mut granule = u64::from(pre_skip);
    let mut i = 0;
    while i < packets.len() {
        let mut segs = 0;
        let mut page: Vec<&[u8]> = Vec::new();
        while i < packets.len() && page.len() < PAGE_PACKETS && segs + segments(packets[i].len()) <= MAX_SEGMENTS {
            segs += segments(packets[i].len());
            page.push(&packets[i]);
            i += 1;
        }
        if page.is_empty() {
            // Paquet trop long pour une page (impossible avec Opus) : ignoré.
            i += 1;
            continue;
        }
        granule += frame * page.len() as u64;
        w.page(&page, granule, if i == packets.len() { EOS } else { 0 });
    }
    w.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_of_a_known_value() {
        // Valeur de référence du CRC-32/MPEG-2 sans inversion finale (départ 0) : « 123456789 ».
        assert_eq!(crc32(b"123456789"), 0x89A1_897F);
    }

    #[test]
    fn opus_file_round_trip_with_long_packets() {
        let packets: Vec<Vec<u8>> = (0..120).map(|i| vec![i as u8; (i * 7) % 600 + 1]).collect();
        let file = write_opus(&packets, 1, 312, 960, 0xF00D);
        let s = read_opus(&file).unwrap();
        assert_eq!((s.channels, s.pre_skip), (1, 312));
        assert_eq!(s.packets, packets);
        // Une page abîmée arrête la lecture sans planter.
        let mut bad = file.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0xFF;
        let s = read_opus(&bad).unwrap();
        assert!(s.packets.len() < packets.len());
        assert!(read_opus(b"pas un fichier ogg").is_err());
        assert!(read_opus(&file[..30]).is_err());
    }

    /// Le flux écrit au fil de l'eau donne les mêmes octets que le fichier écrit d'un coup.
    #[test]
    fn streamed_file_matches_the_one_shot_writer() {
        let packets: Vec<Vec<u8>> = (0..173).map(|i| vec![(i * 3) as u8; (i * 11) % 700 + 1]).collect();
        let mut f = OpusFile::create(Vec::new(), 1, 312, 960, 0xBEEF).unwrap();
        for p in &packets {
            f.push(p).unwrap();
        }
        let streamed = f.finish().unwrap();
        assert_eq!(streamed, write_opus(&packets, 1, 312, 960, 0xBEEF));
        assert_eq!(read_opus(&streamed).unwrap().packets, packets);
        // Sans aucun paquet : des en-têtes et une page de fin vide, lisibles.
        let empty = OpusFile::create(Vec::new(), 1, 312, 960, 1).unwrap().finish().unwrap();
        assert!(read_opus(&empty).unwrap().packets.is_empty());
    }
}
