# Distribuer Ferrocord : installeur et mises à jour

Un seul fichier à donner : `Ferrocord-<version>-x64.exe`. Il s'installe
lui-même, puis se met à jour depuis les « Releases » d'un dépôt GitHub
**public**. Rien ici ne publie quoi que ce soit à ta place : `tools\publier.ps1`
prépare les fichiers et affiche la commande, c'est toi qui la lances.

Depuis le lot « gl », l'exécutable installé pose à côté de lui, à son lancement,
deux bibliothèques qu'il embarque : `libEGL.dll` et `libGLESv2.dll` (ANGLE, le
rendu par Direct3D 11 ; `assets/angle/LISEZMOI.md`), plus un témoin
`angle.essai` le temps du démarrage. Il n'y a toujours qu'un fichier à publier ;
la désinstallation retire ces fichiers. Un exécutable lancé hors de son dossier
d'installation ne pose rien et dessine par le pilote OpenGL, comme avant.

Ce que garantit cette pose (lot « gl2 », `src/rendu/angle.rs`, tout est couvert
par des tests sur un dossier temporaire et un registre simulé) :

- **Écriture atomique** : chaque bibliothèque est écrite dans un fichier à part
  (`<nom>.<pid>.part`), vidée sur le disque, puis renommée. Jamais de fichier à
  moitié écrit ; deux exemplaires lancés ensemble ne s'écrasent pas ; les restes
  d'une pose interrompue sont balayés à la pose suivante et à la désinstallation.
- **Vérification avant chargement** : à chaque lancement, les deux fichiers sont
  relus en entier et comparés au sha256 embarqué dans l'exécutable, puis chargés
  **par leur chemin complet** (`LoadLibraryExW`, dépendances cherchées seulement
  à côté d'eux et dans System32 ; le compilateur de nuanceurs `d3dcompiler_47.dll`
  est pris dans System32). Ils sont tenus ouverts, écriture et effacement
  interdits, de la vérification au chargement. Un fichier altéré n'est pas
  chargé : il est effacé, la fenêtre s'ouvre sur le pilote OpenGL, et le
  lancement suivant le repose.
- **Dossier non inscriptible, fichier en service, disque plein** : pas d'erreur
  montrée, la fenêtre s'ouvre sur le pilote OpenGL.
- **Mise à jour** : si la version d'ANGLE embarquée change, les fichiers sont
  remplacés au premier lancement de la nouvelle version.
- **Témoin de démarrage** `angle.essai` : il contient la version de Ferrocord et
  reste ouvert sans partage pendant le démarrage. Fermer la fenêtre pendant le
  démarrage le retire (ce n'est pas un échec) ; un second exemplaire lancé au
  même moment le voit « occupé », pas « raté ». S'il reste après un démarrage
  qui a planté, les lancements suivants **de la même version** passent par le
  pilote OpenGL ; la version suivante de Ferrocord réessaie ANGLE. Le repli est
  noté dans le journal technique (`rendu_repli_opengl`), pas dans l'interface.
- **Rendu perdu en cours de route** (pilote graphique réinitialisé ou mis à
  jour, carte retirée) : Ferrocord se relance de lui-même, comme après une mise
  à jour. Si cela arrive moins de vingt secondes après le lancement, la relance
  se fait sur le pilote OpenGL et ANGLE attend la version suivante (pas de
  boucle de relances).
- **Carte graphique** : l'exécutable installé demande à Windows la carte économe
  pour lui-même (Paramètres › Système › Écran › Graphiques), seulement si rien
  n'y est déjà choisi : un choix de l'utilisateur n'est jamais remplacé, et la
  désinstallation ne retire que cette valeur-là. Le partage d'écran et la caméra
  n'en dépendent pas : l'encodeur H.264 matériel est demandé à Media Foundation
  parmi tous les encodeurs matériels de la machine (pas seulement ceux de la
  carte qui dessine), il reçoit des images en mémoire ordinaire, et si aucun ne
  s'ouvre, l'encodeur logiciel OpenH264 prend le relais (`src/video/codec`).

## Avant la première publication (une seule fois, dans cet ordre)

1. **Créer le dépôt GitHub public** (vide suffit, il ne sert qu'aux
   Releases) : sur github.com, ou `gh repo create <toi>/<dépôt> --public`.
   Un dépôt privé ne marche pas : le téléchargement demanderait un jeton.
2. **Créer la paire de clés de signature.** La clé privée va dans un dossier
   **hors du dépôt** (le script et l'outil refusent tout dossier suivi par
   git, et ne remplacent jamais une clé existante) :

   ```powershell
   powershell -ExecutionPolicy Bypass -File tools\publier.ps1 -CreerCle -ClePrivee "$env:USERPROFILE\Documents\cles\ferrocord-maj.pk8"
   ```

   La clé publique est écrite dans `src\install\update\cle_publique.txt`.
   Sauvegarde la clé privée sur un support à part (clé USB chiffrée, coffre
   de mots de passe).
3. **Renseigner le dépôt** : une ligne `propriétaire/dépôt` dans
   `src\install\update\depot.txt`. C'est le seul endroit ; l'adresse par
   défaut des mises à jour en découle
   (`https://github.com/<propriétaire>/<dépôt>/releases/latest/download/ferrocord-manifeste.json`).
   Vide : pas d'adresse par défaut, donc aucune vérification automatique.
4. **Commiter** ces deux fichiers (`cle_publique.txt`, `depot.txt`) : ils
   sont intégrés à l'exécutable à la compilation.

## Fabriquer une version

1. Augmenter `version` dans `Cargo.toml` (une version déjà étiquetée est
   refusée ; une installation n'accepte jamais une version égale ou plus
   ancienne que la sienne), commiter.
2. Lancer :

   ```powershell
   powershell -ExecutionPolicy Bypass -File tools\publier.ps1 -ClePrivee "$env:USERPROFILE\Documents\cles\ferrocord-maj.pk8"
   ```

   (ou définir `FERROCORD_CLE_PRIVEE` une fois pour toutes). Le script
   vérifie que l'arbre git est propre et que la clé privée correspond à la
   clé publique intégrée, compile (`cargo build --release`), puis écrit dans
   `dist\<version>\` (ignoré par git) :

   | Fichier | Rôle |
   |---|---|
   | `Ferrocord-<version>-x64.exe` | l'installeur et l'application, en un fichier |
   | `ferrocord-manifeste.json` | manifeste signé (Ed25519) : version, canal, adresse, taille et SHA-256 de l'exécutable, notes courtes |
   | `SHA256SUMS` | empreintes des deux fichiers ci-dessus, au format de `sha256sum` |
   | `NOTES.md` | notes de version : `git log` depuis la dernière étiquette (à relire, elles seront publiques) |

3. Publier, avec la commande que le script affiche :

   ```powershell
   git push origin HEAD
   gh release create v<version> --repo <toi>/<dépôt> --target <commit> --title "Ferrocord <version>" --notes-file dist\<version>\NOTES.md dist\<version>\Ferrocord-<version>-x64.exe dist\<version>\ferrocord-manifeste.json dist\<version>\SHA256SUMS
   ```

   La version doit être publiée normalement : ni brouillon (`--draft`) ni
   préversion (`--prerelease`), sinon `releases/latest` ne la désigne pas.
   Ne renomme pas les fichiers : le manifeste signé contient l'adresse
   exacte de l'exécutable (`…/releases/download/v<version>/Ferrocord-<version>-x64.exe`).

## Ce que voit l'utilisateur final

- **Téléchargement** : Windows SmartScreen affiche « Windows a protégé votre
  ordinateur » (Informations complémentaires > Exécuter quand même), et le
  navigateur peut prévenir que le fichier est rarement téléchargé.
  L'exécutable n'a pas de signature Authenticode : il faudrait un certificat
  de signature de code, payant, hors périmètre.
- **Premier lancement** d'un exécutable qui n'est pas installé : une petite
  fenêtre propose « Installer » (copie dans
  `%LOCALAPPDATA%\Programs\Ferrocord`, raccourci du menu Démarrer, entrée
  dans « Applications installées » ; en option, raccourci sur le bureau et
  démarrage avec Windows) ou « Lancer sans installer » (mode portable).
  Aucun droit d'administrateur. La question n'est posée qu'une fois
  (`%APPDATA%\ferrocord\installation.json`), jamais pour un lien
  `discord://` ou un démarrage réduit, jamais depuis un dossier de
  compilation (`target\…`).
- **Mises à jour** (Paramètres > Ferrocord) : « Vérifier automatiquement »
  (activé dès qu'une adresse existe) vérifie au lancement puis une fois par
  jour au plus, jamais deux fois à moins d'une heure d'écart ; « Rechercher
  maintenant » vérifie tout de suite. Une version plus récente est annoncée
  par une notification et affichée avec ses notes ; « Télécharger et
  vérifier » la télécharge en arrière-plan (signature du manifeste, taille
  et SHA-256 du fichier), puis « Redémarrer pour mettre à jour » l'installe.
  Rien ne redémarre tout seul : sans clic, elle s'installe au lancement
  suivant. Si la nouvelle version n'affiche pas sa première image, le
  lancement d'après remet l'ancienne et ne la propose plus.
- **Désinstallation** : Paramètres de Windows > Applications installées >
  Ferrocord. Réglages et cache sont gardés, sauf si la case est cochée. Les
  jetons enregistrés restent dans le Gestionnaire d'identifiants : se
  déconnecter dans Ferrocord avant de désinstaller pour les retirer.

Ligne de commande, pour les scripts :

| Commande | Effet |
|---|---|
| `ferrocord.exe --installer [--silencieux] [--bureau] [--demarrage]` | installe sans poser de question ; sans `--silencieux`, lance ensuite l'application. Code de sortie 0, ou 1 en cas d'échec (Ferrocord ouvert, par exemple) |
| `ferrocord.exe --desinstaller [--silencieux] [--supprimer-donnees]` | désinstalle ; sans `--silencieux`, une fenêtre demande confirmation |
| `ferrocord.exe --portable` | lance sans proposer l'installation |

`tools\installer.ps1` reste, pour qui compile depuis les sources : il lance
`target\release\ferrocord.exe --installer --silencieux`.

## Changer l'adresse des mises à jour

- **Pour tout le monde** : changer `depot.txt`, publier une version (par
  l'ancien dépôt, pour que les installations existantes la reçoivent ; les
  suivantes viendront du nouveau).
- **Sur une machine** : dans `%APPDATA%\ferrocord\local.toml`,

  ```toml
  [mises_a_jour]
  manifeste = "https://exemple.org/ferrocord/ferrocord-manifeste.json"
  ```

  Cette adresse l'emporte sur celle du dépôt. Le manifeste doit toujours
  être signé par la clé dont l'exécutable intègre la clé publique.

Redirections suivies : seulement en HTTPS vers `github.com`,
`objects.githubusercontent.com` et `release-assets.githubusercontent.com`
(port 443), ou vers l'hôte même qui redirige (`REDIRECT_HOSTS`,
`src\install\update\fetch.rs`). Si GitHub change d'hôte de stockage, la
vérification échouera avec « redirection vers un hôte non autorisé » : il
faudra ajouter l'hôte à cette liste et publier la version à la main.

## Révoquer ou renouveler la clé

La clé publique est dans l'exécutable : une installation n'accepte que les
manifestes signés par la clé qu'elle connaît. Donc :

- **Renouvellement prévu** (clé privée encore sûre) : créer la nouvelle
  paire dans un autre fichier (`-CreerCle` refuse d'écraser ; il réécrit
  `cle_publique.txt`), commiter, puis publier une version **signée par
  l'ancienne clé** qui contient la nouvelle clé publique : signer son
  manifeste à la main avec l'ancienne clé (`tools\signer signe --privee
  <ancienne> …`, mêmes arguments que ceux de `publier.ps1`, qui, lui,
  refuserait puisque la clé privée ne correspond plus à la clé publique
  intégrée). Les installations la reçoivent, et acceptent ensuite la nouvelle
  clé. Détruire l'ancienne clé privée une fois tout le monde passé.
- **Clé perdue** : plus aucune mise à jour automatique n'est possible. Créer
  une nouvelle paire, publier ; chaque utilisateur doit télécharger et
  réinstaller à la main (réinstaller par-dessus garde les réglages).
- **Clé volée** : même chose, sans attendre ; et retirer des Releases tout
  fichier que tu n'as pas publié. Le voleur ne peut nuire qu'en contrôlant
  aussi ce que sert l'adresse des mises à jour (ton compte GitHub).

## Ce que les mises à jour envoient

`GET` du manifeste, puis `GET` de l'exécutable sur un clic : seulement vers
l'adresse configurée et ses redirections autorisées. Jamais vers Discord ;
ni jeton, ni témoin, ni `Referer`, ni identifiant de l'utilisateur ou de la
machine ; `User-Agent: Ferrocord/<version>`. Détail :
`docs/passerelle-utilisateur.md`, « Mises à jour de Ferrocord ».
