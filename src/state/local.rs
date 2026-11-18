//! Réglages de cet ordinateur que Discord ne synchronise pas (fichier
//! `local.toml`, à côté de `theme.toml`) : accessibilité, mode streamer,
//! journal de débogage. Rien ici ne part chez Discord.
//!
//! Discord garde aussi ces réglages sur l'appareil (aucun champ
//! `accessibility` ni « mode streamer » dans `PreloadedUserSettings`,
//! `discord-userdoccers/discord-protos`, lu le 13 novembre 2026).

/// Réduire les animations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Motion {
    /// Comme Windows (« Effets d'animation »).
    #[default]
    System,
    On,
    Off,
}

impl Motion {
    const KEYS: [(Motion, &'static str); 3] = [(Motion::System, "systeme"), (Motion::On, "oui"), (Motion::Off, "non")];

    fn key(self) -> &'static str {
        Self::KEYS.iter().find(|(m, _)| *m == self).map_or("systeme", |(_, k)| k)
    }

    fn of(key: &str) -> Option<Motion> {
        Self::KEYS.iter().find(|(_, k)| *k == key).map(|(m, _)| *m)
    }

    /// Animations réduites, sachant ce que dit Windows (`None` : inconnu).
    pub fn reduced(self, windows_reduced: Option<bool>) -> bool {
        match self {
            Motion::System => windows_reduced.unwrap_or(false),
            Motion::On => true,
            Motion::Off => false,
        }
    }
}

/// Intensité des animations de l'interface (lot ANIM).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Intensity {
    None,
    /// Fondus seulement.
    Subtle,
    #[default]
    Full,
}

impl Intensity {
    const KEYS: [(Intensity, &'static str); 3] = [(Intensity::None, "aucune"), (Intensity::Subtle, "discrete"), (Intensity::Full, "complete")];

    fn key(self) -> &'static str {
        Self::KEYS.iter().find(|(m, _)| *m == self).map_or("complete", |(_, k)| k)
    }

    fn of(key: &str) -> Option<Intensity> {
        Self::KEYS.iter().find(|(_, k)| *k == key).map(|(m, _)| *m)
    }
}

/// Mode streamer : ce qu'il masque ou coupe quand il est actif.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Streamer {
    pub on: bool,
    /// Adresse e-mail, téléphone, notes, identifiants.
    pub hide_personal: bool,
    pub hide_invites: bool,
    pub mute_sounds: bool,
    pub mute_toasts: bool,
}

impl Default for Streamer {
    fn default() -> Self {
        Streamer { on: false, hide_personal: true, hide_invites: true, mute_sounds: true, mute_toasts: true }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalPrefs {
    pub motion: Motion,
    /// Intensité des animations de l'interface (aucune, discrète, complète).
    pub intensity: Intensity,
    /// Saturation des couleurs du thème, de 0 à 100 %.
    pub saturation: u8,
    /// Vitesse de défilement à la molette, en % (100 : un cran de 3 lignes = 100 points, comme Discord).
    pub scroll_speed: u16,
    pub underline_links: bool,
    /// Couleur des rôles dans les noms (liste des membres, mentions de rôle).
    pub role_colors: bool,
    /// Texte des boutons à icône affiché dès le survol, sans délai.
    pub instant_tooltips: bool,
    pub streamer: Streamer,
    /// Journal local de débogage (`ferrocord.log`).
    pub debug_log: bool,
    /// Muet, sourdine, volumes et muets par personne (phase 4, `state::voice`).
    pub voice: super::voice::VoicePrefs,
    /// Périphériques, volumes, détection de la voix, traitement : page « Voix
    /// et vidéo » (section `[voix]`, lot V5).
    pub audio: super::audio_prefs::AudioPrefs,
    /// Caméra, aperçu avant la caméra, qualité et son du partage : même page
    /// (section `[video]`, lot restes).
    pub video: Box<super::video_prefs::VideoPrefs>,
    /// Adresse du manifeste des mises à jour (section `[mises_a_jour]`, lot
    /// C6) ; vide : mises à jour désactivées.
    pub update_manifest: String,
    /// Programmes ajoutés à la main, montrés comme jeux (section `[jeux]`, lot C3).
    pub games: Vec<super::activity::ManualGame>,
    /// Jeux vus récemment et leurs masques (même section, lot « activité »).
    pub recent_games: Vec<super::activity::RecentGame>,
    /// Disposition mobile et marge du clavier virtuel (section `[mobile]`, phase 7).
    pub mobile: super::mobile::MobilePrefs,
    /// Plugins activés et leurs réglages (section `[plugins]`, lot P0). L'historique
    /// hors ligne et la Rich Presence locale en sont : leurs anciennes clés
    /// (`historique_hors_ligne`, `[rich_presence]`) sont encore lues.
    pub plugins: Box<crate::plugins::PluginPrefs>,
    /// Identité annoncée à Discord (section `[identite]`, décision du 14 novembre 2026).
    pub identity: super::identity_prefs::IdentityPrefs,
    /// Personnalisation du client (section `[apparence]`, lot PERSO).
    pub look: Box<super::look_prefs::LookPrefs>,
    /// Mots-clés surveillés (section `[mots_cles]`, plugin du lot E1).
    pub keywords: Box<super::keywords::KeywordPrefs>,
    /// Catégories de contacts de la page Amis (section `[categories_amis]`).
    pub friend_groups: Box<super::friend_groups::FriendGroups>,
    /// Onglets, vue scindée, fenêtres détachées, zoom par salon (section `[fenetres]`, lot E2).
    pub windows: Box<super::windows::WindowPrefs>,
}

impl Default for LocalPrefs {
    fn default() -> Self {
        LocalPrefs { motion: Motion::System, intensity: Intensity::Full, saturation: 100, scroll_speed: 100, underline_links: false, role_colors: true, instant_tooltips: false, streamer: Streamer::default(), debug_log: false, voice: Default::default(), audio: Default::default(), video: Default::default(), update_manifest: String::new(), games: Vec::new(), recent_games: Vec::new(), mobile: Default::default(), plugins: Default::default(), identity: Default::default(), look: Default::default(), keywords: Default::default(), friend_groups: Default::default(), windows: Default::default() }
    }
}

/// Ce que le rendu des messages et des menus doit savoir, à chaque image.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Display {
    pub underline_links: bool,
    pub role_colors_off: bool,
    pub hide_personal: bool,
    pub hide_invites: bool,
    /// Point de la couleur du rôle devant le nom, au lieu du nom coloré (lot PERSO).
    pub name_dot: bool,
}

impl LocalPrefs {
    pub fn display(&self) -> Display {
        let s = &self.streamer;
        Display { underline_links: self.underline_links, role_colors_off: !self.role_colors, hide_personal: s.on && s.hide_personal, hide_invites: s.on && s.hide_invites, name_dot: self.look.name_color == super::look_prefs::NameColor::Dot }
    }

    /// Les sons sont coupés par le mode streamer.
    pub fn sounds_muted(&self) -> bool {
        self.streamer.on && self.streamer.mute_sounds
    }

    /// Les notifications de bureau sont coupées par le mode streamer.
    pub fn toasts_muted(&self) -> bool {
        self.streamer.on && self.streamer.mute_toasts
    }
}

const K_MOTION: &str = "reduire_animations";
const K_INTENSITY: &str = "intensite_animations";
const K_SATURATION: &str = "saturation";
const K_SCROLL: &str = "vitesse_defilement";
/// Bornes de la vitesse de défilement (%).
pub const SCROLL_MIN: u16 = 50;
pub const SCROLL_MAX: u16 = 300;
const K_UNDERLINE: &str = "souligner_liens";
const K_ROLES: &str = "couleur_roles";
const K_TOOLTIPS: &str = "infobulles_immediates";
const K_DEBUG: &str = "journal";
const K_OFFLINE: &str = "historique_hors_ligne";
const S_STREAMER: &str = "streamer";
const K_ON: &str = "actif";
const K_PERSONAL: &str = "masquer_infos_personnelles";
const K_INVITES: &str = "masquer_invitations";
const K_SOUNDS: &str = "couper_sons";
const K_TOASTS: &str = "couper_notifications";
const S_VOICE: &str = "vocal";
const S_PEOPLE: &str = "personnes";
const K_MUTE: &str = "muet";
const K_DEAF: &str = "sourdine";
const K_VOLUME: &str = "volume";
const S_UPDATES: &str = "mises_a_jour";
const K_MANIFEST: &str = "manifeste";

impl LocalPrefs {
    /// Texte de `local.toml`.
    pub fn to_toml(&self) -> String {
        let s = &self.streamer;
        format!(
            "# Réglages de cet ordinateur (Ferrocord) : ils ne partent pas chez Discord.\n\
             {K_MOTION} = \"{}\"\n{K_INTENSITY} = \"{}\"
{K_SATURATION} = {}\n{K_SCROLL} = {}\n{K_UNDERLINE} = {}\n{K_ROLES} = {}\n{K_TOOLTIPS} = {}\n{K_DEBUG} = {}\n\n\
             [{S_STREAMER}]\n{K_ON} = {}\n{K_PERSONAL} = {}\n{K_INVITES} = {}\n{K_SOUNDS} = {}\n{K_TOASTS} = {}\n",
            self.motion.key(),
            self.intensity.key(),
            self.saturation,
            self.scroll_speed,
            self.underline_links,
            self.role_colors,
            self.instant_tooltips,
            self.debug_log,
            s.on,
            s.hide_personal,
            s.hide_invites,
            s.mute_sounds,
            s.mute_toasts
        ) + &self.voice_toml()
            + "\n"
            + &self.audio.to_toml()
            + "\n"
            + &self.video.to_toml()
            + &format!("\n[{S_UPDATES}]\n# Adresse HTTPS du manifeste signé ; vide : pas de mise à jour automatique.\n{K_MANIFEST} = {}\n", toml_string(&self.update_manifest))
            + "\n"
            + &super::activity::games_toml(&self.games, &self.recent_games)
            + "\n"
            + &self.mobile.to_toml()
            + "\n"
            + &self.plugins.to_toml()
            + "\n"
            + &self.identity.to_toml()
            + "\n"
            + &self.look.to_toml()
            + "\n"
            + &self.keywords.to_toml()
            + "\n"
            + &self.friend_groups.to_toml()
            + "\n"
            + &self.windows.to_toml()
    }

    /// Section `[vocal]` (phase 4) : muet, sourdine, puis une ligne par
    /// personne réglée (`identifiant = { volume = 150, muet = false }`).
    fn voice_toml(&self) -> String {
        use std::fmt::Write as _;
        let v = &self.voice;
        let mut s = format!("\n[{S_VOICE}]\n{K_MUTE} = {}\n{K_DEAF} = {}\n", v.mute, v.deaf);
        if !v.users.is_empty() {
            let _ = writeln!(s, "\n[{S_VOICE}.{S_PEOPLE}]");
            for (id, u) in &v.users {
                let _ = writeln!(s, "{id} = {{ {K_VOLUME} = {}, {K_MUTE} = {} }}", u.volume, u.muted);
            }
        }
        s
    }

    /// Lit `local.toml`. Clé inconnue ou valeur illisible : ignorée (valeur
    /// par défaut). `Err` : syntaxe.
    pub fn parse(text: &str) -> Result<LocalPrefs, String> {
        use toml::de::{DeTable, DeValue};
        let root = DeTable::parse(text).map_err(|e| e.message().to_string())?;
        let mut p = LocalPrefs::default();
        // Section `[plugins]`, lue après le reste : elle l'emporte sur les anciennes clés.
        let mut plugins = None;
        let flag = |v: &DeValue, slot: &mut bool| {
            if let Some(b) = v.as_bool() {
                *slot = b;
            }
        };
        for (key, value) in root.get_ref().iter() {
            let v = value.get_ref();
            match key.get_ref().as_ref() {
                K_MOTION => {
                    if let Some(m) = v.as_str().and_then(Motion::of) {
                        p.motion = m;
                    }
                }
                K_INTENSITY => {
                    if let Some(m) = v.as_str().and_then(Intensity::of) {
                        p.intensity = m;
                    }
                }
                K_SATURATION => {
                    if let Some(i) = v.as_integer()
                        && let Ok(n) = i64::from_str_radix(i.as_str(), i.radix())
                    {
                        p.saturation = n.clamp(0, 100) as u8;
                    }
                }
                K_SCROLL => {
                    if let Some(i) = v.as_integer()
                        && let Ok(n) = i64::from_str_radix(i.as_str(), i.radix())
                    {
                        p.scroll_speed = n.clamp(i64::from(SCROLL_MIN), i64::from(SCROLL_MAX)) as u16;
                    }
                }
                K_UNDERLINE => flag(v, &mut p.underline_links),
                K_ROLES => flag(v, &mut p.role_colors),
                K_TOOLTIPS => flag(v, &mut p.instant_tooltips),
                K_DEBUG => flag(v, &mut p.debug_log),
                K_OFFLINE => {
                    if let Some(on) = v.as_bool() {
                        p.plugins.set(crate::plugins::Id::OfflineHistory, on);
                    }
                }
                S_STREAMER => {
                    let Some(t) = v.as_table() else { continue };
                    let s = &mut p.streamer;
                    for (k, v) in t.iter() {
                        let v = v.get_ref();
                        match k.get_ref().as_ref() {
                            K_ON => flag(v, &mut s.on),
                            K_PERSONAL => flag(v, &mut s.hide_personal),
                            K_INVITES => flag(v, &mut s.hide_invites),
                            K_SOUNDS => flag(v, &mut s.mute_sounds),
                            K_TOASTS => flag(v, &mut s.mute_toasts),
                            _ => {}
                        }
                    }
                }
                S_VOICE => {
                    let Some(t) = v.as_table() else { continue };
                    for (k, v) in t.iter() {
                        let v = v.get_ref();
                        match k.get_ref().as_ref() {
                            K_MUTE => flag(v, &mut p.voice.mute),
                            K_DEAF => flag(v, &mut p.voice.deaf),
                            S_PEOPLE => {
                                let Some(people) = v.as_table() else { continue };
                                for (id, u) in people.iter() {
                                    let (Ok(id), Some(u)) = (id.get_ref().parse::<u64>(), u.get_ref().as_table()) else { continue };
                                    let mut one = super::voice::UserVoice::default();
                                    for (k, v) in u.iter() {
                                        let v = v.get_ref();
                                        match k.get_ref().as_ref() {
                                            K_VOLUME => {
                                                if let Some(i) = v.as_integer()
                                                    && let Ok(n) = i64::from_str_radix(i.as_str(), i.radix())
                                                {
                                                    one.volume = n.clamp(0, 200) as u16;
                                                }
                                            }
                                            K_MUTE => flag(v, &mut one.muted),
                                            _ => {}
                                        }
                                    }
                                    p.voice.set_user(id, one);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                S_UPDATES => {
                    let Some(t) = v.as_table() else { continue };
                    for (k, v) in t.iter() {
                        if k.get_ref().as_ref() == K_MANIFEST
                            && let Some(s) = v.get_ref().as_str()
                        {
                            p.update_manifest = s.chars().filter(|c| !c.is_control()).collect();
                        }
                    }
                }
                super::audio_prefs::SECTION => {
                    if let Some(t) = v.as_table() {
                        p.audio = super::audio_prefs::AudioPrefs::from_table(t);
                    }
                }
                super::video_prefs::SECTION => {
                    if let Some(t) = v.as_table() {
                        p.video = Box::new(super::video_prefs::VideoPrefs::from_table(t));
                    }
                }
                super::activity::SECTION => {
                    if let Some(t) = v.as_table() {
                        p.games = super::activity::games_from_table(t);
                        p.recent_games = super::activity::recent_from_table(t);
                    }
                }
                super::mobile::SECTION => {
                    if let Some(t) = v.as_table() {
                        p.mobile = super::mobile::MobilePrefs::from_table(t);
                    }
                }
                super::rpc::SECTION => {
                    if let Some(t) = v.as_table() {
                        let on = super::rpc::rpc_from_table(t, p.plugins.on(crate::plugins::Id::RichPresence));
                        p.plugins.set(crate::plugins::Id::RichPresence, on);
                    }
                }
                crate::plugins::SECTION => plugins = v.as_table(),
                super::look_prefs::SECTION => {
                    if let Some(t) = v.as_table() {
                        p.look = Box::new(super::look_prefs::LookPrefs::from_table(t));
                    }
                }
                super::keywords::SECTION => {
                    if let Some(t) = v.as_table() {
                        p.keywords = Box::new(super::keywords::KeywordPrefs::from_table(t));
                    }
                }
                super::friend_groups::SECTION => {
                    if let Some(t) = v.as_table() {
                        p.friend_groups = Box::new(super::friend_groups::FriendGroups::from_table(t));
                    }
                }
                super::windows::SECTION => {
                    if let Some(t) = v.as_table() {
                        p.windows = Box::new(super::windows::WindowPrefs::from_table(t));
                    }
                }
                super::identity_prefs::SECTION => {
                    if let Some(t) = v.as_table() {
                        p.identity = super::identity_prefs::IdentityPrefs::from_table(t);
                    }
                }
                _ => {}
            }
        }
        if let Some(t) = plugins {
            p.plugins.read_table(t);
        }
        Ok(p)
    }
}

/// Chaîne TOML entre guillemets (contrôles retirés).
fn toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars().filter(|c| !c.is_control()) {
        if matches!(c, '"' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// Adresse d'invitation Discord (masquée en mode streamer).
pub fn is_invite(url: &str) -> bool {
    let rest = url.trim_start_matches("https://").trim_start_matches("http://");
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let lower = rest.to_ascii_lowercase();
    ["discord.gg/", "discord.com/invite/", "discordapp.com/invite/", "discord.me/"].iter().any(|p| lower.starts_with(p) && lower.len() > p.len())
}

/// Ligne du journal local : horodatage, texte sur une seule ligne.
pub fn log_line(now_ms: i64, text: &str) -> String {
    let when = chrono::DateTime::from_timestamp_millis(now_ms).map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)).unwrap_or_default();
    let flat: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    format!("{when} {flat}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_round_trip_and_damaged_values() {
        let mut p = LocalPrefs { motion: Motion::On, intensity: Intensity::Subtle, saturation: 40, scroll_speed: 250, underline_links: true, role_colors: false, instant_tooltips: true, debug_log: true, update_manifest: "https://exemple.invalid/m\"a\\j.json".into(), ..Default::default() };
        p.streamer.on = true;
        p.streamer.mute_sounds = false;
        assert_eq!(LocalPrefs::parse(&p.to_toml()), Ok(p));
        assert_eq!(LocalPrefs::parse(""), Ok(LocalPrefs::default()));
        let odd = "saturation = 900\nreduire_animations = \"peut-etre\"\nsouligner_liens = 3\ninconnu = 1\n[streamer]\nactif = true\n";
        let got = LocalPrefs::parse(odd).unwrap();
        assert_eq!((got.saturation, got.motion, got.underline_links, got.streamer.on), (100, Motion::System, false, true));
        assert!(LocalPrefs::parse("saturation = ").is_err());
    }

    /// Plugins (lot P0) : la section `[plugins]` fait l'aller-retour ; les
    /// anciennes clés d'un fichier d'avant sont reprises, et la section l'emporte.
    #[test]
    fn plugins_round_trip_and_old_keys_are_migrated() {
        use crate::plugins::Id;
        let mut p = LocalPrefs::default();
        p.plugins.set(Id::Animations, false);
        p.plugins.put(Id::ReadAloud, crate::plugins::READ_ALOUD_AUTHOR, crate::plugins::Value::Bool(false));
        let text = p.to_toml();
        assert_eq!(LocalPrefs::parse(&text), Ok(p), "{text}");
        let old = LocalPrefs::parse("historique_hors_ligne = false
[rich_presence]
actif = false
").unwrap();
        assert!(!old.plugins.on(Id::OfflineHistory) && !old.plugins.on(Id::RichPresence) && old.plugins.on(Id::Backdrop));
        let both = LocalPrefs::parse("historique_hors_ligne = false
[plugins]
historique_hors_ligne = true
rich_presence = false
[rich_presence]
actif = true
").unwrap();
        assert!(both.plugins.on(Id::OfflineHistory) && !both.plugins.on(Id::RichPresence), "la section [plugins] l'emporte");
    }

    #[test]
    fn voice_section_round_trips_and_ignores_garbage() {
        let mut p = LocalPrefs::default();
        p.voice.mute = true;
        p.voice.set_user(1000000000000000002, crate::state::voice::UserVoice { volume: 150, muted: false });
        p.voice.set_user(1000000000000000003, crate::state::voice::UserVoice { volume: 100, muted: true });
        let text = p.to_toml();
        assert_eq!(LocalPrefs::parse(&text), Ok(p.clone()), "{text}");
        let odd = "[vocal]\nmuet = 3\nsourdine = true\n[vocal.personnes]\nabc = { volume = 10 }\n42 = { volume = 900, muet = true }\n43 = 7\n";
        let got = LocalPrefs::parse(odd).unwrap();
        assert!(!got.voice.mute && got.voice.deaf);
        assert_eq!(got.voice.users.len(), 1);
        assert_eq!(got.voice.user(42), crate::state::voice::UserVoice { volume: 200, muted: true });
    }

    #[test]
    fn streamer_masks_only_when_on() {
        let mut p = LocalPrefs::default();
        assert_eq!(p.display(), Display::default());
        assert!(!p.sounds_muted() && !p.toasts_muted());
        p.streamer.on = true;
        let d = p.display();
        assert!(d.hide_personal && d.hide_invites && p.sounds_muted() && p.toasts_muted());
        p.streamer.hide_invites = false;
        assert!(!p.display().hide_invites);
    }

    #[test]
    fn motion_follows_windows_only_in_system_mode() {
        assert!(Motion::System.reduced(Some(true)));
        assert!(!Motion::System.reduced(None));
        assert!(Motion::On.reduced(Some(false)));
        assert!(!Motion::Off.reduced(Some(true)));
    }

    #[test]
    fn invites_are_recognised() {
        for u in ["https://discord.gg/abc", "http://www.discord.com/invite/xyz", "https://DISCORD.GG/Abc", "https://discordapp.com/invite/q"] {
            assert!(is_invite(u), "{u}");
        }
        for u in ["https://discord.gg/", "https://example.com/discord.gg/abc", "https://discord.com/channels/1/2"] {
            assert!(!is_invite(u), "{u}");
        }
    }

    #[test]
    fn log_lines_stay_on_one_line() {
        assert_eq!(log_line(0, "a\nb\tc"), "1970-01-01T00:00:00.000Z a b c\n");
    }
}
