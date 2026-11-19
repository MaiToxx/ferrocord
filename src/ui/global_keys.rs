//! Raccourcis globaux (F22) : « Couper le micro » et « Couper le casque »
//! depuis n'importe quelle application, avec les combinaisons de
//! `keys.toml`. Réglage de cet ordinateur (`[voix] raccourcis_globaux`),
//! coupé par défaut : une combinaison inscrite n'arrive plus aux autres
//! applications.

use super::App;
use crate::platform::hotkeys::{self, Binding, Hotkeys};
use crate::state::keys::Action;
use crate::voice::ptt_keys;

/// Actions qui peuvent être globales, dans l'ordre de leurs numéros.
const ACTIONS: [Action; 2] = [Action::ToggleMute, Action::ToggleDeafen];

#[derive(Default)]
pub(super) struct GlobalKeys {
    /// Combinaisons inscrites (ou tentées), et le fil qui les tient.
    held: Option<(Vec<Binding>, Option<Hotkeys>)>,
}

/// Ce que la page des réglages dit d'une action globale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GlobalState {
    Active,
    /// Aucune combinaison, ou une combinaison qui gênerait la saisie ailleurs.
    Unsuitable,
    /// Déjà prise par une autre application.
    Refused,
}

impl App {
    /// Combinaisons à inscrire d'après le réglage et `keys.toml`.
    fn global_wanted(&self) -> Vec<Binding> {
        if !self.local.prefs.audio.global_keys {
            return Vec::new();
        }
        ACTIONS
            .iter()
            .enumerate()
            .filter_map(|(i, a)| {
                let c = self.nav.bindings.get(*a)?;
                let b = Binding { id: i as i32 + 1, vk: ptt_keys::vk_of(&c.key)?, ctrl: c.ctrl, alt: c.alt, shift: c.shift };
                b.safe().then_some(b)
            })
            .collect()
    }

    /// Inscrit, remplace ou retire les raccourcis globaux, puis exécute ceux
    /// qui ont été frappés. Réglage coupé : une comparaison par image.
    pub(super) fn global_keys_tick(&mut self) {
        let want = self.global_wanted();
        if self.global_keys.held.as_ref().map(|h| h.0.as_slice()).unwrap_or_default() != want.as_slice() {
            // L'ancien fil rend ses combinaisons avant que le nouveau les demande.
            self.global_keys.held = None;
            if !want.is_empty() {
                let ctx = self.ctx.clone();
                let keys = Hotkeys::start(want.clone(), std::sync::Arc::new(move || ctx.request_repaint()));
                self.global_keys.held = Some((want, keys));
            }
        }
        if self.global_keys.held.is_none() {
            return;
        }
        for id in hotkeys::take() {
            if let Some(a) = usize::try_from(id - 1).ok().and_then(|i| ACTIONS.get(i)) {
                self.run_action(*a);
            }
        }
    }

    /// État de chaque action globale, pour la page des réglages (réglage en service).
    pub(super) fn global_keys_states(&self) -> Vec<(Action, GlobalState)> {
        let held = self.global_keys.held.as_ref();
        ACTIONS
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let id = i as i32 + 1;
                let state = match held {
                    Some((want, keys)) if want.iter().any(|b| b.id == id) => match keys {
                        Some(k) if !k.refused.contains(&id) => GlobalState::Active,
                        _ => GlobalState::Refused,
                    },
                    _ => GlobalState::Unsuitable,
                };
                (*a, state)
            })
            .collect()
    }
}
