//! Administration des serveurs (phase 3), côté réseau : un fichier par
//! domaine, dans le même ordre que `state::admin`.

pub(super) mod channels;
// Fonctions du serveur, écran de bienvenue, intégration (lot A7).
pub(in crate::net) mod community;
// Événements programmés (lot A7).
pub(in crate::net) mod events;
// Emojis, autocollants et sons du serveur (lot A6).
pub(super) mod expressions;
// Widget, modèle de serveur, élagage (groupe 2, lot V).
mod extras;
pub(super) mod guild;
mod integrations;
pub(super) mod moderation;
pub(super) mod roles;
mod webhooks;

use super::raw::RawGuild;
use super::Out;
use crate::state::admin::expressions::{Boost, ExprEvent};
use crate::state::Event;

/// Événements de la passerelle propres à l'administration (appelé pour
/// chaque paquet, comme `polls::dispatch`).
pub(super) fn dispatch(kind: &str, d: &str, out: &Out) {
    if let Some(e) = guild::gateway(kind, d) {
        out.send(Event::GuildAdmin(e));
    }
    // Champs des rôles, niveau de boost (lot A3).
    roles::dispatch(kind, d, out);
    // Sons du serveur, fonctions qui agrandissent les emplacements (lot A6).
    expressions::dispatch(kind, d, out);
}

/// Seconde moitié, appelée après READY et GUILD_CREATE (le serveur doit
/// être connu du magasin) : modération, AutoMod, journal d'audit, sécurité
/// (lots A4 et A5), événements programmés, fonctions du serveur (lot A7).
pub(super) fn dispatch_after(kind: &str, d: &str, shared: &super::raw::Shared, out: &Out) {
    moderation::dispatch(kind, d, out);
    events::dispatch(kind, d, out);
    community::dispatch(kind, d, shared, out);
}

/// Fonctions d'un serveur reçu (READY, GUILD_CREATE) qui agrandissent les
/// emplacements d'expressions, à publier après lui (le niveau de boost est
/// dans `Guild::premium_tier`, seule source).
pub(super) fn boost(g: &RawGuild, into: &mut Vec<(u64, Boost)>) {
    if g.unavailable {
        return;
    }
    if let (Some(_), features) = g.premium() {
        into.push((g.id, expressions::boost_of(features.iter().map(String::as_str))));
    }
}

pub(super) fn send_boosts(list: Vec<(u64, Boost)>, out: &Out) {
    for (guild_id, boost) in list {
        out.send(Event::Expressions(ExprEvent::Boost { guild_id, boost }));
    }
}
