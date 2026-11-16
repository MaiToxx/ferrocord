//! Compte simulé « réaliste » du relevé de mémoire (lot « ram »).
//!
//! Le magasin du banc de vitesse est construit à la main (serveurs et salons
//! nus) : il ne dit rien de ce qu'un vrai READY laisse en mémoire. Ici, un
//! READY de compte utilisateur est écrit en JSON (serveurs avec rôles, emojis,
//! autocollants, salons avec permissions et sujets, MP, relations, états de
//! lecture, présences) et passe par le vrai décodage (`net::Replay`) ; de même
//! la liste des jeux détectables, dans la forme que renvoie Discord.
//!
//! Tout est inventé : identifiants construits, noms génériques, aucune donnée
//! d'un vrai compte.

use std::fmt::Write as _;

/// Taille du compte simulé.
#[derive(Clone, Copy, Debug)]
pub struct Size {
    pub guilds: usize,
    pub channels: usize,
    pub roles: usize,
    pub emojis: usize,
    pub stickers: usize,
    pub users: usize,
    pub dms: usize,
    pub relations: usize,
    pub reads: usize,
    pub presences: usize,
    /// Applications de la liste des jeux détectables.
    pub games: usize,
}

impl Default for Size {
    /// Gros compte : 100 serveurs de 50 salons (les 5 000 salons du banc).
    fn default() -> Self {
        Size { guilds: 100, channels: 50, roles: 30, emojis: 80, stickers: 15, users: 400, dms: 150, relations: 300, reads: 2000, presences: 200, games: 10_000 }
    }
}

impl Size {
    /// Le même compte, `num / den` fois plus grand ou plus petit (serveurs, MP,
    /// relations, lectures) : ce qui dépend de la taille du compte se lit à la pente.
    pub fn scaled(self, num: usize, den: usize) -> Size {
        let s = |v: usize| (v * num / den).max(1);
        Size { guilds: s(self.guilds), users: s(self.users), dms: s(self.dms), relations: s(self.relations), reads: s(self.reads), presences: s(self.presences), ..self }
    }
}

pub const ME: u64 = 1_000_000_000_000_000_001;
const GUILD: u64 = 6_000_000_000_000_000_000;
const CHANNEL: u64 = 6_100_000_000_000_000_000;
const ROLE: u64 = 6_200_000_000_000_000_000;
const EMOJI: u64 = 6_300_000_000_000_000_000;
const STICKER: u64 = 6_400_000_000_000_000_000;
const USER: u64 = 2_000_000_000_000_000_000;
const DM: u64 = 6_500_000_000_000_000_000;
const APP: u64 = 6_600_000_000_000_000_000;

pub fn guild_id(g: usize) -> u64 {
    GUILD + g as u64
}

pub fn channel_id(g: usize, c: usize) -> u64 {
    CHANNEL + (g as u64) * 1000 + c as u64
}

/// Serveur d'un salon du compte simulé.
pub fn guild_of(channel: u64) -> u64 {
    GUILD + channel.saturating_sub(CHANNEL) / 1000
}

/// Empreinte d'image inventée (32 chiffres hexadécimaux), stable pour `seed`.
fn hash(seed: u64) -> String {
    let a = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x5851_F42D_4C95_7F2D;
    let b = a.rotate_left(29).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    format!("{a:016x}{b:016x}")
}

fn user(out: &mut String, i: usize) {
    let id = USER + i as u64;
    let _ = write!(out, r#"{{"id":"{id}","username":"personne_{i}","global_name":"Personne {i}","avatar":"{}","discriminator":"0","public_flags":0}}"#, hash(id));
}

fn guild(out: &mut String, g: usize, s: &Size) {
    let id = guild_id(g);
    let _ = write!(
        out,
        r#"{{"id":"{id}","properties":{{"name":"Serveur numéro {g}","owner_id":"{}","icon":"{}","premium_tier":2,"premium_subscription_count":9,"features":["COMMUNITY","NEWS","ROLE_ICONS","ANIMATED_ICON"]}},"channels":["#,
        USER + (g as u64 % 7),
        hash(id)
    );
    let cats = 5.min(s.channels);
    for c in 0..s.channels {
        if c > 0 {
            out.push(',');
        }
        let cid = channel_id(g, c);
        if c < cats {
            let _ = write!(out, r#"{{"id":"{cid}","type":4,"name":"Catégorie {c}","position":{c},"permission_overwrites":[{{"id":"{id}","type":0,"allow":"0","deny":"0"}}]}}"#);
        } else {
            // Un salon sur dix est vocal ; les autres ont un sujet et trois permissions.
            let kind = if c % 10 == 9 { 2 } else { 0 };
            let _ = write!(
                out,
                r#"{{"id":"{cid}","type":{kind},"name":"salon-de-discussion-{c}","position":{c},"parent_id":"{}","last_message_id":"{}","rate_limit_per_user":0,"nsfw":false,"topic":"Sujet du salon {c} : de quoi l'on parle ici, en une phrase.","permission_overwrites":[{{"id":"{id}","type":0,"allow":"0","deny":"1024"}},{{"id":"{}","type":0,"allow":"3072","deny":"0"}},{{"id":"{}","type":1,"allow":"1024","deny":"0"}}]}}"#,
                channel_id(g, c % cats),
                cid + 500,
                ROLE + (g as u64) * 1000 + 1,
                USER + (c as u64 % 50),
            );
        }
    }
    out.push_str(r#"],"roles":["#);
    for r in 0..s.roles {
        if r > 0 {
            out.push(',');
        }
        // Le rôle 0 est @everyone (identifiant du serveur).
        let rid = if r == 0 { id } else { ROLE + (g as u64) * 1000 + r as u64 };
        let _ = write!(
            out,
            r#"{{"id":"{rid}","name":"Rôle du serveur {r}","color":{},"position":{r},"permissions":"1071698660929","hoist":{},"mentionable":false,"managed":false,"flags":0,"icon":null,"unicode_emoji":null}}"#,
            (r * 523_711) % 0xFF_FFFF,
            r % 3 == 0
        );
    }
    out.push_str(r#"],"emojis":["#);
    for e in 0..s.emojis {
        if e > 0 {
            out.push(',');
        }
        let _ = write!(out, r#"{{"id":"{}","name":"emoji_du_serveur_{e}","animated":{},"available":true,"roles":[],"require_colons":true,"managed":false}}"#, EMOJI + (g as u64) * 1000 + e as u64, e % 5 == 0);
    }
    out.push_str(r#"],"stickers":["#);
    for k in 0..s.stickers {
        if k > 0 {
            out.push(',');
        }
        let _ = write!(out, r#"{{"id":"{}","name":"Autocollant {k}","format_type":1,"available":true,"description":"Un autocollant du serveur.","tags":"sourire","type":2,"guild_id":"{id}"}}"#, STICKER + (g as u64) * 1000 + k as u64);
    }
    out.push_str(r#"],"threads":[]}"#);
}

/// Champ `d` d'un READY de compte utilisateur.
pub fn ready(s: &Size) -> String {
    let mut out = String::with_capacity(s.guilds * (s.channels * 420 + s.roles * 190 + s.emojis * 130 + s.stickers * 170) + 400_000);
    let _ = write!(out, r#"{{"user":{{"id":"{ME}","username":"moi_fictif","global_name":"Moi","avatar":"{}"}},"session_id":"session-du-banc","resume_gateway_url":"wss://gateway.discord.gg","guilds":["#, hash(ME));
    for g in 0..s.guilds {
        if g > 0 {
            out.push(',');
        }
        guild(&mut out, g, s);
    }
    out.push_str(r#"],"merged_members":["#);
    for g in 0..s.guilds {
        if g > 0 {
            out.push(',');
        }
        let role = |r: u64| ROLE + (g as u64) * 1000 + r;
        let _ = write!(out, r#"[{{"user_id":"{ME}","roles":["{}","{}","{}"],"joined_at":"2021-03-04T10:11:12.000000+00:00","nick":null}}]"#, role(1), role(2), role(3));
    }
    out.push_str(r#"],"user_settings":{"guild_positions":["#);
    for g in 0..s.guilds {
        if g > 0 {
            out.push(',');
        }
        let _ = write!(out, r#""{}""#, guild_id(g));
    }
    out.push_str(r#"]},"users":["#);
    for i in 0..s.users {
        if i > 0 {
            out.push(',');
        }
        user(&mut out, i);
    }
    out.push_str(r#"],"private_channels":["#);
    for i in 0..s.dms {
        if i > 0 {
            out.push(',');
        }
        let id = DM + i as u64;
        let _ = write!(out, r#"{{"id":"{id}","type":1,"last_message_id":"{}","recipient_ids":["{}"]}}"#, id + 900 - i as u64, USER + (i % s.users.max(1)) as u64);
    }
    out.push_str(r#"],"relationships":["#);
    for i in 0..s.relations {
        if i > 0 {
            out.push(',');
        }
        let id = USER + (i % s.users.max(1)) as u64;
        let _ = write!(out, r#"{{"id":"{id}","type":1,"nickname":null,"user_id":"{id}"}}"#);
    }
    out.push_str(r#"],"read_state":{"version":3,"partial":false,"entries":["#);
    for i in 0..s.reads {
        if i > 0 {
            out.push(',');
        }
        // Un salon sur huit garde des messages non lus.
        let cid = channel_id(i % s.guilds.max(1), 5 + (i / s.guilds.max(1)) % s.channels.saturating_sub(5).max(1));
        let _ = write!(out, r#"{{"id":"{cid}","last_message_id":"{}","mention_count":0,"flags":0,"last_pin_timestamp":"1970-01-01T00:00:00+00:00"}}"#, cid + if i % 8 == 0 { 100 } else { 500 });
    }
    out.push_str(r#"]},"presences":["#);
    for i in 0..s.presences {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(out, r#"{{"user":{{"id":"{}"}},"status":"online","client_status":{{"desktop":"online"}},"activities":[]}}"#, USER + (i % s.users.max(1)) as u64);
    }
    out.push_str("]}");
    out
}

/// Réponse de `GET /games/detectable` : `n` applications, un à trois exécutables
/// Windows chacune (plus un exécutable d'un autre système, écarté au décodage).
pub fn detectable(n: usize) -> String {
    let mut out = String::with_capacity(n * 330);
    out.push('[');
    for i in 0..n {
        if i > 0 {
            out.push(',');
        }
        let id = APP + i as u64;
        let _ = write!(out, r#"{{"id":"{id}","name":"Jeu inventé numéro {i}","icon_hash":"{}","aliases":[],"hook":true,"executables":[{{"os":"win32","name":"jeu{i}.exe","is_launcher":false}}"#, hash(id));
        if i % 3 == 0 {
            let _ = write!(out, r#",{{"os":"win32","name":"dossier{i}/bin/win64/jeu{i}-win64-shipping.exe","is_launcher":false}}"#);
        }
        if i % 7 == 0 {
            let _ = write!(out, r#",{{"os":"win32","name":"lanceur{i}.exe","is_launcher":true}},{{"os":"darwin","name":"jeu{i}.app","is_launcher":false}}"#);
        }
        out.push_str("]}");
    }
    out.push(']');
    out
}
