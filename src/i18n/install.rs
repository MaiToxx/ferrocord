//! Textes de l'installation, des liens, des mises à jour et des licences
//! (lot C6). Module enfant de `table` (il en reprend les macros), tenu à part
//! pour la fusion.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        SEC_STARTUP = "Démarrage", "Startup";
        AUTOSTART = "Démarrer avec Windows", "Open Ferrocord at startup";
        AUTOSTART_TIP = "Ferrocord s'ouvre à l'ouverture de votre session Windows.", "Ferrocord opens when you sign in to Windows.";
        MINIMIZED = "Démarrer réduit", "Start minimized";
        MINIMIZED_TIP = "Au démarrage de Windows, Ferrocord reste dans la zone de notification (ou réduit dans la barre des tâches si l'icône est désactivée).", "When Windows starts, Ferrocord stays in the notification area (or minimized to the taskbar if the icon is turned off).";
        SEC_LINKS = "Liens Discord", "Discord links";
        PROTOCOL = "Ouvrir les liens Discord dans Ferrocord", "Open Discord links in Ferrocord";
        PROTOCOL_TIP = "Liens du protocole discord des pages web et des autres programmes, pour votre compte Windows seulement. Décocher rend ces liens au programme qui les ouvrait avant (par exemple l'application officielle).", "Links using the discord protocol from web pages and other programs, for your Windows account only. Unchecking gives them back to the program that opened them before (for example the official app).";
        SEC_UPDATES = "Mises à jour", "Updates";
        UPDATES_TIP = "Signées par l'auteur de Ferrocord et vérifiées avant l'installation. Rien ne se télécharge ni ne s'installe sans votre clic.", "Signed by the author of Ferrocord and checked before installing. Nothing is downloaded or installed without your click.";
        NO_SOURCE = "Aucune source de mises à jour. Pour en ajouter une, écrivez l'adresse du manifeste dans local.toml (section [mises_a_jour], clé manifeste).", "No update source. To add one, write the manifest address in local.toml (section [mises_a_jour], key manifeste).";
        AUTO_CHECK = "Vérifier automatiquement", "Check automatically";
        AUTO_CHECK_TIP = "Au lancement puis une fois par jour au plus. La requête va seulement à l'adresse ci-dessous, sans jeton ni identifiant.", "At launch, then once a day at most. The request only goes to the address below, with no token or identifier.";
        IDLE = "Pas encore vérifié depuis le lancement.", "Not checked yet since launch.";
        NO_KEY = "Cet exécutable n'a pas de clé de signature intégrée\u{a0}: mises à jour automatiques désactivées.", "This build has no embedded signing key: automatic updates are off.";
        CHECKING = "Recherche de mises à jour…", "Checking for updates…";
        UP_TO_DATE = "Ferrocord est à jour.", "Ferrocord is up to date.";
        UNKNOWN = "État inconnu.", "Unknown state.";
        CHECK_NOW = "Rechercher maintenant", "Check now";
        DOWNLOAD = "Télécharger et vérifier", "Download and verify";
        RESTART = "Redémarrer pour mettre à jour", "Restart to update";
        READY_TIP = "Elle s'installe au prochain lancement. Si elle ne démarre pas, la version actuelle revient d'elle-même.", "It installs on next launch. If it fails to start, the current version comes back by itself.";
        SOURCE = "Source", "Source";
        SOURCE_TIP = "Adresse du manifeste signé\u{a0}: celle de local.toml, sinon celle intégrée à cet exécutable.", "Address of the signed manifest: the one in local.toml, otherwise the one built into this executable.";
        LICENSE_FILTER = "Filtrer les paquets", "Filter packages";
        LICENSE_PICK = "Choisissez un paquet pour lire le texte complet de sa licence.", "Pick a package to read the full text of its license.";
        LINK_NOT_FOUND = "Ce lien mène à un salon ou à un serveur inaccessible depuis ce compte.", "This link leads to a channel or server this account can't access.";
        SETUP_TITLE = "Installer Ferrocord", "Install Ferrocord";
        SETUP_DESKTOP = "Créer un raccourci sur le bureau", "Create a desktop shortcut";
        SETUP_INSTALL = "Installer", "Install";
        SETUP_PORTABLE = "Lancer sans installer", "Run without installing";
        SETUP_RUNNING = "Ferrocord est ouvert\u{a0}: quittez-le (menu de l'icône, Quitter), puis recommencez.", "Ferrocord is open: quit it (icon menu, Quit), then try again.";
        UNINSTALL_TITLE = "Désinstaller Ferrocord", "Uninstall Ferrocord";
        UNINSTALL_BODY = "Ferrocord, ses raccourcis et ses entrées du registre seront retirés. Vos réglages et votre cache sont gardés, sauf si vous cochez la case.", "Ferrocord, its shortcuts and its registry entries will be removed. Your settings and cache are kept unless you tick the box.";
        UNINSTALL_WIPE = "Supprimer aussi mes réglages et le cache", "Also delete my settings and the cache";
        UNINSTALL_TOKENS = "Les jetons enregistrés restent dans le Gestionnaire d'identifiants de Windows\u{a0}: déconnectez-vous dans Ferrocord avant de désinstaller pour les retirer.", "Saved tokens stay in the Windows Credential Manager: log out in Ferrocord before uninstalling to remove them.";
        UNINSTALL_GO = "Désinstaller", "Uninstall";
        UNINSTALL_CANCEL = "Annuler", "Cancel";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        available(version) = "Version {version} disponible.", "Version {version} is available.";
        downloading(percent) = "Téléchargement… {percent}\u{a0}%", "Downloading… {percent}%";
        ready(version) = "Version {version} téléchargée et vérifiée.", "Version {version} downloaded and verified.";
        failed(why) = "Mise à jour impossible\u{a0}: {why}", "Update failed: {why}";
        refused(version) = "La version {version} n'a pas démarré\u{a0}: Ferrocord est revenu à la précédente et ne la proposera plus.", "Version {version} failed to start: Ferrocord went back to the previous one and won't offer it again.";
        other_handler(program) = "Aujourd'hui ouverts par\u{a0}: {program}", "Currently opened by: {program}";
        registry_failed(why) = "Registre de Windows\u{a0}: {why}", "Windows registry: {why}";
        licenses_failed(why) = "Licences illisibles\u{a0}: {why}", "Licenses unreadable: {why}";
        packages(count) = "{count} paquets (dépendances de l'exécutable, bibliothèques C comprises)", "{count} packages (dependencies built into the executable, C libraries included)";
        update_toast(version) = "Ferrocord {version} est disponible (Paramètres, Ferrocord).", "Ferrocord {version} is available (Settings, Ferrocord).";
        setup_body(folder) = "Ferrocord sera copié dans {folder}, avec un raccourci dans le menu Démarrer et une entrée dans les applications installées. Aucun droit d'administrateur n'est demandé.", "Ferrocord will be copied to {folder}, with a Start menu shortcut and an entry in installed apps. No administrator rights are needed.";
        setup_failed(why) = "Impossible\u{a0}: {why}", "Failed: {why}";
    }
}
