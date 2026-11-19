# Ce qui manque encore face au client de bureau de Discord

État au 15 novembre 2026, sur `main` à `5b98f72` (tous les lots du rattrapage
fusionnés, C10 compris). Ce document remplace, pour « ce qui reste », les
chapitres 2 de `docs/ecarts-fonctions.md` et 4 de `docs/ecarts-visuels.md`,
qui gardent l'inventaire complet et l'historique.

## Méthode et limites

- Chaque ligne « absent » ou « partiel » des deux audits du 14 novembre a été
  reprise et vérifiée **dans le code** : une fonction compte comme faite si son
  libellé (`src/i18n/`), l'élément d'interface qui la déclenche (`src/ui/`) et
  son effet (état local ou requête de `src/net/`) existent tous les trois.
- L'aspect a été jugé sur 378 captures de l'aperçu hors ligne (`preview`),
  dans cinq thèmes, à trois tailles et dans les deux langues
  (`docs/verification-finale.md`).
- **Rien n'a été essayé sur un vrai compte** : ni l'exécutable ni le réseau
  n'ont été lancés. « Fait » veut dire « branché », pas « vu marcher chez
  Discord ». La liste de ce qui reste à essayer en vrai est au chapitre 5.
- La référence Discord est la connaissance du client de bureau de 2025-2026 ;
  aucune capture du vrai client n'a servi de comparaison.

Importance, pour un utilisateur ordinaire : **A** se remarque tous les jours,
**B** chaque semaine, **C** occasionnel ou réservé aux administrateurs.

---

## 1. Fonctions qui manquent

### 1.1 Importance A

| # | Manque | Où en est le code | Coût |
|---|---|---|---|
| F1 — fait (lot FF) | **Les messages des personnes bloquées ou ignorées ne sont pas masqués** dans le fil (Discord les replie en « 1 message bloqué — Afficher ») | bloquer et ignorer existent (`ui/user_menu.rs`), les notifications en tiennent compte (`state/notify.rs`), mais `ui/chat.rs` et `ui/message.rs` ne regardent jamais la relation | S |
| F2 — fait (lot FF) | **Statut « inactif » automatique** après quelques minutes sans clavier ni souris | la présence envoyée porte toujours `afk: false`, `since: 0` (`net/games.rs`) ; aucune détection d'inactivité | S |
| F3 — fait (lot FF) | **Clic sur une mention de salon** (aller au salon) **et de rôle** | seule la mention d'une personne réagit (`ui/markdown.rs`) | S |
| F4 — fait (lot FF) | **Aperçu de la mise en forme pendant la frappe** : gras, italique, code colorés, mentions en pastilles | seuls les emojis sont rendus en image dans la saisie (`ui/composer_emoji.rs`) | L |
| F5 — fait (lot FF) | **Fréquence des emojis jamais écrite** : la rangée « fréquents » ne suit pas l'usage fait dans Ferrocord | lue depuis le compte (`ui/emoji_picker.rs`), jamais mise à jour (`state/frecency.rs`) | S |
| F6 — fait (lot FF) | **Texte collé trop long converti en fichier** `message.txt` | seule l'erreur « message trop long » existe (`ui/composer.rs`) | S |
| F7 — fait (lot FF) | **Menu contextuel de la zone de saisie et d'un lien** (couper, copier, coller, copier le lien) | image : fait ; lien et saisie : rien | S |
| F8 — fait (lot FF) | **Salons à limite d'âge : écran d'avertissement à l'entrée ; floutage local des médias sensibles** | l'option du salon et les réglages du compte existent ; ni écran ni flou | M |
| F9 — fait (lot FF) | **Messages `@silent`** : envoi, petit indicateur, respect dans les notifications | le drapeau n'est ni posé ni lu | S |

### 1.2 Importance B

| # | Manque | Où en est le code | Coût |
|---|---|---|---|
| F10 — fait (lot FF) | Commandes intégrées de la saisie : `/shrug`, `/tableflip`, `/me`, `/spoiler`, `/nick`, `/tts` | aucune ; seules les commandes d'applications existent | S |
| F11 — fait (lots FF et FF2) | Notifications : un son par événement (appel, arrivée et départ en vocal, sourdine), notification d'un direct qui commence, d'une demande d'ami, d'une réaction ; rappels d'événements | deux sons seulement (`desk/system.rs`) ; une seule sorte d'alerte (`state/notify.rs`) | M |
| F12 — fait (lot FF2) | Fenêtre de notifications du serveur : mentions surlignées, événements muets, notifications mobiles | fenêtre et exceptions par salon faites (`ui/notif_settings.rs`) ; ces trois interrupteurs manquent | S |
| F13 — fait (lots FF et FF2) | Fils : liste des membres du fil, verrouiller, créer un fil privé ou sans message d'origine | panneau « Fils », notification par fil, menu de ligne faits (`ui/threads.rs`) | M |
| F14 — fait (lot FF2) | Fenêtre « Inviter des amis » avec la liste d'amis et un bouton par ami | lien, réglages, copie faits ; contournement par « Inviter sur le serveur » du menu d'une personne | S |
| F15 — fait (lot FF2) | En tête de la liste des salons : lignes Parcourir, Membres, Boosts ; barre de progression des boosts ; menu du serveur : créer un salon, une catégorie, un événement, modifier son profil de serveur | « Événements » seul ; le reste passe par le clic droit ou les paramètres | S |
| F16 — fait sauf la recherche dans tous les MP, sans source publique (lot FF2) | Recherche : recherches récentes, recherche dans tous les MP, onglets (messages, médias, liens, fichiers) | recherche par serveur ou par MP, filtres et tri faits | M |
| F17 — Lottie fait (0.1.10) ; suggestions faites (0.1.11, `ui/sticker_hints.rs`, autocollants du serveur seulement, réglage dans Accessibilité ; **non essayé sur un vrai compte**) | Autocollants Lottie (rendu), suggestions d'autocollants pendant la frappe | PNG, APNG, GIF et Lottie (`images/lottie`, rendu comparé à l'œil sur quatre autocollants officiels ; **non essayé dans l'application sur un vrai compte**) | L |
| F18 | Animation des super réactions ; onglet « Sons » avec sa grille dans le sélecteur | envoi fait ; « Sons » n'est qu'un bouton vers la fenêtre du tableau | M |
| F19 | Parures animées : effets de profil, plaques animées, polices et effets des noms | les champs sont lus (`font_id`, `effect_id`), pas rendus ; emoji de statut animé fait | L |
| F20 — fait (lot FF2) | Groupes : changer l'icône, lien d'invitation, transférer la propriété | icône affichée seulement | S |
| F21 — fait (membre : lot FF2 ; fichier sur un salon ou un MP de la liste, Maj pour l'envoi direct : 0.1.7) | Glisser un fichier sur un salon de la liste (Maj : envoi direct), glisser un membre sur un salon vocal | dépôt sur la fenêtre seulement (`ui/upload.rs`) | S |
| F22 — micro et casque globaux faits (0.1.7, réglage « Micro et casque depuis toute application ») ; les autres actions et les trois raccourcis : non faits | Raccourcis : globaux réglables (hors appui-pour-parler), Ctrl+Alt+A, Ctrl+Maj+Alt+V, Ctrl+Maj+I | les autres raccourcis de Discord sont faits (`state/keys.rs`) | S |
| F23 — changement de source sans arrêter fait (0.1.7) ; décodage VP9 et AV1 fait (0.1.9, réglage « Recevoir la vidéo en VP9 et AV1 », coupé par défaut ; **non essayé sur un vrai appel**, y compris sous DAVE) ; le reste : non fait | Partage d'écran : changer de fenêtre sans arrêter ; atténuation des autres applications ; priorité de parole ; arrière-plan vidéo ; décodage VP9 et AV1 | H.264 et VP8 à l'envoi ; VP9 et AV1 reçus par les décodeurs de Windows (`video/codec/av1.rs`, `net/voice/video/{vp9,av1}.rs`) | L à XL |
| F24 — fait (carte et rôles : lot FF2 ; début de MP : 0.1.7, serveurs en commun affichés quand le profil est déjà connu) | Début de MP complet (serveurs en commun, ajouter en ami, bloquer) ; champ « Envoyer un message » réel sur la carte de profil ; « + » pour ajouter un rôle sur la carte | début réduit (avatar, nom, phrase) ; champ factice qui ouvre le MP ; rôles à cocher dans le menu | S |
| F25 — fait (lot FF) | Forums : consignes de publication affichées, disposition et tri par défaut réglables | consignes modifiables mais pas montrées (`ui/forum.rs`) | S |
| F26 — fait (lot FF2) | Emojis verrouillés (sans Nitro) montrés grisés avec l'explication | ils sont retirés de la liste (`state/emoji/usable.rs`) | S |
| F27 — fait (lots FF et FF2) | Petits manques : « Mentionner » dans le menu du message, « effacer aujourd'hui » du statut, volume d'une personne depuis son menu, domaines de confiance modifiables dans les paramètres, lecture automatique des messages `/tts` | — | S chacun |

### 1.3 Importance C

| # | Manque | Coût |
|---|---|---|
| F28 | Paramètres du serveur : vérification des membres (règles à accepter), page Accès, guide du serveur, tag du serveur, permissions des commandes modifiables, « voir le serveur en tant que rôle », filtres de la vue Membres | L |
| F29 | Lanceur d'applications de la zone de saisie ; « rejoindre la partie », « Écouter avec » | M |
| F30 — débit de la synthèse vocale fait (lot FF2) ; notes de version après une mise à jour faites (0.1.7) | Notes de version dans l'application ; renvois Centre familial et Clips ; débit de la synthèse vocale ; diagnostic et journal vocal | S |
| F31 | Découverte de serveurs intégrée (aujourd'hui : navigateur) ; statut « traîne » d'un salon vocal ; signalement d'une personne ou d'un serveur (renvoi) | M |
| F32 | Correcteur orthographique ; barre de mise en forme sur la sélection | XL, M |
| F33 | Boutique et inventaire en affichage, cartes de cadeau reçues (`discord.gift`) : à trancher avec l'utilisateur | M |
| F34 | Linux, macOS (barre de titre intégrée : Windows seulement) | XL |

### 1.4 Écarté par règle (inchangé)

Achats (Nitro, boosts, boutique, cadeaux à offrir, abonnements de serveur) ;
tout ce qui passe par un captcha (connexion par mot de passe, création de
compte, changement de mot de passe ou d'e-mail, ajout d'une application à un
serveur) ; quêtes ; activités intégrées ; lecteurs web des liens (YouTube,
Twitch, Spotify) ; overlay en jeu ; formulaire de signalement (renvoyé au
navigateur).

---

## 2. Aspect : ce qui diffère encore

Les vingt écarts les plus visibles de l'audit du 14 novembre sont traités, à
quatre restes près (lignes V1 à V4). Le reste de cette liste vient des
captures de la vérification.

| # | Écart | Importance | Où |
|---|---|---|---|
| V1 — menu « + » fait (lot FF) | **Zone de saisie** : Discord montre, à droite, GIF, autocollants, emoji ; et le « + » ouvre un menu (envoyer un fichier, créer un fil, créer un sondage, applications). Ferrocord garde les boutons sondage et micro dans la barre, n'affiche le bouton des autocollants que si le serveur en a, et son « + » ouvre directement le sélecteur de fichiers | A | `ui/composer.rs`, `ui/upload.rs`, `ui/polls.rs` |
| V2 | **Fenêtres de dialogue de deux gabarits** : le gabarit de Discord (titre, croix, pied sombre, boutons à droite) existe (`widgets::modal`), mais la fenêtre d'une application (boutons à gauche), « Créer un serveur » (ni croix ni pied), le partage d'écran et plusieurs fenêtres d'administration gardent l'ancien ; les boutons secondaires des pieds n'ont pas tous le même style | A | `ui/components.rs`, `ui/guild_create.rs`, `ui/share_picker.rs`, `ui/moderation.rs` |
| V3 | **Deux tailles de boutons** : des boutons de 24 de haut (texte de 16 à l'étroit) voisinent avec des contrôles de 40 dans les paramètres, la recherche, les envois, l'éditeur de thème. Discord n'a que 32 et 38 à 40 | A | petit bouton commun (`ui/util.rs`, `ui/components.rs`) |
| V4 | **Carte de profil trop longue** : Discord ne montre que l'identité, l'activité, une bio tronquée, les rôles et le champ de message ; la nôtre empile activité détaillée, écoute, à propos, dates, rôles et sort de la fenêtre à 720 de haut ; pas de boutons (ami, « ⋯ ») sur la bannière ni de bulle de statut | B | `ui/profile.rs` |
| V5 | **Thème clair moins fini que les sombres** : étiquettes des tuiles d'appel et des vidéos, bouton principal désactivé, statut choisi de la carte de compte, croix du profil complet (défauts 3 à 5 et 22 de `docs/verification-finale.md`) | A pour qui l'utilise | `ui/video/tiles.rs`, `ui/player.rs`, `ui/widgets.rs`, `ui/quick_profile.rs` |
| V6 | **Fenêtre étroite** : sous 800 points de large, l'en-tête, la page Amis, l'appel et la recherche se chevauchent ou se coupent (défauts 6 à 10) ; Discord replie ses icônes et garde le titre lisible jusqu'à 940 × 500, sa taille minimale | B | `ui/chat_head.rs`, `ui/friends.rs`, `ui/layout.rs` |
| V7 | Contrôles d'egui encore reconnaissables : listes déroulantes à triangle plein (Discord : chevron fin), cases à cocher et boutons radio petits et sans carte, curseur à poignée arrondie, barres de défilement flottantes | B | `theme/mod.rs` (`apply`), `ui/widgets.rs` |
| V8 | Barre « modifications non enregistrées » : dans la page chez nous, flottante en bas chez Discord (et elle tremble si l'on veut sortir) | B | `ui/settings/shell.rs`, `ui/channel_settings/mod.rs` |
| V9 — fait (lot FF2) | Bloc de code sans bouton « copier » au survol ; heure d'un message groupé non montrée au survol dans la marge | B | `ui/markdown.rs`, `ui/message.rs` |
| V10 | Paramètres du serveur > Membres : trois boutons rouges par ligne (exclure, expulser, bannir) ; Discord range ces actions dans un menu « ⋮ » et montre un tableau | C | `ui/guild_settings/members.rs` |
| V11 | Thèmes personnels (dégradé, image de fond) : panneau utilisateur, pastilles de date, code et champ de recherche restent en aplats opaques (défauts 2 et 17) ; les thèmes dégradés de Discord teintent toutes les surfaces | B pour qui s'en sert | `ui/look.rs`, `ui/channels.rs`, `ui/markdown.rs` |
| V12 | Préréglages d'avant la refonte (Minuit, Rouille, Compact, Aéré, Contraste élevé) : géométrie d'origine, plus serrée, donc des boutons et des croix plus petits qu'ailleurs (défaut 23) | C | `theme/presets.rs` |
| V13 | Illustrations : états vides (aucun ami, aucun résultat) réduits à une icône et une phrase ; pas d'écran de chargement à astuces (voulu) | C | — |
| V14 | Police : Figtree au lieu de gg sans (propriétaire, non copiable) ; icônes redessinées, pas celles de Discord. Écart assumé et permanent | — | — |

Non vérifiable sans le vrai client : les teintes exactes de « Sombre » et
« Onyx » et les rayons d'après refonte, marqués « ≈ » dans `docs/design.md`,
n'ont jamais été relevés à la pipette sur une capture de Discord.

---

## 3. Les dix manques principaux

1. Messages des personnes bloquées ou ignorées toujours affichés (F1).
2. Pas de statut « inactif » automatique (F2).
3. Mentions de salon et de rôle non cliquables (F3).
4. Zone de saisie : boutons et menu « + » différents de Discord (V1) ; pas
   d'aperçu de la mise en forme pendant la frappe (F4).
5. Menus contextuels de la saisie et des liens absents (F7) ; texte trop long
   non converti en fichier (F6).
6. Emojis « fréquents » figés (F5) ; commandes intégrées `/shrug`, `/me`…
   absentes (F10).
7. Sons et notifications par événement : appel, arrivée en vocal, direct,
   demande d'ami (F11).
8. Salons à limite d'âge sans écran d'entrée, médias sensibles non floutés
   (F8).
9. Fenêtres de dialogue et boutons de deux gabarits, carte de profil trop
   longue (V2, V3, V4).
10. Thème clair et fenêtre étroite moins finis que le thème sombre à taille
    normale (V5, V6).

---

## 4. Ce qui a été rattrapé depuis le 14 novembre (rappel)

Sélecteur de GIF et favoris, lecteur vidéo et audio dans le fil, marquer comme
non lu, barre de survol à réactions rapides et menu « ⋯ », menu du message
complet, en-tête à 48 avec sujet, Fils, cloche et recherche à loupe, catégories
repliables, salons muets masqués, menu de l'utilisateur complet, demandes de
message et spam, « Actifs en ce moment », pseudos d'amis, « + » des MP, début
de salon, bandeau des nouveaux messages, barre de titre intégrée, menu du
panneau utilisateur, panneau vocal à boutons, réglages d'un salon vocal,
fenêtre de notifications du serveur, réglage par fil, boîte « Pour vous »,
super réactions (envoi), confirmation des liens externes, la plupart des
raccourcis, profil par serveur complet avec aperçu, en-tête de MP (appel
vidéo, ajouter, profil), galerie des forums et salons médias, bannière du
serveur, modèle, widget et élagage du serveur, actions des alertes AutoMod,
son d'un direct hors appel ; préréglages Sombre, Cendre, Onyx, police Figtree,
73 icônes pleines, thèmes à dégradé et à image de fond, éditeur de thème.

---

## 5. À essayer sur le vrai compte (jamais fait)

Aucune des requêtes non documentées ajoutées pendant le rattrapage n'a été
envoyée à Discord : GIF (`/gifs/*`), favoris et fréquence (`settings-proto/2`),
accusé de lecture manuel (« marquer comme non lu »), demandes de message
(acceptation, refus), repli des catégories et salons muets masqués
(synchronisation), super réaction, pseudo d'ami, suppression des intégrations,
réglages d'un salon vocal, notifications par fil, widget, modèle, élagage.
Restent aussi à voir en vrai : la lecture d'une vraie vidéo MP4 ou WebM du
CDN, les vrais avatars et emojis (l'aperçu n'a que des images factices), la
barre de titre intégrée sur plusieurs écrans et à d'autres échelles que
125 %, et la tenue de l'interface avec un compte chargé (centaines de
serveurs, noms longs, écritures non latines).
