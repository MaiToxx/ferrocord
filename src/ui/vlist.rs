//! Géométrie de la liste de messages virtualisée, sans egui : positions
//! tirées des hauteurs (mesurées ou estimées), plage visible, et ancrage de
//! la vue sur un message pour qu'elle ne saute pas quand le contenu change
//! au-dessus d'elle (insertion en tête, suppression, hauteur corrigée).

use std::ops::Range;

/// Écart (points) en deçà duquel la vue est considérée comme collée en bas.
const STICK: f32 = 0.5;

/// Où la vue est accrochée d'une image à l'autre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    /// Collée en bas : un nouveau message reste visible.
    Bottom,
    /// Haut de la vue = haut du message `id` + `delta` (négatif dans l'en-tête).
    At { id: u64, delta: f32 },
}

/// Délai minimal (s) entre deux pages demandées automatiquement dans un même sens.
pub const AUTO_DELAY: f64 = 1.0;

/// Positions des éléments : `tops[i]` est le haut de l'élément `i`, `tops[n]`
/// la fin du dernier ; `head` (en-tête) les précède, `foot` puis `pad` les suivent.
#[derive(Clone, Debug, Default)]
pub struct Geometry {
    ids: Vec<u64>,
    heights: Vec<f32>,
    tops: Vec<f32>,
    head: f32,
    foot: f32,
    /// Espace vide sous le pied (voir `fit_pad`).
    pad: f32,
}

impl Geometry {
    /// `ids` croissants, une hauteur par identifiant.
    pub fn set(&mut self, ids: Vec<u64>, heights: Vec<f32>) {
        debug_assert_eq!(ids.len(), heights.len());
        self.ids = ids;
        self.heights = heights;
        self.retop(0);
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn ids(&self) -> &[u64] {
        &self.ids
    }

    pub fn height(&self, i: usize) -> f32 {
        self.heights[i]
    }

    pub fn top(&self, i: usize) -> f32 {
        self.tops[i]
    }

    pub fn head(&self) -> f32 {
        self.head
    }

    /// Change une hauteur ; seules les positions qui suivent sont refaites.
    pub fn set_height(&mut self, i: usize, h: f32) -> bool {
        if self.heights[i] == h {
            return false;
        }
        self.heights[i] = h;
        self.retop(i);
        true
    }

    pub fn set_head(&mut self, h: f32) {
        if self.head != h {
            self.head = h;
            self.retop(0);
        }
    }

    pub fn set_foot(&mut self, h: f32) {
        self.foot = h;
    }

    fn retop(&mut self, from: usize) {
        let n = self.ids.len();
        self.tops.resize(n + 1, 0.0);
        let mut y = if from == 0 { self.head } else { self.tops[from] };
        for i in from..n {
            self.tops[i] = y;
            y += self.heights[i];
        }
        self.tops[n] = y;
    }

    /// Hauteur totale du contenu.
    pub fn total(&self) -> f32 {
        self.content() + self.pad
    }

    fn content(&self) -> f32 {
        self.tops.last().copied().unwrap_or(self.head) + self.foot
    }

    /// Contenu qui dépasse à peine la vue (hauteur `view`) : collée en bas, la
    /// vue couperait l'en-tête ou le premier message à mi-hauteur. Un espace
    /// vide sous le dernier élément fait tomber le haut de la vue juste sous
    /// l'en-tête (ou sous le premier message), comme un contenu qui tient
    /// entier est aligné en haut avec du vide en dessous. Au plus un quart de
    /// vue ; au-delà, la coupure est celle d'un défilement ordinaire.
    pub fn fit_pad(&mut self, view: f32) {
        self.pad = 0.0;
        let over = self.content() - view;
        if over <= STICK {
            return;
        }
        let first_end = self.tops.get(1).copied().filter(|_| !self.ids.is_empty());
        for edge in std::iter::once(self.head).chain(first_end) {
            if over <= edge + STICK {
                let pad = (edge - over).max(0.0);
                if pad <= view / 4.0 {
                    self.pad = pad;
                }
                return;
            }
        }
    }

    pub fn max_offset(&self, view: f32) -> f32 {
        (self.total() - view).max(0.0)
    }

    /// Rang du premier identifiant `>= id` (le message lui-même, ou son
    /// successeur s'il a disparu).
    pub fn find(&self, id: u64) -> usize {
        self.ids.partition_point(|x| *x < id)
    }

    /// Élément qui contient l'ordonnée `y` (le premier si `y` est dans l'en-tête).
    pub fn index_at(&self, y: f32) -> Option<usize> {
        let n = self.ids.len();
        if n == 0 {
            return None;
        }
        Some(self.tops[1..=n].partition_point(|end| *end <= y).min(n - 1))
    }

    /// Décalage de la vue (hauteur `view`) qui respecte l'ancre.
    pub fn offset_for(&self, anchor: Anchor, view: f32) -> f32 {
        let max = self.max_offset(view);
        match anchor {
            Anchor::Bottom => max,
            Anchor::At { id, delta } => {
                let i = self.find(id);
                if i >= self.ids.len() {
                    return max;
                }
                // Message disparu : son successeur prend sa place, sans dépasser son haut.
                // Message raccourci (fenêtre élargie) : il reste au moins en partie visible.
                let delta = if self.ids[i] != id { delta.min(0.0) } else { delta.min((self.heights[i] - STICK).max(0.0)) };
                (self.tops[i] + delta).clamp(0.0, max)
            }
        }
    }

    /// Ancre correspondant au décalage `offset`. `stick` : la vue peut se
    /// coller en bas (faux quand le bas du cache n'est pas le présent).
    pub fn anchor_at(&self, offset: f32, view: f32, stick: bool) -> Anchor {
        if stick && offset >= self.max_offset(view) - STICK {
            return Anchor::Bottom;
        }
        match self.index_at(offset) {
            Some(i) => Anchor::At { id: self.ids[i], delta: offset - self.tops[i] },
            None => Anchor::Bottom,
        }
    }

    /// Éléments qui coupent `[from, to)`.
    pub fn range(&self, from: f32, to: f32) -> Range<usize> {
        let n = self.ids.len();
        if n == 0 || to <= from {
            return 0..0;
        }
        let start = self.tops[1..=n].partition_point(|end| *end <= from);
        let end = self.tops[..n].partition_point(|top| *top < to);
        start..end.max(start)
    }
}

/// Messages arrivés après `seen` (le plus récent déjà vu), `ids` croissants.
pub fn unseen(ids: &[u64], seen: u64) -> usize {
    ids.len() - ids.partition_point(|x| *x <= seen)
}

/// Demandes automatiques de pages dans un sens. Elles ne partent qu'après un
/// défilement de l'utilisateur vers ce bord (pas parce que la vue est en haut
/// d'une liste courte), une seule par défilement, et au plus une par
/// `AUTO_DELAY` même quand la barre de défilement est tenue contre le bord.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pacer {
    armed: bool,
    /// Instant (s) à partir duquel la prochaine demande peut partir.
    next: f64,
}

impl Pacer {
    /// `possible` : la vue est près du bord et une page peut être demandée ;
    /// `toward` : l'utilisateur fait défiler vers ce bord pendant cette image.
    pub fn update(&mut self, possible: bool, toward: bool) {
        self.armed = possible && (self.armed || toward);
    }

    /// Une demande peut partir à l'instant `now`.
    pub fn ready(&self, now: f64) -> bool {
        self.armed && now >= self.next
    }

    /// Armée mais retenue par le délai : secondes avant de pouvoir partir.
    pub fn wait(&self, now: f64) -> Option<f64> {
        (self.armed && now < self.next).then_some(self.next - now)
    }

    /// La demande est partie à `now`.
    pub fn sent(&mut self, now: f64) {
        self.armed = false;
        self.next = now + AUTO_DELAY;
    }
}

/// Faut-il demander la page précédente ? La vue arrive à moins d'une
/// hauteur d'écran du haut, il en reste, et aucune demande n'est en cours
/// ni en échec (un échec attend le bouton « Réessayer »).
pub fn wants_older(offset: f32, view: f32, more: bool, busy: bool, failed: bool) -> bool {
    more && !busy && !failed && offset < view
}

/// Faut-il demander la page suivante ? Même règle vers le bas, quand la
/// fenêtre du cache a quitté le présent.
pub fn wants_newer(offset: f32, view: f32, max: f32, newer: bool, busy: bool, failed: bool) -> bool {
    newer && !busy && !failed && offset > max - view
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geo(heights: &[f32], head: f32, foot: f32) -> Geometry {
        let mut g = Geometry::default();
        g.set_head(head);
        g.set_foot(foot);
        g.set((0..heights.len() as u64).map(|i| (i + 1) * 10).collect(), heights.to_vec());
        g
    }

    #[test]
    fn positions_and_total() {
        let g = geo(&[10.0, 20.0, 30.0], 5.0, 7.0);
        assert_eq!((g.top(0), g.top(1), g.top(2)), (5.0, 15.0, 35.0));
        assert_eq!(g.total(), 72.0);
        assert_eq!(g.max_offset(50.0), 22.0);
        assert_eq!(g.max_offset(500.0), 0.0);
        assert_eq!(Geometry::default().total(), 0.0);
    }

    /// Plage affichée pour un décalage donné : seulement ce qui coupe la vue.
    #[test]
    fn visible_range_for_offset() {
        let g = geo(&[10.0; 100], 0.0, 0.0);
        assert_eq!(g.range(0.0, 25.0), 0..3);
        assert_eq!(g.range(10.0, 30.0), 1..3);
        assert_eq!(g.range(15.0, 15.5), 1..2);
        assert_eq!(g.range(995.0, 2000.0), 99..100);
        assert_eq!(g.range(2000.0, 3000.0), 100..100);
        assert_eq!(g.range(5.0, 5.0), 0..0);
        assert_eq!(geo(&[], 3.0, 0.0).range(0.0, 10.0), 0..0);
        // Le nombre d'éléments mis en page ne dépend que de la hauteur de la vue.
        let big = geo(&[10.0; 5000], 0.0, 0.0);
        assert_eq!(big.range(20_000.0, 20_100.0).len(), 10);
    }

    #[test]
    fn index_at_and_anchor_roundtrip() {
        let g = geo(&[10.0, 20.0, 30.0], 5.0, 0.0);
        assert_eq!(g.index_at(0.0), Some(0));
        assert_eq!(g.index_at(14.9), Some(0));
        assert_eq!(g.index_at(15.0), Some(1));
        assert_eq!(g.index_at(1000.0), Some(2));
        for offset in [0.0, 3.0, 5.0, 16.5, 30.0] {
            let a = g.anchor_at(offset, 20.0, true);
            assert_eq!(g.offset_for(a, 20.0), offset, "{a:?}");
        }
        // Dans l'en-tête : ancre sur le premier message, décalage négatif.
        assert_eq!(g.anchor_at(1.0, 20.0, true), Anchor::At { id: 10, delta: -4.0 });
        // En bas : collée.
        assert_eq!(g.anchor_at(g.max_offset(20.0), 20.0, true), Anchor::Bottom);
        assert_eq!(g.anchor_at(g.max_offset(20.0), 20.0, false), Anchor::At { id: 30, delta: 10.0 });
    }

    /// Insertion en tête (historique plus ancien) : le message regardé reste
    /// exactement au même endroit de l'écran.
    #[test]
    fn prepend_keeps_anchor_in_place() {
        let mut g = geo(&[10.0, 20.0, 30.0], 4.0, 0.0);
        let offset = 22.0;
        let a = g.anchor_at(offset, 10.0, true);
        let screen = g.top(1) - offset;
        g.set(vec![1, 2, 3, 10, 20, 30], vec![50.0, 60.0, 70.0, 10.0, 20.0, 30.0]);
        let new = g.offset_for(a, 10.0);
        assert_eq!(g.top(g.find(20)) - new, screen);
        assert_eq!(new, offset + 180.0);
    }

    /// Suppression ou hauteur changée au-dessus de la vue : pas de saut.
    #[test]
    fn changes_above_do_not_move_view() {
        let mut g = geo(&[10.0, 20.0, 30.0, 40.0], 0.0, 0.0);
        let a = g.anchor_at(35.0, 10.0, true);
        assert_eq!(a, Anchor::At { id: 30, delta: 5.0 });
        g.set_height(0, 100.0);
        assert_eq!(g.offset_for(a, 10.0) - g.top(2), 5.0);
        g.set(vec![20, 30, 40], vec![20.0, 30.0, 40.0]);
        assert_eq!(g.offset_for(a, 10.0), 25.0);
        assert_eq!(g.offset_for(a, 10.0) - g.top(g.find(30)), 5.0);
        // Le message ancré raccourcit (fenêtre élargie) : il reste visible en haut.
        let short = Anchor::At { id: 30, delta: 25.0 };
        g.set_height(1, 10.0);
        let off = g.offset_for(short, 10.0);
        assert!(off < g.top(1) + g.height(1) && off >= g.top(1), "{off}");
        g.set_height(1, 30.0);
        // L'en-tête qui grandit (indicateur de chargement) non plus.
        g.set_head(30.0);
        assert_eq!(g.offset_for(a, 10.0) - g.top(g.find(30)), 5.0);
    }

    /// Le message ancré disparaît : son successeur prend sa place.
    #[test]
    fn deleted_anchor_falls_to_successor() {
        let mut g = geo(&[10.0, 20.0, 30.0], 0.0, 0.0);
        let a = Anchor::At { id: 20, delta: 5.0 };
        g.set(vec![10, 30], vec![10.0, 30.0]);
        assert_eq!(g.offset_for(a, 5.0), 10.0);
        // Plus aucun successeur : en bas.
        g.set(vec![10], vec![10.0]);
        assert_eq!(g.offset_for(a, 5.0), g.max_offset(5.0));
    }

    /// Collée en bas, un nouveau message garde la vue en bas ; décollée, la
    /// vue ne bouge pas.
    #[test]
    fn bottom_follows_new_messages_only_when_stuck() {
        let mut g = geo(&[10.0; 10], 0.0, 2.0);
        let stuck = g.anchor_at(g.max_offset(30.0), 30.0, true);
        let up = g.anchor_at(40.0, 30.0, true);
        let mut ids: Vec<u64> = g.ids().to_vec();
        ids.push(1000);
        g.set(ids, vec![10.0; 11]);
        assert_eq!(g.offset_for(stuck, 30.0), g.max_offset(30.0));
        assert_eq!(g.offset_for(up, 30.0), 40.0);
    }

    /// Une hauteur estimée puis mesurée ne bouge que ce qui suit.
    #[test]
    fn set_height_updates_following_tops_only() {
        let mut g = geo(&[10.0, 10.0, 10.0], 0.0, 0.0);
        assert!(g.set_height(1, 25.0));
        assert!(!g.set_height(1, 25.0));
        assert_eq!((g.top(0), g.top(1), g.top(2), g.total()), (0.0, 10.0, 35.0, 45.0));
    }

    #[test]
    fn unseen_counter() {
        let ids = [1, 5, 9, 12];
        assert_eq!(unseen(&ids, 12), 0);
        assert_eq!(unseen(&ids, 5), 2);
        assert_eq!(unseen(&ids, 0), 4);
        assert_eq!(unseen(&ids, 6), 2);
        assert_eq!(unseen(&[], 6), 0);
    }

    /// Barre de défilement tenue contre le bord pendant 30 s (60 images/s),
    /// chaque page servie aussitôt : au plus une demande par seconde.
    #[test]
    fn pacer_limits_held_scrollbar_to_one_page_per_second() {
        let mut p = Pacer::default();
        let mut sent = 0;
        for frame in 0..30 * 60 {
            let now = f64::from(frame) / 60.0;
            p.update(true, true);
            if p.ready(now) {
                p.sent(now);
                sent += 1;
            }
        }
        assert!((29..=31).contains(&sent), "{sent}");
    }

    /// Vue au bord d'une liste courte sans défilement : rien. Un défilement :
    /// une seule demande ; la suivante attend un nouveau défilement.
    #[test]
    fn pacer_needs_a_scroll_toward_the_edge() {
        let mut p = Pacer::default();
        for frame in 0..600 {
            p.update(true, false);
            assert!(!p.ready(f64::from(frame) / 60.0));
        }
        p.update(true, true);
        assert!(p.ready(10.0));
        p.sent(10.0);
        p.update(true, false);
        assert!(!p.ready(20.0), "une seule demande par défilement");
        // Défilement pendant le délai : attend la fin du délai, sans nouveau geste.
        p.update(true, true);
        assert_eq!(p.wait(20.0), None, "délai déjà écoulé");
        let mut q = Pacer::default();
        q.update(true, true);
        q.sent(0.0);
        q.update(true, true);
        assert!(!q.ready(0.5));
        assert!(q.wait(0.5).is_some_and(|w| (w - 0.5).abs() < 1e-9));
        q.update(true, false);
        assert!(q.ready(1.0));
        // La vue quitte le bord (ou une demande est en cours) : désarmée.
        q.update(false, false);
        q.update(true, false);
        assert!(!q.ready(5.0));
    }

    /// Contenu qui dépasse à peine : le haut de la vue tombe sous l'en-tête ou
    /// sous le premier message, jamais au milieu ; sinon, aucun bourrage.
    #[test]
    fn fit_pad_aligns_barely_overflowing_content() {
        // En-tête 40, messages 30 + 30 + 30 : contenu 130.
        let mut g = geo(&[30.0, 30.0, 30.0], 40.0, 0.0);
        g.fit_pad(110.0);
        // Dépasse de 20 (dans l'en-tête) : vide de 20, la vue commence à 40.
        assert_eq!(g.max_offset(110.0), 40.0);
        assert_eq!(g.offset_for(Anchor::Bottom, 110.0), g.top(0));
        g.fit_pad(90.0);
        // Dépasse de 40 : juste l'en-tête, aucun vide.
        assert_eq!(g.max_offset(90.0), 40.0);
        g.fit_pad(75.0);
        // Dépasse de 55 (dans le premier message) : la vue commence sous lui.
        assert_eq!(g.max_offset(75.0), g.top(1));
        // Le vide dépasserait un quart de vue : défilement ordinaire.
        let mut tall = geo(&[200.0, 30.0], 40.0, 0.0);
        tall.fit_pad(225.0);
        assert_eq!(tall.max_offset(225.0), 45.0);
        // Contenu qui tient ou qui dépasse beaucoup : rien.
        g.fit_pad(500.0);
        assert_eq!(g.total(), 130.0);
        g.fit_pad(20.0);
        assert_eq!(g.total(), 130.0);
    }

    #[test]
    fn history_triggers() {
        assert!(wants_older(10.0, 400.0, true, false, false));
        assert!(!wants_older(500.0, 400.0, true, false, false));
        assert!(!wants_older(10.0, 400.0, false, false, false), "début du salon atteint");
        assert!(!wants_older(10.0, 400.0, true, true, false), "une demande à la fois");
        assert!(!wants_older(10.0, 400.0, true, false, true), "échec : on attend Réessayer");
        assert!(wants_newer(900.0, 400.0, 1000.0, true, false, false));
        assert!(!wants_newer(100.0, 400.0, 1000.0, true, false, false));
        assert!(!wants_newer(900.0, 400.0, 1000.0, false, false, false));
    }
}
