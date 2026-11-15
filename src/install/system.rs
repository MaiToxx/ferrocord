//! Démarrage avec Windows (clé `Run`) et protocole `discord://` de
//! l'utilisateur (`Software\Classes\discord`), sur un [`Hive`].
//!
//! Tout est réversible : le démarrage retire sa valeur ; le protocole garde
//! l'ancien gestionnaire (souvent l'application officielle) dans
//! `Software\Ferrocord` et le remet en place quand on le désactive.

use super::hive::Hive;
use std::io;
use std::path::Path;

pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const RUN_VALUE: &str = "Ferrocord";
/// Argument du lancement « réduit dans la zone de notification ».
pub const MINIMIZED_ARG: &str = "--reduit";
pub const PROTOCOL_KEY: &str = r"Software\Classes\discord";
const COMMAND_KEY: &str = r"Software\Classes\discord\shell\open\command";
const ICON_KEY: &str = r"Software\Classes\discord\DefaultIcon";
/// Ce que Ferrocord a remplacé, pour le rendre.
pub const BACKUP_KEY: &str = r"Software\Ferrocord\Protocole";

/// Démarrage avec Windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Autostart {
    Off,
    On { minimized: bool },
}

/// À qui est `discord://`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Protocol {
    /// Personne.
    Free,
    /// À Ferrocord (cet exécutable).
    Ours,
    /// À un autre programme (commande enregistrée).
    Other(String),
}

fn quoted(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// Ligne de commande de la clé `Run`.
pub fn run_command(exe: &Path, minimized: bool) -> String {
    if minimized { format!("{} {MINIMIZED_ARG}", quoted(exe)) } else { quoted(exe) }
}

pub fn autostart(h: &dyn Hive) -> Autostart {
    match h.get(RUN_KEY, RUN_VALUE) {
        None => Autostart::Off,
        Some(cmd) => Autostart::On { minimized: cmd.split_whitespace().any(|a| a == MINIMIZED_ARG) },
    }
}

pub fn set_autostart(h: &mut dyn Hive, exe: &Path, want: Autostart) -> io::Result<()> {
    match want {
        Autostart::Off => h.remove_value(RUN_KEY, RUN_VALUE),
        Autostart::On { minimized } => h.set(RUN_KEY, RUN_VALUE, &run_command(exe, minimized)),
    }
}

/// Commande du protocole pointant vers `exe`.
pub fn protocol_command(exe: &Path) -> String {
    format!("{} \"%1\"", quoted(exe))
}

fn is_ours(command: &str, exe: &Path) -> bool {
    command.to_lowercase().starts_with(&quoted(exe).to_lowercase())
}

pub fn protocol(h: &dyn Hive, exe: &Path) -> Protocol {
    match h.get(COMMAND_KEY, "") {
        None => Protocol::Free,
        Some(c) if is_ours(&c, exe) => Protocol::Ours,
        Some(c) => Protocol::Other(c),
    }
}

/// `discord://` pour Ferrocord ; l'ancien gestionnaire est gardé.
pub fn register_protocol(h: &mut dyn Hive, exe: &Path) -> io::Result<()> {
    if protocol(h, exe) == Protocol::Ours {
        return Ok(());
    }
    if let Some(old) = h.get(COMMAND_KEY, "") {
        h.set(BACKUP_KEY, "commande", &old)?;
        for (key, name, slot) in [(PROTOCOL_KEY, "", "description"), (ICON_KEY, "", "icone")] {
            if let Some(v) = h.get(key, name) {
                h.set(BACKUP_KEY, slot, &v)?;
            }
        }
    }
    h.set(PROTOCOL_KEY, "", "URL:Discord Protocol")?;
    h.set(PROTOCOL_KEY, "URL Protocol", "")?;
    h.set(ICON_KEY, "", &format!("{},0", quoted(exe)))?;
    h.set(COMMAND_KEY, "", &protocol_command(exe))
}

/// Rend `discord://` à son ancien gestionnaire, ou le retire. Rien n'est
/// touché s'il n'est pas (ou plus) à Ferrocord.
pub fn unregister_protocol(h: &mut dyn Hive, exe: &Path) -> io::Result<()> {
    if protocol(h, exe) != Protocol::Ours {
        return Ok(());
    }
    match h.get(BACKUP_KEY, "commande") {
        Some(old) => {
            h.set(COMMAND_KEY, "", &old)?;
            match h.get(BACKUP_KEY, "icone") {
                Some(icon) => h.set(ICON_KEY, "", &icon)?,
                None => h.remove_tree(ICON_KEY)?,
            }
            if let Some(d) = h.get(BACKUP_KEY, "description") {
                h.set(PROTOCOL_KEY, "", &d)?;
            }
        }
        None => h.remove_tree(PROTOCOL_KEY)?,
    }
    h.remove_tree(BACKUP_KEY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::hive::MemoryHive;
    use std::path::PathBuf;

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Users\x\AppData\Local\Programs\Ferrocord\ferrocord.exe")
    }

    #[test]
    fn autostart_round_trip() {
        let mut h = MemoryHive::default();
        assert_eq!(autostart(&h), Autostart::Off);
        set_autostart(&mut h, &exe(), Autostart::On { minimized: true }).unwrap();
        assert_eq!(h.get(RUN_KEY, RUN_VALUE).unwrap(), format!("\"{}\" --reduit", exe().display()));
        assert_eq!(autostart(&h), Autostart::On { minimized: true });
        set_autostart(&mut h, &exe(), Autostart::On { minimized: false }).unwrap();
        assert_eq!(autostart(&h), Autostart::On { minimized: false });
        set_autostart(&mut h, &exe(), Autostart::Off).unwrap();
        assert_eq!(autostart(&h), Autostart::Off);
        assert!(h.is_empty());
    }

    #[test]
    fn protocol_on_a_clean_profile_is_removed_entirely() {
        let mut h = MemoryHive::default();
        assert_eq!(protocol(&h, &exe()), Protocol::Free);
        register_protocol(&mut h, &exe()).unwrap();
        assert_eq!(protocol(&h, &exe()), Protocol::Ours);
        assert_eq!(h.get(COMMAND_KEY, "").unwrap(), format!("\"{}\" \"%1\"", exe().display()));
        assert_eq!(h.get(PROTOCOL_KEY, "URL Protocol").as_deref(), Some(""));
        unregister_protocol(&mut h, &exe()).unwrap();
        assert_eq!(protocol(&h, &exe()), Protocol::Free);
        assert!(h.is_empty(), "{h:?}");
    }

    #[test]
    fn official_app_gets_its_protocol_back() {
        let mut h = MemoryHive::default();
        let official = r#""C:\Users\x\AppData\Local\Discord\Update.exe" --processStart Discord.exe --process-start-args "%1""#;
        h.set(PROTOCOL_KEY, "", "URL:Discord Protocol").unwrap();
        h.set(PROTOCOL_KEY, "URL Protocol", "").unwrap();
        h.set(COMMAND_KEY, "", official).unwrap();
        h.set(ICON_KEY, "", "Discord.exe,0").unwrap();
        let before = h.clone();
        assert_eq!(protocol(&h, &exe()), Protocol::Other(official.into()));
        register_protocol(&mut h, &exe()).unwrap();
        assert_eq!(protocol(&h, &exe()), Protocol::Ours);
        // Deuxième enregistrement : la sauvegarde n'est pas écrasée par nos valeurs.
        register_protocol(&mut h, &exe()).unwrap();
        unregister_protocol(&mut h, &exe()).unwrap();
        assert_eq!(protocol(&h, &exe()), Protocol::Other(official.into()));
        assert_eq!(h.get(ICON_KEY, "").as_deref(), Some("Discord.exe,0"));
        assert_eq!(format!("{h:?}"), format!("{before:?}"), "état d'origine rendu");
    }

    #[test]
    fn someone_elses_protocol_is_never_removed() {
        let mut h = MemoryHive::default();
        h.set(COMMAND_KEY, "", "\"D:\\autre.exe\" \"%1\"").unwrap();
        unregister_protocol(&mut h, &exe()).unwrap();
        assert_eq!(h.len(), 1);
    }
}
