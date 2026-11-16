//! Ressources Windows de l'exécutable (lot « icône ») : l'icône que montrent
//! l'Explorateur, les raccourcis, la barre des tâches et « Applications
//! installées », et le bloc de version (propriétés du fichier, colonne
//! « Nom » du Gestionnaire des tâches).
//!
//! Ce fichier est compilé deux fois : par `build.rs` (`#[path]`), qui écrit le
//! fichier `.res` donné à l'éditeur de liens de MSVC, et par les tests de la
//! bibliothèque, qui relisent la structure produite. Bibliothèque standard
//! seule : ni `rc.exe`, ni dépendance de compilation.
//!
//! Format RES 32 bits : une suite de ressources, chacune précédée d'un
//! en-tête (taille des données, taille de l'en-tête, type, nom, drapeaux,
//! langue) et alignée sur quatre octets ; la première est vide.

/// Types de ressources (`RT_ICON`, `RT_GROUP_ICON`, `RT_VERSION`).
pub const RT_ICON: u16 = 3;
pub const RT_GROUP_ICON: u16 = 14;
pub const RT_VERSION: u16 = 16;
/// Identifiant du groupe d'icônes et du bloc de version. Le groupe est le
/// seul de l'exécutable : c'est lui que l'Explorateur prend (index 0).
pub const ID: u16 = 1;
/// Langue du bloc de version : français (France), texte en UTF-16.
const LANG: u16 = 0x040C;
const CODEPAGE: u16 = 1200;
/// Taille d'un en-tête dont le type et le nom sont des nombres.
const HEADER: u32 = 32;

/// Une image d'un fichier `.ico`.
pub struct Image<'a> {
    /// Les douze premiers octets de son entrée (`ICONDIRENTRY` sans le
    /// décalage) : largeur, hauteur, couleurs, réservé, plans, bits par
    /// pixel, taille. `GRPICONDIRENTRY` commence par les mêmes.
    pub entry: &'a [u8],
    /// DIB (en-tête, pixels, masque) ou fichier PNG entier.
    pub data: &'a [u8],
}

impl Image<'_> {
    /// Côté en pixels (0 dans le fichier veut dire 256).
    #[cfg_attr(not(test), allow(dead_code))] // `build.rs` ne s'en sert pas.
    pub fn side(&self) -> u32 {
        if self.entry[0] == 0 { 256 } else { u32::from(self.entry[0]) }
    }
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Images d'un fichier `.ico`, dans l'ordre du fichier ; `None` s'il est mal formé.
pub fn images(ico: &[u8]) -> Option<Vec<Image<'_>>> {
    if u16_at(ico, 0)? != 0 || u16_at(ico, 2)? != 1 {
        return None;
    }
    let count = usize::from(u16_at(ico, 4)?);
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let at = 6 + i * 16;
        let entry = ico.get(at..at + 12)?;
        let (size, offset) = (u32_at(ico, at + 8)? as usize, u32_at(ico, at + 12)? as usize);
        out.push(Image { entry, data: ico.get(offset..offset.checked_add(size)?)? });
    }
    (!out.is_empty()).then_some(out)
}

/// Ajoute une ressource (en-tête, données, bourrage jusqu'au multiple de quatre).
fn push(out: &mut Vec<u8>, kind: u16, id: u16, flags: u16, lang: u16, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&HEADER.to_le_bytes());
    // Type puis nom : 0xFFFF annonce un nombre et non une chaîne.
    for v in [0xFFFF, kind, 0xFFFF, id] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&lang.to_le_bytes());
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(data);
    pad(out);
}

fn pad(out: &mut Vec<u8>) {
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect()
}

/// Bloc du format de version : longueur, longueur de la valeur, genre
/// (1 : texte), clé en UTF-16, valeur, puis les blocs fils, chacun aligné.
fn block(key: &str, value: &[u8], value_len: u16, text: bool, children: &[Vec<u8>]) -> Vec<u8> {
    let mut b = vec![0, 0];
    b.extend_from_slice(&value_len.to_le_bytes());
    b.extend_from_slice(&u16::from(text).to_le_bytes());
    b.extend_from_slice(&utf16(key));
    pad(&mut b);
    b.extend_from_slice(value);
    for child in children {
        pad(&mut b);
        b.extend_from_slice(child);
    }
    let len = (b.len() as u16).to_le_bytes();
    b[..2].copy_from_slice(&len);
    b
}

/// Les quatre nombres d'une version de Cargo (« 1.2.3 » donne 1, 2, 3, 0 ;
/// un suffixe comme « -beta » est ignoré).
pub fn numbers(version: &str) -> [u16; 4] {
    let mut out = [0; 4];
    let core = version.split(['-', '+']).next().unwrap_or_default();
    for (slot, part) in out.iter_mut().zip(core.split('.')) {
        *slot = part.parse().unwrap_or(0);
    }
    out
}

/// Bloc `VS_VERSIONINFO`. Aucun nom de personne : le dépôt est public.
fn version_info(version: &str) -> Vec<u8> {
    let n = numbers(version).map(u32::from);
    let (high, low) = (n[0] << 16 | n[1], n[2] << 16 | n[3]);
    // VS_FIXEDFILEINFO : signature, version de la structure, version du
    // fichier puis du produit, masque des drapeaux, drapeaux, système
    // (Windows NT 32 bits), genre (application), sous-genre, date.
    let fixed: Vec<u8> = [0xFEEF_04BD, 0x0001_0000, high, low, high, low, 0x3F, 0, 0x0004_0004, 1, 0, 0, 0].into_iter().flat_map(u32::to_le_bytes).collect();
    let strings: Vec<Vec<u8>> = [
        ("CompanyName", "Projet Ferrocord"),
        ("FileDescription", "Ferrocord"),
        ("FileVersion", version),
        ("InternalName", "ferrocord"),
        ("LegalCopyright", "Projet Ferrocord"),
        ("OriginalFilename", "ferrocord.exe"),
        ("ProductName", "Ferrocord"),
        ("ProductVersion", version),
    ]
    .into_iter()
    .map(|(key, value)| block(key, &utf16(value), value.encode_utf16().count() as u16 + 1, true, &[]))
    .collect();
    let table = block(&format!("{LANG:04X}{CODEPAGE:04X}"), &[], 0, true, &strings);
    let translation: Vec<u8> = [LANG, CODEPAGE].into_iter().flat_map(u16::to_le_bytes).collect();
    let children = [block("StringFileInfo", &[], 0, true, &[table]), block("VarFileInfo", &[], 0, true, &[block("Translation", &translation, 4, false, &[])])];
    block("VS_VERSION_INFO", &fixed, fixed.len() as u16, false, &children)
}

/// Fichier `.res` : chaque image de `ico` en `RT_ICON` (identifiants 1, 2, …
/// dans l'ordre du fichier), le groupe `RT_GROUP_ICON` qui les nomme, et le
/// bloc de version. `None` si `ico` est mal formé.
pub fn res(ico: &[u8], version: &str) -> Option<Vec<u8>> {
    let images = images(ico)?;
    let mut out = Vec::with_capacity(ico.len() + 2048);
    push(&mut out, 0, 0, 0, 0, &[]);
    // GRPICONDIR : réservé, genre (1 : icône), nombre d'images.
    let mut group: Vec<u8> = [0, 1, images.len() as u16].into_iter().flat_map(u16::to_le_bytes).collect();
    for (id, image) in (1u16..).zip(&images) {
        // Drapeaux d'usage de `rc.exe` : déplaçable, jetable.
        push(&mut out, RT_ICON, id, 0x1010, 0, image.data);
        group.extend_from_slice(image.entry);
        group.extend_from_slice(&id.to_le_bytes());
    }
    push(&mut out, RT_GROUP_ICON, ID, 0x1030, 0, &group);
    push(&mut out, RT_VERSION, ID, 0x0030, LANG, &version_info(version));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICO: &[u8] = include_bytes!("../../assets/icone/ferrocord.ico");

    /// Ressources d'un fichier `.res` : type, identifiant, langue, données.
    fn resources(res: &[u8]) -> Vec<(u16, u16, u16, &[u8])> {
        let mut out = Vec::new();
        let mut at = 0;
        while at < res.len() {
            assert_eq!(at % 4, 0, "ressource non alignée");
            let (size, header) = (u32_at(res, at).unwrap() as usize, u32_at(res, at + 4).unwrap() as usize);
            assert_eq!(header, 32);
            assert_eq!((u16_at(res, at + 8), u16_at(res, at + 12)), (Some(0xFFFF), Some(0xFFFF)), "type et nom numériques");
            out.push((u16_at(res, at + 10).unwrap(), u16_at(res, at + 14).unwrap(), u16_at(res, at + 22).unwrap(), &res[at + header..at + header + size]));
            at += (header + size).next_multiple_of(4);
        }
        assert_eq!(at, res.len(), "fin alignée, rien après la dernière ressource");
        out
    }

    fn text(utf16: &[u8]) -> String {
        String::from_utf16_lossy(&utf16.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect::<Vec<_>>())
    }

    #[test]
    fn res_holds_every_image_one_group_and_the_version() {
        let images = images(ICO).expect("fichier .ico");
        let res = res(ICO, "3.4.5").expect("fichier .res");
        let all = resources(&res);
        // En-tête vide, une icône par image, le groupe, la version.
        assert_eq!(all.len(), 1 + images.len() + 2);
        assert_eq!((all[0].0, all[0].1, all[0].3.len()), (0, 0, 0));
        for (i, image) in images.iter().enumerate() {
            let (kind, id, _, data) = all[1 + i];
            assert_eq!((kind, usize::from(id)), (RT_ICON, i + 1));
            assert!(data == image.data, "image {} recopiée telle quelle", image.side());
        }
        let (kind, id, _, group) = all[1 + images.len()];
        assert_eq!((kind, id), (RT_GROUP_ICON, 1));
        assert_eq!(group.len(), 6 + 14 * images.len());
        assert_eq!((u16_at(group, 0), u16_at(group, 2), u16_at(group, 4)), (Some(0), Some(1), Some(images.len() as u16)));
        for (i, image) in images.iter().enumerate() {
            let entry = &group[6 + 14 * i..6 + 14 * (i + 1)];
            assert_eq!(&entry[..12], image.entry);
            assert_eq!(u32_at(entry, 8).unwrap() as usize, image.data.len(), "taille de l'image {}", image.side());
            assert_eq!(usize::from(u16_at(entry, 12).unwrap()), i + 1, "identifiant de l'image {}", image.side());
        }
        let (kind, id, lang, version) = all[2 + images.len()];
        assert_eq!((kind, id, lang), (RT_VERSION, 1, 0x040C));
        assert_eq!(usize::from(u16_at(version, 0).unwrap()), version.len(), "longueur du bloc de version");
        assert_eq!(u16_at(version, 2), Some(52));
        assert_eq!(text(&version[6..36]), "VS_VERSION_INFO");
        // Structure fixe alignée après la clé : signature, puis 3.4.5.0 deux fois.
        assert_eq!(u32_at(version, 40), Some(0xFEEF_04BD));
        assert_eq!((u32_at(version, 48), u32_at(version, 52)), (Some(3 << 16 | 4), Some(5 << 16)));
        assert_eq!((u32_at(version, 56), u32_at(version, 60)), (Some(3 << 16 | 4), Some(5 << 16)));
        // Zéros retirés (fins de chaîne et bourrage) : chaque clé est suivie de sa valeur.
        let readable = text(version).replace('\0', "");
        for wanted in ["StringFileInfo", "040C04B0", "ProductNameFerrocord", "FileDescriptionFerrocord", "OriginalFilenameferrocord.exe", "FileVersion3.4.5", "ProductVersion3.4.5", "CompanyNameProjet Ferrocord", "LegalCopyrightProjet Ferrocord", "VarFileInfo", "Translation"] {
            assert!(readable.contains(wanted), "bloc de version sans {wanted:?}");
        }
    }

    #[test]
    fn version_blocks_are_aligned_and_sized() {
        // « ab » : en-tête de 6 octets, clé de 6 (avec son zéro), valeur alignée à 12.
        let b = block("ab", &utf16("xyz"), 4, true, &[]);
        assert_eq!(b.len(), 20);
        assert_eq!((u16_at(&b, 0), u16_at(&b, 2), u16_at(&b, 4)), (Some(20), Some(4), Some(1)));
        // Clé de 4 octets : deux octets de bourrage avant la valeur ; le fils est aligné lui aussi.
        let parent = block("a", &[7, 7], 2, false, std::slice::from_ref(&b));
        assert_eq!(&parent[6..12], &[b'a', 0, 0, 0, 0, 0]);
        assert_eq!(&parent[12..14], &[7, 7]);
        assert_eq!(&parent[16..], &b[..]);
        assert_eq!(usize::from(u16_at(&parent, 0).unwrap()), parent.len());
    }

    #[test]
    fn cargo_versions_become_four_numbers() {
        assert_eq!(numbers("0.1.1"), [0, 1, 1, 0]);
        assert_eq!(numbers("12.3.45-beta.2+abc"), [12, 3, 45, 0]);
        assert_eq!(numbers(""), [0, 0, 0, 0]);
    }

    #[test]
    fn malformed_ico_is_refused() {
        assert!(images(&[]).is_none() && res(&[0, 0, 2, 0, 1, 0], "1.0.0").is_none());
        // Une entrée qui pointe après la fin du fichier.
        let mut cut = ICO[..6 + 16].to_vec();
        cut[4] = 1;
        assert!(images(&cut).is_none());
        // Aucune image.
        assert!(images(&[0, 0, 1, 0, 0, 0]).is_none());
    }
}
