//! Vidéo d'un appel (phase 5, lot M4) : qui a sa caméra ou un direct
//! (d'après `self_video` et `self_stream` de [`super::voice::VoiceStates`]),
//! ma caméra et mon partage, les directs que je regarde, la tuile mise en
//! avant et les tuiles détachées, et le passage des images vers
//! l'interface ([`VideoSink`]).
//!
//! Aucune capture, aucun codec, aucun réseau vidéo ici : le câblage (M5)
//! pousse les images décodées ou capturées dans [`VideoSink::push`] et lit
//! [`VideoSink::wanted_quality`] pour demander la bonne définition.

use super::stream::{parse_key, stream_key};
use super::voice::VoiceState;
use super::{Cmd, Store};
use crate::video::{Frame, FrameData, Quality};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Une tuile de la vue d'appel.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Tile {
    /// Une personne : sa caméra, ou son avatar quand elle est éteinte.
    Camera(u64),
    /// Un direct, par sa clé (`state::stream`).
    Stream(String),
}

/// Interface -> réseau (et câblage vidéo, lot M5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VideoCmd {
    /// Op 4 avec `self_video`.
    SetCamera(bool),
    /// Op 18. `stream_key`, `guild_id`, `channel_id` : calculés par le
    /// magasin (la passerelle s'en sert pour l'op 18 puis l'op 19).
    StartShare { source_id: String, quality: Quality, audio: bool, stream_key: String, guild_id: Option<u64>, channel_id: u64 },
    /// Op 19 sur mon direct.
    StopShare,
    /// Op 20.
    Watch { stream_key: String },
    /// Op 19 sur un direct regardé.
    StopWatching(String),
    /// Purement local (qualité demandée, lot M5) : rien ne part sur le réseau.
    Focus(Option<Tile>),
    PopOut(Tile),
}

/// Ce que je partage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Share {
    pub source_id: String,
    /// Nom montré dans le bandeau (« Écran 1 », titre de la fenêtre).
    pub name: String,
    pub quality: Quality,
    pub audio: bool,
    pub key: String,
    /// Dernière mesure de ce qui est envoyé (`None` : pas encore mesuré).
    pub stats: Option<super::video_link::SendStats>,
}

/// Une tuile de la grille, et ce qu'elle montre.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TileInfo {
    pub tile: Tile,
    pub user_id: u64,
    pub kind: TileKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileKind {
    Person { camera: bool },
    /// `watching` : je le regarde (ou c'est le mien) : des images arrivent.
    Stream { watching: bool, mine: bool },
}

#[derive(Default)]
pub struct VideoStates {
    /// Ma caméra demandée (op 4 envoyée).
    pub camera: bool,
    /// Caméra choisie dans le sélecteur (`None` : la première de Windows).
    pub camera_source: Option<String>,
    pub share: Option<Share>,
    /// Directs regardés (op 20 envoyée).
    watching: Vec<String>,
    /// Tuile mise en avant ; les autres passent dans la bande.
    pub focus: Option<Tile>,
    /// Tuiles détachées dans leur propre fenêtre.
    pub popped: Vec<Tile>,
    /// Images vers l'interface (partagé avec le câblage).
    pub sink: VideoSink,
}

impl VideoStates {
    /// Nouvelle session ou appel quitté : tout s'arrête (la file d'images reste).
    pub fn clear(&mut self) {
        let sink = self.sink.clone();
        sink.forget_all();
        *self = VideoStates { sink, ..Default::default() };
    }

    pub fn watching(&self, key: &str) -> bool {
        self.watching.iter().any(|k| k == key)
    }

    pub fn watched(&self) -> &[String] {
        &self.watching
    }

    /// Caméra ou partage actifs : l'indicateur reste affiché.
    pub fn sending(&self) -> bool {
        self.camera || self.share.is_some()
    }
}

impl Store {
    /// Tuiles d'un salon vocal ou d'un appel : une par personne, puis une par
    /// direct, dans l'ordre d'arrivée.
    pub fn call_tiles(&self, channel_id: u64) -> Vec<TileInfo> {
        let me = self.me.as_ref().map(|m| m.id);
        let v = &self.video;
        let members: Vec<&VoiceState> = self.voice.members(channel_id).collect();
        let mut out: Vec<TileInfo> = members
            .iter()
            .map(|s| {
                let camera = s.self_video || (Some(s.user_id) == me && v.camera);
                TileInfo { tile: Tile::Camera(s.user_id), user_id: s.user_id, kind: TileKind::Person { camera } }
            })
            .collect();
        for s in &members {
            let mine = Some(s.user_id) == me;
            let key = stream_key(s.guild_id, channel_id, s.user_id);
            let live = if mine { v.share.as_ref().is_some_and(|sh| sh.key == key) } else { s.self_stream };
            if live {
                let watching = mine || v.watching(&key);
                out.push(TileInfo { tile: Tile::Stream(key), user_id: s.user_id, kind: TileKind::Stream { watching, mine } });
            }
        }
        out
    }

    /// Allume ou éteint ma caméra (dans un salon vocal ou un appel seulement).
    pub fn video_set_camera(&mut self, on: bool) -> bool {
        if self.voice.me.is_none() || self.video.camera == on {
            return false;
        }
        self.video.camera = on;
        self.push_command(Cmd::Video(VideoCmd::SetCamera(on)));
        true
    }

    /// Démarre un partage choisi dans le sélecteur (lot M3). Un partage en
    /// cours dans le même salon change de source, de qualité ou de son sans
    /// s'arrêter (F23) : le direct reste ouvert, rien ne part sur la passerelle.
    pub fn video_start_share(&mut self, source_id: &str, name: &str, quality: Quality, audio: bool) -> bool {
        let (Some(my), Some(me)) = (self.voice.me, self.me.as_ref().map(|m| m.id)) else { return false };
        let key = stream_key(my.guild_id, my.channel_id, me);
        if let Some(sh) = self.video.share.as_mut().filter(|sh| sh.key == key) {
            (sh.source_id, sh.name, sh.quality, sh.audio) = (source_id.to_string(), name.to_string(), quality, audio);
            sh.stats = None;
            return true;
        }
        if self.video.share.is_some() {
            self.video_stop_share();
        }
        self.video.share = Some(Share { source_id: source_id.to_string(), name: name.to_string(), quality, audio, key: key.clone(), stats: None });
        self.stream.request(&key, true);
        self.push_command(Cmd::Video(VideoCmd::StartShare { source_id: source_id.to_string(), quality, audio, stream_key: key, guild_id: my.guild_id, channel_id: my.channel_id }));
        true
    }

    pub fn video_stop_share(&mut self) {
        let Some(sh) = self.video.share.take() else { return };
        self.stream.forget(&sh.key);
        self.video_untile(&Tile::Stream(sh.key));
        self.push_command(Cmd::Video(VideoCmd::StopShare));
    }

    /// « Regarder » un direct de mon salon.
    pub fn video_watch(&mut self, key: &str) -> bool {
        let Some(parts) = parse_key(key) else { return false };
        let in_my_call = self.voice.me.is_some_and(|m| m.channel_id == parts.channel_id);
        let live = self.voice.state(parts.owner).is_some_and(|s| s.self_stream && s.channel_id == parts.channel_id);
        if !in_my_call || !live || self.video.watching(key) || self.me.as_ref().is_some_and(|m| m.id == parts.owner) {
            return false;
        }
        self.video.watching.push(key.to_string());
        self.stream.request(key, false);
        self.push_command(Cmd::Video(VideoCmd::Watch { stream_key: key.to_string() }));
        true
    }

    pub fn video_stop_watching(&mut self, key: &str) {
        if !self.video.watching(key) {
            return;
        }
        self.video.watching.retain(|k| k != key);
        self.stream.forget(key);
        self.video_untile(&Tile::Stream(key.to_string()));
        self.push_command(Cmd::Video(VideoCmd::StopWatching(key.to_string())));
    }

    /// Met une tuile en avant (`None` : retour à la grille).
    pub fn video_focus(&mut self, tile: Option<Tile>) {
        if self.video.focus == tile {
            return;
        }
        self.video.focus = tile.clone();
        self.push_command(Cmd::Video(VideoCmd::Focus(tile)));
    }

    /// Détache une tuile dans sa propre fenêtre.
    pub fn video_pop_out(&mut self, tile: Tile) {
        if self.video.popped.contains(&tile) {
            return;
        }
        self.video.popped.push(tile.clone());
        self.push_command(Cmd::Video(VideoCmd::PopOut(tile)));
    }

    /// Fenêtre détachée fermée : la tuile revient dans la vue.
    pub fn video_pop_in(&mut self, tile: &Tile) {
        self.video.popped.retain(|t| t != tile);
    }

    fn video_untile(&mut self, tile: &Tile) {
        if self.video.focus.as_ref() == Some(tile) {
            self.video.focus = None;
        }
        self.video.popped.retain(|t| t != tile);
    }

    /// Après chaque événement vocal : hors appel, tout s'arrête ; un direct
    /// dont le propriétaire n'est plus en direct n'est plus regardé.
    pub(super) fn video_follow_voice(&mut self) {
        let Some(my) = self.voice.me else {
            if self.video.sending() || !self.video.watching.is_empty() || self.video.focus.is_some() || !self.video.popped.is_empty() {
                self.video.clear();
                self.stream.clear();
            }
            return;
        };
        let gone: Vec<String> = self
            .video
            .watching
            .iter()
            .filter(|k| parse_key(k).is_none_or(|p| p.channel_id != my.channel_id || !self.voice.state(p.owner).is_some_and(|s| s.self_stream && s.channel_id == p.channel_id)))
            .cloned()
            .collect();
        for k in gone {
            self.video.watching.retain(|x| *x != k);
            self.stream.forget(&k);
            self.video_untile(&Tile::Stream(k));
        }
        // Mon partage était dans un autre salon : Discord l'a fermé avec mon départ.
        if self.video.share.as_ref().and_then(|s| parse_key(&s.key)).is_some_and(|p| p.channel_id != my.channel_id)
            && let Some(sh) = self.video.share.take()
        {
            self.stream.forget(&sh.key);
            self.video_untile(&Tile::Stream(sh.key));
        }
        // Tuile d'une personne partie.
        if let Some(Tile::Camera(u)) = self.video.focus
            && !self.voice.in_channel(u, my.channel_id)
        {
            self.video.focus = None;
        }
    }

    /// Direct fermé par Discord (STREAM_DELETE).
    pub(super) fn video_stream_ended(&mut self, key: &str, mine: bool) {
        if mine {
            if self.video.share.as_ref().is_some_and(|s| s.key == key) {
                self.video.share = None;
            }
        } else {
            self.video.watching.retain(|k| k != key);
        }
        self.video_untile(&Tile::Stream(key.to_string()));
    }

    pub(super) fn apply_stream(&mut self, e: super::stream::StreamEvent) {
        if let Some((key, mine)) = self.stream.apply(e) {
            self.video_stream_ended(&key, mine);
        }
    }
}

/// Réveil de l'interface, appelable depuis n'importe quel fil.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Image prête pour une texture : RGBA, alpha opaque, sans bourrage.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    /// Un mot par pixel, soit ses quatre octets dans l'ordre rouge, vert,
    /// bleu, alpha : l'interface en fait une image d'egui sans le recopier
    /// (voir `convert::pixel_bytes`).
    pub pixels: Vec<u32>,
}

impl Rgba {
    /// Les octets de l'image, quatre par pixel.
    pub fn bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.pixels)
    }
}

#[derive(Default)]
struct Slot {
    /// Dessinée à la dernière image de l'interface.
    visible: bool,
    /// Taille affichée, en pixels physiques.
    shown: (u32, u32),
    generation: u64,
    image: Option<Rgba>,
    /// Tampon rendu par l'interface, réutilisé pour l'image suivante.
    spare: Vec<u32>,
}

#[derive(Default)]
struct SinkInner {
    slots: HashMap<Tile, Slot>,
    wake: Option<Wake>,
}

/// File d'images vers l'interface : une seule image en attente par tuile
/// (la plus récente), seulement pour les tuiles affichées. Une image d'une
/// tuile hors champ est jetée sans être convertie ; la conversion et la
/// réduction à la taille affichée se font sur le fil qui pousse (jamais
/// sur celui de l'interface).
#[derive(Clone, Default)]
pub struct VideoSink {
    inner: Arc<Mutex<SinkInner>>,
}

/// Définitions demandées aux autres selon la taille d'une tuile (16:9).
const LADDER: [(u32, u32); 6] = [(320, 180), (480, 270), (640, 360), (960, 540), (1280, 720), (1920, 1080)];

impl VideoSink {
    fn lock(&self) -> MutexGuard<'_, SinkInner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn set_wake(&self, wake: Wake) {
        self.lock().wake = Some(wake);
    }

    /// Nouvelle image pour cette tuile (fil du décodeur ou de la capture).
    pub fn push(&self, tile: Tile, frame: &Frame) {
        let (shown, mut buf) = {
            let mut g = self.lock();
            let Some(slot) = g.slots.get_mut(&tile).filter(|s| s.visible) else { return };
            (slot.shown, std::mem::take(&mut slot.spare))
        };
        let (w, h) = fit((frame.width, frame.height), shown);
        if !to_rgba(frame, w, h, &mut buf) {
            return;
        }
        let wake = {
            let mut g = self.lock();
            let wake = g.wake.clone();
            let Some(slot) = g.slots.get_mut(&tile).filter(|s| s.visible) else { return };
            if let Some(old) = slot.image.replace(Rgba { width: w, height: h, pixels: buf }) {
                slot.spare = old.pixels;
            }
            slot.generation += 1;
            wake
        };
        if let Some(w) = wake {
            w();
        }
    }

    /// Image plus récente que `seen` (génération), sortie de la file.
    pub fn take(&self, tile: &Tile, seen: u64) -> Option<(u64, Rgba)> {
        let mut g = self.lock();
        let slot = g.slots.get_mut(tile)?;
        if slot.generation <= seen {
            return None;
        }
        Some((slot.generation, slot.image.take()?))
    }

    /// Tampon d'une image téléversée, réutilisé pour la suivante.
    pub fn give_back(&self, tile: &Tile, pixels: Vec<u32>) {
        if let Some(slot) = self.lock().slots.get_mut(tile).filter(|s| s.visible) {
            slot.spare = pixels;
        }
    }

    /// Pixels du tampon en réserve pour la prochaine conversion de cette tuile.
    #[cfg(test)]
    pub fn spare_capacity(&self, tile: &Tile) -> usize {
        self.lock().slots.get(tile).map_or(0, |s| s.spare.capacity())
    }

    /// Tuiles dessinées à cette image et leur taille en pixels physiques ;
    /// les autres ne reçoivent plus rien et leur mémoire est rendue.
    pub fn show(&self, tiles: &[(Tile, (u32, u32))]) {
        let mut g = self.lock();
        g.slots.retain(|t, _| tiles.iter().any(|(x, _)| x == t));
        for (t, size) in tiles {
            let slot = g.slots.entry(t.clone()).or_default();
            slot.visible = true;
            slot.shown = *size;
        }
    }

    pub fn forget_all(&self) {
        self.lock().slots.clear();
    }

    /// Définition à demander pour cette tuile (flux reçus, lot M5) : la plus
    /// petite de l'échelle qui couvre la taille affichée ; `(0, 0)` pour une
    /// tuile hors champ (ne rien demander).
    pub fn wanted_quality(&self, tile: &Tile) -> (u32, u32) {
        let g = self.lock();
        match g.slots.get(tile).filter(|s| s.visible) {
            Some(s) => ladder(s.shown),
            None => (0, 0),
        }
    }

    /// Tuile affichée en ce moment.
    pub fn is_visible(&self, tile: &Tile) -> bool {
        self.lock().slots.get(tile).is_some_and(|s| s.visible)
    }
}

pub fn ladder((w, h): (u32, u32)) -> (u32, u32) {
    LADDER.iter().copied().find(|(lw, lh)| *lw >= w && *lh >= h).unwrap_or(LADDER[LADDER.len() - 1])
}

/// Taille de sortie : la source, réduite (jamais agrandie) pour tenir dans
/// `shown` en gardant ses proportions.
fn fit((sw, sh): (u32, u32), (mw, mh): (u32, u32)) -> (u32, u32) {
    if sw == 0 || sh == 0 || mw == 0 || mh == 0 || (sw <= mw && sh <= mh) {
        return (sw, sh);
    }
    let s = (f64::from(mw) / f64::from(sw)).min(f64::from(mh) / f64::from(sh));
    (((f64::from(sw) * s).round() as u32).max(1), ((f64::from(sh) * s).round() as u32).max(1))
}

/// Convertit `f` en RGBA `w`×`h` (plus proche voisin) dans `out` ; `false`
/// si l'image est incohérente (plans trop courts).
///
/// C'est le coût d'un direct regardé : une fois par image reçue. La boucle
/// d'origine, pixel par pixel avec une division et un `extend_from_slice`
/// chacun, prenait 22 ms pour une image de 1080p (57 % d'un cœur mesurés à
/// 30 images par seconde) ; celles de `convert`, vectorisées, 0,3 ms.
fn to_rgba(f: &Frame, w: u32, h: u32, out: &mut Vec<u32>) -> bool {
    use crate::video::convert::{I420, bgra_to_rgba_into, i420_to_rgba_into, pixel_bytes};
    let (sw, sh) = (f.width as usize, f.height as usize);
    let (w, h) = (w as usize, h as usize);
    let bytes = pixel_bytes(out, w * h);
    match &f.data {
        FrameData::Bgra(b) => bgra_to_rgba_into(sw, sh, b, w, h, bytes).is_ok(),
        // BT.601, plage réduite (ce que donnent les décodeurs H.264 courants).
        FrameData::I420 { y, u, v } => i420_to_rgba_into(I420 { width: sw, height: sh, y, u, v }, w, h, bytes).is_ok(),
    }
}

/// Mire animée (aperçu et tests) : bandes de couleur qui défilent avec
/// `step`, et un carré qui se déplace ; `seed` change les teintes.
pub fn test_pattern(width: u32, height: u32, step: u32, seed: u64) -> Frame {
    let (w, h) = (width.max(2) as usize, height.max(2) as usize);
    let mut px = Vec::with_capacity(w * h * 4);
    let bars: [[u8; 3]; 7] = [[192, 192, 192], [192, 192, 0], [0, 192, 192], [0, 192, 0], [192, 0, 192], [192, 0, 0], [0, 0, 192]];
    let shift = (seed % 7) as usize;
    let side = (h / 4).max(1);
    let bx = (step as usize * 3) % w.saturating_sub(side).max(1);
    let by = h / 2 - side / 2;
    for y in 0..h {
        for x in 0..w {
            let bar = ((x + step as usize * 2) * 7 / w + shift) % 7;
            let [r, g, b] = if (bx..bx + side).contains(&x) && (by..by + side).contains(&y) { [240, 240, 240] } else { bars[bar] };
            px.extend_from_slice(&[b, g, r, 255]);
        }
    }
    Frame { width: w as u32, height: h as u32, pts_us: u64::from(step) * 33_333, data: FrameData::Bgra(px) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::{Avatar, User};
    use crate::state::stream::{EndReason, StreamConnState, StreamEvent};
    use crate::state::voice::{VoiceCmd, VoiceEvent};
    use crate::state::Event;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const ME: u64 = 1;
    const BOB: u64 = 2;
    const CAROL: u64 = 3;
    const G: u64 = 100;
    const VOC: u64 = 501;
    const OTHER: u64 = 502;

    fn state(user_id: u64, channel_id: u64, video: bool, stream: bool) -> VoiceState {
        VoiceState { user_id, guild_id: Some(G), channel_id, self_video: video, self_stream: stream, ..Default::default() }
    }

    fn upd(s: &mut Store, st: VoiceState) {
        s.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(G), user_id: st.user_id, state: Some(st), session_id: None }));
    }

    fn store() -> Store {
        let mut s = Store::new();
        s.apply(Event::Ready { me: User { id: ME, name: "moi".into(), avatar: Avatar::default_for(ME) }, guilds: vec![], dms: vec![] });
        s.apply(Event::Voice(VoiceEvent::States(vec![state(BOB, VOC, true, false), state(CAROL, VOC, false, true)])));
        s.take_commands();
        s
    }

    fn in_call() -> Store {
        let mut s = store();
        s.voice_join_call(VOC);
        upd(&mut s, VoiceState { guild_id: Some(G), ..state(ME, VOC, false, false) });
        s.take_commands();
        s
    }

    fn video_cmds(s: &mut Store) -> Vec<VideoCmd> {
        s.take_commands().into_iter().filter_map(|c| if let Cmd::Video(v) = c { Some(v) } else { None }).collect()
    }

    #[test]
    fn tiles_follow_voice_states() {
        let s = in_call();
        let tiles = s.call_tiles(VOC);
        let carol_key = stream_key(Some(G), VOC, CAROL);
        assert_eq!(
            tiles,
            vec![
                TileInfo { tile: Tile::Camera(BOB), user_id: BOB, kind: TileKind::Person { camera: true } },
                TileInfo { tile: Tile::Camera(CAROL), user_id: CAROL, kind: TileKind::Person { camera: false } },
                TileInfo { tile: Tile::Camera(ME), user_id: ME, kind: TileKind::Person { camera: false } },
                TileInfo { tile: Tile::Stream(carol_key), user_id: CAROL, kind: TileKind::Stream { watching: false, mine: false } },
            ]
        );
    }

    #[test]
    fn camera_needs_a_call_and_is_sent_once() {
        let mut s = store();
        assert!(!s.video_set_camera(true), "hors appel");
        let mut s = in_call();
        assert!(s.video_set_camera(true));
        assert!(!s.video_set_camera(true));
        assert_eq!(video_cmds(&mut s), vec![VideoCmd::SetCamera(true)]);
        assert!(s.call_tiles(VOC).iter().any(|t| t.tile == Tile::Camera(ME) && t.kind == TileKind::Person { camera: true }));
        assert!(s.video.sending());
        // Quitter l'appel éteint tout.
        s.voice_leave();
        assert!(!s.video.camera && !s.video.sending());
        assert_eq!(s.take_commands(), vec![Cmd::Voice(VoiceCmd::Leave)]);
    }

    #[test]
    fn share_creates_my_stream_and_stop_deletes_it() {
        let mut s = in_call();
        let q = Quality { width: 1280, height: 720, fps: 30 };
        assert!(s.video_start_share("ecran-1", "Écran 1", q, true));
        let key = stream_key(Some(G), VOC, ME);
        assert_eq!(video_cmds(&mut s), vec![VideoCmd::StartShare { source_id: "ecran-1".into(), quality: q, audio: true, stream_key: key.clone(), guild_id: Some(G), channel_id: VOC }]);
        assert_eq!(s.stream.get(&key).map(|c| (c.mine, &c.state)), Some((true, &StreamConnState::Requested)));
        assert!(s.call_tiles(VOC).iter().any(|t| t.tile == Tile::Stream(key.clone()) && t.kind == TileKind::Stream { watching: true, mine: true }));
        s.video_focus(Some(Tile::Stream(key.clone())));
        s.video_stop_share();
        assert_eq!(video_cmds(&mut s), vec![VideoCmd::Focus(Some(Tile::Stream(key.clone()))), VideoCmd::StopShare]);
        assert!(s.video.share.is_none() && s.video.focus.is_none() && s.stream.get(&key).is_none());
        // Discord ferme mon direct : le bandeau disparaît.
        s.video_start_share("ecran-1", "Écran 1", q, false);
        s.apply(Event::Stream(StreamEvent::Delete { key: key.clone(), reason: EndReason::StreamEnded }));
        assert!(s.video.share.is_none());
    }

    /// Autre source pendant un partage : le direct reste, rien ne part sur la passerelle (F23).
    #[test]
    fn changing_source_keeps_the_stream_open() {
        let mut s = in_call();
        let q = Quality { width: 1280, height: 720, fps: 30 };
        assert!(s.video_start_share("ecran-1", "Écran 1", q, true));
        let key = stream_key(Some(G), VOC, ME);
        video_cmds(&mut s);
        assert!(s.video_start_share("fenetre-7", "Bloc-notes", Quality::SOURCE_60, false));
        assert!(video_cmds(&mut s).is_empty(), "ni arrêt ni nouveau départ");
        let sh = s.video.share.clone().unwrap();
        assert_eq!((sh.source_id.as_str(), sh.name.as_str(), sh.quality, sh.audio, &sh.key), ("fenetre-7", "Bloc-notes", Quality::SOURCE_60, false, &key));
        assert_eq!(s.stream.get(&key).map(|c| c.mine), Some(true));
    }

    #[test]
    fn watching_needs_a_live_stream_in_my_call() {
        let mut s = in_call();
        let carol = stream_key(Some(G), VOC, CAROL);
        assert!(!s.video_watch(&stream_key(Some(G), VOC, BOB)), "Bob n'est pas en direct");
        assert!(!s.video_watch("n'importe quoi"));
        assert!(s.video_watch(&carol));
        assert!(!s.video_watch(&carol), "déjà regardé");
        assert_eq!(video_cmds(&mut s), vec![VideoCmd::Watch { stream_key: carol.clone() }]);
        assert!(s.call_tiles(VOC).iter().any(|t| t.kind == TileKind::Stream { watching: true, mine: false }));
        // Carol arrête son direct : on ne le regarde plus.
        upd(&mut s, state(CAROL, VOC, false, false));
        assert!(!s.video.watching(&carol) && s.stream.get(&carol).is_none());
        upd(&mut s, state(CAROL, VOC, false, true));
        assert!(s.video_watch(&carol));
        s.video_stop_watching(&carol);
        assert_eq!(video_cmds(&mut s), vec![VideoCmd::Watch { stream_key: carol.clone() }, VideoCmd::StopWatching(carol.clone())]);
        // Direct plein : la fin est annoncée une fois.
        s.video_watch(&carol);
        s.apply(Event::Stream(StreamEvent::Delete { key: carol.clone(), reason: EndReason::Full }));
        assert!(!s.video.watching(&carol));
        assert_eq!(s.stream.take_ended(), Some((carol, EndReason::Full)));
    }

    #[test]
    fn moving_away_drops_focus_and_streams() {
        let mut s = in_call();
        let carol = stream_key(Some(G), VOC, CAROL);
        s.video_watch(&carol);
        s.video_focus(Some(Tile::Camera(BOB)));
        s.video_pop_out(Tile::Camera(CAROL));
        s.video_pop_out(Tile::Camera(CAROL));
        assert_eq!(s.video.popped, vec![Tile::Camera(CAROL)]);
        upd(&mut s, VoiceState { guild_id: Some(G), ..state(ME, OTHER, false, false) });
        assert!(s.video.focus.is_none() && s.video.watched().is_empty());
        s.video_pop_in(&Tile::Camera(CAROL));
        assert!(s.video.popped.is_empty());
    }

    #[test]
    fn sink_only_converts_visible_tiles_and_wakes_once_per_image() {
        let sink = VideoSink::default();
        let wakes = Arc::new(AtomicUsize::new(0));
        let w = wakes.clone();
        sink.set_wake(Arc::new(move || {
            w.fetch_add(1, Ordering::Relaxed);
        }));
        let bob = Tile::Camera(BOB);
        // Hors champ : rien n'est gardé, aucun réveil.
        sink.push(bob.clone(), &test_pattern(64, 36, 0, 1));
        assert!(sink.take(&bob, 0).is_none());
        assert_eq!(wakes.load(Ordering::Relaxed), 0);
        assert_eq!(sink.wanted_quality(&bob), (0, 0));
        // Affichée à 32×18 : réduite à cette taille.
        sink.show(&[(bob.clone(), (32, 18))]);
        assert_eq!(sink.wanted_quality(&bob), (320, 180));
        sink.push(bob.clone(), &test_pattern(64, 36, 1, 1));
        sink.push(bob.clone(), &test_pattern(64, 36, 2, 1));
        assert_eq!(wakes.load(Ordering::Relaxed), 2);
        let (generation, img) = sink.take(&bob, 0).unwrap_or_else(|| panic!("image"));
        assert_eq!((generation, img.width, img.height, img.pixels.len(), img.bytes().len()), (2, 32, 18, 32 * 18, 32 * 18 * 4));
        assert!(sink.take(&bob, generation).is_none(), "rien de neuf");
        sink.give_back(&bob, img.pixels);
        // Plus affichée : mémoire rendue.
        sink.show(&[]);
        assert!(!sink.is_visible(&bob));
        sink.show(&[(bob.clone(), (1000, 1000))]);
        assert_eq!(sink.wanted_quality(&bob), (1920, 1080));
        assert_eq!(ladder((641, 300)), (960, 540));
    }

    #[test]
    fn conversions_are_correct_and_reject_short_planes() {
        let mut out = Vec::new();
        // I420 2×2 : blanc (Y 235), noir (Y 16), U = V = 128.
        let f = Frame { width: 2, height: 2, pts_us: 0, data: FrameData::I420 { y: vec![235, 16, 235, 16], u: vec![128], v: vec![128] } };
        let bytes = |out: &Vec<u32>| bytemuck::cast_slice::<u32, u8>(out).to_vec();
        assert!(to_rgba(&f, 2, 2, &mut out));
        assert_eq!(&bytes(&out)[..8], &[255, 255, 255, 255, 0, 0, 0, 255]);
        // Rouge pur BT.601 : Y 81, U 90, V 240.
        let red = Frame { width: 2, height: 2, pts_us: 0, data: FrameData::I420 { y: vec![81; 4], u: vec![90], v: vec![240] } };
        assert!(to_rgba(&red, 1, 1, &mut out));
        let px = bytes(&out);
        assert!(px[0] > 250 && px[1] < 5 && px[2] < 5, "{px:?}");
        let short = Frame { width: 4, height: 4, pts_us: 0, data: FrameData::I420 { y: vec![0; 4], u: vec![], v: vec![] } };
        assert!(!to_rgba(&short, 4, 4, &mut out));
        let bgra = Frame { width: 1, height: 1, pts_us: 0, data: FrameData::Bgra(vec![1, 2, 3, 4]) };
        assert!(to_rgba(&bgra, 1, 1, &mut out));
        assert_eq!(bytes(&out), vec![3, 2, 1, 255]);
        assert!(!to_rgba(&bgra, 0, 1, &mut out), "taille nulle");
        assert_eq!(fit((1920, 1080), (480, 480)), (480, 270));
        assert_eq!(fit((320, 180), (1920, 1080)), (320, 180), "jamais agrandie");
    }
}
