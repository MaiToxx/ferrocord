//! Plugin « Stream Deck » : tient la commande locale (`crate::deck`) tant
//! que le plugin est activé, exécute ce que les touches demandent et leur
//! dit l'état. Plugin coupé : une comparaison par image, rien d'autre.

use super::App;
use crate::deck::{Deck, Do, Installed, SoundRef, State};
use crate::state::calls::Loadable;
use crate::i18n::table::e2::t as wx;
use crate::state::keys::Action;

#[derive(Default)]
pub(super) struct DeckUi {
    /// Activé à la dernière image : l'installation ne se fait qu'au passage.
    on: bool,
    server: Option<Deck>,
    /// Touche d'appui-pour-parler tenue (lue aussi par `voice_ptt`).
    pub(super) talk: bool,
    /// Le micro était coupé quand la touche a été enfoncée en mode
    /// « détection de la voix » : il est recoupé au relâchement.
    remute: bool,
    /// La liste des sons a été demandée : elle est redite quand elle change.
    /// Dernière forme dite : serveur du salon vocal, nombre de sons du
    /// serveur, nombre de sons par défaut.
    sounds: Option<(Option<u64>, Option<usize>, Option<usize>)>,
}

impl App {
    pub(super) fn deck_tick(&mut self) {
        let on = crate::plugins::on(crate::plugins::Id::StreamDeck);
        if on != self.deck.on {
            self.deck.on = on;
            self.deck.server = None;
            self.deck.sounds = None;
            if self.deck.talk {
                self.deck_talk(false);
            }
            if on {
                let ctx = self.ctx.clone();
                self.deck.server = Deck::start(std::sync::Arc::new(move || ctx.request_repaint()));
                // Le plugin de Stream Deck suit la version de Ferrocord : posé
                // ou mis à jour ici, jamais ailleurs.
                let text = match crate::deck::plugins_dir().map_or(Installed::NoStreamDeck, |d| crate::deck::install(&d)) {
                    Installed::Written => Some(wx::DECK_INSTALLED),
                    Installed::NoStreamDeck => Some(wx::DECK_ABSENT),
                    Installed::Failed => Some(wx::DECK_FAILED),
                    Installed::Current => None,
                };
                if let Some(t) = text {
                    self.notify(t.get(self.lang).to_string());
                }
            }
        }
        let Some(asked) = self.deck.server.as_ref().map(Deck::take) else { return };
        for d in asked {
            self.deck_do(d);
        }
        let now = self.deck_state();
        if let Some(s) = &mut self.deck.server {
            s.publish(now);
        }
        if self.deck.sounds.is_some() {
            self.deck_sounds(false);
        }
    }

    /// Serveur du salon vocal où je suis.
    fn deck_voice_guild(&self) -> Option<u64> {
        let v = &self.store.voice;
        v.me.and_then(|m| v.channel(m.channel_id)).map(|c| c.guild_id)
    }

    /// Dit la liste des sons (ceux du tableau de sons de Ferrocord : sons du
    /// serveur du salon vocal, puis sons par défaut) si elle a changé, ou
    /// d'office avec `force`. Par image : trois comparaisons.
    fn deck_sounds(&mut self, force: bool) {
        let guild = self.deck_voice_guild();
        let own = guild.and_then(|g| self.store.expressions.sounds(g));
        let default = match self.store.calls.default_sounds() {
            Loadable::Ready(list) => Some(list.as_slice()),
            _ => None,
        };
        let shape = (guild, own.map(<[_]>::len), default.map(<[_]>::len));
        if !force && self.deck.sounds == Some(shape) {
            return;
        }
        let list: Vec<SoundRef> = own
            .unwrap_or_default()
            .iter()
            .map(|s| (s, guild))
            .chain(default.unwrap_or_default().iter().map(|s| (s, None)))
            .filter(|(s, _)| s.available)
            .map(|(s, g)| SoundRef { id: s.id, guild: g, name: s.name.clone() })
            .collect();
        if let Some(s) = &self.deck.server {
            s.publish_sounds(&list);
        }
        // Serveur changé, ou liste jamais demandée : chargée comme à
        // l'ouverture du tableau de sons (une fois par session).
        if self.deck.sounds.is_none_or(|old| old.0 != guild) {
            self.store.load_soundboard(guild);
            self.flush_commands();
        }
        self.deck.sounds = Some(shape);
    }

    fn deck_state(&self) -> State {
        let (v, video) = (&self.store.voice, &self.store.video);
        State { voice: v.me.is_some(), mute: v.self_mute, deaf: v.self_deaf, camera: video.camera, share: video.share.is_some(), ringing: !self.ringing_calls().is_empty(), talk: self.deck.talk }
    }

    /// Les mêmes gestes que les boutons du panneau vocal et de l'appel entrant.
    fn deck_do(&mut self, d: Do) {
        let in_voice = self.store.voice.me.is_some();
        match d {
            Do::Mute => self.voice_toggle_mute(),
            Do::Deafen => self.voice_toggle_deaf(),
            Do::Leave if in_voice => self.store.voice_leave(),
            Do::Camera if in_voice && crate::video::capture::AVAILABLE => self.toggle_camera(),
            Do::Share if self.store.video.share.is_some() => self.store.video_stop_share(),
            Do::Share if in_voice && self.share_picker_ready() => {
                // Le choix de l'écran se fait dans Ferrocord : sa fenêtre vient devant.
                self.request_share_picker();
                self.ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Focus);
            }
            Do::Answer => self.run_action(Action::AnswerCall),
            Do::Decline => self.run_action(Action::DeclineCall),
            Do::Talk(on) => self.deck_talk(on),
            Do::Sounds => self.deck_sounds(true),
            // Comme un clic dans le tableau de sons : `play_sound` refuse
            // hors d'un salon vocal, micro coupé ou en sourdine.
            Do::Sound { id, guild } => {
                if let Some(me) = self.store.voice.me {
                    self.store.play_sound(me.channel_id, id, guild);
                }
            }
            Do::Leave | Do::Camera | Do::Share => {}
        }
        self.flush_commands();
    }

    /// Appui-pour-parler d'une touche. En mode « appui-pour-parler », elle
    /// vaut la touche du clavier. En mode « détection de la voix », elle
    /// ouvre un micro coupé le temps de l'appui (rien n'est retenu dans
    /// `local.toml` : c'est un état passager).
    fn deck_talk(&mut self, on: bool) {
        if on == self.deck.talk {
            return;
        }
        self.deck.talk = on;
        if self.local.prefs.audio.mode == crate::state::audio_prefs::InputMode::PushToTalk {
            self.store.voice_set_ptt(on);
        } else if on {
            self.deck.remute = self.store.voice.self_mute && !self.store.voice.self_deaf;
            if self.deck.remute {
                self.store.voice_set_mute(false);
            }
        } else if std::mem::take(&mut self.deck.remute) {
            self.store.voice_set_mute(true);
        }
    }
}
