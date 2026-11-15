# Vérification finale (15 novembre 2026)

Contrôle complet de `main` à `5b98f72` (« Fusion du lot C10 »), fait dans le
worktree `lot/verif`, sans toucher au code de l'application. Rien n'a été
lancé contre Discord : seuls `cargo clippy`, `cargo test` (faux serveurs sur
127.0.0.1) et l'exemple hors ligne `preview` ont tourné. Aucun jeton lu.

## 1. Résumé

| Contrôle | Résultat |
|---|---|
| `cargo clippy --all-targets -- -D warnings` | **propre**, 0 avertissement |
| `cargo test` complet (avec `state_table`) | **1 700 réussis, 0 échec**, 3 ignorés exprès |
| Banc `preview --bench`, une passe au calme | **tous les budgets tenus** : 0 redessin en 10 s de repos, images à 2,0 à 2,3 ms au 95e centile (budget 4 ms) |
| Audit visuel, 378 captures | **61 défauts** : 10 hauts, 27 moyens, 24 bas |
| Traduction (31 captures en anglais) | aucune clé manquante, aucun texte resté en français |
| Glyphes | aucun carré dans l'interface ; un seul dans la planche d'essai (U+180E) |

Aucun défaut ne casse l'écran principal en thème sombre à taille normale. Les
défauts hauts sont concentrés sur trois terrains peu parcourus pendant les
lots : le **thème clair**, la **fenêtre étroite** (800 × 600 et moins) et les
**thèmes personnels** (dégradé, image de fond), plus la fenêtre de partage
d'écran, trop haute pour une fenêtre de 720 points.

## 2. Tests

Commandes, dans `C:\fc-t\verif` :

```
cargo clippy --all-targets -- -D warnings     # 2 min, 0 avertissement
cargo test --no-fail-fast                     # tout, state_table compris
```

| Cible | Réussis | Échecs | Ignorés | Durée |
|---|---|---|---|---|
| Bibliothèque (`src/lib.rs`) | 1 438 | 0 | 3 | 963 s |
| `tests/e2e.rs` (faux serveur, 127.0.0.1) | 252 | 0 | 0 | 8 s |
| `tests/language.rs` | 1 | 0 | 0 | — |
| `tests/remote_auth.rs` | 9 | 0 | 0 | 2 s |
| `src/main.rs`, doctests | 0 | 0 | 0 | le projet n'a pas de doctest |
| **Total** | **1 700** | **0** | **3** | 16 min |

- Les 963 s de la bibliothèque viennent de `ui::state_table`
  (`every_state_keeps_the_token_somewhere`, `table_catches_the_old_behaviours`),
  qui passent tous.
- Ignorés, tous volontaires : `account::vault::tests::system_vault_round_trip`
  (écrirait dans le vrai Gestionnaire d'identifiants de Windows : à ne pas
  lancer), `net::inflate::tests::measure_ready` (mesure),
  `video::codec::tests_vp8::ecrit_jeu_rtp_vp8` (fabrique un jeu d'essai).
- Aucun échec, donc aucune relance : rien d'instable n'est apparu, alors même
  qu'une autre compilation tournait sur la machine pendant les tests.
- Aucun test n'ouvre de connexion ailleurs que sur 127.0.0.1 (les adresses de
  Discord présentes dans `tests/` sont des données de jeux d'essai).
- Rien n'a été corrigé : aucun test faux, aucune clé de traduction oubliée.

## 3. Banc

`preview --bench` (100 serveurs, 5 000 salons, fenêtre 1375 × 900, release).
Taille de l'aperçu : 27 093 504 octets, comme après C10.

| Phase (image, ms) | passe 1, au calme : méd. / p95 / max | passe 2, autre compilation en cours |
|---|---|---|
| repos (redessin forcé) | 1,56 / 2,00 / 3,60 | 2,93 / 3,96 / 25,9 |
| défilement | 1,65 / 2,18 / 2,98 | 3,04 / 7,94 / 19,9 |
| arrivée de 50 messages/s | 1,78 / 2,29 / 3,60 | 3,42 / 5,38 / 25,1 |
| changement de salon | 1,81 / 2,20 / 3,27 | 3,73 / 10,07 / 23,5 |
| historique | 1,79 / 2,68 / 15,0 | 1,95 / 3,61 / 83,4 |

| Autre mesure | passe 1 | passe 2 |
|---|---|---|
| Redessins en 10 s de repos | **0** | **0** |
| Présences injectées (1 000/s) ayant demandé une image | 0 sur 9 993 | 0 |
| Changement de salon : image du changement = image suivante | 8 sur 8 | 8 sur 8 |
| Fenêtre créée, première image | 221 ms, 227 ms | 482 ms, 494 ms |
| Mémoire privée après chargement, en fin de banc | 137 Mo, 192 Mo | 137 Mo, 196 Mo |
| Verdicts du banc | 8 tenus sur 8 | 5 sur 8 |

Lecture :

- **Passe 1, machine au calme : tout est tenu.** 0 redessin au repos, toutes
  les images sous 4 ms, maximum compris (3,60 ms), hors historique où une
  image isolée de 15 ms correspond à l'arrivée d'une page de 50 messages. Les
  chiffres recoupent ceux de `docs/mesures-c10.md` (repos 1,56 ms de médiane).
- **Passe 2 : trois budgets « image sous 4 ms » non tenus au 95e centile**
  (5,4, 7,9 et 10,1 ms). Une compilation d'une autre session a démarré pendant
  cette passe (`cargo clippy`, `rustc` visibles dans la liste des processus) :
  le démarrage a doublé lui aussi (482 ms contre 221). C'est la machine, pas
  l'application ; « 0 redessin au repos » reste tenu même ainsi.
- **Pas de troisième passe.** Vingt-cinq minutes d'attente n'ont pas donné de
  fenêtre calme : d'autres sessions compilaient et testaient sans arrêt
  (`cargo test --lib`, `cargo build --release`, puis d'autres aperçus). Une
  seule passe propre, donc : à refaire machine au repos si l'on veut une
  médiane, comme le demande `docs/mesures-c10.md`.

Non mesuré par le banc (aucune phase) : l'ouverture des paramètres, des
sélecteurs et des menus, la lecture d'une vidéo, un appel.

## 4. Audit visuel

### Méthode

378 captures de l'aperçu hors ligne, toutes regardées une par une (sept
relecteurs en parallèle, puis contrôle à la main des défauts hauts et d'une
quarantaine de scènes) :

| Dossier (`captures\verif\`) | Contenu | Captures |
|---|---|---|
| `A-sombre-1280` | toutes les scènes (celles de `tools\captures.ps1` plus 30 scènes récentes), thème Sombre, français, 1280 × 720 points | 165 |
| `B-cendre-1280`, `B-onyx-1280`, `B-minuit-1280`, `B-clair-1280` | 26 scènes clés dans les quatre autres thèmes | 104 |
| `C-sombre-800x600`, `C-sombre-1536x800` | 34 scènes clés en fenêtre étroite et large | 68 |
| `D-en-1280` | 31 scènes en anglais | 31 |
| `E-divers` | compléments : 640 × 480, 1920 × 1000, clair, demandes | 10 |

Une fenêtre de 1280 × 720 points donne une image de 1600 × 900 pixels (écran
à 125 %). Les captures ne sont pas suivies par git.

Limites : images factices (avatars en disques, emojis en carrés de couleur),
donc rien sur le rendu des vrais médias ; pas de survol sauf trois scènes ;
les menus, infobulles et animations ne sont vus qu'à travers les quelques
scènes qui les ouvrent ; une seule échelle d'écran (125 %).

### Défauts

Gravité : **haute** = illisible, cassé ou hors de la fenêtre ; **moyenne** =
se voit sans chercher ; **basse** = quelques pixels. « Vu » = contrôlé à la
main sur la capture ; sinon relevé par un relecteur sur un recadrage agrandi.

#### Gravité haute (10)

| # | Scène | Thème | Taille | Défaut | Fichier probable |
|---|---|---|---|---|---|
| 1 | `partage` | tous | 1280 × 720 et moins | La fenêtre « Partager » est plus haute que la fenêtre : titre rogné sous la barre de titre, boutons « Annuler » et « Partager » coupés par le bas. Vu | `ui/share_picker.rs` |
| 2 | `theme-degrade`, `fond-image` | thème à dégradé ou à image de fond | toutes | Un aplat noir opaque couvre le bas de la colonne des salons derrière le panneau utilisateur ; la partie du panneau sous le rail laisse voir le fond : le panneau paraît coupé en deux, la couture passe dans le nom. Vu. (Dans les thèmes unis, le même panneau n'a de contour qu'à droite : gravité basse.) | `ui/channels.rs` (panneau utilisateur), `ui/look.rs` |
| 3 | `appel`, `videos`, `sons` | Clair | toutes | Étiquettes des tuiles d'appel (« Alice », « Écran de Ève ») et des vidéos (« clip.mp4 · 11,8 Mo ») : texte sombre du thème sur pastille noire translucide en dur, presque illisible. Vu | `ui/video/tiles.rs`, `ui/player.rs` ou `ui/attachments.rs` |
| 4 | `modale` | Clair | toutes | Bouton principal désactivé (« Envoyer ») : texte blanc sur lavande très pâle, libellé invisible. Vu | `ui/widgets.rs` ou `ui/components.rs` (bouton principal désactivé) |
| 5 | `mon-compte` | Clair | toutes | Carte de mon compte : le statut choisi (« Inactif ») est écrit en blanc sur le gris clair de la sélection, illisible ; « Enregistrer » désactivé bleu sombre sur bleu sombre. La carte garde les couleurs sombres du profil, la sélection suit le thème. Vu | `ui/quick_profile.rs` |
| 6 | `fil`, `salon` | tous | 800 × 600 (fil), 640 × 480 (salon) | En-tête de discussion : les icônes sont dessinées sur la fin du titre (« Organisation de la sort » sous l'icône des fils, « général » collé à l'icône), la loupe sur le texte du champ de recherche rétréci. Vu | `ui/chat_head.rs` |
| 7 | `amis`, `demandes` | tous | 800 × 600 | Page Amis : le bouton vert « Ajouter un ami » disparaît (ou reste une languette verte sans texte), l'icône de droite est coupée par le bord. Vu | `ui/friends.rs` |
| 8 | `appel` | tous | 800 × 600 | Tuile de partage d'écran : « Regarder » se casse sur deux lignes (« Regarde / r ») et déborde de la tuile ; l'étiquette voisine est tronquée en « Écran d… ». Vu | `ui/video/tiles.rs` |
| 9 | `pu-notifications` | tous | 800 × 600 | Ligne « Volume des sons » : le bouton « Essayer » est coupé net (« Ess »), sans bord droit. Vu | `ui/settings/` (page Notifications, `settings_row`) |
| 10 | `recherche` | tous | 800 × 600 | Le panneau de résultats (320) est pris sur la discussion, qui tombe à environ 170 points (420 prévus) : texte recoupé très court, bloc de code cassé, zone de saisie qui passe sous le panneau. Vu | `ui/layout.rs`, `ui/search.rs` |

#### Gravité moyenne (27)

| # | Scène | Thème | Taille | Défaut | Fichier probable |
|---|---|---|---|---|---|
| 11 | `mon-compte` | tous | 1280 × 720 ; pire à 800 × 600 | Carte de mon compte coupée par le bas : la dernière ligne (« Changer de compte », « Copier l'identifiant ») est tranchée à mi-hauteur ; à 800 × 600 tout ce qui suit le champ de statut est invisible, sans défilement. Vu | `ui/quick_profile.rs` |
| 12 | `apparence-editeur`, `fichiers`, `hors-ligne`, `partage`, `pu-appareils`, `recherche`, `pu-raccourcis`, `pu-compte` | tous | toutes | Petits boutons écrasés : environ 24 de haut pour un texte de 16 qui touche les bords (« Appliquer », « Dupliquer », « Supprimer », « Réessayer », « Actualiser », « Aller au message », « Retirer », « Afficher »), à côté de contrôles de 40. Vu | petit bouton commun (`ui/util.rs`, `ui/components.rs`) |
| 13 | `profil-complet`, `profil`, `mp`, `sonnerie`, `rich-presence` | tous | toutes | Fondu de bas de liste de la mauvaise couleur : bande bleu-violet à bords nets, plus étroite que la fenêtre, posée sur la dernière ligne du profil (la couleur du haut du dégradé du profil au lieu du fond local). Vu | `ui/profile_full.rs`, `ui/profile.rs`, `ui/dm_profile.rs`, `ui/scroll.rs` |
| 14 | paramètres (utilisateur et serveur), `theme-degrade`, `fond-image` | tous, surtout Cendre | toutes | Même défaut ailleurs : le fondu du bas de la colonne de navigation des paramètres est un rectangle plus clair à bord net qui coupe la dernière entrée ; celui de la liste des membres est une boîte sombre visible sur un dégradé ou une image de fond. Vu | `ui/settings/shell.rs`, `ui/guild_settings/mod.rs`, `ui/members.rs` |
| 15 | toute fenêtre de dialogue (`modale`, `notifs-serveur`, `lien-externe`, `salons-creer`…) | tous, très visible en Clair et Cendre | toutes | Le voile de la fenêtre de dialogue assombrit tout sauf le bloc réduire / agrandir / fermer de la barre de titre, qui reste un rectangle clair à bord net en haut à droite. Vu | `ui/titlebar.rs` (voile) |
| 16 | `ecriture*`, `reponse`, `saisie` | tous | toutes | La pastille « ••• » (en train d'écrire) posée sur l'avatar rogne l'initiale, qui remonte : le « E » se lit « F ». Vu | `ui/motion.rs` (`typing_pill`), avatar à initiales |
| 17 | `theme-degrade`, `fond-image` | thèmes personnels | toutes | Pastilles de date (« mercredi 15 janvier 2025 11:00 »), code en ligne, bloc de code et champ de recherche en noir opaque, qui tranchent sur le dégradé. Vu | `ui/markdown.rs`, `ui/look.rs` |
| 18 | `ps-membres` | tous | toutes, large comprise | Ligne « Dorian » : le nom est rogné sans points de suspension par les pastilles de rôle, un rôle réduit à « … » est dessiné sur le nom ; la page reste plafonnée en fenêtre large alors que la place existe. Vu | `ui/guild_settings/members.rs` |
| 19 | `sonnerie` | tous | 1280 × 720 | Le bandeau « Appel entrant » recouvre la phrase de début de MP et masque le séparateur « NOUVEAU ». | `ui/call.rs` |
| 20 | `pu-langue` | tous | toutes | « Langue du compte Discord » : liste déroulante vide (aucun libellé quand le compte n'a pas de langue connue) ; la même phrase d'explication est écrite deux fois de suite et le titre de section répète le libellé. Vu | `ui/settings/synced.rs`, `ui/settings/pages.rs` |
| 21 | `notifs-serveur`, `lien-externe`, `pu-apparence` | Onyx | toutes | Boutons radio et cases non cochés en gris très sombre sans contour, fondus dans le noir ; champ « Nom » de « Mes thèmes » sans bordure visible, texte indicatif presque illisible. | `theme/mod.rs` (`apply`), `theme/presets.rs` (Onyx) |
| 22 | `mp`, `profil-complet` | Clair | toutes | Sur le fond sombre d'un profil : mention de la bio en bleu foncé du thème clair, croix de fermeture gris foncé sur disque sombre, peu lisibles. | `ui/profile.rs`, `ui/widgets.rs` (`close_button`) |
| 23 | `vocal`, `appel`, `profil-complet`, `galerie` | Minuit (géométrie d'avant la refonte) | toutes | Rangée caméra / partage / sons du panneau vocal écrasée (24 de haut, icônes minuscules) ; croix du profil complet décentrée, à cheval sur son disque ; étiquettes du forum sur deux lignes. | `ui/voice/panel.rs`, `ui/widgets.rs`, `ui/forum.rs` |
| 24 | `sons` | Cendre, Clair | toutes | Barre de titre de la fenêtre « Tableau de sons » (fenêtre d'egui) gris clair, plus claire que toute l'interface. | `ui/soundboard.rs`, `theme/mod.rs` |
| 25 | `evenement-creer` | tous | toutes | Champ de l'heure trop étroit : « 07:0 », dernier chiffre coupé. | `ui/events.rs` |
| 26 | `membre-roles` | tous | toutes | « Enregistrer les modifications » dépasse la largeur du contenu ; la bande sombre du pied s'arrête avant le bord de la fenêtre. | `ui/moderation.rs`, `ui/widgets.rs` (`modal`) |
| 27 | `ps-securite` | tous | toutes | La page s'ouvre défilée de 46 px, colonne de navigation et bouton ÉCHAP compris ; une bande de l'autre fond passe sous la colonne. | `ui/guild_settings/mod.rs` |
| 28 | `ps-role` | tous | toutes | Double filet sous les onglets Affichage / Permissions / Membres. | `ui/guild_settings/roles.rs` |
| 29 | `ps-emojis`, `ps-autocollants` | tous | toutes | Emoji animé : vignette de 24 calée en haut à gauche d'une case de 60 ; autocollants : auteur 3 px plus haut que le reste de sa ligne, tiret « — » isolé quand il n'y a pas de description. | `ui/guild_settings/` (emojis, autocollants) |
| 30 | `integration` | tous | toutes | Cartes de réponse voisines de hauteurs différentes (celle qui a un emoji et un sous-titre est plus haute). | `ui/welcome.rs` |
| 31 | `connexion-qr-lu` | tous | toutes | Ligne avatar + nom collée à gauche dans un panneau où tout le reste est centré. | `ui/qr_login.rs` |
| 32 | `appel-direct` | tous | 1280 × 720 | Quatrième vignette tranchée net par le bord du panneau, sans marge ni coin arrondi. | `ui/video/tiles.rs` |
| 33 | `salons-creer` | tous | toutes | « Vocal » et « Scène » portent la même icône (note de musique) alors que le jeu a un haut-parleur et une scène. | `ui/channel_settings/dialogs.rs` |
| 34 | `raccourcis` | tous | toutes | Bandes alternées du tableau arrêtées 120 à 150 px avant le bord droit de la fenêtre. | `ui/keys.rs` |
| 35 | `recherche` | tous | toutes | Texte d'un résultat replié à mi-largeur de sa carte (quatre lignes courtes dans une carte de 380). Vu | `ui/search.rs` |
| 36 | `entete-sujet` | tous | 800 × 600 | Le sujet du salon disparaît de l'en-tête, sans repli pour le lire. | `ui/chat_head.rs` |
| 37 | `profil` | tous | 1280 × 720 et moins | La carte de profil recouvre le bas du titre de l'en-tête et descend jusqu'au bord de la fenêtre sans marge ; son contenu est plus haut que la fenêtre. Vu | `ui/profile.rs` |

#### Gravité basse (24)

| # | Scène | Défaut | Fichier probable |
|---|---|---|---|
| 38 | `amis`, `groupe`, `salon-sans-images` | Rail : la pastille de mentions et son anneau rognent la deuxième initiale (« SA »). Vu | `ui/rail.rs` |
| 39 | `vocal`, `appel`, `sons` | Initiales : « B( » pour « Bob (vocal) » ; tuile d'écran aux initiales du libellé traduit (« ÉD » en français, « ÈS » en anglais) ; « CD » dans la barre de titre et « CDC » dans le rail pour le même serveur. | calcul des initiales (`ui/util.rs`), `ui/titlebar.rs` |
| 40 | `notifs-serveur` | « # général » décalé de 10 px par rapport aux autres lignes ; « Terminé » dépasse de 10 px la marge droite ; libellé collé à son bouton radio. | `ui/notif_settings.rs` |
| 41 | `profil-complet` | Liseré clair de 2 à 3 px le long du bord droit de la bannière. | `ui/profile_full.rs` |
| 42 | `parametres-serveur-modele`, `-widget`, `ps-roles`, `serveurs-inviter` | Bouton (40) aligné sur le haut d'un champ (49) au lieu d'être centré ; trois listes déroulantes de trois largeurs dans « Inviter ». | `ui/guild_settings/extras.rs`, `ui/invites.rs` |
| 43 | `pu-apparence`, `apparence-editeur` | Vignettes « Clair », « Minuit », « Contraste élevé » aux coins gauches carrés ; titre « Mes thèmes » gris et maigre à côté de titres blancs et gras. Vu | `ui/appearance.rs` |
| 44 | `profil-edition*` | Case à cocher seule, sans libellé, sous « Thème du profil » (profil de serveur) ; pied décalé de 10 px par rapport au contenu. | `ui/profile_edit.rs` |
| 45 | `parametres-serveur-elagage` | Boutons radio « 7 jours » et « 30 jours » presque collés. | `ui/guild_settings/extras.rs` |
| 46 | `comptes-retrait`, `comptes-doublon`, `comptes-migration`, `comptes-ancien` | « Oui » et « Non » collés ; boutons de largeurs différentes dans la même colonne ; carte collée à la barre de titre quand il y a trois comptes. | `ui/accounts.rs`, `ui/login.rs` |
| 47 | `bienvenue` | Titre centré au-dessus d'un contenu aligné à gauche. | `ui/welcome.rs` |
| 48 | pages `pu-*` | La colonne de navigation ne défile pas jusqu'à la page ouverte (sept pages sans entrée choisie visible) ; premier titre de section qui répète le titre de la page (« Raccourcis clavier », « Mode streamer ») ; filet nu sous le titre (Notifications, vue d'un salon) ; libellé « À propos de moi » gris parmi des libellés blancs. | `ui/settings/` |
| 49 | `pu-comptes-lies`, `pu-activite`, `ps-bannissements` | Valeur tronquée alors que la ligne a la place ; textes d'état à la taille du corps ; champ de recherche qui ne finit pas au bord droit des boutons. | `ui/settings/`, `ui/guild_settings/bans.rs` |
| 50 | `sons` (large), `sons` (ligne « bob_fictif ») | La fenêtre du tableau de sons se pose à cheval sur la liste des membres en fenêtre large ; icône de fin de ligne minuscule et illisible. | `ui/soundboard.rs`, `ui/channels.rs` |
| 51 | `sans-historique` | Avertissement sans marge ni cadre, 2 px avant la colonne des messages. | `ui/chat.rs` |
| 52 | `pu-apparence` en anglais | « Theme colours », « Accent colour » (orthographe britannique, dix occurrences dans `i18n/perso.rs`) alors que le reste de l'interface écrit « color ». | `src/i18n/perso.rs` |
| 53 | `galerie` | Cartes décalées de 10 px par rapport à la barre d'outils ; une seule carte, démesurée, à 800 × 600. | `ui/gallery.rs`, `ui/forum.rs` |
| 54 | `conference`, `appel` à 800 × 600 | « 4 personnes » collé au bouton voisin ; bouton « Découvrir » du rail coupé par le panneau vocal ; ligne « Scène » tranchée sans fondu. | `ui/stage.rs`, `ui/voice/panel.rs` |
| 55 | Minuit | Spoiler et fond du code à peine distincts du noir ; bouton emoji du statut plus petit que son champ. | `theme/presets.rs` (Minuit) |
| 56 | colonne des MP | « Demandes de messa… » tronqué dans une colonne de 240. Vu | `ui/requests.rs`, `ui/groups.rs` |
| 57 | `rich-presence`, `sonnerie` | Titres de section du profil collés au texte qui précède ; « Voir le profil complet » en texte nu sous un bouton pleine largeur. | `ui/profile.rs`, `ui/dm_profile.rs` |
| 58 | `lien-externe`, `moderation-membre`, `pseudo-ami`, `serveurs-ajouter` | Pieds de fenêtre de styles différents : « Retour » en bouton, « Annuler » en texte nu ; « Créer un serveur » sans croix ni pied. | `ui/links.rs`, `ui/moderation.rs`, `ui/guild_create.rs` |
| 59 | `glyphes` | U+180E (séparateur mongol, censé être invisible) rendu en rectangle pointillé ; diacritiques empilés qui débordent sur les lignes voisines dans la colonne de droite. | `ui/fonts.rs` |
| 60 | `parures` | Sous-texte gris peu contrasté sur les rayures claires d'une plaque de nom. | `ui/cosmetics.rs`, `ui/members.rs` |
| 61 | outil d'aperçu | `--scene demandes` seule ne montre pas les demandes (il faut `--view amis --scene demandes`) ; `tools\captures.ps1` ne connaît pas les 30 scènes ajoutées depuis (vidéos, menus, PERSO, notifications…) ni les thèmes. | `examples/preview.rs`, `tools/captures.ps1` |

À confirmer, non comptés : aucun état de survol visible sur la croix de la
barre de titre dans `v1-barre-fermer` (le pointeur simulé n'atteint peut-être
pas la zone gérée par Windows) ; dernier mot gris dans `saisie-longue` ;
liste des membres plus étroite de 14 px en anglais ; zone vide sous le dernier
message de `fil` en Minuit.

### Suivi des corrections (lot FV)

Corrigé sur la branche `lot/fv` ; « revu » = capture `preview` regardée après correction, dans la scène de l'audit.

- [x] Hauts : 1 à 10, tous corrigés et revus (le 5 avait déjà disparu avec la refonte de la carte du compte).
- [x] Moyens corrigés et revus : 11, 13, 15, 17 (code, date et bloc de code en voile ; le champ de recherche reste opaque), 20, 21, 25, 26, 27, 28, 32, 34, 35, 36, 37.
- [x] Moyens corrigés, non revus à l'écran : 12, 14, 16, 18, 22, 24, 33.
- [x] Moyens restants après le lot FV : voir le lot FV2 ci-dessous.
- [x] Bas corrigés : 52 (« color » partout en anglais), 58 (« Annuler » et « Retour » en texte, « Ajouter un serveur » avec titre et croix).
- [x] Bas restants après le lot FV : voir le lot FV2 ci-dessous.

Garde-fous ajoutés : tests de contraste parcourant tous les préréglages pour le bouton principal désactivé, les étiquettes posées sur une image ou une vidéo et les cases non cochées (`ui/widgets.rs`) ; gabarit unique `widgets::modal` / `widgets::sheet` pour les fenêtres, `widgets::compact` pour les petits boutons.

### Suivi des corrections (lot FV2)

Seconde passe, sur la branche `lot/fv2`. « Revu » = capture `preview` regardée après correction (1100 × 720 points à 125 %, thème Sombre sauf mention).

Moyens :

- [x] 23 corrigé et revu en Minuit : rangée caméra / partage / sons du panneau vocal à la hauteur d'un bouton (`widgets::button_height`), croix du profil complet sur un disque jamais plus petit qu'elle. Étiquettes du forum sur deux lignes : disparu (`galerie` en Minuit, une ligne).
- [x] 29 corrigé et revu : l'auteur d'un autocollant finit la ligne de détail (« wave · Un salut · bob »), plus de tiret isolé (emojis, autocollants, sons). Vignette de l'emoji animé : pas un défaut, l'image fictive de l'aperçu est un point qui tourne sur un fond transparent ; elle occupe bien toute la case.
- [x] 30 corrigé et revu : cartes d'une même question de même hauteur, dessinées à la main (survol, anneau de focus et rôle de case à cocher compris).
- [x] 31 corrigé et revu (`--view login --scene qr-lu`) : avatar et nom centrés comme le reste du panneau.
- [ ] 19 laissé : bandeau d'appel entrant au milieu, choix volontaire d'un lot antérieur, couvert par un test.

Bas :

- [x] 38 corrigé et revu (`--view groupe`) : la pastille de mentions du rail est à cheval sur le coin de l'icône, la deuxième initiale reste lisible.
- [x] 39 corrigé et revu : initiales prises sur la première lettre ou le premier chiffre de chaque mot (« BV » pour « Bob (vocal) »), tuile d'écran aux initiales de la personne. Laissé : deux initiales dans la barre de titre contre trois dans le rail (pastille de 24, trois lettres n'y tiennent pas).
- [x] 40 corrigé et revu : « # général » à l'aplomb des autres lignes, pied des fenêtres aux marges du contenu (« Terminé » finit au bord droit), libellé d'une case ou d'un bouton radio écarté comme celui d'un interrupteur (`icon_spacing`).
- [x] 41 corrigé et revu (recadrage agrandi) : la bannière du profil complet va jusqu'au bord droit (un contenu de la zone défilante élargissait la fenêtre de deux points).
- [x] 42 corrigé et revu (`parametres-serveur-widget`, `serveurs-inviter`) : rangée « champ et bouton » commune (`widgets::field_row`), bouton centré sur le champ ; trois listes de même largeur dans « Inviter ». `roles` revu aussi ; `parametres-serveur-modele` : même rangée, non regardé.
- [x] 43 corrigé et revu (`pu-apparence`) : bande du rail des vignettes de thème arrondie à gauche ; « Mes thèmes » et « Thèmes fournis » en demi-gras blanc.
- [x] 44 disparu : l'édition du profil a été refaite (plus de case seule, plus de pied décalé), revu sur `profil-edition-serveur`.
- [x] 45 corrigé et revu : « 7 jours » et « 30 jours » écartés ; même écart pour « Avatar / Bannière » (profils) et la qualité vidéo d'un salon vocal.
- [x] 46 corrigé, revu sur `--view retrait` : « Oui » et « Non » écartés, boutons des notices larges comme la colonne, carte de connexion qui défile au lieu de se coller à la barre de titre. `migration` et `doublon` revus (boutons des notices larges comme la colonne) ; `ancien-fichier` non regardé.
- [x] 47 corrigé et revu : titre de « Bienvenue » à gauche, dans le gabarit commun.
- [x] 48 corrigé : la colonne des paramètres de serveur amène l'entrée ouverte en vue (revu sur `moderation-securite` et `parametres-serveur-elagage`) ; celle des paramètres utilisateur le faisait déjà depuis le lot réglages. Premier titre de section qui ne répète plus le titre de la page (« Raccourcis modifiables », « Pendant une diffusion »), plus de filet nu sous le titre d'une page (revu sur `moderation-bannissements`), « À propos de moi » de l'encre des libellés voisins ; les trois revus sur `pu-streamer`, `pu-raccourcis`, `pu-apparence` et `pu-profil`.
- [x] 49 corrigé : valeur des comptes liés réduite au nom (revu), textes d'état de l'activité en petit (revu) ; champ de recherche des bannis qui finit au bord des boutons (revu).
- [x] 50 corrigé pour la position, revu sur `sons` : le tableau de sons se pose dans le coin de la discussion, plus sur la liste des membres (revu aussi à 1536 × 800). Laissé : la main levée de fin de ligne, à la taille des autres marques (12), reste peu lisible ; il faudrait une icône dessinée pour cette taille.
- [x] 51 corrigé et revu (`--view sans-historique`) : l'avertissement « sans historique » tient dans les marges de la colonne des messages.
- [x] 53 corrigé et revu (`galerie` à 800 × 600) : cartes de la galerie sous la barre d'outils, deux colonnes dès que deux cartes de 180 tiennent.
- [x] 54 en partie : « 4 personnes » cède la place avant d'être collé aux boutons (revu à 800 × 600). Laissés : le bas du rail et de la liste des salons passe sous le panneau flottant sans fondu, comme dans Discord (plus aucun fondu dans l'application depuis le lot FV, défauts 13 et 14).
- [x] 55 disparu pour le spoiler et le fond du code (visibles en Minuit, garde-fou de contraste `spoiler/bg2`) ; bouton emoji du statut : non revu.
- [x] 56 corrigé et revu : « Demandes de mes… » garde son compte, en pastille au bout de la ligne quand « libellé (n) » ne tient pas.
- [x] 57 disparu avec la refonte de la carte de profil (revu sur `rich-presence` : sections espacées, « Voir le profil complet » en bouton).
- [ ] 59 laissé : U+180E est dessiné par une police de repli du système qui lui donne un glyphe visible (le remplaçant invisible ne sert que pour un caractère absent de toutes les polices) ; il faudrait filtrer le texte avant la mise en page. Les diacritiques empilés débordent par nature.
- [x] 60 corrigé et revu : sous-texte d'une ligne à plaque dans l'encre du texte.
- [x] 61 corrigé : `--scene demandes` ouvre seule la page ; `tools\captures.ps1` connaît les scènes ajoutées depuis et prend `-Themes`. Le script lui-même n'a pas été relancé.

Cohérence :

- [x] Colonnes de navigation des paramètres de serveur et de salon : mêmes lignes que les paramètres utilisateur (`settings::nav::entry_row`, icônes, titres en casse normale par `shell::nav_title`, filets, entrée ouverte sur fond arrondi, « Supprimer le salon » en rouge avec sa corbeille). `shell::nav_group` et l'ancien `nav_row` des salons sont retirés. Revu sur `parametres-serveur` et `salons`.
- [x] Fenêtres au gabarit commun : « Parcourir les salons » et « Codes de vérification » (`widgets::sheet`), « Bienvenue » (`sheet`), intégration et « Pièce jointe » (`widgets::modal`, pied à droite). Toutes revues ; deux scènes d'aperçu ajoutées pour celles qui n'en avaient pas (`parcourir`, `fichier-details`).

### Ce qui est propre

Écran principal (salon, MP, amis) dans les thèmes Sombre, Cendre et Onyx à
1280 × 720 et à 1536 × 800 ; barre de survol, menus contextuels, sélecteur
d'emojis et de GIF, lecteur vidéo, galerie, connexion et QR, bandeau des
nouveaux messages, en-têtes, fils, forum, sondages, page Amis et demandes,
paramètres du serveur hors points ci-dessus. Aucun contenu étiré sans borne en
fenêtre large. Aucune clé brute, aucun texte de l'interface resté en français
en anglais ; dates et durées au format anglais.

### Captures les plus parlantes

Dans `captures\verif\` (dossier de travail local) :

1. `A-sombre-1280\partage.png` (défaut 1)
2. `A-sombre-1280\theme-degrade.png` (défauts 2, 14, 17)
3. `B-clair-1280\appel.png` (défaut 3)
4. `B-clair-1280\mon-compte.png` (défauts 5, 11)
5. `E-divers\modale-clair.png` (défauts 4, 15)
6. `C-sombre-800x600\fil.png` (défaut 6)
7. `C-sombre-800x600\recherche.png` (défauts 10, 12, 35)
8. `C-sombre-800x600\appel.png` (défaut 8)
9. `A-sombre-1280\profil-complet.png` (défauts 13, 41)
10. `A-sombre-1280\ps-membres.png` (défaut 18)

## 5. Ce qui n'a pas pu être vérifié

- Tout ce qui demande le vrai compte : les requêtes non documentées des lots
  (liste dans `docs/ecarts-restants.md`, chapitre 5), les vrais avatars,
  emojis, vidéos et autocollants.
- L'exécutable `ferrocord` lui-même (jamais lancé) : démarrage réel, zone de
  notification, mise à jour, barre de titre intégrée en usage (ancrage,
  plusieurs écrans, autres échelles que 125 %).
- L'onglet « Pour vous » avec du contenu : la scène `pour-vous` de l'aperçu
  est vide (« Rien de neuf pour vous »).
- Les états de survol, les infobulles, les animations et la navigation au
  clavier, hors des quelques scènes qui les montrent.
- Le son, la caméra, le partage d'écran et un appel réels.
- Android (hors mission) ; `tools\android.ps1 -Check` n'a pas été lancé.
- La comparaison côte à côte avec une capture du vrai Discord : aucune n'était
  disponible, les écarts d'aspect sont jugés de mémoire.
