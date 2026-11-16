//! Tests de bout en bout : le vrai `net::connect` et le vrai `state::Store`
//! face au faux serveur Discord local de `support/`. Rien ne quitte 127.0.0.1.

mod support;

#[path = "e2e/fixtures.rs"]
mod fixtures;
#[path = "e2e/gateway.rs"]
mod gateway;
#[path = "e2e/messages.rs"]
mod messages;
#[path = "e2e/rest.rs"]
mod rest;
#[path = "e2e/proxy.rs"]
mod proxy;
#[path = "e2e/permissions.rs"]
mod permissions;
#[path = "e2e/accounts.rs"]
mod accounts;
#[path = "e2e/images.rs"]
mod images;
#[path = "e2e/profiles.rs"]
mod profiles;
#[path = "e2e/compression.rs"]
mod compression;
#[path = "e2e/markdown.rs"]
mod markdown;
#[path = "e2e/social.rs"]
mod social;
#[path = "e2e/composer.rs"]
mod composer;
#[path = "e2e/members.rs"]
mod members;
#[path = "e2e/settings.rs"]
mod settings;
#[path = "e2e/relations.rs"]
mod relations;
#[path = "e2e/upload.rs"]
mod upload;
#[path = "e2e/notify.rs"]
mod notify;
#[path = "e2e/threads.rs"]
mod threads;
#[path = "e2e/emoji.rs"]
mod emoji;
#[path = "e2e/servers.rs"]
mod servers;
#[path = "e2e/search.rs"]
mod search;
#[path = "e2e/activity.rs"]
mod activity;
// Tubes nommés : Windows seulement.
#[cfg(windows)]
#[path = "e2e/rpc.rs"]
mod rpc;
#[path = "e2e/inbox.rs"]
mod inbox;
#[path = "e2e/interactions.rs"]
mod interactions;
#[path = "e2e/polls.rs"]
mod polls;
#[path = "e2e/admin_guild.rs"]
mod admin_guild;
#[path = "e2e/channels.rs"]
mod channels;
#[path = "e2e/roles.rs"]
mod roles;
#[path = "e2e/moderation.rs"]
mod moderation;
#[path = "e2e/admin_events.rs"]
mod admin_events;
#[path = "e2e/expressions.rs"]
mod expressions;
#[path = "e2e/voice_transport.rs"]
mod voice_transport;
#[path = "e2e/vocal_etats.rs"]
mod vocal_etats;
#[path = "e2e/calls.rs"]
mod calls;
#[path = "e2e/voice_call.rs"]
mod voice_call;
#[path = "e2e/video.rs"]
mod video;
#[path = "e2e/voice_video.rs"]
mod voice_video;
#[path = "e2e/video_call.rs"]
mod video_call;
#[path = "e2e/quick_profile.rs"]
mod quick_profile;
// Identité annoncée : bureau ou Ferrocord (décision du 14 novembre 2026).
#[path = "e2e/identity.rs"]
mod identity;
// Sélecteur de GIF, favoris du compte, super réaction (lot G).
#[path = "e2e/gifs.rs"]
mod gifs;
// Effets de profil et quêtes (lot ND, routes non documentées).
#[path = "e2e/nd.rs"]
mod nd;
// Fiabilité et cohérence de ce qui est envoyé (lot « fiable »).
#[path = "e2e/fiable.rs"]
mod fiable;
// MP du compte système : aucune écriture (lot « messages »).
#[path = "e2e/official.rs"]
mod official;
