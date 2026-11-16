# Design de Ferrocord

Direction visuelle de l'interface de bureau (lot E, novembre 2026). Tout ce qui
suit est porté par les jetons de `src/theme/` : ce document dit quelles valeurs
ils prennent et pourquoi, et quelles règles suivent les composants. Aucun écran
n'écrit une couleur, un rayon ou un espacement en dur.

## 1. Parti pris : chez soi dans Discord

Ferrocord est un client tiers pour Discord : qui l'ouvre doit s'y retrouver
sans chercher. L'interface reprend donc la disposition et les conventions du
client officiel (où sont les choses, comment se lit un état), avec des moyens
qui restent les nôtres :

1. **Mêmes repères.** Rail des serveurs à gauche, colonne des salons, panneau
   utilisateur en bas, discussion, membres à droite ; paramètres en plein
   écran, catégories à gauche et « ÉCHAP » à droite.
2. **Mêmes signaux.** Gris neutres-froids du plus sombre (rail) au plus clair
   (discussion), accent bleu-violet, vert pour ce qui est actif, rouge pour le
   danger et les mentions, pilule blanche au bord du rail.
3. **Rien de copié.** Ni logo, ni image, ni icône du client officiel, ni sa
   police : les pictogrammes sont dessinés par le code (`theme/icons.rs`), le
   texte est en Figtree (police libre, licence OFL) ou en Segoe UI du système.

## 2. Couleurs

Les noms sont ceux des jetons de `[couleurs]` dans `theme.toml`. Le thème
sombre par défaut garde la clé `rouille` dans les fichiers (compatibilité) ; il
s'affiche « Sombre ».

### Surfaces

Quatre niveaux, du plus sombre au plus clair. Deux niveaux voisins se
distinguent sans trait ; un filet (`separator`) ne sépare que les grandes zones.

| Jeton | Sombre | Clair | Rôle |
|---|---|---|---|
| `bg0` | `#1a1b1e` | `#e3e5e8` | rail, champs de saisie, écran de connexion |
| `bg1` | `#232428` | `#f2f3f5` | colonne des salons, membres, cartes (embeds, fichiers), colonne des paramètres |
| `bg2` | `#2b2d31` | `#ffffff` | discussion, pages de paramètres, fenêtres |
| `bg3` | `#34363c` | `#ebedef` | zone de saisie, barre d'actions, notifications |
| `row_hover` | `#31333a` | `#e1e3e6` | ligne survolée |
| `selected` | `#393b41` | `#d7d9dc` | ligne ouverte |
| `pill` | `#2b2d31` | `#ffffff` | pastille du rail sans icône |
| `border` | `#3d3f46` | `#d0d2d6` | contour des cartes et des champs, filets des paramètres |
| `separator` | `#35373d` | `#e1e2e4` | filet entre deux zones |
| `hover` | noir à 9 % | noir à 3 % | message survolé |
| `scrim` | noir à 70 % | noir à 45 % | voile sous une fenêtre modale |
| `shadow` | noir à 43 % | noir à 18 % | ombre portée |

Le panneau utilisateur (et le panneau vocal) est à mi-chemin de `bg0` et de
`bg1` (`Tokens::panel_bg`) : il se détache de la colonne sans filet.

### Texte

| Jeton | Sombre | Clair | Rôle |
|---|---|---|---|
| `text_strong` | `#f7f8f9` | `#060607` | titres, noms, ligne ouverte ou non lue |
| `text` | `#dbdee1` | `#313338` | texte des messages |
| `muted` | `#a9aeb7` | `#4e5058` | texte secondaire, lignes au repos, descriptions |
| `faint` | `#a2a7b0` | `#5c5e66` | heures, légendes, icônes au repos |

### Accent et états

| Jeton | Sombre | Clair | Rôle |
|---|---|---|---|
| `accent` | `#5966f2` | `#5865f2` | action principale, serveur ouvert ou survolé, sélection |
| `on_accent` | `#ffffff` | `#ffffff` | texte posé sur l'accent et sur l'aplat rouge |
| `link` | `#00a8fc` | `#0068e0` | liens |
| `danger` | `#fb8389` | `#ad2222` | erreurs, suppression, « Déconnexion » (texte) |
| `focus` | `#00a8fc` | `#0b5cd5` | contour de l'élément qui a le clavier |
| `online` / `idle` / `dnd` / `offline` | `#23a55a` `#f0b232` `#f23f43` `#80848e` | `#248046` `#a87708` `#c62828` `#747780` | présence ; `online` : interrupteur activé, « + » du rail |

`danger` est réglé pour être lu comme texte. Un aplat rouge qui porte du texte
(bouton destructeur, pastille de mentions, « EN DIRECT ») prend
`Tokens::danger_fill` : le même rouge, foncé jusqu'à ce que le texte
`on_accent` y atteigne 4,5:1.

Les noms d'auteurs sans couleur de rôle prennent l'une des huit teintes de
`authors`, toutes à 4,5:1 au moins sur `bg2`.

### Contraste

WCAG 2.1 AA, vérifié par les tests `presets_meet_wcag_aa` et
`presets_meet_contrast` pour chaque préréglage : 4,5:1 pour tout texte de
taille courante (y compris `muted`, `faint`, `danger`, liens et auteurs) sur
chaque fond où il se pose ; 3:1 pour les éléments graphiques (pastilles de
présence, contour du clavier, accent sur le fond). C'est ce qui écarte un peu
les valeurs de celles du client officiel : fonds un cran plus sombres, gris de
texte un cran plus clairs.

Les autres préréglages suivent : Clair (blanc, gris clairs, même accent),
Minuit (noir pur, même accent), Contraste (noir, blanc, ambre), Compact et
Aéré (variantes de densité du thème sombre).

## 3. Typographie

Police : **Figtree** embarquée (`assets/fonts/Figtree-wght.ttf`, police
variable, 62 712 octets, licence OFL 1.1 dans `assets/fonts/OFL.txt` et dans
les licences de l'application), un seul fichier pour les graisses 400, 500, 600
et 700 (axe `wght` instancié par egui) ; pas d'italique dans ce fichier,
l'italique est penché par egui. Segoe UI du système (Roboto sur Android) reste
derrière elle pour les caractères qu'elle n'a pas, et la remplace en entier avec
le jeton `typographie.family = "systeme"` (Paramètres › Apparence › Police ;
`"figtree"` par défaut). Sans demi-grasse, le gras la remplace ; sans gras, la
couleur `text_strong`. Code : Consolas, redescendue d'une fraction de sa taille
(`ui/fonts.rs`) pour rester sur la ligne de base du texte.

**Polices de nom.** Un nom au style de nom de Discord
(`display_name_styles.font_id`) est dessiné dans la police que Discord
utilise, quand elle est libre : quatorze polices sous SIL Open Font License 1.1
sont embarquées (`assets/fonts/noms/<famille>/`, fichier de licence à côté de
chaque police, texte complet dans Paramètres › À propos). Table unique
`fonts::NAMES` (`ui/fonts.rs`), lue par le rendu (`fonts::styled`) et par le
sélecteur « Style du pseudonyme », qui écrit chaque choix dans sa police.

| `font_id` | Style dans Discord | Police | Fichier | Auteur (fichier de licence) | Licence |
|---|---|---|---|---|---|
| 1 (retiré) | Bangers | Bangers | `Bangers-Regular.ttf` | The Bangers Project Authors | SIL Open Font License 1.1 |
| 2 (retiré) | BioRhyme | BioRhyme | `BioRhyme[wdth,wght].ttf`, graisse 700 | The BioRhyme Project Authors | SIL Open Font License 1.1 |
| 3 | Sakura | Cherry Bomb One | `CherryBombOne-Regular.ttf` | The Cherry Bomb Project Authors | SIL Open Font License 1.1 |
| 4 | Jellybean | Chicle | `Chicle-Regular.ttf` | Angel Koziupa et Alejandro Paul | SIL Open Font License 1.1 |
| 5 (retiré) | Compagnon | Compagnon | `Compagnon-Bold.otf` | non nommé dans le fichier de licence ; d'après le fichier de police : Chloé Lozano, Velvetyne Type Foundry | SIL Open Font License 1.1 |
| 6 | Modern | MuseoModerno | `MuseoModerno[wght].ttf`, graisse 700 | The MuseoModerno Project Authors | SIL Open Font License 1.1 |
| 7 | Medieval | Néo-Castel | aucun (publiée sur Gumroad seulement, sans licence libre) : Palatino Linotype de Windows, ou le fichier de l'utilisateur | — | — |
| 8 | 8Bit | Pixelify Sans | `PixelifySans[wght].ttf`, graisse 700 | The Pixelify Sans Project Authors | SIL Open Font License 1.1 |
| 9 (retiré) | Ribes | Ribes | `Ribes-Black.otf` | Collletttivo (Luigi Gorlero, d'après le fichier de police) | SIL Open Font License 1.1 |
| 10 | Vampyre | Sinistre | `Sinistre-Bold.otf` | Collletttivo (Jules Durand, d'après le fichier de police) | SIL Open Font License 1.1 |
| 11 | gg Sans | gg sans (propriétaire) | aucun : demi-grasse de l'interface | — | — |
| 12 | Tempo | Zilla Slab | `ZillaSlab-SemiBold.ttf` (demi-grasse, 600) | The Mozilla Foundation | SIL Open Font License 1.1 |
| 13 | Monkey Bars | Playpen Sans | `PlaypenSans[wght].ttf`, graisse 700 | The Playpen Sans Project Authors | SIL Open Font License 1.1 |
| 14 | Mainframe | Orbitron | `Orbitron[wght].ttf`, graisse 700 | The Orbitron Project Authors | SIL Open Font License 1.1 |
| 15 | Headbang | New Rocker | `NewRocker-Regular.ttf` | Pablo Impallari, Brenda Gallo et Rodrigo Fuenzalida | SIL Open Font License 1.1 |
| 16 | Journal | Kalam | `Kalam-Bold.ttf` | Indian Type Foundry | SIL Open Font License 1.1 |

Association identifiant → police : relue le 2026-11-15 sur la source
(« Display Name Font », docs.discord.food/resources/user) ; la table `NAMES`
y correspond pour les seize identifiants (1 à 12 dans l'ordre de l'énumération,
13 Playpen Sans, 14 Orbitron, 15 New Rocker, 16 Kalam).

Graisses des fichiers statiques : la source ne les donne pas. Zilla Slab est
en demi-grasse (600) ; Ribes (Light, Regular, Black) en « Black » et Sinistre
(Regular, Bold, Dark) en « Bold », par cohérence avec les autres polices de
nom (graisse 700), sans confirmation par une source.

Néo-Castel (7) n'est publiée que sur la page Gumroad de son auteur
(maxlilllo.gumroad.com/l/neo-castel), sans licence libre : elle n'est pas
embarquée. Qui l'a obtenue de son auteur peut poser le fichier dans
`%APPDATA%\ferrocord\polices\7.otf` (ou `7.ttf`) : lu au démarrage par le
fil des polices (`fonts::read_names`), il remplace Palatino Linotype pour ce
style. Mêmes contrôles que pour toute police lue sur le disque (8 Mo au plus,
en-tête TrueType ou OpenType).

Chargement à la demande : les fichiers sont dans l'exécutable (3 509 384
octets en tout, dont 1 567 080 pour Playpen Sans, gardée entière faute d'outil
de sous-ensemble), mais egui ne connaît une police de nom qu'à partir de la
première image qui dessine un nom la portant (ou de l'ouverture du
sélecteur). Rien au démarrage ; à la demande, egui refait ses polices au début
de l'image suivante, une fois par police, et une seule image est redemandée
(le nom est en demi-grasse pendant cette image). Les caractères absents d'une
police de nom (cyrillique, emojis, idéogrammes) retombent sur la chaîne de la
demi-grasse puis sur les polices de repli. Graisse des fichiers variables :
700, réglée sur l'axe `wght` par egui (vérifié : le texte s'élargit de 400 à
700). Planche : `--scene polices-nom` de l'aperçu ; sélecteur :
`--scene profil-edition-style`.

| Jeton | Taille | Usage |
|---|---|---|
| `logo` | 30 | écran de connexion |
| `h1` / `h2` / `h3` | 24 / 20 / 17 | titres dans les messages |
| titre de page | 20, demi-gras | pages de paramètres (`title` × 1,25) |
| `title` | 16, demi-gras | nom du salon, du serveur, section de paramètres |
| `body` | 15 | messages, lignes, champs |
| `button` | 14 | boutons |
| `small` | 12 | heures, descriptions, en-têtes de groupe |
| `icon` | 20 | icône d'en-tête |
| `rail` | 15 | initiales du rail |

Règles :
- trois graisses seulement : normale (texte), demi-grasse (noms, titres,
  ligne non lue), grasse (mise en forme `**…**` des messages) ;
- les en-têtes de groupe (catégories, groupes de membres, groupes de la
  colonne des paramètres) sont en `small`, capitales, demi-gras, `muted` ;
- jamais de texte sous 12 points ; jamais de capitales hors de ces en-têtes
  (et de « ÉCHAP »).

## 4. Espacement

Grille de 4 points. Les espacements verticaux suivent la densité (aéré × 1,4,
compact × 0,6).

| Mesure | Valeur | Jeton |
|---|---|---|
| écart entre éléments | 8 × 4 | `item_x`, `item_y` |
| marge intérieure d'un bouton | 12 × 6 | `control_x`, `control_y` |
| ligne de salon, de catégorie de paramètres | 32 | `channel_row` |
| ligne de membre | 42 | `member_row` |
| marge de la colonne | 8 | `channels_pad` |
| avant une catégorie, autour d'un filet de section | 16 | `category_gap` |
| marge latérale d'un message | 16 | `message_x` |
| entre deux groupes de messages | 16 | `message_gap` |
| dans un groupe | 2 | `message_gap_grouped` |
| avatar de message, écart | 40, 14 | `avatar_size`, `avatar_gap` |
| petit avatar (membres, MP, panneau utilisateur) | 32 | `small_avatar` |
| marge d'une carte, d'un embed | 12 | `embed_pad` |
| marge d'une fenêtre | 28 | `card_pad` |

Largeurs : rail 72, salons 240, membres 240, embeds 520 au plus, page de
paramètres 740 au plus, colonne des paramètres 218.

## 5. Formes, élévation

### Rayons

Multipliés par le réglage « arrondi » :

| Rayon | Valeur | Jetons | Pour |
|---|---|---|---|
| petit | 6 | `row`, `embed`, `code_radius` | lignes, embeds, fichiers, pastilles de réaction, code |
| moyen | 8 | `control`, `composer`, `toast` | boutons, champs, zone de saisie, notifications |
| fenêtre | 12 | `card` | fenêtres, cartes de profil, tuiles d'appel |
| rail | 24, puis 16 | `rail`, `rail_selected` | pastille ronde au repos, carré arrondi au survol et ouverte |
| pilule | moitié de la hauteur | — | interrupteur, pastille de mentions |

Les avatars sont ronds (ou carrés arrondis, réglage).

### Élévation

| Niveau | Ce qui s'y trouve | Rendu |
|---|---|---|
| 0 | zones de l'application, paramètres en plein écran | aplats `bg0` à `bg2`, filets |
| 1 | cartes dans le flux (embeds, fichiers, sondages, zone de saisie) | fond `bg1` ou `bg3`, contour `border`, pas d'ombre (`Tokens::card_frame`) |
| 2 | ce qui flotte (menus, infobulles, barre d'actions, carte de profil, sélecteurs, modales) | contour `border`, ombre `shadow` (`window_frame`, `float_frame`) |

Ombres : menus et infobulles, décalage 0 × 6, flou 18 ; fenêtres, 0 × 12,
flou 36. Une modale pose en plus le voile `scrim`.

## 6. États

| État | Ligne (salon, membre, catégorie) | Bouton | Champ |
|---|---|---|---|
| repos | pas de fond, texte `muted`, icône `faint` | fond `button_bg` | fond `bg0`, contour léger |
| survol | fond `row_hover`, texte `text` | fond `control_bg` | — |
| enfoncé | — | fond `selected` | — |
| ouvert, sélectionné | fond `selected`, texte `text_strong` demi-gras | — | — |
| clavier | contour `focus` de 2 points | idem | contour `accent` |
| inactif | — | contenu à 45 % | idem |
| non lu | texte `text_strong` demi-gras, pilule à gauche | — | — |
| mention | pastille rouge à droite | — | — |

Le bouton principal d'une fenêtre est plein (`accent`, texte `on_accent`). Une
action destructrice est rouge (texte `danger`, ou aplat `danger_fill`). Les
autres boutons sont neutres.

Animations : voir le § 10 ; aucune ne tourne au repos, et le réglage « réduire les
animations » les coupe (`ui/motion.rs`).

## 7. Composants

### Rail des serveurs
Fond `bg0`. Pastilles de 48, rondes au repos, carrés arrondis (rayon 16) au
survol et pour le serveur ouvert ; sans icône, la pastille est sur `pill` et
passe à l'accent au survol et quand elle est ouverte. Les messages privés sont
en haut (icône de bulle), suivis d'un filet court. Une pilule `text_strong`,
collée au bord gauche, dit l'état : courte pour un serveur non lu, mi-haute au
survol, haute pour le serveur ouvert. Les mentions sont comptées dans une
pastille rouge en bas à droite. « Ajouter un serveur » est un « + » vert
(`online`), plein au survol.

### Colonne des salons
Fond `bg1`. En-tête : nom du serveur demi-gras et chevron ; il ouvre le menu
du serveur (paramètres, inviter, marquer comme lu, quitter). Catégories en
en-tête de groupe, précédées d'un petit chevron. Lignes de 32, rayon 6, en
retrait de 8 ; icône puis nom. Ligne ouverte : fond `selected`. Non lu : nom
en `text_strong` demi-gras et pilule au bord gauche. Mention : pastille rouge.
Conversations privées : lignes de 44 avec avatar de 32 et pastille de présence.

### Panneau utilisateur
En bas de la colonne, sur `panel_bg` : avatar de 32 (avec ma décoration) et
présence (le statut choisi), nom demi-gras au style de mon nom (couleurs,
lueur, police du style), ma plaque en fond de toute la carte sous un
voile de `panel_bg`. À droite, dans l'ordre : manette (seulement quand une
activité est détectée ; ouvre la page « Activité »), micro et casque suivis
chacun d'un petit chevron, engrenage (`ui/user_panel.rs`). Un clic sur l'icône
coupe ou rétablit ; seul le chevron ouvre le menu (périphérique en sous-menu,
profil d'entrée, volume sur toute la largeur, « Paramètres vocaux »), ancré
au-dessus du bouton. Coupé : icône barrée `danger` sur un fond rouge sombre
(30 % de `danger` sur le fond) qui englobe icône et chevron. Survol : aplat
`row_hover` arrondi par bouton. Colonne étroite : le nom et le statut se
coupent (points de suspension), puis les chevrons disparaissent (le menu passe
au clic droit), puis la manette ; micro, casque et engrenage ne sont jamais
coupés. La déconnexion est en bas des paramètres.

Un clic sur l'avatar ou le nom ouvre la **carte de mon compte**, ancrée
au-dessus du panneau (largeur `profile_width`, ombre de fenêtre) : en-tête de
profil (bannière, avatar, décoration), nom, identifiant, pronoms, statut
personnalisé, bio (quatre lignes), puis deux blocs de lignes de menu pleine
largeur (`menu::line` : icône ou pastille à gauche, hauteur d'un contrôle),
chacun dans une carte arrondie (`r_embed`) dont le fond est la couleur du texte
à 6 % : il se détache du fond de la carte, clair, foncé ou aux couleurs du thème
du profil. Premier bloc : « Modifier le profil », le statut en cours (pastille,
chevron) et « Définir / Modifier le statut personnalisé » (avec une croix pour
le retirer d'un clic) ; second bloc : « Changer de compte » (chevron) et
« Copier l'identifiant ». Aucun champ de saisie dans la carte. Les lignes à
chevron ouvrent, au survol ou au clic, un sous-menu (`menu_frame`) à droite de
la carte, à gauche si la place manque, toujours dans la fenêtre : les quatre
statuts avec leur description, ou les comptes enregistrés et « Gérer les
comptes ». Le statut personnalisé se règle dans une petite fenêtre modale
(emoji, texte, « Effacer après » ; « Retirer le statut », « Annuler »,
« Enregistrer »). Échap ou un clic en dehors ferme le sous-menu, puis la carte.
Le clic droit sur le panneau donne les mêmes actions dans un menu.

**Édition rapide du profil** : fenêtre modale (`window_frame`) par-dessus la
discussion ; titre, onglets « Profil principal » / « Profil de ce serveur »,
formulaire à gauche (340) et aperçu en direct à droite (`profile_width`), pied
avec « Plus d'options » (lien vers la page Profils), l'indication
« Modifications non enregistrées » (`muted`, `danger` après un clic à côté),
« Annuler » et « Enregistrer » (primaire). Un champ que Discord refuserait
sans Nitro est remplacé par la mention « Réservé à Nitro » (`faint`). Fenêtre
étroite : le formulaire seul.

### Discussion
Fond `bg2`. En-tête : icône du salon, nom demi-gras, actions en icônes à
droite, champ de recherche ; filet dessous. Messages groupés par auteur :
avatar de 40, nom demi-gras à la couleur du rôle, heure en `small` `faint` ;
16 entre deux groupes, 2 dans un groupe. Survol : voile `hover` (la ligne
s'assombrit) et barre d'actions flottante à droite ; entre deux messages, un
seul est survolé. Séparateur de jour : filet et date centrée. « Nouveaux
messages » : filet et étiquette `danger`.

Dans les messages :
- citation : barre `quote_bar` de 3 à gauche ;
- code : fond `code_bg`, contour `border`, rayon 6 ;
- mention : fond d'accent translucide, texte `mention_text` ;
- réaction : pastille de rayon 6 sur `bg3` ; la mienne : fond d'accent
  translucide et contour `accent` ;
- embed, fichier, sondage, invitation : carte de niveau 1 ; l'embed garde sa
  barre de couleur de 4 à gauche ;
- image : rayon 6, jamais de contour.

### Zone de saisie
Carte de niveau 1 : fond `bg3`, rayon 8. À gauche, « + » dans un disque
(joindre un fichier) ; à droite, sondage, message vocal et sélecteur d'emojis
(qui contient aussi les autocollants). Les bandes d'état (« … écrit », réponse
en cours, envois) se posent au-dessus, en `small`.

### Liste des membres
Fond `bg1`. Sections « EN LIGNE — N » en en-tête de groupe. Lignes de 42 :
avatar de 32 avec pastille de présence, nom à la couleur du rôle, activité en
`small` `muted`. Hors ligne : nom estompé (`muted`).

### Carte de profil
Niveau 2 : rayon 12, ombre. Bannière en 17:6 sur toute la largeur de la carte,
recadrée au centre (jamais déformée), coins du haut arrondis ; à défaut la
couleur d'accent du compte. Dans le profil complet, bannière de 0,3 fois la
largeur. Avatar de 80 à cheval sur le bas de la bannière, cerclé du fond de
la carte, avec sa décoration (1,2 fois le côté de l'avatar). Thème du profil :
fond en dégradé vertical des deux couleurs sur toute la hauteur de la carte,
contour de la couleur claire du thème, jetons de texte recalculés pour
garder le contraste (4,5 pour le texte, 3 pour le texte secondaire) ; les blocs
(rangée de badges, bulle de statut) sont translucides (couleur du texte à 7 %).
Deux boutons ronds sur la bannière, en haut à droite (voile `media_veil`) :
l'amitié (menu) et « … ». Statut personnalisé en bulle de pensée à droite de
l'avatar (deux petits ronds, trois lignes au plus). Nom en 20 demi-gras au
style du profil (couleur ou dégradé, lueur, contour ou ombre, police du style :
§ 3, « Polices de nom »), ligne « identifiant • pronoms » et pastille du tag de serveur,
rangée de badges dans une pastille arrondie (le téléphone d'abord quand la
personne est sur mobile ; infobulle à grande image, catégorie grise, palier en
capitales, ancienneté). Suivent « avatars empilés, N amis en commun • N serveurs
en commun », la bio sans titre, « Membre depuis » (petit titre demi-gras, sans
capitales), les rôles, les comptes connectés en pastilles rondes. « Voir le
profil complet » est collé en bas, sur toute la largeur ; la hauteur de la carte
suit son contenu, bornée à la fenêtre (le corps défile).
sections en en-têtes de groupe, rôles et badges en étiquettes de rayon 6.
Bannière, avatar et décoration ne s'animent que dans un profil ouvert, selon
« lire les GIF » et « réduire les animations » ; ailleurs, image fixe (avatar
animé au survol).

### Paramètres (utilisateur, serveur, salon)
Un seul gabarit, `ui/settings/shell.rs`, pour les trois fenêtres.

- **Plein écran**, par-dessus l'application ; Échap ferme.
- **Colonne des catégories** à gauche, sur `bg1` jusqu'au bord de la fenêtre,
  large de 218 et collée contre la page (la colonne et la page sont centrées
  ensemble). Titres de groupe en en-tête de groupe, filets entre les groupes,
  lignes de 32 ; en bas, l'action rouge (« Déconnexion », « Supprimer le
  salon »).
- **Page** sur `bg2`, large de 740 au plus : titre de page en grand
  (20, demi-gras), sections titrées (`title`, demi-gras) sous un filet.
- **Fermeture** : bouton rond « X » à droite de la page, « ÉCHAP » dessous.
- **Ligne de réglage** (`shell::settings_row`) : libellé en `text_strong` et
  description en `small` gris à gauche, réglage à droite, centré sur la ligne.
  Interrupteur en pilule (`shell::switch`, 40 × 24, vert `online` quand il est
  activé, gris `offline` sinon), liste de choix, curseur avec sa valeur, bouton.
- **Dans une page**, une ligne a d'emblée la hauteur d'un bouton
  (`shell::page_style`) : un libellé posé devant un bouton est centré sur lui.
- Fenêtre étroite ou disposition mobile : une colonne (la liste des pages, ou
  la page avec son titre et sa croix), comme avant.

Les pages ne dessinent pas leurs lignes elles-mêmes : elles passent par
`settings::{section, toggle, choice, value_row, in_discord, note}` et
`appearance::row`, qui s'appuient sur le gabarit. Une case à cocher posée par
une page devient `shell::switch_label` (interrupteur puis libellé).

### Modales, sélecteurs
Niveau 2 : fond `bg2`, contour, rayon 12, ombre de fenêtre, voile. Marge 28.
Titre en `title` demi-gras. Boutons en bas, le principal en premier.

### Boutons, champs, menus
Bouton : hauteur 33 (14 de texte, marge 12 × 6), rayon 8. Champ : fond `bg0`,
contour léger, contour `accent` quand il a le clavier. Curseur : partie
remplie en accent. Menu et infobulle : niveau 2, ligne survolée en `row_hover`.

### Appel
Tuiles sur `bg3`, rayon 12 (l'image vidéo épouse les coins) ; celle de qui
parle est cerclée de `online`. Étiquette du nom en bas à gauche sur `scrim`.
« Regarder » sous l'avatar, jamais dessus. Barre de contrôle : boutons ronds,
actifs en `accent`, raccrocher en rouge.

### Notifications
Niveau 2 : fond `bg3`, contour, rayon 8, ombre de menu ; en bas de la
discussion, jamais sur un message.

## 8. Fichiers de thème existants

Un `theme.toml` écrit avant ce lot contient toutes les valeurs de l'ancien
thème. À la lecture, un fichier sans `version` est mis à jour : chaque valeur
restée égale à celle de l'ancien préréglage prend la nouvelle valeur ; ce que
l'utilisateur avait réglé lui-même est gardé. Le fichier réécrit porte
`version = 2` et n'est plus touché.

## 9. Fondations visuelles (lot V1, `docs/ecarts-visuels.md` § 5)

Ce lot aligne le gabarit sur le client de 2025-2026. Ce qui précède décrit le
préréglage d'origine, gardé sous le nom « Rouille » ; ce qui suit vaut pour les
quatre préréglages au calque (`Preset::calque`) : Sombre, Cendre, Onyx, Clair.
Les valeurs marquées ≈ sont lues sur des captures : à vérifier à la pipette.

### Préréglages
| Clé | Nom | Fonds (`bg0` → `bg3`) | Remarque |
|---|---|---|---|
| `sombre` | Sombre (défaut) | ≈ `#121214`, `#121214`, `#1a1a1e`, `#222327` | filets `panel_border` blancs à 8 % |
| `cendre` | Cendre | ceux de Rouille | l'ancien gris-bleu ; ses fonds d'origine, plus clairs, ne tiennent pas 3:1 avec l'accent, ils ne sont donc pas repris |
| `onyx` | Onyx | `#000000`, ≈ `#070709`, `#0a0a0c`, `#131416` | noir (écrans OLED) |
| `clair` | Clair | inchangés | géométrie du calque |
| `rouille`, `minuit`, `contraste`, `compact`, `aere` | — | inchangés | géométrie d'avant ce lot |

`presets_meet_wcag_aa` et `presets_meet_contrast` valent pour tous.

### Nouveaux jetons
| Jeton | Rôle | Calque |
|---|---|---|
| `couleurs.panel_border` | filet entre rail, salons et discussion, et sous la barre de titre | blanc 8 % |
| `couleurs.menu_bg`, `menu_hover` | fond des menus et infobulles, ligne survolée | ≈ `#28282d`, `#36363b` |
| `couleurs.channel_idle` | nom d'un salon lu, au repos | ≈ `#82838b` |
| `typographie.line_height` | interligne du texte courant (0 : celui de la police) | 22 |
| `typographie.family` | police du texte : `figtree` (embarquée) ou `systeme` | `figtree` |
| `espacements.header_height` | hauteur des en-têtes (0 : d'après les marges) | 48 |
| `espacements.message_right` | marge à droite d'un message | 48 |
| `espacements.control_height` | hauteur des boutons, champs et lignes de menu | 32 |
| `mise_en_page.titlebar` | `integree` ou `windows` | `integree` |
| `mise_en_page.titlebar_height` | hauteur de la barre de titre | 32 |

Un `theme.toml` d'avant ce lot (`version` 2 ou absente) est mis à jour comme au
§ 8 : ce qui valait le préréglage d'avant prend la valeur du nouveau, `rouille`
devient `sombre`, les réglages de l'utilisateur sont gardés ; il est réécrit en
`version = 5`.

### Typographie
Texte courant 16, graisse moyenne (500) pour les boutons, lignes de menu et
titres de la barre. Depuis l'intégration du lot : Figtree embarquée (§ 3), axe
`wght` à 500 ; avec `typographie.family = "systeme"`, Segoe UI Variable du
système comme avant. Code : Consolas.

### Icônes
`theme/icons.rs` : jeu plein sur une grille de 24 (`Icon::ALL`, 73 dessins),
toujours dessiné par le code. Planche : `preview --gallery icones`.

### Barre de titre
Sous Windows, la fenêtre est créée sans décoration et `ui/titlebar.rs` dessine
la barre (fond `bg0`, nom du serveur ou « Messages privés » au centre, boîte de
réception, aide, réduire, agrandir, fermer). `platform/caption.rs` double la
procédure de fenêtre : la barre répond `HTCAPTION` (déplacement, double-clic,
menu système, ancrage : faits par Windows), le bouton d'agrandissement
`HTMAXBUTTON` (volet d'ancrage de Windows 11), les bords gardent leur
redimensionnement. `mise_en_page.titlebar = "windows"` rend la barre du système.

### Composants partagés
- `ui/widgets.rs` : `button` (primaire, secondaire, danger, succès, lien ;
  hauteur `control_height`), `field` (hauteur 40), `tip` (infobulle à flèche,
  immédiate), `modal` (titre et croix, contenu, pied en bande `bg1` où les
  boutons sont à droite, le principal au bout), `close_button`.
- `ui/menu.rs` : `item`, `check`, `radio`, `submenu`, `separator` ; texte à
  gauche, icône ou marque à droite, ligne survolée en `menu_hover`, danger en
  rouge. Le cadre des menus d'egui vient de `Tokens::menu_frame`.
- `settings/shell.rs` : l'interrupteur porte une coche ou une croix.
- Planche : `preview --gallery composants` et `--gallery fenetre`.

## 10. Structure et place (lot V2, `docs/ecarts-visuels.md` § 5)

- **Colonnes** : rail 72, salons 240, membres 240. Les deux colonnes se
  redimensionnent à la souris par leur bord ; la largeur est un réglage du
  thème (`channels_width`, `members_width`), donc retenue. Le bouton
  « Membres » de l'en-tête affiche ou masque la liste.
- **Fenêtre étroite** (`ui/layout.rs`) : la discussion garde 420. En dessous,
  la liste des membres s'efface d'abord, puis la colonne des salons se
  resserre jusqu'à sa borne (160). Les réglages ne changent pas : tout revient
  avec la place.
- **Panneau utilisateur** : carte `panel_bg` bordée (`panel_border`, rayon
  `card`), à 8 des bords, qui passe sous le rail (le rail lui laisse le bas
  libre). Deux lignes : nom demi-gras, puis statut personnalisé ou statut en
  mots (`small`, `muted`). Le filet entre le rail et la colonne s'arrête à la
  carte.
- **Rail** : pilule d'état de 4 de large, haute de 8 (non lu), 20 (survol) ou
  40 (ouvert) pour une pastille de 48. Pastille de mentions : 16 de haut,
  chiffres de 12 demi-gras, anneau de 3 à la couleur du rail. Filet sous les
  messages privés : 32 × 2. Dossier replié : teinte à 40 %.
- **Colonne des salons** : marque de non-lu de 4 × 8, arrondie côté texte.
- **Liste des membres** : pas de 44 (ligne de 42, écart de 2). Hors ligne : la
  ligne entière à 30 % d'opacité, pleine au survol et au focus.

## 11. Animations (lot ANIM, `ui/motion.rs`)

Tout mouvement de l'interface passe par `ui/motion.rs` : aucun écran n'écrit
de durée ni de courbe.

- **Jetons.** Durées dans `motion::dur` (survol 120 ms, apparition 140 ms,
  fenêtre 160 ms, changement de forme 180 ms, message 200 ms, mention 900 ms),
  échelles `POP_SCALE` (0,95), `POP_START`, glissement `SLIDE` (8 points).
  Une seule courbe, `ease_out`.
- **Primitives.** `hover` / `toward` (fondu vers un état), `value` (grandeur
  qui en rejoint une autre), `appear` (depuis la première image d'un élément),
  `watch` (« pop » quand une valeur change), `kick` / `played` (une fois),
  `Span` (fondu et déplacement de ce qui vient d'être dessiné), `dots` (trois
  points d'écriture), `typing_pill` (pastille de statut qui écrit), `skeleton`.
- **Zones flottantes.** Menus contextuels, popovers, cartes de profil,
  fenêtres modales et infobulles d'egui apparaissent en fondu et à l'échelle
  0,95 → 1 sans rien demander aux écrans : toute zone d'ordre `Foreground` ou
  `Tooltip` qui vient d'apparaître est animée en fin de passe. Le voile d'une
  fenêtre modale se fond sans changer d'échelle.
- **Repos.** Une animation ne demande une image que pendant sa durée ; les
  points d'écriture et les squelettes ne battent (30 images par seconde) que
  tant qu'ils sont dessinés. Chaque primitive a un test d'arrêt.
- **Réglages.** Accessibilité > « Intensité des animations » (`local.toml`,
  `intensite_animations`) : `aucune` (tout est immédiat), `discrete` (fondus
  seulement), `complete`. « Réduire les animations » vaut `aucune`.
- **Aperçu.** `preview --scene ecriture | ecriture-deux | ecriture-plusieurs |
  transitions`, avec `--at <ms>` pour capturer à un instant donné.

## 12. Contenu et écrans (lot V3, `docs/ecarts-visuels.md` § 5)

- **Cartes de contenu.** Embeds, invitations, sondages, composants, pièces
  jointes et messages système sur `Tokens::card_fill()` (un cran plus clair
  que le fond dans les thèmes sombres calqués) ; marges d’embed par
  `embed_card_margin()`.
- **Réactions.** Pastilles de `reaction_height()` (24) avec émoji de
  `reaction_emoji()` (16), compteur en demi-gras, sans contour hors « la
  mienne » ; le « + » n’apparaît qu’au survol du message.
- **Zone de saisie.** Bande « Réponse à … » collée au-dessus de la carte
  (coins hauts arrondis, coins bas droits) ; mode lent sous la carte, à
  droite de la ligne d’écriture ; bouton émoji gris au repos.
- **Profils.** Carte : anneau `profile_ring`, bannière `profile_banner`,
  une ligne « Membre depuis », champ « Envoyer un message à @nom ». Fenêtre
  complète : avatar `profile_full_avatar()`, croix sur la bannière, onglets
  soulignés `widgets::tab`, connexions en cartes sur deux colonnes.
- **Mon compte.** Carte extérieure (en-tête de profil, « Modifier le
  profil ») et carte intérieure des valeurs.
- **Connexion.** Deux colonnes sur ordinateur : formulaire à gauche, QR à
  droite. Le QR n’est demandé au réseau qu’après « Afficher le QR code ».
- **Appel.** Fond `call_bg()`, tuiles `r_media`, raccrocher en pilule rouge.
- **Aperçu.** `preview --scene reponse` (bande de réponse).
