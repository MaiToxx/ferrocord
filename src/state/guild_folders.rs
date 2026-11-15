//! Dossiers de serveurs du rail (lot S7) : ordre des serveurs et des
//! dossiers, lu et écrit dans les réglages synchronisés
//! (`PreloadedUserSettings.guild_folders`, champ 14).
//!
//! Forme (`discord-userdoccers/discord-protos`,
//! `PreloadedUserSettings.proto`, lue le 13 novembre 2026) :
//! `GuildFolders { repeated GuildFolder folders = 1; repeated fixed64
//! guild_positions = 2; }` et `GuildFolder { repeated fixed64 guild_ids = 1;
//! Int64Value id = 2; StringValue name = 3; UInt64Value color = 4; }`. Un
//! serveur hors dossier est une entrée sans `id` d'un seul serveur.
//!
//! Une écriture renvoie tout le champ 14 (règle de Discord pour un champ de
//! premier niveau), chaque entrée gardant ses champs inconnus et les
//! serveurs que le magasin ne connaît pas (serveur indisponible) : rien
//! n'est perdu de ce qu'on n'affiche pas.

use super::usersettings::proto::{wrapper, DecodeError, Msg, Value};
use super::usersettings::schema::{top, Folder};
use super::usersettings::{AccountState, ProtoState, Refusal};

/// Une entrée de `folders`, avec son message d'origine (champs inconnus).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub folder: Folder,
    raw: Msg,
}

impl Entry {
    fn lone(guild: u64) -> Entry {
        Entry { folder: Folder { id: None, name: None, color: None, guilds: vec![guild] }, raw: Msg::default() }
    }

    /// Un vrai dossier (et non un serveur seul).
    pub fn is_folder(&self) -> bool {
        is_folder(&self.folder)
    }

    fn encode(&self) -> Vec<u8> {
        let mut m = self.raw.clone();
        let f = &self.folder;
        // Liste compactée, comme l'encodeur officiel (proto3).
        let ids: Vec<u8> = f.guilds.iter().flat_map(|g| g.to_le_bytes()).collect();
        m.set(1, Value::Len(ids));
        match f.id {
            Some(id) => m.set(2, Value::Len(wrapper(Value::Varint(id as u64)).encode())),
            None => m.remove(2),
        }
        match f.name.as_deref().filter(|n| !n.is_empty()) {
            Some(n) => m.set(3, Value::Len(wrapper(Value::Len(n.as_bytes().to_vec())).encode())),
            None => m.remove(3),
        }
        match f.color {
            Some(c) => m.set(4, Value::Len(wrapper(Value::Varint(c)).encode())),
            None => m.remove(4),
        }
        m.encode()
    }
}

pub fn is_folder(f: &Folder) -> bool {
    f.id.is_some() || f.guilds.len() > 1
}

/// Le champ `guild_folders` entier.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Folders {
    pub entries: Vec<Entry>,
    /// Les autres champs de `GuildFolders` (`guild_positions`, inconnus).
    rest: Msg,
}

/// Serveurs connus du magasin absents des réglages : en tête du rail, dans
/// l'ordre du magasin (Discord y met les serveurs rejoints depuis).
pub fn missing(folders: &[Folder], known: &[u64]) -> Vec<u64> {
    known.iter().copied().filter(|g| !folders.iter().any(|f| f.guilds.contains(g))).collect()
}

impl Folders {
    pub fn read(root: &Msg) -> Result<Folders, DecodeError> {
        let mut out = Folders::default();
        let Some(gf) = root.message(top::GUILD_FOLDERS)? else { return Ok(out) };
        for field in gf.fields {
            match (&field.num, &field.value) {
                (1, Value::Len(b)) => {
                    let m = Msg::decode(b)?;
                    let folder = Folder {
                        guilds: m.repeated_fixed64(1),
                        id: m.message(2)?.and_then(|w| w.varint(1)).map(|v| v as i64),
                        name: m.message(3)?.and_then(|w| w.string(1)).filter(|s| !s.is_empty()),
                        color: m.message(4)?.and_then(|w| w.varint(1)),
                    };
                    out.entries.push(Entry { folder, raw: m });
                }
                _ => out.rest.fields.push(field),
            }
        }
        Ok(out)
    }

    /// Ajoute en tête les serveurs connus absents (même règle que [`missing`]).
    pub fn normalize(&mut self, known: &[u64]) {
        let folders: Vec<Folder> = self.entries.iter().map(|e| e.folder.clone()).collect();
        let lone: Vec<Entry> = missing(&folders, known).into_iter().map(Entry::lone).collect();
        self.entries.splice(0..0, lone);
    }

    /// Message `GuildFolders` à écrire. `guild_positions`, s'il était là,
    /// suit le nouvel ordre.
    pub fn encode(&self) -> Msg {
        let mut m = Msg::default();
        for e in &self.entries {
            m.fields.push(super::usersettings::proto::Field { num: 1, value: Value::Len(e.encode()) });
        }
        for f in &self.rest.fields {
            if f.num == 2 {
                if !m.has(2) {
                    let flat: Vec<u8> = self.entries.iter().flat_map(|e| e.folder.guilds.iter()).flat_map(|g| g.to_le_bytes()).collect();
                    m.fields.push(super::usersettings::proto::Field { num: 2, value: Value::Len(flat) });
                }
            } else {
                m.fields.push(f.clone());
            }
        }
        m
    }

    fn find(&self, guild: u64) -> Option<(usize, usize)> {
        self.entries.iter().enumerate().find_map(|(i, e)| e.folder.guilds.iter().position(|g| *g == guild).map(|p| (i, p)))
    }

    /// Applique `edit` ; `false` si rien ne change (ou cible invalide).
    /// `new_id` : identifiant d'un dossier créé (unique parmi les dossiers).
    pub fn apply(&mut self, edit: &Edit, new_id: i64) -> bool {
        let before = self.entries.clone();
        match edit {
            Edit::MoveGuild { guild, to } => self.move_guild(*guild, *to, new_id),
            Edit::MoveFolder { entry, before } => {
                let (from, to) = (*entry, *before);
                if from >= self.entries.len() || to > self.entries.len() {
                    return false;
                }
                let e = self.entries.remove(from);
                let to = if to > from { to - 1 } else { to };
                self.entries.insert(to, e);
            }
            Edit::Settings { entry, name, color } => {
                let Some(e) = self.entries.get_mut(*entry).filter(|e| e.is_folder()) else { return false };
                e.folder.name = name.as_ref().map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
                e.folder.color = *color;
            }
            Edit::Ungroup { entry } => {
                let Some(e) = self.entries.get(*entry).filter(|e| e.is_folder()) else { return false };
                let lone: Vec<Entry> = e.folder.guilds.iter().copied().map(Entry::lone).collect();
                self.entries.splice(*entry..=*entry, lone);
            }
        }
        self.entries != before
    }

    fn move_guild(&mut self, guild: u64, to: Target, new_id: i64) {
        let Some((src, pos)) = self.find(guild) else { return };
        let n = self.entries.len();
        let valid = match to {
            Target::Before(i) => i <= n,
            Target::InFolder { entry, at } => self.entries.get(entry).is_some_and(|e| e.is_folder() && at <= e.folder.guilds.len()),
            Target::Combine(i) => i < n && i != src,
        };
        if !valid {
            return;
        }
        // Retiré d'abord, sans effacer l'entrée vidée : les indices de la cible restent justes.
        self.entries[src].folder.guilds.remove(pos);
        match to {
            Target::Before(i) => self.entries.insert(i, Entry::lone(guild)),
            Target::InFolder { entry, at } => {
                let at = if entry == src && at > pos { at - 1 } else { at };
                self.entries[entry].folder.guilds.insert(at, guild);
            }
            Target::Combine(i) => {
                let e = &mut self.entries[i];
                if !e.is_folder() {
                    e.folder.id = Some(new_id);
                }
                e.folder.guilds.push(guild);
            }
        }
        self.entries.retain(|e| !e.folder.guilds.is_empty());
    }

    /// Identifiant libre pour un nouveau dossier, tiré de `seed` (positif,
    /// sur 31 bits comme ceux que crée Discord).
    pub fn fresh_id(&self, seed: u64) -> i64 {
        let mut id = (seed % 0x7fff_fffe) as i64 + 1;
        while self.entries.iter().any(|e| e.folder.id == Some(id)) {
            id = id % 0x7fff_fffe + 1;
        }
        id
    }
}

/// Où déposer un serveur (indices de la liste complète, voir [`layout`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Au premier niveau, avant l'entrée `i` (`len` : à la fin).
    Before(usize),
    /// Dans le dossier `entry`, avant le serveur `at` (`len` : à la fin).
    InFolder { entry: usize, at: usize },
    /// Sur l'entrée `i` : un serveur seul devient un dossier, un dossier reçoit le serveur.
    Combine(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    MoveGuild { guild: u64, to: Target },
    /// Déplace l'entrée `entry` avant l'entrée `before` (`len` : à la fin).
    MoveFolder { entry: usize, before: usize },
    /// Nom et couleur d'un dossier (0xRRGGBB, `None` : couleur par défaut).
    Settings { entry: usize, name: Option<String>, color: Option<u64> },
    /// Le dossier disparaît, ses serveurs restent à sa place.
    Ungroup { entry: usize },
}

/// Élément affiché du rail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Guild { guild: u64, entry: usize },
    /// `guilds` : serveurs connus, avec leur place dans la liste complète du dossier.
    Folder { entry: usize, id: Option<i64>, name: Option<String>, color: Option<u64>, guilds: Vec<(u64, usize)> },
}

/// Rail à afficher : entrées des réglages, précédées des serveurs absents ;
/// seuls les serveurs connus (`known`) sont montrés, un dossier sans serveur
/// connu ne l'est pas. Les indices `entry` sont ceux de [`Folders::normalize`].
pub fn layout(folders: &[Folder], known: &[u64]) -> Vec<Item> {
    let lone = missing(folders, known);
    let mut out: Vec<Item> = lone.iter().enumerate().map(|(i, g)| Item::Guild { guild: *g, entry: i }).collect();
    for (i, f) in folders.iter().enumerate() {
        let entry = lone.len() + i;
        let guilds: Vec<(u64, usize)> = f.guilds.iter().enumerate().filter(|(_, g)| known.contains(g)).map(|(i, g)| (*g, i)).collect();
        if guilds.is_empty() {
            continue;
        }
        if is_folder(f) {
            out.push(Item::Folder { entry, id: f.id, name: f.name.clone(), color: f.color, guilds });
        } else {
            out.extend(guilds.into_iter().map(|(guild, _)| Item::Guild { guild, entry }));
        }
    }
    out
}

/// Écrit une modification des dossiers. `Ok(false)` : rien ne change, rien
/// n'est envoyé. Réglages inconnus ou illisibles : refusé.
pub fn write(account: &mut AccountState, known: &[u64], edit: &Edit, seed: u64) -> Result<bool, Refusal> {
    if account.proto_busy() {
        return Err(Refusal::Busy);
    }
    let ProtoState::Ready(root) = account.proto_state() else { return Err(Refusal::Unknown) };
    let mut f = Folders::read(root).map_err(|_| Refusal::Unknown)?;
    f.normalize(known);
    let id = f.fresh_id(seed);
    if !f.apply(edit, id) {
        return Ok(false);
    }
    let body = f.encode().encode();
    account.edit_top(top::GUILD_FOLDERS, move |root| {
        root.set(top::GUILD_FOLDERS, Value::Len(body));
        true
    })?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::super::usersettings::proto::Field;
    use super::super::usersettings::{AccountCmd, AccountEvent};
    use super::*;

    fn folder(guilds: &[u64], id: Option<i64>, name: Option<&str>) -> Msg {
        let mut m = Msg::default();
        m.set(1, Value::Len(guilds.iter().flat_map(|g| g.to_le_bytes()).collect()));
        if let Some(id) = id {
            m.set(2, Value::Len(wrapper(Value::Varint(id as u64)).encode()));
        }
        if let Some(n) = name {
            m.set(3, Value::Len(wrapper(Value::Len(n.as_bytes().to_vec())).encode()));
        }
        m
    }

    /// Réglages : un dossier « Jeux » (1, 2, et 99 que le magasin ne connaît
    /// pas, avec un champ inconnu), le serveur 3 seul, `guild_positions` et
    /// un champ inconnu dans `GuildFolders`, une autre section intacte.
    fn root() -> Msg {
        let mut games = folder(&[1, 2, 99], Some(7), Some("Jeux"));
        games.set(40, Value::Len(b"futur".to_vec()));
        let mut gf = Msg::default();
        gf.fields.push(Field { num: 1, value: Value::Len(games.encode()) });
        gf.fields.push(Field { num: 1, value: Value::Len(folder(&[3], None, None).encode()) });
        gf.fields.push(Field { num: 2, value: Value::Len([1u64, 2, 99, 3].iter().flat_map(|g| g.to_le_bytes()).collect()) });
        gf.fields.push(Field { num: 9, value: Value::Varint(5) });
        let mut r = Msg::default();
        r.set(top::TEXT_AND_IMAGES, Value::Len(b"\x50\x01".to_vec()));
        r.set(top::GUILD_FOLDERS, Value::Len(gf.encode()));
        r
    }

    const KNOWN: [u64; 4] = [4, 1, 2, 3];

    fn folders() -> Folders {
        let mut f = Folders::read(&root()).unwrap();
        f.normalize(&KNOWN);
        f
    }

    fn order(f: &Folders) -> Vec<(Option<i64>, Vec<u64>)> {
        f.entries.iter().map(|e| (e.folder.id, e.folder.guilds.clone())).collect()
    }

    #[test]
    fn layout_puts_new_guilds_first_and_hides_unknown_ones() {
        let snap = super::super::usersettings::schema::read_folders(&root()).unwrap();
        let items = layout(&snap, &KNOWN);
        assert_eq!(
            items,
            vec![
                Item::Guild { guild: 4, entry: 0 },
                Item::Folder { entry: 1, id: Some(7), name: Some("Jeux".into()), color: None, guilds: vec![(1, 0), (2, 1)] },
                Item::Guild { guild: 3, entry: 2 },
            ]
        );
        // Mêmes indices que la liste complète écrite.
        assert_eq!(order(&folders()), vec![(None, vec![4]), (Some(7), vec![1, 2, 99]), (None, vec![3])]);
        // Aucun réglage : l'ordre du magasin.
        assert_eq!(layout(&[], &[5, 6]), vec![Item::Guild { guild: 5, entry: 0 }, Item::Guild { guild: 6, entry: 1 }]);
    }

    #[test]
    fn combining_two_guilds_creates_a_folder() {
        let mut f = folders();
        assert!(f.apply(&Edit::MoveGuild { guild: 4, to: Target::Combine(2) }, 42));
        assert_eq!(order(&f), vec![(Some(7), vec![1, 2, 99]), (Some(42), vec![3, 4])]);
        // Sur un dossier : ajouté à la fin.
        assert!(f.apply(&Edit::MoveGuild { guild: 3, to: Target::Combine(0) }, 43));
        assert_eq!(order(&f), vec![(Some(7), vec![1, 2, 99, 3]), (Some(42), vec![4])]);
        // Sur lui-même : rien.
        assert!(!f.apply(&Edit::MoveGuild { guild: 4, to: Target::Combine(1) }, 44));
    }

    #[test]
    fn moving_in_and_out_of_folders_keeps_unknown_guilds() {
        let mut f = folders();
        // Sorti du dossier, avant le serveur 4.
        assert!(f.apply(&Edit::MoveGuild { guild: 2, to: Target::Before(0) }, 1));
        assert_eq!(order(&f), vec![(None, vec![2]), (None, vec![4]), (Some(7), vec![1, 99]), (None, vec![3])]);
        // Dans le dossier, en tête.
        assert!(f.apply(&Edit::MoveGuild { guild: 3, to: Target::InFolder { entry: 2, at: 0 } }, 1));
        assert_eq!(order(&f), vec![(None, vec![2]), (None, vec![4]), (Some(7), vec![3, 1, 99])]);
        // Dans le même dossier, plus loin : l'indice tient compte du retrait.
        assert!(f.apply(&Edit::MoveGuild { guild: 3, to: Target::InFolder { entry: 2, at: 2 } }, 1));
        assert_eq!(order(&f), vec![(None, vec![2]), (None, vec![4]), (Some(7), vec![1, 3, 99])]);
        // Déposé à sa place : rien ne change, rien ne part.
        assert!(!f.apply(&Edit::MoveGuild { guild: 2, to: Target::Before(0) }, 1));
        assert!(!f.apply(&Edit::MoveGuild { guild: 2, to: Target::Before(1) }, 1));
        // Le dernier serveur d'un dossier en sort : l'entrée disparaît.
        let mut g = folders();
        for guild in [1, 2, 99] {
            g.apply(&Edit::MoveGuild { guild, to: Target::Before(g.entries.len()) }, 1);
        }
        assert_eq!(order(&g), vec![(None, vec![4]), (None, vec![3]), (None, vec![1]), (None, vec![2]), (None, vec![99])]);
    }

    #[test]
    fn folder_edits() {
        let mut f = folders();
        assert!(f.apply(&Edit::MoveFolder { entry: 1, before: 3 }, 1));
        assert_eq!(order(&f), vec![(None, vec![4]), (None, vec![3]), (Some(7), vec![1, 2, 99])]);
        assert!(!f.apply(&Edit::MoveFolder { entry: 2, before: 3 }, 1));
        assert!(f.apply(&Edit::Settings { entry: 2, name: Some("  Amis ".into()), color: Some(0xff0000) }, 1));
        assert_eq!((f.entries[2].folder.name.as_deref(), f.entries[2].folder.color), (Some("Amis"), Some(0xff0000)));
        assert!(!f.apply(&Edit::Settings { entry: 2, name: Some("Amis".into()), color: Some(0xff0000) }, 1), "inchangé");
        assert!(!f.apply(&Edit::Settings { entry: 0, name: Some("x".into()), color: None }, 1), "pas un dossier");
        assert!(f.apply(&Edit::Ungroup { entry: 2 }, 1));
        assert_eq!(order(&f), vec![(None, vec![4]), (None, vec![3]), (None, vec![1]), (None, vec![2]), (None, vec![99])]);
    }

    #[test]
    fn encoding_keeps_unknown_fields_and_follows_positions() {
        let mut f = folders();
        f.apply(&Edit::Settings { entry: 1, name: Some("Autres".into()), color: Some(0x123456) }, 1);
        let m = f.encode();
        let mut r = Msg::default();
        r.set(top::GUILD_FOLDERS, Value::Len(m.encode()));
        let back = Folders::read(&r).unwrap();
        assert_eq!(back.entries.len(), 3);
        let games = &back.entries[1];
        assert_eq!((games.folder.id, games.folder.name.as_deref(), games.folder.color), (Some(7), Some("Autres"), Some(0x123456)));
        assert_eq!(games.raw.bytes(40), Some(&b"futur"[..]), "champ inconnu du dossier gardé");
        assert_eq!(m.varint(9), Some(5), "champ inconnu de GuildFolders gardé");
        assert_eq!(m.repeated_fixed64(2), vec![4, 1, 2, 99, 3], "guild_positions suit l'ordre");
        assert_eq!(f.fresh_id(6), 7 + 1, "7 est pris");
    }

    #[test]
    fn write_sends_only_the_folders_field() {
        let mut a = AccountState::default();
        assert_eq!(write(&mut a, &KNOWN, &Edit::Ungroup { entry: 1 }, 1), Err(Refusal::Unknown));
        a.apply(AccountEvent::Proto { bytes: Some(root().encode()), partial: false });
        assert_eq!(write(&mut a, &KNOWN, &Edit::MoveFolder { entry: 2, before: 3 }, 1), Ok(false));
        assert!(a.take_commands().is_empty(), "rien ne change : rien ne part");
        assert_eq!(write(&mut a, &KNOWN, &Edit::MoveGuild { guild: 4, to: Target::Combine(2) }, 1000), Ok(true));
        let cmds = a.take_commands();
        let [AccountCmd::PatchProto { body, .. }] = cmds.as_slice() else { panic!("{cmds:?}") };
        let sent = Msg::decode(body).unwrap();
        assert_eq!(sent.top_numbers(), vec![top::GUILD_FOLDERS]);
        let back = Folders::read(&sent).unwrap();
        assert_eq!(back.entries.iter().map(|e| e.folder.guilds.clone()).collect::<Vec<_>>(), vec![vec![1, 2, 99], vec![3, 4]]);
        // Optimiste : le rail suit tout de suite.
        assert_eq!(a.snapshot().folders.len(), 2);
        assert_eq!(write(&mut a, &KNOWN, &Edit::Ungroup { entry: 1 }, 1), Err(Refusal::Busy));
    }
}
