//! Structures brutes de l'API (seuls les champs utiles sont décodés) et
//! leur conversion vers le modèle de `state/`.

use crate::state::model::{
    sort_channels, Avatar, Channel, ChannelKind, Dm, Emoji, Guild, ImageHash, ImageRef, Member, Message, Overwrite, OverwriteKind, Role,
};
use crate::state::{DmKnown, Permissions};
use serde::de::{DeserializeOwned, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;
use serde_json::value::RawValue;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

/// Texte attendu, lu sans copie : `f` reçoit la chaîne décodée. Même
/// exigence que `String::deserialize` (tout autre type est une erreur), mais
/// sans allouer une `String` par identifiant.
struct StrWith<F>(F);

impl<'de, R, F: FnOnce(&str) -> R> Visitor<'de> for StrWith<F> {
    type Value = R;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a string")
    }

    fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<R, E> {
        Ok((self.0)(s))
    }
}

/// Identifiant obligatoire, en chaîne : sinon la structure est illisible.
pub(super) fn de_id<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    d.deserialize_str(StrWith(|s: &str| s.parse::<u64>()))?.map_err(serde::de::Error::custom)
}

fn de_opt_id<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    struct Opt;
    impl<'de> Visitor<'de> for Opt {
        type Value = Option<u64>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a string or null")
        }

        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
            d.deserialize_str(StrWith(|s: &str| s.parse().ok()))
        }
    }
    d.deserialize_option(Opt)
}

/// Nom sous lequel `serde_json` remet le texte brut d'une valeur (celui que
/// lit `RawValue`). Privé mais stable depuis la 1.0 ; le test
/// `raw_text_is_lent_without_copy` casse si une mise à jour le changeait.
const RAW_TOKEN: &str = "$serde_json::private::RawValue";

/// Donne à `f` le texte JSON brut de la prochaine valeur, **sans le copier**
/// quand le texte lu est déjà en mémoire (`from_str`, `from_slice` : tous nos
/// paquets). `Box<RawValue>` recopiait chaque champ lu à part avant de le
/// relire : pour un READY, trois copies emboîtées du nom de chaque rôle.
/// Même texte, mêmes erreurs que `Box::<RawValue>::deserialize`.
pub(super) fn with_raw<'de, D: Deserializer<'de>, R>(d: D, f: impl FnOnce(&str) -> R) -> Result<R, D::Error> {
    struct Outer<F>(F);

    impl<'de, R, F: FnOnce(&str) -> R> Visitor<'de> for Outer<F> {
        type Value = R;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("any valid JSON value")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<R, A::Error> {
            let token = map.next_key_seed(Token)?;
            if token != Some(true) {
                return Err(serde::de::Error::invalid_type(serde::de::Unexpected::Map, &self));
            }
            map.next_value_seed(Text(self.0))
        }
    }

    struct Token;

    impl<'de> DeserializeSeed<'de> for Token {
        type Value = bool;

        fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<bool, D::Error> {
            d.deserialize_str(StrWith(|s: &str| s == RAW_TOKEN))
        }
    }

    struct Text<F>(F);

    impl<'de, R, F: FnOnce(&str) -> R> DeserializeSeed<'de> for Text<F> {
        type Value = R;

        fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<R, D::Error> {
            d.deserialize_str(StrWith(self.0))
        }
    }

    d.deserialize_newtype_struct(RAW_TOKEN, Outer(f))
}

/// Élément décodé à part : s'il a une forme inattendue, il vaut `None` au
/// lieu de faire échouer tout le paquet (le READY entier, sinon). Réservé
/// aux données de permissions, dont la forme utilisateur n'est pas documentée.
pub(super) struct Item<T>(pub Option<T>);

impl<'de, T: DeserializeOwned> Deserialize<'de> for Item<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        with_raw(d, |raw| Item(serde_json::from_str(raw).ok()))
    }
}

// ---------------------------------------------------------------- serveur lu une fois
//
// Un READY ou un GUILD_CREATE est lu par plusieurs décodeurs (principal,
// fonctions du serveur, vocal, conférences), chacun ne gardant que ses champs.
// Chacun relisait tout le texte : pour un READY de 100 serveurs, quatre
// parcours entiers de plusieurs Mo. Le texte est maintenant découpé une fois
// en champs bruts empruntés ([`Parts`]) et chaque décodeur ne lit que les
// champs que sa structure déclare. La tolérance est exactement celle d'avant :
// même structure, mêmes aides, mêmes erreurs (voir les tests
// `parts_read_like_the_whole_text` de ce fichier et ceux des trois décodeurs).

/// Valeur JSON d'un paquet, découpée une fois.
pub(super) enum Parts<'a> {
    /// Objet : ses champs dans l'ordre, valeurs laissées en texte brut.
    Object(Vec<(Cow<'a, str>, &'a RawValue)>),
    /// Tout le reste (tableau, nombre, texte invalide…) : lu comme avant, en entier.
    Other(&'a str),
}

impl<'a> Parts<'a> {
    pub(super) fn new(text: &'a str) -> Parts<'a> {
        struct Fields;

        impl<'de> Visitor<'de> for Fields {
            type Value = Vec<(Cow<'de, str>, &'de RawValue)>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut fields = Vec::with_capacity(48);
                while let Some(key) = map.next_key_seed(Key)? {
                    fields.push((key, map.next_value::<&'de RawValue>()?));
                }
                Ok(fields)
            }
        }

        /// Nom de champ : emprunté au texte, copié seulement s'il porte un échappement.
        struct Key;

        impl<'de> DeserializeSeed<'de> for Key {
            type Value = Cow<'de, str>;

            fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
                d.deserialize_str(self)
            }
        }

        impl<'de> Visitor<'de> for Key {
            type Value = Cow<'de, str>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a field name")
            }

            fn visit_borrowed_str<E: serde::de::Error>(self, s: &'de str) -> Result<Self::Value, E> {
                Ok(Cow::Borrowed(s))
            }

            fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<Self::Value, E> {
                Ok(Cow::Owned(s.to_owned()))
            }
        }

        struct Whole<'a>(Vec<(Cow<'a, str>, &'a RawValue)>);

        impl<'de> Deserialize<'de> for Whole<'de> {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                d.deserialize_map(Fields).map(Whole)
            }
        }

        if !text.trim_start_matches([' ', '\t', '\n', '\r']).starts_with('{') {
            return Parts::Other(text);
        }
        match serde_json::from_str::<Whole>(text) {
            Ok(w) => Parts::Object(w.0),
            Err(_) => Parts::Other(text),
        }
    }

    /// Lit une structure dérivée : seuls les champs qu'elle déclare sont
    /// relus, les autres ne coûtent plus rien. La structure ne doit porter ni
    /// `flatten` (tous les champs seraient relus) ni `alias` (serde ne
    /// déclare que le nom principal : l'autre nom serait écarté).
    pub(super) fn read<T: Deserialize<'a>>(&self) -> Result<T, serde_json::Error> {
        match self {
            Parts::Object(fields) => T::deserialize(PartsDe(fields)),
            Parts::Other(text) => serde_json::from_str(text),
        }
    }
}

struct PartsDe<'p, 'a>(&'p [(Cow<'a, str>, &'a RawValue)]);

struct PartsAccess<'p, 'a> {
    fields: std::slice::Iter<'p, (Cow<'a, str>, &'a RawValue)>,
    /// Champs que la structure déclare ; `None` : tous.
    wanted: Option<&'static [&'static str]>,
    value: Option<&'a RawValue>,
}

impl<'de> MapAccess<'de> for PartsAccess<'_, 'de> {
    type Error = serde_json::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error> {
        for (key, value) in self.fields.by_ref() {
            if self.wanted.is_none_or(|w| w.contains(&key.as_ref())) {
                self.value = Some(value);
                return seed.deserialize(serde::de::value::StrDeserializer::new(key)).map(Some);
            }
        }
        Ok(None)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Self::Error> {
        match self.value.take() {
            Some(raw) => seed.deserialize(raw),
            None => Err(serde::de::Error::custom("valeur demandée sans champ")),
        }
    }
}

impl<'de> Deserializer<'de> for PartsDe<'_, 'de> {
    type Error = serde_json::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_map(PartsAccess { fields: self.0.iter(), wanted: None, value: None })
    }

    fn deserialize_struct<V: Visitor<'de>>(self, _name: &'static str, fields: &'static [&'static str], visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_map(PartsAccess { fields: self.0.iter(), wanted: Some(fields), value: None })
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_some(self)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(self, _name: &'static str, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_unit()
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf unit unit_struct seq tuple tuple_struct map enum identifier
    }
}

/// Ce que les décodeurs d'un même paquet se partagent : découpé une fois par
/// [`Shared::of`], au lieu d'un parcours du texte par décodeur.
pub(super) enum Shared<'a> {
    /// READY et READY_SUPPLEMENTAL : les serveurs, un par un ; `None` si le
    /// paquet n'a pas la forme `{ "guilds": [...] }` (aucun serveur lu, comme avant).
    Ready(Option<Vec<Parts<'a>>>),
    /// GUILD_CREATE : le serveur.
    Guild(Parts<'a>),
    /// Autres paquets : rien à partager.
    Nothing,
}

impl<'a> Shared<'a> {
    pub(super) fn of(kind: &str, d: &'a str) -> Shared<'a> {
        /// Même forme que les anciennes structures `{ guilds: Vec<Item<…>> }`
        /// de chaque décodeur : champ facultatif, éléments pris tels quels.
        #[derive(Deserialize)]
        struct Guilds<'a> {
            #[serde(borrow, default)]
            guilds: Vec<&'a RawValue>,
        }
        match kind {
            "READY" | "READY_SUPPLEMENTAL" => Shared::Ready(serde_json::from_str::<Guilds>(d).ok().map(|g| g.guilds.into_iter().map(|raw| Parts::new(raw.get())).collect())),
            "GUILD_CREATE" => Shared::Guild(Parts::new(d)),
            _ => Shared::Nothing,
        }
    }

    /// Serveurs d'un READY (aucun si le paquet était illisible).
    pub(super) fn guilds(&self) -> &[Parts<'a>] {
        match self {
            Shared::Ready(Some(guilds)) => guilds,
            _ => &[],
        }
    }

    /// Serveur d'un GUILD_CREATE.
    pub(super) fn guild(&self) -> Option<&Parts<'a>> {
        match self {
            Shared::Guild(g) => Some(g),
            _ => None,
        }
    }
}

/// Champ décodé à part (voir [`Item`]) : `None` s'il est absent ou de forme inattendue.
pub(super) fn de_lenient<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Option<T>, D::Error> {
    Ok(Item::<T>::deserialize(d)?.0)
}

/// Identifiant en chaîne (forme documentée) ou en nombre ; `None` sinon.
pub(super) fn de_loose_id<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    loose(d, |v| match v {
        Loose::Str(s) => s.parse().ok(),
        Loose::Num(n) => Some(n),
        Loose::Other => None,
    })
}

/// Valeur dont la forme n'est pas garantie : chaîne, entier positif, ou autre
/// (gardé illisible au lieu de faire échouer tout le paquet).
enum Loose<'a> {
    Str(&'a str),
    Num(u64),
    Other,
}

/// Lit une valeur de forme libre et la donne à `f`, sans tampon ni copie (un
/// `untagged` de serde recopiait la valeur avant d'essayer chaque forme).
fn loose<'de, D: Deserializer<'de>, R>(d: D, f: impl FnOnce(Loose) -> R) -> Result<R, D::Error> {
    struct V<F>(F);

    impl<'de, R, F: FnOnce(Loose) -> R> Visitor<'de> for V<F> {
        type Value = R;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("any value")
        }

        fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<R, E> {
            Ok((self.0)(Loose::Str(s)))
        }

        fn visit_u64<E: serde::de::Error>(self, n: u64) -> Result<R, E> {
            Ok((self.0)(Loose::Num(n)))
        }

        fn visit_i64<E: serde::de::Error>(self, n: i64) -> Result<R, E> {
            Ok((self.0)(u64::try_from(n).map_or(Loose::Other, Loose::Num)))
        }

        fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<R, E> {
            Ok((self.0)(Loose::Other))
        }

        fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<R, E> {
            Ok((self.0)(Loose::Other))
        }

        fn visit_unit<E: serde::de::Error>(self) -> Result<R, E> {
            Ok((self.0)(Loose::Other))
        }

        fn visit_none<E: serde::de::Error>(self) -> Result<R, E> {
            Ok((self.0)(Loose::Other))
        }

        fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<R, D::Error> {
            d.deserialize_any(self)
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<R, A::Error> {
            while seq.next_element::<IgnoredAny>()?.is_some() {}
            Ok((self.0)(Loose::Other))
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<R, A::Error> {
            while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
            Ok((self.0)(Loose::Other))
        }
    }

    d.deserialize_any(V(f))
}

/// Permissions en chaîne décimale (forme documentée) ou en entier (anciennes
/// versions de l'API) ; `None` si illisible.
fn de_perms<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Permissions>, D::Error> {
    loose(d, |v| match v {
        Loose::Str(s) => Permissions::parse(s),
        Loose::Num(n) => Some(Permissions(n)),
        Loose::Other => None,
    })
}

/// Type de dérogation : 0/1 (forme documentée) ou "role"/"member" (anciennes versions).
fn de_overwrite_kind<'de, D: Deserializer<'de>>(d: D) -> Result<Option<OverwriteKind>, D::Error> {
    loose(d, |v| match v {
        Loose::Num(0) => Some(OverwriteKind::Role),
        Loose::Num(1) => Some(OverwriteKind::Member),
        Loose::Str("role") => Some(OverwriteKind::Role),
        Loose::Str("member") => Some(OverwriteKind::Member),
        _ => None,
    })
}

#[derive(Deserialize)]
pub(super) struct RawOverwrite {
    #[serde(deserialize_with = "de_id")]
    id: u64,
    #[serde(rename = "type", default, deserialize_with = "de_overwrite_kind")]
    kind: Option<OverwriteKind>,
    #[serde(default, deserialize_with = "de_perms")]
    allow: Option<Permissions>,
    #[serde(default, deserialize_with = "de_perms")]
    deny: Option<Permissions>,
}

impl RawOverwrite {
    pub(super) fn into_overwrite(self) -> Option<Overwrite> {
        Some(Overwrite { id: self.id, kind: self.kind?, allow: self.allow?, deny: self.deny? })
    }
}

#[derive(Deserialize)]
pub(super) struct RawRole {
    #[serde(deserialize_with = "de_id")]
    pub id: u64,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    // `color` est déprécié au profit de `colors` : sa forme peut changer sans
    // que cela rende le rôle illisible.
    #[serde(default, deserialize_with = "de_lenient")]
    color: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    position: Option<i64>,
    #[serde(default, deserialize_with = "de_perms")]
    permissions: Option<Permissions>,
    #[serde(default, deserialize_with = "de_lenient")]
    mentionable: Option<bool>,
    /// Le reste de l'objet Role, lu par `net/admin/roles.rs` (lot A3).
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) hoist: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) icon: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) unicode_emoji: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) managed: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) colors: Option<super::admin::roles::RawColors>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) tags: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) flags: Option<u64>,
}

impl RawRole {
    /// `None` si les permissions sont illisibles : le rôle est alors absent,
    /// et un membre qui le porte a des permissions inconnues.
    pub fn into_role(self) -> Option<Role> {
        let position = self.position.unwrap_or(0);
        Some(Role {
            id: self.id,
            name: self.name.unwrap_or_default(),
            color: self.color.and_then(|c| u32::try_from(c).ok()).unwrap_or(0),
            position: i32::try_from(position).unwrap_or(if position < 0 { i32::MIN } else { i32::MAX }),
            permissions: self.permissions?,
            mentionable: self.mentionable.unwrap_or(false),
        })
    }
}

/// Membre de serveur : `user.id` (GUILD_CREATE, GUILD_MEMBER_UPDATE) ou
/// `user_id` (`merged_members` du READY utilisateur).
///
/// Chaque champ est lu à part : une forme inattendue ne fait échouer que
/// ce champ, et `to_member` dit alors « inconnu ».
#[derive(Deserialize)]
pub(super) struct RawGuildMember {
    #[serde(default, deserialize_with = "de_lenient")]
    user: Option<RawUser>,
    #[serde(default, deserialize_with = "de_loose_id")]
    user_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    pub guild_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    roles: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_lenient")]
    nick: Option<String>,
    /// Gardé brut : absent ou `null` (pas d'exclusion) se distingue ainsi
    /// d'une valeur illisible (exclusion inconnue).
    #[serde(default)]
    communication_disabled_until: Option<Box<RawValue>>,
    // Pour la carte de profil (`net::people`).
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) joined_at: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) avatar: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) premium_since: Option<String>,
    // Bannière et décoration propres au serveur (lot P).
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) banner: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) avatar_decoration_data: Option<super::cosmetics::RawDecoration>,
}

impl RawGuildMember {
    pub(super) fn user(&self) -> Option<&RawUser> {
        self.user.as_ref()
    }

    pub(super) fn nick(&self) -> Option<&str> {
        self.nick.as_deref()
    }

    /// Fin de l'exclusion temporaire : `None` si absente, nulle ou illisible.
    pub(super) fn timeout_ms(&self) -> Option<i64> {
        let s: String = serde_json::from_str(self.communication_disabled_until.as_ref()?.get()).ok()?;
        Some(chrono::DateTime::parse_from_rfc3339(&s).ok()?.timestamp_millis())
    }

    pub fn user_id(&self) -> Option<u64> {
        self.user.as_ref().map(|u| u.id).or(self.user_id)
    }

    /// Mes rôles, `None` s'ils sont absents ou illisibles.
    pub fn role_ids(&self) -> Option<Vec<u64>> {
        self.roles.as_ref()?.iter().map(|r| r.parse().ok()).collect()
    }

    /// `None` si un champ nécessaire au calcul est absent ou illisible.
    pub fn to_member(&self) -> Option<Member> {
        let roles = self.role_ids()?;
        let timeout_until = match &self.communication_disabled_until {
            None => None,
            Some(raw) => {
                let s: String = serde_json::from_str(raw.get()).ok()?;
                Some(chrono::DateTime::parse_from_rfc3339(&s).ok()?.timestamp_millis())
            }
        };
        Some(Member { roles, nick: self.nick.clone(), timeout_until })
    }
}

#[derive(Deserialize)]
pub(super) struct RawUser {
    #[serde(deserialize_with = "de_id")]
    pub id: u64,
    #[serde(default)]
    username: String,
    #[serde(default)]
    global_name: Option<String>,
    /// Empreinte de l'avatar ; absente, `null` ou de forme inattendue : avatar par défaut.
    #[serde(default, deserialize_with = "de_lenient")]
    avatar: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    discriminator: Option<String>,
    // Pour la carte de profil (`net::people`) : absents de la plupart des
    // objets, ils n'effacent alors rien de ce qui est connu.
    #[serde(default, deserialize_with = "de_present_hash")]
    pub(super) banner: Option<Option<ImageHash>>,
    #[serde(default, deserialize_with = "de_present_color")]
    pub(super) accent_color: Option<Option<u32>>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) public_flags: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) bot: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) system: Option<bool>,
    // Parures (lot P, `net::cosmetics`) : absentes de beaucoup d'objets.
    #[serde(default, deserialize_with = "super::cosmetics::de_present")]
    avatar_decoration_data: Option<Option<super::cosmetics::RawDecoration>>,
    #[serde(default, deserialize_with = "super::cosmetics::de_present")]
    collectibles: Option<Option<super::cosmetics::RawCollectibles>>,
    #[serde(default, deserialize_with = "super::cosmetics::de_present")]
    primary_guild: Option<Option<super::cosmetics::RawPrimaryGuild>>,
    #[serde(default, deserialize_with = "super::cosmetics::de_present")]
    display_name_styles: Option<Option<super::cosmetics::RawNameStyle>>,
}

/// Couleur présente (même `null`) : `Some`, avec sa valeur si elle tient sur 24 bits.
fn de_present_color<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<u32>>, D::Error> {
    with_raw(d, |raw| Some(serde_json::from_str::<u32>(raw).ok().filter(|c| *c <= 0xFF_FFFF)))
}

impl RawUser {
    pub(super) fn username(&self) -> &str {
        &self.username
    }

    pub(super) fn global_name(&self) -> Option<&str> {
        self.global_name.as_deref()
    }

    /// Ancien discriminant, sauf « 0 » (nouveau système de noms).
    pub(super) fn discriminator(&self) -> Option<&str> {
        self.discriminator.as_deref().filter(|d| d.parse::<u16>().is_ok_and(|n| n != 0))
    }

    pub fn display(&self) -> String {
        match &self.global_name {
            Some(n) if !n.is_empty() => n.clone(),
            _ => self.username.clone(),
        }
    }

    pub fn avatar(&self) -> Avatar {
        Avatar::of_user(self.id, self.avatar.as_deref().and_then(ImageHash::parse), self.discriminator.as_deref())
    }

    /// Décoration, plaque, tag et style du nom (lot P).
    pub(super) fn look(&self) -> crate::state::cosmetics::Look {
        super::cosmetics::look(&self.avatar_decoration_data, &self.collectibles, &self.primary_guild, &self.display_name_styles)
    }
}

/// Empreinte d'icône lue à part : `None` si absente, `null` ou illisible.
fn icon_hash(icon: &Option<String>) -> Option<ImageHash> {
    icon.as_deref().and_then(ImageHash::parse)
}

#[derive(Deserialize)]
pub(super) struct RawChannel {
    #[serde(deserialize_with = "de_id")]
    id: u64,
    #[serde(rename = "type")]
    kind: u8,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    position: i32,
    #[serde(default, deserialize_with = "de_opt_id")]
    parent_id: Option<u64>,
    #[serde(default, deserialize_with = "de_opt_id")]
    pub guild_id: Option<u64>,
    #[serde(default)]
    pub(super) recipients: Vec<RawUser>,
    #[serde(default)]
    recipient_ids: Vec<String>,
    #[serde(default, deserialize_with = "de_opt_id")]
    last_message_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    permission_overwrites: Option<Vec<Item<RawOverwrite>>>,
    /// Icône d'un MP de groupe : `None` absente du paquet, `Some(None)` nulle ou illisible.
    #[serde(default, deserialize_with = "de_present_hash")]
    icon: Option<Option<ImageHash>>,
    /// Mode lent (secondes, 0 à 21600) ; illisible : aucun.
    #[serde(default, deserialize_with = "de_lenient")]
    rate_limit_per_user: Option<u32>,
    /// Forum ou média (lot T10, `net/threads.rs`).
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) available_tags: Option<Vec<Item<super::threads::RawTag>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) default_reaction_emoji: Option<super::threads::RawEmojiRef>,
    /// Sujet du salon, montré dans l'en-tête de la discussion (`net/threads.rs`).
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) topic: Option<String>,
    /// Salon à limite d'âge (champ `nsfw` de l'objet Channel, documenté) :
    /// écran d'entrée avant d'en montrer les messages (lot FF, F8).
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) nsfw: Option<bool>,
}

impl RawChannel {
    /// (identifiant, type) : `net/threads.rs`.
    pub(super) fn id_kind(&self) -> (u64, u8) {
        (self.id, self.kind)
    }

    /// MP à deux : (correspondant, salon).
    pub(super) fn dm_peer(&self) -> Option<(u64, u64)> {
        if self.kind != 1 {
            return None;
        }
        let mut ids = self.recipients.iter().map(|u| u.id).chain(self.recipient_ids.iter().filter_map(|s| s.parse().ok()));
        match (ids.next(), ids.next()) {
            (Some(peer), None) => Some((peer, self.id)),
            _ => None,
        }
    }

    pub fn into_channel(self) -> Option<Channel> {
        let kind = match self.kind {
            0 => ChannelKind::Text,
            5 => ChannelKind::Announcement,
            4 => ChannelKind::Category,
            15 => ChannelKind::Forum,
            16 => ChannelKind::Media,
            _ => return None, // vocal, scène, fils (`net/threads.rs`)… non gérés
        };
        // Absentes ou une seule illisible : inconnues (le salon reste visible).
        let overwrites = self.permission_overwrites.and_then(|v| v.into_iter().map(|o| o.0.and_then(RawOverwrite::into_overwrite)).collect());
        Some(Channel {
            id: self.id,
            name: self.name.unwrap_or_default(),
            kind,
            position: self.position,
            parent: self.parent_id,
            overwrites,
            slowmode: self.rate_limit_per_user.unwrap_or(0),
        })
    }

    /// `users` : nom affiché et avatar des utilisateurs du READY, pour les
    /// MP qui ne donnent que `recipient_ids`.
    pub fn into_dm(self, users: &HashMap<u64, (String, Avatar)>) -> Option<Dm> {
        if self.kind != 1 && self.kind != 3 {
            return None;
        }
        let mut people: Vec<(String, Avatar)> = self.recipients.iter().map(|u| (u.display(), u.avatar())).collect();
        let mut recipients: Vec<(u64, String)> = self.recipients.iter().map(|u| (u.id, u.display())).collect();
        for id in self.recipient_ids.iter().filter_map(|id| id.parse::<u64>().ok()) {
            if let Some((name, avatar)) = users.get(&id) {
                people.push((name.clone(), *avatar));
                recipients.push((id, name.clone()));
            }
        }
        let avatar = match (self.kind, people.first()) {
            // MP à deux : l'avatar du correspondant.
            (1, Some((_, a))) => *a,
            _ => Avatar {
                image: self.icon.flatten().map(|hash| ImageRef::GroupIcon { channel_id: self.id, hash }),
                ..Avatar::default_for(self.id)
            },
        };
        let name = match self.name.filter(|n| !n.is_empty()) {
            Some(n) => n,
            // Sans nom ni destinataire connu : vide, l'interface affiche un nom traduit.
            None if people.is_empty() => String::new(),
            None => people.into_iter().map(|(n, _)| n).collect::<Vec<_>>().join(", "),
        };
        Some(Dm { id: self.id, name, last_message: self.last_message_id.unwrap_or(self.id), avatar, recipients })
    }

    /// Destinataires d'un MP de groupe (liste des membres), si le paquet les
    /// donne ; `users` : ceux du READY, pour `recipient_ids`.
    pub fn group_recipients(&self, users: &HashMap<u64, (String, Avatar)>) -> Option<(u64, Vec<crate::state::model::User>)> {
        use crate::state::model::User;
        if self.kind != 3 || (self.recipients.is_empty() && self.recipient_ids.is_empty()) {
            return None;
        }
        let mut people: Vec<User> = self.recipients.iter().map(|u| User { id: u.id, name: u.display(), avatar: u.avatar() }).collect();
        people.extend(
            self.recipient_ids
                .iter()
                .filter_map(|id| id.parse::<u64>().ok())
                .filter_map(|id| users.get(&id).map(|(name, avatar)| User { id, name: name.clone(), avatar: *avatar })),
        );
        Some((self.id, people))
    }

    /// MP d'un CHANNEL_CREATE / CHANNEL_UPDATE, avec ce que le paquet apporte
    /// vraiment : sans destinataires, ni le nom calculé ni l'avatar du
    /// correspondant ne sont connus ; l'icône d'un groupe ne l'est que si le
    /// champ est présent (même nul).
    pub fn into_dm_update(self) -> Option<(Dm, DmKnown)> {
        let people = !self.recipients.is_empty();
        let named = self.name.as_deref().is_some_and(|n| !n.is_empty());
        let known = DmKnown { name: named || people, avatar: if self.kind == 3 { self.icon.is_some() } else { people } };
        self.into_dm(&HashMap::new()).map(|dm| (dm, known))
    }
}

/// « Guild Identity Structure » (docs.discord.food, resources/guild).
#[derive(Deserialize)]
struct RawGuildIdentity {
    #[serde(default, deserialize_with = "de_lenient")]
    tag: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    badge: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct RawGuildProps {
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_loose_id")]
    owner_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    icon: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) premium_tier: Option<u8>,
    #[serde(default, deserialize_with = "de_lenient")]
    premium_subscription_count: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) features: Option<Vec<String>>,
    /// Tag du serveur (lot ND).
    #[serde(default, deserialize_with = "de_lenient")]
    profile: Option<RawGuildIdentity>,
    /// Bannière du serveur (lot V).
    #[serde(default, deserialize_with = "de_lenient")]
    banner: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct RawGuild {
    #[serde(deserialize_with = "de_id")]
    pub id: u64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    icon: Option<String>,
    /// Tag du serveur : à la racine ou dans `properties` (lot ND).
    #[serde(default, deserialize_with = "de_lenient")]
    profile: Option<RawGuildIdentity>,
    /// Bannière du serveur : à la racine (bots) ou dans `properties` (lot V).
    #[serde(default, deserialize_with = "de_lenient")]
    banner: Option<String>,
    #[serde(default, deserialize_with = "de_loose_id")]
    owner_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) properties: Option<RawGuildProps>,
    /// Niveau de boost : à la racine (bots) ou dans `properties` (comptes utilisateur).
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) premium_tier: Option<u8>,
    #[serde(default, deserialize_with = "de_lenient")]
    premium_subscription_count: Option<u32>,
    #[serde(default)]
    pub(super) channels: Vec<RawChannel>,
    /// Fils actifs (GUILD_CREATE), retirés par `net/threads.rs` avant `into_guild`.
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) threads: Option<Vec<Item<super::threads::RawThread>>>,
    /// `None` : absents ou illisibles (permissions inconnues).
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) roles: Option<Vec<Item<RawRole>>>,
    /// Fonctions du serveur (icônes de rôle, lot A3) ; niveau de boost : `premium_tier` plus haut.
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) features: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) members: Option<Vec<Item<RawGuildMember>>>,
    #[serde(default)]
    pub unavailable: bool,
    /// Présences des membres (GUILD_CREATE).
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) presences: Option<Vec<Item<super::people::RawPresence>>>,
    /// Emojis du serveur (autocomplétion, `net/social.rs` pour le sélecteur de réactions).
    #[serde(default, deserialize_with = "de_lenient")]
    emojis: Option<Vec<Item<RawEmoji>>>,
    /// Autocollants du serveur (`net/stickers.rs`), retirés avant `into_guild`.
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) stickers: Option<Vec<Item<super::stickers::RawSticker>>>,
}

impl RawGuild {
    /// Niveau de boost et fonctions, dans le paquet ou ses `properties`
    /// (READY des comptes utilisateur).
    pub(super) fn premium(&self) -> (Option<u8>, &[String]) {
        let p = self.properties.as_ref();
        let tier = self.premium_tier.or_else(|| p.and_then(|p| p.premium_tier));
        let features = self.features.as_deref().or_else(|| p.and_then(|p| p.features.as_deref())).unwrap_or(&[]);
        (tier, features)
    }
}

/// Emoji d'un serveur (objet Emoji de la documentation). Un emoji illisible
/// est ignoré, sans gêner les autres.
#[derive(Deserialize)]
pub(super) struct RawEmoji {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    animated: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    available: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    roles: Option<Vec<String>>,
}

/// Emojis lisibles d'une liste (ceux sans identifiant ni nom sont écartés).
pub(super) fn into_emojis(list: Vec<Item<RawEmoji>>) -> Vec<Emoji> {
    list.iter().filter_map(|e| e.0.as_ref()?.to_emoji()).collect()
}

impl RawEmoji {
    fn to_emoji(&self) -> Option<Emoji> {
        Some(Emoji {
            id: self.id?,
            name: self.name.clone().filter(|n| !n.is_empty())?,
            animated: self.animated.unwrap_or(false),
            available: self.available.unwrap_or(true),
            roles: self.roles.iter().flatten().filter_map(|r| r.parse().ok()).collect(),
        })
    }
}

/// GUILD_EMOJIS_UPDATE
#[derive(Deserialize)]
pub(super) struct RawEmojisUpdate {
    #[serde(deserialize_with = "de_id")]
    pub guild_id: u64,
    #[serde(default)]
    pub emojis: Vec<Item<RawEmoji>>,
}

/// TYPING_START : `member` n'est présent que dans un serveur.
#[derive(Deserialize)]
pub(super) struct RawTyping {
    #[serde(deserialize_with = "de_id")]
    pub channel_id: u64,
    #[serde(deserialize_with = "de_id")]
    pub user_id: u64,
    #[serde(default, deserialize_with = "de_lenient")]
    member: Option<RawTypingMember>,
}

#[derive(Deserialize)]
struct RawTypingMember {
    #[serde(default, deserialize_with = "de_lenient")]
    nick: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    user: Option<RawUser>,
}

impl RawTyping {
    /// Nom affiché (surnom, sinon nom global, sinon nom d'utilisateur) ; `None` hors serveur.
    pub fn name(&self) -> Option<String> {
        let m = self.member.as_ref()?;
        match &m.nick {
            Some(n) if !n.is_empty() => Some(n.clone()),
            _ => m.user.as_ref().map(RawUser::display).filter(|n| !n.is_empty()),
        }
    }
}

/// Ma part de `merged_members` pour un serveur, une fois l'alignement contrôlé.
pub(super) struct MergedPart {
    pub members: Vec<RawGuildMember>,
    /// L'alignement de ce READY a pu être confirmé (par un `guild_id` ou par
    /// des rôles présents dans le bon serveur) : un membre sans aucun rôle
    /// peut alors être cru. Sinon sa position seule ne prouve rien.
    pub roleless_ok: bool,
}

impl RawGuild {
    /// Identifiants des rôles, si la liste est entièrement lisible.
    fn role_ids(&self) -> Option<HashSet<u64>> {
        self.roles.as_ref()?.iter().map(|r| r.0.as_ref().map(|r| r.id)).collect()
    }

    /// `me` : mon identifiant, pour trouver mon membre dans `members` ou
    /// dans `merged` (ma part de `merged_members` du READY utilisateur).
    pub fn into_guild(self, me: Option<u64>, merged: Option<MergedPart>) -> Option<Guild> {
        if self.unavailable {
            return None;
        }
        let (prop_tier, prop_boosts) = self.properties.as_ref().map(|p| (p.premium_tier, p.premium_subscription_count)).unwrap_or_default();
        let premium_tier = self.premium_tier.or(prop_tier).unwrap_or(0);
        let boosts = self.premium_subscription_count.or(prop_boosts).unwrap_or(0);
        let prop_banner = self.properties.as_ref().and_then(|p| p.banner.clone());
        let banner = icon_hash(&self.banner).or_else(|| icon_hash(&prop_banner)).map(|hash| ImageRef::Banner { user_id: self.id, hash });
        let tag = self.profile.as_ref().or_else(|| self.properties.as_ref().and_then(|p| p.profile.as_ref())).and_then(|p| crate::state::cosmetics::GuildTag::new(self.id, p.tag.as_deref()?, icon_hash(&p.badge)));
        let (prop_name, prop_owner, prop_icon) = self.properties.map(|p| (p.name, p.owner_id, p.icon)).unwrap_or_default();
        let name = self.name.or(prop_name).unwrap_or_default();
        let guild_id = self.id;
        let icon = icon_hash(&self.icon).or_else(|| icon_hash(&prop_icon)).map(|hash| ImageRef::GuildIcon { guild_id, hash });
        let mut channels: Vec<Channel> = self.channels.into_iter().filter_map(RawChannel::into_channel).collect();
        sort_channels(&mut channels);
        let own = |m: &&RawGuildMember| me.is_some() && m.user_id() == me;
        let member = match self.members.iter().flatten().filter_map(|m| m.0.as_ref()).find(own) {
            Some(m) => m.to_member(),
            None => merged.as_ref().and_then(|p| {
                let m = p.members.iter().find(own)?.to_member()?;
                (p.roleless_ok || !m.roles.is_empty()).then_some(m)
            }),
        };
        let (roles, roles_unreadable) = match self.roles {
            Some(items) => {
                let bad = items.iter().any(|r| r.0.is_none());
                (items.into_iter().filter_map(|r| r.0.and_then(RawRole::into_role)).collect(), bad)
            }
            None => (Vec::new(), true),
        };
        let emojis = self.emojis.map(into_emojis).unwrap_or_default();
        let mut guild = Guild { id: self.id, name, channels, owner_id: self.owner_id.or(prop_owner), roles, me: member, roles_unreadable, icon, emojis, premium_tier, boosts, banner, tag };
        // Gardé toute la session : sans la réserve des tableaux bruts (lot « ram »).
        guild.shrink_to_fit();
        Some(guild)
    }
}

/// Répartit `merged_members` entre les serveurs du READY, ou l'ignore en
/// entier si l'alignement est douteux : nombre d'entrées différent, un
/// `guild_id` qui ne correspond pas, ou des rôles absents du serveur de même
/// rang (on ne devine jamais quel membre va avec quel serveur).
pub(super) fn align_merged(guilds: &[RawGuild], merged: Option<Vec<Vec<Item<RawGuildMember>>>>) -> Vec<Option<MergedPart>> {
    let none = || guilds.iter().map(|_| None).collect();
    let Some(merged) = merged.filter(|m| m.len() == guilds.len()) else { return none() };
    let mut confirmed = false;
    for (g, entry) in guilds.iter().zip(&merged) {
        let roles = if g.unavailable { None } else { g.role_ids() };
        for m in entry.iter().filter_map(|m| m.0.as_ref()) {
            match m.guild_id {
                Some(id) if id != g.id => return none(),
                Some(_) => confirmed = true,
                None => {}
            }
            let mine = m.role_ids().unwrap_or_default();
            if let Some(known) = &roles
                && !mine.is_empty()
            {
                if !mine.iter().all(|r| known.contains(r)) {
                    return none();
                }
                confirmed = true;
            }
        }
    }
    merged
        .into_iter()
        .map(|entry| Some(MergedPart { members: entry.into_iter().filter_map(|m| m.0).collect(), roleless_ok: confirmed }))
        .collect()
}

/// GUILD_UPDATE : seuls le propriétaire et l'icône sont lus.
#[derive(Deserialize)]
pub(super) struct RawGuildUpdate {
    #[serde(deserialize_with = "de_id")]
    pub id: u64,
    /// Gardé brut pour distinguer « absent » (`None`) de « illisible ».
    #[serde(default)]
    owner_id: Option<Box<RawValue>>,
    #[serde(default, deserialize_with = "de_lenient")]
    properties: Option<RawGuildProps>,
    /// `None` : absente ; `Some(None)` : `null` (icône retirée) ou illisible.
    #[serde(default, deserialize_with = "de_present_hash")]
    icon: Option<Option<ImageHash>>,
}

/// Champ présent (même `null`) : `Some`, avec l'empreinte si elle est lisible.
pub(super) fn de_present_hash<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<ImageHash>>, D::Error> {
    with_raw(d, |raw| Some(serde_json::from_str::<String>(raw).ok().as_deref().and_then(ImageHash::parse)))
}

impl RawGuildUpdate {
    /// `None` : inchangée ; `Some(None)` : retirée ou illisible (initiales).
    pub fn icon(&self) -> Option<Option<ImageRef>> {
        let guild_id = self.id;
        let to_ref = |h: Option<ImageHash>| h.map(|hash| ImageRef::GuildIcon { guild_id, hash });
        match self.icon {
            Some(h) => Some(to_ref(h)),
            None => self.properties.as_ref().filter(|p| p.icon.is_some()).map(|p| to_ref(icon_hash(&p.icon))),
        }
    }

    /// `None` : absent ; `Some(None)` : présent mais illisible.
    pub fn owner_id(&self) -> Option<Option<u64>> {
        match &self.owner_id {
            Some(raw) => Some(serde_json::from_str::<String>(raw.get()).ok().and_then(|s| s.parse().ok())),
            None => self.properties.as_ref().and_then(|p| p.owner_id).map(Some),
        }
    }
}

/// GUILD_ROLE_CREATE / GUILD_ROLE_UPDATE
#[derive(Deserialize)]
pub(super) struct RawRoleEvent {
    #[serde(deserialize_with = "de_id")]
    pub guild_id: u64,
    pub role: Item<RawRole>,
}

/// GUILD_ROLE_DELETE
#[derive(Deserialize)]
pub(super) struct RawRoleDelete {
    #[serde(deserialize_with = "de_id")]
    pub guild_id: u64,
    #[serde(default, deserialize_with = "de_loose_id")]
    pub role_id: Option<u64>,
}

#[derive(Deserialize, Default)]
pub(super) struct RawSettings {
    #[serde(default)]
    pub guild_positions: Vec<String>,
}

#[derive(Deserialize)]
pub(super) struct RawReady {
    pub user: RawUser,
    pub session_id: String,
    pub resume_gateway_url: String,
    #[serde(default)]
    pub guilds: Vec<RawGuild>,
    #[serde(default)]
    pub private_channels: Vec<RawChannel>,
    #[serde(default)]
    pub users: Vec<RawUser>,
    #[serde(default)]
    pub user_settings: Option<RawSettings>,
    /// Format « capacités modernes » des comptes utilisateur : un tableau
    /// de membres par serveur, dans l'ordre de `guilds`.
    #[serde(default, deserialize_with = "de_lenient")]
    pub merged_members: Option<Vec<Vec<Item<RawGuildMember>>>>,
    /// Présences (documenté pour READY : absent des bots ; amis d'un compte utilisateur).
    #[serde(default, deserialize_with = "de_lenient")]
    pub presences: Option<Vec<Item<super::people::RawPresence>>>,
    /// Réglages du compte (`net/account.rs`), comptes utilisateur seulement.
    #[serde(default, deserialize_with = "de_lenient")]
    pub user_settings_proto: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub user_guild_settings: Option<super::account::RawGuildSettingsList>,
}

use super::media::{de_items, RawAttachment, RawEmbed};

#[derive(Deserialize)]
pub(super) struct RawMember {
    #[serde(default)]
    pub(super) nick: Option<String>,
    // Pour la carte de profil (`net::people`).
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) roles: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) joined_at: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) avatar: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) premium_since: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) communication_disabled_until: Option<String>,
    // Bannière et décoration propres au serveur (lot P).
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) banner: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    pub(super) avatar_decoration_data: Option<super::cosmetics::RawDecoration>,
}

#[derive(Deserialize)]
pub(super) struct RawMessage {
    #[serde(deserialize_with = "de_id")]
    id: u64,
    #[serde(deserialize_with = "de_id")]
    channel_id: u64,
    #[serde(default, deserialize_with = "de_opt_id")]
    pub(super) guild_id: Option<u64>,
    pub(super) author: RawUser,
    #[serde(default)]
    pub(super) member: Option<RawMember>,
    #[serde(default)]
    content: String,
    #[serde(default, deserialize_with = "de_items")]
    attachments: Vec<RawAttachment>,
    #[serde(default, deserialize_with = "de_items")]
    embeds: Vec<RawEmbed>,
    #[serde(default)]
    mentions: Vec<RawUser>,
    #[serde(default)]
    mention_everyone: bool,
    /// Rôles mentionnés (notifications) ; une forme inattendue vaut « aucun ».
    #[serde(default, deserialize_with = "de_lenient")]
    mention_roles: Option<Vec<String>>,
    #[serde(default)]
    edited_timestamp: Option<String>,
    #[serde(rename = "type", default)]
    kind: u8,
    /// Nonce de l'envoi (texte ou nombre selon la documentation).
    #[serde(default)]
    nonce: Option<serde_json::Value>,
    /// Réactions, réponse, transfert, épingle (lots T5/T6, `net/social.rs`),
    /// chacun lu à part : une forme inattendue ne rend pas le message illisible.
    #[serde(default, deserialize_with = "de_lenient")]
    reactions: Option<Vec<Item<super::social::RawReaction>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    message_reference: Option<super::social::RawReference>,
    #[serde(default, deserialize_with = "super::social::present")]
    referenced_message: Option<Box<RawValue>>,
    #[serde(default, deserialize_with = "de_lenient")]
    message_snapshots: Option<Vec<super::social::RawSnapshot>>,
    #[serde(default, deserialize_with = "de_lenient")]
    pinned: Option<bool>,
    /// Fil né de ce message (lot T10).
    #[serde(default, deserialize_with = "de_lenient")]
    thread: Option<super::threads::RawThread>,
    /// Autocollants (lot T4, `net/stickers.rs`).
    #[serde(default, deserialize_with = "de_lenient")]
    sticker_items: Option<Vec<Item<super::stickers::RawSticker>>>,
    /// Composants et interaction d'origine (lot T11, `net/interactions.rs`) ;
    /// `interaction*` sont aussi lus par le lot T13 (`net/announce.rs`).
    #[serde(default, deserialize_with = "de_lenient")]
    flags: Option<u64>,
    /// Message `/tts` (lot FF2) : lu à voix haute si le réglage du compte le demande.
    #[serde(default, deserialize_with = "de_lenient")]
    tts: Option<bool>,
    #[serde(default)]
    components: Option<serde_json::Value>,
    #[serde(default)]
    interaction_metadata: Option<serde_json::Value>,
    #[serde(default)]
    interaction: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "de_loose_id")]
    application_id: Option<u64>,
    /// Sondage, appel (lot T13, `net/polls.rs`, `net/announce.rs`).
    #[serde(default, deserialize_with = "de_lenient")]
    poll: Option<super::polls::RawPoll>,
    #[serde(default, deserialize_with = "de_lenient")]
    call: Option<super::announce::RawCall>,
}

impl RawMessage {
    /// Fil né de ce message, à faire connaître du magasin (lot T10).
    pub(super) fn thread_event(&self) -> Option<crate::state::Event> {
        super::threads::message_thread(self.thread.as_ref()?, self.guild_id)
    }

    /// Nonce de ce message s'il est de moi et lisible : confirme un envoi.
    pub fn own_nonce(&self, me: Option<u64>) -> Option<(u64, u64)> {
        if me != Some(self.author.id) {
            return None;
        }
        let n = match self.nonce.as_ref()? {
            serde_json::Value::String(s) => s.parse().ok()?,
            serde_json::Value::Number(n) => n.as_u64()?,
            _ => return None,
        };
        Some((self.channel_id, n))
    }
}

impl RawMessage {
    pub fn into_message(self) -> Message {
        let author = match self.member.and_then(|m| m.nick) {
            Some(n) if !n.is_empty() => n,
            _ => self.author.display(),
        };
        let reference = self.message_reference.as_ref().map(super::social::RawReference::ids);
        let mut meta = super::social::meta(super::social::RawMeta {
            reactions: self.reactions,
            reference: self.message_reference,
            referenced: self.referenced_message,
            snapshots: self.message_snapshots,
            pinned: self.pinned,
        });
        meta.mention_roles = self.mention_roles.iter().flatten().filter_map(|r| r.parse().ok()).collect();
        meta.flags = self.flags.unwrap_or(0);
        meta.tts = self.tts.unwrap_or(false);
        meta.thread = self.thread.as_ref().and_then(super::threads::RawThread::stub).map(Box::new);
        meta.stickers = super::stickers::items(self.sticker_items);
        let mut attachments = super::media::attachments(self.attachments);
        meta.app = super::interactions::app_meta(self.flags, self.components.as_ref(), self.interaction_metadata.as_ref(), self.interaction.as_ref(), self.application_id, &mut attachments);
        let content = self.content;
        // Message système sans texte : son texte, qui dépend de la langue, est
        // mis par le magasin.
        // Types documentés (lot T13, `state::system_msgs`) ; une réponse
        // d'application avec composants a un contenu (lot T11).
        let has_content = !(content.is_empty() && attachments.is_empty() && self.embeds.is_empty() && meta.app.is_none());
        let system = crate::state::system_msgs::kind_of(self.kind, has_content);
        // Lecture typée, indulgente : une forme inattendue est ignorée.
        let typed = |v: Option<&serde_json::Value>| v.and_then(|v| serde_json::from_value::<super::announce::RawInteraction>(v.clone()).ok());
        let parts = super::announce::Parts {
            code: self.kind,
            flags: self.flags,
            reference,
            poll: self.poll,
            metadata: typed(self.interaction_metadata.as_ref()),
            interaction: typed(self.interaction.as_ref()),
            call: self.call,
        };
        let mut m = Message {
            id: self.id,
            channel_id: self.channel_id,
            guild_id: self.guild_id,
            author_id: self.author.id,
            author_avatar: self.author.avatar(),
            author,
            content,
            attachments,
            embeds: super::media::embeds(self.embeds),
            mentions: self.mentions.iter().map(|u| (u.id, u.display())).collect(),
            mention_everyone: self.mention_everyone,
            edited: self.edited_timestamp.is_some(),
            system,
            meta,
        };
        super::announce::finish(&mut m, parts);
        m
    }
}

#[derive(Deserialize)]
pub(super) struct RawMessageUpdate {
    #[serde(deserialize_with = "de_id")]
    pub id: u64,
    #[serde(deserialize_with = "de_id")]
    pub channel_id: u64,
    #[serde(default)]
    pub content: Option<String>,
    /// Absents : inchangés.
    #[serde(default, deserialize_with = "super::media::de_opt_items")]
    pub attachments: Option<Vec<RawAttachment>>,
    #[serde(default, deserialize_with = "super::media::de_opt_items")]
    pub embeds: Option<Vec<RawEmbed>>,
}

/// Ce qu'un CHANNEL_UPDATE de serveur dit de la visibilité du salon : son
/// type, sa catégorie et ses permissions (lot « perf »). Tant que leur
/// empreinte ne change pas, le paquet ne modifie ni pastille ni compteur hors
/// de son serveur (`Interest::calm_channel_update`).
#[derive(Deserialize)]
pub(super) struct RawChannelMark {
    #[serde(deserialize_with = "de_id")]
    pub id: u64,
    #[serde(default, deserialize_with = "de_opt_id")]
    pub guild_id: Option<u64>,
    #[serde(default, rename = "type")]
    kind: serde_json::Value,
    #[serde(default)]
    parent_id: serde_json::Value,
    #[serde(default)]
    permission_overwrites: serde_json::Value,
}

impl RawChannelMark {
    /// Empreinte du type, de la catégorie et des permissions, tels que reçus.
    pub fn mark(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (self.kind.to_string(), self.parent_id.to_string(), self.permission_overwrites.to_string()).hash(&mut h);
        h.finish()
    }
}

#[derive(Deserialize)]
pub(super) struct RawIdChannel {
    #[serde(deserialize_with = "de_id")]
    pub id: u64,
    #[serde(default, deserialize_with = "de_opt_id")]
    pub channel_id: Option<u64>,
    #[serde(default, deserialize_with = "de_opt_id")]
    pub guild_id: Option<u64>,
    #[serde(default)]
    pub unavailable: bool,
}

/// Enveloppe d'un paquet de la passerelle : `d` reste brut tant qu'on ne
/// sait pas si l'événement nous intéresse.
#[derive(Deserialize)]
pub(super) struct Payload<'a> {
    pub op: u8,
    #[serde(borrow, default)]
    pub d: Option<&'a RawValue>,
    #[serde(default)]
    pub s: Option<u64>,
    #[serde(default)]
    pub t: Option<&'a str>,
}

#[derive(Deserialize)]
pub(super) struct Hello {
    pub heartbeat_interval: u64,
}

/// Paquets READY, READY_SUPPLEMENTAL et GUILD_CREATE des tests du découpage
/// partagé ([`Shared`]) : tous les jeux d'essai, chaque serveur d'un READY
/// rejoué en GUILD_CREATE, et des formes hostiles (éléments qui ne sont pas des
/// objets, champs en double, noms échappés, texte tronqué, espaces).
#[cfg(test)]
pub(super) fn guild_texts() -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = Vec::new();
    let dir = format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"));
    let mut names: Vec<String> = std::fs::read_dir(&dir).into_iter().flatten().flatten().filter_map(|e| e.file_name().into_string().ok()).collect();
    names.sort();
    for name in names {
        let kind = if name.starts_with("ready_supplemental") {
            "READY_SUPPLEMENTAL"
        } else if name.starts_with("ready_") {
            "READY"
        } else if name.starts_with("guild_create_") {
            "GUILD_CREATE"
        } else {
            continue;
        };
        let text = std::fs::read_to_string(format!("{dir}/{name}")).unwrap_or_else(|e| panic!("{name} : {e}"));
        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name} : {e}"));
        let d = &v["d"];
        for g in d["guilds"].as_array().into_iter().flatten() {
            out.push(("GUILD_CREATE", g.to_string()));
        }
        // Avec des retours à la ligne et des espaces, comme sans.
        out.push((kind, serde_json::to_string_pretty(d).unwrap_or_default()));
        out.push((kind, d.to_string()));
    }
    assert!(out.len() > 20, "jeux d'essai introuvables dans {dir}");
    let guilds = [
        r#"{ "id": "1", "properties": { "features": ["COMMUNITY"] }, "guild_scheduled_events": [] }"#,
        "7",
        r#"{ "id": "2", "unavailable": true }"#,
        "[1, 2]",
        r#"["3", "n"]"#,
        "null",
        r#""x""#,
        "{}",
        r#"{ "id": 3 }"#,
        r#"{ "id": "4", "id": "5" }"#,
        r#"{ "id": "4", "channels": [], "channels": [] }"#,
        r#"{ "id": "4", "inconnu": 1, "inconnu": 2 }"#,
        r#"{ "id": "7", "channels": [{ "id": "70", "type": 2, "name": "vocal" }] }"#,
        r#"{ "channels": [{ "id": "9", "type": 2, "name": "v", "bitrate": 64000 }, 5, { "id": 9 }, { "id": "10", "type": 13, "name": "scène" }], "id": "6", "voice_states": [{ "user_id": "1", "channel_id": "9", "session_id": "s" }, 7], "stage_instances": [{ "id": "11", "channel_id": "10", "topic": "sujet" }, null] }"#,
        r#"{ "id": "8", "channels": 5, "voice_states": {}, "stage_instances": "non", "properties": [], "roles": 3, "emojis": null }"#,
        r#"  { "id" : "12" , "name" : "espaces" }  "#,
    ];
    for g in guilds {
        out.push(("GUILD_CREATE", g.to_string()));
    }
    let all = guilds.join(", ");
    for kind in ["READY", "READY_SUPPLEMENTAL"] {
        out.push((kind, format!(r#"{{ "guilds": [{all}], "user": {{}} }}"#)));
        out.push((kind, format!(r#"{{ "user": {{}}, "guilds": [{all}] }}"#)));
        for odd in [r#"{ "guilds": { "a": 1 } }"#, r#"{ "guilds": null }"#, "{}", "[]", r#"[[{ "id": "1" }]]"#, r#"{ "guilds": [{ "id": "1" }], "guilds": [] }"#, r#"{ "guilds": [{ "id": "1" }"#, r#"  { "guilds" : [ { "id" : "1" } ] }  "#, "", "7"] {
            out.push((kind, odd.to_string()));
        }
    }
    for cut in [r#"{ "id": "1""#, r#"{ "id": "1", "channels": ["#, "{", r#"{ "id": "1" } trop"#, ""] {
        out.push(("GUILD_CREATE", cut.to_string()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SystemKind;

    /// Le serveur lu par champs ([`Parts`]) est celui qu'on lisait d'un bloc,
    /// sur tous les jeux d'essai et toutes les formes hostiles.
    #[test]
    fn parts_read_like_the_whole_text() {
        let mut objects = 0;
        for (kind, d) in guild_texts() {
            if kind != "GUILD_CREATE" {
                continue;
            }
            let parts = Parts::new(&d);
            objects += usize::from(matches!(parts, Parts::Object(_)));
            let guild = |g: Result<RawGuild, serde_json::Error>| g.ok().map(|g| (g.id, g.unavailable, g.channels.len(), g.roles.as_ref().map(Vec::len), format!("{:?}", g.into_guild(Some(1), None))));
            assert_eq!(guild(parts.read::<RawGuild>()), guild(serde_json::from_str::<RawGuild>(&d)), "{d}");
            // Une structure de sonde : stricte sur `id`, tolérante ailleurs.
            let probe = |p: Result<Inner, serde_json::Error>| p.ok().map(|p| (p.id, p.name));
            assert_eq!(probe(parts.read::<Inner>()), probe(serde_json::from_str::<Inner>(&d)), "{d}");
            // Et lu sans structure : tous les champs, dans l'ordre.
            let tree = |v: Result<serde_json::Value, serde_json::Error>| v.ok();
            if !d.contains(r#""id": "4", "#) {
                assert_eq!(tree(parts.read::<serde_json::Value>()), tree(serde_json::from_str::<serde_json::Value>(&d)), "{d}");
            }
        }
        assert!(objects > 20, "{objects} serveurs découpés");
        // Seuls les champs déclarés sont relus : un champ ignoré illisible à la
        // relecture ne compte pas (il a déjà été validé au découpage).
        let parts = Parts::new(r#"{ "name": "n", "id": "5", "gros": [1, 2, 3] }"#);
        let Parts::Object(fields) = &parts else { panic!("objet attendu") };
        assert_eq!(fields.iter().map(|(k, v)| (k.as_ref(), v.get())).collect::<Vec<_>>(), [("name", r#""n""#), ("id", r#""5""#), ("gros", "[1, 2, 3]")]);
        assert!(fields.iter().all(|(k, _)| matches!(k, Cow::Borrowed(_))), "noms empruntés au texte");
    }

    fn message(json: &str) -> Message {
        serde_json::from_str::<RawMessage>(json).map(RawMessage::into_message).unwrap_or_else(|e| panic!("JSON de test invalide : {e}"))
    }

    fn dm(json: &str, users: &HashMap<u64, String>) -> Option<Dm> {
        let users: HashMap<u64, (String, Avatar)> = users.iter().map(|(id, n)| (*id, (n.clone(), Avatar::default_for(*id)))).collect();
        serde_json::from_str::<RawChannel>(json).map(|c| c.into_dm(&users)).unwrap_or_else(|e| panic!("JSON de test invalide : {e}"))
    }

    #[test]
    fn message_full() {
        let m = message(
            r#"{
                "id": "1100", "channel_id": "200", "guild_id": "300", "type": 0,
                "author": { "id": "42", "username": "alice", "global_name": "Alice A." },
                "member": { "nick": "Ali" },
                "content": "salut <@43>",
                "attachments": [ { "filename": "a.png", "url": "https://cdn.example/a.png", "size": 3 } ],
                "embeds": [ { "title": "T", "description": "D" }, { "title": "Seul" }, { "description": "Desc" }, { "url": "x" } ],
                "mentions": [ { "id": "43", "username": "bob", "global_name": null } ],
                "mention_everyone": true,
                "edited_timestamp": "2024-01-01T00:00:00+00:00",
                "unknown_field": { "ignored": true }
            }"#,
        );
        assert_eq!((m.id, m.channel_id, m.guild_id, m.author_id), (1100, 200, Some(300), 42));
        assert_eq!(m.author, "Ali");
        assert_eq!(m.content, "salut <@43>");
        assert_eq!(m.attachments.len(), 1);
        assert_eq!(m.attachments[0].name, "a.png");
        assert_eq!(m.attachments[0].url, "https://cdn.example/a.png");
        let texts: Vec<(Option<&str>, Option<&str>)> = m.embeds.iter().map(|e| (e.title.as_deref(), e.description.as_deref())).collect();
        assert_eq!(texts, vec![(Some("T"), Some("D")), (Some("Seul"), None), (None, Some("Desc"))], "l'embed sans rien à montrer est ignoré");
        assert_eq!(m.mentions, vec![(43, "bob".to_string())]);
        assert!(m.mention_everyone);
        assert!(m.edited);
        assert!(m.meta.mention_roles.is_empty(), "absent : aucun rôle");
        let roles = |v: &str| message(&format!(r#"{{ "id": "1", "channel_id": "2", "author": {{ "id": "3", "username": "u" }}, "mention_roles": {v} }}"#)).meta.mention_roles;
        assert_eq!(roles(r#"["5", "x", "6"]"#), vec![5, 6]);
        assert!(roles("12").is_empty(), "forme inattendue : ignorée, message lisible");
    }

    #[test]
    fn message_author_fallbacks() {
        let m = message(r#"{ "id": "1", "channel_id": "2", "author": { "id": "3", "username": "u", "global_name": "G" }, "member": { "nick": "" } }"#);
        assert_eq!(m.author, "G");
        assert_eq!(m.guild_id, None);
        assert!(!m.edited);
        let m = message(r#"{ "id": "1", "channel_id": "2", "author": { "id": "3", "username": "u", "global_name": "" }, "content": "x" }"#);
        assert_eq!(m.author, "u");
    }

    #[test]
    fn message_system_kinds() {
        let sys = |kind: u8| message(&format!(r#"{{ "id": "1", "channel_id": "2", "type": {kind}, "author": {{ "id": "3", "username": "u" }} }}"#));
        let kind = |k: u8| sys(k).system;
        assert_eq!(kind(7), Some(SystemKind::Joined));
        assert_eq!(kind(8), Some(SystemKind::Boosted));
        assert_eq!(kind(6), Some(SystemKind::Pinned));
        assert_eq!(kind(0), None);
        assert_eq!(kind(19), None);
        assert_eq!(kind(46), Some(SystemKind::PollResult));
        assert_eq!(kind(99), Some(SystemKind::Other));
        // Le texte, qui dépend de la langue, est mis par le magasin.
        assert_eq!(sys(7).content, "");
        // Un message système qui a du texte le garde.
        assert_eq!(message(r#"{ "id": "1", "channel_id": "2", "type": 7, "content": "x", "author": { "id": "3" } }"#).system, None);
    }

    #[test]
    fn message_rejects_bad_id() {
        assert!(serde_json::from_str::<RawMessage>(r#"{ "id": "abc", "channel_id": "2", "author": { "id": "3" } }"#).is_err());
        assert!(serde_json::from_str::<RawMessage>(r#"{ "id": 1, "channel_id": "2", "author": { "id": "3" } }"#).is_err());
    }

    #[test]
    fn dm_from_recipients() {
        let d = dm(
            r#"{ "id": "10", "type": 1, "last_message_id": "99",
                 "recipients": [ { "id": "5", "username": "bob", "global_name": "Bob" } ] }"#,
            &HashMap::new(),
        );
        let d = d.unwrap_or_else(|| panic!("MP attendu"));
        assert_eq!((d.id, d.name.as_str(), d.last_message), (10, "Bob", 99));
    }

    #[test]
    fn dm_group_from_recipient_ids_and_name() {
        let users: HashMap<u64, String> = [(5, "Bob".to_string()), (6, "Eve".to_string())].into();
        let d = dm(r#"{ "id": "11", "type": 3, "name": "", "recipient_ids": ["5", "6", "7", "x"] }"#, &users);
        let d = d.unwrap_or_else(|| panic!("MP attendu"));
        assert_eq!(d.name, "Bob, Eve");
        // Sans dernier message, le tri se fait sur l'identifiant du salon.
        assert_eq!(d.last_message, 11);

        let d = dm(r#"{ "id": "12", "type": 3, "name": "Les amis", "last_message_id": null }"#, &users);
        assert_eq!(d.map(|d| d.name), Some("Les amis".into()));

        let d = dm(r#"{ "id": "13", "type": 1 }"#, &users);
        assert_eq!(d.map(|d| d.name), Some(String::new()), "nom laissé vide, l'interface le traduit");
    }

    #[test]
    fn dm_rejects_guild_channels() {
        assert!(dm(r#"{ "id": "10", "type": 0, "guild_id": "1", "name": "général" }"#, &HashMap::new()).is_none());
    }

    #[test]
    fn guild_channels_are_filtered_and_sorted() {
        let g: RawGuild = serde_json::from_str(
            r#"{ "id": "1", "properties": { "name": "Mon serveur" }, "channels": [
                { "id": "20", "type": 0, "name": "b", "position": 1, "parent_id": "10" },
                { "id": "10", "type": 4, "name": "Cat", "position": 0 },
                { "id": "30", "type": 2, "name": "vocal", "position": 0 },
                { "id": "21", "type": 5, "name": "annonces", "position": 0 }
            ] }"#,
        )
        .unwrap_or_else(|e| panic!("JSON de test invalide : {e}"));
        let g = g.into_guild(None, None).unwrap_or_else(|| panic!("serveur attendu"));
        assert_eq!(g.name, "Mon serveur");
        let ids: Vec<u64> = g.channels.iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![21, 10, 20]);
        // Sans nom : vide, l'interface affiche un nom traduit.
        let g: RawGuild = serde_json::from_str(r#"{ "id": "2" }"#).unwrap_or_else(|e| panic!("JSON de test invalide : {e}"));
        assert_eq!(g.into_guild(None, None).map(|g| g.name), Some(String::new()));
    }

    // ------------------------------------------------------------ permissions

    fn channel(json: &str) -> Channel {
        serde_json::from_str::<RawChannel>(json)
            .ok()
            .and_then(RawChannel::into_channel)
            .unwrap_or_else(|| panic!("salon attendu : {json}"))
    }

    #[test]
    fn overwrites_documented_and_older_forms() {
        let c = channel(
            r#"{ "id": "1", "type": 0, "permission_overwrites": [
                { "id": "10", "type": 0, "allow": "1024", "deny": "2048" },
                { "id": "11", "type": 1, "allow": "0", "deny": "36893488147419103232" },
                { "id": "12", "type": "role", "allow": 64, "deny": 0 },
                { "id": "13", "type": "member", "allow": "0", "deny": "1024" }
            ] }"#,
        );
        let ows = c.overwrites.unwrap_or_else(|| panic!("dérogations attendues"));
        let p = Permissions;
        assert_eq!(
            ows,
            vec![
                Overwrite { id: 10, kind: OverwriteKind::Role, allow: p(1024), deny: p(2048) },
                // 2^65 : bits au-delà de 64 ignorés sans erreur.
                Overwrite { id: 11, kind: OverwriteKind::Member, allow: p(0), deny: p(0) },
                Overwrite { id: 12, kind: OverwriteKind::Role, allow: p(64), deny: p(0) },
                Overwrite { id: 13, kind: OverwriteKind::Member, allow: p(0), deny: p(1024) },
            ]
        );
    }

    #[test]
    fn unreadable_or_missing_overwrites_are_unknown() {
        for ows in [
            r#""permission_overwrites": [{ "id": "10", "type": 2, "allow": "0", "deny": "0" }]"#,
            r#""permission_overwrites": [{ "id": "10", "type": 0, "allow": "beaucoup", "deny": "0" }]"#,
            r#""permission_overwrites": [{ "id": "10", "type": 0, "allow": "0" }]"#,
            r#""permission_overwrites": [{ "id": "10", "type": 0, "allow": "0", "deny": null }]"#,
            r#""permission_overwrites": null"#,
            r#""name": "sans dérogations""#,
        ] {
            let c = channel(&format!(r#"{{ "id": "1", "type": 0, {ows} }}"#));
            assert_eq!(c.overwrites, None, "{ows}");
        }
        assert_eq!(channel(r#"{ "id": "1", "type": 0, "permission_overwrites": [] }"#).overwrites, Some(Vec::new()));
    }

    fn guild(json: &str, me: Option<u64>, merged: Option<Vec<RawGuildMember>>) -> Guild {
        serde_json::from_str::<RawGuild>(json)
            .ok()
            .and_then(|g| g.into_guild(me, merged.map(|members| MergedPart { members, roleless_ok: true })))
            .unwrap_or_else(|| panic!("serveur attendu : {json}"))
    }

    #[test]
    fn guild_roles_owner_and_my_member() {
        let json = r#"{ "id": "1", "name": "S", "owner_id": "5",
            "roles": [
                { "id": "1", "name": "@everyone", "permissions": "1024", "position": 0, "color": 0 },
                { "id": "2", "name": "Rouge", "permissions": "8", "position": 3, "color": 15158332 },
                { "id": "3", "name": "Illisible", "permissions": "n/a", "position": 1 },
                { "id": "4", "name": "Ancien", "permissions": 2048, "position": 99999999999 }
            ],
            "members": [
                { "user": { "id": "6", "username": "autre" }, "roles": ["2"] },
                { "user": { "id": "7", "username": "moi" }, "roles": ["4"], "nick": "Pseudo", "communication_disabled_until": "2030-01-01T00:00:00+00:00" }
            ] }"#;
        let g = guild(json, Some(7), None);
        assert_eq!(g.owner_id, Some(5));
        let roles: Vec<(u64, &str, u32, i32, u64)> = g.roles.iter().map(|r| (r.id, r.name.as_str(), r.color, r.position, r.permissions.0)).collect();
        assert_eq!(roles, vec![(1, "@everyone", 0, 0, 1024), (2, "Rouge", 15158332, 3, 8), (4, "Ancien", 0, i32::MAX, 2048)]);
        let me = g.me.unwrap_or_else(|| panic!("membre attendu"));
        assert_eq!(me.roles, vec![4]);
        assert_eq!(me.nick.as_deref(), Some("Pseudo"));
        assert_eq!(me.timeout_until, Some(1_893_456_000_000));
        // Sans mon identifiant, ou si je ne suis pas dans la liste : inconnu.
        assert!(guild(json, None, None).me.is_none());
        assert!(guild(json, Some(8), None).me.is_none());
    }

    #[test]
    fn guild_properties_and_merged_member() {
        let json = r#"{ "id": "1", "properties": { "name": "P", "owner_id": "5" }, "roles": [] }"#;
        let merged: Vec<RawGuildMember> = serde_json::from_str(r#"[{ "user_id": "6", "roles": [] }, { "user_id": "7", "roles": ["9"], "communication_disabled_until": null }]"#)
            .unwrap_or_else(|e| panic!("{e}"));
        let g = guild(json, Some(7), Some(merged));
        assert_eq!((g.name.as_str(), g.owner_id), ("P", Some(5)));
        assert_eq!(g.me, Some(Member { roles: vec![9], nick: None, timeout_until: None }));
    }

    #[test]
    fn unreadable_member_is_unknown() {
        for m in [
            r#"{ "user": { "id": "7" }, "roles": ["x"] }"#,
            r#"{ "user": { "id": "7" } }"#,
            r#"{ "user": { "id": "7" }, "roles": [], "communication_disabled_until": "demain" }"#,
        ] {
            let g = guild(&format!(r#"{{ "id": "1", "members": [{m}] }}"#), Some(7), None);
            assert!(g.me.is_none(), "{m}");
        }
    }

    // ------------------------------------------------------------ images

    const H: &str = "0123456789abcdef0123456789abcdef";

    fn hash() -> ImageHash {
        ImageHash::parse(H).unwrap_or_else(|| panic!("empreinte de test"))
    }

    #[test]
    fn message_author_avatar() {
        let m = message(&format!(r#"{{ "id": "1", "channel_id": "2", "author": {{ "id": "3", "username": "u", "avatar": "{H}" }} }}"#));
        assert_eq!(m.author_avatar.image, Some(ImageRef::Avatar { user_id: 3, hash: hash() }));
        // Absent, null, illisible ou d'un autre type : avatar par défaut, message gardé.
        for avatar in [r#""#, r#", "avatar": null"#, r#", "avatar": "../x""#, r#", "avatar": 12"#, r#", "avatar": {}"#] {
            let m = message(&format!(r#"{{ "id": "1", "channel_id": "2", "author": {{ "id": "{}", "username": "u"{avatar} }} }}"#, 5u64 << 22));
            assert_eq!(m.author_avatar, Avatar { image: None, default: 5 }, "{avatar}");
        }
        // Ancien discriminant.
        let m = message(r#"{ "id": "1", "channel_id": "2", "author": { "id": "3", "username": "u", "discriminator": "0007" } }"#);
        assert_eq!(m.author_avatar.default, 2);
    }

    #[test]
    fn guild_icon_from_field_or_properties() {
        let g = guild(&format!(r#"{{ "id": "1", "name": "S", "icon": "{H}" }}"#), None, None);
        assert_eq!(g.icon, Some(ImageRef::GuildIcon { guild_id: 1, hash: hash() }));
        let g = guild(&format!(r#"{{ "id": "1", "properties": {{ "name": "P", "icon": "a_{H}" }} }}"#), None, None);
        assert!(matches!(g.icon, Some(ImageRef::GuildIcon { guild_id: 1, hash }) if hash.animated));
        assert_eq!(guild(r#"{ "id": "1", "icon": null }"#, None, None).icon, None);
        assert_eq!(guild(r#"{ "id": "1", "icon": "pas une empreinte" }"#, None, None).icon, None);
    }

    #[test]
    fn guild_update_icon_absent_null_or_new() {
        let upd = |json: &str| serde_json::from_str::<RawGuildUpdate>(json).unwrap_or_else(|e| panic!("{e}")).icon();
        assert_eq!(upd(r#"{ "id": "1" }"#), None);
        assert_eq!(upd(r#"{ "id": "1", "icon": null }"#), Some(None));
        assert_eq!(upd(r#"{ "id": "1", "icon": 3 }"#), Some(None));
        assert_eq!(upd(&format!(r#"{{ "id": "1", "icon": "{H}" }}"#)), Some(Some(ImageRef::GuildIcon { guild_id: 1, hash: hash() })));
        assert_eq!(upd(&format!(r#"{{ "id": "1", "properties": {{ "icon": "{H}" }} }}"#)), Some(Some(ImageRef::GuildIcon { guild_id: 1, hash: hash() })));
    }

    #[test]
    fn dm_avatar_and_group_icon() {
        let d = dm(&format!(r#"{{ "id": "10", "type": 1, "recipients": [ {{ "id": "5", "username": "bob", "avatar": "{H}" }} ] }}"#), &HashMap::new());
        assert_eq!(d.map(|d| d.avatar.image), Some(Some(ImageRef::Avatar { user_id: 5, hash: hash() })));
        // MP par recipient_ids : avatar tiré de `users`.
        let users: HashMap<u64, (String, Avatar)> = [(5, ("Bob".to_string(), Avatar::of_user(5, Some(hash()), None)))].into();
        let d = serde_json::from_str::<RawChannel>(r#"{ "id": "10", "type": 1, "recipient_ids": ["5"] }"#).ok().and_then(|c| c.into_dm(&users));
        assert_eq!(d.map(|d| (d.name, d.avatar.image)), Some(("Bob".into(), Some(ImageRef::Avatar { user_id: 5, hash: hash() }))));
        // Groupe : son icône, pas l'avatar d'un membre.
        let d = dm(&format!(r#"{{ "id": "11", "type": 3, "icon": "{H}", "recipients": [ {{ "id": "5", "username": "bob", "avatar": "{H}" }} ] }}"#), &HashMap::new());
        assert_eq!(d.map(|d| d.avatar.image), Some(Some(ImageRef::GroupIcon { channel_id: 11, hash: hash() })));
        let d = dm(r#"{ "id": "11", "type": 3, "icon": null }"#, &HashMap::new());
        assert_eq!(d.map(|d| d.avatar), Some(Avatar::default_for(11)));
    }

    /// CHANNEL_UPDATE : seul ce que le paquet contient est déclaré connu.
    #[test]
    fn dm_update_declares_what_it_carries() {
        let upd = |json: &str| serde_json::from_str::<RawChannel>(json).ok().and_then(RawChannel::into_dm_update).map(|(_, k)| k);
        // Ni destinataires ni icône : rien à remplacer.
        assert_eq!(upd(r#"{ "id": "10", "type": 1 }"#), Some(DmKnown { name: false, avatar: false }));
        assert_eq!(upd(r#"{ "id": "11", "type": 3 }"#), Some(DmKnown { name: false, avatar: false }));
        assert_eq!(upd(r#"{ "id": "11", "type": 3, "name": "Amis" }"#), Some(DmKnown { name: true, avatar: false }));
        // Avec destinataires (MP à deux) : nom et avatar du correspondant.
        assert_eq!(upd(&format!(r#"{{ "id": "10", "type": 1, "recipients": [ {{ "id": "5", "username": "bob", "avatar": "{H}" }} ] }}"#)), Some(DmKnown::ALL));
        // Groupe : l'icône, même nulle (retirée), est une information.
        assert_eq!(upd(r#"{ "id": "11", "type": 3, "icon": null }"#), Some(DmKnown { name: false, avatar: true }));
        assert_eq!(upd(&format!(r#"{{ "id": "11", "type": 3, "icon": "{H}" }}"#)), Some(DmKnown { name: false, avatar: true }));
    }

    // ---------------------------------------------------------------- lecture sans copie
    //
    // Le module `old` garde les anciennes écritures (copie par
    // `Box<RawValue>`, `untagged`, `String` par identifiant) comme
    // références : les nouvelles doivent rendre exactement la même chose,
    // réussite comme échec, sur toutes les formes.

    #[derive(Debug, PartialEq, Deserialize)]
    struct Probe {
        #[serde(default, deserialize_with = "de_lenient")]
        a: Option<u32>,
        #[serde(default, deserialize_with = "de_lenient")]
        b: Option<Vec<Item<Inner>>>,
        #[serde(default, deserialize_with = "de_loose_id")]
        c: Option<u64>,
        #[serde(default, deserialize_with = "de_opt_id")]
        d: Option<u64>,
        #[serde(default, deserialize_with = "de_perms")]
        e: Option<Permissions>,
        #[serde(default, deserialize_with = "de_overwrite_kind")]
        f: Option<OverwriteKind>,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct Inner {
        #[serde(deserialize_with = "de_id")]
        id: u64,
        #[serde(default, deserialize_with = "de_lenient")]
        name: Option<String>,
    }

    impl<T: PartialEq> PartialEq for Item<T> {
        fn eq(&self, o: &Self) -> bool {
            self.0 == o.0
        }
    }

    impl<T: std::fmt::Debug> std::fmt::Debug for Item<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            self.0.fmt(f)
        }
    }

    mod old {
        use crate::state::model::OverwriteKind;
        use crate::state::Permissions;
        use serde::{de::DeserializeOwned, de::IgnoredAny, Deserialize, Deserializer};
        use serde_json::value::RawValue;

        pub fn de_id<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
            let s = String::deserialize(d)?;
            s.parse().map_err(serde::de::Error::custom)
        }

        pub fn de_opt_id<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
            Ok(Option::<String>::deserialize(d)?.and_then(|s| s.parse().ok()))
        }

        #[derive(Debug, PartialEq)]
        pub struct Item<T>(pub Option<T>);

        impl<'de, T: DeserializeOwned> Deserialize<'de> for Item<T> {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let raw = Box::<RawValue>::deserialize(d)?;
                Ok(Item(serde_json::from_str(raw.get()).ok()))
            }
        }

        pub fn de_lenient<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Option<T>, D::Error> {
            Ok(Item::<T>::deserialize(d)?.0)
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Loose {
            Str(String),
            Num(u64),
            #[allow(dead_code)]
            Other(IgnoredAny),
        }

        pub fn de_loose_id<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
            Ok(match Loose::deserialize(d)? {
                Loose::Str(s) => s.parse().ok(),
                Loose::Num(n) => Some(n),
                Loose::Other(_) => None,
            })
        }

        pub fn de_perms<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Permissions>, D::Error> {
            Ok(match Loose::deserialize(d)? {
                Loose::Str(s) => Permissions::parse(&s),
                Loose::Num(n) => Some(Permissions(n)),
                Loose::Other(_) => None,
            })
        }

        pub fn de_overwrite_kind<'de, D: Deserializer<'de>>(d: D) -> Result<Option<OverwriteKind>, D::Error> {
            Ok(match Loose::deserialize(d)? {
                Loose::Num(0) => Some(OverwriteKind::Role),
                Loose::Num(1) => Some(OverwriteKind::Member),
                Loose::Str(s) if s == "role" => Some(OverwriteKind::Role),
                Loose::Str(s) if s == "member" => Some(OverwriteKind::Member),
                _ => None,
            })
        }

        #[derive(Debug, PartialEq, Deserialize)]
        pub struct Probe {
            #[serde(default, deserialize_with = "de_lenient")]
            pub a: Option<u32>,
            #[serde(default, deserialize_with = "de_lenient")]
            pub b: Option<Vec<Item<Inner>>>,
            #[serde(default, deserialize_with = "de_loose_id")]
            pub c: Option<u64>,
            #[serde(default, deserialize_with = "de_opt_id")]
            pub d: Option<u64>,
            #[serde(default, deserialize_with = "de_perms")]
            pub e: Option<Permissions>,
            #[serde(default, deserialize_with = "de_overwrite_kind")]
            pub f: Option<OverwriteKind>,
        }

        #[derive(Debug, PartialEq, Deserialize)]
        pub struct Inner {
            #[serde(deserialize_with = "de_id")]
            pub id: u64,
            #[serde(default, deserialize_with = "de_lenient")]
            pub name: Option<String>,
        }
    }

    /// Valeurs de toutes les formes, bonnes et mauvaises, y compris des
    /// échappements, des espaces autour et des nombres hors bornes.
    const SHAPES: &[&str] = &[
        "null",
        "true",
        "false",
        "0",
        "1",
        "2",
        "-1",
        "7",
        "4294967296",
        "18446744073709551615",
        "18446744073709551616",
        "1.5",
        "-0.0",
        "1e3",
        r#""""#,
        r#""12""#,
        r#""role""#,
        r#""member""#,
        r#""-3""#,
        r#""1024""#,
        r#""abc""#,
        r#""a\"b\\cé\n""#,
        r#""18446744073709551616""#,
        "[]",
        "[1,2]",
        r#"[{"id":"5","name":"x"}]"#,
        r#"[{"id":"5","name":7}, 3, {"id":5}, {"id":"6"}, null, [], {"id":"x"}]"#,
        "{}",
        r#"{"id":"9"}"#,
        r#"{"a":{"b":[1,{"c":null}]}}"#,
        r#"  [ { "id" : "5" , "name" : "é\té" } ]  "#,
    ];

    type Flat = (Option<u32>, Option<Vec<Option<(u64, Option<String>)>>>, Option<u64>, Option<u64>, Option<Permissions>, Option<OverwriteKind>);

    fn flat(p: Probe) -> Flat {
        (p.a, p.b.map(|l| l.into_iter().map(|i| i.0.map(|x| (x.id, x.name))).collect()), p.c, p.d, p.e, p.f)
    }

    fn flat_old(p: old::Probe) -> Flat {
        (p.a, p.b.map(|l| l.into_iter().map(|i| i.0.map(|x| (x.id, x.name))).collect()), p.c, p.d, p.e, p.f)
    }

    #[test]
    fn lenient_readers_match_the_copying_ones_on_every_shape() {
        let mut checked = 0;
        for field in ["a", "b", "c", "d", "e", "f"] {
            for shape in SHAPES {
                let json = format!(r#"{{ "x": [1, "{{"], "{field}": {shape}, "y": null }}"#);
                // Texte en mémoire (tous nos paquets), octets, flux (le texte
                // brut y est possédé) et arbre déjà décodé.
                let new = serde_json::from_str::<Probe>(&json).ok().map(flat);
                let was = serde_json::from_str::<old::Probe>(&json).ok().map(flat_old);
                assert_eq!(new, was, "{json}");
                assert_eq!(serde_json::from_slice::<Probe>(json.as_bytes()).ok().map(flat), was, "octets {json}");
                assert_eq!(serde_json::from_reader::<_, Probe>(json.as_bytes()).ok().map(flat), was, "flux {json}");
                if let Ok(tree) = serde_json::from_str::<serde_json::Value>(&json) {
                    let n = serde_json::from_value::<Probe>(tree.clone()).ok().map(flat);
                    let w = serde_json::from_value::<old::Probe>(tree).ok().map(flat_old);
                    assert_eq!(n, w, "arbre {json}");
                }
                checked += 1;
            }
        }
        assert_eq!(checked, 6 * SHAPES.len());
        // Un texte tronqué reste une erreur de tout le paquet, comme avant.
        for cut in [r#"{ "a": [1, "#, r#"{ "b": [{"id":"5"}"#, r#"{ "c": "12"#] {
            assert!(serde_json::from_str::<Probe>(cut).is_err() && serde_json::from_str::<old::Probe>(cut).is_err(), "{cut}");
        }
    }

    #[test]
    fn raw_text_is_lent_without_copy() {
        struct Seen(String);
        impl<'de> Deserialize<'de> for Seen {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                with_raw(d, |raw| Seen(raw.to_owned()))
            }
        }
        // Le texte remis est celui de `RawValue`, caractère pour caractère.
        for shape in SHAPES {
            let json = format!("[ {shape} ,1]");
            let (got, _): (Seen, u8) = serde_json::from_str(&json).unwrap();
            let (want, _): (Box<RawValue>, u8) = serde_json::from_str(&json).unwrap();
            assert_eq!(got.0, want.get(), "{json}");
        }
        // Et il pointe dans le texte lu : aucune copie.
        struct Inside(usize);
        impl<'de> Deserialize<'de> for Inside {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                with_raw(d, |raw| Inside(raw.as_ptr() as usize))
            }
        }
        let json = String::from(r#"{"k": {"deep": [1, 2, 3]}}"#);
        let got: HashMap<String, Inside> = serde_json::from_str(&json).unwrap();
        let range = json.as_ptr() as usize..json.as_ptr() as usize + json.len();
        assert!(range.contains(&got["k"].0), "le texte brut a été copié");
        // Sous un tampon de serde (`untagged`), échec comme `RawValue`.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Buffered {
            #[allow(dead_code)]
            New(Seen),
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum BufferedOld {
            #[allow(dead_code)]
            Old(Box<RawValue>),
        }
        assert!(serde_json::from_str::<Buffered>(r#"{"a":1}"#).is_err());
        assert!(serde_json::from_str::<BufferedOld>(r#"{"a":1}"#).is_err());
    }
}
