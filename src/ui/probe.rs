//! Accès réservés à l'aperçu hors ligne et à son banc de mesure
//! (`examples/preview.rs --bench`) : ouvrir un salon sans clic, lire le
//! magasin et l'état des images. Tous en lecture, sauf l'ouverture d'un
//! salon (le même chemin qu'un clic) : l'aperçu ne modifie jamais le magasin
//! de l'interface directement. L'exécutable ne lit que le magasin, et
//! seulement avec `FERROCORD_PERF=1` (`crate::perf`).

use super::App;
use crate::images::Stats;
use crate::state::Store;

impl App {
    /// Ouvre `channel` (dans `guild`, ou parmi les MP), comme un clic dans la liste.
    pub fn show_channel(&mut self, guild: Option<u64>, channel: u64) {
        match guild {
            Some(g) => {
                self.store.select_guild(g);
            }
            None => self.store.select_dms(),
        }
        self.open_channel(channel);
    }

    /// Magasin lu par l'interface (mesure de sa taille, position affichée).
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// État du pipeline d'images (fin du préchauffage du banc).
    pub fn image_stats(&self) -> Stats {
        self.images.stats()
    }

    /// Mises en page de messages gardées d'une image à l'autre : (nombre, octets).
    pub fn text_layouts(&self) -> (usize, usize) {
        super::text_cache::stats(&self.ctx)
    }
}
