//! Sons courts côté interface (phase 6) : sons du tableau joués sur ce
//! poste, lecture des messages vocaux reçus (cartes de `ui/voice_note.rs`),
//! enregistrement et envoi d'un message vocal depuis la saisie.
//!
//! Le son lui-même passe par l'orchestrateur (`VoiceLink::play_clip`,
//! `record`) ; ici, seulement ce qu'il faut montrer. Pendant une lecture ou
//! un enregistrement, l'image se redessine quelques fois par seconde (temps
//! écoulé) ; sinon, rien.

use super::icons::{paint, VIcon};
use super::App;
use crate::i18n::table::voice_msg::{f as mf, t as mt};
use crate::state::voice_msg::{sound_url, ClipKey, ClipPlay, ClipUp, PttKey, RecordCmd};
use crate::theme::Tokens;
use crate::ui::voice_note::{self, NoteView};
use eframe::egui::{self, RichText, Sense, Vec2};
use std::time::{Duration, Instant};

/// Rythme du redessin pendant une lecture ou un enregistrement.
const TICK: Duration = Duration::from_millis(100);

/// Message vocal en lecture ou en pause.
struct Current {
    id: u64,
    duration_ms: u32,
    /// Position au départ (ou à la pause).
    offset_ms: u32,
    /// Lecture commencée (son décodé et envoyé à la sortie) ; `None` en
    /// chargement ou en pause.
    started: Option<Instant>,
    playing: bool,
}

impl Current {
    fn pos_ms(&self, now: Instant) -> u32 {
        let run = self.started.map_or(0, |s| now.saturating_duration_since(s).as_millis().min(u128::from(u32::MAX)) as u32);
        self.offset_ms.saturating_add(run).min(self.duration_ms.max(self.offset_ms))
    }
}

struct Rec {
    channel: u64,
    since: Instant,
    /// Arrêt demandé, message en préparation.
    finishing: bool,
}

#[derive(Default)]
pub(in crate::ui) struct Notes {
    current: Option<Current>,
    sounds: u64,
    rec: Option<Rec>,
}

impl App {
    /// Une fois par image : sons du tableau, clics sur les messages vocaux,
    /// ce que l'orchestrateur a rendu, état montré par les cartes.
    pub(in crate::ui) fn voice_notes_tick(&mut self, ctx: &egui::Context) {
        let here = self.store.voice.me.map(|m| m.channel_id);
        for p in self.store.take_sound_plays() {
            // Seuls les sons du salon où l'on est s'entendent.
            if here == Some(p.channel_id) {
                self.voice.notes.sounds += 1;
                let key = ClipKey::Sound(self.voice.notes.sounds);
                self.voice.link.play_clip(ClipPlay { key, url: sound_url(p.sound_id), volume: p.volume, from_ms: 0 });
            }
        }
        if let Some(c) = voice_note::take_click(ctx) {
            self.note_toggle(c);
        }
        for u in self.voice.link.poll_clips() {
            self.on_clip_up(u);
        }
        let now = Instant::now();
        let view = self.voice.notes.current.as_ref().map(|c| NoteView { id: c.id, pos_ms: c.pos_ms(now), playing: c.playing });
        voice_note::set_view(ctx, view);
        if view.is_some_and(|v| v.playing) || self.voice.notes.rec.is_some() {
            ctx.request_repaint_after(TICK);
        }
    }

    fn note_toggle(&mut self, c: voice_note::NoteClick) {
        let now = Instant::now();
        let n = &mut self.voice.notes;
        if let Some(cur) = n.current.as_mut().filter(|cur| cur.id == c.id) {
            if cur.playing {
                cur.offset_ms = cur.pos_ms(now);
                cur.started = None;
                cur.playing = false;
                self.voice.link.stop_clip(ClipKey::Note(c.id));
                return;
            }
            let from_ms = if cur.offset_ms >= cur.duration_ms { 0 } else { cur.offset_ms };
            cur.offset_ms = from_ms;
            cur.playing = true;
            self.voice.link.play_clip(ClipPlay { key: ClipKey::Note(c.id), url: c.url, volume: 1.0, from_ms });
            return;
        }
        if let Some(old) = n.current.take() {
            self.voice.link.stop_clip(ClipKey::Note(old.id));
        }
        self.voice.notes.current = Some(Current { id: c.id, duration_ms: c.duration_ms, offset_ms: 0, started: None, playing: true });
        self.voice.link.play_clip(ClipPlay { key: ClipKey::Note(c.id), url: c.url, volume: 1.0, from_ms: 0 });
    }

    fn on_clip_up(&mut self, u: ClipUp) {
        let n = &mut self.voice.notes;
        match u {
            ClipUp::Started { key: ClipKey::Note(id), duration_ms } => {
                if let Some(c) = n.current.as_mut().filter(|c| c.id == id && c.playing) {
                    c.started = Some(Instant::now());
                    c.duration_ms = duration_ms;
                }
            }
            ClipUp::Ended(ClipKey::Note(id)) => {
                if n.current.as_ref().is_some_and(|c| c.id == id && c.playing) {
                    n.current = None;
                }
            }
            ClipUp::Failed { key: ClipKey::Note(id), why } => {
                if n.current.as_ref().is_some_and(|c| c.id == id) {
                    n.current = None;
                }
                self.notify(why);
            }
            // Un son du tableau qui ne se joue pas n'est pas signalé (il passe).
            ClipUp::Started { .. } | ClipUp::Ended(_) | ClipUp::Failed { .. } => {}
            ClipUp::Recorded(r) => {
                let Some(rec) = n.rec.take() else { return };
                if let Err(e) = self.store.send_voice(rec.channel, &r) {
                    let why = super::super::upload::refusal_text(self.lang, &e);
                    self.notify(mf::send_failed(self.lang, &why));
                }
                self.flush_commands();
            }
            ClipUp::RecordFailed(why) => {
                n.rec = None;
                self.notify(why);
            }
        }
    }

    /// Touche de l'appui-pour-parler pour le crochet global (orchestrateur).
    pub(super) fn voice_ptt_key(&mut self) {
        let chord = self.nav.bindings.get(crate::state::keys::Action::PushToTalk);
        let key = chord.as_ref().map(|c| PttKey { key: &c.key, ctrl: c.ctrl, alt: c.alt, shift: c.shift });
        self.voice.link.set_ptt_key(key);
    }

    /// Un message vocal peut-il commencer ici (aucun en cours, permission) ?
    pub(in crate::ui) fn can_start_voice_note(&self, cid: u64) -> bool {
        self.voice.notes.rec.is_none() && self.store.can_send_voice(cid)
    }

    /// « Enregistrer un message vocal » du menu « + » de la saisie.
    pub(in crate::ui) fn start_voice_note(&mut self, cid: u64) {
        if !self.can_start_voice_note(cid) {
            return;
        }
        self.voice.notes.rec = Some(Rec { channel: cid, since: Instant::now(), finishing: false });
        self.voice.link.record(RecordCmd::Start);
    }

    /// Ligne « Enregistrement… 0:05  Annuler  Envoyer » au-dessus du champ.
    pub(in crate::ui) fn voice_note_row(&mut self, ui: &mut egui::Ui, cid: u64) {
        let Some(rec) = self.voice.notes.rec.as_ref().filter(|r| r.channel == cid) else { return };
        let t = self.theme.clone();
        let lang = self.lang;
        let elapsed = rec.since.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
        let finishing = rec.finishing;
        let (mut cancel, mut send) = (false, false);
        ui.horizontal(|ui| {
            let side = Tokens::icon_side(t.typography.body);
            let (r, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
            paint(VIcon::Mic, ui.painter(), r, t.colors.danger);
            ui.label(RichText::new(format!("{} {}", mt::RECORDING.get(lang), voice_note::clock(elapsed))).color(t.colors.muted));
            ui.add_enabled_ui(!finishing, |ui| {
                cancel = ui.button(mt::CANCEL.get(lang)).clicked();
                send = ui.button(mt::SEND.get(lang)).clicked();
            });
        });
        if cancel {
            self.voice.notes.rec = None;
            self.voice.link.record(RecordCmd::Cancel);
        } else if send {
            if let Some(r) = self.voice.notes.rec.as_mut() {
                r.finishing = true;
            }
            self.voice.link.record(RecordCmd::Finish);
        }
    }
}
