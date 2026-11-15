//! Raccords du magasin pour le lot FF (manques fonctionnels de la
//! vérification finale). Module enfant de `store`, tenu à part pour les fusions.

use super::super::model::ChannelKind;
use super::Store;

/// Ce qu'on fait d'un média marqué « contenu sensible » (F8).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sensitive {
    /// Montré comme les autres.
    #[default]
    Show,
    /// Masqué jusqu'au clic.
    Blur,
    /// Masqué, sans moyen de le dévoiler.
    Block,
}

impl Store {
    /// Clic sur une mention de salon (F3) : ouvre ce salon, dans son serveur.
    /// Faux s'il est inconnu, masqué, ou si c'est une catégorie.
    pub fn go_to_channel(&mut self, id: u64) -> bool {
        let category = self.guilds.iter().flat_map(|g| g.channels.iter()).any(|c| c.id == id && c.kind == ChannelKind::Category);
        if category || !self.channel_exists(id) || self.is_hidden(id) {
            return false;
        }
        if let Some(g) = self.chan_guild.get(&id).copied().or_else(|| self.threads.get(id).map(|t| t.guild_id)) {
            self.guild = Some(g);
        }
        self.select_channel(id);
        true
    }

    /// Salon dont porte la limite d'âge de `channel` : lui-même, ou le salon
    /// parent d'un fil.
    fn age_owner(&self, channel: u64) -> u64 {
        self.threads.get(channel).map_or(channel, |t| t.parent_id)
    }

    /// Salon à limite d'âge dont l'écran d'entrée n'a pas encore été passé
    /// (F8) : ses messages ne sont pas montrés. Un fil suit son salon.
    pub fn age_gated(&self, channel: u64) -> bool {
        self.threads.age_gated(self.age_owner(channel))
    }

    /// « Continuer » sur l'écran d'entrée : vaut pour la session.
    pub fn pass_age_gate(&mut self, channel: u64) {
        let owner = self.age_owner(channel);
        self.threads.pass_age_gate(owner);
    }

    /// « Retour » sur l'écran d'entrée : le premier salon textuel du serveur
    /// qui n'a pas de limite d'âge. Faux s'il n'y en a pas (rien ne change).
    pub fn leave_age_gate(&mut self, channel: u64) -> bool {
        let owner = self.age_owner(channel);
        let other = self.visible_channels(owner).into_iter().find(|c| matches!(c.kind, ChannelKind::Text | ChannelKind::Announcement) && !self.threads.age_restricted(c.id)).map(|c| c.id);
        match other {
            Some(id) => {
                self.select_channel(id);
                true
            }
            None => false,
        }
    }

    /// Sort réservé aux médias que Discord a marqués « contenu sensible »
    /// dans ce salon (F8), d'après le réglage du compte « Filtre de contenu
    /// explicite » : serveurs, MP d'amis, autres MP. Sans réglage choisi,
    /// le défaut de Discord pour un adulte : flouté hors des amis en MP.
    pub fn sensitive_media(&self, channel: u64) -> Sensitive {
        use super::super::relations::RelKind;
        use super::super::usersettings::schema::{Key, SValue};
        let (key, default) = if self.chan_guild.contains_key(&channel) || self.threads.get(channel).is_some() {
            (Key::ExplicitGuilds, Sensitive::Show)
        } else {
            let friends = self.dms.iter().find(|d| d.id == channel).is_some_and(|d| !d.recipients.is_empty() && d.recipients.iter().all(|(u, _)| self.relations.relation(*u).is_some_and(|r| r.kind == RelKind::Friend)));
            if friends { (Key::ExplicitFriendDm, Sensitive::Show) } else { (Key::ExplicitOtherDm, Sensitive::Blur) }
        };
        match self.account.value(key) {
            Some(SValue::Enum(1)) => Sensitive::Show,
            Some(SValue::Enum(2)) => Sensitive::Blur,
            Some(SValue::Enum(3)) => Sensitive::Block,
            _ => default,
        }
    }

    /// MP ou groupe où mon appel sonne encore chez quelqu'un d'autre (F11,
    /// son de l'appel sortant) : j'y suis en vocal et la liste des personnes
    /// qu'on fait sonner n'est pas vide.
    pub fn calls_ringing_out(&self) -> Option<u64> {
        let me = self.me.as_ref()?.id;
        let channel = self.my_voice_channel(None)?;
        self.calls.call(channel).filter(|c| c.ringing.iter().any(|u| *u != me)).map(|c| c.channel_id)
    }

    /// Membres connus localement qui portent le rôle `role` sur le serveur
    /// `guild` (F3, fenêtre d'une mention de rôle) : (identifiant, nom
    /// affiché), triés par nom. Aucune requête : ce que la liste des membres,
    /// les messages et les profils ont déjà appris.
    pub fn known_role_members(&self, guild: u64, role: u64) -> Vec<(u64, String)> {
        let people = self.people.lock();
        let mut out: Vec<(u64, String)> = people
            .members_of(guild)
            .filter(|(_, m)| m.roles.as_ref().is_some_and(|r| r.contains(&role)))
            .map(|(id, m)| {
                let name = m.nick.clone().or_else(|| people.user(id).map(|u| u.display_name().to_string())).unwrap_or_else(|| id.to_string());
                (id, name)
            })
            .collect();
        out.sort_by_cached_key(|(id, name)| (name.to_lowercase(), *id));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::model::{Channel, ChannelKind, Guild, User};
    use super::super::super::people::MemberInfo;
    use super::super::super::Event;
    use super::Store;

    fn store() -> Store {
        let chan = |id, kind| Channel { id, name: format!("s{id}"), kind, position: id as i32, parent: None, overwrites: Some(Vec::new()), slowmode: 0 };
        let mut s = Store::new();
        s.apply(Event::Ready {
            me: User::new(1, "Moi".into()),
            guilds: vec![Guild::new(9, "A".into(), vec![chan(100, ChannelKind::Text)]), Guild::new(8, "B".into(), vec![chan(200, ChannelKind::Text), chan(201, ChannelKind::Category)])],
            dms: Vec::new(),
        });
        s
    }

    #[test]
    fn channel_mention_opens_the_channel_in_its_guild() {
        let mut s = store();
        s.select_guild(9);
        assert!(s.go_to_channel(200));
        assert_eq!((s.guild, s.channel), (Some(8), Some(200)));
        assert!(!s.go_to_channel(201), "catégorie");
        assert!(!s.go_to_channel(999), "inconnu");
        assert_eq!(s.channel, Some(200));
    }

    #[test]
    fn age_gate_hides_a_channel_until_passed() {
        use super::super::super::threads::ThreadEvent;
        let mut s = store();
        s.apply(Event::Threads(ThreadEvent::AgeGates { guild_id: 8, channels: vec![200] }));
        assert!(s.age_gated(200) && !s.age_gated(100));
        s.select_guild(8);
        s.select_channel(200);
        assert!(!s.leave_age_gate(200), "aucun autre salon textuel sur ce serveur");
        s.pass_age_gate(200);
        assert!(!s.age_gated(200));
        // Le serveur revient (reconnexion) : l'écran passé le reste.
        s.apply(Event::Threads(ThreadEvent::AgeGates { guild_id: 8, channels: vec![200] }));
        assert!(!s.age_gated(200));
        // La limite retirée puis remise : l'écran revient.
        s.apply(Event::Threads(ThreadEvent::AgeGate { guild_id: 8, channel_id: 200, on: false }));
        s.apply(Event::Threads(ThreadEvent::AgeGate { guild_id: 8, channel_id: 200, on: true }));
        assert!(s.age_gated(200));
        s.apply(Event::Threads(ThreadEvent::AgeGate { guild_id: 9, channel_id: 100, on: true }));
        s.select_guild(9);
        assert!(!s.leave_age_gate(100));
    }

    #[test]
    fn sensitive_media_follow_the_account_setting() {
        use super::super::super::model::Dm;
        use super::Sensitive;
        let mut s = store();
        assert_eq!(s.sensitive_media(100), Sensitive::Show, "serveur, sans réglage");
        s.dms.push(Dm::new(500, "Inconnu".into(), 0));
        assert_eq!(s.sensitive_media(500), Sensitive::Blur, "MP hors amis, sans réglage");
    }

    #[test]
    fn role_members_come_from_what_is_already_known() {
        let s = store();
        {
            let mut p = s.people.lock();
            p.upsert_member(9, 5, MemberInfo { nick: Some("Zoé".into()), roles: Some(vec![70, 71]), ..Default::default() });
            p.upsert_member(9, 6, MemberInfo { nick: Some("alice".into()), roles: Some(vec![70]), ..Default::default() });
            p.upsert_member(9, 7, MemberInfo { nick: None, roles: Some(vec![71]), ..Default::default() });
            p.upsert_member(8, 5, MemberInfo { roles: Some(vec![70]), ..Default::default() });
        }
        assert_eq!(s.known_role_members(9, 70), vec![(6, "alice".to_string()), (5, "Zoé".to_string())]);
        assert_eq!(s.known_role_members(9, 72), Vec::new());
    }
}
