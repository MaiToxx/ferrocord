# Écarts de fonctions avec le client Discord de bureau

Audit du 14 novembre 2026, sur `main` à `e7aab17`. Aucun code n'a été écrit ni
lancé pour cet audit.

> **Mise à jour du 15 novembre 2026 (vérification finale, `main` à `5b98f72`).**
> Les états en gras (**fait**) et les détails réécrits ont été revérifiés dans
> le code après la fusion des lots G, M, U, S, N, V, R, VID, PERSO, V1 à V3 et
> C10 ; « fait » veut toujours dire « branché dans le code », pas « essayé sur
> le vrai compte ». La liste à jour de ce qui manque est dans
> `docs/ecarts-restants.md`.

## Méthode et limites

- **Référence Discord** : ce que fait le client de bureau pour Windows, d'après
  la connaissance de son interface, la documentation des développeurs et les
  comptes rendus publics de la refonte de mars 2025. Le centre d'aide
  `support.discord.com` refuse les lectures automatiques (HTTP 403) : aucun de
  ses articles n'a pu être relu pendant l'audit.
- **État de Ferrocord** : établi à partir de `PLAN.md`, de `REPRISE.md`, des
  textes de l'interface (`src/i18n/*.rs`, qui listent chaque libellé affiché) et
  de recherches ciblées dans `src/`. L'exécutable n'a pas été lancé et rien n'a
  été vérifié sur un vrai compte.
- Un état **fait** veut dire « le code et ses libellés existent », pas « vérifié
  sur le vrai compte ». Les phases 4 à 9 n'ont jamais été essayées en vrai.
- Les états marqués *(à confirmer)* reposent sur une absence de trace dans le
  code, sans lecture complète du module.

États : **fait**, **partiel** (ce qui manque est précisé), **absent**,
**écarté** (interdit par nos règles ou par le chapitre 5 du plan).

---

## 1. Inventaire par zone

### 1.1 Messages

| Fonction de Discord | État | Détail, fichier |
|---|---|---|
| Envoi, modification, suppression, historique, non envoyés | fait | `ui/chat.rs`, `ui/composer.rs` |
| Mise en forme : gras, italique, souligné, barré, titres, listes, citations, spoilers, code coloré, sous-texte `-#`, liens masqués, dates `<t:…>` | fait | `state/markdown/`, `ui/markdown.rs` |
| Aperçu de la mise en forme pendant la frappe (la saisie colore `**gras**`, les mentions deviennent des pastilles) | partiel | emojis rendus en image dans la saisie (`ui/composer_emoji.rs`) ; pas de coloration du gras ni de pastilles de mention |
| Barre de mise en forme qui apparaît sur une sélection (gras, italique, barré, citation, code, spoiler) | absent | Ctrl+B, Ctrl+I, Ctrl+U existent sur la sélection (`ui/composer.rs`) ; pas de barre flottante |
| Correcteur orthographique | absent | pas d'équivalent dans egui ; coût élevé |
| Mentions @membre, @rôle, #salon, @everyone, @here, avec autocomplétion | fait | `ui/complete.rs` |
| Clic sur une mention : profil, salon, rôle | partiel | mention d'utilisateur : clic gauche carte, clic droit menu ; mentions de salon et de rôle sans action (`ui/markdown.rs`) |
| Réponses, avec choix de mentionner l'auteur | fait | `MENTION_AUTHOR`, `ui/msg_actions.rs` |
| Flèche haut : modifier son dernier message | fait | `ui/msg_actions.rs:633` |
| Transfert de message | fait | `Action::Forward` |
| Épingles (épingler, panneau, saut) | fait | `ui/pins.rs` |
| Copier le texte, le lien, l'identifiant | fait | menu du message |
| **Marquer comme non lu** | **fait** | entrée « Marquer comme non lu » du menu du message (`Action::MarkUnread`, `RelationsCmd::Unack`, lot M) |
| Menu « ⋯ » de la barre de survol, réactions rapides (3 emojis récents) dans la barre et en tête du menu | **fait** | réactions rapides et bouton « ⋯ » (`ui/msg_actions.rs`, lot M) |
| Supprimer les intégrations d'un message (`SUPPRESS_EMBEDS`) | **fait** | `SocialCmd::SuppressEmbeds` (lot M) |
| Lire le message à voix haute (synthèse vocale), commande `/tts` | partiel | « Lire le message à voix haute » du menu du message : synthèse de Windows, locale (`platform/speech.rs`, lot R) ; pas de lecture automatique des messages `/tts` |
| Signaler un message | renvoyé | « Signaler » ouvre le message dans le navigateur (`ui/msg_actions.rs`) |
| Types de messages système (arrivée, boost, épinglage, fil créé, appel, AutoMod, rappel d'invitation, découverte…) | fait | `ui/system_msgs.rs`, `state/system_msgs.rs` |
| Messages `@silent` (drapeau `SUPPRESS_NOTIFICATIONS`) | absent | le drapeau n'est ni lu, ni posé à l'envoi (`@silent`), ni signalé |
| Texte collé trop long converti en fichier `message.txt` | absent | seule l'erreur `MESSAGE_TOO_LONG` existe |
| Pièces jointes : glisser-déposer, coller une image, progression, spoiler, texte alternatif, renommer | fait | `ui/upload.rs`, `src/attach.rs` |
| Images, GIF, WebP, APNG animés, visionneuse | fait | `ui/attachments.rs`, `ui/viewer.rs` |
| Visionneuse : zoom, copier l'image, enregistrer l'image, ouvrir dans le navigateur | fait | `ui/viewer.rs`, `ui/image_ops.rs` (lot VID) |
| Menu contextuel d'une image (copier l'image, enregistrer, copier le lien, ouvrir le lien) | fait | clic droit sur une image du fil, et menu du message (`ui/image_ops.rs`, lot VID) |
| **Lecture des vidéos et des sons dans le fil** (lecteur intégré, volume, plein écran) | **fait** | lecteur intégré `ui/player.rs` (MP4, MOV, WebM, audio ; volume, position, plein écran ; lot VID) ; plafond de taille ; non essayé sur un vrai fichier du compte |
| Lecteurs intégrés des liens (YouTube, Twitch, Spotify…) | écarté | pages web : il faudrait un navigateur embarqué (plan, chapitre 5) ; vignette et lien suffisent |
| Embeds complets, aperçus de liens | fait | `ui/embeds.rs` |
| Composants des bots, y compris la génération 2 (conteneur, section, galerie) | fait *(à confirmer pour la génération 2)* | `net/interactions.rs`, `ui/components.rs` |
| **Sélecteur de GIF** (recherche, tendances, catégories, favoris) | **fait** | `ui/gif_picker.rs`, `net/gifs.rs` (lot G) |
| GIF favoris (étoile sur un GIF, onglet Favoris) | **fait** | étoile et onglet Favoris (`state/gifs.rs`) ; un favori vidéo n'a pas d'aperçu |
| Autocollants : sélecteur, envoi, affichage | partiel | PNG, APNG, GIF faits ; Lottie non rendu (image fixe ou libellé) |
| Suggestions d'autocollants pendant la frappe | absent | — |
| Emojis Unicode (Twemoji), personnalisés, animés | fait | `state/emoji/`, `ui/emoji.rs` |
| Réactions : voir, ajouter, retirer, liste de qui a réagi | fait | `ui/reactions.rs` |
| **Super réactions** | partiel | envoi fait (interrupteur « Super réaction » du sélecteur, lot G) ; pas d'animation |
| Sondages : créer, voter, résultats, terminer | fait | `ui/polls.rs` |
| Messages vocaux : écouter, enregistrer | fait | `ui/voice_note.rs` (sur le bureau, Discord ne permet que l'écoute) |
| Fils : créer, lister, ouvrir, rejoindre, quitter, archiver | fait | `ui/threads.rs` |
| Fils : panneau « Fils » de l'en-tête (actifs, archivés), liste des membres du fil, réglages de notification du fil, verrouiller, fil privé à la création | partiel | panneau « Fils » de l'en-tête (actifs, archivés), notification par fil et menu de la ligne faits (lot S) ; manquent les membres du fil, le verrou, le fil privé à la création |
| Forums : liste, tri, étiquettes, création de publication | fait | `ui/forum.rs` |
| Forums : vue galerie, extrait et vignette des publications, consignes de publication affichées | partiel | galerie de cartes (vignette, extrait), par défaut dans un salon média, bouton Liste / Galerie (`ui/forum.rs`, lot R) ; consignes de publication non affichées |
| Salons médias (type 16) | **fait** | galerie de vignettes par défaut dans un salon média (`ui/forum.rs`, lot R) |
| Annonces : suivre, publier | fait | `ui/polls.rs` (`follow_button`) |
| Cartes d'invitation | fait | `ui/invites.rs` |
| Confirmation avant d'ouvrir un lien externe, liste de domaines de confiance | fait | fenêtre « Vous quittez Discord » et case de confiance (`ui/links.rs`, lot VID) ; la liste se modifie dans `domaines-de-confiance.txt`, pas encore dans les paramètres |
| Écran d'avertissement des salons soumis à une limite d'âge | absent | l'option existe dans les réglages du salon, pas l'écran à l'entrée |
| Floutage des médias sensibles (réglage « contenu explicite ») | partiel | réglages du compte affichés et modifiables ; aucun floutage local |
| Bandeau « N nouveaux messages depuis … — Marquer comme lu » en haut du salon | **fait** | bandeau en haut du salon (`ui/chat.rs`, `new_banner`, lot M) |
| En-tête de début de salon (« Bienvenue dans #salon ») et de début de MP (avatar, serveurs en commun, ajouter en ami, bloquer) | partiel | début de salon fait (disque, titre, phrase) ; début de MP : avatar, nom et phrase, sans serveurs en commun ni boutons (le panneau de profil du MP compense) |
| Résumés de conversation, Clips | partiel | résumés lus au clic (lot ND, `ui/summaries.rs`, menu du salon) ; Clips absents |

### 1.2 Recherche

| Fonction | État | Détail |
|---|---|---|
| Recherche dans un serveur ou un MP, résultats, pagination, saut vers le message | fait | `ui/search.rs`, `state/search.rs` |
| Filtres `de:`, `mentionne:`, `contient:`, `dans:`, `avant:`, `apres:`, `pendant:`, `epingle:`, `typeAuteur:` | fait | `ui/search.rs:645` |
| Tri récents, anciens, pertinence | fait | — |
| Historique des recherches récentes | absent *(à confirmer)* | — |
| Recherche dans tous les MP à la fois, onglets de résultats (messages, médias, liens, fichiers) de la recherche récente de Discord | absent | — |
| Sélecteur rapide Ctrl+K | fait | `ui/quick_switcher.rs` |
| Recherche dans les paramètres | fait | champ « Rechercher un réglage » |

### 1.3 Notifications

| Fonction | État | Détail |
|---|---|---|
| Notifications Windows, sons, pastille de la barre des tâches, zone de notification, fermer vers la zone | fait | `src/desk/`, `ui/desk.rs` |
| Muet avec durée, niveau (tout, mentions, rien), par serveur et par salon, @everyone et rôles | fait | `ui/settings/menu.rs` (clic droit) |
| Muet et niveau **par catégorie** | **fait** | menu de la ligne de catégorie et fenêtre des notifications du serveur (`ui/notif_settings.rs`, lot N) |
| Fenêtre « Paramètres de notification » du serveur (tableau des exceptions par salon, mentions surlignées, événements muets, notifications mobiles) | partiel | fenêtre et exceptions par salon faites (lot N) ; mentions surlignées, événements muets et notifications mobiles absents |
| Réglage par fil (tous les messages, mentions, rien) | **fait** | `ThreadCmd::Notify` (`ui/threads.rs`, lot N) |
| Page Notifications des paramètres : un son par type d'événement (message, sourdine, appel, arrivée en vocal…) | partiel | un volume et un son d'essai ; pas de liste de sons par événement |
| Notification d'un direct qui commence, des réactions, des demandes d'ami | partiel | interrupteurs du compte affichés ; effet local non vérifié |
| Ne pas déranger : aucune notification | **fait** | `state/notify.rs`, avec test |
| Rappels d'événements | absent | — |

### 1.4 Boîte de réception, mentions, non-lus

| Fonction | État | Détail |
|---|---|---|
| Boîte : onglet Mentions (filtres @everyone, rôles, ce serveur), retirer une mention, saut | fait | `ui/inbox.rs` |
| Boîte : onglet Non lus, marquer comme lu, ouvrir | fait | `unreads_tab` |
| Boîte : onglet « Pour vous » (demandes d'ami, événements, activité) | **fait** | demandes d'ami et événements proches (`ui/inbox.rs`, lot N) ; pas d'« activité » |
| Accusés de lecture synchronisés, marquer un salon, un serveur, un dossier comme lu | fait | `ui/reads.rs`, `state/reads.rs` |
| Marquer comme non lu | **fait** | voir 1.1 |
| Saut au premier non lu, navigation Alt+Maj+flèches entre salons non lus | fait | `Action::PrevUnread` / `NextUnread` |

### 1.5 Amis, MP, groupes

| Fonction | État | Détail |
|---|---|---|
| Liste d'amis (en ligne, tous, en attente, bloqués), recherche, ajout par nom, accepter, refuser, retirer, bloquer, ignorer | fait | `ui/friends.rs` |
| Colonne « Actifs en ce moment » | **fait** | colonne de droite de la page Amis, fenêtre large (`ui/friends.rs`, lot U) |
| Pseudo d'ami | **fait** | `RelationsCmd::Nickname` (`ui/user_menu.rs`, lot U) |
| Suggestions d'amis, synchronisation des contacts | partiel | suggestions dans l'onglet « En attente » (lot ND, `ui/suggestions.rs`) ; pas de synchronisation des contacts |
| MP : ouvrir, fermer, lire, écrire | fait | — |
| Bouton « Rechercher ou lancer une conversation », en-tête « Messages privés » avec « + » | **fait** | bouton de recherche et « + » de la colonne des MP (`ui/groups.rs`, lot U) |
| Groupes : créer, ajouter, retirer, renommer, quitter | fait | `ui/groups.rs` |
| Groupes : icône, lien d'invitation, transfert de propriété | partiel | icône affichée, non modifiable ; ni lien d'invitation ni transfert de propriété |
| **Demandes de message** (dossier séparé, accepter ou ignorer) et dossier de spam | **fait** | dossier des demandes, accepter, ignorer (`ui/requests.rs`, lot U) ; forme réelle à vérifier sur le compte |
| Notes sur un utilisateur | fait | `ui/profile.rs` |
| Appel vocal depuis un MP | fait | `ui/call.rs` |
| Bouton d'appel vidéo, « Ajouter des amis au MP », panneau de profil à droite du MP | **fait** | appel vidéo, ajouter au MP, panneau de profil (`ui/groups.rs`, `ui/dm_profile.rs`, lots S et U) |

### 1.6 Profils

| Fonction | État | Détail |
|---|---|---|
| Carte de profil, profil complet (à propos, activité, serveurs et amis en commun), badges, comptes liés, note | fait | `ui/profile.rs`, `ui/profile_full.rs` |
| Bannières et avatars animés, décorations, plaques, tag de serveur, style de nom (couleurs), thème du profil | fait | lot P |
| Effets de profil, plaques animées, polices et effets des noms, emoji animé du statut | partiel | emoji animé du statut fait (`ui/status_emoji.rs`) ; plaques animées sous le pointeur faites (lot plaques, `images/webm.rs`) ; effets de profil, polices et effets des noms : voir les lots ND, profil et polices |
| Champ « Envoyer un message » en bas de la carte | partiel | bouton « Envoyer un message » qui ouvre le MP, pas de champ |
| Modifier son profil : nom, pronoms, bio, couleur, avatar, bannière | fait | `ui/settings/account.rs`, `ui/settings/avatar.rs` |
| Profil **par serveur** : pseudo, avatar, bannière, bio, pronoms | **fait** | bio et pronoms par serveur (`ui/profile_edit.rs`, lot U) |
| Aperçu du profil pendant la modification | **fait** | la carte du profil, modifiable sur place, au centre de la fenêtre (`ui/profile_edit_card.rs`) |
| Rôles sur la carte, avec ajout et retrait d'un rôle depuis la carte | **fait** | rôles à cocher dans le menu de la personne (`ui/user_menu.rs`) ; pas de « + » sur la carte elle-même |

### 1.7 Serveurs

| Fonction | État | Détail |
|---|---|---|
| Rail, dossiers (nom, couleur), glisser-déposer, rejoindre, quitter, créer, modèles | fait | `ui/rail.rs`, `ui/rail_dnd.rs`, `ui/guild_create.rs` |
| Découverte de serveurs | fait (consultation) | page « Découvrir des serveurs » (lot ND, `ui/discovery.rs`) : recherche, catégories ; rejoindre par la fenêtre d'invitation ou le navigateur |
| Menu du serveur (en-tête et clic droit) : inviter, paramètres, marquer comme lu, quitter, notifications, événements, parcourir | fait | — |
| Menu du serveur : créer un salon, une catégorie, un événement ; modifier son profil de serveur ; masquer les salons muets ; afficher tous les salons ; confidentialité du serveur ; signaler | partiel | création et profil existent ailleurs (clic droit dans la liste, paramètres) ; confidentialité par serveur et signalement absents (masquer les salons muets : **fait**) |
| **Replier une catégorie**, état gardé et synchronisé | **fait** | `state/chanlist.rs`, synchronisé par les réglages du compte (lot S) ; synchronisation à vérifier sur le compte |
| Masquer les salons muets | **fait** | entrée du menu du serveur (`ui/rail.rs`, `state/chanlist.rs`, lot S) |
| Fenêtre « Inviter des amis » (liste d'amis avec bouton Inviter, lien, réglages du lien) | partiel | création et copie du lien, réglages ; pas de liste d'amis dans la fenêtre ; « Inviter sur le serveur » du menu d'une personne envoie le lien en MP (lot U) |
| Intégration des nouveaux membres (questions), écran de bienvenue | fait | `ui/welcome.rs`, `ui/guild_settings/onboarding.rs` |
| Guide du serveur (page d'accueil, tâches, ressources) | absent | — |
| Parcourir les salons, « Salons et rôles » | fait | `ui/browse.rs` |
| Lignes fixes en tête de la liste : Événements, Parcourir, Membres, Boosts | partiel | « Événements » seulement ; le reste est dans le menu |
| Vérification des membres (règles à accepter avant de parler) | fait | fenêtre « Règles et vérification » du menu du serveur, demandes d'adhésion pour la modération (lot ND, `ui/verification.rs`) ; l'état « en attente » du compte n'est pas affiché à l'avance |
| Événements : liste, création, modification, intéressé, récurrence, couverture, lien | fait | `ui/events.rs` |
| Tableau de sons : écouter, jouer, gérer | fait | `ui/soundboard.rs` |
| Salons de conférence : orateurs, lever la main, sujet, modération | fait | `ui/stage.rs` |
| Activités intégrées | écarté | plan, chapitre 5 |
| Bannière du serveur dans l'en-tête de la colonne, barre de progression des boosts | partiel | bannière derrière le nom faite (`ui/channels.rs`) ; barre de progression des boosts absente |
| Icônes de rôle, couronne du propriétaire dans la liste des membres | fait *(à confirmer sur un vrai serveur)* | couronne et icône du plus haut rôle après le nom (`ui/members.rs`, lot R) |

### 1.8 Vocal, vidéo, partage d'écran

| Fonction | État | Détail |
|---|---|---|
| Rejoindre, quitter, muet, sourdine, volume par personne, muet local, modération vocale, déplacer, déconnecter | fait | `ui/voice/` |
| Détection de voix, appui-pour-parler (global), suppression de bruit, écho, gain, choix et test des périphériques | fait | `src/audio/dsp/`, `ui/settings/voice.rs` |
| Atténuation des autres applications, priorité de parole | absent | — |
| Chiffrement DAVE, codes de vérification | fait | — |
| Statut du salon vocal | fait | `calls::SET_STATUS` |
| Statut « traîne » (hang status) | absent | — |
| Caméra, partage d'écran et de fenêtre avec le son, qualité, regarder un direct, plein écran, fenêtre détachée, toujours au-dessus | fait | `ui/video/`, `ui/share_picker.rs` |
| Arrière-plan vidéo flou ou image | absent | — |
| Aperçu de son direct, pointeur, changer de fenêtre sans arrêter | partiel *(à confirmer)* | aperçu avant de partager ; changement à chaud non repéré |
| Panneau vocal : boutons caméra, partage, tableau de sons, suppression de bruit | **fait** | bruit, caméra, partage, tableau de sons dans le panneau (`ui/voice/panel.rs`, lot V) |
| Appels privés et de groupe, sonnerie | fait | `ui/call.rs` |
| Réglages d'un **salon vocal** : débit, nombre maximal, région, qualité vidéo | **fait** | débit, limite, région, qualité vidéo (`net/admin/channels.rs`, `ui/channel_settings/overview.rs`, lot S) |
| Inviter au salon vocal, afficher le nombre de personnes et la limite | **fait** | « Inviter » du menu d'un salon vocal (`ui/channel_settings/list.rs`) |
| Codecs AV1, VP9 ; son d'un direct regardé hors appel | partiel | son d'un direct regardé hors appel fait (`voice/media.rs`) ; VP9 et AV1 décodés (0.1.9, réglage coupé par défaut, non essayé sur un vrai appel), jamais envoyés |
| Overlay en jeu | écarté | plan, chapitre 5 |

### 1.9 Sélecteur d'emojis, de GIF et d'autocollants

| Fonction | État | Détail |
|---|---|---|
| Onglets Emojis et Autocollants, recherche, récents, sections par serveur, catégories Unicode, teinte de peau | fait | `ui/emoji_picker.rs`, `ui/stickers.rs` |
| Onglet GIF | **fait** | lot G |
| Onglet Sons (en vocal) | partiel | bouton d'onglet « Sons » qui ouvre la fenêtre du tableau ; pas de grille dans le sélecteur |
| Emojis favoris, emojis les plus utilisés (fréquence), synchronisés avec le compte | partiel | favoris synchronisés (lot G) ; « fréquents » lus mais jamais écrits : l'usage fait dans Ferrocord ne les met pas à jour |
| Rail de catégories à gauche (serveurs, puis Unicode), bandeau du bas (emoji survolé, nom, serveur d'origine) | **fait** | rail et bandeau (`ui/emoji_picker.rs`, lot G) |
| Replier une section, Maj+clic pour en choisir plusieurs | **fait** | repli des sections (lot G) |
| Emojis verrouillés (sans Nitro) grisés avec explication | absent | les emojis non utilisables sont retirés de la liste (`state/emoji/usable.rs`), pas grisés |
| Envoyer un emoji en super réaction | **fait** | interrupteur du sélecteur (lot G) |
| Ajouter un emoji au serveur depuis un message (« cloner ») | écarté | absent de Discord aussi |

### 1.10 Raccourcis clavier

Faits et réglables (`state/keys.rs`, `keys.toml`) : Ctrl+K, Ctrl+I, Ctrl+/,
Ctrl+virgule, Alt+flèches (salons), Alt+Maj+flèches (non lus), Ctrl+Alt+flèches
(serveurs), Alt+gauche et droite (historique), Échap et Maj+Échap (marquer comme
lu), Ctrl+Maj+A, Page précédente et suivante, Ctrl+E, Ctrl+P, Ctrl+U,
Ctrl+Maj+M, Ctrl+Maj+D, appui-pour-parler, et les touches du message
sélectionné (R, E, +, P, Ctrl+C, Retour arrière).

| Raccourci de Discord | État |
|---|---|
| Ctrl+G (GIF), Ctrl+S (autocollants) | **fait** |
| Ctrl+Maj+U (joindre un fichier) | **fait** |
| Ctrl+F (recherche) | **fait** |
| Ctrl+Maj+N (créer ou rejoindre un serveur), Ctrl+Maj+T (créer un groupe) | **fait** |
| Ctrl+Maj+I ou Ctrl+T (fils), Ctrl+Maj+H (aide) | **fait** — Ctrl+T (fils) et Ctrl+Maj+H (aide) ; Ctrl+Maj+I absent |
| Ctrl+Alt+A (répondre à un appel), Ctrl+Entrée et Échap (accepter, refuser un appel), Ctrl+' (appeler) | partiel — Ctrl+Entrée, Échap, Ctrl+' faits ; Ctrl+Alt+A absent |
| Ctrl+Maj+Alt+V (annuler le dernier son), Ctrl+1 à 9 (aller au serveur n) | partiel — Ctrl+1 à 9 fait ; annuler le dernier son absent |
| Maj+Page précédente (aller au plus ancien non lu), Maj+Échap fait | **fait** — `Action::OldestUnread` |
| Ctrl+B, Ctrl+I, Ctrl+U dans la saisie (gras, italique, souligné sur la sélection) | **fait** — `ui/composer.rs` |
| Tab, F6 entre les zones, navigation complète au clavier | fait (lot C5) |
| Raccourcis globaux réglables (hors appui-pour-parler) | absent |

### 1.11 Menus contextuels

| Cible | État | Ce qui manque |
|---|---|---|
| Message | partiel | il ne manque que « Mentionner » |
| Utilisateur (membre, auteur d'un message, mention, ligne de MP) | partiel | menu commun `ui/user_menu.rs` (lot U), clic droit sur l'auteur compris ; manquent le volume vocal dans ce menu et le transfert d'un groupe |
| Salon | partiel | inviter et ouvrir la discussion faits ; manquent créer un salon ici, masquer les noms, favoris |
| Catégorie | **fait** | replier, marquer comme lu, notifications (`ui/channel_settings/list.rs`) |
| Serveur | partiel | voir 1.7 |
| Dossier | fait | marquer comme lu, réglages, dégrouper |
| Rôle (pastille de rôle sur une carte, mention de rôle) | partiel | copier l'identifiant sur la pastille (mode développeur) ; rien sur une mention de rôle, pas de « voir les membres » |
| Fil | **fait** | menu de la ligne (`ui/threads.rs`) |
| Réaction | fait | liste de qui a réagi |
| Participant vocal | fait | `ui/voice/list.rs` |
| Lien, image, zone de saisie (couper, copier, coller, orthographe) | partiel | image faite (`ui/attachments.rs`, lot VID) ; lien et zone de saisie : rien |
| Fond de la liste des salons | fait | créer un salon, une catégorie |

### 1.12 Glisser-déposer

| Fonction | État |
|---|---|
| Fichiers sur la fenêtre, image collée | fait |
| Serveurs dans le rail, création de dossier par dépôt | fait |
| Salons et catégories (réordonner, changer de catégorie) | fait (`ui/channels.rs`, administrateurs) |
| Rôles (réordonner dans les paramètres) | **fait** — poignée de glissement (`ui/guild_settings/roles.rs`) |
| Fichier déposé sur un salon de la liste pour l'y envoyer, Maj au dépôt pour envoyer sans fenêtre | absent |
| Utilisateur déposé sur un salon vocal pour le déplacer | absent |
| Ordre des MP épinglés, des autocollants favoris | absent (fonctions absentes) |

### 1.13 Barre de titre et fenêtre

| Fonction | État |
|---|---|
| Barre de titre personnalisée (nom du lieu, boîte de réception, aide, réduire, agrandir, fermer) | **fait** — barre intégrée sous Windows (`ui/titlebar.rs`, `platform/caption.rs`, lot V1) |
| Bouton d'aide | **fait** — `ui/titlebar.rs` |
| Colonne des salons redimensionnable à la souris | **fait** — bord de la colonne (`ui/mod.rs`, lot V2) |
| Fenêtre détachée pour un appel | fait |
| Réduire dans la zone de notification, démarrer avec Windows, liens `discord://` | fait |
| Zoom Ctrl+plus et Ctrl+moins | **fait** — `ui/appearance.rs` |

### 1.14 Statut et compte

| Fonction | État |
|---|---|
| Statut en ligne, inactif, ne pas déranger, invisible | fait |
| Statut personnalisé : texte, emoji, expiration (30 min, 1 h, 4 h, 24 h, jamais) | **fait** — `ui/settings/synced.rs`, emoji animé compris ; seul « effacer aujourd'hui » manque |
| Menu du panneau utilisateur (clic sur l'avatar : statut, statut personnalisé, changer de compte, copier l'identifiant) | **fait** — clic : carte de mon compte ; clic droit : statut, statut personnalisé, compte, identifiant (`ui/quick_profile.rs`) |
| Changement de compte, plusieurs comptes | fait (`ui/accounts.rs`) |
| Connexion par jeton, par QR code | fait |
| Connexion e-mail et mot de passe, création de compte, A2F, clés d'accès | écarté (captcha) |

### 1.15 Paramètres utilisateur, page par page

Ordre du client officiel. « Affichage » veut dire : valeurs lues et montrées,
modification renvoyée vers Discord.

| Page de Discord | État | Détail |
|---|---|---|
| Mon compte | partiel | affichage ; nom d'utilisateur, e-mail, mot de passe, A2F, suppression : « Ouvrir dans Discord » (mot de passe exigé) |
| Profils (utilisateur, par serveur) | **fait** | voir 1.6 |
| Contenu et social (ex-Confidentialité et sécurité) | fait | MP de serveurs, demandes de message, spam, demandes d'ami, contenu explicite |
| Données et confidentialité | partiel | interrupteurs faits ; « Demander mes données » renvoyé vers Discord |
| Centre familial | absent | à renvoyer vers Discord |
| Applications autorisées | fait | liste, révoquer |
| Appareils | fait | liste des sessions, déconnecter une session |
| Connexions | partiel | affichage et visibilité ; ajout d'une connexion renvoyé vers Discord (OAuth) |
| Clips | absent | fonction absente |
| Nitro, Boost de serveur, Abonnements, Inventaire de cadeaux, Facturation | écarté pour les achats | une page « Nitro et facturation » en affichage ; pas de liste des boosts posés ni des cadeaux reçus |
| Apparence | fait, différent | thème propre à Ferrocord, bien plus réglable ; thème du compte Discord affiché avec « Appliquer à Ferrocord » ; pas d'icône d'application (dégradés et image de fond : **faits**, lot PERSO) |
| Accessibilité | fait | mouvement, contraste, saturation, liens soulignés, couleurs de rôle, infobulles ; « synthèse vocale : débit » absent |
| Voix et vidéo | fait | voir 1.8 ; manque : atténuation, arrière-plan vidéo, diagnostic, journal de débogage vocal |
| Discussion (ex-Texte et images) | fait | médias, embeds, réactions, emojis animés, GIF, émoticônes, autocollants, suggestions de commandes, synthèse vocale |
| Notifications | partiel | voir 1.3 |
| Raccourcis clavier | fait | — |
| Langue | fait | langue de Ferrocord (fr, en) et langue du compte |
| Paramètres Windows (démarrage, réduire dans la zone) | fait | pages Ferrocord et Notifications |
| Mode streamer | fait | — |
| Avancés | fait | mode développeur, accélération matérielle (valeur), journal |
| Confidentialité de l'activité, Jeux enregistrés, Overlay | partiel | page Activité (détection, jeux ajoutés, Rich Presence) ; overlay écarté |
| Quoi de neuf, Marchandises, HypeSquad | absent | sans intérêt ; « Quoi de neuf » pourrait montrer les notes de version de Ferrocord |
| Déconnexion | fait | — |

### 1.16 Paramètres du serveur, page par page

| Page de Discord | État | Détail |
|---|---|---|
| Profil du serveur (ex-Vue d'ensemble) | fait | nom, icône, bannière, splash, description, salon système, AFK, notifications par défaut, langue, lien personnalisé |
| Tag du serveur | absent | — |
| Rôles, permissions, membres d'un rôle | fait | `ui/guild_settings/roles.rs`, `role_perms.rs` ; « voir le serveur en tant que rôle » absent |
| Emojis, Autocollants, Tableau de sons | fait | — |
| Membres | **fait** | recherche, rôles, expulser, bannir, exclure, tri par ancienneté ; élagage fait (page à part) ; filtres avancés absents |
| Invitations | fait | liste, révoquer, suspendre |
| Accès (règles, vérification des membres) | absent | — |
| Intégrations, webhooks, permissions des commandes | partiel | webhooks complets ; permissions des commandes en lecture seule |
| Répertoire d'applications | fait (consultation) | onglet « Applications » de la page « Découvrir » (lot ND) ; l'ajout ouvre le navigateur |
| Configuration de sécurité, AutoMod, Journal d'audit, Bannissements | fait | — |
| Communauté : activation, intégration, écran de bienvenue, découverte | fait | — |
| Statistiques du serveur | partiel | renvoi vers Discord |
| Guide du serveur | absent | — |
| Boosts | fait en affichage | l'achat est écarté |
| Abonnements au serveur, boutique du serveur | écarté | paiement |
| Modèle de serveur | **fait** | page Modèle (`ui/guild_settings/extras.rs`, lot V) |
| Widget | **fait** | page Widget (`ui/guild_settings/extras.rs`, lot V) |
| Supprimer le serveur, transférer la propriété | renvoyé | « Ouvrir dans Discord » (A2F) |

### 1.17 Paramètres d'un salon

| Page | État | Détail |
|---|---|---|
| Vue d'ensemble | partiel | nom, sujet, mode lent, limite d'âge, annonces, archivage, étiquettes, réaction par défaut ; débit, limite, région et qualité vidéo d'un salon vocal **faits** (lot S) ; manque : disposition et tri par défaut (forum), masquer après inactivité |
| Permissions | fait | rôles, membres, synchronisation avec la catégorie, salon privé |
| Invitations | fait | — |
| Intégrations | fait | webhooks, salons suivis |
| Supprimer le salon | fait | — |

### 1.18 Nitro et boutique (affichage seulement)

| Fonction | État |
|---|---|
| Avantages Nitro acquis : emojis de partout, envoi de gros fichiers, bannière, avatar animé | partiel : emojis et images faits ; limite d'envoi des serveurs boostés notée comme reste |
| Type d'abonnement, « Nitro depuis » | fait |
| Boutique (parures), aperçu d'une parure sur son profil, inventaire | absent ; affichage seul possible, sans bouton d'achat |
| Cadeaux reçus dans un message (carte de cadeau), liens `discord.gift` | absent ; l'acceptation d'un cadeau n'est pas un achat, mais à trancher avec l'utilisateur |
| Lignes « Nitro » et « Boutique » dans la colonne des MP | absent, et à ne pas ajouter |

### 1.19 Intégrations, applications, commandes

| Fonction | État |
|---|---|
| Commandes slash, options, autocomplétion, composants, fenêtres de saisie, menus contextuels d'applications | fait (`ui/commands.rs`, `ui/components.rs`) |
| Options de type pièce jointe | fait (lot fin-texte-restes) |
| Lanceur d'applications (bouton de la zone de saisie, applications récentes) | absent |
| Applications installées sur le compte (commandes utilisables partout) | partiel *(à confirmer)* : `integration_types` est lu |
| Ajouter une application à un serveur (autorisation OAuth dans le client) | écarté : captcha possible, à ouvrir dans le navigateur |
| Activités, points d'entrée d'activité | écarté |
| Rich Presence locale, activité détectée, boutons d'activité | fait (`src/rpc/`) ; images nommées : traduites après « Charger les images » (page Activités, lot R) |
| Invitation à rejoindre une partie, « Écouter avec » Spotify | absent |

### 1.20 Modération

| Fonction | État |
|---|---|
| Expulser, bannir (avec suppression d'historique), débannir, exclure temporairement, bannissement en masse, suppression en masse | fait |
| AutoMod : règles, mots-clés, expressions, mentions, réponses, exemptions | fait |
| Messages d'alerte AutoMod dans le salon, avec leurs actions | **fait** — voir le message, sanctions, règles (`ui/system_msgs.rs`, lot V) |
| Journal d'audit avec filtres | fait |
| Protection contre les raids, pause des invitations et des MP | fait (`ui/guild_settings/safety.rs`) |
| Vue « Membres » de modération (signaux, ancienneté, filtres) | partiel |
| Signaler un message, un utilisateur, un serveur | renvoyé — pour un message (navigateur) ; utilisateur et serveur : rien |
| Élagage des membres inactifs | **fait** — page Élagage (`net/admin/extras.rs`, lot V) |
| Vérification des membres, règles | absent |

### 1.21 Quêtes et le reste

| Fonction | État |
|---|---|
| Quêtes (promotions sponsorisées avec récompense) | partiel (lot ND) : page des quêtes, accepter, réclamer, code, progression renvoyée par Discord, solde d'Orbes. **Jamais** de progression déclarée par Ferrocord : une quête se termine dans l'application officielle |
| Mise à jour automatique signée, installateur, licences | fait |
| Historique hors ligne | fait (Windows) |
| Notes de version dans l'application | absent |
| Écran de chargement avec astuces | absent (indicateur sobre, voulu) |
| Indicateur de connexion perdue, reconnexion | fait |
| Sondage de qualité d'appel, retours | absent, sans intérêt |
| Linux, macOS | absent (C7) |

---

## 2. Ce qui manque, classé

**État au 15 novembre 2026.** Faits : 1 à 7, 9 (début de MP réduit), 10 à 14,
16, 23, 24, 29. Faits en partie : 8 (la fréquence des emojis n'est jamais
écrite), 15 (restent mentions surlignées, événements muets, sons par
événement), 17 (envoi fait, pas d'animation), 18 (par le menu d'une personne
et depuis un salon vocal ; pas de liste d'amis dans la fenêtre), 19 (liens
externes faits ; limite d'âge et floutage absents), 20 (restent Ctrl+Alt+A,
Ctrl+Maj+Alt+V, Ctrl+Maj+I, raccourcis globaux), 22 (galerie et salons médias
faits ; membres du fil, verrou, fil privé absents), 25 (emoji de statut animé
seul), 27 (bannière faite ; lignes absentes), 28 (modèle, widget, élagage
faits), 30 (son d'un direct hors appel fait), 32 (image faite, rôle en
partie). Toujours absents : 21, 26, 31, 33 à 36.

Importance : **A** se voit tous les jours, **B** chaque semaine, **C**
occasionnel ou réservé aux administrateurs. Coût : S (moins d'une demi-journée
d'agent), M, L, XL.

### 2.1 Importance A

| # | Manque | Coût | Remarque |
|---|---|---|---|
| 1 | Sélecteur de GIF (recherche, tendances, catégories), puis favoris | L | points `/gifs/search`, `/gifs/trending`, `/gifs/select` non documentés : forme à vérifier et à inscrire dans `docs/passerelle-utilisateur.md` (règle 1.2.5) ; les GIF s'affichent déjà |
| 2 | Lecture des vidéos et des sons dans le fil | L | décodeurs H.264 et VP8 déjà là (`src/video/codec/`) ; il manque un démultiplexeur MP4 et WebM, l'AAC par Media Foundation et un lecteur (lecture, pause, position, volume, plein écran) |
| 3 | Marquer comme non lu | S | accusé de lecture posé sur le message précédent (`POST …/ack` avec `manual: true`), forme à vérifier |
| 4 | Sujet du salon dans l'en-tête, bouton Fils (panneau actifs et archivés), cloche de notification | M | `ui/chat.rs`, `ui/threads.rs` |
| 5 | Menu de l'utilisateur complet : appeler, inviter au serveur, pseudo, rôles à cocher, ignorer, fermer le MP ; et clic droit sur l'avatar et le nom d'un auteur | M | les commandes réseau existent presque toutes (`net/admin/roles.rs`, `net/calls.rs`) |
| 6 | Replier les catégories, masquer les salons muets, menu de catégorie (replier, marquer comme lu, muet) | M | état dans les réglages du compte synchronisés (protobuf, `state/usersettings/`) |
| 7 | Barre de survol à la façon de Discord : réactions rapides, menu « ⋯ » ; menu du message complété (copier et enregistrer l'image, supprimer les intégrations, lire à voix haute, signaler vers Discord) | M | — |
| 8 | Emojis favoris et les plus utilisés, rail de catégories, bandeau d'aperçu | M | fréquence synchronisée dans les réglages du compte (protobuf) |
| 9 | Début de salon et début de MP, bandeau « nouveaux messages depuis… » | S | — |
| 10 | Demandes de message et spam (dossier, accepter, ignorer) | M | forme à capturer |
| 11 | Barre de titre personnalisée avec boîte de réception et aide | M | traitée avec le lot visuel 1 (`docs/ecarts-visuels.md`) |
| 12 | Menu du panneau utilisateur : statut et statut personnalisé en deux clics | S | la fenêtre de statut existe dans les paramètres |

### 2.2 Importance B

| # | Manque | Coût |
|---|---|---|
| 13 | « Actifs en ce moment », pseudos d'amis, « + » de la colonne des MP | M |
| 14 | Panneau vocal : caméra, partage, tableau de sons, suppression de bruit ; réglages d'un salon vocal (débit, limite, région, qualité) | M |
| 15 | Fenêtre de notifications du serveur (exceptions par salon, mentions surlignées, événements), réglage par fil, sons par événement | M |
| 16 | Boîte « Pour vous » | M |
| 17 | Envoi de super réactions, animation | M |
| 18 | Inviter des amis depuis la fenêtre d'invitation, depuis un salon vocal | S |
| 19 | Confirmation des liens externes, écran des salons à limite d'âge, floutage local des médias sensibles | M |
| 20 | Raccourcis manquants (Ctrl+F, Ctrl+G, Ctrl+Maj+U, Ctrl+1 à 9, appels, gras et italique dans la saisie) | S |
| 21 | Aperçu de la mise en forme dans la saisie, barre de mise en forme | L |
| 22 | Fils : membres, verrou, fil privé ; forums : galerie, extrait, vignette ; salons médias en grille | M |
| 23 | Profil par serveur complet (bio, pronoms), aperçu en direct | S |
| 24 | En-tête de MP : appel vidéo, ajouter au MP, panneau de profil | M |
| 25 | Autocollants Lottie, plaques animées, noms stylisés, emoji de statut animé (effets de profil : faits, lot ND) | L |
| 26 | Texte collé trop long en fichier, message silencieux signalé | S |
| 27 | Lignes Parcourir, Membres, Boosts en tête de liste ; bannière du serveur | S |

### 2.3 Importance C

| # | Manque | Coût |
|---|---|---|
| 28 | Paramètres du serveur : vérification des membres, guide, modèle, widget, tag, élagage, permissions des commandes modifiables | L |
| 29 | Actions des alertes AutoMod | S |
| 30 | Atténuation, arrière-plan vidéo, AV1 et VP9, son d'un direct hors appel | L à XL |
| 31 | Glisser un fichier sur un salon, un membre sur un salon vocal | S |
| 32 | Menus contextuels du rôle, du lien, de la zone de saisie | S |
| 33 | Recherches récentes, onglets de résultats | M |
| 34 | Correcteur orthographique | XL |
| 35 | Notes de version dans l'application, Centre familial et Clips renvoyés vers Discord | S |
| 36 | Inventaire en affichage, cartes de cadeau (boutique en consultation : faite, lot ND) | M, à trancher |

### 2.4 Écarté

- Achats : Nitro, boosts, boutique, cadeaux à offrir, abonnements de serveur.
- Tout ce qui passe par un captcha : connexion par mot de passe, ajout d'une
  application à un serveur, changement de mot de passe ou d'e-mail.
- Progression des quêtes : la déclarer supposerait d'automatiser ou d'imiter
  le client officiel. Les quêtes sont affichées, acceptées et réclamées
  (lot ND), jamais accomplies par Ferrocord.
- Activités intégrées, lecteurs web des liens, overlay en jeu (plan,
  chapitre 5).
- Signalements : le formulaire est renvoyé vers le navigateur.

---

## 3. Découpage en lots parallèles

Six lots, sans fichier commun hors des points de branchement habituels
(`src/ui/mod.rs`, `src/state/store.rs`, `src/net/gateway.rs`,
`examples/preview.rs`) : chaque lot y ajoute ses lignes, signalées dans son
rapport, et tient ses textes dans un fichier `src/i18n/` à lui. Toute requête
non documentée suit la règle 1.2.5 (forme vérifiée, jeu d'essai, limiteur,
inscription dans `docs/passerelle-utilisateur.md`).

### Lot G — Expressions (L)

Fonctions : sélecteur de GIF (recherche, tendances, catégories, envoi), GIF
favoris, emojis favoris et les plus utilisés, rail de catégories, bandeau
d'aperçu, repli des sections, envoi de super réactions, onglet Sons en vocal,
raccourcis Ctrl+G et Ctrl+S.

Fichiers : `src/ui/emoji_picker.rs`, `src/ui/composer_emoji.rs`,
`src/ui/stickers.rs`, `src/ui/reactions.rs`, `src/state/emoji/`,
`src/net/stickers.rs` ; nouveaux : `src/ui/gif_picker.rs`, `src/state/gifs.rs`,
`src/net/gifs.rs`, `src/i18n/gifs.rs`.

### Lot M — Messages et fil (L)

Fonctions : marquer comme non lu ; barre de survol (réactions rapides, « ⋯ ») ;
menu du message complété (copier et enregistrer l'image, supprimer les
intégrations, lire à voix haute, signaler vers Discord) ; visionneuse (copier,
enregistrer) ; lecteur vidéo et audio intégré ; début de salon et de MP ;
bandeau des nouveaux messages ; confirmation des liens externes ; texte trop
long en fichier ; écran des salons à limite d'âge.

Fichiers : `src/ui/msg_actions.rs`, `src/ui/message.rs`,
`src/ui/attachments.rs`, `src/ui/viewer.rs`, `src/ui/links.rs`,
`src/ui/reads.rs`, `src/state/reads.rs`, `src/state/media.rs`,
`src/net/social.rs`, `src/net/media.rs` ; nouveaux : `src/ui/player.rs`,
`src/video/demux/`. Dans `src/ui/chat.rs`, seulement la liste des messages
(`message_list` et ce qu'elle appelle), pas l'en-tête.

### Lot U — Personnes et MP (M)

Fonctions : menu de l'utilisateur complet et présent partout (membre, auteur,
mention, ligne de MP, ami) ; demandes de message et spam ; « Actifs en ce
moment » ; pseudos d'amis ; « + » de la colonne des MP ; inviter un ami à un
serveur ; profil par serveur complet ; champ de message de la carte ; menu du
panneau utilisateur (statut, statut personnalisé, changer de compte).

Fichiers : `src/ui/profile.rs`, `src/ui/profile_full.rs`,
`src/ui/friends.rs`, `src/ui/groups.rs`, `src/ui/members.rs`,
`src/ui/moderation.rs`, `src/state/relations.rs`, `src/state/people.rs`,
`src/state/profiles.rs`, `src/net/relations.rs`, `src/net/relations_rest.rs`,
`src/net/people.rs`, `src/net/profile.rs`, `src/ui/settings/account.rs`.

### Lot S — Salons et serveurs (M)

Fonctions : en-tête de discussion (sujet du salon, bouton Fils et son panneau,
cloche, appel vidéo et « ajouter au MP ») ; replier les catégories ; masquer
les salons muets ; menus de catégorie, de fil et de rôle ; lignes Parcourir,
Membres, Boosts ; bannière du serveur ; réglages du salon vocal ; fils
(membres, verrou, privé) ; forums (galerie, extrait) ; salons médias en
grille ; glisser un fichier sur un salon.

Fichiers : `src/ui/channels.rs`, `src/ui/threads.rs`, `src/ui/forum.rs`,
`src/ui/browse.rs`, `src/ui/channel_settings/`, `src/state/threads.rs`,
`src/state/store_threads.rs`, `src/state/nav.rs`, `src/state/admin/channels.rs`,
`src/net/threads.rs`, `src/net/admin/channels.rs`. Dans `src/ui/chat.rs`,
seulement l'en-tête (lignes 455 à 512 aujourd'hui) : à sortir d'abord dans un
nouveau `src/ui/chat_head.rs`, pour ne plus partager `chat.rs` avec le lot M.

### Lot N — Notifications, boîte, raccourcis (M)

Fonctions : fenêtre de notifications du serveur (exceptions par salon,
mentions surlignées, événements muets) ; muet et niveau par catégorie et par
fil ; sons par type d'événement ; boîte « Pour vous » ; raccourcis manquants
(Ctrl+F, Ctrl+Maj+U, Ctrl+1 à 9, Ctrl+Maj+N, Ctrl+Maj+T, appels) et gras,
italique, souligné dans la saisie ; notes de version ; renvois Centre familial
et Clips.

Fichiers : `src/ui/inbox.rs`, `src/ui/keys.rs`, `src/ui/settings/menu.rs`,
`src/ui/settings/synced.rs`, `src/ui/settings/pages.rs`, `src/ui/desk.rs`,
`src/state/notify.rs`, `src/state/inbox.rs`, `src/state/keys.rs`,
`src/state/usersettings/`, `src/net/inbox.rs`, `src/desk/`.

État (groupe 2) : faits, la fenêtre des notifications du serveur avec ses
exceptions par salon ou par catégorie, le réglage par fil, l'onglet « Pour
vous » (demandes d'ami, événements proches), Ctrl+F, Ctrl+Maj+U, Ctrl+G,
Ctrl+S, Ctrl+Maj+N, Ctrl+T, Ctrl+Maj+H, Maj+Page préc., Ctrl+Entrée et Échap
(appel entrant), Ctrl+', Ctrl+1 à 9, gras, italique et souligné sur la
sélection, la pastille chiffrée des salons, les notifications Windows rangées
par serveur. Restent : sons par type d'événement, mentions surlignées,
événements muets et notifications mobiles du serveur,
raccourcis globaux, notes de version, renvois Centre familial et Clips.

### Lot V — Vocal et administration (M à L)

Fonctions : panneau vocal (caméra, partage, tableau de sons, suppression de
bruit) ; inviter depuis un salon vocal ; glisser un membre vers un salon vocal ;
atténuation ; paramètres du serveur manquants (vérification des membres,
modèle, widget, tag, élagage, permissions des commandes) ; actions des alertes
AutoMod.

Fichiers : `src/ui/voice/`, `src/ui/video/`, `src/ui/call.rs`,
`src/ui/soundboard.rs`, `src/state/voice.rs`, `src/audio/`,
`src/ui/guild_settings/`, `src/state/admin/` (sauf `channels.rs`),
`src/net/admin/` (sauf `channels.rs`), `src/ui/system_msgs.rs`.

### Ordre conseillé

G, M, U et S d'abord : ils portent les douze manques d'importance A. N et V
ensuite. La barre de titre personnalisée est dans le lot visuel 1
(`docs/ecarts-visuels.md`), parce qu'elle change la structure de la fenêtre.
