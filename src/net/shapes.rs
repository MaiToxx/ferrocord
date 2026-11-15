//! Enregistreur de formes anonymisées, compilé seulement en debug.
//!
//! Avec `FERROCORD_RECORD_SHAPES=<dossier>`, la première occurrence de chaque
//! type d'événement de la passerelle est écrite dans `<dossier>/<TYPE>.json`,
//! réduite à sa forme : clés connues de l'API, types des valeurs, aucune
//! valeur. Sert à comparer les jeux de données de `tests/fixtures/` aux vrais
//! paquets sans rien divulguer du compte.

use serde_json::{Map, Value};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

pub const ENV: &str = "FERROCORD_RECORD_SHAPES";

/// Noms de champs de l'API Discord conservés tels quels. Toute autre clé
/// peut être une donnée (identifiant, pseudo, code de langue…) et devient
/// `"<id>"` ou `"<key>"`. Liste fixe : un champ absent est masqué, jamais
/// divulgué ; on l'ajoute ici quand on en a besoin.
const KNOWN_FIELDS: &[&str] = &[
    // corps d'erreur de l'API (« Error Messages » : `errors.<champ>._errors`, lot « profil rapide »)
    "_errors", "errors",
    // paramètres du serveur, webhooks, intégrations (lot A8 + A9)
    "account", "enable_emoticons", "enabled", "expire_behavior", "expire_grace_period", "permission", "source_channel", "source_guild",
    "subscriber_count", "synced_at", "syncing",
    // modération, journal d'audit, AutoMod, sécurité (lots A4 + A5)
    "action", "action_type", "actions", "alert_system_message_id", "allow_list", "audit_log_entries", "auto_moderation_rules", "changes",
    "custom_message", "dm_spam_detected_at", "dms_disabled_until", "duration_seconds", "event_type", "exempt_channels",
    "exempt_roles", "invites_disabled_until", "key", "keyword_filter", "matched_content", "matched_keyword", "mention_raid_protection_enabled",
    "mention_total_limit", "new_value", "old_value", "raid_detected_at", "reason", "regex_patterns", "rule_id", "rule_trigger_type",
    "target_id", "trigger_metadata", "trigger_type", "webhooks",
    // événements programmés, écran de bienvenue, intégration (lot A7)
    "auto_start", "below_requirements", "by_month", "by_month_day", "by_n_weekday", "by_weekday", "by_year_day", "day", "default_channel_ids", "entity_id",
    "entity_metadata", "entity_type", "frequency", "guild_scheduled_event_exception_id", "guild_scheduled_event_exceptions", "guild_scheduled_event_id", "in_onboarding",
    "interval", "mode", "n", "onboarding_prompts_seen", "onboarding_responses_seen", "privacy_level", "prompts", "recurrence_rule", "response", "responses", "role_ids",
    "scheduled_end_time", "scheduled_start_time", "single_select", "sku_ids", "user_count", "welcome_channels",
    // emojis, autocollants et sons d'un serveur (lot A6)
    "sound_id", "volume",
    // invitations et modèles de serveur (lot A1)
    "channel", "code", "creator", "creator_id", "guild", "inviter", "is_dirty", "max_age", "max_uses", "new_member",
    "serialized_source_guild", "show_verification_form", "source_guild_id", "temporary", "updated_at", "usage_count", "uses",
    // recherche de messages (lot T12)
    "analytics_id", "doing_deep_historical_index", "documents_indexed", "hit", "messages", "retry_after", "total_results",
    // commandes et composants des applications (lot T11)
    "accessory", "application_commands", "applications", "attachment_id", "autocomplete", "bot_id", "channel_types",
    "choices", "command_type", "component", "contexts", "custom_id", "default", "default_member_permissions", "disabled",
    "divider", "integration_types", "label", "max_length", "max_value", "max_values", "media", "min_value", "min_values",
    "name_localized", "options", "reason_code", "required", "spacing", "spoiler", "style",
    // sondages, invitations, adresses rafraîchies (lot T13)
    "allow_multiselect", "answer_counts", "answer_id", "answers", "expiry",
    "is_finalized", "layout_type", "me_voted", "original", "poll_media", "question", "refreshed", "refreshed_urls", "results",
    // fils et forums (lot T10)
    "added_members", "archive_timestamp", "archived", "auto_archive_duration", "channel_ids", "create_timestamp", "invitable",
    "join_timestamp", "locked", "moderated", "newly_created", "removed_member_ids",
    // enveloppe et session
    "_trace", "analytics_token", "api_code_version", "application", "auth", "auth_session_id_hash", "connected_accounts",
    "consents", "country_code", "d", "experiments", "friend_suggestion_count", "geo_ordered_rtc_regions",
    "guild_experiments", "guild_join_requests", "heartbeat_interval", "lazy_private_channels", "merged_members",
    "merged_presences", "notification_settings", "op", "presences", "private_channels", "read_state", "relationships",
    "required_action", "resume_gateway_url", "s", "seq", "session_id", "session_type", "sessions", "shard",
    "static_client_session_id", "t", "tutorial", "user", "user_guild_settings", "user_settings", "user_settings_proto",
    "users", "v",
    // réglages et états de lecture
    "badge_count", "channel_overrides", "custom_status", "developer_mode", "entries", "flags", "guild_folders",
    "guild_positions", "last_acked_id", "last_pin_timestamp", "last_viewed", "locale", "mention_count",
    "message_notifications", "mobile_push", "muted", "mute_config", "partial", "read_state_type", "status",
    "suppress_everyone", "suppress_roles", "theme", "version",
    // utilisateurs et membres
    "accent_color", "age_verification_status", "asset", "avatar", "avatar_decoration_data", "banner", "banner_color",
    "bio", "bot", "clan", "collectibles", "communication_disabled_until", "deaf", "discriminator", "display_name_styles",
    "email", "expires_at", "global_name", "joined_at", "mfa_enabled", "mute", "nick", "nickname", "nsfw_allowed",
    "pending", "phone", "premium_since", "premium_type", "primary_guild", "pronouns", "public_flags", "purchased_flags",
    "sku_id", "system", "type", "user_id", "username", "verified",
    // serveurs, rôles, emojis
    "afk_channel_id", "afk_timeout", "animated", "application_id", "approximate_member_count",
    "approximate_presence_count", "available", "color", "colors", "data_mode", "default_message_notifications",
    "description", "discovery_splash", "embedded_activities", "emoji_id", "emoji_name", "emojis",
    "explicit_content_filter", "features", "format_type", "guild_scheduled_events", "hoist", "home_header",
    "hub_type", "icon", "icon_hash", "id", "incidents_data", "large", "lazy", "managed", "max_members",
    "max_presences", "max_stage_video_channel_users", "max_video_channel_users", "member_count", "members",
    "mentionable", "mfa_level", "name", "nsfw_level", "owner", "owner_id", "pack_id", "permissions", "position",
    "preferred_locale", "premium_progress_bar_enabled", "premium_subscription_count", "premium_tier",
    "primary_color", "properties", "public_updates_channel_id", "region", "require_colons", "roles",
    "role", "role_id", "rules_channel_id", "safety_alerts_channel_id", "secondary_color", "sort_value", "soundboard_sounds", "splash",
    "stage_instances", "stickers", "system_channel_flags", "system_channel_id", "tags", "tertiary_color",
    "unavailable", "unicode_emoji", "vanity_url_code", "verification_level", "voice_states", "widget_channel_id",
    "widget_enabled",
    // appels, conférences, effets vocaux (lot V6)
    "animation_id", "animation_type", "discoverable_disabled", "ringing", "sound_volume",
    // salons et fils
    "allow", "applied_tags", "available_tags", "bitrate", "channel_id", "channels", "default_auto_archive_duration",
    "default_forum_layout", "default_reaction_emoji", "default_sort_order", "default_thread_rate_limit_per_user",
    "deny", "guild_id", "guilds", "icon_emoji", "is_message_request", "is_message_request_timestamp", "is_spam",
    "last_message_id", "member", "message_count", "nsfw", "parent_id", "permission_overwrites", "rate_limit_per_user",
    "recipient_ids", "recipients", "rtc_region", "theme_color", "thread_metadata", "threads", "topic",
    "total_message_sent", "user_limit", "video_quality_mode",
    // messages, pièces jointes, embeds, réactions
    "attachments", "author", "burst", "burst_colors", "burst_count", "call", "components", "content", "content_scan_version",
    "content_type", "count", "count_details", "duration_secs", "edited_timestamp", "embeds", "emoji", "enforce_nonce",
    "ephemeral", "fail_if_not_exists", "fields", "filename", "footer", "height", "icon_url", "image", "inline",
    "interaction", "interaction_metadata", "me", "me_burst", "mention_channels", "mention_everyone", "mention_roles",
    "mentions", "message_id", "message_reference", "message_snapshots", "nonce", "original_content_type", "pinned",
    "placeholder", "placeholder_version", "poll", "provider", "proxy_icon_url", "proxy_url", "reactions",
    "referenced_message", "resolved", "role_subscription_data", "size", "sticker_items", "text", "thread", "thumbnail",
    "timestamp", "title", "tts", "url", "value", "video", "waveform", "webhook_id", "width",
    // réactions et épingles (lots T5/T6)
    "has_more", "items", "message", "message_author_id", "normal", "pinned_at",
    // présences, activités, vocal
    "activities", "afk", "assets", "buttons", "client_status", "created_at", "desktop", "details", "end", "large_image",
    "large_text", "mobile", "party", "platform", "request_to_speak_timestamp", "secrets", "self_deaf", "self_mute",
    "self_stream", "self_video", "since", "small_image", "small_text", "start", "state", "suppress", "sync_id",
    "timestamps", "web", "endpoint",
    // liste des membres (GUILD_MEMBER_LIST_UPDATE)
    "group", "groups", "index", "item", "online_count", "ops", "presence", "range", // « items » : déjà plus haut (épingles)
    // réglages du compte, appareils, comptes liés, applications autorisées
    "approx_last_used_time", "bot_public", "client_info", "collapsed", "end_time", "friend_sync", "hide_muted_channels",
    "id_hash", "integrations", "location", "notify_highlights", "os", "out_of_date", "proto", "required_data_version",
    "revoked", "scopes", "selected_time_window", "settings", "show_activity", "two_way_link", "user_sessions", "visibility",
    // activité de jeu (lot C3)
    "aliases", "arguments", "artist_ids", "button_urls", "executables", "hook", "is_launcher", "overlay",
    "overlay_compatibility_hook", "overlay_methods", "overlay_warn", "themes",
    // amis, accusés de lecture (lot S3 + T9)
    "disclose", "friends", "manual", "should_notify", "user_ignored",
    // profil complet et notes (lot profil-complet)
    "application_role_connections", "badges", "guild_badges", "guild_member", "guild_member_profile", "legacy_username", "link",
    "metadata", "mutual_friends", "mutual_friends_count", "mutual_guilds", "note", "note_user_id",
    "popout_animation_particle_type", "premium_guild_since", "profile_effect", "theme_colors", "user_profile", "widgets",
    // consentements (pages Données et confidentialité)
    "consented", "personalization", "usage_statistics",
    // étiquettes des rôles (lot A3)
    "guild_connections", "integration_id", "premium_subscriber",
    // directs (phase 5, lot M4)
    "paused", "rtc_channel_id", "rtc_server_id", "stream_key", "viewer_ids",
    // parures des profils (lot P)
    "badge", "effect_id", "font_id", "identity_enabled", "identity_guild_id", "nameplate", "palette", "tag",
    // régions vocales (phase 10, lot S ; Voice Region Object, officiel)
    "custom", "deprecated", "optimal",
    // sélecteur de GIF (lot G)
    "categories", "gif_src", "gifs", "preview", "src",
];

/// Champs des paquets ENVOYÉS par l'app (Identify, Resume, Heartbeat), en
/// plus de `KNOWN_FIELDS`. `token` n'y est pas : la clé elle-même est masquée,
/// et sa valeur, comme toute valeur, est remplacée.
const SENT_FIELDS: &[&str] = &[
    "browser", "compress", "device", "intents", "large_threshold", "os", "subscriptions", "typing",
    // Propriétés du mode bureau (`net::identity`).
    "app_arch", "browser_user_agent", "browser_version", "client_build_number", "client_event_source", "client_heartbeat_session_id", "client_launch_id", "client_version",
    "gateway_connect_reasons", "has_client_mods", "is_fast_connect", "launch_signature", "native_build_number", "os_arch", "os_sdk_version", "os_version", "release_channel", "system_locale",
];

fn is_known_field(k: &str) -> bool {
    KNOWN_FIELDS.contains(&k)
}

fn is_sent_field(k: &str) -> bool {
    is_known_field(k) || SENT_FIELDS.contains(&k)
}

/// Forme anonymisée de `v` : chaînes remplacées par `"<id>"` (chiffres
/// seulement) ou `"<str>"`, nombres par 0, booléens par `false`, tableaux
/// réduits à leur premier élément. Les clés hors de `KNOWN_FIELDS` sont des
/// données : elles deviennent `"<id>"` ou `"<key>"`, et seule la première
/// (dans l'ordre trié) est gardée.
pub fn anonymize(v: &Value) -> Value {
    anonymize_with(v, is_known_field)
}

/// Comme [`anonymize`], pour un paquet envoyé (liste de champs élargie).
pub fn anonymize_sent(v: &Value) -> Value {
    anonymize_with(v, is_sent_field)
}

fn anonymize_with(v: &Value, known: fn(&str) -> bool) -> Value {
    match v {
        Value::Null => Value::Null,
        Value::Bool(_) => Value::Bool(false),
        Value::Number(_) => Value::from(0),
        Value::String(s) => Value::String(if is_id(s) { "<id>" } else { "<str>" }.into()),
        Value::Array(a) => Value::Array(a.first().map(|x| anonymize_with(x, known)).into_iter().collect()),
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, x) in m {
                let key = if known(k) {
                    k.clone()
                } else if is_id(k) {
                    "<id>".into()
                } else {
                    "<key>".into()
                };
                out.entry(key).or_insert_with(|| anonymize_with(x, known));
            }
            Value::Object(out)
        }
    }
}

fn is_id(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Nom d'événement utilisable comme nom de fichier (`MESSAGE_CREATE`).
fn is_event_name(k: &str) -> bool {
    !k.is_empty() && k.len() <= 64 && k.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

pub struct Recorder {
    dir: PathBuf,
    seen: HashSet<String>,
}

impl Recorder {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, seen: HashSet::new() }
    }

    /// Ce type d'événement reste-t-il à enregistrer ? (vérification bon marché)
    pub fn wants(&self, kind: &str) -> bool {
        is_event_name(kind) && !self.seen.contains(kind)
    }

    /// Écrit la forme de `d` la première fois que `kind` est vu ; renvoie le
    /// fichier écrit. Bloquant (analyse et disque) : à appeler hors de la
    /// tâche réseau.
    pub fn record(&mut self, kind: &str, d: &str) -> Option<PathBuf> {
        if !self.wants(kind) {
            return None;
        }
        let v: Value = serde_json::from_str(d).ok()?;
        self.seen.insert(kind.to_string());
        let shape = if kind.starts_with(SENT_PREFIX) { anonymize_sent(&v) } else { anonymize(&v) };
        let text = serde_json::to_string_pretty(&shape).ok()?;
        std::fs::create_dir_all(&self.dir).ok()?;
        let path = self.dir.join(format!("{kind}.json"));
        std::fs::write(&path, text).ok()?;
        Some(path)
    }
}

/// Préfixe des fichiers de paquets envoyés : `SENT_OP_2.json` (Identify),
/// `SENT_OP_6.json` (Resume), `SENT_OP_1.json` (Heartbeat)… Aucun type
/// d'événement de Discord ne commence ainsi.
const SENT_PREFIX: &str = "SENT_OP_";

/// Nom de fichier d'un paquet envoyé (`{"op": 2, "d": …}`) et sa partie `d`.
fn sent_kind(packet: &str) -> Option<(String, String)> {
    #[derive(serde::Deserialize)]
    struct Sent<'a> {
        op: u8,
        #[serde(borrow, default)]
        d: Option<&'a serde_json::value::RawValue>,
    }
    let p: Sent = serde_json::from_str(packet).ok()?;
    Some((format!("{SENT_PREFIX}{}", p.op), p.d.map_or("null", |d| d.get()).to_string()))
}

fn recorder() -> Option<&'static Mutex<Recorder>> {
    static REC: OnceLock<Option<Mutex<Recorder>>> = OnceLock::new();
    REC.get_or_init(|| {
        std::env::var_os(ENV)
            .filter(|v| !v.is_empty())
            .map(|dir| Mutex::new(Recorder::new(dir.into())))
    })
    .as_ref()
}

/// Enregistre la forme d'un paquet envoyé par l'app (jamais ses valeurs :
/// le jeton de l'Identify devient `"<str>"`, sous une clé masquée).
pub fn record_sent(packet: &str) {
    let Some(rec) = recorder() else { return };
    let Some((kind, d)) = sent_kind(packet) else { return };
    if !rec.lock().is_ok_and(|r| r.wants(&kind)) {
        return;
    }
    tokio::task::spawn_blocking(move || {
        if let Ok(mut r) = rec.lock() {
            r.record(&kind, &d);
        }
    });
}

/// Enregistre `d` si la variable d'environnement est définie (lue une fois).
/// L'analyse et l'écriture passent par `spawn_blocking` : la tâche réseau ne
/// fait qu'une vérification et, une fois par type, une copie du paquet.
pub fn record(kind: &str, d: &str) {
    let Some(rec) = recorder() else { return };
    if kind.starts_with(SENT_PREFIX) || !rec.lock().is_ok_and(|r| r.wants(kind)) {
        return;
    }
    let (kind, d) = (kind.to_string(), d.to_string());
    tokio::task::spawn_blocking(move || {
        if let Ok(mut r) = rec.lock() {
            r.record(&kind, &d);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn strings<'a>(v: &'a Value, keys: &mut Vec<&'a str>, values: &mut Vec<&'a str>) {
        match v {
            Value::String(s) => values.push(s),
            Value::Array(a) => a.iter().for_each(|x| strings(x, keys, values)),
            Value::Object(m) => {
                for (k, x) in m {
                    keys.push(k);
                    strings(x, keys, values);
                }
            }
            _ => {}
        }
    }

    /// Aucune chaîne d'origine (valeur ou clé de données) ne survit.
    fn assert_no_leak(input: &Value) {
        let out = anonymize(input);
        let text = out.to_string();
        let (mut in_keys, mut in_values) = (Vec::new(), Vec::new());
        strings(input, &mut in_keys, &mut in_values);
        let (mut out_keys, mut out_values) = (Vec::new(), Vec::new());
        strings(&out, &mut out_keys, &mut out_values);

        assert!(out_values.iter().all(|s| *s == "<id>" || *s == "<str>"), "valeur conservée : {text}");
        for k in &out_keys {
            assert!(["<id>", "<key>"].contains(k) || is_known_field(k), "clé inattendue {k} : {text}");
        }
        for s in in_values.iter().chain(in_keys.iter().filter(|k| !is_known_field(k))) {
            // Une valeur qui serait aussi un nom de champ connu (ou une partie :
            // « online » dans « online_count ») apparaît légitimement comme clé.
            if s.len() >= 3 && !out_keys.iter().any(|k| k.contains(*s)) {
                assert!(!text.contains(*s), "« {s} » a survécu : {text}");
            }
        }
    }

    fn fixtures() -> Vec<(String, Value)> {
        let dir = format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"));
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{dir} : {e}")).flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                let v = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path:?} : {e}"));
                out.push((path.display().to_string(), v));
            }
        }
        assert!(!out.is_empty());
        out
    }

    #[test]
    fn known_fields_are_field_names() {
        let mut seen = HashSet::new();
        for k in KNOWN_FIELDS {
            assert!(k.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'), "{k}");
            assert!(seen.insert(k), "{k} en double");
        }
    }

    #[test]
    fn known_fields_cover_fixtures() {
        // Les jeux de données suivent la documentation : leur forme doit être
        // conservée en entier (sinon la liste est incomplète).
        for (name, v) in fixtures() {
            let (mut keys, mut _values) = (Vec::new(), Vec::new());
            strings(&v, &mut keys, &mut _values);
            for k in keys {
                assert!(is_known_field(k), "{name} : champ {k} absent de KNOWN_FIELDS");
            }
            assert_no_leak(&v);
        }
    }

    #[test]
    fn anonymize_hides_lowercase_username_keys() {
        // Clés de données qui ont la forme d'un nom de champ (pseudos en minuscules).
        let input = json!({
            "content": "x",
            "notes_by_user": { "alice_fictive": "note", "bob42": "autre", "carol": "encore" }
        });
        let out = anonymize(&input);
        let text = out.to_string();
        for pseudo in ["alice_fictive", "bob42", "carol", "notes_by_user"] {
            assert!(!text.contains(pseudo), "« {pseudo} » a survécu : {text}");
        }
        assert_eq!(out, json!({ "content": "<str>", "<key>": { "<key>": "<str>" } }));
        assert_no_leak(&input);
    }

    #[test]
    fn anonymize_keeps_shape_only() {
        let input = json!({
            "id": "1234567890123456789",
            "content": "texte secret",
            "member_count": 42,
            "size": 0.5,
            "tts": true,
            "topic": null,
            "embeds": [{ "title": "x1" }, { "description": "x2" }],
            "attachments": [],
            "author": { "roles": ["s", "t"] }
        });
        assert_eq!(
            anonymize(&input),
            json!({
                "id": "<id>",
                "content": "<str>",
                "member_count": 0,
                "size": 0,
                "tts": false,
                "topic": null,
                "embeds": [{ "title": "<str>" }],
                "attachments": [],
                "author": { "roles": ["<str>"] }
            })
        );
    }

    #[test]
    fn anonymize_hides_data_keys() {
        let input = json!({
            "users": { "1000000000000000001": "note privée", "1000000000000000002": "autre" },
            "locale": { "fr-FR": "Bonjour", "Nom Propre": "x" },
            "": "vide"
        });
        assert_eq!(
            anonymize(&input),
            json!({ "users": { "<id>": "<str>" }, "locale": { "<key>": "<str>" }, "<key>": "<str>" })
        );
        assert_no_leak(&input);
    }

    #[test]
    fn anonymize_never_leaks_strings() {
        assert_no_leak(&json!({
            "token": "faux.jeton.QUI.NE.DOIT.PAS.SORTIR",
            "user": { "id": "4242424242424242", "username": "pseudo_secret", "email": "adresse@secrete.invalid" },
            "messages": [{ "content": "contenu privé", "mentions": [{ "id": "777" }] }, { "content": "second message privé" }],
            "4242424242424242": { "nick": "surnom secret" }
        }));
        let out = anonymize(&json!({ "token": "faux.jeton" }));
        assert_eq!(out, json!({ "<key>": "<str>" }), "« token » n'est pas un champ conservé");
    }

    #[test]
    fn sent_packets_keep_field_names_but_never_the_token() {
        let token = "faux.jeton.ENVOYE-QUI-NE-DOIT-PAS-SORTIR";
        let identify = json!({ "op": 2, "d": {
            "token": token,
            "properties": { "os": "windows", "browser": "Ferrocord", "device": "Ferrocord" },
            "compress": false, "large_threshold": 50, "intents": 4096
        } })
        .to_string();
        let (kind, d) = sent_kind(&identify).unwrap_or_else(|| panic!("paquet lisible"));
        assert_eq!(kind, "SENT_OP_2");
        let dir = std::env::temp_dir().join(format!("ferrocord-shapes-sent-{}", std::process::id()));
        let mut r = Recorder::new(dir.clone());
        let path = r.record(&kind, &d).unwrap_or_else(|| panic!("fichier attendu"));
        let written = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(!written.contains("jeton") && !written.contains("ENVOYE") && !written.contains("Ferrocord"), "{written}");
        let v: Value = serde_json::from_str(&written).unwrap_or(Value::Null);
        assert_eq!(
            v,
            json!({ "<key>": "<str>", "properties": { "os": "<str>", "browser": "<str>", "device": "<str>" },
                    "compress": false, "large_threshold": 0, "intents": 0 })
        );
        let (kind, _) = sent_kind(r#"{"op":1,"d":null}"#).unwrap_or_else(|| panic!("heartbeat lisible"));
        assert_eq!(kind, "SENT_OP_1");
        assert!(sent_kind("pas du json").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recorder_writes_once_per_kind_with_safe_names() {
        let dir = std::env::temp_dir().join(format!("ferrocord-shapes-test-{}", std::process::id()));
        let mut r = Recorder::new(dir.clone());

        assert!(r.wants("MESSAGE_CREATE"));
        let first = r.record("MESSAGE_CREATE", r#"{ "content": "premier" }"#);
        let path = first.unwrap_or_else(|| panic!("fichier attendu"));
        assert_eq!(path, dir.join("MESSAGE_CREATE.json"));
        assert!(!r.wants("MESSAGE_CREATE"));
        assert!(r.record("MESSAGE_CREATE", r#"{ "content": "second", "tts": 1 }"#).is_none(), "une seule forme par type");
        let written = std::fs::read_to_string(&path).unwrap_or_default();
        let written: Value = serde_json::from_str(&written).unwrap_or(Value::Null);
        assert_eq!(written, json!({ "content": "<str>" }));

        assert!(r.record("../ECHAPPEMENT", "{}").is_none());
        assert!(r.record("message_create", "{}").is_none());
        assert!(r.record("", "{}").is_none());
        assert!(r.record("GUILD_CREATE", "pas du json").is_none());
        // Un JSON illisible ne consomme pas le type : le suivant est enregistré.
        assert!(r.record("GUILD_CREATE", "{}").is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
