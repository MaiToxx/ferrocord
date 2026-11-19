//! Vocal des serveurs côté interface (phase 4, lot V4) : salons vocaux et
//! personnes connectées dans la liste des salons, panneau de connexion,
//! boutons muet et sourdine, menu d'une personne (volume, muet pour moi,
//! modération), appui-pour-parler, sons d'arrivée, de départ et de muet.
//!
//! Tout est piloté par `state::voice` : aucun son de conversation ni
//! connexion vocale ici (lot V7). Les sons courts passent par `desk`.

pub(super) mod icons;
mod link;
mod list;
mod menu;
mod notes;
mod panel;
mod overlay;
mod record;

use super::App;
use crate::i18n::table::voice::{f as vf, t as vt};
use crate::state::model::Avatar;
use crate::state::notify::Kind;
use crate::state::voice::{JoinBlock, ModOutcome, MyVoice, UserVoice, VoiceMod, VoiceState};
use eframe::egui;

/// Proportions de la bannière d'un serveur (16:9, celles de l'image envoyée).
pub(super) const GUILD_BANNER_ASPECT: f32 = 16.0 / 9.0;
/// Hauteur maximale de la bannière en tête de la colonne des salons (135 pour 240 de large).
const GUILD_BANNER_MAX: f32 = 135.0;

/// Bande de la bannière d'un serveur en tête de la colonne : toute la
/// largeur, en 16:9, jamais plus haute que dans le client.
pub(super) fn banner_band(top_left: egui::Pos2, width: f32) -> egui::Rect {
    egui::Rect::from_min_size(top_left, egui::vec2(width, (width / GUILD_BANNER_ASPECT).min(GUILD_BANNER_MAX)))
}

/// Ce que l'interface retient du vocal d'une image à l'autre.
pub(super) struct VoiceUi {
    /// Orchestrateur du vocal (lot V7) ; aucun hors de l'application.
    link: Box<dyn crate::state::voice_link::VoiceLink>,
    /// Fenêtre des codes de vérification ouverte.
    codes_open: bool,
    /// Modération en attente de confirmation.
    confirm: Option<menu::Confirm>,
    /// Appui-pour-parler enfoncé à l'image précédente.
    ptt: bool,
    /// Ce qui a été vu à l'image précédente (sons d'arrivée, de départ, de muet).
    heard: Option<Heard>,
    /// Muet ou sourdine changés à la main : `local.toml` ne les remplace plus.
    touched: bool,
    /// Volume en cours de réglage (curseur tenu) : enregistré au relâché.
    volume_edit: Option<(u64, UserVoice)>,
    /// Sons courts et messages vocaux (phase 6).
    notes: notes::Notes,
    /// Enregistrement local d'un appel (plugin, lot E4).
    rec: record::RecUi,
    /// Incrustation « qui parle » (plugin, lot E4).
    overlay: overlay::OverlayUi,
}

/// Couleur d'effacement voulue par l'incrustation « qui parle » quand sa
/// fenêtre existe (transparente), sinon `None`.
pub(in crate::ui) fn overlay_clear_color() -> Option<[f32; 4]> {
    overlay::clear_color()
}

impl Default for VoiceUi {
    fn default() -> Self {
        VoiceUi {
            link: Box::new(crate::state::voice_link::NoVoice),
            codes_open: false,
            confirm: None,
            ptt: false,
            heard: None,
            touched: false,
            volume_edit: None,
            notes: notes::Notes::default(),
            rec: record::RecUi::default(),
            overlay: overlay::OverlayUi::default(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Heard {
    me: Option<MyVoice>,
    /// Personnes de mon salon, moi exclu.
    people: Vec<u64>,
    mute: bool,
    deaf: bool,
    /// Personnes de mon salon qui diffusent un direct (moi compris).
    streams: Vec<u64>,
}

/// Geste fait dans la liste ou un menu, appliqué après le dessin.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum VoiceAct {
    Join(u64),
    /// Volume (pour cent) ; `done` : curseur relâché, à enregistrer.
    Volume { user: u64, volume: u16, done: bool },
    LocalMute(u64, bool),
    /// Ouvre la confirmation d'une modération.
    Moderate { user: u64, action: VoiceMod },
    /// « Regarder le stream » : rejoint le salon de `user` et regarde son direct.
    Watch { channel: u64, user: u64 },
}

impl App {
    /// Une fois par image, quel que soit l'écran : plugins vocaux qui vivent
    /// hors de la liste des salons (enregistrement, lot E4).
    pub(in crate::ui) fn voice_plugins_tick(&mut self, ctx: &egui::Context) {
        self.voice_rec_tick(ctx);
        self.voice_overlay_tick(ctx);
    }

    /// Aperçu hors ligne des plugins vocaux (lot E4) : 0 enregistrement en
    /// cours, 1 fenêtre de démarrage d'un enregistrement, 2 incrustation
    /// « qui parle », 3 ligne RNNoise de la page Voix et vidéo.
    #[doc(hidden)]
    pub fn voice_e4_preview(&mut self, scene: u8) {
        use crate::plugins::{Id, Value, REC_FOLDER};
        match scene {
            0 | 1 => {
                let mut p = self.local.prefs.clone();
                p.plugins.set(Id::CallRecorder, true);
                p.plugins.put(Id::CallRecorder, REC_FOLDER, Value::Str("C:\\Users\\moi\\Music\\Appels".into()));
                self.set_local_prefs(p);
                if scene == 0 { self.voice_rec_preview_active(83) } else { self.voice_rec_preview_ask() }
            }
            2 => {
                // Dans la fenêtre principale, pour la capture : l'incrustation
                // réelle est une fenêtre à part, que la capture ne voit pas.
                self.ctx.set_embed_viewports(true);
                let mut p = self.local.prefs.clone();
                p.plugins.set(Id::SpeakerOverlay, true);
                p.plugins.put(Id::SpeakerOverlay, crate::plugins::OVERLAY_X, Value::Int(620));
                p.plugins.put(Id::SpeakerOverlay, crate::plugins::OVERLAY_Y, Value::Int(240));
                self.set_local_prefs(p);
                let other = self.store.voice.me.and_then(|my| self.store.voice.members(my.channel_id).map(|s| s.user_id).find(|u| Some(*u) != self.store.me.as_ref().map(|m| m.id)));
                if let Some(user_id) = other {
                    self.store.apply(crate::state::Event::Voice(crate::state::voice::VoiceEvent::Speaking { user_id, speaking: true }));
                }
            }
            3 => {
                self.set_plugin(Id::NoiseRnn, true);
                self.show_settings(crate::ui::settings::Page::Voice);
            }
            _ => {}
        }
    }

    /// Nom et avatar d'une personne connectée : ceux du paquet, sinon ceux connus.
    pub(super) fn voice_who(&self, s: &VoiceState) -> (String, Avatar) {
        let people = self.store.people.lock();
        let user = people.user(s.user_id);
        let nick = s.guild_id.and_then(|g| people.member(g, s.user_id)).and_then(|m| m.nick.clone()).filter(|n| !n.is_empty());
        let me = self.store.me.as_ref().filter(|m| m.id == s.user_id);
        let name = s
            .name
            .clone()
            .or(nick)
            .or_else(|| user.map(|u| u.display_name().to_string()))
            .or_else(|| me.map(|m| m.name.clone()))
            .unwrap_or_else(|| vt::UNKNOWN_USER.get(self.lang).to_string());
        let avatar = s.avatar.or_else(|| user.map(|u| u.avatar)).or_else(|| me.map(|m| m.avatar)).unwrap_or_else(|| Avatar::default_for(s.user_id));
        (name, avatar)
    }

    /// Réglages de cette personne (le curseur tenu l'emporte).
    pub(super) fn voice_user_prefs(&self, user: u64) -> UserVoice {
        match self.voice.volume_edit {
            Some((u, v)) if u == user => v,
            _ => self.local.prefs.voice.user(user),
        }
    }

    pub(super) fn voice_toggle_mute(&mut self) {
        let on = !self.store.voice.self_mute;
        self.store.voice_set_mute(on);
        self.voice_keep_self();
    }

    pub(super) fn voice_toggle_deaf(&mut self) {
        let on = !self.store.voice.self_deaf;
        self.store.voice_set_deaf(on);
        self.voice_keep_self();
    }

    /// Muet et sourdine retenus dans `local.toml`.
    fn voice_keep_self(&mut self) {
        self.voice.touched = true;
        let mut p = self.local.prefs.clone();
        p.voice.mute = self.store.voice.mute_choice();
        p.voice.deaf = self.store.voice.self_deaf;
        self.set_local_prefs(p);
    }

    /// Rejoint un salon vocal ; un refus est expliqué.
    pub(super) fn voice_join(&mut self, channel: u64) {
        match self.store.voice_join(channel) {
            Ok(()) => {
                // Le moteur audio (lot V7) reçoit les réglages des personnes déjà là.
                let people: Vec<u64> = self.store.voice.members(channel).map(|s| s.user_id).collect();
                self.voice_send_prefs(&people);
            }
            Err(b) => {
                let text = match b {
                    JoinBlock::NoConnect => vt::NO_CONNECT,
                    JoinBlock::Full => vt::FULL,
                    JoinBlock::Unknown => vt::GONE,
                };
                self.notify(text);
            }
        }
    }

    /// Volumes et muets réglés de ces personnes (seulement ceux qui ne sont pas par défaut).
    fn voice_send_prefs(&mut self, people: &[u64]) {
        for u in people {
            if let Some(p) = self.local.prefs.voice.users.get(u).copied() {
                self.store.voice_user(*u, p);
            }
        }
    }

    /// Volumes et muets retenus pour le son des directs (clés au bit
    /// `STREAM_AUDIO`) : le moteur audio les garde pour l'appel en cours et
    /// les suivants.
    pub(super) fn voice_send_stream_prefs(&mut self) {
        let kept: Vec<(u64, UserVoice)> = self.local.prefs.voice.users.range(crate::voice::STREAM_AUDIO..).map(|(k, v)| (*k, *v)).collect();
        for (k, v) in kept {
            self.store.voice_user(k, v);
        }
    }

    pub(super) fn voice_acts(&mut self, acts: Vec<VoiceAct>) {
        for a in acts {
            match a {
                VoiceAct::Join(c) => self.voice_join(c),
                VoiceAct::Volume { user, volume, done } => {
                    let u = UserVoice { volume, ..self.voice_user_prefs(user) };
                    self.store.voice_user(user, u);
                    if done {
                        self.voice.volume_edit = None;
                        let mut p = self.local.prefs.clone();
                        p.voice.set_user(user, u);
                        self.set_local_prefs(p);
                    } else {
                        self.voice.volume_edit = Some((user, u));
                    }
                }
                VoiceAct::LocalMute(user, muted) => {
                    let u = UserVoice { muted, ..self.voice_user_prefs(user) };
                    self.store.voice_user(user, u);
                    let mut p = self.local.prefs.clone();
                    p.voice.set_user(user, u);
                    self.set_local_prefs(p);
                }
                VoiceAct::Moderate { user, action } => self.voice_ask(user, action),
                VoiceAct::Watch { channel, user } => {
                    // Fonctions existantes : rejoindre le salon, regarder le direct, ouvrir la vue du salon.
                    self.voice_join(channel);
                    if self.store.voice.me.is_some_and(|m| m.channel_id == channel) {
                        let guild = self.store.voice.channel(channel).map(|c| c.guild_id);
                        self.store.video_watch(&crate::state::stream::stream_key(guild, channel, user));
                        self.open_channel(channel);
                    }
                }
            }
        }
    }

    /// Une fois par image (liste des salons) : `local.toml` relu, appui-pour-parler, sons, issue d'une modération.
    pub(super) fn voice_tick(&mut self, ui: &egui::Ui) {
        let p = &self.local.prefs.voice;
        if !self.voice.touched && (self.store.voice.mute_choice(), self.store.voice.self_deaf) != (p.mute, p.deaf) {
            let (mute, deaf) = (p.mute, p.deaf);
            self.store.voice_restore(mute, deaf);
        }
        self.voice_ptt(ui);
        self.voice_sounds();
        if let Some(o) = self.store.voice.take_outcome() {
            let text = match o {
                ModOutcome::Done(_) => vt::DONE.get(self.lang).to_string(),
                ModOutcome::Failed(_, why) => vf::mod_failed(self.lang, super::words::problem(self.lang, &why, self.h24())),
            };
            self.notify(text);
        }
    }

    /// Appui-pour-parler : touche tenue dans la fenêtre, modificateurs exacts ;
    /// hors de la fenêtre, le crochet global de l'orchestrateur prend le relais.
    fn voice_ptt(&mut self, ui: &egui::Ui) {
        self.voice_ptt_key();
        let chord = self.nav.bindings.get(crate::state::keys::Action::PushToTalk);
        let down = chord.and_then(|c| egui::Key::from_name(&c.key).map(|k| (c, k))).is_some_and(|(c, k)| {
            ui.input(|i| i.key_down(k) && (i.modifiers.ctrl || i.modifiers.command) == c.ctrl && i.modifiers.alt == c.alt && i.modifiers.shift == c.shift)
        });
        // Touche du Stream Deck tenue (plugin « Stream Deck ») : même effet.
        let down = down || self.deck.talk;
        if down != self.voice.ptt {
            self.voice.ptt = down;
            self.store.voice_set_ptt(down);
        }
    }

    fn voice_sounds(&mut self) {
        let v = &self.store.voice;
        let my_id = self.store.me.as_ref().map(|m| m.id);
        let people: Vec<u64> = v.me.map(|m| v.members(m.channel_id).map(|s| s.user_id).filter(|u| Some(*u) != my_id).collect()).unwrap_or_default();
        let streams: Vec<u64> = v.me.map(|m| v.members(m.channel_id).filter(|s| s.self_stream).map(|s| s.user_id).collect()).unwrap_or_default();
        let now = Heard { me: v.me, people, mute: v.self_mute, deaf: v.self_deaf, streams };
        let Some(before) = self.voice.heard.replace(now.clone()) else { return };
        if before == now {
            return;
        }
        let mut sound = None;
        if before.me != now.me {
            sound = Some(if now.me.is_some() { Kind::VoiceConnect } else { Kind::VoiceDisconnect });
        } else if now.me.is_some() {
            let came: Vec<u64> = now.people.iter().filter(|u| !before.people.contains(u)).copied().collect();
            if !came.is_empty() {
                self.voice_send_prefs(&came);
                sound = Some(Kind::VoiceJoin);
            } else if before.people.iter().any(|u| !now.people.contains(u)) {
                sound = Some(Kind::VoiceLeave);
            }
        }
        // Direct commencé ou arrêté dans mon salon (pas à mon arrivée ni à mon départ).
        if before.me == now.me && now.me.is_some() {
            if now.streams.iter().any(|u| !before.streams.contains(u)) {
                sound = Some(Kind::StreamStart);
            } else if before.streams.iter().any(|u| !now.streams.contains(u) && (now.people.contains(u) || Some(*u) == my_id)) {
                sound = Some(Kind::StreamStop);
            }
        }
        if before.deaf != now.deaf {
            sound = Some(if now.deaf { Kind::VoiceDeafen } else { Kind::VoiceUndeafen });
        } else if before.mute != now.mute {
            sound = Some(if now.mute { Kind::VoiceMute } else { Kind::VoiceUnmute });
        }
        if let Some(k) = sound {
            self.voice_sound(k);
        }
    }
}

#[cfg(test)]
mod banner_tests {
    use super::*;

    #[test]
    fn la_banniere_tient_la_largeur_sans_depasser_sa_hauteur() {
        let at = egui::pos2(72.0, 0.0);
        assert_eq!(banner_band(at, 240.0).size(), egui::vec2(240.0, 135.0));
        // Colonne élargie : la hauteur reste celle du client.
        assert_eq!(banner_band(at, 320.0).size(), egui::vec2(320.0, 135.0));
        assert_eq!(banner_band(at, 160.0).size(), egui::vec2(160.0, 90.0));
    }
}
