# Audit d'alignement (lot A)

Méthode : `tools\captures.ps1` lance l'exemple hors ligne `preview` sur chaque
scène (112 écrans et états) à quatre tailles de fenêtre — 1280×720, 1600×900,
1920×1000 et 1000×750 pixels physiques (à 125 %, soit 1024×576, 1280×720,
1536×800 et 800×600 points). Les captures « avant » viennent de `e8ccfb4`.
Chaque défaut est corrigé à la source (jeton du thème, composant partagé ou
style de page) ; les planches avant/après sont dans `captures-alignement\`.

État : **C** corrigé et revu sur capture, **R** reste à faire.

## Colonne des salons, barre des serveurs

| # | Défaut | Cause | État |
|---|--------|-------|------|
| 1 | Micro, casque et réglages 3 px au-dessus de l'axe de l'avatar dans le panneau utilisateur | ligne haute de 18 pt, boutons de 33 | C — `Tokens::user_bar_row` |
| 2 | Filet au-dessus du panneau utilisateur qui ne rejoint rien ; double filet avec le panneau vocal | trait par défaut des `Panel` | C — un seul filet, sous le panneau vocal |
| 3 | « + » des catégories 3 px trop bas et hors de l'axe des points de mention | bouton posé dans la ligne | C — peint sur l'axe du titre |
| 4 | Point de mention de la barre des serveurs d'une autre teinte et d'un autre contour que la pastille chiffrée | couleurs différentes | C — `danger_fill()` partout |
| 5 | Anneaux de parole des participants vocaux trop grands, qui se touchent d'une ligne à l'autre | avatar pas réduit de l'anneau | C — `voice_avatar_in`, `speaking_ring` |

## Fil de messages et zone de saisie

| # | Défaut | Cause | État |
|---|--------|-------|------|
| 6 | Texte saisi et bouton emoji 3,5 px au-dessus de l'axe des icônes de la zone de saisie ; marges gauche/droite inégales | champ aligné en haut | C |
| 7 | Bouton emoji plus petit que ses voisins, survol sans fond | zone de clic à la taille de l'image | C — côté d'un bouton d'icône |
| 8 | Filet au-dessus de la zone de saisie qui s'arrête aux marges | trait par défaut du `Panel` | C |
| 9 | Messages système (arrivée, épinglage) hors de la colonne du texte | icône et phrase posées à la marge | C — icône dans la colonne des avatars |
| 10 | Pièces jointes en attente : état écrit sous « Réessayer », tuiles de hauteurs inégales | libellés non tronqués | C |
| 11 | Sélecteur d'emojis qui recouvre le haut de la zone de saisie dans une fenêtre basse | hauteur fixe | C (lot R) — la grille rend la hauteur qui manque au-dessus de la saisie |

## Panneaux latéraux et fenêtres volantes

| # | Défaut | Cause | État |
|---|--------|-------|------|
| 12 | Recherche : pagination coupée au bas du panneau, boutons et « Page 1 sur 3 » pas centrés | ligne de 18 pt | C |
| 13 | Boîte de réception : entrées espacées irrégulièrement, croix hors de l'axe du nom | hauteur de ligne estimée | C |
| 14 | Amis : lignes en retrait de 8 pt par rapport au titre et au champ de recherche | fond de survol compté dans la marge | C — le fond déborde, le contenu s'aligne |
| 15 | Groupe : en-tête du panneau 4 px plus bas que celui de la discussion, filet décalé, « Renommer » hors de la ligne du champ | marges propres au panneau | C — `Tokens::side_panel_margin` |
| 16 | Forum : pas de filet à la hauteur des autres en-têtes | titre et filtres dans le même bloc | C |
| 17 | Forum : emoji d'étiquette rendu en carré vide | police, pas alignement | C (lot R) — emoji de l'étiquette dessiné en image |

## Vocal, appels, conférence

| # | Défaut | Cause | État |
|---|--------|-------|------|
| 18 | Conférence : boutons de deux tailles côte à côte, « Terminer la conférence » coupé, auditeurs sur des lignes différentes | `small_button`, ligne sans retour | C |
| 19 | Appel : « Regarder » posé sur le nom de la tuile | bouton centré sans tenir compte du nom | C — le nom s'arrête au bouton, avec points de suspension |
| 20 | Appel entrant : avatars et noms pas sur le même axe | ligne de 18 pt | C |

## Fenêtres de dialogue

| # | Défaut | Cause | État |
|---|--------|-------|------|
| 21 | Libellés alignés en haut à côté de leur champ ou de leur liste (événement, sondage, fil, salon, serveur, invitations, modération, automod, bannissements, rôles d'un membre, accueil, parcourir) | lignes de 18 pt | C — `shell::page_style` dans chaque fenêtre |
| 22 | Listes déroulantes qui débordent de leur colonne quand le texte est long | largeur sans troncature | C — `.truncate()` |
| 23 | Commande d'application : liste de choix plus étroite que les champs voisins | largeur par défaut | C |
| 24 | Invitations : tableau plus étroit que la fenêtre | largeur au contenu | C (lot R) — colonnes étalées sur la largeur |
| 25 | Sélecteur de comptes : « Se déconnecter de ce compte » sur deux lignes, calé à gauche sous un bouton centré | texte plus large que le menu | C (lot R) — le menu s'élargit à son bouton le plus long |

## Paramètres

| # | Défaut | Cause | État |
|---|--------|-------|------|
| 26 | Commandes d'une même page qui ne finissent pas au même bord droit (interrupteurs, listes, curseurs, valeurs) | commande posée à la suite du libellé | C — `settings_row` cale à droite au pixel près |
| 27 | Voix : liste des périphériques qui déborde | texte long | C |
| 28 | Rôles : « Créer un rôle » loin du bord, en-tête MEMBRES pas au-dessus des nombres, colonne des boutons de largeur variable | pas de colonne réservée | C |
| 29 | Membres : barre d'outils sur deux lignes, dates et rôles à des abscisses différentes selon les boutons de la ligne | pas de colonne réservée | C |
| 30 | Membres : dans une fenêtre étroite, boutons, dates et puces de rôles écrits par-dessus l'avatar et le nom | pas de repli | C — barre sur deux lignes et actions dans un menu « … » quand la place manque |
| 30 bis | Conférence : en-tête coupé à gauche et trop haut quand la colonne centrale fait moins de 320 pt | pas de repli | C (lot R) — sous 320 pt, l'en-tête ne garde que l'icône et les boutons |
| 31 | Permissions d'un salon : carte de synchronisation plus étroite que la page, boutons à 9 px du bord, état neutre choisi sans fond | largeur au contenu, espace compté en trop, couleur de la page | C |
| 32 | Vue d'ensemble du serveur : icône seule sur sa ligne | disposition | C (lot R) — l'icône ouvre la ligne de ses boutons |

## Vérifications transversales

- Rayons : tuiles de pièces jointes passées au rayon et au fond des cartes du thème.
- Zones de clic : bouton emoji et « + » de catégorie portés au côté d'un bouton d'icône.
- Survol : aucun saut constaté sur `salon-survol` après correction.
- Jetons ajoutés à `theme::Tokens` (fonctions, pas de nouveau champ de thème) :
  `user_bar_row`, `speaking_ring`, `voice_avatar_in`, `side_panel_margin`.
- Scènes absentes de `preview`, non auditées : menu contextuel, info-bulle,
  panneau des messages épinglés, réponse en cours dans la zone de saisie.
