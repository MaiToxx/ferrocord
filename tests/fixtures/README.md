# Jeux de données de test

## Fournir les vraies formes (à faire par l'utilisateur, sur son compte)

Les agents ne se connectent jamais à ton compte. Pour comparer ces jeux de
données à ce que Discord envoie vraiment, tu peux enregistrer toi-même la
*forme* des paquets, sans leur contenu :

1. Compile une version **debug** (l'enregistreur n'existe pas en release) :
   `cargo build`
2. Choisis un dossier vide et lance l'app avec la variable définie, par
   exemple dans PowerShell :
   ```powershell
   $env:FERROCORD_RECORD_SHAPES = "$env:USERPROFILE\ferrocord-formes"
   .\target\debug\ferrocord.exe
   ```
3. Connecte-toi normalement et utilise l'app quelques minutes (ouvrir des
   salons, recevoir des messages, en modifier un…) : chaque nouveau type
   d'événement est enregistré à sa première apparition.
4. Ferme l'app. Le dossier contient un fichier par type d'événement :
   `READY.json`, `MESSAGE_CREATE.json`, `GUILD_CREATE.json`, etc. Un nouveau
   lancement réécrit les fichiers des types qu'il rencontre.
5. **Ouvre les fichiers et vérifie-les** avant de les transmettre, puis
   donne le dossier à l'agent chargé de mettre à jour ces jeux de données.

Ce que contiennent les fichiers : uniquement la structure de la partie `d`
de chaque paquet. Les noms de champs de l'API figurant dans une liste fixe
(`KNOWN_FIELDS` dans `src/net/shapes.rs`) sont conservés ; chaque chaîne
devient `"<str>"` (ou `"<id>"` si elle n'est faite que de chiffres), chaque
nombre `0`, chaque booléen `false`, et chaque tableau est réduit à son
premier élément.

Ce qu'ils ne contiennent pas : aucun message, aucun nom, pseudo, adresse
e-mail ou identifiant réel, aucun jeton. Toute clé qui n'est pas un nom de
champ connu (identifiant, pseudo, code de langue…) est remplacée par `"<id>"`
ou `"<key>"`. Revers de cette prudence : un vrai champ absent de la liste
apparaît aussi comme `"<key>"` ; on l'ajoute à la liste au besoin.

## Origine de ces jeux de données

**Ce sont des données synthétiques, écrites à la main d'après la documentation
publique de Discord (https://discord.com/developers/docs), et non des captures
réelles.** Identifiants, noms, contenus et adresses sont inventés :

- identifiants à 19 chiffres par plage : `1…` utilisateurs, `2…` serveurs,
  `3…` salons, `4…` MP, `5…` messages, `6…` application, `7…` pièces jointes ;
  les rôles sont dans la plage des serveurs (`2000000000000001xxx`, @everyone
  portant l'identifiant du serveur, comme le veut l'API) ;
- adresses en `.invalid` ou chemins manifestement fictifs ;
- aucun jeton, même factice (vérifié par `tests/e2e/fixtures.rs`).
- empreintes d'images (`avatar`, `icon`) inventées, de la forme documentée (32 chiffres hexadécimaux) : avatars d'Alice et de moi, icône d'Alpha ; Bob, Ève, Carol, Bêta, Gamma et le groupe n'en ont pas (avatar par défaut ou initiales).

Chaque fichier de passerelle est un paquet complet `{ "op": 0, "t", "s", "d" }` ;
`rest_history.json` est la réponse de `GET /channels/{id}/messages` (du plus
récent au plus ancien).

| Fichier | Contenu |
|---|---|
| `ready_user_classic.json` | READY de compte utilisateur, ancien format : `guilds[].name`, `private_channels[].recipients`, `user_settings.guild_positions` |
| `ready_user_properties.json` | READY de compte utilisateur, format récent : `guilds[].properties.name`, `private_channels[].recipient_ids` + `users`, sans `user_settings` |
| `ready_bot.json` | READY de bot (forme documentée) : serveurs `unavailable` |
| `guild_create_alpha.json`, `guild_create_beta.json` | GUILD_CREATE complets (avec un salon vocal, ignoré par le client) |
| `guild_delete.json`, `guild_delete_unavailable.json` | départ d'un serveur / panne |
| `channel_create.json`, `channel_update.json`, `channel_delete.json` | salon de serveur |
| `channel_create_dm.json` | nouveau MP (sans `guild_id`, avec `recipients`) |
| `message_create_*.json` | simple, pièces jointes, embeds, mention, pseudo de serveur, message système (type 7), MP |
| `message_create_media.json` | toutes les formes de pièces jointes (image, spoiler, GIF, vidéo, son, fichier) et d'embeds (riche complet, vidéo, gifv), d'après la documentation « Message Resource » |
| `message_update_partial.json`, `message_update_embeds_only.json` | modifications partielles |
| `message_delete.json` | suppression |
| `rest_history.json` | une page d'historique REST (3 messages, sans `guild_id` ni `member`) |
| `ready_user_roles.json` | READY utilisateur, format classique, avec rôles, dérogations et mon membre dans `guilds[].members` : Alpha (salon et catégorie réservés aux modérateurs, #annonces en lecture seule, #ancien sans historique), Bêta (j'en suis propriétaire), Gamma (mon membre absent : permissions inconnues) |
| `ready_user_merged.json` | même contenu au format « capacités modernes » : `guilds[].properties.owner_id`, mon membre dans `merged_members` (un tableau par serveur, dans l'ordre de `guilds`, avec `user_id`) |
| `guild_member_update_roles.json`, `guild_member_update_timeout.json` | mon membre change : rôle Modérateur ajouté, ou exclusion temporaire (`communication_disabled_until`) |
| `guild_role_create.json`, `guild_role_update.json`, `guild_role_delete.json` | rôle créé, modifié (@everyone perd SEND_MESSAGES), supprimé (Modérateur) |
| `guild_update.json` | GUILD_UPDATE : je deviens propriétaire |
| `channel_update_overwrites.json` | dérogation de membre qui me retire VIEW_CHANNEL sur #hors-sujet |
| `message_create_reply.json` | réponse de Bob (type 19) à « Bonjour » d'Alice, avec `referenced_message` et des réactions (Unicode, du serveur, super-réaction) |
| `message_create_forward.json` | transfert (`message_reference.type` 1, `message_snapshots`, drapeau 16384), message épinglé |
| `message_reaction_add.json`, `message_reaction_add_custom.json` | réaction d'Alice ; super-réaction d'Ève avec un emoji du serveur |
| `message_reaction_remove*.json` | retrait d'une réaction, de toutes, de toutes celles d'un emoji |
| `channel_pins_update.json`, `guild_emojis_update.json` | épingles modifiées ; emojis du serveur (dont un indisponible). Les emojis personnalisés sont dans la plage `6…` |
| `rest_pins.json` | réponse de `GET /channels/{id}/messages/pins` (`items[].pinned_at` + `message`, `has_more`) |
| `rest_reactors.json` | réponse de `GET /channels/{id}/messages/{id}/reactions/{emoji}` (utilisateurs) |
| `typing_start_guild.json`, `typing_start_dm.json` | TYPING_START (documenté) : dans un serveur avec `member` (surnom), dans un MP sans `member` |
| `guild_emojis_update_roles.json` | GUILD_EMOJIS_UPDATE (documenté) : emojis ordinaire, animé, indisponible, réservé à un rôle, plus deux entrées illisibles (sans identifiant, nom non textuel) |
| `channel_update_slowmode.json` | CHANNEL_UPDATE : mode lent de 30 s (`rate_limit_per_user`) sur #général |
| `presence_update.json` | PRESENCE_UPDATE (forme documentée) : Alice en ligne, `client_status` ordinateur + mobile, statut personnalisé (type 4, emoji Unicode), jeu (type 0, `timestamps.start`), écoute (type 2) |
| `presence_update_rich.json` | PRESENCE_UPDATE (lot C3, forme de docs.discord.food « Presence » › Activity Object) : écoute Spotify (`sync_id`, `timestamps` en chaînes avec `end`, image `spotify:`), jeu avec `application_id`, `session_id`, images d'application et `mp:`, deux boutons ; regarde, avec des images refusées (`twitch:`, chemin hostile) |
| `rest_games_detectable.json` | réponse de `GET /games/detectable` (lot C3, **non documentée**, forme de docs.discord.food « Game » › Detectable Application Object, exemple de la page anonymisé) : exécutables Windows et Linux, lanceur seul, exécutable à arguments, jeu macOS seul, application sans exécutable, formes étranges |
| `rest_activity_metadata.json` | réponse de `GET /users/{id}/sessions/{session}/activities/{app}/metadata` (lot C3, **non documentée**, docs.discord.food « Presence » › Activity Metadata) : deux adresses de boutons, dont une refusée (`javascript:`) |
| `rpc/set_activity.json` | trame `SET_ACTIVITY` d'un jeu sur le tube local (lot R, forme de discord-rpc `src/serialization.cpp` et de la documentation Discord RPC) : horodatages en secondes, image par identifiant et image par nom (refusée), groupe, secrets (ignorés), deux boutons dont un refusé (`javascript:`), champ inconnu. Dans un sous-dossier : ce n'est pas une forme de Discord, l'enregistreur de formes (`net::shapes`) ne la couvre pas |
| `rest_user.json` | réponse de `GET /users/{id}` : bannière, `accent_color`, `public_flags` (HypeSquad Brilliance + développeur actif) |
| `rest_guild_member.json` | réponse de `GET /guilds/{id}/members/{id}` : pseudo, avatar de serveur, rôle, `joined_at`, `premium_since` |
| `rest_dm_create.json` | réponse de `POST /users/@me/channels` : MP à deux avec Bob |
| `rest_message_upload.json` | réponse de `POST /channels/{id}/messages` en multipart (lot T8, forme documentée « Uploading Files ») : message avec deux pièces jointes (dont `SPOILER_` et `description`), `nonce` |
| `rest_recent_mentions.json` | réponse de `GET /users/@me/mentions` (lot navigation, forme de docs.discord.food « User » : liste d'objets message) : une mention dans #hors-sujet d'Alpha (sans `guild_id`, avec un champ inconnu) et une dans le MP d'Alice |
| `guild_member_list_update_sync.json`, `guild_member_list_update_ops.json` | liste des membres d'Alpha (**non documentée**, forme tirée de sources communautaires, voir `docs/passerelle-utilisateur.md`) : SYNC d'un rôle affiché à part, en ligne, hors ligne ; puis DELETE, INSERT, UPDATE, INVALIDATE et une opération illisible |
| `ready_user_social.json` | READY utilisateur (lot S3 + T9) : `relationships` (ami, demandes reçue et envoyée, bloqué, utilisateur ignoré de type 0, une relation sans `user` mais avec `user_id`), `users`, MP et groupe avec `owner_id`, `read_state` (tableau ; un `id` en nombre, une entrée de type 1 ignorée), `last_message_id` des salons |
| `ready_supplemental.json` | READY_SUPPLEMENTAL : `merged_presences.friends` (statuts et activités) |
| `relationship_add.json`, `relationship_update.json`, `relationship_remove.json` | Carol devient amie ; Alice reçoit un surnom et est ignorée (`user_ignored`) ; Dan est retiré |
| `message_ack.json` | MESSAGE_ACK (type 0) : #général lu sur un autre appareil |
| `presence_update_friend.json` | PRESENCE_UPDATE sans `guild_id` : Bob passe « absent » |
| `channel_recipient_add.json`, `channel_recipient_remove.json` | Carol rejoint le groupe ; Ève le quitte |
| `channel_create_message_request.json` | demande de message rangée dans le spam (`is_message_request`, `is_spam`, lot U) |

Sources des formes sociales : docs.discord.food (Relationships, Gateway
Events) et discord-userdoccers (Read State), documentation communautaire de
l'API utilisateur ; détail dans `docs/passerelle-utilisateur.md`.

## Formes non confirmées

La documentation publique ne couvre que les bots. Pour les comptes utilisateur,
les formes suivantes viennent du comportement observé de clients tiers et
**n'ont pas pu être confirmées** :

- **READY utilisateur** dans son ensemble (non documenté) : `guilds[].properties`,
  `guilds[].data_mode`, `lazy`, `private_channels[].recipient_ids`, `users`,
  `merged_members`, `read_state`, `relationships`, `session_type` ;
- **`user_settings.guild_positions`** : ancien format JSON ; les comptes récents
  reçoivent `user_settings_proto` (protobuf en base64), et l'ordre des serveurs
  y est dans les dossiers (`guild_folders`). Le contenu `"AAAA"` est un
  remplissage sans signification ;
- **MESSAGE_UPDATE partiel** : la documentation actuelle décrit un objet message
  avec les mêmes champs que MESSAGE_CREATE, sans dire s'il est toujours complet.
  Les fichiers `message_update_*` couvrent le cas partiel, que le client doit
  continuer à supporter ;
- présence ou absence de `guild_id` dans les salons de GUILD_CREATE (la
  documentation ne le précise pas ; ici il est absent) ;
- valeurs de `flags`, `premium_type`, `public_flags` : arbitraires (0).

### Permissions (lot F4)

- **Rôles, propriétaire et dérogations dans le READY utilisateur** : supposés
  aux mêmes places que dans GUILD_CREATE (`guilds[].roles`,
  `guilds[].owner_id` ou `guilds[].properties.owner_id`,
  `channels[].permission_overwrites`). S'ils manquent, rien n'est masqué.
- **Mon membre** : cherché dans `guilds[].members` (format classique, élément
  avec `user.id`) puis dans `merged_members[i]` (élément avec `user_id`).
  `merged_members` est supposé aligné sur `guilds`, serveurs indisponibles
  compris ; s'il n'a pas exactement autant d'entrées que `guilds`, il est
  ignoré. Garde-fou supplémentaire : si un de mes rôles est absent de la
  liste du serveur, les permissions de ce serveur sont inconnues.
- **Formes anciennes tolérées** : `type` d'une dérogation en texte
  (`"role"`, `"member"`) et permissions en nombre plutôt qu'en chaîne. Toute
  autre forme rend la dérogation illisible, et le salon garde des
  permissions inconnues (affiché, saisie ouverte). Un rôle aux permissions
  illisibles est écarté : ceux qui le portent ont des permissions inconnues.
- **GUILD_MEMBER_UPDATE** pour un compte utilisateur : supposé complet (avec
  `roles`), comme pour les bots. Illisible ou sans `roles` : permissions du
  serveur inconnues jusqu'au prochain paquet lisible.
- **GUILD_CREATE** d'un serveur déjà connu sans mon membre : les permissions
  de ce serveur redeviennent inconnues (on ne garde pas un ancien membre
  peut-être périmé).
- **GUILD_UPDATE** : seul `owner_id` est lu (ou `properties.owner_id`).
- Ce qui n'est jamais supposé : un serveur dont mon membre, le rôle
  @everyone ou un de mes rôles manque garde exactement le comportement
  d'avant (tous les salons visibles, saisie active, erreur 403 affichée).

### Profils et présence (lot S)

- **PRESENCE_UPDATE, `presences` de GUILD_CREATE** : forme documentée (bots).
  Pour un compte utilisateur, Discord n'envoie ces présences que pour les
  petits serveurs et après souscription pour les gros (op 14/37, non
  documentés, non envoyés) : la quantité reçue n'est pas connue.
- **`presences` du READY d'un compte utilisateur** (amis) : supposées de la
  même forme qu'un Presence Update ; absentes ou illisibles, elles sont ignorées.
  Les présences de `READY_SUPPLEMENTAL` (`merged_presences`, non documenté)
  ne sont pas lues.
- **`GET /users/{id}` et `GET /guilds/{id}/members/{id}` avec un jeton
  utilisateur** : documentés pour les bots ; s'ils sont refusés, la carte de
  profil s'en passe sans message.
## Réglages du compte (fenêtre Paramètres)

Données synthétiques, formes tirées de docs.discord.food (voir
`docs/passerelle-utilisateur.md`) :

- `rest_settings_proto.json` : réponse de `GET /users/@me/settings-proto/1`,
  même chaîne que le champ `user_settings_proto` du READY. Contenu du
  protobuf (`PreloadedUserSettings`) : `versions.data_version` = 4 ;
  `text_and_images` : `inline_embed_media` vrai, `message_display_compact`
  vrai, champ 500 inconnu (« inconnu ») ; `status` : « idle », statut
  personnalisé « En pause » ; `appearance.theme` = 2 (clair) ; champ de
  premier niveau 250 inconnu (« futur »). Les champs inconnus servent à
  vérifier qu'une écriture les renvoie tels quels.
- `user_settings_proto_update.json` : mise à jour partielle
  (`inline_embed_media` à faux, enveloppe vide).
- `user_guild_settings.json` : extrait du READY (forme versionnée), avec une
  entrée de forme inattendue qui doit être ignorée seule.
- `rest_me.json`, `rest_auth_sessions.json`, `rest_connections.json`,
  `rest_oauth2_tokens.json` : réponses REST de « Mon compte », « Appareils »,
  « Comptes liés » et « Applications autorisées » (adresse e-mail en
  `.invalid`, numéro de téléphone fictif).
- `rest_profile_errors.json` : refus d'une écriture du profil (lot « profil
  rapide »), au format d'erreur documenté par Discord (`code`, `message`,
  `errors.<champ>._errors`), en cas nommés (`name`, `response`) : nom, pseudo ou avatar changé trop souvent, image
  trop lourde (champ ou 40005), couleurs de thème sans Nitro, bio trop longue,
  refus sans détail. Codes de champ synthétiques.
- `rest_consent.json` : réponse de `GET /users/@me/consent` (docs.discord.food,
  « User Settings », Get User Consents).

## Vocal des serveurs (phase 4, lot V4)

Données synthétiques, formes documentées par Discord (Voice State,
VOICE_STATE_UPDATE, VOICE_SERVER_UPDATE, salons de type 2 et 13) :

- `guild_create_voice.json` : serveur « Serveur Vocal » dont je suis
  propriétaire, trois salons vocaux (dont un limité à 2 personnes et sans
  SPEAK pour @everyone, et une scène) et quatre états vocaux : complets
  (`member.user`) pour Alice, Dana (micro coupé, caméra) et Bob (sourdine),
  partiel pour Ève (rendue muette par le serveur, en direct ; son nom vient
  de `members`).
- `voice_state_update.json` : mon arrivée dans « Salon vocal », avec ma session.
- `voice_server_update.json` : serveur vocal (`endpoint` en `.invalid`).
  Le champ `token` est absent (aucun jeton, même factice, dans les jeux de
  données) : les tests l'ajoutent en mémoire.

## Appels, conférences, tableau de sons (phase 4, lot V6)

| Fichier | Contenu |
|---|---|
| `call_create.json`, `call_update.json`, `call_delete.json` | Appel du MP d'Alice (lot V6, non documenté, forme de docs.discord.food) : Alice est dans l'appel et me fait sonner (un état illisible), puis la sonnerie s'arrête, puis l'appel se termine |
| `voice_state_update_stage.json` | VOICE_STATE_UPDATE : Bob, auditeur de la Grande scène, lève la main |
| `guild_create_stage.json` | Serveur « Club de conférences » dont je suis propriétaire : une scène en cours (« Questions du jeudi ») avec une oratrice, une main levée et un auditeur, un salon vocal avec un statut et une personne au micro coupé, un état sans utilisateur |
| `stage_instance_create.json` | STAGE_INSTANCE_CREATE de la Grande scène |
| `voice_channel_status_update.json` | VOICE_CHANNEL_STATUS_UPDATE (non documenté) : nouveau statut du salon vocal |
| `voice_channel_effect_send.json` | VOICE_CHANNEL_EFFECT_SEND : Alice joue le son 3 du tableau à mi-volume |
| `rest_soundboard_default_sounds.json` | Réponse de `GET /soundboard-default-sounds` : trois sons et une entrée illisible |

## Personnalisation des profils (lot P)

| Fichier | Contenu |
|---|---|
| `rest_user_cosmetics.json` | Objets tels que REST les renvoie, sans enveloppe. `user` : Alice parée (avatar et bannière animés, décoration animée, plaque `cobalt`, tag de serveur « RUST », style de nom en dégradé) ; `member` : sa bannière et sa décoration de serveur ; `roles` : un rôle uni, un en dégradé, un holographique (formes de la documentation officielle, sauf `display_name_styles` : docs.discord.food) |

## Salons et serveurs (phase 10, lot S)

| Fichier | Contenu |
|---|---|
| `rest_channel_voice.json` | Salon vocal tel que `GET /channels/{id}` le renvoie : `bitrate`, `user_limit`, `rtc_region`, `video_quality_mode` (documentation officielle, Channel Object) |
| `rest_voice_regions.json` | Réponse de `GET /voice/regions` : régions courantes, une déconseillée et des entrées de forme inattendue qui doivent être écartées seules |
| `av1/frames.json` | Synthétique : huit images AV1 320×240 de la mire des tests, encodées par l'encodeur AV1 matériel de Windows (`ecrit_jeu_av1`), unités temporelles telles que sorties (image clé à la première et à la cinquième) |
