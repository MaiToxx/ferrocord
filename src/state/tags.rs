//! Choix du tag de serveur que mon compte affiche (lot ND).
//!
//! Source : docs.discord.food, resources/user, « Set Guild Identity »
//! (`PUT /users/@me/clan`, `identity_guild_id` et `identity_enabled`) ;
//! resources/guild, « Guild Identity Structure » (`profile.tag`,
//! `profile.badge` des serveurs reçus par la passerelle).
//!
//! Une requête par clic, une seule à la fois. Un compte bot n'en envoie pas.

use super::cosmetics::{GuildTag, NameEffect, NameStyle};
use super::effects::Wearable;
use super::event::Cmd;
use super::quests::Refused;
use super::Store;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TagCmd {
    /// `None` : ne plus afficher de tag.
    Set { guild: Option<u64> },
    /// Décoration d'avatar ou plaque portée (docs.discord.food, resources/user,
    /// « Modify Current User » : `avatar_decoration_sku_id`, `nameplate_sku_id`) ;
    /// `None` : retirée.
    Wear { slot: Slot, sku: Option<u64> },
    /// Style du nom d'affichage (docs.discord.food, resources/user, « Modify
    /// Current User » : `display_name_font_id`, `display_name_effect_id`,
    /// `display_name_colors`, deux couleurs au plus, réservé à Nitro) ;
    /// `None` : retiré.
    Style { style: Option<StyleEdit> },
}

/// Style du nom d'affichage tel qu'il part (identifiants de la source).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleEdit {
    pub font_id: u16,
    pub effect_id: u16,
    /// 0xRRGGBB, une ou deux couleurs.
    pub colors: Vec<u32>,
}

impl StyleEdit {
    pub fn of(s: &NameStyle) -> StyleEdit {
        StyleEdit { font_id: s.font_id, effect_id: s.effect.code(), colors: s.colors().iter().copied().take(2).collect() }
    }

    /// Ce que la carte en montre (rien sans couleur lisible).
    pub fn shown(&self) -> Option<NameStyle> {
        NameStyle::new(self.font_id, NameEffect::from_code(u64::from(self.effect_id)), &self.colors)
    }
}

/// Emplacement d'une parure que le compte choisit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Decoration,
    Nameplate,
}

impl Slot {
    pub fn of(look: &Wearable) -> Slot {
        match look {
            Wearable::Decoration(_) => Slot::Decoration,
            Wearable::Nameplate(_) => Slot::Nameplate,
        }
    }
}

#[derive(Debug)]
pub enum TagEvent {
    Set { guild: Option<u64>, result: Result<(), Refused> },
    Worn { slot: Slot, sku: Option<u64>, result: Result<(), Refused> },
    Styled { style: Option<StyleEdit>, result: Result<(), Refused> },
}

#[derive(Default)]
pub struct Tags {
    busy: bool,
    /// Dernier refus, montré sous le choix.
    refused: Option<Refused>,
}

impl Tags {
    pub fn clear(&mut self) {
        *self = Tags::default();
    }

    pub fn busy(&self) -> bool {
        self.busy
    }

    pub fn refused(&self) -> Option<&Refused> {
        self.refused.as_ref()
    }
}

impl Store {
    /// Tags que mon compte peut afficher : ceux de mes serveurs qui en ont un.
    pub fn tag_choices(&self) -> Vec<GuildTag> {
        self.guilds.iter().filter_map(|g| g.tag).collect()
    }

    /// Tag que mon compte affiche.
    pub fn own_tag(&self) -> Option<GuildTag> {
        let me = self.me.as_ref()?.id;
        self.people.lock().user(me)?.look.tag()
    }

    /// Clic sur un tag (ou sur « Aucun ») : la requête part, sauf si une
    /// autre est en cours ou si ce serveur n'a pas de tag.
    pub fn set_guild_tag(&mut self, guild: Option<u64>) -> bool {
        let known = guild.is_none_or(|g| self.guilds.iter().any(|x| x.id == g && x.tag.is_some()));
        if self.tags.busy || !known || self.own_tag().map(|t| t.guild_id) == guild {
            return false;
        }
        self.tags.busy = true;
        self.tags.refused = None;
        self.push_command(Cmd::Tags(TagCmd::Set { guild }));
        true
    }

    /// Ce que mon compte porte dans cet emplacement, parmi ce qu'il possède.
    pub fn worn(&self, slot: Slot) -> Option<u64> {
        let me = self.me.as_ref()?.id;
        let look = self.people.lock().user(me)?.look;
        self.effects.looks.iter().find(|o| match o.look {
            Wearable::Decoration(a) => slot == Slot::Decoration && look.decoration() == Some(a),
            Wearable::Nameplate(n) => slot == Slot::Nameplate && look.nameplate().map(|p| p.asset) == Some(n.asset),
        }).map(|o| o.sku_id)
    }

    /// Clic sur une décoration ou une plaque possédée (ou sur « Aucune ») :
    /// `PATCH /users/@me`, une requête à la fois.
    pub fn wear(&mut self, slot: Slot, sku: Option<u64>) -> bool {
        let owned = sku.is_none_or(|s| self.effects.looks.iter().any(|o| o.sku_id == s && Slot::of(&o.look) == slot));
        if self.tags.busy || !owned || self.worn(slot) == sku {
            return false;
        }
        self.tags.busy = true;
        self.tags.refused = None;
        self.push_command(Cmd::Tags(TagCmd::Wear { slot, sku }));
        true
    }

    /// Style du nom que mon compte porte.
    pub fn own_name_style(&self) -> Option<NameStyle> {
        let me = self.me.as_ref()?.id;
        self.people.lock().user(me)?.look.name_style()
    }

    /// Style du nom enregistré (`None` : retiré) : `PATCH /users/@me`, une
    /// requête à la fois, rien si le style est déjà celui-là.
    pub fn set_name_style(&mut self, style: Option<StyleEdit>) -> bool {
        let style = style.filter(|s| !s.colors.is_empty()).map(|mut s| {
            s.colors.truncate(2);
            s
        });
        // Effet sans valeur d'envoi documentée (prisme, « gummy », inconnu) : jamais écrit.
        let writable = style.as_ref().is_none_or(|s| NameEffect::from_code(u64::from(s.effect_id)).writable());
        if self.tags.busy || !writable || self.own_name_style().as_ref().map(StyleEdit::of) == style {
            return false;
        }
        self.tags.busy = true;
        self.tags.refused = None;
        self.push_command(Cmd::Tags(TagCmd::Style { style }));
        true
    }

    pub(super) fn apply_tags(&mut self, e: TagEvent) {
        let (guild, result) = match e {
            TagEvent::Set { guild, result } => (guild, result),
            TagEvent::Styled { style, result } => {
                self.tags.busy = false;
                match result {
                    Ok(()) => {
                        let Some(me) = self.me.as_ref().map(|m| m.id) else { return };
                        let mut people = self.people.lock();
                        if let Some(mut user) = people.user(me).cloned() {
                            user.look.name_style = Some(style.as_ref().and_then(StyleEdit::shown));
                            people.upsert_user(user);
                        }
                    }
                    Err(r) => self.tags.refused = Some(r),
                }
                return;
            }
            TagEvent::Worn { slot, sku, result } => {
                self.tags.busy = false;
                match result {
                    Ok(()) => {
                        let look = sku.and_then(|s| self.effects.looks.iter().find(|o| o.sku_id == s)).map(|o| o.look);
                        let Some(me) = self.me.as_ref().map(|m| m.id) else { return };
                        let mut people = self.people.lock();
                        if let Some(mut user) = people.user(me).cloned() {
                            match (slot, look) {
                                (Slot::Decoration, Some(Wearable::Decoration(a))) => user.look.decoration = Some(Some(a)),
                                (Slot::Nameplate, Some(Wearable::Nameplate(n))) => user.look.nameplate = Some(Some(n)),
                                (Slot::Decoration, _) => user.look.decoration = Some(None),
                                (Slot::Nameplate, _) => user.look.nameplate = Some(None),
                            }
                            people.upsert_user(user);
                        }
                    }
                    Err(r) => self.tags.refused = Some(r),
                }
                return;
            }
        };
        self.tags.busy = false;
        match result {
            Ok(()) => {
                // Discord l'annonce aussi par la passerelle ; en attendant, mon nom porte déjà le nouveau tag.
                let tag = guild.and_then(|g| self.guilds.iter().find(|x| x.id == g)).and_then(|g| g.tag);
                let Some(me) = self.me.as_ref().map(|m| m.id) else { return };
                let mut people = self.people.lock();
                if let Some(mut user) = people.user(me).cloned() {
                    user.look.tag = Some(tag);
                    people.upsert_user(user);
                }
            }
            Err(r) => self.tags.refused = Some(r),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::{Guild, User};
    use crate::state::{Event, Problem};

    fn store() -> Store {
        let mut tagged = Guild::new(9, "Avec tag".into(), Vec::new());
        tagged.tag = GuildTag::new(9, "RUST", None);
        let mut s = Store::new();
        s.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: vec![tagged, Guild::new(8, "Sans tag".into(), Vec::new())], dms: vec![] });
        // Mon utilisateur, comme la passerelle le verse à la connexion.
        let me = crate::state::people::UserInfo { id: 1, username: "moi".into(), global_name: None, discriminator: None, avatar: crate::state::model::Avatar::default_for(1), banner: None, accent: None, flags: None, bot: false, system: false, look: Default::default() };
        s.people.lock().upsert_user(me);
        s.take_commands();
        s
    }

    #[test]
    fn one_request_per_click_and_only_for_a_tagged_guild() {
        let mut s = store();
        assert_eq!(s.tag_choices().iter().map(|t| (t.guild_id, t.text().to_owned())).collect::<Vec<_>>(), vec![(9, "RUST".to_owned())]);
        assert!(!s.set_guild_tag(Some(8)), "serveur sans tag");
        assert!(!s.set_guild_tag(None), "aucun tag porté : rien à retirer");
        assert!(s.set_guild_tag(Some(9)));
        assert!(!s.set_guild_tag(None), "une requête à la fois");
        assert_eq!(s.take_commands(), vec![Cmd::Tags(TagCmd::Set { guild: Some(9) })]);
        s.apply(Event::Tags(TagEvent::Set { guild: Some(9), result: Ok(()) }));
        assert_eq!(s.own_tag().map(|t| t.guild_id), Some(9));
        assert!(!s.set_guild_tag(Some(9)), "déjà porté");

        assert!(s.set_guild_tag(None));
        s.apply(Event::Tags(TagEvent::Set { guild: None, result: Err(Refused::Captcha) }));
        assert_eq!((s.own_tag().map(|t| t.guild_id), s.tags.refused()), (Some(9), Some(&Refused::Captcha)), "refus : rien ne change");
        assert!(s.set_guild_tag(None));
        assert_eq!(s.tags.refused(), None);
        s.apply(Event::Tags(TagEvent::Set { guild: None, result: Err(Refused::Other(Problem::Forbidden)) }));
        assert!(!s.tags.busy());
    }

    /// Style du nom : les effets que la source ne décrit qu'en lecture
    /// (7 prisme, 8 « gummy ») ne sont jamais écrits ; les autres partent.
    #[test]
    fn name_style_is_only_written_with_a_documented_effect() {
        let mut s = store();
        for effect_id in [7, 8] {
            assert!(!s.set_name_style(Some(StyleEdit { font_id: 3, effect_id, colors: vec![0xff0000, 0x0000ff] })), "effet {effect_id}");
        }
        assert!(s.take_commands().is_empty() && !s.tags.busy());
        let style = StyleEdit { font_id: 3, effect_id: 6, colors: vec![0xff0000, 0x0000ff] };
        assert!(s.set_name_style(Some(style.clone())));
        assert_eq!(s.take_commands(), vec![Cmd::Tags(TagCmd::Style { style: Some(style) })]);
        for (code, writable) in [(1, true), (2, true), (3, true), (4, true), (5, true), (6, true), (7, false), (8, false), (1002, false)] {
            assert_eq!(NameEffect::from_code(code).writable(), writable, "effet {code}");
        }
    }
}
