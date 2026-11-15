//! Emojis personnalisés et autocollants que le sélecteur propose dans un salon.

use crate::state::model::{Emoji, Guild};
use crate::state::perms::{Permissions, Verdict};
use crate::state::Store;

use super::GuildSticker;

impl Store {
    /// Abonnement Nitro connu (profil chargé) : emojis des autres serveurs permis.
    pub fn has_nitro(&self) -> bool {
        self.account.profile.ready().is_some_and(|p| p.premium != 0)
    }

    /// Nitro complet (`premium_type` 2) : super réactions. Nitro Basic (3) et
    /// Classic (1) ont les emojis de partout, pas les super réactions.
    pub fn has_full_nitro(&self) -> bool {
        self.account.profile.ready().is_some_and(|p| p.premium == 2)
    }

    /// Emojis personnalisés utilisables dans `channel`, par serveur : ceux
    /// du serveur du salon (disponibles, rôles permis), puis, avec Nitro et
    /// la permission USE_EXTERNAL_EMOJIS, ceux des autres serveurs.
    pub fn picker_emojis(&self, channel: u64) -> Vec<(&Guild, Vec<&Emoji>)> {
        let here = self.guild_of(channel);
        let mut out = Vec::new();
        if let Some(g) = here.and_then(|id| self.guilds.iter().find(|g| g.id == id)) {
            let list = self.usable_emojis(channel);
            if !list.is_empty() {
                out.push((g, list));
            }
        }
        let external = self.has_nitro() && (here.is_none() || self.can(channel, Permissions::USE_EXTERNAL_EMOJIS) != Verdict::Refuse);
        if external {
            for g in self.guilds.iter().filter(|g| Some(g.id) != here) {
                let mine = g.me.as_ref().map(|m| &m.roles);
                let list: Vec<&Emoji> =
                    g.emojis.iter().filter(|e| e.available && (e.roles.is_empty() || mine.is_none_or(|r| e.roles.iter().any(|x| r.contains(x))))).collect();
                if !list.is_empty() {
                    out.push((g, list));
                }
            }
        }
        out
    }

    /// Emojis des autres serveurs que le compte ne peut pas utiliser dans
    /// `channel` faute de Nitro (F26) : le sélecteur les montre grisés, avec
    /// l'explication, au lieu de les taire. Vide avec Nitro.
    pub fn locked_emojis(&self, channel: u64) -> Vec<(&Guild, Vec<&Emoji>)> {
        if self.has_nitro() || self.account.profile.ready().is_none() {
            return Vec::new();
        }
        let here = self.guild_of(channel);
        self.guilds
            .iter()
            .filter(|g| Some(g.id) != here)
            .filter_map(|g| {
                let list: Vec<&Emoji> = g.emojis.iter().filter(|e| e.available).collect();
                (!list.is_empty()).then_some((g, list))
            })
            .collect()
    }

    /// Autocollants du serveur de `channel` (disponibles) : ceux qu'on peut
    /// envoyer sans Nitro. Hors serveur : aucun.
    pub fn picker_stickers(&self, channel: u64) -> Vec<&GuildSticker> {
        let Some(g) = self.guild_of(channel) else { return Vec::new() };
        self.stickers.guild(g).iter().filter(|s| s.available).collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::state::emoji::{GuildSticker, StickerEvent, StickerFormat};
    use crate::state::model::{Channel, ChannelKind, Emoji, Guild, User};
    use crate::state::{Event, Store};

    fn emoji(id: u64, roles: Vec<u64>) -> Emoji {
        Emoji { id, name: format!("e{id}"), animated: false, available: id != 3, roles }
    }

    #[test]
    fn current_guild_only_without_nitro() {
        let mut s = Store::new();
        let chan = Channel { id: 100, name: "g".into(), kind: ChannelKind::Text, position: 0, parent: None, overwrites: Some(Vec::new()), slowmode: 0 };
        let mut g = Guild::new(9, "Ici".into(), vec![chan]);
        g.emojis = vec![emoji(1, Vec::new()), emoji(3, Vec::new())];
        let mut other = Guild::new(8, "Ailleurs".into(), Vec::new());
        other.emojis = vec![emoji(2, Vec::new())];
        s.apply(Event::Ready { me: User::new(1, "moi".into()), guilds: vec![g, other], dms: Vec::new() });
        let got: Vec<(u64, Vec<u64>)> = s.picker_emojis(100).iter().map(|(g, l)| (g.id, l.iter().map(|e| e.id).collect())).collect();
        assert_eq!(got, [(9, vec![1])]);
        // Profil pas encore lu : on ne sait pas s'il y a Nitro, rien n'est montré verrouillé.
        assert!(s.locked_emojis(100).is_empty());
        let stickers = vec![
            GuildSticker { id: 5, name: "a".into(), format: StickerFormat::Png, available: true },
            GuildSticker { id: 6, name: "b".into(), format: StickerFormat::Png, available: false },
        ];
        s.apply(Event::Stickers(StickerEvent::Guild { guild_id: 9, stickers }));
        assert_eq!(s.picker_stickers(100).iter().map(|s| s.id).collect::<Vec<_>>(), [5]);
        assert!(s.picker_stickers(12345).is_empty());
        s.apply(Event::GuildDelete(9));
        assert!(s.stickers.guild(9).is_empty());
    }
}
