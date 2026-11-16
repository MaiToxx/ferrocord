# Écarts visuels avec le client Discord de bureau

Audit du 14 novembre 2026, sur `main` à `e7aab17`. Aucun code n'a été écrit.

> **Mise à jour du 15 novembre 2026 (vérification finale, `main` à `5b98f72`).**
> Les lots V1, V2, V3, PERSO et les retouches des lots de fonctions sont
> fusionnés. L'état de chaque manque (M1 à M20) est noté dans sa ligne, celui
> des vingt écarts les plus visibles dans le tableau du chapitre 4. Les
> tableaux chiffrés du chapitre 2 décrivent l'état d'avant : leurs valeurs
> (texte de 16, en-têtes de 48, contrôles de 32, rayons de 8, pastilles du
> rail de 40, images de 550 × 350, citation de 4, emoji de 22, carte de profil
> de 300, modale de 440) sont maintenant celles du calque
> (`theme/presets.rs`, `discord()`), vérifiées dans le code. Ce qui reste est
> dans `docs/ecarts-restants.md`, les défauts vus sur capture dans
> `docs/verification-finale.md`.

## Méthode et limites

- **Ferrocord** : captures existantes de `captures\captures-esthetique-v2\`
  (planches « Avant / Après » du lot E, prises à 125 %), de
  `captures\captures-esthetique\` et de `captures\captures-perso\`, plus les
  valeurs lues dans `src/theme/presets.rs` (`rouille()`), `src/theme/mod.rs` et
  `docs/design.md`. L'aperçu n'a pas été relancé.
- **Discord** : valeurs de la feuille de style du client de bureau telles que
  je les connais (thème sombre d'avant mars 2025, devenu « Ash », et refonte
  de mars 2025 avec « Dark » et « Onyx »). Le centre d'aide
  `support.discord.com` a refusé toute lecture automatique (HTTP 403) ; seuls
  des comptes rendus de presse de la refonte ont pu être relus (quatre thèmes
  Light, Ash, Dark, Onyx ; trois densités ; colonne redimensionnable).
- **Conséquence** : les mesures de Discord ci-dessous sont à **vérifier sur le
  vrai client avant de coder**, avec une capture à 100 % et une pipette. Les
  valeurs marquées « ≈ » sont les moins sûres (surtout les couleurs de « Dark »
  et « Onyx » et les rayons d'après refonte). Les valeurs de Ferrocord, elles,
  sont lues dans le code.
- Les captures de l'aperçu utilisent des images factices (losanges, disques de
  couleur) : rien n'est dit ici sur le rendu des vrais avatars et emojis.

Toutes les mesures sont en points (pixels à 100 %).

---

## 1. Ce qui manque entièrement

| # | Élément de Discord | Ferrocord | Où l'ajouter |
|---|---|---|---|
| M1 | **Barre de titre personnalisée**, haute de ≈ 32 : nom du serveur ou du MP au centre avec son icône, puis à droite boîte de réception, aide, réduire, agrandir, fermer ; même fond que le rail | **fait** (lot V1, `ui/titlebar.rs`, Windows seulement). Avant : barre de titre de Windows, blanche ou grise selon le système, qui tranche sur l'interface sombre | `src/lib.rs:56` (`with_decorations(false)`), nouveau `src/ui/titlebar.rs` ; glisser, double-clic, bords de redimensionnement et accrochage de Windows 11 à refaire (`unsafe` Win32 permis) |
| M2 | Bouton d'aide (« ? ») | **fait** (barre de titre). Avant : absent | avec M1 |
| M3 | **Panneau utilisateur flottant** : carte arrondie qui passe sous le rail et la colonne des salons (largeur ≈ 72 + 240 − 16), deux lignes : nom, puis statut (« En ligne », statut personnalisé) | **fait** (lot V2) ; défaut restant avec un thème à dégradé ou à image de fond, voir `docs/verification-finale.md` n° 2. Avant : bande pleine largeur de la colonne des salons seulement, une ligne (nom seul) | `ui/channels.rs:60-100`, `Tokens::user_bar_*`, `panel_bg` |
| M4 | **Sujet du salon** dans l'en-tête, après un filet vertical, sur une ligne tronquée, cliquable | **fait** (lot S, `ui/chat_head.rs`). Avant : absent | `ui/chat.rs:457` |
| M5 | Icônes d'en-tête d'un salon : **Fils, cloche de notification**, épingles, membres ; recherche à droite | **fait** (lot S). Avant : membres, (suivre), épingles, boîte, recherche | `ui/chat.rs:480-500` ; la boîte part dans la barre de titre |
| M6 | Icônes d'en-tête d'un MP : appel, **appel vidéo**, épingles, **ajouter des amis**, **profil**, recherche | **fait** (lots S et U). Avant : appel, épingles, boîte, recherche | idem |
| M7 | **Début de salon** : disque de 68 avec « # », titre de 32 en gras « Bienvenue dans #salon ! », phrase grise. **Début de MP** : avatar de 80, nom de 32, nom d'utilisateur, phrase, serveurs en commun, boutons | **fait** pour le salon ; début de MP sans serveurs en commun ni boutons. Avant : une ligne centrée « Début de l'historique du salon. » | `ui/chat.rs:985` |
| M8 | **Colonne « Actifs en ce moment »** de la page Amis (≈ 360 de large, masquée sous ≈ 1 200) | **fait** (lot U, fenêtre large). Avant : absente | `ui/friends.rs` |
| M9 | Colonne des MP : bouton « Rechercher ou lancer une conversation » en tête (à la place d'un titre), en-tête de groupe « Messages privés » avec « + », croix de fermeture au survol d'une ligne | **fait** (lot U). Avant : titre « Messages privés », pas de « + », pas de croix. Lot « quêtes » : quatre entrées en tête, dans l'ordre du client (« Amis », « Ton Nitro », « Boutique », « Quêtes »), au gabarit d'une ligne de salon, icônes pleines dessinées (`Icon::Wave`, `Nitro`, `Shop`, `Quest`) ; « Demandes de message » ensuite. « Ton Nitro » ouvre « Nitro et facturation » des paramètres (état de l'abonnement connu, « Ouvrir dans Discord ») : aucun achat | `ui/channels.rs` |
| M10 | Icônes au survol d'une ligne de salon : créer une invitation, modifier le salon (16 chacune, à droite) | **fait** (inviter, modifier). Avant : rien au survol (brouillon : crayon) | `ui/channels.rs:309` |
| M11 | Rail : boussole « Découvrir » sous le « + » | **fait** (lot R, ouvre le navigateur). Avant : absent | `ui/rail.rs:400` |
| M12 | En-tête de serveur : bannière derrière le nom (serveurs boostés), lignes Parcourir, Membres, Boosts sous Événements | **partiel** : bannière faite ; lignes Parcourir, Membres, Boosts absentes. Avant : nom et chevron ; « Événements » seul | `ui/channels.rs:100-130` |
| M13 | Sélecteur d'expressions : onglet GIF, rail de catégories à gauche, bandeau d'aperçu en bas, sélecteur de teinte à droite de la recherche | **fait** (lot G). Avant : deux onglets, une colonne | `ui/emoji_picker.rs` |
| M14 | Barre de survol d'un message : trois emojis récents, puis réagir, répondre, transférer, « ⋯ » | **fait** (lot M). Avant : réagir, répondre, transférer, fil, épingler, supprimer | `ui/msg_actions.rs:448` |
| M15 | Réponse : trait coudé qui part de l'avatar vers la ligne de réponse (« spine »), mini-avatar de 16 | **fait** (`reply_header`, coude depuis l'avatar). Avant : ligne de réponse sans coude *(à vérifier sur capture : scène absente de `preview`)* | `ui/msg_actions.rs:201` |
| M16 | Étiquette « NOUVEAU » pleine (aplat rouge, texte blanc) au bout du filet rouge | **fait** (lot V3). Avant : mot « Nouveaux » en rouge, sans aplat | `ui/chat.rs:930` |
| M17 | Infobulles noires à flèche, immédiates, sur chaque icône | **fait** (`widgets::tip`, lot V1). Avant : infobulles d'egui (cadre du thème, sans flèche, avec délai sauf réglage) | `Tokens::apply`, 82 appels à `on_hover_text` |
| M18 | Panneau vocal : rangée de quatre boutons (caméra, partage, activité, tableau de sons) sous « Voix connectée » | **fait** (lot V : caméra, partage, tableau de sons ; pas d'« activité »). Avant : texte, latence, raccrocher, lien des codes | `ui/voice/panel.rs:48` |
| M19 | Écran de connexion en deux colonnes avec le QR code affiché d'emblée à droite, sur un fond illustré | **fait** (deux colonnes ; le QR n'est demandé qu'après « Afficher le QR code », voulu). Avant : une carte étroite centrée sur fond uni ; le QR est derrière un bouton | `ui/login.rs`, `ui/qr_login.rs` |
| M20 | Colonne des salons redimensionnable à la souris | **fait** (lot V2). Avant : largeur fixe (réglable dans Apparence) | `ui/mod.rs` (panneau latéral `resizable`) |

---

## 2. Écarts, écran par écran

Chaque ligne : ce que fait Discord, ce que fait Ferrocord, le fichier et le
jeton, la correction. Classement par visibilité dans chaque section ; le
classement d'ensemble est au chapitre 4.

### 2.1 Couleurs des surfaces (thème sombre)

Discord propose trois thèmes sombres. Ferrocord n'en a qu'un (« Sombre », clé
`rouille`) plus « Minuit ».

| Surface | Discord Ash | Discord Dark (≈) | Discord Onyx (≈) | Ferrocord | Jeton |
|---|---|---|---|---|---|
| Rail des serveurs | `#1e1f22` | `#121214` | `#000000` | `#1a1b1e` | `bg0` |
| Colonne des salons, membres | `#2b2d31` | `#121214` à `#18181c` | `#070709` | `#232428` | `bg1` |
| Discussion | `#313338` | `#1a1a1e` | `#000000` à `#0c0c0e` | `#2b2d31` | `bg2` |
| Zone de saisie | `#383a40` | `#222327` | `#131416` | `#34363c` | `bg3` |
| Panneau utilisateur | `#232428` | `#1a1a1e`, carte bordée | `#0c0c0e`, carte bordée | mi-chemin `bg0`/`bg1` ≈ `#1e1f23` | `Tokens::panel_bg` |
| Menus, infobulles | `#111214` | `#28282d` (≈) | `#131416` (≈) | `bg3` `#34363c` + contour | `float_frame` |
| Ligne survolée | `#35373c` | blanc à ≈ 5 % | blanc à ≈ 6 % | `#31333a` | `row_hover` |
| Ligne ouverte | `#404249` | blanc à ≈ 9 % | blanc à ≈ 10 % | `#393b41` | `selected` |
| Message survolé | noir à 6 % (`rgba(2,2,2,.06)`) | blanc à ≈ 3 % | blanc à ≈ 4 % | noir à 9 % | `hover` |
| Filets | `#3f4147`, souvent absents | blanc à ≈ 8 %, **partout** (entre chaque zone) | idem | `#35373d` | `separator` |
| Champs | `#1e1f22` | fond de la zone + contour | idem | `#1a1b1e` + contour | `bg0`, `border` |
| Texte courant | `#dbdee1` | `#dfe0e2` (≈) | idem | `#dbdee1` | `text` |
| Texte fort | `#f2f3f5` | `#fbfbfb` (≈) | idem | `#f7f8f9` | `text_strong` |
| Texte secondaire | `#b5bac1` | `#aaaab1` (≈) | idem | `#a9aeb7` | `muted` |
| Texte estompé (heures, icônes au repos) | `#949ba4` | `#82838b` (≈) | idem | `#a2a7b0` | `faint` |
| Accent | `#5865f2` | idem | idem | `#5966f2` | `accent` |
| Liens | `#00a8fc` | idem | idem | `#00a8fc` | `link` |
| Vert, jaune, rouge, gris de présence | `#23a55a` `#f0b232` `#f23f43` `#80848e` | idem | idem | identiques | `online`… |
| Rouge de danger (texte) | `#fa777c` | idem | idem | `#fb8389` | `danger` |
| Rouge plein (pastille, bouton) | `#f23f43` ; bouton `#da373c` | idem | idem | `danger` foncé par calcul | `danger_fill()` |

Constats :

1. **Ferrocord est entre Ash et Dark** : ses fonds sont un cran plus sombres
   qu'Ash (voulu, pour le contraste AA, `docs/design.md` § 2) sans avoir
   l'allure de Dark. Un utilisateur du Discord actuel (Dark par défaut) voit un
   Ferrocord nettement plus clair et plus gris ; un utilisateur d'Ash le voit
   un peu plus sombre.
2. Dans Dark et Onyx, **rail et colonne des salons ont le même fond** et la
   discussion est plus claire ; les zones sont séparées par des filets fins.
   Ferrocord a trois niveaux de gris distincts et presque pas de filets.
3. `faint` (`#a2a7b0`) est presque aussi clair que `muted` (`#a9aeb7`) : les
   heures et les icônes au repos ressortent plus que dans Discord, où elles
   s'effacent (`#949ba4`). C'est le contraste AA qui l'impose ; Discord ne le
   tient pas sur ce texte-là.

Correction : garder « Sombre » tel quel n'est pas la question. Ajouter trois
préréglages calqués, **« Cendre » (Ash), « Sombre » (Dark, par défaut) et
« Onyx »**, dans `src/theme/presets.rs`, avec les valeurs relevées à la pipette
sur le vrai client ; renommer l'actuel en « Rouille » ou le remplacer par
« Cendre » dont il est le plus proche. Ajouter un jeton `panel_border` (filet
entre zones, blanc translucide) et le dessiner entre rail, colonne, discussion
et membres. Le test `presets_meet_wcag_aa` devra accepter un `faint` à 3:1 pour
les heures, ou les trois préréglages calqués en être dispensés et le dire.

### 2.2 Typographie

| Élément | Discord | Ferrocord | Jeton, fichier | Correction |
|---|---|---|---|---|
| Police | gg sans (propriétaire), graisses 400, 500, 600, 700, 800 | Segoe UI du système, 400, 600 (demi-gras), 700 | `ui/fonts.rs:24` | police libre embarquée, voir chapitre 3 |
| **Texte des messages** | 16, interligne 22 (1,375) | 15, interligne d'egui (≈ 1,2 à 1,35) | `typography.body` | 16 ; fixer l'interligne à 22 (`TextFormat::line_height`) |
| Nom de l'auteur | 16, graisse 500 | 15, demi-gras 600 | `message.rs`, `fonts::demi()` | 16 en 500 |
| Heure | 12, graisse 500, `#949ba4`, à 4 du nom | 12, `faint` | `typography.small` | couleur après 2.1 |
| Nom de salon (liste) | 16, graisse 500 ; non lu : 600 et blanc | 15 ; non lu : demi-gras | `body` | 16 en 500 |
| Nom du salon (en-tête) | 16, graisse 600 | 16 demi-gras | `title` | rien |
| Nom du serveur (en-tête) | 16, graisse 600 | 16 demi-gras | `title` | rien |
| En-tête de catégorie | Ash : 12, 600, capitales, interlettrage 0,24. Dark : ≈ 14, 500, casse de l'auteur *(à vérifier)* | 12, capitales, demi-gras | `small` | suivre le thème calqué |
| En-tête de groupe de membres | 12 à 14, 600, « En ligne — 12 » | 12, capitales | `small` | idem |
| Boutons | 14, graisse 500 | 14, normale | `typography.button` | graisse 500 (demande une graisse moyenne, voir chapitre 3) |
| Titres dans un message | 24, 20, 16 en 700 | 24, 20, 17 | `markdown.h1..h3` | `h3` à 16 |
| Code | Consolas (Windows), 14, interligne 18 | police à chasse fixe d'egui (Hack), ≈ 15 | `ui/fonts.rs:114` | Consolas du système en tête de la famille `Monospace` (0 octet), taille 14 |
| Titre de page des paramètres | 20 à 24, 600 à 700 | 20 demi-gras | `title × 1,25` | rien |
| Logo de l'écran de connexion | titre de 24, 600 | 30, bicolore | `typography.logo` | voir 2.17 |

Trois graisses ne suffisent pas : Discord écrit presque toute l'interface en
**500** (noms, salons, boutons, onglets). Avec Segoe UI, il n'y a rien entre
400 et 600 : tout ce qui est « moyen » chez Discord est soit trop maigre, soit
trop gras chez nous. C'est, avec la taille 15, la première cause de l'air
« différent » du texte.

### 2.3 Rail des serveurs

| Élément | Discord | Ferrocord | Jeton | Correction |
|---|---|---|---|---|
| Largeur | 72 | 72 | `layout.rail_width` | rien |
| Icône de serveur | Ash : 48, ronde, devient carré arrondi (rayon 16) au survol. Dark : ≈ 40, **carré arrondi en permanence** (rayon ≈ 12) *(à vérifier)* | 48, ronde puis rayon 16 | `rail_button`, `shapes.rail`, `rail_selected` | suivre le thème calqué : `rail_button` 40, `rail` = `rail_selected` = 12 |
| Écart entre icônes | 8 | 8 | `rail_gap` | rien |
| Pilule d'état | large de 8 dont 4 visibles, rayon 4 ; hauteurs 8 (non lu), 20 (survol), 40 (ouvert) ; blanche | large de 4, hauteur en part de la pastille | `shapes.unread_ring`, `Tokens::rail_mark` | hauteurs fixes 8, 20, 40 |
| Bouton des MP | logo Discord sur aplat ; accent quand il est ouvert | bulle pleine, accent quand ouvert | `Icon::Chat` | garder (le logo n'est pas copiable) |
| Filet sous les MP | 32 × 2, rayon 1, `#35363c` | court filet | `rail.rs` | 32 × 2 |
| Pastille de mentions | 16 de haut, largeur 16 au moins, rayon 8, texte 12 en 700, anneau de 4 de la couleur du rail, en bas à droite | pastille rouge avec chiffre, anneau | `layout.badge`, `danger_fill()` | vérifier 16 / 12 / anneau 4 |
| « + » | vert sur fond de pastille, plein au survol | idem | — | rien |
| Dossier fermé | carré arrondi teinté (couleur à 40 %), grille 2 × 2 d'icônes de 16 | grille 2 × 2 | `rail.rs` | vérifier la teinte |
| Dossier ouvert | fond arrondi continu derrière ses serveurs | *(à vérifier)* | `rail.rs` | — |

### 2.4 Colonne des salons

| Élément | Discord | Ferrocord | Jeton | Correction |
|---|---|---|---|---|
| Largeur | 240 (redimensionnable depuis 2025) | 240 | `channels_width` | poignée de redimensionnement (M20) |
| **En-tête de serveur** | haut de 48, marge 16, filet ou ombre dessous | `head_row()` 33 + 2 × 10 = **53** | `guild_title_y`, `Tokens::head_row` | hauteur fixe de 48 (jeton `header_height`), contenu centré |
| Ligne de salon | 32 de haut + 2 d'écart (pas de 34), rayon 4 (Ash) ou 8 (Dark), marge latérale 8, marge intérieure 8 | 32, rayon 6, marge 8 | `channel_row`, `shapes.row` | écart de 2 entre lignes ; rayon selon le thème |
| Icône de salon | 20, à 6 du nom, `#80848e` | ≈ 16 à 18, `faint` | `Tokens::icon_side`, `channel_row_parts` | 20 |
| Nom | 16 en 500, `#949ba4` au repos, `#dbdee1` au survol, blanc ouvert ou non lu | 15, `muted` `#a9aeb7` au repos | `body`, `muted` | au repos trop clair : un jeton `channel_idle` (`#949ba4`) |
| **Non lu** | pastille blanche de 8 × 8 à moitié sortie du bord gauche (4 visibles), rayon 4 | barre de 4 × 13 (40 % de la ligne), collée au bord | `Tokens::unread_mark` | 4 × 8, rayon à droite |
| **Mention** | pastille rouge **chiffrée** à droite : 16 de haut, rayon 8, 12 en 700 | **point rouge** de rayon 4, sans chiffre | `shapes.mention_dot` | pastille chiffrée (le compte existe déjà pour le rail) |
| En-tête de catégorie | marge haute 16, chevron de 12 à gauche, « + » de 18 à droite au survol | chevron, « + » toujours visible | `category_gap` | « + » au survol de la catégorie seulement *(à vérifier)* |
| Participants vocaux | avatar de 24, ligne de 32, retrait de 32 ; anneau vert de 2 quand ça parle | avatar ≈ 26 à 28 avec anneau | `voice_avatar_in`, `speaking_ring` | avatar 24 |
| Ligne de MP | 42 de haut (pas de 44 avec l'écart), avatar 32, nom 16 en 500, sous-ligne de statut 12 | `dm_row()` = 44, nom seul | `Tokens::dm_row` | 42 ; statut ou activité en sous-ligne |
| Ligne « Amis » | icône de 24, 42 de haut | icône, 44 | idem | 42 |

### 2.5 Panneau utilisateur

| Élément | Discord | Ferrocord | Correction |
|---|---|---|---|
| Hauteur | Ash : 52, bande. Dark : ≈ 56, **carte flottante** bordée, rayon ≈ 8 à 12, marges de 8, à cheval sous le rail et la colonne | 32 + 2 × 10 = 52, bande dans la colonne | M3 |
| Avatar | 32 (Ash), ≈ 40 (Dark) avec présence | 32 | suivre le thème |
| Texte | nom 14 en 600 ; dessous, 12 : statut, qui bascule sur le nom d'utilisateur au survol | nom seul, 15 demi-gras | deux lignes |
| Boutons | micro, casque, engrenage : 32 × 32, rayon 4 à 8, icône de 20 ; micro et casque barrés d'un trait rouge quand ils sont coupés ; petits chevrons de réglage (Dark) | trois boutons d'icône | vérifier 32 et l'icône de 20 |
| Clic sur l'avatar | menu du compte (statut, changer de compte) | sélecteur de compte | voir `docs/ecarts-fonctions.md` |

### 2.6 En-tête de la discussion

| Élément | Discord | Ferrocord | Jeton | Correction |
|---|---|---|---|---|
| **Hauteur** | 48, filet de 1 dessous | **53** | `chat_head_y` (10), `head_row()` | 48 fixe, même jeton que 2.4 |
| Icône du salon | 24, `#80848e`, à 8 du nom | 20 (`typography.icon`) | `icon` | 24 |
| Nom | 16 en 600 | 16 demi-gras | — | rien |
| Sujet | filet vertical de 24, puis 14 en 400, `#b5bac1`, tronqué | absent | — | M4 |
| Icônes | 24 chacune, écart 16, `#b5bac1`, blanches au survol, **actives en blanc** (membres ouverts) | 20, serrées | `chat.rs:480` | 24, écart 16, état actif |
| **Recherche** | Ash : 144 de large (240 au clavier), 24 de haut, rayon 4, fond `#1e1f22`, loupe **à droite**, 14. Dark : ≈ 244 × 32, rayon 8, loupe à gauche, « Rechercher *serveur* » | ≈ 160 × 30, rayon 8, sans loupe, « Rechercher » | `ui/search.rs` (`search_field`) | loupe, largeur et hauteur du thème calqué |
| Boîte et aide | Ash : dans l'en-tête, à droite de la recherche. Dark : dans la barre de titre | boîte dans l'en-tête, à gauche de la recherche | — | avec M1 |

### 2.7 Messages

| Élément | Discord | Ferrocord | Jeton | Correction |
|---|---|---|---|---|
| Avatar | 40, à 16 du bord ; texte à **72** du bord | 40, marge 16, écart 14 : texte à **70** | `avatar_gap` | 16 |
| Entre deux groupes | 17 (1,0625 rem) | 16 | `message_gap` | 17 |
| Dans un groupe | 0 d'écart, interligne 22, marge 2 haut et bas | 2 | `message_gap_grouped` | à régler avec l'interligne de 22 |
| Heure d'un message groupé | au survol, à gauche, dans la colonne de l'avatar, 11 | *(non vu sur les captures)* | `message.rs` | à vérifier |
| Marge droite | 48 | 16 | `message_x` | marge droite propre (`message_right`, 48) |
| **Survol** | voile sur toute la largeur, sans rayon | voile `hover`, rayon 0 | `shapes.hover` | couleur après 2.1 |
| **Barre de survol** | à droite, à cheval de 16 au-dessus du message ; haute de 32 ; boutons de 32 (Ash : 24 + marges) avec icône de 20 ; rayon 4 (Ash) ou 8 (Dark) ; fond de la discussion, contour de 1, ombre légère | flottante, fond `bg3`, contour, six icônes ≈ 18 | `ui/msg_actions.rs:448`, `float_frame` | contenu M14 ; fond `bg2` ; icônes de 20 |
| Message qui me mentionne | fond jaune à 10 % (`#f0b232`), barre jaune de 2 à gauche | *(à vérifier : scène non vue)* | `message.rs` | — |
| Message en réponse ciblée (saut) | fond d'accent à ≈ 10 %, qui s'estompe | *(à vérifier)* | — | — |
| « (modifié) » | 10, `#949ba4` | `EDITED`, `small` | — | taille 10 à 11 |
| Étiquette APP / BOT | aplat d'accent, texte blanc 10 en 600, rayon 4, hauteur 15, « APP » avec coche si vérifié | aplat d'accent, « BOT » ou « APPLI » | `members.rs`, `message.rs` | libellé « APP », taille 10 |
| Message système | icône de 16 dans la colonne de l'avatar (flèche verte d'arrivée), texte `#949ba4`, noms en blanc | icône, texte courant | `ui/system_msgs.rs` | texte en `muted`, flèche verte |

### 2.8 Séparateurs

| Élément | Discord | Ferrocord | Correction |
|---|---|---|---|
| Date | filet de 1 `#3f4147` sur toute la largeur (marges 16), texte centré 12 en 600, `#949ba4`, fond de la discussion sous le texte, marges 24 en haut et 8 en bas | texte centré 12, filet court ou absent selon la capture (`04-mp.png` : pas de filet visible à droite et à gauche) | filet pleine largeur ; `chat.rs` (`separator`) |
| Nouveaux messages | filet rouge `#f23f43` de 1, étiquette pleine à droite : aplat rouge, « NOUVEAU » blanc 10 en 700, capitales, rayon 4 côté droit, pointe à gauche | filet `danger` (`#fb8389`, rosé), mot « Nouveaux » en texte | M16 ; utiliser `danger_fill()` pour le filet aussi |
| Date et nouveaux à la fois | le filet de date passe au rouge, date en rouge, étiquette à droite | *(à vérifier)* | — |

### 2.9 Contenu des messages

| Élément | Discord | Ferrocord | Jeton | Correction |
|---|---|---|---|---|
| **Mention** | fond d'accent à 30 % (`rgba(88,101,242,.3)`), texte `#c9cdfb`, graisse 500, rayon 3, marge 0 × 2 ; survol : aplat `#5865f2`, texte blanc | fond accent à 30 %, texte `#d5d9ff`, graisse normale | `markdown.mention_bg`, `mention_text` | graisse 500, survol plein |
| Mention de rôle | couleur du rôle, fond de cette couleur à 10 % | fond dérivé | `role_mention_bg` | rien |
| **Code en ligne** | fond `#2b2d31` (Ash), rayon 4, marge 0 × 3, contour de 1 (Dark), 85 % de la taille | fond `code_bg` `#1e1f22`, sans marge visible (« du` code `et » sur `02-serveur.png`) | `markdown.code_bg` | marge de 3, taille 85 % |
| **Bloc de code** | fond `#2b2d31`, contour `#1e1f22` de 1, rayon 4, marge 8, Consolas 14 / 18 ; bouton « copier » au survol | fond `#1e1f22`, contour `border`, rayon 6, marge 10 | `code_pad`, `code_radius` | marge 8 ; bouton copier ; fond plus clair que la discussion en Ash, plus sombre en Dark |
| **Citation** | barre de **4**, rayon 4, `#4e5058`, texte à 12 de la barre | barre de **3**, écart 12 | `quote_width` | 4 |
| Spoiler | aplat `#1e1f22`, rayon 4 ; révélé : fond blanc à 10 % | aplat `#111214` | `markdown.spoiler` | `#1e1f22`, fond révélé |
| Lien | `#00a8fc`, souligné au survol | idem | `link` | rien |
| **Embed** | fond `#2b2d31`, **barre de 4 à gauche, rayon 4**, pas de contour (Ash) ; marge 8 en haut, 16 à droite et en bas, 12 à gauche ; largeur 520 au plus (432 sans image) ; titre 16 en 600, description 14 / 18, champs 14, pied 12 en 500 | carte `bg1` avec **contour** `border`, rayon 6, marge 12, barre de 4 | `card_frame`, `embed_pad`, `shapes.embed` | sans contour en Ash ; marges 8 / 16 / 16 / 12 ; tailles 16, 14, 12 |
| Image | rayon 8, pas de contour ; 550 × 350 au plus ; mosaïque à écart de 4 | rayon 6 ; 400 × 300 au plus ; écart 6 | `r_media`, `media_width`, `media_height`, `media_gap` | 8 ; 550 × 350 ; 4 |
| Fichier joint | carte `#2b2d31`, contour `#232428`, rayon 8, marge 16, icône de fichier de 30 × 40, nom en lien 16, taille 12 ; téléchargement au survol | carte du thème, icône, nom, taille | `file_card` | marge 16 |
| **Réaction** | pastille haute de 24 (+ contour de 1), rayon 8, fond `#2b2d31`, marge 2 × 6, emoji 16, compte 14 en 600 à 6 ; **la mienne** : fond accent à 15 %, contour `#5865f2`, compte `#dee0fc` ; écart 4 ; « + » fantôme au survol du message | pastille ≈ 28, rayon 6, fond `bg3`, emoji 20, « + » **toujours visible** | `reactions.rs`, `markdown.emoji` | rayon 8, emoji 16, « + » au survol seulement |
| Emoji dans le texte | 22 (1,375 em) ; seul : 48 | 20 ; 48 | `markdown.emoji`, `jumbo` | 22 |
| Autocollant | 160 | 160 | `markdown.sticker` | rien |
| Sondage | carte `#2b2d31`, rayon 8, réponses hautes de ≈ 50, case ronde à droite, barre de fond au vote, bouton « Voter » d'accent | carte, réponses ≈ 28, pourcentage à droite | `ui/polls.rs` | réponses plus hautes, case à droite |
| Carte d'invitation | fond `#2b2d31`, rayon 4 à 8, marge 16, en-tête 12 en 700 capitales, icône 50 (rayon 16), bouton **vert** « Rejoindre » haut de 40 | icône ronde, bouton d'**accent** | `ui/invites.rs:50` | bouton vert (`online` foncé), icône carrée arrondie |
| Message transféré | barre grise de 4 à gauche (comme une citation), « ↱ Transféré » en italique 12, pied avec le salon d'origine | **carte encadrée** | `msg_actions.rs` (`FORWARD_LABEL`) | barre de citation, pas de carte |

### 2.10 Zone de saisie

| Élément | Discord | Ferrocord | Jeton | Correction |
|---|---|---|---|---|
| Carte | fond `#383a40`, rayon 8, hauteur 44 au moins, marges latérales 16, 24 sous la carte (l'indicateur de frappe s'y loge) | fond `bg3`, rayon 8, marge intérieure 14 × 10 (≈ 42), 16 dessous | `composer_inner_*`, `composer_bottom` | 44 ; 24 dessous, « … écrit » **sous** la carte |
| **Indicateur de frappe** | sous la carte, 12 en 500, trois points animés, noms en gras | **au-dessus** de la carte (`13-saisie.png`) | `composer.rs:400` | le déplacer dessous |
| À gauche | « + » dans un disque de 24, `#b5bac1` ; ouvre un menu (envoyer un fichier, créer un fil, créer un sondage, applications) | « + » dans un disque : ouvre le sélecteur de fichiers | `upload.rs:185` | menu |
| **À droite, dans l'ordre** | cadeau (Nitro), **GIF**, **autocollant**, **emoji**, puis lanceur d'applications | **sondage, micro, emoji** | `composer.rs:224-228` | GIF, autocollants, emoji ; sondage dans le menu « + » ; micro gardé ou mis dans le « + » (Discord de bureau n'enregistre pas) ; pas de cadeau |
| Bouton emoji | visage **gris** de 22, qui prend couleur et grossit (× 1,14) au survol, emoji tiré au hasard | emoji en couleur, fixe | `composer_emoji.rs:83` | gris au repos |
| Texte | 16, interligne 22, indice `#6d6f78` | 15 | `body` | 16 |
| Réponse en cours | bande au-dessus de la carte, collée (coins bas de la bande et haut de la carte droits), fond `#2b2d31`, « Réponse à **nom** », interrupteur « @ ACTIVÉ », croix | bande au-dessus *(scène absente de `preview`)* | `msg_actions.rs` | coins joints |
| Envoi en cours, pièces jointes | tuiles de 200 × 200 dans la carte, au-dessus du texte, trois boutons au survol (spoiler, modifier, retirer) | tuiles dans un plateau | `upload.rs:202` | dans la carte |
| Mode lent | sous la carte à droite, « Mode lent activé », sablier | au-dessus, à gauche | `composer.rs:400` | à droite, dessous |

### 2.11 Liste des membres

| Élément | Discord | Ferrocord | Jeton | Correction |
|---|---|---|---|---|
| Largeur | 240 (Ash), ≈ 264 (Dark) *(à vérifier)* | 240 | `members_width` | suivre le thème |
| Fond | celui de la colonne des salons (Ash) ; celui de la discussion avec filet (Dark) | `bg1` | — | idem |
| En-tête de groupe | 12 en 600, capitales (Ash), `#949ba4`, marge 24 en haut, 16 à gauche, hauteur 40 | 12, capitales, `muted`, hauteur 40 | `member_header` | couleur `faint` |
| Ligne | 42 de haut + 2 d'écart = pas de 44, rayon 4 (Ash) ou 8, marge latérale 8, intérieure 8 | 42 | `member_row` | pas de 44 |
| Avatar | 32, présence de 10 avec anneau de 3 | 32, pastille de rayon 5 et anneau de 2 | `Tokens::status_dot` | anneau 3 |
| Nom | 16 en 500, couleur du rôle | 15 | `body` | 16 en 500 |
| Activité | 12 en 500, `#949ba4`, « Joue à **Jeu** » (jeu en gras), icône de Rich Presence à droite | 12, `muted` | `small` | nom du jeu en gras |
| Hors ligne | ligne à 30 % d'opacité, 100 % au survol | nom en `muted` | `members.rs` | opacité |
| Couronne du propriétaire, icône de rôle, emblème de boost | à droite du nom, 14 à 20 | *(non vus)* | `members.rs` | à ajouter |

### 2.12 MP (pas de bulles)

Sur le bureau, Discord **n'affiche pas de bulles** dans les MP : même rendu
que dans un salon. Ferrocord fait de même : aucun écart sur ce point. Les
écarts des MP sont ailleurs : début de conversation (M7), colonne (M9, 2.4),
en-tête (M6), panneau de profil à droite (absent).

### 2.13 Page Amis

| Élément | Discord | Ferrocord | Correction (`ui/friends.rs`) |
|---|---|---|---|
| En-tête | icône Amis de 24, « Amis » en 600, filet vertical, onglets, bouton **vert plein** « Ajouter un ami » ; à droite : nouveau groupe (icône), boîte, aide | « Amis », onglets, « Nouveau groupe » et « Ajouter un ami » en **texte nu** à droite | icône ; bouton vert ; « Nouveau groupe » en icône |
| Onglet | 16 en 500, marge 2 × 8, rayon 4 ; actif : fond `#404249`, texte blanc ; survol `#35373c` | actif : fond d'**accent translucide**, texte bleu clair | fond neutre (`selected`) |
| Recherche | pleine largeur, haute de 36 à 40, fond `#1e1f22`, rayon 4 à 8, loupe à droite, marges 16 × 30 | ≈ 30 de haut, sans loupe | 40, loupe |
| Compte | « EN LIGNE — 2 », 12 en 600, capitales, marges 16 × 30 | « 2 amis », 12 | libellé par onglet |
| **Ligne** | haute de **62**, filet de 1 au-dessus, marges latérales 30 ; au survol : fond `#393c41`, rayon 8, filet masqué | ≈ 46, sans filet | 62, filet |
| Texte | nom 16 en 600 ; le nom d'utilisateur n'apparaît qu'au survol ; dessous, statut 14 `#b5bac1` | nom et nom d'utilisateur toujours ; statut 12 | 14 ; nom d'utilisateur au survol |
| **Actions** | **deux disques de 36** (`#2b2d31`, icône de 20) : bulle (message), « ⋮ » (plus : appel, retirer) ; en attente : coche verte et croix rouge au survol | **trois boutons texte** « Message », « Retirer », « Bloquer », les deux derniers en rouge | disques ; « Retirer » et « Bloquer » dans le menu « ⋮ » |
| Vide | illustration et phrase | phrase | motif maison, pas l'illustration de Discord |
| Ajouter un ami | page dédiée : titre, phrase, grand champ de 50 avec bouton d'accent **dans** le champ | *(non capturé)* | à vérifier |

### 2.14 Carte de profil et profil complet

| Élément | Discord | Ferrocord | Jeton | Correction |
|---|---|---|---|---|
| Largeur de la carte | 300 | 320 | `profile_width` | 300 |
| Bannière | 105 de haut (300 × 105) ; sans image : couleur, 60 de haut ≈ | 320 × 6/17 = 113 ; sans : 48 | `profile_banner` | 105 / 60 |
| Avatar | 80, anneau de 6 de la couleur de la carte, à 16 du bord, présence de 16 avec anneau | 80, anneau de 5 | `profile_avatar`, `profile_ring` | anneau 6 |
| Boutons sur la bannière | en haut à droite : ami, « ⋯ », disques de 32 translucides | absents (clic droit) | `profile.rs` | à ajouter |
| Statut personnalisé | **bulle** à droite de l'avatar, à cheval sur la bannière | ligne dans le corps (emoji puis texte) | — | bulle |
| Nom | 20 en 700 ; dessous nom d'utilisateur 14, puis pronoms, séparés par « • » ; badges sur la même ligne dans un cadre arrondi | 16 demi-gras ; nom d'utilisateur ; « Anciennement … » ; badges sur leur ligne avec un libellé | `title` | 20 en 700 ; badges en ligne, sans libellé (infobulle) |
| Sections | la carte courte ne montre que : activité (carte interne), bio tronquée, rôles, **champ « Envoyer un message à @nom »** ; le reste est dans le profil complet | tout : activité, à propos, quatre dates en quatre sections, rôles, compteurs, note, bouton (`07-profil.png` déborde de l'écran) | `profile.rs` | raccourcir : dates en **une** ligne « Membre depuis » (icône Discord · date, icône du serveur · date) ; note et compteurs dans le profil complet |
| Étiquette de section | 12 en 600 à 700 ; Ash en capitales, Dark en casse normale | 12, capitales | `small` | suivre le thème |
| Pastille de rôle | fond `#232428`, contour, rayon 4 (Ash) ou 8, hauteur 22 à 24, point de 12 de la couleur du rôle, 12 en 500 | rayon 6, point | `profile_chip_margin` | rien d'urgent |
| Fond de la carte | `#111214` (Ash), contour de 1 ; thème du profil : dégradé avec cadre intérieur | `bg2` + contour | `window_frame` | fond plus sombre que la discussion |
| **Profil complet** | fenêtre de **600** de large, bannière de 210 ≈, avatar de **120** ; depuis 2024 : deux zones, à gauche l'identité et les boutons (« Envoyer un message » d'accent, ami, « ⋯ »), à droite un panneau à onglets (Activité, Amis en commun, Serveurs en commun) ; à propos, membre depuis, connexions, note dans une carte | 640 de large (2 × 320), bannière 0,3 × largeur = 192, avatar de **80**, une colonne, boutons « Envoyer un message » et « Fermer » gris, onglets à fond d'accent | `profile_full_width`, `profile_full_banner` | avatar 120 ; bouton principal d'accent ; pas de bouton « Fermer » (croix ou clic hors de la fenêtre) ; onglets soulignés |
| Onglets du profil | texte, **soulignement** de 2 sous l'actif | pastille pleine | `profile_full.rs` | soulignement |
| Comptes liés | cartes bordées en grille 2 colonnes, icône du service de 24, nom, coche, flèche | lignes « GitHub lien Vérifié » | `profile_full.rs` | cartes |

### 2.15 Paramètres

Référence : la disposition plein écran (encore en service en 2025). Discord a
commencé à déployer des paramètres en **fenêtre modale** avec colonne à icônes
et recherche *(à vérifier sur le client du jour : si c'est la disposition
actuelle, c'est elle qu'il faut viser)*.

| Élément | Discord | Ferrocord | Jeton, fichier | Correction |
|---|---|---|---|---|
| Colonne | 218 de large, fond de la colonne des salons, marge 60 en haut, 6 à droite | 218, `bg1` | `DialogGeometry::nav_width` | marge haute 60 |
| Ligne de catégorie | 32 de haut, rayon 4, marge 6 × 10, 16 en 500, écart 2 ; ouverte : `#404249`, blanc | 32, rayon 6, 15 | `channel_row` | écart 2, 16 |
| En-tête de groupe | 12 en 700, capitales, marge 6 × 10 | 12, capitales | — | rien |
| Filet de groupe | 1, marges 8 × 10 | filet | — | rien |
| Page | 740 au plus, marges 60 en haut, 40 de côté, 80 en bas | 740 | `page_width`, `margin` | vérifier 60 / 40 |
| Titre de page | 20 en 600, marge basse 20 | 20 demi-gras | — | rien |
| **Bouton de fermeture** | disque de 36, contour de 2 `#b5bac1`, croix de 18 ; « ÉCHAP » 13 en 600 dessous ; à 21 à droite de la page, fixe | disque, « ÉCHAP » | `shell.rs` | vérifier 36 / 2 / 13 |
| **Mon compte** | **carte de profil** : bannière de 100, avatar de 80 à cheval, nom en 20, bouton d'accent « Modifier le profil utilisateur » ; dessous, encart plus sombre (rayon 8, marge 16) avec les lignes « NOM D'AFFICHAGE / valeur / bouton Modifier » | liste de lignes libellé-valeur, sans carte, sans avatar | `ui/settings/account.rs` | carte d'en-tête ; encart |
| Interrupteur | 40 × 24, pilule ; activé : **vert `#23a55a`** avec coche dans le curseur ; coupé : `#80848e` avec croix | 40 × 24, vert `online`, gris `offline` | `shell::switch` | coche et croix dans le curseur |
| Bouton | haut de 32 (petit) ou 38 à 40 (moyen), rayon 3 (Ash) ou 8 (Dark), 14 en 500 ; primaire `#5865f2` (survol `#4752c4`) ; secondaire `#4e5058` (survol `#6d6f78`) ; danger plein `#da373c` ; danger en contour ; lien | haut de 33 (14 + 2 × 6 + interligne), rayon 8, neutre `#4a4c54` | `control_y`, `button_bg`, `control_bg` | hauteur 32 fixe ; `button_bg` `#4e5058` ; survol `#6d6f78` |
| Champ | haut de **40**, fond `#1e1f22`, rayon 3 (Ash) ou 8, marge 10, 16 ; contour au clavier `#00a8fc` (Ash) ou d'accent | ≈ 30, fond `bg0`, contour `border`, contour d'accent au clavier | `Tokens::apply` | hauteur 40 pour les champs de formulaire |
| Liste de choix | haute de 40, chevron à droite, liste déroulante bordée, option survolée `#35373c`, option choisie avec coche d'accent | liste d'egui, triangle plein | `Tokens::apply` (ComboBox) | chevron fin à la place du triangle ; hauteur 40 |
| Curseur | piste de 8, rayon 4, partie pleine d'accent, poignée blanche 10 × 24 (rayon 3) ; repères avec valeurs au-dessus | curseur d'egui, poignée ronde | `visuals()` | poignée rectangulaire ; repères |
| Case à cocher, bouton radio | case de 24, rayon 6, coche blanche sur accent ; radio : ligne entière en carte `#2b2d31` de 46 à 50, rond de 20 à gauche, carte choisie plus claire | cases d'egui (petites) | — | radios en cartes pour les choix des paramètres |
| Barre « modifications non enregistrées » | barre flottante en bas : texte, « Réinitialiser » (lien), « Enregistrer » (vert), qui tremble si on veut sortir | boutons dans la page (`UNSAVED`, `RESET`, `SAVE`) | `shell.rs` | barre flottante |

### 2.16 Menus contextuels, infobulles, modales

| Élément | Discord | Ferrocord | Jeton | Correction |
|---|---|---|---|---|
| **Menu** : cadre | fond `#111214` (Ash), rayon 4 (Ash) ou 8, marge 6 × 8, largeur 188 au moins, ombre 0 × 8 × 16 noir à 24 %, contour de 1 (Dark) | fond `bg3` `#34363c`, contour, rayon 6, marge 7 | `float_frame`, `menu_corner_radius`, `menu_margin` | fond plus sombre que la discussion (jeton `menu_bg`) ; largeur minimale 188 |
| Menu : ligne | haute de 32, marge 6 × 8, rayon 2 (Ash) ou 4, 14 en 500, `#b5bac1` ; **survol : aplat `#5865f2`, texte blanc** (Ash) ; aplat neutre en Dark *(à vérifier)* ; icône de 18 **à droite** | bouton d'egui : survol `row_hover` gris, pas d'icône | `Tokens::apply` (widgets) | composant `menu_item(texte, icône, danger)` commun ; survol d'accent |
| Menu : danger | texte `#f23f43` ; survol : aplat rouge, texte blanc | texte courant (« Supprimer le message » non distingué *(à vérifier)*) | — | rouge |
| Menu : sous-menu, case, radio | chevron à droite ; case et rond **à droite** | `menu_button` d'egui ; radio **à gauche** (`settings/menu.rs:77`) | — | à droite |
| Menu : filet | 1, `#2e2f34`, marge 4 | filet d'egui | — | rien |
| **Infobulle** | fond `#111214`, texte `#dbdee1` 14 en 500 (16 en 600 pour le rail), rayon 4 à 8, marge 8 × 12, **flèche** de 5, ombre, apparition immédiate, placée au-dessus (à droite pour le rail), largeur 190 au plus | cadre du thème, sans flèche, délai d'egui | `Tokens::apply`, `interaction.tooltip_delay` | composant d'infobulle maison ; délai 0 sur les icônes |
| **Modale** : cadre | largeur 440 (petite), 600 ou 800 ; fond de la discussion ; rayon 4 (Ash) ou 12 à 16 ; pas de contour (Ash) ; voile noir à 85 % | 400, `bg2`, contour, rayon 12, voile à 70 % | `card_width`, `scrim` | 440 ; voile plus dense |
| Modale : en-tête | titre **centré** 24 en 700 (ou à gauche 20 en 600), sous-titre 16 centré `#b5bac1`, croix en haut à droite | titre à gauche 16 demi-gras, pas de croix | `ui/modal.rs` | titre 20 à 24 ; croix |
| **Modale : pied** | **bande plus sombre** (`#2b2d31`) sur toute la largeur, marge 16 ; boutons **alignés à droite**, « Annuler » en lien ou secondaire à gauche du principal | boutons **à gauche**, principal en premier, pas de bande (`11-modale.png`) | `ui/modal.rs` | bande ; à droite ; ordre inversé |
| Modale : apparition | échelle 0,7 → 1 et fondu, ≈ 200 ms à ressort | fondu d'egui | `ui/motion.rs` | facultatif |
| Notification en bas | bandeau vert ou rouge en haut de l'application (connexion), pastilles discrètes | carte en bas de la discussion | `r_toast` | rien |
| Barres de défilement | discussion : rail de 16 avec poignée de 8, rayon 4, `#1a1b1e` sur `#2b2d31` ; colonnes : fine (4 à 8), visible au survol seulement | barres flottantes d'egui | `Tokens::apply` (`spacing.scroll`) | barre pleine de 8 dans la discussion |

### 2.17 Sélecteur d'emojis

| Élément | Discord | Ferrocord (`06-emojis.png`) | Fichier, jeton | Correction |
|---|---|---|---|---|
| Fenêtre | ≈ 498 de large sur 498 de haut au plus (réduite si la fenêtre est basse), rayon 8, fond de la colonne des salons, ombre | ≈ 330 × 380 | `ui/emoji_picker.rs`, `GRID_ROWS` | ≈ 480 × 460 ; hauteur bornée par la fenêtre (défaut 11 de l'audit d'alignement) |
| Onglets | **GIF, Autocollants, Emojis** (Sons en vocal), 16 en 500, onglet actif à fond neutre | Emojis, Autocollants ; actif à fond d'accent | — | ordre et couleur ; GIF (lot G) |
| Recherche | pleine largeur, haute de 30 à 34, loupe à droite, bouton de teinte (main) à droite du champ | champ à contour d'accent, pastille verte à droite (teinte) | — | loupe |
| **Rail de catégories** | colonne de 48 à gauche : étoile (favoris), horloge (fréquents), icônes des serveurs (32, rondes), filet, huit catégories Unicode en pictogrammes gris | absent ; une rangée de dix emojis sous la recherche | — | rail |
| **Cellule** | 48 × 48, emoji de **40**, rayon 4 au survol (`#404249`) ; 9 par ligne | emoji de 32, ≈ 10 par ligne | `markdown.picker_emoji` | 40 dans des cellules de 48 |
| En-tête de section | collant, 12 en 600, capitales, icône du serveur de 16 à gauche, chevron de repli | 12, capitales | — | collant, icône, repli |
| **Bandeau du bas** | haut de 48, fond plus sombre : emoji survolé en 28, `:nom:` en 16 en 600, serveur d'origine | ligne « Maj+clic : en choisir plusieurs » | — | bandeau |
| Emojis verrouillés | grisés, cadenas | *(non vu)* | — | — |

### 2.18 Écran de connexion

| Élément | Discord | Ferrocord (`01-connexion.png`) | Correction (`ui/login.rs`) |
|---|---|---|---|
| Fond | illustration plein écran, logo en haut à gauche | aplat `bg0` | motif ou dégradé maison (jamais l'illustration de Discord) |
| Carte | **784** de large, deux colonnes (formulaire à gauche, QR à droite, filet entre les deux), rayon 5 à 8, marge 32, fond de la discussion, ombre | **400**, une colonne, contour | 720 à 784 en deux colonnes quand la fenêtre le permet |
| Titre | « Ha, te revoilà ! » 24 en 600, sous-titre 16 `#b5bac1` | logo « Ferrocord » de 30 bicolore, sous-titre | titre 24 ; le nom reste le nôtre |
| Libellés | 12 en 700, capitales, astérisque rouge | « JETON DISCORD » 12, centré | à gauche, 700 |
| Champ | haut de 40, fond `#1e1f22`, rayon 3 à 8, 16 | ≈ 30 | 40 |
| Bouton | pleine largeur, haut de **44**, d'accent, 16 en 500, rayon 3 à 8 | haut de 40 (`button_height`) | 44 |
| QR | à droite : QR de 176 sur fond blanc (rayon 4, marge 8), logo au centre, titre 24 « Se connecter avec un code QR », phrase | bouton « Se connecter avec un QR code » qui ouvre un autre écran | QR affiché d'emblée à droite (la requête ne part qu'à l'affichage : garder un geste si la règle 1.2.3 l'exige, par exemple « Afficher le code ») |
| Avertissement | — | paragraphe « client non officiel » | à garder |

### 2.19 Vocal et appel

| Élément | Discord | Ferrocord | Correction |
|---|---|---|---|
| Panneau vocal | « Voix connectée » vert 14 en 600 avec icône de signal à gauche (clic : latence et détails) ; dessous, salon / serveur 12 `#949ba4` ; à droite : suppression de bruit, raccrocher ; dessous : rangée de quatre boutons hauts de 32, largeur égale, fond `#2b2d31` → `#35373c` | « Connecté au vocal » vert + « 38 ms » en clair, raccrocher, lien « Codes de vérification » | M18 ; latence dans une infobulle |
| Tuile d'appel | rayon 8, fond `#1e1f22` (ou couleur dominante de l'avatar), avatar de 80, nom en bas à gauche dans une pastille noire à 60 %, anneau vert de 2 quand ça parle | rayon 12, fond `bg3`, nom sur `scrim` | rayon 8 ; fond plus sombre |
| Barre de contrôle | flottante en bas, visible au mouvement ; boutons ronds de 56 (caméra, partage, activité, micro) ; raccrocher **rouge, en pilule large** | boutons ronds, actifs en accent, raccrocher rouge rond | pilule ; tailles |
| Fond de la vue d'appel | noir `#000000` | `bg2` *(à vérifier)* | noir |
| Appel dans un MP | bandeau noir d'environ 240 sous l'en-tête, la conversation reste affichée dessous ; avatars de 100 (anneau vert, points d'attente pour qui n'a pas décroché) ; blocs arrondis [micro ⌄ \| caméra ⌄] [partage, activités, sons, « … »] et raccrocher rouge 80 × 60 ; la vidéo agrandit le bandeau | **fait** (lot « appel », `ui/call_band.rs`). Avant : vue plein cadre à la place de la conversation, bouton « Discussion » | reste : pas de bouton « activités » (fonction absente) ; la sourdine est dans « … » |
| Message d'appel dans l'historique | combiné vert (rouge : manqué), « **Nom** a commencé un appel qui a duré 2 minutes. », heure courte à droite | **fait** (lot « appel ») | — |

---

## 3. Police et icônes : remplaçants libres

### 3.1 Police

gg sans est une police propriétaire de Discord : ni fichier ni tracé ne
peuvent être repris. Elle est géométrique-humaniste, assez large, à grande
hauteur d'x, avec des graisses 400, 500, 600, 700 et 800.

| Candidate | Licence | Proximité | Graisses | Coût à embarquer (estimation, à mesurer) |
|---|---|---|---|---|
| **Figtree** | OFL 1.1 | la plus proche d'allure : géométrique, ronde, amicale ; chiffres et « g » voisins | 300 à 900, italiques | ≈ 55 à 70 Ko par fichier statique ; 400, 500, 600, 700 + deux italiques ≈ **0,4 Mo** |
| Inter | OFL 1.1 | plus neutre et plus étroite ; excellente lisibilité à 12 à 16 ; couverture latine, grecque, cyrillique complète | 100 à 900, italiques | ≈ 300 à 400 Ko par fichier complet ; six fichiers ≈ **2 Mo**, ou ≈ 0,6 Mo après réduction aux jeux utiles |
| Noto Sans | OFL 1.1 | plus humaniste ; la plus grande couverture ; c'est le repli de Discord pour les autres écritures | 100 à 900 | ≈ 500 Ko par fichier ; trop lourd pour le gain |
| Segoe UI (actuelle) | système | la plus éloignée ; pas de graisse 500 | 400, 600, 700 | 0 |

Recommandation : **Figtree** en 400, 500, 600, 700 et en italique 400 et 700,
**Segoe UI gardée en repli** dans chaque famille (elle couvre le grec, le
cyrillique étendu et le reste, que Figtree n'a pas). Ajouter la graisse 500 est
ce qui rapproche le plus de Discord, plus encore que le dessin des lettres.

Coût : ≈ 0,4 Mo d'exécutable (1,3 % de 28,7 Mio), licence OFL à ajouter dans
`tools/licences` ; aucune mémoire privée en plus si les fichiers sont lus dans
l'image de l'exécutable (`include_bytes!`, pages partagées) ; atlas de glyphes
d'egui : une famille de plus (500), quelques centaines de Kio de texture. À
mesurer avec `--bench` : première image (les glyphes préchauffés par
`ui/warm.rs` changent) et mémoire. egui ne règle pas les axes d'une police
variable : il faut les fichiers statiques.

Fichiers : `src/ui/fonts.rs` (familles `medium()` et `demi()`), jeton
`typography.family` (« Figtree », « système ») pour pouvoir revenir à Segoe UI
sans rien embarquer côté rendu.

Code : **Consolas** du système en tête de la famille à chasse fixe (c'est ce
que Discord utilise sous Windows), 0 octet.

### 3.2 Icônes

Les icônes de Discord sont propriétaires. Leur style : **pleines** (aplats, pas
de contour), grille de 24, coins arrondis de 1 à 2, formes simples, parfois
deux tons (fond à 100 %, détail évidé).

Ferrocord dessine déjà ses pictogrammes par le code (`src/theme/icons.rs`,
27 icônes ; `ui/voice/icons.rs` ; `ui/video/icons.rs`), mais à la façon d'un
trait, plus maigre et plus petit (16 à 20) que les aplats de 20 à 24 de
Discord. C'est visible dans l'en-tête et dans la zone de saisie.

Proposition : redessiner un jeu complet **plein, sur grille de 24**, dans le
même esprit, sans copier un seul tracé :

- soit à la main dans `theme/icons.rs` (formes simples : c'est faisable pour
  les ≈ 60 icônes nécessaires) ;
- soit en partant d'un jeu libre plein, retouché : **Phosphor « Fill »** (MIT),
  **Tabler « filled »** (MIT) ou **Material Symbols Rounded, rempli**
  (Apache 2.0). Les tracés SVG sont convertis à la compilation en listes de
  points (`build.rs`), tessellés une fois par taille : ≈ 20 à 40 Kio pour
  60 icônes, licence à citer dans « À propos ».

Icônes à ajouter ou à refaire (≈ 60) : dièse, dièse cadenassé, haut-parleur,
haut-parleur cadenassé, porte-voix, forum, média, conférence, fil, cloche,
cloche barrée, punaise, membres, loupe, boîte, aide, plus cerclé, cadeau
(non), GIF, autocollant, visage, applications, micro, micro barré, casque,
casque barré, engrenage, téléphone, caméra, caméra barrée, écran, écran
barré, raccrocher, signal, crayon, corbeille, répondre, transférer, « ⋯ »,
« ⋮ », lien, copier, identifiant, drapeau, couronne, boost, ami ajouté, bulle,
croix, coche, chevrons, boussole, télécharger, dossier, calendrier, étoile,
horloge, œil, cadenas, sablier, fichier, lecture, pause, plein écran.

Tailles : 24 dans l'en-tête et la zone de saisie, 20 dans les lignes de salon,
la barre de survol et le panneau utilisateur, 16 à 18 dans les menus.

---

## 4. Classement par visibilité

Ce qu'on remarque en premier, fenêtre ouverte sur un salon.

| Rang | Écart | Section | État au 15 novembre 2026 |
|---|---|---|---|
| 1 | Barre de titre de Windows au lieu d'une barre intégrée | M1 | fait |
| 2 | Police : Segoe UI, pas de graisse 500, texte à 15 au lieu de 16, interligne plus serré | 2.2, 3.1 | fait (Figtree, 16, interligne 22) |
| 3 | Teintes des surfaces : ni Ash ni Dark ; trois gris sans filets | 2.1 | fait (Sombre, Cendre, Onyx) |
| 4 | Icônes au trait, petites (16 à 20) au lieu d'aplats de 20 à 24 | 3.2 | fait (73 icônes pleines) |
| 5 | En-tête : 53 de haut au lieu de 48, pas de sujet, icônes différentes, recherche sans loupe | 2.6, M4, M5 | fait |
| 6 | Zone de saisie : sondage et micro à la place de GIF et autocollants ; « … écrit » au-dessus | 2.10 | partiel : GIF et emoji à droite, « … écrit » sous la carte ; sondage et micro restent dans la barre, le « + » n'ouvre pas de menu |
| 7 | Panneau utilisateur : bande d'une ligne au lieu d'une carte à deux lignes | 2.5, M3 | fait |
| 8 | Colonne des salons : point rouge sans chiffre, barre de non-lu trop haute, noms trop clairs au repos, pas d'icônes au survol | 2.4 | fait |
| 9 | Barre de survol des messages : six icônes sans réactions rapides ni « ⋯ » | 2.7, M14 | fait |
| 10 | Menus contextuels gris à survol gris, sans icônes ; infobulles sans flèche | 2.16 | fait (`ui/menu.rs`, `widgets::tip`) |
| 11 | Embeds et messages transférés encadrés ; réactions à « + » permanent ; citation de 3 | 2.9 | fait |
| 12 | Modales : boutons à gauche, sans pied sombre, sans croix | 2.16 | partiel : le gabarit `widgets::modal` existe, mais plusieurs fenêtres ne l'utilisent pas (fenêtre d'une application, « Créer un serveur », pieds à boutons de styles différents) |
| 13 | Page Amis : boutons texte rouges, lignes basses, pas d'« Actifs en ce moment » | 2.13 | fait |
| 14 | Carte de profil trop longue ; profil complet à avatar de 80 et boutons gris | 2.14 | partiel : profil complet refait ; la carte reste longue (activité, à propos, dates, rôles) et sort de la fenêtre |
| 15 | Sélecteur d'emojis étroit, sans rail ni bandeau ; écran de connexion en une colonne | 2.17, 2.18 | fait |
| 16 | Début de salon et de MP réduits à une ligne ; étiquette « NOUVEAU » en texte | M7, M16 | fait |
| 17 | « Mon compte » sans carte de profil ; champs et boutons de 30 à 33 au lieu de 40 et 32 | 2.15 | partiel : carte de « Mon compte » faite ; petits boutons de 24 à côté de contrôles de 40 |
| 18 | Rail : pastilles de 48 rondes (Dark : 40, carrées arrondies), hauteurs de pilule | 2.3 | fait |
| 19 | Liste des membres : hors-ligne non estompés, pas de couronne ni d'icône de rôle | 2.11 | fait |
| 20 | Vocal : panneau sans rangée de boutons, tuiles et barre de contrôle | 2.19 | fait |

---

## 5. Découpage en lots

Trois lots. Les deux premiers ne partagent aucun fichier ; le troisième passe
après eux, parce qu'il retouche les écrans avec les composants et les jetons
qu'ils créent.

### Lot V1 — Fondations visuelles (L)

Thèmes, police, icônes, fenêtre. Aucun écran n'est redessiné à la main : tout
passe par les jetons et les composants partagés.

- Relevé à la pipette sur le vrai client (par l'utilisateur ou sur des
  captures qu'il fournit) : les valeurs « ≈ » du chapitre 2 sont remplacées par
  des mesures, consignées dans `docs/design.md`.
- Préréglages « Cendre », « Sombre » (Dark) et « Onyx » ; jetons ajoutés :
  `panel_border`, `menu_bg`, `channel_idle`, `header_height` (48),
  `message_right`, `typography.family`, `typography.line_height` (22).
- Corps de texte à 16, graisse 500, interligne 22 ; Figtree embarquée, Consolas
  pour le code.
- Jeu d'icônes pleines sur grille de 24.
- Barre de titre personnalisée (M1, M2), avec la boîte de réception et l'aide ;
  colonne des salons redimensionnable (M20).
- Composants partagés : `menu_item`, infobulle à flèche, pied de modale,
  bouton (primaire, secondaire, danger, lien ; 32 de haut), champ de 40,
  liste de choix, curseur, interrupteur à coche, barres de défilement.

Fichiers : `src/theme/` (tout), `src/ui/fonts.rs`, `src/ui/modal.rs`,
`src/ui/settings/shell.rs`, `src/ui/util.rs`, `src/ui/warm.rs`, `src/lib.rs`,
`build.rs`, `tools/licences` ; nouveaux : `src/ui/titlebar.rs`,
`src/ui/widgets.rs`, dossier des polices. Tests de contraste à adapter
(`theme/contrast.rs`).

### Lot V2 — Structure : rail, colonnes, en-tête, panneau utilisateur (M)

- Rail (2.3), colonne des salons (2.4 : pastille chiffrée, non-lu, icônes au
  survol, pas de 34), colonne des MP (M9), en-tête de serveur (M12).
- Panneau utilisateur en carte à deux lignes (M3, 2.5), panneau vocal (M18).
- En-tête de discussion à 48 : sujet, icônes de 24, recherche à loupe (2.6, M4
  à M6) — avec le lot S de `docs/ecarts-fonctions.md`, qui branche les
  fonctions derrière ces boutons : **un seul des deux lots** touche l'en-tête
  (le sortir d'abord dans `src/ui/chat_head.rs`).
- Liste des membres (2.11).

Fichiers : `src/ui/rail.rs`, `src/ui/channels.rs`, `src/ui/members.rs`,
`src/ui/voice/panel.rs`, `src/ui/voice/list.rs`, `src/ui/chat_head.rs`,
`src/ui/search.rs` (le champ seulement).

À faire en parallèle de V1 seulement si V1 a livré d'abord ses jetons (une
première fusion de V1 limitée à `src/theme/`), sinon après.

### Lot V3 — Contenu et écrans (L)

- Messages (2.7), séparateurs (2.8), contenu (2.9), zone de saisie (2.10),
  barre de survol (M14), début de salon et de MP (M7).
- Page Amis (2.13), carte et profil complet (2.14), « Mon compte » et réglages
  (2.15), sélecteur d'emojis (2.17), écran de connexion (2.18), appel (2.19).
- Scènes à ajouter à `examples/preview.rs` pour l'audit : menu contextuel,
  infobulle, épinglés, réponse en cours, message qui mentionne, ajouter un ami.

Fichiers : `src/ui/message.rs`, `src/ui/chat.rs` (liste), `src/ui/markdown.rs`,
`src/ui/embeds.rs`, `src/ui/attachments.rs`, `src/ui/reactions.rs`,
`src/ui/polls.rs`, `src/ui/invites.rs`, `src/ui/composer.rs`,
`src/ui/composer_emoji.rs`, `src/ui/msg_actions.rs`, `src/ui/friends.rs`,
`src/ui/profile.rs`, `src/ui/profile_full.rs`, `src/ui/settings/account.rs`,
`src/ui/emoji_picker.rs`, `src/ui/login.rs`, `src/ui/qr_login.rs`,
`src/ui/video/`, `examples/preview.rs`.

Ce lot partage `msg_actions.rs`, `emoji_picker.rs`, `friends.rs` et
`profile*.rs` avec les lots M, G et U de `docs/ecarts-fonctions.md` : le
lancer **après** eux, ou leur confier la retouche visuelle de leurs propres
écrans avec ce document comme cahier des charges.

### Vérification

Après chaque lot : `tools\captures.ps1` aux quatre tailles, planches
avant/après, et comparaison côte à côte avec une capture du vrai client à la
même taille et au même zoom, fournie par l'utilisateur.
