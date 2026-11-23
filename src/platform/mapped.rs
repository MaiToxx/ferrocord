//! Fichier projeté en mémoire, en lecture seule (lot C9) : les polices de
//! Windows sont lues ainsi plutôt que copiées dans le tas. Les pages restent
//! celles du cache de fichiers du système, partagées avec les autres
//! programmes qui affichent du Segoe UI : environ 4,8 Mo de mémoire privée
//! en moins pour les cinq fichiers. egui l'accepte tel quel
//! (`FontData::from_blob`, « a memory-mapped system font file »).

use std::{fs::File, os::windows::io::AsRawHandle};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Memory::{CreateFileMappingW, MapViewOfFile, UnmapViewOfFile, VirtualQuery, FILE_MAP_READ, MEMORY_BASIC_INFORMATION, MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READONLY};

/// Vue en lecture seule d'un fichier entier ; démappée à la fin de vie.
pub struct Mapped {
    view: MEMORY_MAPPED_VIEW_ADDRESS,
    len: usize,
}

// SAFETY: la vue est en lecture seule, jamais modifiée par ce programme, et
// valable jusqu'à `Drop` ; la partager entre fils revient à partager un `&[u8]`.
unsafe impl Send for Mapped {}
// SAFETY: comme ci-dessus : seulement des lectures.
unsafe impl Sync for Mapped {}

impl Mapped {
    /// Projette `file` (déjà ouvert en lecture) sur `len` octets ; `None` si
    /// Windows refuse ou si le fichier est vide.
    pub fn new(file: &File, len: usize) -> Option<Mapped> {
        if len == 0 {
            return None;
        }
        let handle = HANDLE(file.as_raw_handle());
        // SAFETY: `handle` est le descripteur ouvert de `file`, vivant pendant
        // l'appel ; taille 0 = tout le fichier ; sans nom ni attributs. Le
        // descripteur de projection rendu est fermé juste après la vue (la vue
        // garde la projection en vie, documenté par Windows).
        let mapping = unsafe { CreateFileMappingW(handle, None, PAGE_READONLY, 0, 0, windows::core::PCWSTR::null()) }.ok()?;
        // SAFETY: projection valide créée ci-dessus, accès en lecture seulement,
        // depuis le début, longueur 0 = jusqu'à la fin.
        let view = unsafe { MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, 0) };
        // SAFETY: descripteur rendu par CreateFileMappingW, fermé une seule fois.
        unsafe {
            let _ = CloseHandle(mapping);
        }
        if view.Value.is_null() {
            return None;
        }
        // La vue a la taille du fichier à l'instant de la projection, pas
        // forcément `len` (lu avant) : un fichier raccourci entre-temps aurait
        // donné une tranche plus longue que la vue (audit 0.1.33).
        let mut info = MEMORY_BASIC_INFORMATION::default();
        // SAFETY: `view` est une adresse valide de ce processus ; `info` est
        // un emplacement de la taille annoncée, rempli par Windows.
        let got = unsafe { VirtualQuery(Some(view.Value), &mut info, size_of::<MEMORY_BASIC_INFORMATION>()) };
        if got == 0 || info.RegionSize < len {
            // SAFETY: vue rendue par MapViewOfFile, démappée une seule fois (aucun `Mapped` n'est construit).
            unsafe {
                let _ = UnmapViewOfFile(view);
            }
            return None;
        }
        Some(Mapped { view, len })
    }
}

impl AsRef<[u8]> for Mapped {
    fn as_ref(&self) -> &[u8] {
        // SAFETY: `view` désigne au moins `len` octets lisibles (vérifié par
        // `VirtualQuery` à la création), valables jusqu'à `Drop` : Windows
        // refuse de tronquer un fichier projeté. Le contenu peut en revanche
        // être réécrit en place par un autre programme (polices du dossier de
        // l'utilisateur) : ce sont des octets lus, jamais des références.
        unsafe { std::slice::from_raw_parts(self.view.Value.cast::<u8>(), self.len) }
    }
}

impl Drop for Mapped {
    fn drop(&mut self) {
        // SAFETY: vue rendue par MapViewOfFile, démappée une seule fois, ici.
        unsafe {
            let _ = UnmapViewOfFile(self.view);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_a_whole_file_read_only() {
        let dir = std::env::temp_dir().join(format!("ferrocord-mapped-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("f.bin");
        std::fs::write(&path, b"\x00\x01\x00\x00police").unwrap();
        let file = File::open(&path).unwrap();
        let m = Mapped::new(&file, 10).expect("projection");
        drop(file);
        assert_eq!(m.as_ref(), b"\x00\x01\x00\x00police");
        drop(m);
        let empty = dir.join("vide.bin");
        std::fs::write(&empty, b"").unwrap();
        assert!(Mapped::new(&File::open(&empty).unwrap(), 0).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
