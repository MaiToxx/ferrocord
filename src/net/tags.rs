//! Choix du tag de serveur (lot ND) : `PUT /users/@me/clan`.
//!
//! Source : docs.discord.food, resources/user, « Set Guild Identity » :
//! `identity_guild_id` (identifiant ou nul) et `identity_enabled`. La
//! réponse (un objet `user`) n'est pas lue : le magasin connaît déjà le tag
//! du serveur choisi. Captcha : l'action s'arrête là.

use super::ratelimit::Route;
use super::rest::{api, Ctx};
use super::Event;
use crate::state::quests::Refused;
use crate::state::tags::{Slot, StyleEdit, TagCmd, TagEvent};
use crate::state::Problem;
use reqwest::Method;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

const MAX_WAIT: Duration = Duration::from_secs(10);

/// Corps de la requête : un serveur choisi, ou plus aucun tag.
pub(super) fn body(guild: Option<u64>) -> Value {
    json!({ "identity_guild_id": guild.map(|g| g.to_string()), "identity_enabled": guild.is_some() })
}

/// Corps de `PATCH /users/@me` pour une parure : son produit, ou nul pour la retirer
/// (docs.discord.food, « Modify Current User » ; aucun mot de passe n'est demandé pour ces champs).
pub(super) fn wear_body(slot: Slot, sku: Option<u64>) -> Value {
    let key = match slot {
        Slot::Decoration => "avatar_decoration_sku_id",
        Slot::Nameplate => "nameplate_sku_id",
    };
    json!({ key: sku.map(|s| s.to_string()) })
}

/// Corps de `PATCH /users/@me` pour le style du nom (docs.discord.food,
/// « Modify Current User » : trois champs annulables ; nuls : style retiré).
pub(super) fn style_body(style: Option<&StyleEdit>) -> Value {
    match style {
        Some(s) => json!({ "display_name_font_id": s.font_id, "display_name_effect_id": s.effect_id, "display_name_colors": s.colors.iter().take(2).collect::<Vec<_>>() }),
        None => json!({ "display_name_font_id": null, "display_name_effect_id": null, "display_name_colors": null }),
    }
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: TagCmd) {
    type Done = Box<dyn Fn(Result<(), Refused>) -> Event + Send>;
    let (done, route, method, path, body): (Done, _, _, _, _) = match cmd {
        TagCmd::Set { guild } => (Box::new(move |result| Event::Tags(TagEvent::Set { guild, result })), "PUT /users/@me/clan", Method::PUT, "/users/@me/clan", body(guild)),
        TagCmd::Wear { slot, sku } => (Box::new(move |result| Event::Tags(TagEvent::Worn { slot, sku, result })), "PATCH /users/@me", Method::PATCH, "/users/@me", wear_body(slot, sku)),
        TagCmd::Style { style } => {
            let body = style_body(style.as_ref());
            (Box::new(move |result| Event::Tags(TagEvent::Styled { style: style.clone(), result })), "PATCH /users/@me", Method::PATCH, "/users/@me", body)
        }
    };
    if ctx.is_bot() {
        ctx.out.send(done(Err(Refused::Other(Problem::Forbidden))));
        return;
    }
    let url = format!("{}{path}", ctx.api_root);
    let route = Route { template: route, major: 0 };
    let ev = match api(&ctx, route, method, &url, Some(body), MAX_WAIT, std::future::pending()).await {
        Ok(_) => done(Ok(())),
        Err(e) => match super::quests::refused(e) {
            Ok(r) => done(Err(r)),
            Err(fatal) => *fatal,
        },
    };
    ctx.out.send(ev);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_names_the_guild_or_clears_the_tag() {
        assert_eq!(body(Some(9)), json!({ "identity_guild_id": "9", "identity_enabled": true }));
        assert_eq!(body(None), json!({ "identity_guild_id": null, "identity_enabled": false }));
        assert_eq!(wear_body(Slot::Decoration, Some(7)), json!({ "avatar_decoration_sku_id": "7" }));
        assert_eq!(wear_body(Slot::Nameplate, None), json!({ "nameplate_sku_id": null }));
        let style = StyleEdit { font_id: 11, effect_id: 2, colors: vec![0xff0000, 0x0000ff, 0x00ff00] };
        assert_eq!(style_body(Some(&style)), json!({ "display_name_font_id": 11, "display_name_effect_id": 2, "display_name_colors": [0xff0000, 0x0000ff] }), "deux couleurs au plus");
        assert_eq!(style_body(None), json!({ "display_name_font_id": null, "display_name_effect_id": null, "display_name_colors": null }));
    }
}
