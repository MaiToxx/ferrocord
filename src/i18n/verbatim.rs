//! Textes des comptes, du coffre, de la migration, des réglages et des
//! fichiers surveillés (`theme.toml`, `settings.toml`). Ce sont des textes de
//! sécurité relus mot à mot : leur français est gardé exactement tel qu'il
//! était écrit (espace ordinaire avant « : », « ; » et « ? », guillemets
//! « » avec espaces ordinaires), et des tests de bout en bout en vérifient
//! certains. Le test de ponctuation française ne les vérifie donc pas ;
//! l'anglais, lui, suit les mêmes règles que le reste de la table.
//!
//! Les macros `texts!` et `templates!` sont celles de `table.rs`.

/// Textes constants.
pub mod t {
    use crate::i18n::Text;

    texts! {
        // Écran de connexion.
        STILL_RUNNING_MINUTE = "Toujours en cours (plus d'une minute)", "Still in progress (over a minute)";
        OPENING_ACCOUNTS = "Ouverture des comptes enregistrés…", "Opening saved accounts…";
        TAGLINE = "Client Discord natif et léger", "Lightweight native Discord client";
        UNOFFICIAL_WARNING = "Attention, client non officiel : l'utiliser avec un compte utilisateur enfreint les conditions d'utilisation de Discord et peut entraîner la suspension du compte.",
            "Warning, unofficial client: using it with a user account violates Discord's Terms of Service and may get the account suspended.";
        CANCEL = "Annuler", "Cancel";
        SAVED_ACCOUNTS = "COMPTES ENREGISTRÉS", "SAVED ACCOUNTS";
        ENV_SESSION = "Session éphémère, jamais enregistrée", "Temporary session, never saved";
        ADD_ACCOUNT_HEADING = "AJOUTER UN COMPTE", "ADD AN ACCOUNT";
        TOKEN_HEADING = "JETON DISCORD", "DISCORD TOKEN";
        TOKEN_HINT = "Collez votre jeton ici", "Paste your token here";
        REMEMBER_ME = "Se souvenir de moi", "Remember me";
        SESSION_ONLY = "Ce jeton ne servira que pour cette session", "This token will only be used for this session";
        NO_VAULT_SESSION_ONLY = "Aucun coffre sur ce système : le jeton ne servira que pour cette session", "No vault on this system: the token will only be used for this session";
        LOG_IN = "Connexion", "Log in";
        SAME_ACCOUNT_TWICE = "Même compte enregistré deux fois", "Same account saved twice";
        REMOVE_THIS_ACCOUNT_ASK = "Retirer ce compte ?", "Remove this account?";
        FROM_OLD_FILE = "Jeton repris de l'ancien fichier", "Token taken from the old file";
        OPEN_THIS_ACCOUNT = "Ouvrir ce compte", "Open this account";
        YES = "Oui", "Yes";
        NO = "Non", "No";
        REMOVE_ACCOUNT_TIP = "Retirer ce compte (son jeton est effacé du coffre)", "Remove this account (its token is erased from the vault)";
        DELETING = "Suppression en cours…", "Deleting…";

        // Comptes : avertissement durable du fichier de jeton en clair.
        FILE_REMAINS = "un fichier de jeton en clair existe encore : le compte importé est retiré du coffre, mais ce fichier n'est jamais supprimé sans votre accord",
            "a plaintext token file still exists: the imported account is removed from the vault, but this file is never deleted without your consent";
        STALE_WIPE = "Le fichier de jeton en clair n'a pas été supprimé : le fichier ou le coffre a changé entre-temps. Relisez la question et confirmez de nouveau.",
            "The plaintext token file was not deleted: the file or the vault changed in the meantime. Reread the question and confirm again.";
        AFTER_READY = "le fichier en clair sera supprimé à la prochaine connexion du compte importé, une fois la copie vérifiée",
            "the plaintext file will be deleted the next time the imported account connects, once the copy is verified";
        DECLINED = "un fichier de jeton en clair existe encore : vous avez retiré le compte importé, ce fichier n'est plus importé tout seul",
            "a plaintext token file still exists: you removed the imported account, so this file is no longer imported automatically";
        STEP_TWO_RUNNING = "Le fichier de jeton en clair est en train d'être supprimé après vérification : le compte importé n'est pas retiré. Réessayez dans un instant.",
            "The plaintext token file is being deleted after verification: the imported account is not removed. Try again in a moment.";
        WIPE_GONE = "Le fichier de jeton en clair n'existe plus : rien à supprimer.", "The plaintext token file no longer exists: nothing to delete.";
        COPY_AGE_UNKNOWN = " On ne sait pas quand la copie dans le coffre a été écrite ; si c'était il y a moins de 24 h, elle pourrait ne pas être conservée en cas de coupure de courant. Il est plus sûr d'attendre 24 h.",
            " It is not known when the copy in the vault was written; if it was less than 24 hours ago, it might not survive a power outage. It is safer to wait 24 hours.";
        /// Format `chrono` d'une date locale (jour et heure).
        LOCAL_DATE_FORMAT = "%d/%m/%Y à %H:%M", "%m/%d/%Y at %H:%M";
        IN_24_HOURS = "dans 24 h", "in 24 hours";
        FILE_HAS_OTHER_TOKEN = " Le fichier de jeton en clair contient un autre jeton : « Essayer le jeton du fichier ».",
            " The plaintext token file contains another token: “Try the file's token”.";
        ALREADY_DELETED = "Le fichier de jeton en clair a déjà été supprimé ailleurs.", "The plaintext token file was already deleted elsewhere.";

        // Comptes : remplacement d'un jeton enregistré.
        SAVED_TOKEN_CHANGED = "Le jeton enregistré a changé entre-temps : rien n'est remplacé. Relisez la question et confirmez de nouveau.",
            "The saved token changed in the meantime: nothing is replaced. Reread the question and confirm again.";
        OCCUPIED_TYPED = "Ce compte est déjà enregistré avec un autre jeton : rien n'est écrit sans votre accord (sélecteur de compte).",
            "This account is already saved with another token: nothing is written without your consent (account switcher).";
        OCCUPIED_FILE = "Jeton du fichier accepté par Discord : rien n'est écrit dans le coffre sans votre accord (sélecteur de compte).",
            "File token accepted by Discord: nothing is written to the vault without your consent (account switcher).";
        OCCUPIED_RECOPY = "L'entrée du coffre a changé depuis l'échec de l'écriture : rien n'est recopié sans votre accord.",
            "The vault entry changed since the failed write: nothing is copied back without your consent.";
        REPLACE_TYPED = "Ce compte est déjà enregistré. Remplacer le jeton enregistré par celui que vous venez de saisir ? L'ancien sera effacé du coffre.",
            "This account is already saved. Replace the saved token with the one you just entered? The old one will be erased from the vault.";
        REPLACE_FILE_REFUSED = "Discord a refusé le jeton enregistré et accepté celui du fichier en clair. Remplacer le jeton enregistré par celui du fichier ? L'ancien sera effacé du coffre.",
            "Discord refused the saved token and accepted the one from the plaintext file. Replace the saved token with the file's? The old one will be erased from the vault.";
        REPLACE_FILE = "Ce compte est déjà enregistré avec un autre jeton. Remplacer le jeton enregistré par celui du fichier de jeton en clair ? L'ancien sera effacé du coffre.",
            "This account is already saved with another token. Replace the saved token with the one from the plaintext token file? The old one will be erased from the vault.";
        DIFFERENT_TOKEN = "Jeton différent d'un compte enregistré", "Different token for a saved account";

        // Comptes : noms, lignes et sélecteur.
        IMPORTED_ACCOUNT = "Compte importé", "Imported account";
        IMPORTED_TIP = "Jeton repris de l'ancien fichier, pas encore vérifié : le nom du compte apparaîtra après la première connexion.",
            "Token taken from the old file, not yet verified: the account name will appear after the first connection.";
        NEW_ACCOUNT = "nouveau compte", "new account";
        PLAINTEXT_FILE = "fichier de jeton en clair", "plaintext token file";
        REMOVING = "Retrait en cours…", "Removing…";
        DELETING_PLAIN_FILE = "Suppression du fichier en clair en cours…", "Deleting the plaintext file…";
        SAVING = "Enregistrement en cours…", "Saving…";
        EMPTIED_FILE_HEADING = "Fichier vidé sur le disque (sans jeton)", "Emptied file on disk (no token)";
        PLAIN_FILE_HEADING = "Fichier de jeton en clair sur le disque", "Plaintext token file on disk";
        VERIFY_THEN_DELETE = "Vérification puis suppression en cours…", "Verifying, then deleting…";
        IMPORT_TIP = "Le jeton du fichier est copié dans le coffre ; le fichier reste jusqu'à un lancement suivant", "The file's token is copied to the vault; the file remains until a later launch";
        WIPE_TIP = "Le fichier est écrasé par des zéros, puis supprimé", "The file is overwritten with zeros, then deleted";
        TWIN_TIP = "L'autre ligne garde ce compte", "The other row keeps this account";
        PREVIOUS_KEPT = "Le jeton d'avant est gardé en mémoire jusqu'à la fermeture de l'application.", "The previous token is kept in memory until the application closes.";
        WRITING = "Écriture en cours…", "Writing…";
        TRY_FILE_TOKEN = "Essayer le jeton du fichier", "Try the file's token";
        TRY_FILE_TOKEN_DETAIL = "Jeton du fichier en clair, différent de celui du coffre", "Token from the plaintext file, different from the vault's";
        FILE_TOKEN = "Jeton du fichier en clair", "Token from the plaintext file";
        FILE_TOKEN_DETAIL = "Pour cette session ; le fichier n'est pas touché", "For this session; the file is not touched";
        ACCOUNTS_HEADING = "COMPTES", "ACCOUNTS";
        IMPORTED_TWICE = "Importé de l'ancien fichier, en double", "Imported from the old file, duplicate";
        ACCOUNT_OPEN = "Compte ouvert", "Current account";
        ADD_ACCOUNT = "Ajouter un compte", "Add an account";
        CLOSE_SESSION = "Fermer la session", "Close the session";
        ENV_NEVER_SAVED = "DISCORD_TOKEN n'est jamais enregistré : rien n'est effacé", "DISCORD_TOKEN is never saved: nothing is erased";
        CONFIRMED_REPLACE_GOES_ON = "Le remplacement confirmé du jeton enregistré va au bout ; seule la session est fermée", "The confirmed replacement of the saved token completes; only the session is closed";
        TOKEN_NOT_SAVED = "Ce jeton n'est pas enregistré (et ne le sera pas)", "This token is not saved (and will not be)";
        FILE_NOT_TOUCHED = "Le fichier de jeton en clair n'est pas touché", "The plaintext token file is not touched";
        LOG_OUT_ACCOUNT = "Se déconnecter de ce compte", "Log out of this account";
        LOGOUT_IMPORTED_TIP = "Le compte est retiré de la liste et son jeton effacé du coffre ; le fichier de jeton en clair, s'il existe encore, n'est pas touché",
            "The account is removed from the list and its token erased from the vault; the plaintext token file, if it still exists, is not touched";
        LOGOUT_TIP = "Le compte est retiré de la liste et son jeton effacé du coffre", "The account is removed from the list and its token erased from the vault";
        WIPE_LABEL = "Supprimer le fichier de jeton en clair", "Delete the plaintext token file";
        WIPE_QUESTION = "Supprimer le fichier de jeton en clair ?", "Delete the plaintext token file?";
        IMPORT_LABEL = "Importer", "Import";
        RECOPY_LABEL = "Recopier le jeton d'avant", "Copy back the previous token";
        CHECKING_VAULT = "Vérification du coffre…", "Checking the vault…";

        // Fil des comptes.
        VAULT_DID_NOT_RETURN = "le coffre n'a pas rendu le jeton écrit", "the vault did not return the written token";
        REREAD_MISMATCH = "la relecture ne rend pas la valeur d'avant", "reading back does not return the previous value";
        LOCK_BUSY = "une autre instance de Ferrocord utilise le coffre en ce moment", "another instance of Ferrocord is using the vault right now";

        // Jeton.
        TOKEN_EMPTY = "jeton vide", "empty token";
        TOKEN_CONTROL = "le jeton contient un caractère de contrôle", "the token contains a control character";
        TOKEN_SPACES = "le jeton contient des espaces", "the token contains spaces";
        TOKEN_ODD_CHAR = "le jeton contient un caractère qu'un jeton Discord n'a jamais (caractère invisible ou accentué)",
            "the token contains a character a Discord token never has (invisible or accented character)";

        // Coffre.
        DEMO_VAULT = "coffre de démonstration (en mémoire)", "demo vault (in memory)";
        INDETERMINATE = "le coffre n'a pas répondu à temps plus tôt : état inconnu, plus rien n'y est fait avant le prochain lancement",
            "the vault did not respond in time earlier: unknown state, nothing more is done with it until the next launch";
        VAULT_THREAD_FAILED = "fil du coffre impossible à lancer", "could not start the vault thread";
        VAULT_THREAD_STOPPED = "le fil du coffre s'est arrêté", "the vault thread has stopped";
        VAULT_REPLY_LOST = "réponse du coffre perdue", "vault reply lost";
        CREDMAN_UNREACHABLE = "Gestionnaire d'identifiants inaccessible", "Credential Manager inaccessible";
        ENTRY_NOT_FOUND = "entrée introuvable", "entry not found";
        ENTRY_UNREADABLE = "entrée illisible", "unreadable entry";
        TOKEN_TOO_LONG = "jeton trop long pour le Gestionnaire d'identifiants", "token too long for the Credential Manager";
        ENTRY_REJECTED = "entrée refusée par le Gestionnaire d'identifiants", "entry rejected by the Credential Manager";
        ENTRY_AMBIGUOUS = "plusieurs entrées portent ce nom", "several entries have this name";
        OPERATION_REFUSED = "opération refusée par le Gestionnaire d'identifiants", "operation refused by the Credential Manager";
        WINDOWS_VAULT = "Gestionnaire d'identifiants de Windows", "Windows Credential Manager";
        NO_VAULT = "aucun coffre pris en charge sur ce système", "no supported vault on this system";
        // Android (phase 7) : clé du Keystore, jetons chiffrés dans le stockage privé.
        ANDROID_VAULT = "Keystore d'Android", "Android Keystore";
        KEYSTORE_UNREACHABLE = "Keystore d'Android inaccessible", "Android Keystore inaccessible";
        KEYSTORE_FILE_FAILED = "fichier du coffre illisible ou impossible à écrire", "vault file unreadable or cannot be written";

        // Migration de l'ancien fichier.
        UNRECOGNIZED_CONTENT = "contenu non reconnu", "unrecognized content";
        NOT_A_FILE = "ce n'est pas un fichier", "this is not a file";
        FILE_TOO_BIG = "fichier trop gros pour contenir un jeton", "file too big to contain a token";
        NOT_UTF8 = "ce n'est pas du texte UTF-8", "this is not UTF-8 text";
        IS_LINK = "c'est un lien vers un autre fichier", "this is a link to another file";
        NOT_ORDINARY = "ce n'est pas un fichier ordinaire", "this is not a regular file";
        HARD_LINKS = "ce fichier a plusieurs noms (liens physiques)", "this file has several names (hard links)";
        PATH_CHANGED = "le chemin désigne maintenant un autre fichier (renommé ou remplacé) : il n'est pas supprimé",
            "the path now points to another file (renamed or replaced): it is not deleted";
        KEPT_FOR_SESSION = " ; l'ancien fichier est gardé tel quel et son jeton reste utilisable pour cette session ; nouvel essai au lancement suivant",
            "; the old file is kept as is and its token remains usable for this session; will retry at the next launch";
        VAULT_HAS_OTHER = "le coffre contient déjà un autre jeton pour ce compte, différent de celui du fichier : l'ancien fichier n'a pas été migré par-dessus",
            "the vault already contains another token for this account, different from the file's: the old file was not migrated over it";
        LIST_FULL = "la liste des comptes est pleine : le jeton n'est pas copié dans le coffre", "the account list is full: the token is not copied to the vault";
        VAULT_CHANGED_TOKEN = "le coffre ne rend plus le même jeton", "the vault no longer returns the same token";
        OLD_FILE_OTHER_TOKEN = "l'ancien fichier contient un autre jeton que celui du coffre", "the old file contains a different token from the vault's";
        VAULT_OTHER_TOKEN = "le coffre a rendu un autre jeton que celui écrit", "the vault returned a different token from the one written";
        VAULT_LOST_TOKEN = "le coffre n'a pas gardé le jeton", "the vault did not keep the token";

        // Réglages (`settings.toml`).
        SETTINGS_FIX_HINT = "corrigez-le ou supprimez-le ; en attendant, rien n'y est enregistré", "fix it or delete it; until then, nothing is saved to it";
        SETTINGS_SAVE_FAILED = "Enregistrement des réglages impossible", "Could not save the settings";
        TRUE_OR_FALSE = "true ou false attendu", "true or false expected";
        QUOTED_NUMBER = "nombre entre guillemets attendu", "quoted number expected";
        QUOTED_TEXT = "texte entre guillemets attendu", "quoted text expected";
        ACCOUNTS_NOT_LIST = "« comptes » doit être une liste de sections [[comptes]]", "“comptes” must be a list of [[comptes]] sections";
        EACH_ACCOUNT_SECTION = "chaque compte doit être une section [[comptes]]", "each account must be a [[comptes]] section";
        KEY_MISSING = "comptes : « cle » manquante", "comptes: “cle” missing";
        INVALID_ID = "identifiant invalide", "invalid identifier";
        INVALID_HASH = "empreinte invalide", "invalid hash";
        ID_OR_MP = "identifiant ou \"mp\" attendu", "identifier or \"mp\" expected";
        NUMBER_EXPECTED = "nombre attendu", "number expected";

        // Fichiers surveillés (thème, réglages).
        THEME_FIX_HINT = "corrigez-le ou cliquez sur Réinitialiser le fichier", "fix it or click Reset file";
        THEME_SAVE_FAILED = "Enregistrement du thème impossible", "Could not save the theme";
        EMPTY_TEXT = "texte vide : rien n'est écrit", "empty text: nothing is written";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use crate::i18n::{fill::fill, Lang};

    templates! {
        // Écran de connexion.
        still_running_minutes(n) = "Toujours en cours (plus de {n} min)", "Still in progress (over {n} min)";
        still_running_seconds(n) = "Toujours en cours (plus de {n} s)", "Still in progress (over {n} s)";
        reading_token(name) = "Lecture du jeton de « {name} »…", "Reading the token of “{name}”…";
        connecting_to_discord(name) = "Connexion de « {name} » à Discord…", "Connecting “{name}” to Discord…";
        saved_once_accepted(vault) = "Enregistré dans le {vault} une fois accepté par Discord (compte déjà enregistré avec un autre jeton : remplacé seulement si vous le confirmez)",
            "Saved in the {vault} once accepted by Discord (account already saved with another token: replaced only if you confirm)";
        account_list_not_saved(error) = "Liste des comptes non enregistrée : {error}", "Account list not saved: {error}";
        token_invalid(error) = "Ce jeton n'est pas valable : {error}. Collez-le de nouveau, sans rien autour.", "This token is not valid: {error}. Paste it again, with nothing around it.";

        // Comptes.
        env_ignored(error) = "DISCORD_TOKEN est ignoré : {error}.", "DISCORD_TOKEN is ignored: {error}.";
        vault_still_has(name, max) = "Le coffre a encore le jeton de « {name} », mais la liste a déjà {max} comptes : retirez-en un pour le voir.",
            "The vault still has the token of “{name}”, but the list already has {max} accounts: remove one to see it.";
        remove_ask(name) = "Retirer « {name} » ?", "Remove “{name}”?";
        logout_ask(name) = "Se déconnecter de « {name} » ?", "Log out of “{name}”?";
        imported_suffix(name) = "{name} (importé)", "{name} (imported)";
        wipe_also_in_vault(ask, warning) = "{ask} Ce jeton est aussi dans le coffre.{warning}", "{ask} This token is also in the vault.{warning}";
        wipe_only_copy(ask) = "ATTENTION : ce fichier est la seule copie de ce jeton. {ask}", "WARNING: this file is the only copy of this token. {ask}";
        wipe_empty(ask) = "{ask} Il est vide : il ne contient aucun jeton.", "{ask} It is empty: it contains no token.";
        wipe_unreadable(error, ask) = "ATTENTION : contenu inconnu ({error}), c'est peut-être votre seul exemplaire de ce jeton. {ask}",
            "WARNING: unknown content ({error}), this may be your only copy of this token. {ask}";
        wipe_vault_unknown(error, ask) = "ATTENTION : le coffre n'a pas pu être lu ({error}) : ce fichier est peut-être la seule copie de ce jeton. {ask}",
            "WARNING: the vault could not be read ({error}): this file may be the only copy of this token. {ask}";
        copy_recent(date) = " La copie dans le coffre date de moins de 24 h ; en cas de coupure de courant elle pourrait ne pas être conservée. Il est plus sûr d'attendre le {date}.",
            " The copy in the vault is less than 24 hours old; it might not survive a power outage. It is safer to wait until {date}.";
        token_refused(name, message) = "Discord a refusé le jeton de « {name} » ({message}). Rien n'est effacé : s'il n'est plus valable, retirez ce compte (×) puis ajoutez-le avec un nouveau jeton.",
            "Discord refused the token of “{name}” ({message}). Nothing is erased: if it is no longer valid, remove this account (×) then add it again with a new token.";
        file_token_refused(message) = "Discord a refusé le jeton du fichier de jeton en clair ({message}). Le fichier est gardé : supprimez-le depuis cet écran s'il n'est plus valable.",
            "Discord refused the token from the plaintext token file ({message}). The file is kept: delete it from this screen if it is no longer valid.";
        env_token_refused(message) = "Discord a refusé le jeton de DISCORD_TOKEN ({message}). Aucun compte enregistré n'a été touché.",
            "Discord refused the DISCORD_TOKEN token ({message}). No saved account was touched.";
        not_in_list(max) = "Ce compte n'apparaît pas dans la liste : {max} comptes au plus sont enregistrés.", "This account does not appear in the list: at most {max} accounts are saved.";
        not_kept_add_again(max) = "Ce compte n'est pas retenu : {max} comptes au plus sont enregistrés. Retirez-en un, puis ajoutez celui-ci de nouveau.",
            "This account is not kept: at most {max} accounts are saved. Remove one, then add this one again.";
        not_kept_retry(max) = "Ce compte n'est pas retenu : {max} comptes au plus sont enregistrés. Retirez-en un, puis recommencez.",
            "This account is not kept: at most {max} accounts are saved. Remove one, then try again.";
        token_gone(name) = "Le jeton de « {name} » n'est plus dans le coffre : retirez ce compte puis ajoutez-le de nouveau.",
            "The token of “{name}” is no longer in the vault: remove this account then add it again.";
        token_read_failed(name, error) = "Lecture du jeton de « {name} » impossible ({error}).", "Could not read the token of “{name}” ({error}).";
        undo_failed(name, error) = "L'enregistrement annulé de « {name} » n'a pas pu être défait : {error}. Le compte est remis dans la liste pour pouvoir le retirer.",
            "The cancelled save of “{name}” could not be undone: {error}. The account is put back in the list so it can be removed.";
        remove_timeout(name, error) = "Le coffre n'a pas répondu à temps pour retirer « {name} » ({error}) : état inconnu, le retrait a peut-être eu lieu ; vérifiez au prochain lancement.",
            "The vault did not respond in time to remove “{name}” ({error}): unknown state, the removal may have happened; check at the next launch.";
        remove_failed(name, error) = "« {name} » n'a pas pu être retiré du coffre ({error}) : il reste dans la liste.", "“{name}” could not be removed from the vault ({error}): it stays in the list.";
        wipe_untouched(error) = "suppression du fichier de jeton en clair impossible ({error}) : il est intact", "could not delete the plaintext token file ({error}): it is intact";
        not_removed_changed(name) = "« {name} » n'a pas été retiré : le coffre ou le fichier a changé entre-temps. Relisez la question et confirmez de nouveau.",
            "“{name}” was not removed: the vault or the file changed in the meantime. Reread the question and confirm again.";
        replace_changed_closed(name) = "Le jeton enregistré de « {name} » a changé entre-temps : rien n'est remplacé (la session de ce jeton est fermée).",
            "The saved token of “{name}” changed in the meantime: nothing is replaced (this token's session is closed).";
        already_saved_closed(name) = "« {name} » est déjà enregistré avec un autre jeton : rien n'est écrit (la session de ce jeton est fermée).",
            "“{name}” is already saved with another token: nothing is written (this token's session is closed).";
        replace_typed_gone(name) = "« {name} » n'a plus de jeton dans le coffre (retiré ailleurs). Enregistrer celui que vous venez de saisir ?",
            "“{name}” no longer has a token in the vault (removed elsewhere). Save the one you just entered?";
        replace_file_gone(name) = "« {name} » n'a plus de jeton dans le coffre (retiré ailleurs). Enregistrer celui du fichier de jeton en clair ?",
            "“{name}” no longer has a token in the vault (removed elsewhere). Save the one from the plaintext token file?";
        replace_recopy(name) = "L'entrée du coffre de « {name} » a changé depuis l'échec de l'écriture. Remplacer ce qu'elle contient par le jeton d'avant ? Ce qu'elle contient sera effacé du coffre.",
            "The vault entry of “{name}” changed since the failed write. Replace what it contains with the previous token? What it contains will be erased from the vault.";
        recopy_gone(name) = "« {name} » n'a plus de jeton dans le coffre. Y enregistrer le jeton d'avant ?", "“{name}” no longer has a token in the vault. Save the previous token there?";
        recopied(name) = "Le jeton d'avant de « {name} » est de nouveau dans le coffre.", "The previous token of “{name}” is back in the vault.";
        recopy_failed(name, error) = "Le jeton d'avant de « {name} » n'a pas pu être recopié ({error}) : il reste en mémoire, réessayez « Recopier le jeton d'avant ».",
            "The previous token of “{name}” could not be copied back ({error}): it stays in memory, try “Copy back the previous token” again.";
        in_vault_list_full(name, max) = "Le jeton de « {name} » est dans le coffre, mais la liste a déjà {max} comptes : retirez-en un pour le voir.",
            "The token of “{name}” is in the vault, but the list already has {max} accounts: remove one to see it.";
        save_failed_session(error) = "Le jeton n'a pas pu être enregistré ({error}) : la session reste ouverte mais ne sera pas retenue.",
            "The token could not be saved ({error}): the session stays open but will not be remembered.";
        replace_failed(name, error) = "Le jeton de « {name} » n'a pas pu être remplacé ({error}) : le coffre garde ce qu'il contenait.",
            "The token of “{name}” could not be replaced ({error}): the vault keeps what it contained.";
        save_failed_detail(error, detail) = "Le jeton n'a pas pu être enregistré ({error}). {detail}", "The token could not be saved ({error}). {detail}";
        migration_failed(why) = "Migration du jeton vers le coffre impossible : {why}.", "Could not migrate the token to the vault: {why}.";
        imported_list_full(max) = "Le coffre a un jeton repris de l'ancien fichier, mais la liste a déjà {max} comptes : retirez-en un pour le voir.",
            "The vault has a token taken from the old file, but the list already has {max} accounts: remove one to see it.";
        accounts_unreachable(error) = "Les comptes enregistrés sont inaccessibles ({error}).", "The saved accounts are inaccessible ({error}).";
        account_number(key) = "Compte {key}", "Account {key}";
        twin_heading(name, kept) = "Même compte enregistré deux fois : « {name} » et « {kept} »", "Same account saved twice: “{name}” and “{kept}”";
        remove_named(name) = "Retirer « {name} »", "Remove “{name}”";
        entry_damaged(name) = "Entrée du coffre de « {name} » peut-être abîmée", "Vault entry of “{name}” may be damaged";
        removal_unknown(ask, error) = "{ask} Copie restante inconnue : le coffre n'a pas pu être lu ({error}) ; c'est peut-être le seul exemplaire de ce jeton.",
            "{ask} Remaining copy unknown: the vault could not be read ({error}); this may be the only copy of this token.";
        removal_list_only(name) = "« {name} » n'a plus de jeton dans le coffre (retiré ailleurs) : il sera seulement retiré de la liste. Continuer ?",
            "“{name}” no longer has a token in the vault (removed elsewhere): it will only be removed from the list. Continue?";
        removal_file_copy(ask) = "{ask} Son jeton est effacé du coffre ; le fichier en clair, qui n'est pas touché, contient encore ce jeton.",
            "{ask} Its token is erased from the vault; the plaintext file, which is not touched, still contains this token.";
        removal_entry_copy(ask) = "{ask} Son jeton est effacé de cette entrée ; une autre entrée du coffre contient encore ce jeton.",
            "{ask} Its token is erased from this entry; another vault entry still contains this token.";
        removal_twin(ask, kept) = "{ask} Son jeton est effacé du coffre : c'est le seul exemplaire enregistré de ce jeton-là. Le compte reste enregistré sous « {kept} », avec un autre jeton.",
            "{ask} Its token is erased from the vault: it is the only saved copy of that token. The account stays saved as “{kept}”, with another token.";
        removal_last(ask) = "{ask} Son jeton est effacé du coffre : c'est le seul exemplaire enregistré, il faudra récupérer le jeton depuis Discord pour revenir sur ce compte.",
            "{ask} Its token is erased from the vault: it is the only saved copy, you will have to get the token from Discord again to return to this account.";
        removal_file_unknown(ask) = "{ask} Son jeton est effacé du coffre. Copie restante inconnue : le fichier en clair n'a pas pu être lu ; c'est peut-être le seul exemplaire de ce jeton.",
            "{ask} Its token is erased from the vault. Remaining copy unknown: the plaintext file could not be read; this may be the only copy of this token.";
        waiting(date) = "le fichier en clair sera supprimé à un prochain lancement, au plus tôt le {date}, une fois la copie vérifiée (le jeton est déjà dans le coffre)",
            "the plaintext file will be deleted at a later launch, on {date} at the earliest, once the copy is verified (the token is already in the vault)";

        // Fil des comptes.
        entry_may_hold(entry, error) = "l'entrée « {entry} » du coffre contient peut-être encore ce jeton ({error})", "the vault entry “{entry}” may still contain this token ({error})";
        write_timeout(entry) = "Le coffre n'a pas répondu à temps : rien n'a été retiré ni remis. L'entrée « {entry} » contient peut-être ce jeton ou celui d'avant ; le compte reste dans la liste.",
            "The vault did not respond in time: nothing was removed or put back. The entry “{entry}” may contain this token or the previous one; the account stays in the list.";
        put_back_failed(error, entry) = "Le jeton d'avant n'a pas pu être remis ({error}) : l'entrée « {entry} » du coffre est peut-être abîmée. Il est gardé en mémoire : « Recopier le jeton d'avant » le réécrit ; le compte reste dans la liste.",
            "The previous token could not be put back ({error}): the vault entry “{entry}” may be damaged. It is kept in memory: “Copy back the previous token” rewrites it; the account stays in the list.";
        new_entry_not_removed(entry, error) = "L'entrée « {entry} » écrite à l'instant n'a pas pu être retirée ({error}) : elle contient peut-être un jeton abîmé ; le compte reste dans la liste pour pouvoir la retirer.",
            "The entry “{entry}” just written could not be removed ({error}): it may contain a damaged token; the account stays in the list so it can be removed.";
        lock_create(error) = "verrou entre instances impossible à créer ({error})", "could not create the lock between instances ({error})";
        lock_open(error) = "verrou entre instances impossible à ouvrir ({error})", "could not open the lock between instances ({error})";
        lock_failed(error) = "verrou entre instances impossible ({error})", "the lock between instances failed ({error})";

        // Coffre.
        vault_unavailable(why) = "coffre indisponible : {why}", "vault unavailable: {why}";
        entry_account(key) = "compte {key}", "account {key}";
        vault_no_answer(secs) = "le coffre ne répond pas (plus de {secs} s)", "the vault is not responding (over {secs} s)";
        access_denied(error) = "accès refusé ({error})", "access denied ({error})";
        windows_error(error) = "erreur de Windows ({error})", "Windows error ({error})";

        // Migration de l'ancien fichier.
        emptied_not_deleted(error) = "fichier vidé (il ne contient plus le jeton) mais pas supprimé ({error})", "file emptied (it no longer contains the token) but not deleted ({error})";
        nothing_overwritten(what) = "{what} : rien n'est écrasé ni supprimé", "{what}: nothing is overwritten or deleted";
        overwrite_failed(error) = "écrasement impossible ({error})", "could not overwrite ({error})";
        held_elsewhere(error) = "fichier ouvert par un autre programme ou protégé ({error}) : rien n'est touché, nouvel essai au prochain lancement",
            "file open in another program or protected ({error}): nothing is touched, will retry at the next launch";
        not_copied(why, kept) = "{why} : rien n'est copié dans le coffre à ce lancement{kept}", "{why}: nothing is copied to the vault at this launch{kept}";
        old_file_unreadable(error) = "ancien fichier de jeton illisible ({error}) : laissé en place", "old token file unreadable ({error}): left in place";
        old_file_refused(why) = "ancien fichier de jeton refusé ({why}) : laissé en place, non migré", "old token file refused ({why}): left in place, not migrated";
        vault_read_failed(error) = "lecture du coffre impossible ({error})", "could not read the vault ({error})";
        old_file_kept(why) = "{why} : l'ancien fichier est gardé", "{why}: the old file is kept";
        vault_reread_failed(error) = "relecture du coffre impossible ({error})", "could not read the vault back ({error})";
        file_inaccessible(error) = "fichier inaccessible ({error})", "file inaccessible ({error})";
        old_file_changed(error) = "l'ancien fichier a changé ({error})", "the old file changed ({error})";
        old_file_delete_failed(error) = "suppression de l'ancien fichier impossible ({error})", "could not delete the old file ({error})";
        old_file_emptied(error) = "l'ancien fichier a été vidé (il ne contient plus le jeton) mais n'a pas pu être supprimé ({error})",
            "the old file was emptied (it no longer contains the token) but could not be deleted ({error})";
        vestige_not_deleted(error) = "un ancien fichier vidé (sans jeton) n'a pas pu être supprimé ({error})", "an emptied old file (no token) could not be deleted ({error})";
        vault_write_failed(error) = "écriture dans le coffre impossible ({error})", "could not write to the vault ({error})";

        // Réglages (`settings.toml`).
        field_error(field, message) = "{field} : {message}", "{field}: {message}";
        must_be_section(name) = "« {name} » doit être une section [{name}]", "“{name}” must be a [{name}] section";
        field_missing(field) = "{field} manquant", "{field} missing";
        too_many_accounts(max) = "plus de {max} comptes : retirez les sections [[comptes]] en trop", "more than {max} accounts: remove the extra [[comptes]] sections";
        invalid_key(key) = "clé invalide « {key} »", "invalid key “{key}”";
        settings_syntax_detail(detail) = "syntaxe TOML invalide (détail : {detail})", "invalid TOML syntax (detail: {detail})";

        // Fichiers surveillés (thème, réglages).
        theme_exported(path) = "Thème exporté dans {path}", "Theme exported to {path}";
        export_failed(path, error) = "Export impossible vers {path} ({error})", "Could not export to {path} ({error})";
        import_refused(file, line, message) = "Import refusé : {file} contient une erreur{line} ({message})", "Import refused: {file} contains an error{line} ({message})";
        import_unreadable(path, error) = "Import impossible : {path} est illisible ({error})", "Could not import: {path} is unreadable ({error})";
        save_failed_in(what, path, error) = "{what} dans {path} ({error})", "{what} to {path} ({error})";
        backup_failed(path, error) = "Copie de sauvegarde impossible vers {path}, fichier laissé tel quel ({error})", "Could not make a backup copy at {path}, file left as is ({error})";
        file_reset(path) = "Fichier réinitialisé ; l'ancien est gardé dans {path}", "File reset; the old one is kept in {path}";
        reset_failed(path, error) = "Réinitialisation impossible de {path} ({error})", "Could not reset {path} ({error})";
        at_line(line) = " ligne {line}", " on line {line}";
        file_has_error(file, line, hint, why) = "{file} contient une erreur{line} : {hint} ({why})", "{file} contains an error{line}: {hint} ({why})";
        file_over_kb(kb) = "fichier de plus de {kb} Ko", "file larger than {kb} KB";
        over_kb_not_read(kb) = "plus de {kb} Ko, fichier non lu", "over {kb} KB, file not read";
        file_unreadable(error) = "fichier illisible : {error}", "unreadable file: {error}";
    }
}
