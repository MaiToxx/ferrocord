# Ferrocord

Client Discord **natif** écrit en Rust : il se connecte à ton vrai compte Discord, sans Electron ni navigateur embarqué.

> ⚠ **Client non officiel.** Utiliser un client tiers avec un compte utilisateur enfreint les conditions d'utilisation de Discord et peut entraîner la suspension du compte. À utiliser en connaissance de cause.

## Lancer

```bash
cargo run --release
```

L'exécutable se trouve dans `target/release/ferrocord.exe` (un seul fichier d'environ 46 Mo, rien à installer).

Au premier lancement, colle ton jeton Discord. Si tu coches « Se souvenir de moi », il est rangé dans le **Gestionnaire d'identifiants de Windows** (une entrée `ferrocord:compte:<identifiant>` par compte), une fois accepté par Discord ; jamais dans un fichier. Plusieurs comptes peuvent être enregistrés : clique sur ton nom, en bas à gauche, pour changer de compte, en ajouter un ou te déconnecter (le compte est alors retiré de la liste et son jeton effacé du coffre). Au lancement, l'application rouvre le dernier compte sur le dernier salon ouvert, à la même taille et la même place de fenêtre (`%APPDATA%\ferrocord\settings.toml`, qui ne contient aucun jeton).

Un jeton enregistré n'est jamais remplacé sans ton accord : si tu saisis un autre jeton pour un compte déjà enregistré, rien n'est écrit, la session continue avec le jeton saisi et le sélecteur de compte demande « Ce compte est déjà enregistré. Remplacer le jeton enregistré par celui que vous venez de saisir ? L'ancien sera effacé du coffre. » Sans réponse ou sur « Non », le coffre garde l'ancien ; sur « Oui », l'entrée est relue (verrou tenu) et remplacée seulement si elle n'a pas changé entre-temps, sinon la question est reposée. Un remplacement confirmé va au bout même si l'on change de compte ou ferme la session pendant l'écriture ; s'il ne peut pas être fait (entrée changée entre-temps, écriture refusée), un message le dit. Même jeton : rien à écrire, aucune question. Coffre illisible : rien n'est écrit, et l'application le dit.

Les versions précédentes enregistraient le jeton en clair dans `%APPDATA%\ferrocord\token`. La migration se fait en deux étapes, **jamais dans le même lancement, ni à moins de 24 h d'intervalle** :

1. au lancement, le jeton du fichier est copié dans le coffre (entrée `ferrocord:compte:importe`) puis relu pour vérification ; **le fichier reste en place** et la session s'ouvre avec le jeton du coffre. Une écriture que le coffre vient d'accepter peut encore n'être qu'en mémoire et se perdre dans une coupure de courant : ce lancement-là ne supprime donc rien. L'instant de la copie (heure de l'horloge, jamais le jeton) est retenu dans `settings.toml`, et l'avertissement dit « le fichier en clair sera supprimé à un prochain lancement, au plus tôt le <date> » ;
2. à un lancement suivant, si le coffre a déjà ce jeton (l'entrée a survécu à l'arrêt de l'application), que la copie date d'au moins 24 h (une écriture relue quelques secondes plus tard par une autre instance peut encore n'être qu'en mémoire) et que ce n'est pas le processus qui l'a écrite, le fichier est supprimé une fois la connexion acceptée par Discord (READY) avec ce jeton : le coffre est relu, le fichier est ouvert une seule fois, en exclusivité (aucun autre programme ne peut y écrire, le renommer ni le remplacer pendant ce temps ; s'il est déjà ouvert ailleurs, rien n'est touché et l'essai est refait au lancement suivant), son contenu est comparé à celui du coffre sur ce même descripteur, puis il est écrasé (zéros de même longueur, écrits sur le disque) ; le chemin est ensuite rouvert et relu, et supprimé seulement s'il désigne encore un fichier sans jeton (vide ou de zéros).

Lancement par lancement : 1er lancement, copie dans le coffre, fichier gardé ; premier lancement au moins 24 h plus tard, fichier supprimé après READY ; lancements suivants, plus rien à faire. Copie trouvée dans le coffre sans instant retenu (`settings.toml` perdu ou illisible), ou avec un instant à plus de 48 h dans le futur : l'instant est retenu à ce lancement, et l'on attend 24 h de plus. Un instant à moins de 48 h dans le futur (horloge en retard) est gardé : on attend qu'il soit passé de 24 h. L'étape 2 exige aussi que l'instant soit antérieur au démarrage du processus, et toute écriture du compte importé (pas seulement la migration) remet l'instant à maintenant et déprogramme une étape 2 déjà prévue ; une relecture de `settings.toml` ne le fait jamais reculer ; retirer le compte importé l'efface ; l'étape 2 n'utilise qu'un instant relu sur le disque.

Si les zéros sont écrits mais que la suppression échoue (ou que l'application s'arrête entre les deux), le fichier ne contient plus le jeton : il est signalé comme « fichier vidé (sans jeton) », jamais comme une copie, et supprimé au lancement suivant. Un fichier vide ou qui ne contient que des zéros n'est jamais présenté comme une copie du jeton.

Aucun autre chemin automatique ne supprime ce fichier : un refus de Discord, le retrait d'un compte ou une déconnexion ne touchent qu'au coffre, jamais au fichier. Seul le bouton « Supprimer le fichier de jeton en clair », confirmé, le supprime en dehors de l'étape 2 ; sa question dit, après lecture du coffre et du fichier, si ce jeton y est aussi, si le fichier en est la seule copie, ou que son contenu est inconnu (fichier illisible : c'est peut-être le seul exemplaire). Moins de 24 h après la dernière écriture de ce jeton dans le coffre (quelle que soit l'entrée qui le contient ; instant retenu par compte dans `settings.toml`, ou inconnu), elle prévient aussi qu'en cas de coupure de courant la copie du coffre pourrait ne pas être conservée et donne la date à partir de laquelle il est plus sûr de supprimer ; la suppression reste possible après confirmation. La question de retrait d'un compte (ou de déconnexion) dit de même s'il restera une copie du jeton (« le fichier en clair contient encore ce jeton »), si c'est le seul exemplaire enregistré (« il faudra récupérer le jeton depuis Discord pour revenir sur ce compte » ; pour un doublon dont l'autre ligne a un autre jeton, elle dit plutôt que le compte reste enregistré sous cette autre ligne), ou que c'est inconnu. Au moment d'exécuter l'action confirmée, coffre et fichier sont relus, verrou entre instances tenu : s'ils ont changé (une autre instance a fait l'étape 2, retiré le compte…), rien n'est fait et la question est reposée avec le texte à jour. En détail :

- si quoi que ce soit échoue (coffre indisponible ou qui ne répond pas, relecture différente, autre instance de Ferrocord occupée, liste des comptes pleine), le fichier est laissé intact et son jeton reste utilisable pour la session (il l'ouvre si aucun compte enregistré n'est rouvert, sinon il est proposé dans le sélecteur) ; la migration est retentée au lancement suivant. Si la lecture du dernier compte échoue alors que la migration vient d'écrire, le jeton du fichier reste proposé pour la session ;
- si le coffre contient déjà un autre jeton pour ce compte (différent de celui du fichier), c'est lui qui sert : le fichier n'est pas migré par-dessus. Si Discord refuse ce jeton du coffre, rien n'est effacé et « Essayer le jeton du fichier » est proposé ; accepté par Discord, ce jeton n'est écrit dans le coffre qu'après « Oui » à la question « Discord a refusé le jeton enregistré et accepté celui du fichier en clair. Remplacer le jeton enregistré par celui du fichier ? L'ancien sera effacé du coffre. » (le fichier n'est toujours pas touché) ;
- un fichier vide, illisible, ou dont le jeton est abîmé (espace interne, caractère invisible ; une marque d'ordre des octets en tête est simplement retirée) n'est ni migré ni touché ;
- un lien (symbolique ou jonction), un dossier ou un fichier qui a plusieurs noms (liens physiques) n'est jamais écrasé ni supprimé ;
- retirer le compte importé pendant que l'étape 2 est programmée l'annule (le fichier reste, comme le dit la question) ; pendant qu'elle tourne, le retrait est refusé avec une explication. Une fois le compte importé retiré, le refus est retenu dans `settings.toml` (taille et date du fichier, jamais son contenu) : le fichier n'est plus importé tout seul, il reste signalé avec « Importer » et « Supprimer le fichier de jeton en clair » ;
- le même compte enregistré deux fois (par exemple importé, puis saisi de nouveau) n'est jamais nettoyé tout seul : le sélecteur le signale et propose d'en retirer un, confirmé ;
- tant qu'un fichier de jeton en clair subsiste, l'écran de connexion et le sélecteur de compte l'indiquent de façon durable, avec le bouton « Supprimer le fichier de jeton en clair ».

Un refus de Discord n'efface jamais rien : ni entrée du coffre, ni fichier ; une écriture du coffre en cours au moment du refus va au bout. Le message le dit, le compte n'est plus rouvert tout seul au lancement suivant, et le × le retire si le jeton n'est plus valable.

L'écrasement par des zéros ne garantit pas un effacement physique : un SSD, un cliché instantané de Windows ou une sauvegarde peuvent garder l'ancien contenu. Après la migration, il est conseillé de renouveler son jeton (changer son mot de passe Discord invalide les jetons existants), puis d'ajouter le compte de nouveau (et de confirmer le remplacement du jeton enregistré).

Tous les appels au Gestionnaire d'identifiants passent par un seul fil, dans l'ordre (le Gestionnaire ne garantit pas l'ordre d'appels venus de fils différents), avec un délai de 10 s. Un appel qui dépasse ce délai est abandonné et le coffre devient « indéterminé » jusqu'au prochain lancement : plus rien n'y est écrit, retiré ou remis, le fichier en clair est gardé, et le verrou entre instances reste tenu tant que l'appel abandonné n'est pas terminé. Une réponse « absent » qui mènerait à écrire ou à retirer est vérifiée par une deuxième lecture. La fermeture de l'application n'attend pas plus de 3 s le fil des comptes.

Deux instances de Ferrocord ouvertes en même temps ne migrent pas et n'écrivent pas dans le coffre au même moment (verrou `%APPDATA%\ferrocord\comptes.lock`, sans contenu ; rendu par Windows même si le processus est tué). Une écriture qui échoue remet la valeur précédente du coffre plutôt que de l'effacer, vérifiée par relecture ; si cette remise échoue, un message nomme l'entrée exacte qui est peut-être perdue ou abîmée, et le compte reste dans la liste ; le jeton d'avant est gardé en mémoire jusqu'à la fermeture, et « Recopier le jeton d'avant » le réécrit, seulement si l'entrée est vide ou contient encore ce que l'écriture ratée y a laissé (sinon une question est posée). Un enregistrement annulé (session fermée pendant l'écriture d'un jeton saisi, sans question de remplacement) est défait tout de suite après l'écriture, même si l'application se ferme avant la réponse. À l'ajout d'un compte, le coffre n'est écrit qu'une fois la liste (`settings.toml`) écrite sur le disque avec ce compte (sauf si elle ne peut pas l'être : fichier refusé ou écriture en échec) ; au retrait, le coffre est vidé avant la liste. Une entrée qui vient d'être écrite remet toujours son compte dans la liste (même retiré entre-temps, ou ajouté de nouveau juste après un retrait). Deux fenêtres ouvertes écrivent le même `settings.toml` : quand une fenêtre relit la liste réécrite par l'autre, elle garde les comptes qu'elle a ajoutés et qui n'étaient pas encore sur le disque, et cherche dans le coffre les comptes que cette liste a fait disparaître : ceux dont l'entrée y est encore reviennent dans la liste. Le Gestionnaire d'identifiants n'est pas parcouru au démarrage (cela demanderait une fonction de recherche en plus) : y sont cherchées l'entrée `importe` et chaque entrée dont une écriture est retenue dans `settings.toml` (`[ecritures]`) sans être dans la liste ; celles qui y sont reviennent dans la liste. Si Ferrocord s'arrête entre l'écriture de la liste et celle du coffre, le compte apparaît avec le message « le jeton n'est plus dans le coffre » ; s'il s'arrête entre le vidage du coffre et celui de la liste, le compte reste listé et se retire normalement. Une entrée `ferrocord:compte:<identifiant>` absente de la liste ne peut donc venir que d'une liste supprimée ou modifiée à la main (ou d'une annulation qui a échoué alors que l'application se fermait) : elle se retire depuis le Gestionnaire d'identifiants de Windows. Au plus 16 comptes sont enregistrés ; au-delà, l'application le dit et n'écrit rien de plus dans le coffre.

Tu peux aussi passer un jeton via la variable d'environnement `DISCORD_TOKEN` : c'est une session éphémère, jamais enregistrée, et un refus de Discord n'efface alors aucun compte enregistré ; une valeur abîmée est ignorée (signalée) et le compte enregistré se rouvre normalement. La taille et la place de la fenêtre sont retenues ; une place qui ne tombe plus sur un écran présent (écran débranché) est ignorée. Hors de Windows, aucun coffre n'est pris en charge : le jeton ne sert que pour la session en cours.

Un jeton donne un accès complet à ton compte : ne le partage jamais et ne le colle nulle part ailleurs. Un jeton de bot fonctionne aussi, en le préfixant par `Bot `.

## Installer, démarrer avec Windows, liens, mises à jour

**Installer** (pour ton compte Windows, sans droits d'administrateur) : l'exécutable s'installe lui-même. Lancé hors de son dossier d'installation (un téléchargement, par exemple), il propose une fois « Installer » ou « Lancer sans installer » ; jamais depuis un dossier de compilation (`target\…`). Depuis les sources :

```powershell
cargo build --release
powershell -ExecutionPolicy Bypass -File tools\installer.ps1
```

(ce script lance `ferrocord.exe --installer --silencieux` ; options `-Bureau`, `-Demarrage`). L'exécutable est copié dans `%LOCALAPPDATA%\Programs\Ferrocord`, avec un raccourci dans le menu Démarrer et une entrée dans « Applications installées ». La désinstallation (depuis cette entrée, ou `ferrocord.exe --desinstaller`) retire l'exécutable, les raccourcis, le démarrage avec Windows et le protocole `discord://` s'ils pointent vers cette installation, et **garde tes réglages et ton cache** (`%APPDATA%\ferrocord`, `%LOCALAPPDATA%\ferrocord`), sauf si tu demandes de les effacer (case à cocher, ou `--supprimer-donnees`). Les jetons enregistrés restent dans le Gestionnaire d'identifiants : déconnecte-toi dans Ferrocord avant de désinstaller pour les retirer.

**Paramètres > Ferrocord** :
- « Démarrer avec Windows » (valeur `Ferrocord` de `HKCU\…\CurrentVersion\Run`) et « Démarrer réduit » (`--reduit` : la fenêtre reste dans la zone de notification) ;
- « Ouvrir les liens Discord dans Ferrocord » : enregistre le protocole `discord://` pour ton compte seulement (`HKCU\Software\Classes\discord`). Si un autre programme l'avait (l'application officielle), sa commande est gardée dans `HKCU\Software\Ferrocord\Protocole` et remise en place quand tu décoches.

**Instance unique** : un second lancement (raccourci, lien `discord://`) transmet son lien à la fenêtre ouverte par un canal nommé local (`\\.\pipe\ferrocord-<utilisateur>`), puis se ferme. Liens compris : salon, message, serveur, invitation (la fenêtre « Rejoindre » s'ouvre, préremplie), profil, accueil ; formes `discord://-/channels/…` et `https://discord.com/channels/…`, `/invite/…`, `discord.gg/…`, `/users/…`. Un lien Discord collé dans un message s'ouvre aussi dans Ferrocord. Les autres liens s'ouvrent dans le navigateur, lancé par Ferrocord sans la variable `DISCORD_TOKEN` dans son environnement.

**Mises à jour** : depuis les « Releases » du dépôt GitHub public nommé dans `src/install/update/depot.txt` (vide : désactivées), ou depuis une adresse écrite dans `%APPDATA%\ferrocord\local.toml`, qui l'emporte :

```text
[mises_a_jour]
manifeste = "https://exemple.org/ferrocord/ferrocord-manifeste.json"
```

Ferrocord vérifie au démarrage puis une fois par jour au plus (réglage « Vérifier automatiquement », Paramètres > Ferrocord), et annonce la version plus récente ; rien ne se télécharge ni ne s'installe sans un clic. Le manifeste est signé (Ed25519) ; l'exécutable téléchargé doit avoir la taille et l'empreinte SHA-256 du manifeste. Il remplace l'actuel au lancement suivant (l'ancien est gardé sous `ferrocord.old.exe`) ; si la nouvelle version n'affiche pas sa première image, le lancement suivant remet l'ancienne et cette version n'est plus proposée. Une version égale ou plus ancienne, ou d'un autre canal, est refusée.

**Publier une version** (clé de signature, `tools\publier.ps1`, `gh release create`, renouvellement de la clé) : voir `docs/distribution.md`. La **clé privée ne doit jamais entrer dans le dépôt** : les outils refusent de l'écrire ou de la lire dans un dossier suivi par git.

**Licences** : Paramètres > À propos montre le texte complet de la licence de chaque dépendance de l'exécutable, bibliothèques C comprises (libopus, OpenH264, aws-lc). Le fichier intégré (`src/install/licences.json.gz`) se refait après tout changement de `Cargo.lock` : `powershell -ExecutionPolicy Bypass -File tools\licences.ps1`.

## Android (première version)

L'APK se fabrique sous Windows : `powershell -ExecutionPolicy Bypass -File tools\android.ps1` (sortie : `target\android\ferrocord-debug.apk`, ARM 64 bits, Android 8 ou plus). Outils : JDK 17 (`winget install Microsoft.OpenJDK.17`), SDK Android en ligne de commande dans `%LOCALAPPDATA%\Android\Sdk` avec `platform-tools`, `platforms;android-36`, `build-tools;36.1.0`, `ndk;28.2.13676358` et `cmake;4.1.2` (pour Ninja), puis `rustup target add aarch64-linux-android` et `cargo install cargo-ndk`. L'APK est signée par la clé de débogage d'Android Studio (`%USERPROFILE%\.android\debug.keystore`, créée si elle manque, jamais dans le dépôt).

Installation : copier l'APK sur le téléphone et l'ouvrir (autoriser l'installation d'applications inconnues pour le gestionnaire de fichiers), ou `adb install target\android\ferrocord-debug.apk` avec le débogage USB activé.

Sur Android : jeton chiffré par une clé du Keystore d'Android (fichier dans le stockage privé de l'application), notifications du système (permission demandée à la première), micro et haut-parleur par AAudio (permission du micro demandée au premier appel), vidéo reçue décodée par OpenH264, liens ouverts par un Intent (seule l'adresse part), polices Roboto du système. Absents : partage d'écran et caméra (boutons masqués), zone de notification, démarrage avec le système, liens `discord://`, instance unique, mises à jour et installateur, détection des jeux, appui-pour-parler global, son par processus, accessibilité (eframe ne l'offre qu'avec GameActivity), boîtes « Ouvrir » (envoi de fichiers, émojis et autocollants, programmes).

## Ce qui marche

- Serveurs (dans l'ordre de ton compte), catégories, salons textuels et d'annonces
- Messages privés et groupes
- Messages en temps réel : envoi, réception, modification, suppression
- Historique avec « Charger les messages précédents »
- Non-lus (gras) et mentions (pastille rouge), y compris sur les icônes de serveur
- Mentions, salons, emojis personnalisés et horodatages affichés lisiblement
- Liens cliquables, pseudos de serveur
- Images jointes (GIF, WebP et APNG animés compris) en vignette (seules ou en grille, place réservée avant le chargement), spoilers masqués jusqu'au clic, pastille « GIF », vidéos et sons lus dans le fil, cartes pour les autres fichiers (ouverts dans le navigateur) ; visionneuse plein écran (zoom, déplacement, original, lien, texte alternatif) ; embeds complets et aperçus de liens. Réglage « Charger les images : automatiquement / au clic »
- Copier le texte ou l'ID d'un message (clic droit)
- Vrai gras et vraie italique (Segoe UI de Windows, lue au démarrage ; police intégrée sinon). Emojis en couleur partout (messages, réactions, saisie, statut, noms de salons), en grand dans un message qui n'a qu'eux ; emojis personnalisés des serveurs, animés compris ; sélecteur d'emojis (recherche, récents, emojis des serveurs, catégories, teinte de peau) depuis la saisie et « Ajouter une réaction » ; autocollants affichés (Lottie : leur nom) et envoyés depuis le sélecteur
- Reconnexion automatique avec reprise de session (pas de messages perdus)
- Gestion des limites de débit de Discord (429)
- Permissions : salons inaccessibles masqués, saisie fermée (avec la raison) sans droit d'écrire ou pendant une exclusion temporaire, historique non demandé sans droit de le lire
- Avatars (auteurs des messages, conversations privées, toi) et icônes des serveurs et des groupes, téléchargés depuis le CDN de Discord sans jamais y envoyer ton jeton, gardés dans un cache disque borné (`%LOCALAPPDATA%\ferrocord\images`, 256 Mo au plus)

## Ajouté depuis

Le code et les libellés de ce qui suit existent et sont couverts par les tests hors ligne. « Fait » veut dire « branché » : ces fonctions n'ont pas été vérifiées une à une sur un vrai compte, et la liste de ce qui reste à essayer est au chapitre 5 de `docs/ecarts-restants.md`.

- Vocal, appels, vidéo et partage d'écran (H.264 et VP8)
- Réactions, réponses, modification de ses messages, envoi de fichiers, sondages
- Fils et forums (liste, galerie, étiquettes, création de publication)
- Liste des membres, statuts en ligne, statut personnalisé, indicateur « en train d'écrire »
- Sélecteur de GIF et favoris, recherche de messages, boîte de réception
- Paramètres du serveur et des salons, modération, plugins (`docs/plugins.md`)
- Catégories de contacts : des groupes que l'on crée pour ranger ses amis, sur la page Amis et dans la colonne des messages privés. Elles n'existent pas chez Discord et restent sur l'ordinateur (`local.toml`)

## Pas encore géré

L'inventaire complet et à jour est dans `docs/ecarts-restants.md`. L'essentiel :

- Autocollants Lottie (leur nom est affiché à la place), animation des super réactions, polices et effets des noms
- Partage d'écran : VP9 et AV1, changement de fenêtre sans arrêter le partage
- Raccourcis globaux réglables (hors appui-pour-parler), correcteur orthographique, barre de mise en forme sur la sélection
- Plusieurs pages d'administration d'un serveur (vérification des membres, guide du serveur, « voir en tant que rôle »)
- Linux et macOS
- Écarté volontairement : achats (Nitro, boosts, boutique), tout ce qui passe par un captcha (connexion par mot de passe, création de compte), quêtes, activités intégrées, overlay en jeu
- Permissions sur les serveurs dont Discord n'envoie pas ton membre ou les rôles au démarrage : tout y reste affiché, et Discord répond « accès refusé » au besoin
- Proxys : les variables `HTTP_PROXY`, `HTTPS_PROXY` et `ALL_PROXY` sont volontairement ignorées, pour qu'elles ne puissent pas détourner les requêtes qui portent ton jeton. Aucune prise en charge des proxys pour l'instant.

## Pourquoi c'est plus léger que l'app officielle

| | App officielle | Ferrocord |
|---|---|---|
| Moteur | Electron (Chromium + Node.js), plusieurs processus | Un seul processus natif |
| Taille | ~660 Mo installés | ~46 Mo, un seul fichier |
| CPU au repos | animations, JS en continu | 0 % : redessin uniquement quand il se passe quelque chose |

Ce qui est optimisé dans le code :

- Les événements de la passerelle sont filtrés sans être décodés : les présences, statuts, etc., qui forment l'essentiel du trafic, sont ignorés immédiatement.
- Les événements utiles sont décodés directement dans des structures minimales.
- Les identifiants sont stockés en `u64`, pas en chaînes.
- Le cache est borné : 500 messages par salon et 20 salons en mémoire, les plus anciens sont libérés.
- Les images sont téléchargées et décodées hors du fil d'interface, à la taille d'affichage, seulement quand elles sont visibles ; les textures sont plafonnées (48 Mo) et libérées quand elles ne sont plus affichées.
- Les décodages simultanés sont limités, une image plus grande que le double de la taille demandée est refusée avant d'être décodée, et au plus 32 textures sont créées par image affichée.
- Le rendu se fait en OpenGL, plus léger que wgpu (mesuré : ~170 Mo contre ~400 Mo) ; sous Windows, il passe par ANGLE (Direct3D 11), qui évite le pilote OpenGL (`docs/mesures-d3d.md`).

La majorité de la mémoire restante vient du pilote graphique chargé par Windows, pas du code du client.

### Images

- Cache disque : `%LOCALAPPDATA%\ferrocord\images` (le dossier de cache de ton compte Windows), 256 Mo au plus. Un fichier inutilisé depuis 30 jours est supprimé ; au-delà du plafond, les moins récemment utilisés partent jusqu'à redescendre à 90 %. Le dossier peut être supprimé à tout moment (les images sont alors retéléchargées).
- Une image en échec réseau est retentée au premier redessin qui suit la fin de son délai (5 s, doublé à chaque échec, 10 min au plus) : aucun réveil n'est programmé pour cela, une fenêtre au repos ne retente rien. Une image introuvable (404) n'est plus redemandée pendant la session.
- Si le CDN répond 429 (trop de requêtes), plus aucune image n'est demandée pendant le délai qu'il indique (30 s s'il n'en indique pas, 5 min au plus), puis une seule à la fois avant de reprendre le rythme normal.
- L'adresse des icônes de groupe (`cdn.discordapp.com/channel-icons/…`) n'est pas documentée par Discord : c'est celle qu'utilise l'app officielle. Si elle changeait, ces icônes seraient remplacées par des pastilles aux initiales.

## Paramètres du compte

La fenêtre « Paramètres » rassemble aussi les réglages de ton compte Discord : statut et statut personnalisé, confidentialité, texte et images, notifications, langue du compte, dossiers de serveurs (lecture), thème Discord (lecture, avec « Appliquer à Ferrocord »), Mon compte (adresse e-mail et téléphone masqués), profil (pseudo, bio, pronoms, couleur, pseudo sur un serveur), appareils, comptes liés et applications autorisées. Rien n'est envoyé sans geste ; un refus de Discord rétablit l'ancienne valeur. Ce qui demande un mot de passe, un code ou un paiement s'ouvre dans Discord, dans le navigateur. Un clic droit sur un serveur, une catégorie ou un salon règle ses notifications (muet pour une durée, niveau, mentions @everyone et de rôles) ; un salon muet ne devient pas non lu, sauf s'il te mentionne.

## Apparence

Toute l'apparence (couleurs, tailles de texte, rayons, espacements, largeurs des panneaux) se règle à la volée :

- La roue dentée à côté de ton nom ouvre la fenêtre « Paramètres » (catégories à gauche comme dans Discord, recherche d'un réglage, Échap la ferme). **Ctrl+,** l'ouvre directement sur la page « Apparence ». En haut, l'essentiel : préréglage (Sombre, Minuit, Clair, Contraste élevé, Compact, Aéré), couleur d'accent, fond principal, taille du texte, zoom, densité, messages groupés ou sur une ligne (« 12:34 auteur : texte »), horloge 12 h / 24 h, rail des serveurs en pastilles ou en liste avec les noms, avatars affichés ou masqués (masqués, ils disparaissent des messages, des conversations privées et du pied, et aucun n'est téléchargé ; les icônes de serveur restent), avatars ronds ou carrés arrondis, taille des avatars, arrondi des coins. Tout le reste est rangé sous « Avancé ». Chaque réglage s'applique immédiatement ; le survol d'un libellé l'explique.
- Les réglages sont enregistrés dans `%APPDATA%\ferrocord\theme.toml` (un court instant après le dernier changement). Le fichier est relu dès qu'il est modifié : tu peux l'éditer dans un éditeur de texte et voir le résultat en direct ; s'il change pendant qu'un enregistrement attend, c'est le fichier qui l'emporte.
- Un fichier invalide est refusé avec la ligne en cause, le thème précédent reste en place et rien n'est enregistré tant qu'il n'est pas corrigé ; le bouton « Réinitialiser le fichier » le remplace après en avoir gardé une copie dans `theme.toml.bak`. Un fichier vide compte comme absent ; un fichier de plus de 1 Mo est refusé sans être lu.
- La largeur de la colonne des salons se règle aussi à la souris, et le zoom au clavier (Ctrl + / Ctrl -) : les deux sont ramenés dans leurs bornes et enregistrés.
- Les boutons « Importer » et « Exporter » (« Avancé », « Partager un thème ») lisent ou écrivent un thème dans un autre fichier, pour le partager.

Le fichier peut être partiel : les clés absentes prennent la valeur du préréglage `base` (`rouille`, le thème sombre, par défaut). Par exemple :

```toml
base = "minuit"

[couleurs]
accent = "#7c8cff"      # couleurs en "#rrggbb"

[typographie]
body = 15               # texte courant

[mise_en_page]
zoom = 1.1
density = "compact"     # "aere", "normal" ou "compact"
messages = "compact"    # "groupes" ou "compact"
clock = "12h"           # "24h" ou "12h"
rail_style = "liste"    # "pastilles" ou "liste"
avatar_shape = "carres" # "ronds" ou "carres"
avatars = "masques"     # "affiches" ou "masques"
```

« Exporter » écrit un fichier complet et commenté qui liste toutes les clés et leurs bornes.

Pour voir l'interface sans compte ni réseau (données fictives de `tests/fixtures/`) :

```bash
cargo run --example preview -- --theme clair --panel
cargo run --example preview -- --theme mon-theme.toml --view dm --hover-none --screenshot capture.png
cargo run --example preview -- --view login
cargo run --example preview -- --view masque   # aussi : lecture-seule, sans-historique, inconnu, exclusion
cargo run --example preview -- --set mise_en_page.avatar_shape=carres --set mise_en_page.avatar_size=48
```

`--audit` modifie chaque jeton à tour de rôle et liste ceux qui ne changent rien à la vue affichée (pour vérifier qu'aucun réglage n'est inutile).

## Architecture

| Dossier | Rôle |
|---|---|
| `src/net/` | Passerelle WebSocket (heartbeat, reprise, reconnexion) et API REST. Ne connaît pas l'interface : publie des événements et appelle un réveil. |
| `src/state/` | Magasin central (serveurs, salons, messages, non-lus, mentions, rôles et permissions) ; reçoit les événements du réseau, accumule les commandes de l'interface. Ne dépend ni d'egui ni du réseau. |
| `src/session.rs` | Raccordement interface ↔ réseau : runtime tokio (2 threads), connexion. |
| `src/voice/` | Orchestrateur du vocal : relie la connexion vocale (`net::voice`, DAVE), le moteur audio (`audio::engine`, coupé en moitiés capture et lecture, sans verrou), les périphériques et le traitement à l'état vocal, par le trait `state::voice_link::VoiceLink`. Rien ne tourne hors appel. |
| `src/account/` | Jeton enveloppé (jamais affiché), coffre à jetons (Gestionnaire d'identifiants de Windows ; en mémoire pour les tests et l'aperçu), migration de l'ancien fichier, fil des comptes. |
| `src/settings.rs` | Réglages persistants (`settings.toml`) : comptes connus, dernier salon par compte, fenêtre, panneau Apparence. Même mécanisme que `theme.toml`. |
| `src/theme/` | Jetons d'apparence, préréglages, lecture/écriture de `theme.toml` et surveillance du fichier dans un fil à part. |
| `src/desk/` | Bureau de Windows : notifications (toasts WinRT), lecture des sons d'interface (fabriqués par `src/audio/sons/`), icône de la zone de notification, `notify.toml`. Fil à part bloqué sur sa file ; la décision de notifier est dans `src/state/notify.rs`. |
| `src/ui/` | Interface egui, un module par zone (rail, salons, discussion, connexion, fenêtre Paramètres). Lit le magasin et parle au reste par le trait `ui::Backend` ; ni réseau ni disque. |
| `tests/` | Faux serveur Discord local et tests de bout en bout sur des jeux de données fictifs. |
| `examples/preview.rs` | Aperçu hors ligne de l'interface (sans réseau ni jeton), avec capture d'écran. |

L'interface n'est redessinée que lorsqu'un événement arrive (réseau, saisie, thème modifié) : 0 % de CPU au repos.

## Attributions

- Images des emojis : [Twemoji](https://github.com/jdecked/twemoji) (version 16.0.1), © Twitter, Inc. et autres contributeurs, sous licence [CC-BY 4.0](https://creativecommons.org/licenses/by/4.0/). Elles ne sont pas incluses dans l'exécutable : téléchargées à la demande depuis `cdn.jsdelivr.net` (aucune donnée du compte n'y est envoyée) et gardées dans le cache disque d'images.
- Noms courts et catégories des emojis : paquet [discord-emoji](https://github.com/xCykrix/discord_emoji) 2.5.3 (licence MIT), table générée dans `src/state/emoji/table.rs`.
