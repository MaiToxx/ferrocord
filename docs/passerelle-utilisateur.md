# Passerelle des comptes utilisateur : ce qu'il faut capturer avant d'aller plus loin

Le lot F8 prévoyait aussi « le format des capacités modernes » et « les
souscriptions aux serveurs (op 14) ». Ni l'un ni l'autre n'est documenté
publiquement par Discord : la documentation ne décrit que la passerelle des
bots. Envoyer à l'aveugle un champ ou un opcode dont on ne connaît pas la
forme exacte rend le client reconnaissable et peut mettre le compte de
l'utilisateur en danger (règles 1.1.6 et 1.2 du plan). Ils **ne sont donc pas
implémentés**. Ce document dit précisément ce qu'il faudra observer pour le
faire plus tard, et comment le capturer sans rien divulguer.

## Ce que Ferrocord envoie aujourd'hui (après F8)

Adresse : `wss://gateway.discord.gg/?v=10&encoding=json&compress=zlib-stream`
(le paramètre `compress` est le seul ajout de F8 ; il disparaît après un flux
compressé illisible).

| Opcode | Quand | Champs de `d` |
|---|---|---|
| 2 Identify | nouvelle session (au plus une toutes les 5 s, 1000 par 24 h) | `token`, `properties`, `compress`: false, `large_threshold`: 50, `intents` (bots seulement). `properties` : en mode bureau (par défaut depuis le 14 novembre 2026), celles du client de bureau, voir « Identité annoncée » à la fin ; en mode Ferrocord et pour les bots, { `os`, `browser`: "Ferrocord", `device`: "Ferrocord" } |
| 6 Resume | reprise | `token`, `session_id`, `seq` |
| 1 Heartbeat | toutes les `heartbeat_interval` ms, et sur op 1 du serveur | numéro de séquence ou `null` |
| 37 Guild Subscriptions Bulk (**non documentée**, lot S1) | liste des membres affichée ou quittée (comptes utilisateur seulement) ; voir « Liste des membres » plus bas | `subscriptions` { `<guild_id>`: { `typing`, `activities`, `channels` } } |

Rien d'autre : pas de `capabilities`, pas de `presence`, pas de `client_state`,
pas d'op 3, 4, 8 ni 14. Tout envoi passe par le limiteur de
`src/net/gateway.rs` (`GwSink`) : 120 commandes par 60 s, places réservées aux
heartbeats. Un futur envoi (présence, souscriptions) n'a qu'à appeler
`GwSink::queue`.

## Écart au plan : `zlib-stream` plutôt que `zstd-stream`

Les deux formats sont documentés par Discord (Gateway › Transport
Compression). F8 utilise `zlib-stream`, décodé par `flate2` sur le moteur
`miniz_oxide` (Rust pur, déjà présent dans l'arbre de dépendances via `png`).
Mesures (profil release, READY synthétiques, `inflate::tests::measure_ready`) :

| READY | En clair | Compressé | Taux | Décompression | Tampon maximal |
|---|---|---|---|---|---|
| 100 serveurs | 1,27 Mo | 0,23 Mo | 18,4 % | 3,7 ms | 2 Mio |
| 400 serveurs | 5,1 Mo | 0,93 Mo | 18,2 % | 13 ms | 8 Mio |
| 1500 serveurs | 19,2 Mo | 3,5 Mo | 18,2 % | 50 ms | 32 Mio |

Pour comparaison, la seule analyse JSON du même READY coûte 1,4 / 4,9 /
18,9 ms. Binaire release : 12 510 208 → 12 670 976 octets (+157 Kio).

Pourquoi ne pas passer à zstd :

- **gain restant borné** : zlib retire déjà 82 % des octets ; même un
  compresseur parfait ne gagnerait au plus que les 18 % restants (3,5 Mo sur
  le plus gros READY mesuré, 0,23 Mo sur un compte courant) ;
- **coût CPU déjà faible** : 2,6 ms par Mo décompressé, hors de la boucle
  pour les gros paquets (`spawn_blocking`) ;
- **dépendance** : la bibliothèque `zstd` lie la bibliothèque C (contraire
  à l'exigence « Rust pur ») ; le décodeur Rust pur (`ruzstd`) serait une
  dépendance nouvelle dont le décodage d'un flux sans fin de trame, vidé
  message par message comme l'envoie Discord, n'a pas été éprouvé ici.
  Taille et débit de `ruzstd` **non mesurés** dans ce lot.

À reconsidérer si une mesure sur de vrais comptes montre un READY ou un
débit d'événements où les 18 % restants comptent.

## Envoi des messages : `nonce` et `enforce_nonce`

Corps de `POST /channels/{id}/messages` : `content`, `tts`: false, `nonce`
(texte, un snowflake construit à l'heure d'envoi, ≤ 25 caractères) et
**nouveau** `enforce_nonce`: true. Ces deux champs sont documentés (Create
Message) : « si `enforce_nonce` vaut vrai et qu'un nonce est présent, son
unicité est vérifiée sur les dernières minutes ; si un autre message du même
auteur a été créé avec le même nonce, ce message est renvoyé et aucun nouveau
n'est créé ». La durée exacte n'est **pas** documentée (« past few
minutes ») : un nouvel essai garde le même nonce et ne crée pas de doublon
s'il suit de près ; un nouvel essai très tardif d'un envoi « non confirmé »
pourrait en créer un.

Issue d'un envoi :

- réponse 2xx : confirmé ;
- échec certain (refus, file pleine, attente au-delà du plafond, connexion
  impossible, blocage) : le texte revient à l'utilisateur au-dessus de la
  saisie, avec « Réessayer » (même nonce) et « Supprimer » ;
- requête peut-être arrivée (délai dépassé, réponse coupée) : « envoi non
  confirmé » jusqu'à ce que la passerelle diffuse un MESSAGE_CREATE de
  l'utilisateur portant ce nonce (confirmé), ou que l'utilisateur réessaie ou
  abandonne. Aucun nouvel essai automatique.

## REST : 429 sans corps de l'API

Un 429 sans corps JSON de l'API ni en-tête `X-RateLimit-*` (pare-feu de
Discord) met **toutes** les requêtes en pause jusqu'à l'en-tête HTTP standard
`Retry-After` (60 s s'il manque), sans nouvel essai automatique ; le message
« Discord limite temporairement cette connexion, nouvel essai à HH:MM »
s'affiche. Les réponses 401, 403 et 429 sont comptées sur 10 minutes
glissantes : à 1000 (dix fois sous la limite documentée de 10 000), plus rien
ne part jusqu'à ce que la plus ancienne sorte de la fenêtre.

## Enregistreur de formes : ce qu'il note désormais

Version **debug** seulement, avec `FERROCORD_RECORD_SHAPES=<dossier>` (voir
`tests/fixtures/README.md` pour la marche à suivre) :

- `<TYPE>.json` : forme du premier paquet reçu de chaque type (inchangé) ;
- **nouveau** `SENT_OP_<n>.json` : forme du premier paquet de chaque opcode
  **envoyé par Ferrocord** (`SENT_OP_2` Identify, `SENT_OP_6` Resume,
  `SENT_OP_1` Heartbeat). Même anonymisation : toute valeur est remplacée
  (`"<str>"`, `"<id>"`, 0, `false`), et la clé `token` elle-même est masquée en
  `"<key>"`. Le jeton n'est jamais écrit (test
  `sent_packets_keep_field_names_but_never_the_token`).

L'enregistreur ne voit que la connexion de Ferrocord. Il ne peut donc pas
capturer ce qu'envoie l'application officielle : pour cela, voir la dernière
section.

## À capturer pour les « capacités modernes »

D'après le comportement observé de clients tiers (non vérifié, comme tout ce
qui suit sur les noms de champs et d'opcodes), le client officiel ajoute à
l'Identify un champ `capabilities` (entier, champ de bits) qui change la forme
du READY et de plusieurs événements. Avant de l'envoyer, il faut connaître :

1. **La valeur exacte et la signification de chaque bit** utilisé par le
   client officiel actuel, et quels bits sont indispensables pour recevoir
   chacune des formes déjà tolérées par `src/net/raw.rs` (`guilds[].properties`,
   `merged_members`, `private_channels[].recipient_ids` + `users`,
   `user_settings_proto`). Un bit inconnu ne doit jamais être allumé.
2. **Les autres champs de l'Identify officiel** que le serveur pourrait exiger
   avec ces capacités : `presence`, `client_state` (et ses sous-champs, par ex.
   versions de serveurs), `properties` complètes. Depuis la décision du 14 novembre 2026,
   Ferrocord envoie par défaut les `properties` du client de bureau (voir
   « Identité annoncée ») ; il reste à savoir si le serveur accepte les
   capacités avec les `properties` du mode Ferrocord.
3. **Le READY obtenu avec ces capacités** (forme seulement) : `READY.json`,
   `READY_SUPPLEMENTAL.json` (présences, `merged_members` complémentaires,
   `lazy_private_channels`), `SESSIONS_REPLACE.json`.
4. **Les événements dont la forme change** avec les capacités :
   `GUILD_CREATE` (format `properties`), `CHANNEL_UNREAD_UPDATE`,
   `PASSIVE_UPDATE_V1` / `PASSIVE_UPDATE_V2` (serveurs « paresseux »),
   `GUILD_MEMBER_UPDATE` d'un compte utilisateur, `USER_SETTINGS_PROTO_UPDATE`,
   `READ_STATE_*` le cas échéant.
5. **Le comportement à la reprise** (op 6) avec capacités : RESUMED rejoue-t-il
   les mêmes formes ?

## À capturer pour les souscriptions aux serveurs (op 14, et op 37)

Sur un compte utilisateur, la liste des membres et certains événements d'un
serveur ne sont envoyés qu'après une souscription. Il faut connaître :

1. **La forme exacte de l'op 14** envoyée par le client officiel à
   l'ouverture d'un salon : `guild_id`, `typing`, `threads`, `activities`,
   `members`, `channels` (plages `[[0, 99]]` par salon ?),
   `thread_member_lists`, et quels champs sont facultatifs.
2. **S'il a été remplacé par l'op 37** (souscriptions groupées par serveur,
   `subscriptions: { <guild_id>: { … } }`) dans le client actuel, et lequel des
   deux le serveur attend.
3. **Le rythme** : combien d'ops 14/37 le client officiel envoie par minute en
   naviguant normalement, quand il en renvoie (défilement de la liste des
   membres, changement de salon), et s'il « désabonne ». Le limiteur garantit
   120 commandes par minute, mais l'objectif est de ressembler à un usage
   humain, très en dessous.
4. **Les réponses** : `GUILD_MEMBER_LIST_UPDATE` avec ses opérations (`SYNC`,
   `INSERT`, `UPDATE`, `DELETE`, `INVALIDATE`), `groups`, `member_count`,
   `online_count`, `id` de liste ; `THREAD_LIST_SYNC`, `THREAD_MEMBER_LIST_UPDATE` ;
   `TYPING_START` et `PRESENCE_UPDATE` reçus après souscription.
5. **Ce qui arrive sans souscription** sur un gros serveur (pour savoir si
   l'on peut s'en passer pour le texte seul).

## Requêtes non documentées : amis, MP de groupe, accusés de lecture (lot S3 + T9)

Autorisées par l'utilisateur le 13 novembre 2026 (PLAN.md 1.2.6). Chacune
part **seulement après un geste de l'utilisateur** (un clic ; retirer un ami,
bloquer, quitter un groupe et retirer un membre demandent en plus « Oui »),
passe par `rest::api` et donc par le limiteur de débit commun, et n'ajoute
**aucun en-tête propre au client officiel** (ni `X-Super-Properties`, ni
`X-Context-Properties`). Code : `src/net/social_rest.rs`, formes vérifiées par
`social_rest::tests::request_shapes` et `tests/e2e/social.rs`.

Sources : [docs.discord.food](https://docs.discord.food) (« Relationships »,
« Channels », « Gateway Events ») et
[discord-userdoccers](https://discord-userdoccers.pages.dev/topics/read-state)
(« Read State », « CAPTCHA Handling »), documentation communautaire de l'API
utilisateur ; les points marqués « officiel » figurent aussi dans la
documentation de Discord.

| Méthode et chemin | Corps | Quand | Source |
|---|---|---|---|
| `POST /channels/{id}/messages/{id}/ack` | `token`: null, `last_viewed` (jours depuis 2015) | salon réellement lu (fenêtre active, bas de la liste visible), au plus un envoi par salon toutes les 3 s (`reads::ACK_DELAY`) ; « Marquer comme lu » | userdoccers, Acknowledge Message |
| `POST /read-states/ack-bulk` | `read_states`: [{ `channel_id`, `message_id`, `read_state_type`: 0 }] (100 au plus) | « Marquer le serveur comme lu » | userdoccers, Bulk Update Read States |
| `POST /channels/{id}/messages/{id − 1}/ack` | `manual`: true, `mention_count` (mentions de moi dans les messages chargés à partir du message choisi) | « Marquer comme non lu » du menu d'un message (lot M) ; l'identifiant est celui qui précède le message, les accusés automatiques du salon sont retenus jusqu'à ce qu'il soit rouvert | docs.discord.food, Read State, Acknowledge Message (`manual`, `mention_count` « Requires manual to be true ») |
| `PATCH /channels/{id}/messages/{id}` | `flags` seul : indicateurs déjà posés + SUPPRESS_EMBEDS (1 << 2) | « Supprimer les intégrations » du menu d'un message (le mien, ou avec « gérer les messages ») | officiel (Edit Message) ; docs.discord.food, Edit Message |
| `POST /users/@me/relationships` | `username` | « Envoyer la demande » | discord.food, Send Friend Request |
| `PUT /users/@me/relationships/{id}` | `{}` | « Accepter » | discord.food, Create Relationship |
| `PUT /users/@me/relationships/{id}` | `type`: 2 | « Bloquer », confirmé | discord.food, Create Relationship |
| `DELETE /users/@me/relationships/{id}` | — | « Retirer » (confirmé), « Refuser », « Annuler », « Débloquer » | discord.food, Remove Relationship |
| `DELETE /users/@me/relationships/{id}/ignore` | — | « Ne plus ignorer » | discord.food, Unignore User |
| `PUT /users/@me/relationships/{id}/ignore` | — | « Ignorer » du menu d'un utilisateur (lot U) | discord.food, Relationships, Ignore User |
| `PATCH /users/@me/relationships/{id}` | `nickname` (1 à 32 caractères, `null` pour le retirer ; amis seulement) | « Enregistrer » de la fenêtre « Pseudo d'ami » (lot U) | discord.food, Relationships, Modify Relationship |
| `PUT /channels/{id}/recipients/@me` | `consent_status`: 2 (ACCEPTED) | « Accepter » une demande de message (lot U) ; la réponse (le MP) est relue | discord.food, Channels, Update Message Request |
| `DELETE /channels/{id}/recipients/@me` | — | « Ignorer » une demande de message (lot U) ; le MP disparaît par CHANNEL_DELETE | discord.food, Channels, Reject Message Request |
| `POST /users/@me/channels` | `recipients`: [id] | « Message » sur un ami sans MP ouvert | officiel (Create DM) ; `recipients` : discord.food |
| `POST /users/@me/channels` | `recipients`: [ids] (moi en plus s'il n'y a qu'un ami) | « Créer le groupe » | discord.food, Create Private Channel |
| `PUT /channels/{id}/recipients/{id}` | — | « Ajouter » (panneau du groupe) | officiel (Group DM Add Recipient) |
| `DELETE /channels/{id}/recipients/{id}` | — | « Retirer du groupe », confirmé (propriétaire seulement) | officiel (Group DM Remove Recipient) |
| `PATCH /channels/{id}` | `name` | « Renommer » | officiel (Modify Channel, Group DM) |
| `DELETE /channels/{id}` | — | « Quitter le groupe » (confirmé), « Fermer la conversation » | officiel (Delete/Close Channel) |

Événements lus (aucun n'est demandé : tout vient du READY et de la
passerelle) : `relationships`, `read_state` (tableau ou structure versionnée
`entries`), `private_channels[].owner_id` / `recipients` / `recipient_ids`,
`users`, `guilds[].channels[].last_message_id` du READY (relus à part, seuls
ces champs sont décodés) ; `READY_SUPPLEMENTAL.merged_presences.friends` ;
`PRESENCE_UPDATE` sans `guild_id` ; `RELATIONSHIP_ADD` / `_UPDATE` /
`_REMOVE` ; `MESSAGE_ACK` (type 0 seulement) ; `CHANNEL_RECIPIENT_ADD` /
`_REMOVE` ; `private_channels[].is_message_request` et `is_spam` (READY,
CHANNEL_CREATE et CHANNEL_UPDATE : dossier « Demandes de message », lot U ;
absents : conversation ordinaire). Jeux d'essai : `tests/fixtures/ready_user_social.json`,
`ready_supplemental.json`, `relationship_*.json`, `message_ack.json`,
`presence_update_friend.json`, `channel_recipient_*.json`.

**Captcha** : une réponse d'erreur portant `captcha_key` n'est jamais
contournée ni retentée. L'utilisateur lit « Discord demande une vérification
(captcha) : faites cette action dans l'application officielle. » (en pratique,
surtout l'envoi d'une demande d'ami par nom d'utilisateur).

**Non vérifié sans le vrai compte** : la forme exacte du READY et du
`read_state` reçus par Ferrocord (sans capacités, le tableau simple est
attendu ; les deux formes sont lues) ; le fait que Discord accepte ces
écritures sans `X-Super-Properties` ni `X-Context-Properties` ; le calcul des
non-lus pour un salon **sans** état de lecture (tenu pour lu ici).

## Requêtes non documentées : sondages, invitations, adresses expirées (lot T13)

Toutes passent par le limiteur commun, sans `X-Super-Properties`, et ne
partent qu'après un geste (ou, pour les adresses, l'échec d'une image
affichée). Formes vérifiées sur docs.discord.food, jeux d'essai synthétiques
et tests de bout en bout (`tests/e2e/polls.rs`).

| Méthode et chemin | Corps | Quand | Source |
|---|---|---|---|
| `PUT /channels/{id}/polls/{id}/answers/@me` | `answer_ids`: [entiers] (vide : retrait) ; réponse 204 | clic sur une réponse (choix unique), « Voter » (choix multiples), « Retirer mon vote » ; un vote en cours à la fois par sondage | discord.food, Create Poll Vote |
| `POST /invites/{code}` | `{}` | « Rejoindre » d'une carte d'invitation (serveur seulement, pas si j'en suis déjà membre) ; envoyé par `Store::join_invite` du lot serveurs (même requête, voir plus bas) ; captcha : jamais contourné ni retenté, la carte renvoie à l'application officielle | discord.food, Accept Invite |
| `POST /attachments/refresh-urls` | `attachment_urls`: [adresses] (50 au plus) ; réponse `refreshed_urls`: [{ `original`, `refreshed` }] | image de pièce jointe en échec dont la signature `ex=` est passée : une demande groupée, une seule fois par adresse (`state::announce`), puis un seul nouvel essai de l'image | discord.food, Refresh Attachment URLs |

Requêtes documentées du même lot : `GET /channels/{id}/polls/{id}/answers/{id}?limit=25`
(votants), `POST /channels/{id}/polls/{id}/expire` (« Terminer le sondage »,
auteur seulement), `POST /channels/{id}/messages` avec `poll` (création),
`GET /invites/{code}?with_counts=true` (une fois par code et par session,
cache borné à 128), `POST /channels/{id}/messages/{id}/crosspost`
(« Publier »), `POST /channels/{id}/followers` (« Suivre »,
`webhook_channel_id`). Paquets lus : `MESSAGE_POLL_VOTE_ADD` / `_REMOVE`,
`poll`, `flags` (CROSSPOSTED, IS_CROSSPOST) et `message_reference` des
messages, embed `poll_result` du type 46.

**Non vérifié sans le vrai compte** : l'acceptation de ces trois requêtes
sans propriétés du client officiel ; la fréquence réelle des captchas sur
`POST /invites/{code}`.

## Comment capturer sans rien divulguer

1. Les formes **reçues par Ferrocord** : enregistreur actuel (`<TYPE>.json`).
2. Les formes **envoyées par Ferrocord** : enregistreur, fichiers
   `SENT_OP_<n>.json`.
3. Ce qu'envoie **le client officiel** (capacités, op 14/37) : seul l'utilisateur
   peut l'observer, dans le client web officiel (outils de développement du
   navigateur, onglet Réseau, connexion WebSocket de la passerelle, messages
   **envoyés**). Les trames contiennent le jeton et des identifiants : ne jamais
   les transmettre telles quelles. Il faudra un petit outil hors ligne
   réutilisant `shapes::anonymize_sent` (à écrire dans un lot suivant) qui lit
   une trame copiée et n'en garde que la forme ; à défaut, ne noter à la main
   que les noms de champs, les types et, pour `capabilities`, la valeur entière
   (qui n'identifie pas le compte).
4. Vérifier chaque fichier avant de le transmettre, comme indiqué dans
   `tests/fixtures/README.md`.

Une fois ces formes obtenues : les ajouter à `tests/fixtures/` (données
synthétiques de même forme), écrire les tests sur ces captures, et seulement
ensuite envoyer quoi que ce soit (règle 1.1.6).

## Liste des membres (lot S1) : op 37, requête non documentée

Envoi autorisé par l'utilisateur le 13 novembre 2026 (PLAN.md 1.2.6). Aucune
capture du client officiel n'a été faite : la forme vient de sources
publiques de la communauté, vérifiées le 13 novembre 2026.

### Sources

- **discord-userdoccers** (docs.discord.food), `pages/gateway/opcodes-and-close-codes.mdx` :
  op 14 « Guild Subscriptions **(deprecated)** », op 37 « Guild Subscriptions
  Bulk — Update subscriptions for multiple guilds ». La page des événements ne
  décrit ni le contenu de l'op 37 ni GUILD_MEMBER_LIST_UPDATE (ticket ouvert
  discord-userdoccers#461, qui donne la forme de l'événement et des opérations
  `SYNC`, `UPDATE`, `INSERT`, `DELETE`, et indique que la liste arrive après
  une op 37 avec `activities: true`).
- **discord.py-self** (bibliothèque publique, `discord/gateway.py`,
  `discord/types/gateway.py`, `discord/state.py`) : `BULK_GUILD_SUBSCRIBE = 37`,
  paquet `{"op": 37, "d": {"subscriptions": {"<guild_id>": {…}}}}` ; champs
  `typing`, `threads`, `activities`, `member_updates` (booléens), `members`,
  `channels` (`{"<channel_id>": [[début, fin], …]}`), `thread_member_lists` ;
  « seules les clés présentes sont mises à jour » ; un serveur n'est
  souscrit qu'avec `typing: true` ; `channels: {}` quitte les listes ;
  plages de 100 ; types de `GUILD_MEMBER_LIST_UPDATE` (`id`, `guild_id`,
  `member_count`, `online_count`, `groups[{id, count}]`, `ops`, avec
  `INVALIDATE` + `range`) ; regroupement des demandes sur 0,5 s.
- **discord-unofficial-docs**, « lazy_guilds » (arandomnewaccount.gitlab.io) :
  le client officiel demande toujours `[0, 99]`, plus deux autres plages au
  plus ; les titres de groupe occupent un index ; les opérations s'appliquent
  dans l'ordre reçu.

### Ce que Ferrocord envoie

```json
{"op": 37, "d": {"subscriptions": {"<guild_id>": {"typing": true, "activities": true, "channels": {"<channel_id>": [[0, 99]]}}}}}
```

- `typing` et `activities` seulement la première fois pour un serveur dans
  une session ; ensuite seulement `channels` (ce qui change) ;
- `channels` : le salon ouvert, `[0, 99]` puis les plages de 100 qui couvrent
  les lignes visibles, trois au plus (`state::members::ranges_for`) ;
- en changeant de serveur, l'ancien reçoit `{"channels": {}}` dans le même
  paquet ; liste masquée, messages privés : `{"channels": {}}` pour le serveur
  quitté, une fois ; jamais rien si aucune liste n'a été affichée ;
- pas de `threads`, `member_updates`, `members` ni `thread_member_lists`.

### Quand, et à quel rythme

- seulement sur un geste : ouverture d'un salon avec la liste affichée,
  défilement de la liste, bouton ou réglage qui l'affiche ou la masque ;
- `net::members::Pacer` : le paquet part 250 ms après la demande (les
  demandes d'un défilement sont regroupées), et au moins 1,5 s après le
  précédent, soit **40 par minute au plus** en faisant défiler sans arrêt
  (le limiteur `GwSink` en laisse passer 120) ; une demande identique à la
  précédente n'envoie rien ;
- après un nouvel Identify (pas après une reprise), la dernière souscription
  est renvoyée une fois ;
- **comptes bot : jamais** (`Event::MemberListUnavailable`, le panneau le dit).

### Ce qui est reçu et gardé

GUILD_MEMBER_LIST_UPDATE (`net::members::decode`, jeux d'essai
`guild_member_list_update_sync.json` et `guild_member_list_update_ops.json`,
synthétiques) : seules les lignes des plages souscrites restent en mémoire,
quatre listes au plus. Un élément illisible garde sa place (ligne en
attente) ; une opération illisible vide la liste plutôt que de la décaler.
Une mise à jour qui ne touche pas les lignes affichées ne redessine pas
l'interface (`state::members::ListWatch`), au plus 256 paquets de suite.

### `capabilities` : pas envoyé

Aucune des trois sources ne lie la liste des membres à un bit de
`capabilities` : la liste de discord-userdoccers (« Gateway Capabilities »,
bits 0 à 16) ne mentionne ni GUILD_MEMBER_LIST_UPDATE ni l'op 37. Ferrocord
garde donc un Identify sans `capabilities`. **Non vérifié** : que le
serveur accepte l'op 37 et envoie la liste à une session ainsi identifiée.
Si la liste n'arrive jamais sur un vrai compte, c'est la première hypothèse
à examiner ; il faudrait alors une capture. (Depuis le 14 novembre 2026, les
`properties` sont par défaut celles du client de bureau : voir « Identité
annoncée ».)

### Avec `large_threshold: 50`

discord-userdoccers : pour un compte utilisateur, au-delà de
`large_threshold` membres, le serveur « cesse d'envoyer les événements non
essentiels des serveurs sans souscription ». discord.py-self observe au
contraire une souscription automatique des serveurs de moins de 75 000
membres. Non tranché ici ; la souscription du serveur ouvert (`typing: true`)
couvre ce cas pour le serveur affiché.
## Réglages du compte (lot C1 + S4 + C4, fenêtre Paramètres)

Requêtes réservées aux comptes utilisateur, autorisées par l'utilisateur le
13 novembre 2026 (PLAN.md 1.2.6). Toutes passent par le limiteur de débit
REST (`net/rest.rs`, `api`), avec 10 s d'attente au plus avant de partir, et
ne partent que d'un geste : ouvrir la page qui affiche la donnée (lecture,
une fois), ou un clic (écriture). Aucune n'est répétée automatiquement.
Forme vérifiée le 13 novembre 2026 sur docs.discord.food (documentation
communautaire de l'API utilisateur) et, pour les numéros de champ protobuf,
sur `discord-userdoccers/discord-protos`
(`discord_protos/discord_users/v1/PreloadedUserSettings.proto`). Jeux
d'essai : `tests/fixtures/rest_*.json`, `user_guild_settings.json`,
`user_settings_proto_update.json` ; tests : `net::account::tests`,
`state::usersettings::*::tests`, `tests/e2e/settings.rs`.

### Reçu de la passerelle

| Quoi | Où | Usage |
|---|---|---|
| `user_settings_proto` (base64) | READY | réglages synchronisés (type 1, `PreloadedUserSettings`) |
| `user_guild_settings` (`entries`, `partial`, `version` ; ou tableau) | READY | muet et notifications par serveur et salon |
| `USER_SETTINGS_PROTO_UPDATE` (`settings.type`, `settings.proto`, `partial`) | événement | type 1 seulement ; `partial` : seuls les champs de premier niveau présents sont remplacés |
| `USER_GUILD_SETTINGS_UPDATE` | événement | réglages d'un serveur, remplacés en entier |

### Envoyé (REST)

| Méthode et chemin | Quand | Corps | Documenté |
|---|---|---|---|
| `GET /users/@me/settings-proto/1` | « Les lire depuis Discord » (READY sans réglages ou illisibles) | — | non (docs.discord.food, User Settings Proto) |
| `PATCH /users/@me/settings-proto/1` | clic sur un réglage synchronisé | `settings` : base64 d'un message ne contenant **que** le champ de premier niveau modifié, en entier, champs inconnus compris ; `required_data_version` : `versions.data_version` connu | non (idem) |
| `PATCH /users/@me/guilds/{id}/settings` (`@me` pour les MP) | menu du clic droit (muet, niveau, @everyone, rôles) ; clic sur une catégorie, « Replier toutes les catégories », « Masquer les salons muets » (phase 10, lot S) | un seul réglage : `muted` + `mute_config` (`end_time`, `selected_time_window`), `message_notifications`, `suppress_everyone`, `suppress_roles`, `hide_muted_channels`, ou `channel_overrides` : { id : { … } } (pour une catégorie repliée : { id : { `collapsed` } }, plusieurs catégories dans la même requête) | non (docs.discord.food, Modify User Guild Settings ; `hide_muted_channels` : User Guild Settings Structure ; `collapsed` : Channel Override Structure, https://docs.discord.food/resources/user-settings) |
| `GET /users/@me` | ouverture de « Mon compte » ou « Profils » | — | oui (Get Current User) ; champs privés (`email`, `phone`, `mfa_enabled`…) : docs.discord.food |
| `PATCH /users/@me` | « Enregistrer » du profil, si le nom d'affichage a changé | `global_name` seulement | `global_name` : non (docs.discord.food, Modify Current User) |
| `PATCH /users/@me/profile` | « Enregistrer » du profil (page Profils ou édition rapide), si bio, pronoms, couleur ou couleurs du thème ont changé | `bio`, `pronouns`, `accent_color`, `theme_colors` : [principale, secondaire] ou `null` (seulement ceux qui ont changé ; `theme_colors` : Nitro exigé par Discord) | non (docs.discord.food, Modify User Profile) |
| `PATCH /guilds/{id}/members/@me` | « Enregistrer » du pseudo sur un serveur | `nick` | oui (Modify Current Member) |
| `PATCH /guilds/{id}/profile/@me` | « Enregistrer » de l'édition rapide, onglet « Profil de ce serveur », si bio, pronoms ou couleurs du thème de ce serveur ont changé (lot « profil rapide ») | `bio`, `pronouns`, `theme_colors` (seulement ceux qui ont changé ; `bio` et `theme_colors` : Nitro exigé par Discord) | non (docs.discord.food, Modify Guild Member Profile) ; jeu `rest_profile_errors.json`, tests `net::account::tests::rest_bodies` et `e2e quick_profile` |
| `GET /auth/sessions` | ouverture de « Appareils », « Actualiser » | — | non (docs.discord.food, Get Auth Sessions) |
| `POST /auth/sessions/logout` | « Déconnecter » confirmé (deuxième clic) | `session_id_hashes` : [un seul] | non (docs.discord.food, Logout Auth Sessions) |
| `GET /users/@me/connections` | ouverture de « Comptes liés » | — | oui (Get Current User Connections) |
| `GET /oauth2/tokens` | ouverture de « Applications autorisées » | — | non (docs.discord.food, Get OAuth2 Tokens) |
| `DELETE /oauth2/tokens/{id}` | « Révoquer » confirmé (deuxième clic) | — | non (docs.discord.food, Delete OAuth2 Token) |
| `GET /users/@me/consent` | ouverture de « Données et confidentialité » | — | non (docs.discord.food, User Settings, Get User Consents) |
| `POST /users/@me/consent` | clic sur « Statistiques d'utilisation » ou « Personnalisation » | `grant` : [un type] et `revoke` : [], ou l'inverse | non (docs.discord.food, Modify User Consents) |
| `PATCH /users/@me` | « Envoyer » ou « Retirer » de l'avatar ou de la bannière du compte | `avatar` ou `banner` seul : `data:image/png;base64,…` (PNG 512 × 512 ou 600 × 240 recadré par Ferrocord), ou `null` | oui (Modify Current User) ; la réponse peut contenir un champ `token` : elle n'est pas lue |
| `PATCH /guilds/{id}/members/@me` | idem, pour le profil d'un serveur (Nitro exigé par Discord) | `avatar` ou `banner` seul | non pour ces champs (docs.discord.food, Modify Current Guild Member) |

Refus d'une écriture du profil (lot « profil rapide ») : les réponses 4xx des
routes ci-dessus (`/users/@me`, `/users/@me/profile`, `/guilds/{id}/members/@me`,
`/guilds/{id}/profile/@me`) sont lues au format d'erreur documenté par Discord
(`code`, `message`, `errors.<champ>._errors[].code`) pour dire le refus dans la
langue de l'interface : image trop lourde (413, codes 40005, 50045, 50138, ou
champ `avatar`/`banner` trop long), changement trop fréquent du nom ou d'une
image (code de champ contenant `RATE_LIMIT`), réservé à Nitro (code de champ
contenant `PREMIUM`). Tout autre refus garde le message de Discord. Les codes
de champ ne sont pas documentés un à un : la reconnaissance est tolérante et
vérifiée sur le jeu synthétique `rest_profile_errors.json`
(`net::account::tests::profile_refusals_are_recognised`). Rien n'est relancé
après un refus.

Garde-fous :

- réglages synchronisés absents, illisibles ou mis à jour partiellement sans
  base connue : affichés comme inconnus, **jamais réécrits** ;
- une seule écriture en cours par réglage (le message protobuf entier, un
  serveur, le profil, le pseudo) : un deuxième geste est refusé avec un
  message ; chaque écriture est optimiste et rétablie à l'identique (octet
  pour octet pour le protobuf) si Discord la refuse ou répond `out_of_date` ;
- un 401 portant le code 60003 (vérification A2F exigée) n'est pas pris pour
  un jeton mort : l'action est refusée avec le message commun de l'A2F (chemin
  unique avec le 403 de même code, `Problem::MfaRequired`) ;
- rien de ce qui exige le mot de passe, un code A2F, un captcha ou un
  paiement n'est implémenté (changer d'e-mail, de mot de passe, de nom
  d'utilisateur, A2F, suppression du compte, Nitro, facturation, cadeaux,
  lier un compte) : la page propose « Ouvrir dans
  Discord » (`https://discord.com/app`, navigateur).

Non vérifié sur un vrai compte (aucun agent ne s'y connecte) : réponses
réelles de ces points d'API, prise en compte du statut (`status.status`) par
la présence de la session de Ferrocord (aucun op 3 n'est envoyé), forme
réelle d'un `PATCH settings-proto` réussi (le code accepte une réponse avec
ou sans `settings`).

Champs protobuf ajoutés en vague 3 (même source, `PreloadedUserSettings.proto`) :
`appearance.developer_mode` (13.2, booléen), `privacy.allow_accessibility_detection`
(8.7, booléen), `privacy.allow_activity_party_privacy_friends` et
`…_voice_channel` (8.1 et 8.2, `BoolValue`, absent = vrai : supposé),
`privacy.default_guilds_activity_restricted` (8.14, énumération 0 à 2). Non
vérifié : Discord lit peut-être désormais `default_guilds_activity_restricted_v2`
(8.29) à la place de 8.14.

## Fils de discussion et forums (lot T10)

Toutes les requêtes de ce lot sont **documentées** par Discord (Channels
Resource) ; aucune route réservée aux comptes utilisateur n'est envoyée.
Chacune suit un clic, passe par `rest::api` (limiteur commun, 15 s
d'attente au plus) et n'est jamais répétée automatiquement. Code :
`src/net/threads.rs` (formes vérifiées par `net::threads::tests` et
`tests/e2e/threads.rs`, jeux d'essai synthétiques `thread_*.json`,
`guild_create_forum.json`, `threads_archived_public.json`).

| Méthode et chemin | Corps | Quand |
|---|---|---|
| `PUT /channels/{fil}/thread-members/@me` | — | « Rejoindre » (en-tête du fil) |
| `DELETE /channels/{fil}/thread-members/@me` | — | « Quitter le fil » |
| `PATCH /channels/{fil}/thread-members/@me/settings` | `flags` (un seul bit : 2 tous les messages, 4 mentions, 8 rien) | menu « Notifications » de l'en-tête d'un fil dont je suis membre (phase 10, lot N). **Non documentée par Discord** : forme vérifiée sur docs.discord.food, « Modify Thread Settings » et « Thread Member Flags » (https://docs.discord.food/resources/channel). Réponse : le membre du fil, ou 204 si rien n'a changé. Les drapeaux reçus (`member` d'un fil, `members` de THREAD_LIST_SYNC, THREAD_MEMBER_UPDATE) donnent le niveau affiché ; sans bit choisi, le niveau du serveur s'applique. |
| `PATCH /channels/{fil}` | `archived` | « Archiver » / « Désarchiver » (créateur, ou MANAGE_THREADS) |
| `POST /channels/{salon}/messages/{message}/threads` | `name` | « Créer un fil » (menu ou barre d'un message) |
| `POST /channels/{forum}/threads` | `name`, `message.content`, `applied_tags` | « Publier » (vue forum) |
| `GET /channels/{salon}/threads/archived/public?limit=25&before=…` | — | « Charger les publications archivées » (vue forum) |

Reçu : `threads` de GUILD_CREATE (et des serveurs du READY), THREAD_CREATE,
THREAD_UPDATE, THREAD_DELETE, THREAD_LIST_SYNC, THREAD_MEMBER_UPDATE,
THREAD_MEMBERS_UPDATE, `available_tags` et `default_reaction_emoji` des
salons forum (15) et média (16), champ `thread` des messages.

**Pas fait, faute de source vérifiable pendant le lot** : l'extrait et la
vignette du premier message d'une publication (le client officiel les
obtient par une route non documentée de recherche des fils d'un forum, dont
la forme n'a pas pu être vérifiée sur docs.discord.food : accès au web
indisponible pendant le lot) ; `threads: true` dans l'op 37 (même raison :
non envoyé, l'op 37 de Ferrocord reste celle du lot S1).

**Non vérifié sans le vrai compte** : que GUILD_CREATE d'un compte
utilisateur contienne bien les fils actifs et leur champ `member` sans
souscription ; que les fils d'un serveur non souscrit arrivent par
THREAD_LIST_SYNC.

## Profil complet et notes (lot profil-complet)

Requêtes réservées aux comptes utilisateur, autorisées par l'utilisateur le
13 novembre 2026 (PLAN.md 1.2.5). Forme vérifiée le 13 novembre 2026 sur
[docs.discord.food](https://docs.discord.food) (« Users » : Get User Profile,
Get User Note, Modify User Note ; « Reference » : CDN Formatting). Toutes
passent par `rest::api` (limiteur commun, 10 s d'attente au plus), sans
`X-Super-Properties` ni `X-Context-Properties`. Code : `src/net/profile.rs`,
`src/state/profiles.rs` ; jeux d'essai synthétiques
`tests/fixtures/rest_user_profile.json` et `rest_user_note.json` ; tests :
`net::profile::tests`, `state::profiles::tests`, `ui::profile::tests`,
`tests/e2e/profiles.rs`.

| Méthode et chemin | Corps | Quand | Documenté |
|---|---|---|---|
| `GET /users/{id}/profile?with_mutual_guilds=true&with_mutual_friends=true&with_mutual_friends_count=true[&guild_id={id}]` | — | ouverture d'une carte ou du profil complet ; une fois par utilisateur et par serveur, redemandé seulement si la carte est rouverte plus de 5 min après (`PROFILE_TTL_MS`) | non (docs.discord.food, Get User Profile) |
| `GET /users/@me/notes/{id}` | — | ouverture d'une carte, une fois par session ; 404 : pas de note | non (docs.discord.food, Get User Note) |
| `PUT /users/@me/notes/{id}` | `note` (256 caractères au plus) | sortie du champ de note (ou fermeture de la carte) si le texte a changé ; une écriture à la fois | non (docs.discord.food, Modify User Note) |

Ce qui remplace : la carte ne demande plus `GET /users/{id}` ni
`GET /guilds/{id}/members/{id}` (documentés) que **si le profil échoue**
(403 sans serveur commun, 404, erreur) et pour les **comptes bot**, qui
n'appellent jamais les trois points ci-dessus.

Erreurs : 403, 404 ou réponse illisible, sans message (la carte montre ce
qu'elle sait) ; captcha : jamais retenté, la carte dit « ouvrez ce profil
dans l'application officielle » ; 401 : comme partout, retour à l'écran de
connexion.

Images (CDN, sans jeton) : `badge-icons/{icon}.png` (icônes des badges,
PNG seulement) et `guilds/{guild}/users/{user}/banners/{hash}` (bannière de
serveur). Liens des comptes liés : adresses publiques des services
(`github.com/{nom}`, `steamcommunity.com/profiles/{id}`…), construites
seulement à partir de noms faits de lettres, chiffres, `.`, `_`, `-`
(`state::profiles::Connection::url`) ; ces modèles ne viennent pas de
Discord et ne sont pas documentés par lui ; l'adresse est montrée au survol.

Reçu de la passerelle, sans requête : les `presence` des membres de
GUILD_MEMBER_LIST_UPDATE alimentent aussi le magasin des présences
(`state::people`), comme PRESENCE_UPDATE.

**Non vérifié sans le vrai compte** : la réponse réelle de Get User Profile
(champs présents pour un profil privé, `mutual_friends` quand
`with_mutual_friends` est demandé), le code d'erreur rendu par Get User Note
sans note (404 supposé), l'acceptation de ces requêtes sans en-têtes du
client officiel.

## Serveurs : dossiers, rejoindre, quitter, créer, invitations (lot S7 + A1)

Chaque requête suit un clic (bouton « + » du rail, menu du clic droit d'un
serveur, fenêtres « Ajouter un serveur », « Inviter des personnes »,
« Invitations »), passe par `rest::api` (limiteur commun, 15 s d'attente au
plus), n'est jamais répétée automatiquement et part sans
`X-Super-Properties` ni `X-Context-Properties`. Une seule action de chaque
sorte à la fois. Code : `src/net/guilds.rs`, `src/state/servers.rs`,
`src/state/guild_folders.rs` ; jeux d'essai synthétiques
`rest_invite_accept.json`, `rest_guild_create.json`,
`rest_guild_template.json`, `rest_guild_invites.json` ; tests :
`net::guilds::tests`, `state::servers::tests`, `state::guild_folders::tests`,
`ui::rail_dnd::tests`, `tests/e2e/servers.rs`. Formes vérifiées le
13 novembre 2026.

| Méthode et chemin | Corps | Quand | Documenté |
|---|---|---|---|
| `POST /invites/{code}` | `{}` (`session_id`, facultatif, non envoyé) | « Rejoindre » (lien ou code saisi) ; réutilisable par les cartes d'invitation des messages : `Store::join_invite` | non (docs.discord.food, « Invites » › Accept Invite) |
| `DELETE /users/@me/guilds/{id}` | — | « Quitter le serveur », après confirmation ; jamais proposé au propriétaire | oui (Leave Guild) |
| `POST /guilds` | `name` (2 à 100 caractères), `icon` facultatif (PNG 128 × 128 en `data:`) | « Créer le mien » | non pour les comptes utilisateur (docs.discord.food, « Guilds » › Create Guild) |
| `GET /guilds/templates/{code}` | — | « Aperçu » d'un modèle (lien `discord.new/…` ou code) | oui (Get Guild Template) |
| `POST /guilds/templates/{code}` | `name`, `icon` facultatif | « Créer » depuis le modèle prévisualisé | non pour les comptes utilisateur (docs.discord.food, « Guild Templates » › Use Guild Template) |
| `POST /channels/{id}/invites` | `max_age` (0 à 604 800 s), `max_uses` (0 à 100), `temporary` | « Générer un lien » | oui (Create Channel Invite) |
| `GET /guilds/{id}/invites` | — | ouverture de « Invitations » (permission « Gérer le serveur » ; inconnue : proposé, Discord répond) et « Actualiser » | oui (Get Guild Invites) |
| `DELETE /invites/{code}` | — | « Révoquer » puis « Confirmer » | oui (Delete Invite) |
| `PATCH /users/@me/settings-proto/1` | champ 14 `guild_folders` seul, en entier | dépôt d'un glisser-déposer sur le rail, « Paramètres du dossier », « Retirer le dossier » | non (même écriture que les autres réglages synchronisés, voir plus haut) |

Les codes saisis ne sont acceptés que faits de lettres, chiffres et `-`
(2 à 64), et seulement derrière `discord.gg/`, `discord.com/invite/` (ou
`discord.new/`, `discord.com/template/` pour un modèle) : rien d'autre
n'entre dans le chemin.

Dossiers (`PreloadedUserSettings.guild_folders`, forme de
`discord-userdoccers/discord-protos`) : chaque entrée garde ses champs
inconnus ; les serveurs que le magasin ne connaît pas (indisponibles)
restent dans leur dossier ; `guild_positions`, s'il est présent, suit le
nouvel ordre ; un dossier créé reçoit un identifiant positif sur 31 bits,
libre parmi les dossiers. Rien n'est écrit pendant le glissement, ni si le
dépôt ne change rien ; réglages inconnus ou illisibles : rien n'est écrit,
le rail garde l'ordre du magasin.

Erreurs : captcha (`captcha_key`) jamais retenté ni contourné, le message
renvoie à l'application officielle ; `show_verification_form` vrai : le
serveur est rejoint et le message dit de remplir le formulaire d'adhésion
dans l'application officielle ; 404 : « invitation ou modèle invalide,
expiré ou épuisé » ; réponse 204 vide de Create Channel Invite (documentée :
invitations suspendues par Discord) : annoncée telle quelle. Un compte bot
n'envoie ni Accept Invite ni Create Guild.

Découverte de serveurs : les routes de la découverte sont réservées au
client officiel ; « Découvrir des serveurs » ouvre
`https://discord.com/discovery` dans le navigateur, sans rien envoyer.

**Non vérifié sans le vrai compte** : la réponse réelle d'Accept Invite
(champ `guild` présent, `show_verification_form`), l'acceptation de
Create Guild par un compte utilisateur sans en-têtes du client officiel,
le comportement de Discord si un captcha est demandé à la création, la
prise en compte de `guild_positions` par le client officiel après une
écriture de Ferrocord.
## Recherche de messages (lot T12)

Routes réservées aux comptes utilisateur. Forme vérifiée le 13 novembre 2026
sur [docs.discord.food](https://docs.discord.food) (« Messages » : Search
Messages). Chaque requête suit un geste (Entrée dans le champ de recherche,
changement de tri, page suivante ou précédente) et passe par `rest::api`
(limiteur commun, 10 s d'attente au plus), sans `X-Super-Properties`. Les
**comptes bot** n'envoient rien (le panneau affiche une erreur « interdit »). Code : `src/net/search.rs`, `src/state/search.rs`,
`src/state/store_search.rs`, `src/ui/search.rs` ; jeux d'essai synthétiques
`tests/fixtures/rest_search_messages.json` et `rest_search_indexing.json` ;
tests : `net::search::tests`, `state::search::tests`, `ui::search::tests`,
`tests/e2e/search.rs`.

| Méthode et chemin | Quand | Documenté |
|---|---|---|
| `GET /guilds/{id}/messages/search?content=…&author_id=…&mentions=…&has=…&channel_id=…&author_type=…&pinned=…&min_id=…&max_id=…&sort_by=…&sort_order=…&offset=…&limit=25` | recherche dans un serveur | non (docs.discord.food, Search Messages) |
| `GET /channels/{id}/messages/search?…` (mêmes paramètres, sans `channel_id`) | recherche dans un MP ou un groupe | non (docs.discord.food, Search Messages) |
| `GET /channels/{id}/messages?around={message}&limit=50` | « Aller au message » (résultat, extrait de réponse, lien `discord.com/channels/…`) quand le message n'est pas en mémoire | oui |

Paramètres multiples : clés répétées (`has=link&has=file`). `offset` est
borné à 9975 (limite de l'API), `content` à 1024 caractères.

Index en cours (réponse 202, `code` 110000, `retry_after`) : **une seule**
nouvelle tentative après le délai indiqué (2 s si absent ou nul, 15 s au
plus), jamais de boucle ; si l'index n'est toujours pas prêt, le panneau le
dit et l'utilisateur relance lui-même.

Historique local hors ligne : **pas fait** (il faudrait une base locale,
dépendance non justifiée pour ce lot).

**Non vérifié sans le vrai compte** : l'encodage réel attendu pour les
paramètres multiples, la réponse 202 réelle, le format courant de `messages`
(tableaux d'un message, ou contexte avec `hit` — les deux sont lus),
l'acceptation de ces requêtes sans en-têtes du client officiel.

## Boîte de réception : mentions récentes (lot navigation)

Requêtes réservées aux comptes utilisateur, non documentées par Discord.
Forme vérifiée le 13 novembre 2026 sur
[docs.discord.food](https://docs.discord.food) (« User » : List Recent
Mentions, Delete Recent Mention). Elles passent par `rest::api` (limiteur
commun, 10 s d'attente au plus), sans `X-Super-Properties` ni
`X-Context-Properties`. Un compte bot n'envoie rien (l'onglet affiche un
refus). Code : `src/net/inbox.rs`, `src/state/inbox.rs`, `src/ui/inbox.rs` ;
jeu d'essai synthétique `tests/fixtures/rest_recent_mentions.json` ; tests :
`net::inbox::tests`, `state::inbox::tests`, `ui::inbox::tests`,
`tests/e2e/inbox.rs`.

| Méthode et chemin | Corps | Quand | Documenté |
|---|---|---|---|
| `GET /users/@me/mentions?limit=25&roles={bool}&everyone={bool}[&guild_id={id}][&before={id}]` | — | ouverture de l'onglet « Mentions » (une fois, tant que la liste est gardée), « Charger plus » (250 mentions gardées au plus), changement d'un filtre, « Réessayer » ; une demande à la fois | non (docs.discord.food, List Recent Mentions) |
| `DELETE /users/@me/mentions/{message.id}` | — | croix d'une mention ; une fois par mention ; 404 : déjà retirée, la liste suit | non (docs.discord.food, Delete Recent Mention) |

Rien n'est demandé au démarrage ni périodiquement. L'onglet « Non-lus » est
local (états de lecture déjà reçus) et son « Marquer comme lu » réutilise
l'accusé de lecture existant (`POST /channels/{id}/messages/{id}/ack`, voir
plus haut).

Non géré : l'évènement de passerelle `RECENT_MENTION_DELETE` (retrait fait
depuis un autre appareil) ; la liste se met à jour à la réouverture.

**Non vérifié sans le vrai compte** : la réponse réelle (présence de
`guild_id` dans les messages, mentions de plus de 7 jours), le code renvoyé
par Delete Recent Mention pour une mention déjà retirée (404 supposé),
l'acceptation de ces requêtes sans en-têtes du client officiel.

## Commandes d'applications et composants (lot T11)

Requêtes non documentées par Discord (réservées aux comptes utilisateur ;
un compte bot n'en envoie aucune). Formes vérifiées le 13 novembre 2026
sur docs.discord.food (pages « Application Commands », « Receiving and
Responding », « Gateway Events », « Message Components », « Message ») ;
jeux synthétiques dans `tests/fixtures/` (`rest_application_command_index`,
`interaction_*`, `message_create_components*`), tests dans
`src/net/interactions.rs` et `tests/e2e/interactions.rs`. Toutes passent par
le limiteur REST commun.

### Ce que Ferrocord envoie

| Requête | Corps | Quand | Documentée |
|---|---|---|---|
| `GET /guilds/{id}/application-command-index` | — | « / » tapé au début de la saisie, ou menu « Applications » ouvert, dans un salon de serveur ; gardé 5 min, nouvel essai 30 s après un échec | non (docs.discord.food, Get Guild Application Command Index) |
| `GET /channels/{id}/application-command-index` | — | même geste, dans un MP ou un groupe | non (Get Channel Application Command Index) |
| `GET /users/@me/application-command-index` | — | même geste (applications installées sur le compte) | non (Get User Application Command Index) |
| `POST /interactions` type 2 | `application_id`, `guild_id`, `channel_id`, `session_id`, `nonce`, `data` (`id`, `version`, `type`, `name`, `options`, `target_id`) | « Envoyer » du formulaire de commande, ou choix dans un menu « Applications » (message : type 3, utilisateur : type 2) | non (Create Interaction) |
| `POST /interactions` type 3 | même chose, plus `message_id`, `message_flags`, `data` (`component_type`, `custom_id`, `values`) | clic sur un bouton, choix dans une liste d'un message ; refusé tant que le même composant attend sa réponse | non |
| `POST /interactions` type 4 | `data` avec l'option en cours marquée `focused` | 350 ms après la dernière frappe dans une option à autocomplétion | non |
| `POST /interactions` type 5 | `data` (`id` de l'interaction, `custom_id`, `components` : rangée 1 ou étiquette 18 autour de chaque valeur) | « Envoyer » d'une fenêtre modale | non |

`session_id` : celui du READY de la passerelle en cours (il n'est jamais
écrit dans un journal : `SessionId` masque sa valeur). `nonce` : un
identifiant de type snowflake tiré de l'heure, propre à chaque envoi.
Aucune requête n'est retentée seule : un échec réseau ou une absence de
réponse s'affiche au-dessus de la saisie, et c'est l'utilisateur qui relance.
Aucun en-tête du client officiel n'est ajouté.

### Ce qui est reçu de la passerelle

- `INTERACTION_CREATE` (`id`, `nonce`), `INTERACTION_SUCCESS`,
  `INTERACTION_FAILURE` (`reason_code`, 2 : délai dépassé, affiché
  « L'application n'a pas répondu ») ;
- `INTERACTION_MODAL_CREATE` : fenêtre modale du robot ;
- `APPLICATION_COMMAND_AUTOCOMPLETE_RESPONSE` (`choices`, `nonce`) ;
- dans MESSAGE_CREATE / MESSAGE_UPDATE : `flags` (64 éphémère, 128
  réponse différée « … réfléchit », 32768 composants v2), `components`,
  `interaction_metadata`, `application_id`. Un message éphémère
  « Ignorer le message » disparaît localement, sans requête.

Pas encore pris en charge : options de type pièce jointe (l'envoi est
désactivé si elle est obligatoire), fenêtres « iframe », composants de
fenêtre File Upload, Radio Group et Checkbox, filtrage des commandes par
permissions.

**Non vérifié sans le vrai compte** : que Discord accepte ces requêtes
sans les en-têtes du client officiel, la forme réelle de l'index (champs
`application_commands` et `applications` des applications installées sur le
compte), le rythme des accusés INTERACTION_SUCCESS sur une réponse
différée.

## Paramètres du serveur : vue d'ensemble, webhooks, intégrations, boosts (lot A8 + A9)

Toutes les routes de ce lot sont documentées par Discord (vérifiées le
13 novembre 2026 sur discord.com/developers et docs.discord.food). Chaque
requête suit un geste dans la fenêtre « Paramètres du serveur » (clic droit
sur un serveur), passe par `rest::api_req` (limiteur commun, 15 s d'attente
au plus), n'est jamais répétée automatiquement et part sans
`X-Super-Properties`. Code : `src/net/admin/`, `src/state/admin/`,
`src/ui/guild_settings/` ; jeux d'essai `rest_guild_get.json`,
`rest_guild_webhooks.json`, `rest_guild_integrations.json`,
`rest_command_permissions.json` ; tests : `net::admin::guild::tests`,
`state::admin::*::tests`, `ui::guild_settings::*::tests`,
`tests/e2e/admin_guild.rs`.

| Méthode et chemin | Corps | Quand | Documenté |
|---|---|---|---|
| `GET /guilds/{id}?with_counts=true` | — | ouverture de la fenêtre (une fois par ouverture) | oui (Get Guild) |
| `PATCH /guilds/{id}` | seuls les champs modifiés : `name`, `description`, `icon`, `banner`, `splash` (PNG recadré en `data:`, ou `null`), `system_channel_id`, `system_channel_flags`, `afk_channel_id`, `afk_timeout` (60, 300, 900, 1800, 3600), `default_message_notifications`, `preferred_locale`, `premium_progress_bar_enabled` | « Enregistrer les modifications », case de la barre de progression des boosts | oui (Modify Guild, MANAGE_GUILD) |
| `GET /guilds/{id}/vanity-url` | — | vue d'ensemble d'un serveur qui a la fonction `VANITY_URL` (une fois par ouverture) | oui (Get Guild Vanity URL) |
| `GET /guilds/{id}/webhooks` ou `GET /channels/{id}/webhooks` | — | ouverture de la page Webhooks, « Actualiser », liste affichée rendue périmée par WEBHOOKS_UPDATE | oui |
| `POST /channels/{id}/webhooks` | `name` (1 à 80), `avatar` facultatif | « Nouveau webhook » | oui (Create Webhook) |
| `PATCH /webhooks/{id}` | `name`, `avatar` (ou `null`), `channel_id` : seuls les champs modifiés | « Modifier » puis « Enregistrer » | oui (Modify Webhook) |
| `DELETE /webhooks/{id}` | — ; `X-Audit-Log-Reason` si une raison est saisie | « Supprimer le webhook » puis « Supprimer » | oui (Delete Webhook) |
| `GET /guilds/{id}/integrations` | — | ouverture de la page Intégrations, « Actualiser », GUILD_INTEGRATIONS_UPDATE pendant qu'elle est affichée | oui (Get Guild Integrations) |
| `DELETE /guilds/{id}/integrations/{id}` | — ; `X-Audit-Log-Reason` si une raison est saisie | « Retirer » puis « Retirer définitivement » | oui (Delete Guild Integration) |
| `GET /applications/{app}/guilds/{guild}/commands/permissions` | — | « Voir les permissions des commandes » d'une application | oui (List Guild Application Command Permissions) ; appel par un compte utilisateur non confirmé |
| `GET /guilds/{id}/widget` | — | ouverture de la page Widget (une fois par ouverture), « Réessayer » (groupe 2, lot V) | oui (Get Guild Widget Settings) |
| `PATCH /guilds/{id}/widget` | `enabled`, `channel_id` (ou `null`) | interrupteur ou choix du salon de la page Widget | oui (Modify Guild Widget) |
| `GET /guilds/{id}/templates` | — | ouverture de la page Modèle de serveur (une fois par ouverture) | oui (Get Guild Templates) |
| `POST /guilds/{id}/templates` | `name` (1 à 100), `description` (0 à 120) | « Créer le modèle » | oui (Create Guild Template) |
| `PUT /guilds/{id}/templates/{code}` | — | « Synchroniser le modèle » | oui (Sync Guild Template) |
| `DELETE /guilds/{id}/templates/{code}` | — | « Supprimer le modèle » puis confirmation | oui (Delete Guild Template) |
| `GET /guilds/{id}/prune?days=N` | — | page Élagage : à l'ouverture et à chaque durée choisie (7 ou 30 jours), une fois par durée | oui (Get Guild Prune Count) |
| `POST /guilds/{id}/prune` | `days`, `compute_prune_count: true` ; `X-Audit-Log-Reason` si une raison est saisie | « Élaguer » puis confirmation | oui (Begin Guild Prune) |

Reçu de la passerelle : `GUILD_UPDATE` (objet Guild, champs à la racine ou
dans `properties`) met à jour la page, le nom du rail et le niveau de
boost ; `WEBHOOKS_UPDATE` et `GUILD_INTEGRATIONS_UPDATE` rendent la liste
chargée périmée (relue si elle est affichée). `premium_tier` et
`premium_subscription_count` sont aussi lus dans READY et GUILD_CREATE :
la limite d'envoi d'un fichier dans un serveur boosté devient 50 Mio
(niveau 2) ou 100 Mio (niveau 3) pour tous, sans demander le profil.

Refus : 401 ou 403 avec le code 60003 (serveur qui exige l'A2F des
modérateurs ; Discord répond l'un ou l'autre selon la route, et les deux
mènent au même chemin, `Problem::MfaRequired`) :
« l'authentification à deux facteurs est requise pour cette action, à
faire dans l'application officielle » ; autre 403 (50013 compris) : « vous
n'avez pas la permission ». L'adresse d'un webhook (elle contient son
jeton) est masquée par défaut, copiée sur un clic, jamais journalisée
(`Secret`, `Debug` masqué).

Non envoyés : écriture du lien personnalisé (`PATCH /guilds/{id}/vanity-url`,
renvoyée à l'application officielle), écriture des permissions des
commandes (jeton OAuth2 de l'application exigé), statistiques du serveur
(aucune route publique : ouvertes dans le navigateur), transfert de
propriété et suppression du serveur (A2F souvent exigée : application
officielle), achat de boosts.

**Non vérifié sans le vrai compte** : forme de GUILD_UPDATE pour un compte
utilisateur (racine ou `properties`), lecture des permissions de commandes
par un compte utilisateur, valeurs exactes des avantages par niveau (aide
de Discord, « Server Boosting FAQ » ; l'API ne les publie pas).

## Requêtes non documentées : événements programmés et intégration (lot A7)

Réservées aux comptes utilisateur (un robot reçoit un refus local, sans
requête). Chacune part d'un geste, passe par le limiteur de débit commun,
n'est jamais retentée, et a sa forme vérifiée par un jeu d'essai synthétique
(`tests/fixtures/rest_guild_scheduled_events.json`,
`rest_guild_onboarding.json`) et des tests (`net::admin::*`,
`tests/e2e/admin_events.rs`).

| Méthode et chemin | Corps | Quand | Source |
|---|---|---|---|
| `PUT /guilds/{id}/scheduled-events/{id}/users/@me` | — | « Ça m'intéresse » | discord.food, Guild Scheduled Event, Create Guild Scheduled Event User |
| `DELETE /guilds/{id}/scheduled-events/{id}/users/@me` | — | « Ça ne m'intéresse plus » | discord.food, Guild Scheduled Event, Delete Guild Scheduled Event User |
| `GET /users/@me/scheduled-events?guild_ids={id}` | — | ouverture de la liste des événements (mes inscriptions ; seules celles à l'événement entier, réponse 1, sont gardées) | discord.food, Guild Scheduled Event, Get User Guild Scheduled Events |
| `POST /guilds/{id}/onboarding-responses` | `onboarding_responses`: [ids], `onboarding_prompts_seen` et `onboarding_responses_seen`: { id: ms Unix } | « Enregistrer » de la fenêtre d'intégration, premières réponses | discord.food, Guild, Modify Guild Onboarding Responses |
| `PUT /guilds/{id}/onboarding-responses` | idem | même geste, réponses déjà données | discord.food, Guild, Modify Guild Onboarding Responses |

Documentées (API officielle) et aussi envoyées par ce lot : `GET`, `POST`,
`PATCH`, `DELETE /guilds/{id}/scheduled-events[/{id}]` (création, édition,
`status` 2/3/4 pour commencer, terminer, annuler ; suppression confirmée),
`GET`/`PATCH /guilds/{id}/welcome-screen`, `GET`/`PUT /guilds/{id}/onboarding`.
Un refus 401/403 de code 60003 (chemin unique, `Problem::MfaRequired`) est
affiché « l'authentification à deux facteurs est requise pour cette action, à
faire dans l'application officielle ».

**Non vérifié sans le vrai compte** : que Discord accepte des identifiants de
questions et d'options créés localement (horodatés comme des snowflakes) dans
`PUT /guilds/{id}/onboarding`, et le `POST` des premières réponses sans les
en-têtes du client officiel.

## Paramètres du serveur : emojis, autocollants, sons (lot A6)

Fenêtre « Paramètres du serveur » (clic droit sur un serveur), proposée
seulement avec la permission « Gérer les expressions » (bit 30,
`MANAGE_GUILD_EXPRESSIONS` ; droits inconnus : rien n'est proposé). Le
bit 43 (`CREATE_GUILD_EXPRESSIONS`) n'est pas utilisé : docs.discord.food
indique qu'il n'est pris en compte que pour les clients qui envoient un
numéro de version récent du client officiel, ce que Ferrocord ne fait pas.
Chaque requête suit un clic (ouverture d'une page, « Actualiser »,
« Envoyer », « Enregistrer », « Supprimer définitivement » après
confirmation), passe par `rest::api_req` (limiteur commun, 15 s d'attente
au plus), n'est jamais répétée automatiquement et part sans
`X-Super-Properties`. Une seule action de chaque sorte par élément à la
fois. Code : `src/net/admin/expressions.rs`,
`src/state/admin/{expressions,expr_files}.rs`, `src/expr_pick.rs`,
`src/ui/guild_settings/` ; jeux d'essai synthétiques
`rest_guild_emojis.json`, `rest_guild_stickers.json`,
`rest_guild_soundboard_sounds.json` ; tests : `net::admin::expressions::tests`,
`state::admin::*::tests`, `expr_pick::tests`, `ui::guild_settings::tests`,
`tests/e2e/expressions.rs`. Formes vérifiées le 13 novembre 2026
(docs.discord.food, « Emoji », « Sticker », « Soundboard » ; documentation
officielle : mêmes routes).

| Méthode et chemin | Corps | Quand | Documenté |
|---|---|---|---|
| `GET /guilds/{id}/emojis` | — | ouverture de la page Emojis, « Actualiser » (auteurs) | oui (List Guild Emojis) |
| `POST /guilds/{id}/emojis` | `name` (2 à 32, lettres, chiffres, `_`), `image` (`data:` PNG 128 × 128, ou GIF animé tel quel ; 256 Kio au plus), `roles` | « Envoyer » | oui (Create Guild Emoji) |
| `PATCH /guilds/{id}/emojis/{emoji}` | `name`, `roles` | « Enregistrer » | oui (Modify Guild Emoji) |
| `DELETE /guilds/{id}/emojis/{emoji}` | — | « Supprimer définitivement » | oui (Delete Guild Emoji) |
| `GET /guilds/{id}/stickers` | — | page Autocollants (descriptions, emojis associés, auteurs) | oui (List Guild Stickers) |
| `POST /guilds/{id}/stickers` | `multipart/form-data` : `name` (2 à 30), `description` (vide ou 2 à 100), `tags` (1 à 200), `file` (PNG, APNG ou GIF, 500 Kio au plus) | « Envoyer » | oui (Create Guild Sticker) |
| `PATCH /guilds/{id}/stickers/{sticker}` | `name`, `description`, `tags` | « Enregistrer » | oui (Modify Guild Sticker) |
| `DELETE /guilds/{id}/stickers/{sticker}` | — | « Supprimer définitivement » | oui (Delete Guild Sticker) |
| `GET /guilds/{id}/soundboard-sounds` | — | page Sons | oui (List Guild Soundboard Sounds) |
| `POST /guilds/{id}/soundboard-sounds` | `name` (2 à 32), `sound` (`data:` MP3 ou Ogg, 512 Kio et 5,2 s au plus, vérifiés avant l'envoi), `volume` (0 à 1), `emoji_id` ou `emoji_name` | « Envoyer » | oui (Create Guild Soundboard Sound) |
| `PATCH /guilds/{id}/soundboard-sounds/{sound}` | `name`, `volume`, `emoji_id`, `emoji_name` | « Enregistrer » | oui (Modify Guild Soundboard Sound) |
| `DELETE /guilds/{id}/soundboard-sounds/{sound}` | — | « Supprimer définitivement » | oui (Delete Guild Soundboard Sound) |

Reçu de la passerelle : listes d'emojis et d'autocollants (GUILD_CREATE,
GUILD_EMOJIS_UPDATE, GUILD_STICKERS_UPDATE, déjà lus), sons
(GUILD_SOUNDBOARD_SOUND_CREATE, _UPDATE, _DELETE,
GUILD_SOUNDBOARD_SOUNDS_UPDATE), fonctions
`MORE_EMOJI`, `MORE_STICKERS`, `MORE_SOUNDBOARD` (READY, GUILD_CREATE,
GUILD_UPDATE) pour les emplacements, avec le niveau de boost du serveur
(`Guild::premium_tier`, seule source). Une écriture acceptée ne change pas la
liste affichée : c'est l'événement de la passerelle qui le fait.

Refus : 401 ou 403 de code 60003 (chemin unique, `Problem::MfaRequired`) : « L'authentification
à deux facteurs est requise pour cette action, à faire dans l'application
officielle » ; 403 / 50013 : message de permission ; captcha jamais retenté.
Aucun son n'est joué (pas de bouton « écouter »).

**Non vérifié sans le vrai compte** : acceptation des envois par un compte
utilisateur sans en-têtes du client officiel, réponse réelle 401/60003 sur
ces routes, et emplacements des niveaux de boost actuels.

## Appels privés, conférences, statut des salons vocaux, tableau de sons (lot V6)

Sources (13 novembre 2026) : docs.discord.food, pages « Gateway Events »
(CALL_CREATE, CALL_UPDATE, CALL_DELETE, VOICE_CHANNEL_STATUS_UPDATE,
VOICE_CHANNEL_EFFECT_SEND), « Channels » (Ring et Stop Ringing Channel
Recipients, Modify Channel Status), « Voice » (état vocal, Modify Current
User Voice State) ; documentation officielle de Discord pour les ressources
Stage Instance, Soundboard et Voice. Jeux d'essai : `call_*.json`,
`voice_state_update_stage.json`, `guild_create_voice.json`,
`stage_instance_create.json`, `voice_channel_status_update.json`,
`voice_channel_effect_send.json`, `rest_soundboard_default_sounds.json`.

Tout part sur un geste, par le limiteur commun, sans en-tête du client officiel.

| Requête | Corps | Quand | Documentée |
|---|---|---|---|
| `POST /channels/{id}/call/ring` | `recipients` : `null` (tout le monde) ou une liste | « Démarrer un appel » (une fois, quand CALL_CREATE montre l'appel), « Faire sonner » | non |
| `POST /channels/{id}/call/stop-ringing` | `recipients` : moi (« Refuser ») ou ceux qui sonnent (« Arrêter la sonnerie ») | « Refuser », « Arrêter la sonnerie » | non |
| `PUT /channels/{id}/voice-status` | `status` (500 caractères au plus, `null` pour l'effacer) | « Définir le statut » | non |
| `POST /stage-instances` | `channel_id`, `topic` (1 à 120), `privacy_level: 2`, `send_start_notification` (seulement avec MENTION_EVERYONE) | « Démarrer la conférence » | oui |
| `PATCH /stage-instances/{salon}` | `topic` | « Modifier le sujet » | oui |
| `DELETE /stage-instances/{salon}` | — | « Terminer la conférence » | oui |
| `PATCH /guilds/{id}/voice-states/@me` | `channel_id` et `request_to_speak_timestamp` (instant ISO ou `null`), ou `suppress` | « Lever / Baisser la main », « Prendre la parole », « Redevenir auditeur » | oui |
| `PATCH /guilds/{id}/voice-states/{utilisateur}` | `channel_id`, `suppress` | « Inviter à parler », « Rendre auditeur » (modérateurs) | oui |
| `GET /soundboard-default-sounds` | — | première ouverture du tableau de sons de la session | oui |
| `GET /guilds/{id}/soundboard-sounds` | — | première ouverture du tableau de sons sur ce serveur | oui |
| `POST /channels/{id}/send-soundboard-sound` | `sound_id`, `source_guild_id` (son d'un autre serveur) | clic sur un son, connecté au salon et micro ouvert | oui |

Reçu de la passerelle : CALL_CREATE / UPDATE / DELETE, VOICE_STATE_UPDATE,
`voice_states`, `stage_instances` et `status` des salons dans READY,
READY_SUPPLEMENTAL et GUILD_CREATE, STAGE_INSTANCE_*,
VOICE_CHANNEL_STATUS_UPDATE, VOICE_CHANNEL_EFFECT_SEND.

Pas envoyé : la capacité `AUTO_CALL_CONNECT` (`1 << 12`) ni « Request Call
Connect » : les appels déjà en cours au moment de la connexion ne sont pas
vus avant leur prochain CALL_UPDATE. La connexion vocale elle-même (op 4)
est faite par le lot V4 et câblée par V7 (`Calls::take_join_request`).

**Non vérifié sans le vrai compte** : sonnerie acceptée sans en-têtes du
client officiel, forme réelle de CALL_CREATE (champ `voice_states` présent
pour un compte utilisateur), `status` des salons vocaux dans GUILD_CREATE.

## Caméra et directs (phase 5, lot M4) : opcodes non documentés

Sources (13 novembre 2026) : docs.discord.food, page « Gateway Events »
(Update Voice State, Create Stream, Delete Stream, Watch Stream, Stream
Create, Stream Update, Stream Server Update, Stream Delete, format de la clé
de direct). Jeux d'essai : `stream_create.json`, `stream_update.json`,
`stream_server_update.json` (sans jeton : ajouté par les tests),
`stream_delete.json`. Tests : `net::streamgw`, `state::stream`,
`state::video`, `tests/e2e/video.rs`.

Tout part après un geste (bouton Caméra, sélecteur de partage, « Regarder »,
« Arrêter »), par le limiteur de la passerelle (`GwSink`), sans champ propre
au client officiel. Avant READY/RESUMED, l'op 4 de la caméra est gardée ; les
ops 18 à 20 sont abandonnées (une session neuve n'a aucun direct).

| Opcode | Quand | Champs de `d` |
|---|---|---|
| 4 Update Voice State, champ `self_video` (**non documenté**) | caméra allumée ou éteinte ; ensuite présent dans chaque op 4 jusqu'au départ du salon | `guild_id`, `channel_id`, `self_mute`, `self_deaf`, `self_video` |
| 18 Create Stream (**non documenté**) | partage choisi dans le sélecteur ; un partage en cours est d'abord supprimé (op 19) | `type` (`guild` ou `call`), `guild_id` (nul pour un appel), `channel_id`, `preferred_region`: `null` |
| 19 Delete Stream (**non documenté**) | « Arrêter » le partage (ma clé), « Arrêter de regarder » (clé du direct) | `stream_key` |
| 20 Watch Stream (**non documenté**) | « Regarder » sur la tuile d'un direct de mon salon | `stream_key` |

Clé de direct : `guild:<serveur>:<salon>:<propriétaire>` ou
`call:<salon>:<propriétaire>`. Reçu : STREAM_CREATE et STREAM_UPDATE
(`stream_key`, `rtc_server_id`, `rtc_channel_id`, `region`, `viewer_ids`,
`paused`), STREAM_SERVER_UPDATE (`stream_key`, `endpoint` nul pendant une
réattribution, `token` : secret, jamais journalisé ni affiché), STREAM_DELETE
(`stream_key`, `reason` : `user_requested`, `stream_ended`, `stream_full`,
`unauthorized`, `safety_guild_rate_limited`… ; `unavailable`). Les fins
anormales sont annoncées une fois. Pas envoyé : op 22 (pause du direct), op 21.

La connexion vidéo elle-même (passerelle vocale du direct, `rtc_server_id`
et jeton) est l'affaire du câblage M5 : `state::stream::StreamConnState::Connecting`.

**Non vérifié sans le vrai compte** : acceptation de ces opcodes sans les
en-têtes ni les capacités du client officiel, présence réelle de
`viewer_ids` dans STREAM_UPDATE, nécessité de `preferred_region`.

## Vidéo dans la connexion vocale (lot M1, phase 5)

Sources (13 novembre 2026) : docs.discord.food, « Topics > Voice
Connections » (Identify `video`/`streams`, Ready `streams`, Select Protocol
`codecs`, Session Description et Session Update `video_codec`, op 12 Video,
« Go Live ») et « Gateway Events » (STREAM_CREATE : `rtc_server_id`,
`rtc_channel_id`) ; RFC 3550 (RTP/RTCP), 4585 (NACK, PLI), 5104 (FIR),
4588 (RTX), 6184 (H.264), 7741 (VP8) ; DAVE : protocol.md, « Codec
Handling », et libdave `codec_utils.cpp` (plages en clair). Rien n'est
envoyé à l'API REST ni à la passerelle principale : tout passe par la
passerelle vocale et l'UDP déjà ouverts. Jeux d'essai synthétiques dans
`src/net/voice/video/tests.rs` et `tests/e2e/voice_video.rs`.

| Message (passerelle vocale) | Champs ajoutés | Quand | Documenté |
|---|---|---|---|
| op 0 Identify | `video: true`, `streams` (un flux, `video` ou `screen`) ; direct : `channel_id` | à chaque connexion quand un codec vidéo est proposé | non |
| op 1 Select Protocol | `codecs` (Opus 1000, H264 103/104, `encode`/`decode`) ; lot V (phase 8) : VP8 105/106 en priorité 2000, si les « Extensions vidéo VP9 » de Windows sont là (jeu d'essai : `offre_vp8_et_bascule_du_codec_d_envoi`, `tests/e2e/voice_video.rs` `vp8_negocie_puis_envoye`) ; 0.1.9 : VP9 107/108 et AV1 101/102 avec `encode` à `false`, seulement si le réglage `[voix] codecs_vp9_av1` est posé et que Windows a les extensions ; charge RTP VP9 selon la RFC 9628, AV1 selon « RTP Payload Format For AV1 » ; image AV1 rendue sous la forme qu'attend DAVE (dernière OBU sans taille, protocol.md annexe A) ; **non vérifié sur un vrai appel** : que Discord envoie bien ces codecs à un client qui ne fait que les décoder, et les types de charge 105 à 108 (seuls 101 à 104 viennent d'un exemple) | après la découverte d'adresse | non |
| op 12 Video | `audio_ssrc`, `video_ssrc`, `rtx_ssrc`, `streams` (résolution, images/s, débit max) ; `video_ssrc` à 0 pour couper | caméra ou partage allumé ou coupé (geste) | non |
| RTCP NACK / PLI / FIR (UDP) | — | paquets perdus, décodeur perdu (rythme borné) | RFC |
| op 15 Media Sink Wants (lot M5) | `{ "<ssrc vidéo>": 0..100 }` : 0 tuile hors champ, 50 petite, 100 au-delà de 720p ; jamais `any` ni `pixelCounts` | quand la taille affichée d'une tuile change de palier, et après l'op 12 d'une personne réglée (pas de minuterie) | non (forme vérifiée le 13 novembre 2026 sur docs.discord.food, Topics › Voice Connections, « Media Sink Wants » ; jeu d'essai : `media_sink_wants` dans `src/net/voice/video/tests.rs`, `tests/e2e/video_call.rs`) |

Groupe MLS de DAVE pour un direct : `rtc_server_id - 1`.

**Non vérifié sans le vrai compte** : acceptation de l'offre H.264 seule
(et de VP8 en 105/106, types de charge choisis ici, absents de l'exemple),
noms et types de charge réellement attribués par le serveur, présence de
`rtx_ssrc` dans Ready, groupe DAVE d'un direct.

## « Parcourir les salons » (lot fin-texte-restes)

Source : docs.discord.food, « User Settings » : drapeaux des réglages d'un
serveur (`User Guild Settings Flags` : `OPT_IN_CHANNELS_OFF` = 1 << 13,
`OPT_IN_CHANNELS_ON` = 1 << 14) et d'un salon (`Channel Override Flags` :
`OPT_IN_ENABLED` = 1 << 12) ; route « Modify User Guild Settings ».

Envoyé, sur un clic dans la fenêtre « Parcourir les salons » (menu d'un
serveur avec intégration), une requête par case, jamais deux à la fois pour
un même serveur, par le limiteur :

- `PATCH /users/@me/guilds/{guild.id}/settings` `{ "flags": … }` (afficher
  tous les salons, ou seulement les suivis) ;
- même route, `{ "channel_overrides": { "{channel.id}": { "flags": … } } }`
  (suivre un salon ou une catégorie).

Reçu : `flags` des réglages du serveur et de chaque `channel_overrides`
(READY, USER_GUILD_SETTINGS_UPDATE, réponse du PATCH). En mode « seulement
les suivis », la liste ne montre que les salons suivis (ou dans une
catégorie suivie) et le salon ouvert. Les salons vocaux restent montrés.
Jeu d'essai : corps et décodage dans `net/account.rs` (`rest_bodies`).

## Événements : couverture et occurrences sautées (lot fin-texte-restes)

Source : docs.discord.food, « Guild Scheduled Event » (objet Exception,
routes Create / Delete Guild Scheduled Event Exception) ; champ `image`
(« image data ») de Create / Modify Guild Scheduled Event, documenté aussi
par Discord.

- Couverture : choisie par la boîte « Ouvrir », réduite à 1000 × 400 au plus
  dans un fil à part, envoyée en PNG `data:` dans `image` du `POST` ou du
  `PATCH /guilds/{id}/scheduled-events[/{event}]` déjà employés ; rien n'est
  envoyé si elle n'a pas été changée.
- « Sauter la prochaine fois » (événement récurrent, MANAGE_EVENTS) :
  `POST /guilds/{id}/scheduled-events/{event}/exceptions`
  `{ "original_scheduled_start_time": …, "is_canceled": true }` ;
  « Rétablir » : `DELETE …/exceptions/{event_exception_id}`, l'identifiant
  étant le snowflake du début d'origine (forme de la documentation).
- Reçu : `guild_scheduled_event_exceptions` de l'objet événement (liste et
  passerelle) ; seules les exceptions `is_canceled` sont gardées.
Un clic, une requête, par le limiteur. Jeu d'essai : `request_shapes`
(`net/admin/events.rs`).

## Commandes : options pièce jointe (lot fin-texte-restes)

Source : docs.discord.food, « Create Interaction » (`files[n]`,
`data.attachments`, valeur d'une option ATTACHMENT = identifiant de la
pièce jointe) ; forme `payload_json` + `files[n]` de « Uploading Files »
(documentation de Discord). Quand une commande a des fichiers,
`POST /interactions` part en `multipart/form-data` : `payload_json` (le
corps habituel, `data.attachments` = `[{ "id": "0", "filename": … }]`),
puis une partie `files[n]` par fichier ; l'option vaut `"n"`. Fichiers lus
en entier hors du fil d'interface, 10 Mio au plus chacun. Jeu d'essai :
`attachment_options_go_multipart` (`net/interactions.rs`).
## Activité de jeu détectée et présence enrichie (lot C3)

### Sources (lues le 13 novembre 2026)

- docs.discord.food, page « Game » : « List Detectable Games », `GET /games/detectable`, liste d'objets « Detectable Application » (`id`, `name`, `executables[]` avec `os`, `name`, `is_launcher`). Le chemin `/applications/detectable` cité dans le plan n'y figure pas : Ferrocord suit la page.
- docs.discord.food, page « Gateway Events » : « Update Presence » (op 3), champs `since`, `activities`, `status`, `afk` ; 5 mises à jour par 20 s au plus.
- docs.discord.food, page « Presence » : objet Activity (`application_id`, `session_id`, `sync_id`, `assets`, `buttons`, `timestamps` en chaînes), images d'activité (`mp:`, identifiant d'image d'application, `spotify:`…), « Get Activity Metadata » : `GET /users/{user.id}/sessions/{session_id}/activities/{application.id}/metadata` (`0` pour une écoute sans application), réponse `button_urls`.
- Règle de correspondance des exécutables (non décrite par la documentation) : client libre arRPC (OpenAsar, `src/process/index.js`) : fin du chemin comparée composant par composant, `>` = nom de fichier exact, lanceurs ignorés.

### Ce que Ferrocord envoie

| Quand | Quoi | Rythme |
|---|---|---|
| Connexion, réglage « Afficher l'activité en cours » actif, cache absent ou de plus d'un jour | `GET /games/detectable` | 60 s après la connexion, au plus une fois par jour (cache `%LOCALAPPDATA%\ferrocord\detectable.json`, forme compacte : identifiant, nom, exécutables Windows) ; limiteur commun |
| Un jeu détecté démarre ou s'arrête, ou le statut change après un premier envoi | op 3 : `status` (réglage synchronisé), `since: 0`, `afk: false`, `activities` : statut personnalisé (type 4, s'il y en a un et qu'il n'a pas expiré) puis le jeu (type 0, `name`, `application_id` s'il vient de la liste, `timestamps.start` = lancement du processus) | sondage toutes les 20 s ; 4 s au moins entre deux op 3 ; limiteur de la passerelle |
| Clic sur un bouton d'activité d'une autre personne | `GET /users/{id}/sessions/{session}/activities/{app}/metadata` | une demande à la fois, geste de l'utilisateur ; `session` réduit aux lettres et chiffres |

Jamais d'op 3 tant qu'aucun jeu n'a été montré dans la session : Discord garde le statut qu'il connaît (comportement d'avant ce lot). Statut invisible : `activities` vide. Réglage coupé : le sondage s'arrête (aucun relevé des processus) et le jeu disparaît à l'envoi suivant. Compte bot : rien de tout cela.

### Sur cet ordinateur

Les processus sont relevés par un instantané ToolHelp (noms de fichier seulement, aucun processus ouvert) ; seul un nouveau processus dont le nom de fichier figure dans la liste (ou parmi les programmes ajoutés) est ouvert en lecture limitée pour son chemin et son heure de lancement, une fois. Windows n'offre pas d'avis de création de processus sans WMI ou ETW. Programmes ajoutés à la main : `local.toml`, section `[jeux]`.

### Reçu et affiché

Activités des autres : images d'application (`https://cdn.discordapp.com/app-assets/{app}/{image}.png`) et du proxy média (`mp:` → `https://media.discordapp.net/…`), dans le cache d'images borné ; images d'autres sites (`spotify:`, `twitch:`, `youtube:`) jamais chargées (aucun contact avec un tiers) ; durée écoulée, progression d'une écoute (`timestamps.end`), boutons (libellés reçus, adresse demandée au clic), « Écouter sur Spotify » (`https://open.spotify.com/track/{sync_id}`, ouvert dans le navigateur).

### À vérifier sur un vrai compte

- `GET /games/detectable` répond bien sans paramètre, et la taille de la réponse ;
- un jeu reconnu s'affiche chez les autres (« Joue à … ») avec sa durée, et disparaît à sa fermeture ;
- le statut personnalisé reste visible pendant et après un jeu ; statut invisible : rien n'est montré ;
- `afk: false` n'empêche pas les notifications mobiles attendues ;
- l'adresse d'un bouton d'activité est bien rendue par la route des métadonnées.

## Rich Presence locale (lot R)

Serveur des tubes `\\.\pipe\discord-ipc-{0..9}` (premier libre) pour les jeux et logiciels ; code dans `src/rpc/`, état dans `src/state/rpc.rs`. Sources : documentation Discord RPC (docs.discord.com/developers/topics/rpc : charge utile, READY, SET_ACTIVITY, erreurs), bibliothèque discord-rpc (`src/connection_win.cpp`, `src/rpc_connection.h`, `src/serialization.cpp` : tubes, trames, poignée de main, champs de l'activité), docs.discord.food « Presence » › Activity Object (forme dans l'op 3 : `buttons` = libellés, `metadata.button_urls`). Détail dans l'en-tête de `src/rpc/mod.rs`.

### Ce que Ferrocord envoie à Discord

| Quand | Quoi | Rythme |
|---|---|---|
| Une application branchée envoie SET_ACTIVITY (ou se débranche) | op 3 (documentée), comme pour un jeu détecté, plus une activité : `name` (nom de la liste des jeux de Discord, sinon celui donné, sinon le programme), `type` (0, 2, 3 ou 5), `application_id` (le `client_id` de la poignée de main), `details`, `state`, `timestamps`, `assets` (identifiants numériques et `mp:` seulement), `party` (`id`, `size`), `buttons` + `metadata.button_urls` | au plus une activité par seconde et par application vers le magasin, puis le limiteur de l'op 3 (4 s entre deux envois) |

Une seule requête REST, et seulement au clic sur « Charger les images » (lot R, voir « Lot R : restes ») : la liste des images de l'application, qui traduit les images désignées par un nom. Non transmis : les secrets de groupe (`match`, `join`, `spectate`), `instance`, les images désignées par une adresse (le client officiel les résout par `external-assets`, que Ferrocord n'envoie pas), et les images nommées tant que la liste n'a pas été chargée. Même réglage que les jeux : « Afficher l'activité en cours » coupé ou statut invisible, rien n'est montré. Réglage de cet ordinateur : `local.toml`, section `[rich_presence]`.

### Ce que les programmes locaux reçoivent

READY avec un utilisateur neutre (`id` « 0 », aucun nom ni avatar du compte) ; la réponse à SET_ACTIVITY ; une erreur 4002 pour toute autre commande (AUTHORIZE, AUTHENTICATE, SUBSCRIBE…), rien n'étant fait pour le compte. Clients distants refusés, trames de 64 Kio au plus, poignée de main attendue 10 s, 8 applications à la fois.

### À vérifier sur un vrai compte

- une activité Rich Presence (avec `application_id`, `assets` numériques, `party`, boutons) envoyée par l'op 3 d'un compte utilisateur s'affiche chez les autres ;
- le nom affiché : celui de l'op 3 ou celui que Discord tire de l'application ;
- un vrai jeu (SDK ou discord-rpc) se branche quand le client officiel ne tourne pas.

## Messages vocaux et sons du tableau (phase 6)

Sources (13 novembre 2026) : docs.discord.food, « Resources › Message »,
sections « Message Flags » (`IS_VOICE_MESSAGE` = 1 << 13), « Voice
Messages » (une seule pièce jointe audio, ni texte, ni embed, ni autre
pièce jointe ; type `audio/…` ; clients officiels : Opus mono 48 kHz,
32 kbit/s, conteneur Ogg ; onde : un octet par point, base64, 256 points
au plus) et « Attachment Object » (`duration_secs` flottant, `waveform`
chaîne base64) ; envoi par `multipart/form-data` comme les autres fichiers
(« Create Message »). Adresse des sons : documentation de Discord, « CDN
Endpoints » (`soundboard-sounds/{sound_id}`).

| Requête | Quand | Documentée |
|---|---|---|
| `POST /channels/{id}/messages` multipart : `payload_json` avec `flags: 8192`, sans `content`, `attachments: [{ id: 0, filename: "voice-message.ogg", duration_secs, waveform }]`, puis `files[0]` en `audio/ogg` | bouton « Envoyer » d'un message vocal enregistré (geste) ; même file, même limiteur, même nonce que les autres envois de fichiers | non (docs.discord.food) |
| `GET https://cdn.discordapp.com/soundboard-sounds/{id}` (sans jeton) | son du tableau joué dans le salon où l'on est (SOUNDBOARD/VOICE_CHANNEL_EFFECT_SEND reçu) ; décodé une fois, gardé en cache borné | oui (CDN) |
| `GET` de l'adresse de la pièce jointe (cdn.discordapp.com ou media.discordapp.net, sans jeton) | clic sur « Écouter » d'un message vocal | oui (CDN) |
| `GET` de l'adresse de la pièce jointe (mêmes hôtes seulement, sans jeton, 64 Mio au plus, tenue en mémoire le temps de la lecture) | clic sur « Lecture » d'une vidéo (MP4, MOV) ou d'un son (MP3, Ogg/Opus, WAV, M4A, AAC, FLAC) joint à un message : lecteur du fil (lot M, `voice/player.rs`) ; jamais avant le clic, un seul fichier à la fois | oui (CDN) |

Jeux d'essai : `tests/fixtures/message_create_voice.json`
(`net::media::tests::voice_message_fixture`), corps envoyé :
`net::upload::tests::voice_message_payload`.

**Non vérifié sans le vrai compte** : acceptation du message vocal envoyé en
multipart (et non par téléversement préalable), affichage de l'onde chez les
autres clients.

## Connexion par QR code (lot C2, phase 6) : non documentée

Source (13 novembre 2026) : docs.discord.food, « Remote Authentication ›
Desktop » (<https://docs.discord.food/remote-authentication/desktop>).
Code : `src/net/remote_auth.rs` ; faux serveur et jeux d'essai synthétiques :
`tests/remote_auth.rs`. Déclenché uniquement par le bouton « Se connecter
avec un QR code » ; à l'expiration, au plus 3 renouvellements d'eux-mêmes,
puis un clic sur « Nouveau QR code ».

| Envoi | Contenu | Quand | Documenté |
|---|---|---|---|
| WebSocket `wss://remote-auth-gateway.discord.gg/?v=2` | en-têtes `Origin: https://discord.com` (exigé par la passerelle, refus sinon) et `User-Agent: Ferrocord/<version>` ; rien d'autre | ouverture du panneau QR, renouvellement | non |
| `init` | `encoded_public_key` : clé publique RSA-OAEP 2048 (SPKI DER, base64), créée en mémoire pour cette tentative, jamais écrite | après `hello` | non |
| `nonce_proof` | `nonce` : nonce déchiffré (SHA-256/MGF1 SHA-256), base64url sans `=` | une fois, sur `nonce_proof` du serveur | non |
| `heartbeat` | — | toutes les `heartbeat_interval` ms (sans accusé : connexion abandonnée) | non |
| `POST /users/@me/remote-auth/login` | `{ "ticket" }`, sans `Authorization` ; réponse `encrypted_token` déchiffrée par la clé privée | une fois, après `pending_login` (confirmation sur le téléphone) ; par le limiteur de débit | non |

Contrôles : l'empreinte de `pending_remote_init` doit être le SHA-256 de
notre clé publique (sinon rien n'est affiché) ; étapes hors d'ordre ou
indéchiffrables : abandon. Captcha demandé à l'échange : message qui renvoie
à l'application officielle, jamais de contournement. Le jeton obtenu passe
par `Token::parse` puis le même chemin qu'un jeton saisi (`add_account`,
coffre après READY si « Se souvenir de moi »). Le journal ne note que les
étapes (ni clé, ni nonce, ni ticket, ni jeton ; vérifié par les tests).

**Écart possible au client officiel** : l'en-tête `Origin` vaut celui du
site de Discord, faute de quoi la passerelle refuse la connexion ; aucune
autre propriété du client officiel n'est envoyée.
L'en-tête `Origin: https://discord.com` est exigé par la passerelle
d'authentification à distance ; c'est le seul écart : le User-Agent reste
celui de Ferrocord.

**Non vérifié sans le vrai compte** : acceptation de la connexion avec un
User-Agent de Ferrocord, captcha ou non à l'échange du ticket, durée réelle
de `timeout_ms`.

## Personnalisation des profils (lot P) : champs lus, aucune requête ajoutée

Tout ce qui suit est lu dans des objets déjà reçus (`user`, `member`, `role`,
réponse de `GET /users/{id}/profile`). **Aucune requête n'a été ajoutée** ;
seules de nouvelles adresses d'images du CDN sont demandées, par la file
d'images habituelle (limiteur, cache borné, seulement pour ce qui est affiché).

### Sources (lues le 14 novembre 2026)

- Documentation officielle (docs.discord.com/developers) : User Object
  (`avatar_decoration_data`, `collectibles.nameplate`, `primary_guild`),
  Guild Member Object (`avatar`, `banner`, `avatar_decoration_data`), Role
  Object (`colors` : `primary_color`, `secondary_color`, `tertiary_color`),
  Image Formatting (chemins du CDN, `?animated=true` en WebP).
- docs.discord.food (non officielle) : `display_name_styles` (`font_id`,
  `effect_id`, `colors`), `user_profile.theme_colors`, chemin des plaques
  (`/assets/collectibles/{asset}static.png`), paramètre `passthrough`.

### Champs lus

| Champ | Objet | Documenté | Affichage |
|---|---|---|---|
| `banner` (`a_` : animée) | user, member, profil | oui | carte et profil complet, recadrée au centre (17:6 dans la carte) |
| `avatar` (`a_`) | user, member | oui | animé au survol et dans un profil ouvert |
| `avatar_decoration_data.asset` | user, member | oui | autour de l'avatar (1,2 fois son côté) |
| `collectibles.nameplate` (`asset`, `palette`) | user | oui | fond de la ligne (membres, MP), image fixe |
| `primary_guild` (`tag`, `badge`, `identity_guild_id`, `identity_enabled`) | user | oui | pastille à côté du nom |
| `colors` | role | oui | nom en dégradé (deux couleurs) ou holographique (trois) |
| `display_name_styles.colors`, `effect_id`, `font_id` | user | non | couleur ou dégradé du nom, décor de l'effet (1 à 8), police du style (quatorze polices libres embarquées, `docs/design.md` § 3) |
| `user_profile.theme_colors` | profil | non | fond en dégradé du profil, texte au contraste calculé |

### Adresses du CDN

| Adresse | Quand |
|---|---|
| `avatar-decoration-presets/{asset}.png?size=N&passthrough=false` | décoration fixe (listes, messages) |
| `avatar-decoration-presets/{asset}.png?size=N&passthrough=true` | décoration animée (APNG), profil ouvert seulement |
| `assets/collectibles/{asset}static.png` | plaque visible dans une liste |
| `assets/collectibles/{asset}asset.webm` | plaque sous le pointeur (lot plaques), une fois par plaque |
| `guild-tag-badges/{guild_id}/{hash}.png?size=N` | pastille de tag visible |
| `banners/…/a_….webp?size=N&animated=true` | bannière animée, profil ouvert seulement |

Le nom d'un `asset` reçu est validé avant d'entrer dans une adresse
(minuscules, chiffres, `_` ; pour une plaque, segments séparés par `/` avec `-`
en plus, jamais vides ; longueur bornée) : il ne peut
changer ni d'hôte ni de requête.

### Non fait, et pourquoi

- **Effets de profil** : faits depuis par le lot ND (voir « Effets de profil
  (lot ND) » plus bas) ; la liste `GET /user-profile-effects` envisagée ici
  n'existe plus.
- **Plaques animées** : faites depuis par le lot plaques. Le WebM
  (`asset.webm`) n'est demandé que pour la ligne que survole le pointeur ;
  il est démultiplexé ici (`video/mkv.rs`) et décodé par Windows (VP8 ou VP9,
  « Extensions vidéo VP9 »), ramené à la taille de la ligne et joué en boucle
  (`images/webm.rs`). Sans décodeur, ou fichier illisible : l'image fixe
  reste, sans nouvelle tentative. La transparence du WebM (piste qui annonce
  `AlphaMode` : l'alpha de chaque image est une seconde image du même codec,
  dans les `BlockAdditions` de son groupe) est décodée et composée (lot
  plaques2) ; si l'alpha manque ou ne se décode pas, l'image fixe reste.
  Adresse et lecture confirmées sur un vrai compte ; la forme de l'alpha
  n'a été vérifiée que sur des fichiers fabriqués ici.
- **Polices et effets du nom** (néon, pop, etc.) : polices non embarquées ;
  seules les couleurs (unie ou dégradé) sont rendues.
- **Teintes des palettes de plaque** : la documentation ne donne que leurs
  noms ; les teintes sont les nôtres.

### À vérifier sur un vrai compte

- les adresses des plaques (`static.png`, `asset.webm`) et du `passthrough`
  (source non officielle) ; le codec du WebM des plaques (VP9 supposé) et la
  présence d'une piste alpha ;
- la présence de `display_name_styles` et de `collectibles` dans les objets
  `user` partiels des messages et de la liste des membres ;
- une bannière animée lourde : délai avant la première image animée (l'image
  fixe tient la place entre-temps) ;
- le rendu d'un rôle holographique face au client officiel.

## Identité annoncée : application de bureau ou Ferrocord (lot identité-bureau)

**Décision de l'utilisateur du 14 novembre 2026.** Jusqu'ici Ferrocord
s'annonçait sous son nom (`browser: "Ferrocord"`, pas de
`X-Super-Properties`), et Discord classait la session comme « navigateur ».
L'utilisateur, propriétaire du compte, veut apparaître connecté depuis un
ordinateur ; averti du risque, il l'a accepté. La règle « ne pas imiter le
client officiel » (PLAN.md 1.2.4) est levée **pour l'identité annoncée
seulement** : aucun captcha n'est contourné, aucun achat, aucune
automatisation, et le limiteur comme le rythme humain restent.

Tout sort d'un seul module, `src/net/identity.rs`. Deux modes :

- **bureau** (par défaut sous Windows) : l'identité du client de bureau
  stable pour Windows, la même sur la passerelle, l'API, le CDN, le vocal et
  la connexion par QR code ;
- **Ferrocord** : l'identité d'origine, inchangée (tableau du début de ce
  document ; les sections précédentes qui disent « sans `X-Super-Properties` »
  décrivent ce mode). Toujours employée pour un **jeton de bot**, et **hors de
  Windows** (Android compris : on n'annonce pas un système qu'on n'a pas).

Réglage : Paramètres › Avancés, « Se présenter à Discord comme l'application
de bureau » ; section `[identite]` de `local.toml` (`bureau = true`). Le mode
est relu à chaque connexion ; les numéros de version, la machine, le
User-Agent du CDN et les en-têtes du vocal sont figés au lancement (un
changement du réglage est donc complet après un redémarrage).

### Sources (lues le 14 novembre 2026)

- [docs.discord.food](https://docs.discord.food/reference#client-properties),
  « Reference » › Client Properties : structure, valeurs de `os` et de
  `browser` (`Discord Client` → type de session `desktop`), canaux, signature
  de lancement, exemple Windows ; › Locales (`X-Discord-Locale`, puis
  `Accept-Language`) ; › HTTP API (`X-Debug-Options`, facultatif).
- docs.discord.food, « Gateway Events » › Identify (`properties`,
  `capabilities`, `presence`, `client_state`) et « Gateway » › Gateway
  Capabilities (bits 0 à 16) ; « Voice Connections » › Identify (aucune
  propriété du client).
- docs.discord.food, « Client Distribution » › Get Latest Distributed
  Application Manifest.
- discord.py-self (`discord/tracking.py`, `http.py`, `gateway.py`) : en-têtes
  des requêtes et des WebSockets, propriétés propres à la passerelle, lecture
  du numéro de build, `Sec-CH-UA`. Son service public
  (`cordapi.dolfi.es/api/v2/properties/windows`) a été interrogé **une fois, à
  la main, le 14 novembre 2026**, pour relever l'ordre des champs et les
  versions d'Electron et de Chromium ; Ferrocord ne l'appelle jamais.
- Norme Fetch (WHATWG) : `Origin` hors GET et HEAD pour une requête de même
  origine.

### Propriétés envoyées (mode bureau)

Dans cet ordre, dans `properties` de l'Identify et, encodées en base64, dans
`X-Super-Properties` :

| Champ | Valeur | D'où |
|---|---|---|
| `os` | `Windows` | constante (le mode n'existe que sous Windows) |
| `browser` | `Discord Client` | constante |
| `release_channel` | `stable` | constante |
| `client_version` | `1.0.9268` | version de l'hôte : rafraîchie (manifeste), sinon embarquée |
| `os_version` | `10.0.26100` | **machine** : registre (`CurrentMajorVersionNumber`, `CurrentMinorVersionNumber`, `CurrentBuildNumber`) |
| `os_arch`, `app_arch` | `x64` | **machine** (`PROCESSOR_ARCHITECTURE`) et architecture de l'exécutable |
| `system_locale` | `fr-FR` | **machine** (langue du système) |
| `has_client_mods` | `false` | constante |
| `client_launch_id` | UUID v4 | tiré au hasard à chaque connexion |
| `browser_user_agent` | `Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) discord/{client_version} Chrome/{chrome} Electron/{electron} Safari/537.36` | identique à l'en-tête `User-Agent` |
| `browser_version` | `44.10.2` | version d'Electron : embarquée ou `local.toml` |
| `os_sdk_version` | `26100` | **machine** (numéro de build de Windows) |
| `client_build_number` | `673764` | rafraîchi (page `/login`), sinon embarqué |
| `native_build_number` | `98625` | rafraîchi (manifeste), sinon embarqué |
| `client_event_source` | `null` | constante |
| `launch_signature` | UUID v4 dont les 12 bits de détection des modifications sont à zéro | tiré au hasard à chaque connexion |
| `client_heartbeat_session_id` | UUID v4 | refait toutes les 30 minutes |
| `is_fast_connect`, `gateway_connect_reasons` | `false`, `AppSkeleton` | **Identify seulement** |

L'Identify garde par ailleurs ses champs d'avant : `token`, `compress` :
false, `large_threshold` : 50.

### En-têtes (mode bureau)

| Où | En-têtes |
|---|---|
| API REST (toute requête, `rest::api_req`) | `User-Agent`, `X-Super-Properties`, `X-Discord-Locale` (depuis le lot « fiable » : la langue choisie dans Ferrocord, `fr` ou `en-US`, comme le client officiel annonce la sienne ; sans choix, la langue de Discord la plus proche de celle du système ; figée pour la connexion), `X-Discord-Timezone` (fuseau IANA du système ; omis s'il est inconnu), `Accept-Language` (`fr-FR,fr;q=0.9`), `Referer: https://discord.com/channels/@me`, `Sec-CH-UA` (`"Not?A_Brand";v="24", "Chromium";v="152"`, dérivé de la version de Chromium), `Sec-CH-UA-Mobile: ?0`, `Sec-CH-UA-Platform: "Windows"`, `Sec-Fetch-Dest: empty`, `Sec-Fetch-Mode: cors`, `Sec-Fetch-Site: same-origin` ; `Origin: https://discord.com` hors GET et HEAD |
| Passerelle principale et passerelle vocale (ouverture du WebSocket) | `User-Agent`, `Origin: https://discord.com`, `Accept-Language` |
| CDN (images, et depuis le lot « fiable » les sons et fichiers joints lus par `voice::clips`, qui partaient sans `User-Agent`) | `User-Agent` seulement (jamais de jeton ni de propriétés) |
| Connexion par QR code | WebSocket : `User-Agent`, `Accept-Language` (lot « fiable ») et l'`Origin` déjà exigé ; échange du ticket : comme l'API |

L'Identify de la passerelle vocale (op 0) ne contient aucune propriété du
client : inchangé.

### Numéros de version

Valeurs embarquées, relevées le 14 novembre 2026
(`identity::Versions::embedded`) : hôte 1.0.9268, build 673764, natif 98625,
Electron 44.10.2, Chromium 152.0.7977.169.

Rafraîchissement (`identity::refresh`) : à la connexion en mode bureau, **au
plus une fois par jour** (l'heure du dernier essai, réussi ou non, est dans
`<cache>/ferrocord/identite.json`), **sans jeton**, par le limiteur de débit,
en arrière-plan. Le résultat sert **au prochain lancement** ; en cas d'échec,
ce qui était connu reste. Une valeur reçue n'est gardée que si sa forme est
celle attendue, et jamais si elle est plus ancienne que la valeur embarquée.

| Requête | Ce qui en est lu | Source |
|---|---|---|
| `GET https://updates.discord.com/distributions/app/manifests/latest?install_id={uuid}&channel=stable&platform=win&arch=x64` | `full.host_version` → `client_version` ; `metadata_version` → `native_build_number` | docs.discord.food, Get Latest Distributed Application Manifest |
| `GET https://discord.com/login` | `"BUILD_NUMBER":"…"` de la page → `client_build_number` | discord.py-self, `scrape_client_build_number` |

`install_id` : `metadata_version` n'est renvoyé que si ce paramètre est
présent (documenté, et vérifié le 14 novembre 2026). C'est un UUID tiré au
hasard une fois et gardé dans `identite.json` ; il n'est lié à aucun compte
et ne part nulle part ailleurs.

Les versions d'**Electron** et de **Chromium** ne se lisent dans aucune
réponse publique de Discord : elles restent celles embarquées jusqu'à une
mise à jour de Ferrocord, ou se règlent dans `local.toml`, comme les autres :

```toml
[identite]
bureau = true
client_version = "1.0.9268"
client_build_number = 673764
native_build_number = 98625
electron = "44.10.2"
chrome = "152.0.7977.169"
```

Une valeur de `local.toml` l'emporte sur le rafraîchissement.

### Non envoyé, faute de source ou par prudence

- `capabilities`, `presence` et `client_state` de l'Identify : le client
  officiel les envoie toujours. `capabilities` change la forme du READY et de
  plusieurs événements ; l'allumer sans capture d'un vrai compte casserait la
  lecture. **C'est l'écart le plus visible qui reste.**
- `client_app_state` (`focused`, `unfocused`…) : présent dans le relevé du
  client actuel ; la couche réseau ne connaît pas l'état de la fenêtre, et
  une valeur fixe serait fausse.
- `X-Debug-Options` : documenté comme facultatif, valeur du client non établie.
- `X-Installation-ID`, `X-Context-Properties`, `X-Fingerprint` : non établis
  pour une session connectée, ou propres à chaque action.
- `Accept-Encoding`, `Priority`, extension `permessage-deflate` du
  WebSocket : Ferrocord ne sait pas décoder ce qu'ils annoncent.

### Limites connues : ce qui peut encore distinguer Ferrocord

- **Empreinte TLS** (rustls, et non BoringSSL de Chromium) et **HTTP/1.1**
  (Chromium parle HTTP/2 ou HTTP/3, avec son ordre d'en-têtes et ses
  réglages) : non corrigeable simplement, non tenté.
- Ordre et casse des en-têtes HTTP, décidés par reqwest.
- Champs de l'Identify absents (ci-dessus), `large_threshold` : 50, passerelle
  en `v=10` avec `zlib-stream`.
- **Comportement** : le client officiel envoie des événements d'analyse, des
  requêtes au démarrage et des en-têtes de contexte que Ferrocord n'envoie
  pas ; il n'utilise pas les mêmes routes au même rythme.
- Numéros de version : Electron et Chromium vieillissent entre deux versions
  de Ferrocord ; le build du client web change souvent, le rafraîchissement
  est quotidien et ne vaut qu'au lancement suivant.
- `Accept-Language` suit la langue du système ; `X-Discord-Locale` suit la
  langue choisie dans Ferrocord (lot « fiable »), qui peut différer de la
  langue enregistrée dans les réglages Discord du compte ; la valeur exacte d'`Accept-Language` pour
  une langue autre que `en-US` n'est pas établie par les sources.
- Connexion par QR code : ses identifiants de lancement ne sont pas ceux de
  la session qui suit.
- Après un changement du réglage sans redémarrage, le CDN et le vocal gardent
  l'identité du lancement.

### Risque

Un client tiers enfreint déjà les conditions de Discord. Se présenter comme
son application peut être tenu pour une circonstance aggravante si
l'incohérence est détectée (empreinte TLS, champs absents) : avertissement,
vérification demandée, suspension ou fermeture du compte. Le mode Ferrocord
reste à un clic.

### Tests

`net::identity::tests` (champs, ordre, cohérence Identify / en-têtes, jeton
absent, signature de lancement, lecture défensive des réponses),
`state::identity_prefs::tests`, `tests/e2e/identity.rs` (faux serveur : mode
bureau sur la passerelle, l'API et le CDN ; mode Ferrocord inchangé ; bot ;
rafraîchissement une fois par jour, sans jeton, échec sans perte).

### À vérifier sur le vrai compte

- Paramètres de Discord › Appareils : la session de Ferrocord apparaît comme
  un ordinateur (client Windows) et non comme un navigateur ; les amis voient
  l'icône d'ordinateur ;
- aucune demande de vérification (captcha, e-mail, téléphone) après le
  premier Identify en mode bureau, ni dans les jours qui suivent ;
- le READY garde la forme lue jusqu'ici (les propriétés du client de bureau
  sans `capabilities` pourraient en changer une partie) ;
- les requêtes qui pouvaient échouer sans `X-Super-Properties` (demande
  d'ami, invitations, création de serveur) passent-elles mieux ;
- après 24 h : `identite.json` contient des numéros au moins aussi récents que
  ceux embarqués, repris au lancement suivant ;
- le fuseau horaire et la langue relevés sont les bons (ils viennent de
  Windows).

## Salons et serveurs (phase 10, lot S) : repli, salons muets, réglages vocaux

Aucune route nouvelle hors documentation officielle ; deux champs de plus
dans une requête non documentée déjà utilisée.

| Méthode et chemin | Quand | Corps | Documenté |
|---|---|---|---|
| `PATCH /users/@me/guilds/{id}/settings` | clic sur une catégorie, « Replier toutes les catégories » | `channel_overrides` : { catégorie : { `collapsed` } } | non (docs.discord.food, Channel Override Structure) |
| `PATCH /users/@me/guilds/{id}/settings` | « Masquer les salons muets » | `hide_muted_channels` | non (docs.discord.food, User Guild Settings Structure) |
| `PATCH /users/@me/guilds/{id}/settings` | fenêtre « Paramètres de notification » du serveur (phase 10, lot N) : un clic = un réglage, comme le menu ; « Retirer l'exception » d'un salon | `channel_overrides` : { salon : { `message_notifications`: 3, `muted`: false, `mute_config`: null } } pour le retrait ; sinon les corps déjà décrits | non (docs.discord.food, Modify User Guild Settings, Channel Override Structure) |
| `GET /voice/regions` | ouverture de « Modifier le salon » d'un salon vocal (une fois par session) | — | oui (List Voice Regions) |
| `PATCH /channels/{id}` | « Enregistrer » d'un salon vocal | seulement ce qui a changé : `bitrate`, `user_limit`, `rtc_region` (nul : automatique), `video_quality_mode` | oui (Modify Channel) |

Lecture : `collapsed` et `hide_muted_channels` viennent de
`user_guild_settings` (READY) et de `USER_GUILD_SETTINGS_UPDATE` ; le sujet
des salons (`topic`) de GUILD_CREATE, CHANNEL_CREATE et CHANNEL_UPDATE
(officiel). Une catégorie repliée garde visibles le salon ouvert et les
salons non lus ; « Masquer les salons muets » garde le salon ouvert et ceux
qui ont une mention. Chaque écriture part d'un clic, passe par le limiteur,
et l'affichage revient en arrière si elle échoue. Le débit est borné avant
l'envoi (8 kbps au minimum ; 96, 128, 256 ou 384 kbps selon le niveau de
boost ; 64 kbps pour une conférence), la limite à 99 personnes.

À vérifier sur un vrai compte : qu'une dérogation ne contenant que
`collapsed` est acceptée telle quelle (la source ne le précise pas).

## Sélecteur de GIF, favoris du compte, super réaction (lot G)

Toutes ces requêtes sont **non documentées par Discord** et réservées aux
comptes utilisateur. Formes vérifiées sur docs.discord.food avant tout envoi :
page « Integration » (`https://docs.discord.food/resources/integration`,
sections « List Trending GIF Categories », « List Trending GIFs », « Search
GIFs », « Track Selected GIF », « GIF Object », « GIF Media Formats »), page
« User Settings Proto » (`https://docs.discord.food/resources/user-settings-proto`,
type 2 `FRECENCY`), page « Message » (« Create Reaction », « Delete Own
Reaction », « Reaction Type »), et `discord-userdoccers/discord-protos`,
`FrecencyUserSettings.proto` (numéros des champs). Chacune passe par
`rest::api` (limiteur commun, 10 s d'attente au plus), sans
`X-Super-Properties` ni `X-Context-Properties` ; un compte bot n'appelle rien.
Code : `src/net/gifs.rs`, `src/state/gifs.rs`, `src/state/frecency.rs`
(formes vérifiées par leurs tests et par `tests/e2e/gifs.rs`, jeux d'essai
synthétiques `rest_gifs_search.json` et `rest_gifs_trending.json`).

| Méthode et chemin | Paramètres ou corps | Quand |
|---|---|---|
| `GET /gifs/trending` | `media_format=tinygif`, `locale` (`fr` ou `en-US`) | ouverture de l'onglet GIF (une fois par session ; redemandé à la réouverture après un échec, ou par « Réessayer ») |
| `GET /gifs/trending-gifs` | `media_format=tinygif`, `limit=50`, `locale` | clic sur la tuile « GIF tendance » |
| `GET /gifs/search` | `q`, `media_format=tinygif`, `limit=50`, `locale` | Entrée, clic sur une tuile de catégorie, ou **frappe** dans la recherche : la requête ne part qu'après 400 ms sans nouvelle touche (`SEARCH_DELAY`), une seule par mot tapé ; la même recherche n'est pas renvoyée ; une réponse d'une recherche remplacée est ignorée |
| `POST /gifs/select` | `{ "id", "q" }` (identifiant du GIF, recherche qui l'a trouvé) | envoi d'un GIF de la liste (pas pour un favori, qui n'a pas d'identifiant) ; la réponse n'est pas lue |
| `GET /users/@me/settings-proto/2` | — | première ouverture d'un sélecteur d'expressions (saisie ou réaction), une fois par session |
| `PATCH /users/@me/settings-proto/2` | `settings` : base64 d'un message ne contenant **que** le champ de premier niveau modifié, en entier, champs inconnus compris (`favorite_gifs` = 2 ou `favorite_emojis` = 5) ; `required_data_version` : `versions.data_version` connu | clic sur l'étoile d'un GIF ; « Ajouter aux favoris » / « Retirer des favoris » du clic droit sur un emoji |
| `PUT /channels/{id}/messages/{id}/reactions/{emoji}/@me?type=1` | — | emoji choisi dans le sélecteur de réaction, interrupteur « Super réaction » actif (proposé seulement si le profil chargé dit Nitro) |
| `DELETE /channels/{id}/messages/{id}/reactions/{emoji}/1/@me` | — | même geste sur un emoji que j'ai déjà en super réaction |

Passerelle : `USER_SETTINGS_PROTO_UPDATE` de type 2 est lu (favoris modifiés
depuis un autre client) ; le type 1 reste à `net/account.rs`.

Un GIF s'envoie comme le fait le client officiel : son champ `url` (la page
du GIF) est le texte du message, par la file d'envoi ordinaire (Create
Message, documenté), réponse en cours comprise.

Champs lus de `FrecencyUserSettings` : `versions.data_version` (1.3),
`favorite_gifs.gifs` (2.1, table adresse → `format`, `src`, `width`,
`height`, `order`), `favorite_emojis.emojis` (5.1, identifiants ou noms
courts), `emoji_frecency.emojis` (6.1, table → `total_uses`, `score`). Un GIF
ajouté aux favoris est écrit avec `format` = 1 (image), `src` = `gif_src`,
`order` = le plus grand connu + 1.

Garde-fous :

- réglages de type 2 absents, illisibles ou mis à jour partiellement sans
  base connue : **jamais réécrits** (le geste est refusé avec un message) ;
- une seule écriture à la fois, optimiste, rétablie octet pour octet si
  Discord refuse ou répond `out_of_date` (la réponse fait alors foi) ;
- **la fréquence d'usage n'est jamais écrite** : la source demande de
  regrouper ces écritures sur un jour, ce qui supposerait un envoi différé
  sans geste. La section « Fréquents » montre les emojis choisis pendant la
  session, puis ceux du compte ;
- une super réaction n'est pas comptée d'avance : le compte suit l'écho
  `MESSAGE_REACTION_ADD` (`burst`) ; un refus s'affiche et ne change rien ;
- adresses reçues (`url`, `src`, `gif_src`) : `https` seulement, sans espace
  ni caractère de contrôle, 512 caractères au plus ; les autres entrées sont
  écartées.

### Hôte d'images ajouté : `media.tenor.com`

Les réponses de `/gifs/*` désignent des fichiers de `media.tenor.com`
(Tenor, Google). Aucun proxy de Discord ne les sert sans signature : comme
le client officiel, Ferrocord les télécharge directement. La règle des hôtes
d'images (`net::cdn::CdnHosts::check`) accepte donc, en plus des domaines de
Discord et de Twemoji : `https://media.tenor.com/…` sans port, sans
identifiants, sans paramètres, pour un fichier `.gif`, `.webp` ou `.png`.
Aucun jeton ni aucun en-tête de Discord n'y part ; Tenor voit l'adresse IP
et les GIF affichés. Rien n'est demandé tant que l'onglet GIF n'est pas
ouvert, et seuls les GIF visibles sont téléchargés (format `tinygif`,
environ 220 px de large).

### Non fait, et pourquoi

- **Favoris vidéo** (ajoutés depuis le client officiel, `format` = 2, média
  MP4) : listés, envoyables et retirables, mais sans aperçu (pas de lecteur
  vidéo dans le sélecteur) ;
- **`GET /gifs/suggest` et `/gifs/trending-search`** (suggestions de mots) :
  la source n'en donne pas la forme de réponse, ils ne sont pas appelés ;
- **Animation des super réactions** : la pastille seule est affichée.

### À vérifier sur un vrai compte

- que `/gifs/*` renvoie toujours des médias de `media.tenor.com` : si Discord
  a changé de fournisseur, les aperçus seront refusés par la règle des hôtes
  (cadre vide), et il faudra y ajouter le nouveau domaine ;
- la réponse réelle d'un `PATCH settings-proto/2` (le code accepte une
  réponse avec ou sans `settings`) et la reprise des favoris écrits ici par
  le client officiel (format image) ;
- les clés de `favorite_emojis` et d'`emoji_frecency` pour un emoji Unicode
  (nom court supposé, d'après la source) ;
- le code d'erreur d'une super réaction sans Nitro ou sans « burst »
  disponible, et son écho de passerelle.

## Lot VID : vidéos, images, liens externes

Aucune requête à l'API. Seuls des fichiers servis par Discord sont demandés,
sans jeton (client HTTP du CDN ou des sons, `User-Agent` seul).

| Requête | Quand | Documentée |
|---|---|---|
| `GET https://media.discordapp.net/attachments/…/video.mp4?…&format=jpeg&width=…&height=…` | affiche d'une vidéo jointe visible (sa première image), comme une vignette d'image : même cache, même réglage « charger les médias » ; jamais pour un spoiler | non : paramètre `format` du proxy média. Forme relevée dans une source publique : `https://github.com/dannycake/discord-resolution-vuln` (README, lu le 15 novembre 2026), qui montre la vignette d'une vidéo jointe à l'adresse `https://media.discordapp.net/attachments/…/output.mp4?format=jpeg&width=400&height=225` ; en cas de refus, la case reste sombre avec son bouton de lecture |
| `GET https://media.discordapp.net/attachments/…/image.jpg?…&width=…&height=…&format=webp&quality=lossless` (de même sur `images-ext-N.discordapp.net`) | vignette ou image agrandie d'un message visible : la même demande qu'avant, avec deux paramètres de plus pour que le proxy ne recompresse pas l'image (JPEG, PNG : `format=webp&quality=lossless` ; WebP : `quality=lossless` seul ; GIF, extension inconnue ou `format` déjà présent : rien). La taille demandée est arrondie au palier supérieur (32, 64 ou 128 pixels) ; zoomée dans la visionneuse, l'image est redemandée à la taille de la source (12 Mio de texture au plus) | en partie : `quality` (`lossless`, `high`, `low`, « Only supported with WebP and JPG formats ») et `format` sont décrits par `https://docs.discord.food/reference` (« CDN Formats », lu le 15 novembre 2026) ; leur emploi sur le proxy média est la forme des liens d'images que le client officiel copie (`…&format=webp&quality=lossless&width=…&height=…`). Non vérifié face au vrai proxy (aucun test ne contacte Discord) : en cas de refus, la vignette reste sur son fond d'attente |
| `GET` de `video.proxy_url` d'un embed `video` ou `gifv` (hôtes `cdn.discordapp.com`, `media.discordapp.net`, `images-ext-N.discordapp.net` seulement, 64 Mio au plus) | clic sur la vignette : lecture sur place (lien direct vers un fichier vidéo, GIF de Tenor en MP4 lu en boucle et sans son) ; jamais avant le clic, un seul fichier à la fois | oui (objet Embed, « Embed Video Structure » : `proxy_url`) |
| `GET` du fichier d'une image (pièce jointe : adresse d'origine du CDN ; image d'embed : copie du proxy) | « Copier l'image » ou « Enregistrer l'image » (visionneuse, clic droit) ; 10 Mio au plus | oui (CDN) |

Le site d'origine d'une vidéo ou d'une image d'embed n'est jamais contacté :
sans `proxy_url` sur un hôte de Discord (YouTube, Twitch…), la vignette garde
un bouton de lecture qui ouvre le navigateur.

Liens externes : avant d'ouvrir le navigateur, une fenêtre montre l'adresse
entière (nom international en forme ASCII) et demande confirmation, sauf pour
les domaines de Discord et ceux que l'utilisateur a cochés « de confiance »
(fichier local `domaines-de-confiance.txt`, jamais envoyé).

### À vérifier sur un vrai compte

- l'affiche : que `format=jpeg` rend bien la première image pour un MP4, un
  MOV et un WebM joints (sinon la case reste sombre, sans erreur visible) ;
- un WebM VP9 + Opus réel et un WebM VP8 + Vorbis (les essais fabriquent du
  VP8 + Opus ; le décodeur VP9 de Windows est présent sur la machine d'essai
  mais n'a lu aucun vrai flux) ;
- un GIF de Tenor (`gifv`) et un lien direct vers un MP4 : que `video.proxy_url`
  est bien sur `images-ext-N.discordapp.net` et que le fichier s'y télécharge.

## Lot R : restes (galerie, invitation dans le MP, images Rich Presence nommées)

Sources (15 novembre 2026) : docs.discord.com/developers (Get Channel
Messages, Create Channel Invite, Create Message, Modify Current User) ;
docs.discord.food, « Resources › Application », « List OAuth2 Application
Assets » et « Application OAuth2 Asset Object » (`id`, `type` déprécié,
`name`).

| Requête | Quand | Documentée |
|---|---|---|
| `GET /channels/{fil}/messages?after={fil-1}&limit=1` | galerie d'un forum ou d'un salon média affichée : message de départ (extrait, vignette) des cartes visibles seulement, une fois par publication, par le limiteur ; échec silencieux (la carte garde son pictogramme) | oui (Get Channel Messages ; le message de départ d'une publication porte l'identifiant du fil) |
| `POST /channels/{salon}/invites` (`max_age` 7 jours, `max_uses` 0) puis `POST /channels/{mp}/messages` avec le lien | « Inviter sur le serveur » du menu d'une personne : l'invitation est créée dans le premier salon qui le permet, son lien part dans le MP (ouvert par `POST /users/@me/channels` s'il n'existe pas ; abandon sans rien envoyer s'il n'arrive pas en 15 s) | oui |
| `PATCH /users/@me` avec `banner: null` | « Retirer » la bannière dans « Modifier le profil », permis sans Nitro (abonnement fini) ; la bannière ne peut toujours être changée qu'avec Nitro | oui |
| `GET /oauth2/applications/{application}/assets` | clic sur « Charger les images » d'une application Rich Presence branchée (page Activités) ; une fois par application et par connexion, 8 applications et 300 images gardées ; réponse `[{ "id", "type", "name" }]`, jeu d'essai `tests/fixtures/rest_application_assets.json` | non officielle : docs.discord.food (List OAuth2 Application Assets) |
| `GET /users/{id}/profile?…` (requête existante du profil, voir « Profil complet et notes ») | nouveau déclencheur : ouverture d'un MP à deux quand le panneau de profil de droite est affiché (réglage « liste des membres », bouton de l'en-tête pour le masquer) ; même cache de 5 min, une demande par personne ; la note (`GET /users/@me/notes/{id}`, existante) aussi | non (docs.discord.food, Get User Profile) |

Sans requête : couronne du propriétaire et icône de rôle (données déjà
reçues), emoji du statut personnalisé (réglages synchronisés existants ;
emojis personnalisés proposés seulement avec Nitro), lecture à voix haute
(synthèse vocale de Windows, rien ne quitte l'ordinateur), bouton
« Découvrir » (ouvre `https://discord.com/discovery` dans le navigateur),
bandeau d'une demande de message (mêmes écritures que la page des demandes).
Super réactions : proposées au seul Nitro complet (`premium_type` 2), plus à
Nitro Basic ni Classic.

Les images Rich Presence nommées ne sont pas résolues d'elles-mêmes quand un
programme les envoie : la règle « un geste de l'utilisateur » des requêtes
non officielles l'interdit. Le bouton « Charger les images » est ce geste.

### À vérifier sur un vrai compte

- `GET /oauth2/applications/{id}/assets` répond à un compte utilisateur pour
  une application qui n'est pas la sienne, et les identifiants traduits
  s'affichent chez les autres ;
- le message de départ d'une publication ancienne (créée avant que le fil
  et son premier message partagent leur identifiant) : `after={fil-1}` peut
  rendre un autre message, l'extrait serait alors celui de la première
  réponse ;
- retrait d'une bannière sur un compte sans Nitro (`banner: null` accepté) ;
- lien d'invitation envoyé dans un MP tout juste ouvert (délai de création).

## Effets de profil (lot ND, chaîne corrigée par le lot profil)

Liste complète des fonctions non documentées, avec la décision prise pour
chacune : `docs/non-documente.md`.

### Sources (lues le 15 novembre 2026)

- https://docs.discord.food/resources/user : « Profile Metadata Object »
  (`profile_effect` : `id`, `expires_at`), « Modify User Profile »
  (`profile_effect_id`, nul pour le retirer) ;
- https://docs.discord.food/resources/collectibles : « Get Collectibles
  Categories », « Get Collectibles Product » (par **SKU**), « List User
  Purchased Collectibles », « Profile Effect Structure » et « Profile
  Effect Animation Structure » ;
- https://www.npmjs.com/package/@discord-user-card/profile-effects : liste
  publique des effets, où chaque effet a un `id` **et** un `sku_id`
  différents (exemple : `id` 1139323097930027068, celui de l'exemple de
  discord.food, `sku_id` 1139323098370424932).

L'ancienne liste publique `GET /user-profile-effects` répond 404 : elle
n'est pas utilisée.

### Pourquoi l'effet ne se voyait pas

`profile_effect.id` est l'identifiant de **l'effet**, pas le SKU de son
produit. Le lot ND le passait tel quel à `GET /collectibles-products/{id}`,
qui attend un SKU : 404, effet tenu pour absent, carte normale. La chaîne
est désormais :

1. le profil donne un `sku_id` (forme lue si elle se présente) : le produit
   est demandé par ce SKU ;
2. sinon le produit est demandé par l'identifiant (cas des effets dont les
   deux identifiants coïncident) ;
3. s'il est inconnu, le catalogue (`GET /collectibles-categories`) est lu
   **une fois par session** pour relier l'identifiant de l'effet à son
   SKU ; si le catalogue donne déjà les couches, elles servent telles
   quelles, sinon le produit est demandé par ce SKU ;
4. sinon l'effet est tenu pour absent : la carte reste normale.

Formes de `profile_effect` lues (`net/profile.rs`, `effect_of`) : objet
`{ id, expires_at }` (date en secondes, millisecondes ou ISO ; expiré :
ignoré), objet `{ id, sku_id }`, objet `{ sku_id }` seul, identifiant nu
(chaîne ou nombre), ancien champ `profile_effect_id`.

### Ce que Ferrocord envoie

| Requête | Corps | Quand | Documentée |
|---|---|---|---|
| `GET /collectibles-products/{sku}` | aucun | un profil **ouvert** (carte, panneau de MP ou profil complet) porte un effet dont la configuration n'est pas connue ; au plus deux fois par effet et par session (par l'identifiant, puis par le SKU du catalogue ; 64 configurations gardées au plus) | non (discord.food, Get Collectibles Product) |
| `GET /collectibles-categories` | aucun | un profil **ouvert** porte un effet que le produit ne donne pas ; une fois par session (trois essais au plus après un échec, chacun à l'ouverture d'un profil ; 2 048 liens gardés au plus). La boutique en consultation lit la même liste (lot ND) | non (discord.food, Get Collectibles Categories) |
| `GET /users/@me/collectibles-purchases` | aucun | ouverture de « Modifier le profil » ; une fois par session, de nouveau après un échec | non (discord.food, List User Purchased Collectibles) |
| `PATCH /users/@me/profile` | `{ "profile_effect_id": "<id>" }` ou `null` | « Enregistrer » après avoir choisi une vignette ; `<id>` est l'identifiant de l'effet (`items[].id`), pas le SKU | non (discord.food, Modify User Profile) |

Toutes passent par le limiteur commun, sans en-tête du client officiel. Un
compte bot n'en envoie aucune. 404, 403 ou captcha sur un produit : la
chaîne passe à l'étape suivante, puis l'effet est tenu pour absent, rien
n'est redemandé ni retenté pour lui.

Fichiers du CDN : les couches (`effects[].src`), la vignette
(`thumbnailPreviewSrc`) et l'image fixe (`reducedMotionSrc`) sont des
adresses complètes ; seules celles en `https://cdn.discordapp.com/…` sont
gardées, et le téléchargement repasse par le contrôle d'hôte habituel.

### Lecture et mémoire

Un effet est fait d'une à quatre couches APNG de 450 × 880. Rien n'est
décodé d'avance (`ui/effects.rs`) : seul le profil ouvert a une scène ;
chaque couche est décodée en flux, une image à la fois, à la taille
affichée (200 000 pixels au plus) ; au plus 8 Mo compressés par effet ;
tout est libéré à l'image où le profil n'est plus dessiné. Avec « réduire
les animations » ou « lire les GIF » coupé, une seule image fixe est
décodée. Profil fermé : aucun fil, aucun réveil.

### Jeux d'essai

`tests/fixtures/nd/rest_collectibles_product.json`, `…/rest_collectibles_purchases.json` ;
`tests/fixtures/profil/profile_effect_forms.json` (chaque forme du champ),
`…/rest_collectibles_product_effect.json` (identifiant d'effet différent du
SKU), `…/rest_collectibles_categories_effects.json` (catalogue) ;
tests `net::effects`, `net::profile`, `state::effects`, `ui::effects`,
`tests/e2e/nd.rs` (chaîne entière sur 127.0.0.1).
Aperçu : `--scene effet`, `effet-fixe` (avec `--profile`), `profil-carte`
(effet trouvé par le catalogue), `profil-edition`.

### À vérifier sur un vrai compte

- le profil donne-t-il un `sku_id` à côté de `profile_effect.id` ? Sinon
  la chaîne passe par le catalogue (une requête de plus, une fois) ;
- les effets du catalogue portent-ils bien leur `id` (la documentation ne
  liste que `sku_id` ; la liste publique npm donne les deux) ? Sinon le
  lien identifiant → SKU ne se fait pas et l'effet reste invisible ;
- un effet retiré de la boutique (absent du catalogue) reste invisible ;
- `profile_effect_id` à l'écriture : identifiant de l'effet (et non SKU) ;
- un effet reçu par un abonnement Nitro (non acheté) apparaît-il dans les
  achats ? Sinon il reste porté et affiché, mais absent du choix ;
- `profile_effect_id` refusé sans Nitro : le refus est affiché tel quel ;
- poids réel des couches (plafond de 8 Mo) et cadence sur un vrai effet.

### Carte de profil (lot profil) : aucune requête de plus

La carte refaite (boutons de la bannière, bulle de statut, infobulles des
badges, amis en commun, comptes connectés, style du nom) ne lit que ce que
`GET /users/{id}/profile` et la passerelle donnent déjà. Les boutons
d'amitié de la bannière envoient les écritures existantes du lot U, au clic
sur la ligne du menu. Écartés faute de source publique : « Collection de
jeux » et « Liste de souhaits » (voir `docs/non-documente.md`).

## Quêtes et Orbes (lot ND)

### Sources (lues le 15 novembre 2026)

https://docs.discord.food/resources/quests (« Get User Quests », « Enroll
in Quest », « Claim Quest Reward », « Get Quest Reward Code », objets Quest,
Quest Config, Quest User Status) et https://docs.discord.food/topics/gateway-events
(`QUESTS_USER_STATUS_UPDATE`, `QUESTS_USER_COMPLETION_UPDATE`,
`VIRTUAL_CURRENCY_BALANCE_UPDATE`).

### Ce que Ferrocord envoie

| Requête | Corps | Quand | Documentée |
|---|---|---|---|
| `GET /quests/@me` | aucun | ouverture de la page « Quêtes » (liste absente ou vieille) ; bouton « Actualiser » ; une seule à la fois | non (discord.food, Get User Quests) |
| `POST /quests/{id}/enroll` | `{ "location": 11 }` (page des quêtes, bureau) | clic sur « Accepter la quête » | non (Enroll in Quest) |
| `POST /quests/{id}/claim-reward` | `{ "location": 11, "platform": 0 }` | clic sur « Réclamer la récompense », seulement si Discord dit la quête terminée | non (Claim Quest Reward) |
| `GET /quests/{id}/reward-code` | aucun | clic sur « Afficher le code » d'une récompense réclamée | non (Get Quest Reward Code) |

Reçu de la passerelle, sans requête : la progression
(`QUESTS_USER_STATUS_UPDATE`), le délai avant une nouvelle quête
(`QUESTS_USER_COMPLETION_UPDATE`) et le solde d'Orbes
(`VIRTUAL_CURRENCY_BALANCE_UPDATE`). Aucun point REST de solde n'est
appelé : le solde n'est affiché que si Discord l'annonce.

### Ce que Ferrocord n'envoie jamais

`POST /quests/{id}/heartbeat`, `POST /quests/{id}/video-progress`,
`POST /quests/{id}/console/start` et tout ce qui déclarerait une
progression. Ferrocord ne simule ni jeu, ni vidéo, ni processus : une
quête acceptée porte « Commencer la quête » (ou la progression reçue), qui ouvre
`https://discord.com/quests/{id}` ; la progression affichée est celle que
Discord renvoie. Les récompenses qui demandent de choisir une console se
réclament dans l'application officielle. Un test (`tests/e2e/nd.rs`)
vérifie qu'aucune de ces requêtes ne part.

Captcha demandé par une écriture : l'action s'arrête, la carte le dit, rien
n'est retenté. Compte bot : aucune requête.

### Page des quêtes (lot « quêtes », refonte) : aucune requête d'API nouvelle

La page reprend la disposition du client de bureau (grille de cartes de
même hauteur, tri, filtre, menu « … », fenêtre de détail). Tout s'applique
à la liste déjà reçue :

- tri (« Suggérées », « Les plus récentes », « Bientôt terminées ») et
  filtre (type de récompense, type de tâche) : locaux ;
- « Masquer cette quête » : local, pour la session ; le point
  `POST /quests/{id}/dismissible-content/…/dismiss` n'est **pas** appelé ;
- « Copier le lien de la quête » : presse-papiers ; « Ouvrir dans
  Discord », « Commencer la quête », « Réclamer dans Discord » : ouvrent
  `https://discord.com/quests/{id}` ;
- second bouton (« Jouer », « Rejoindre le serveur », « En savoir plus ») :
  ouvre le lien que la quête fournit (`cta_config.link`, à défaut
  `application.link`, déprécié), seulement s'il est en `https://`, par
  `App::open_link` (navigateur, avec confirmation hors des domaines de
  confiance ; une invitation suit le chemin ordinaire des invitations).
  **Rien n'est déclaré à Discord** : ni jeu lancé, ni progression.

### Images des quêtes (CDN, sans jeton)

Source : https://docs.discord.food/resources/quests (« Quest Assets
Structure » : `hero`, `hero_video`, `quest_bar_hero`,
`quest_bar_hero_video`, `game_tile`, `game_tile_light`, `game_tile_dark`,
`logotype`, `logotype_light`, `logotype_dark` ; « Quest Reward Structure » :
`asset`, `asset_video`) et https://docs.discord.food/reference (« CDN
Endpoints » : `/assets/quests/{quest_id}/{asset_name}` et
`/assets/quests/{quest_id}/{theme}/{asset_name}`, `theme` valant `dark` ou
`light`, pour `game_tile` et `logotype` seulement ; format d'origine,
paramètre `size` ignoré), relues le 15 novembre 2026.

Ce ne sont pas des requêtes d'API : `GET` sur `cdn.discordapp.com`, sans
jeton ni en-tête d'identification, par le chargeur d'images (limiteur et
cache du chargeur), pour les cartes visibles seulement — donc après le
geste qui a ouvert la page. `state::quests::asset_urls` construit, pour une
valeur reçue, les adresses à essayer dans l'ordre ; une adresse en échec
(404) laisse la place à la suivante :

| Valeur reçue | Adresses essayées |
|---|---|
| nom de fichier (`hero.jpg`) — forme documentée | `/assets/quests/{id}/hero.jpg`, puis `/quests/{id}/hero.jpg` |
| chemin complet (`quests/{id}/{fichier}`, `assets/quests/{id}/{fichier}`) | tel quel, puis sous l'autre racine |
| adresse entière du CDN (`https://cdn.discordapp.com/…`) | son chemin, relu comme ci-dessus |
| vidéo (`.mp4`, `.webm`, `.mov`, `.m3u8`) | jamais demandée : l'image fixe prévue la remplace (`quest_bar_hero` pour `hero`, vignette du jeu pour une récompense), sinon fond neutre |
| logotype, vignette du jeu | version du thème (`*_dark` / `*_light`), puis version ordinaire, puis version ordinaire dans le dossier du thème |

Seule la première ligne est documentée. Les formes « chemin complet »,
« adresse entière » et la racine `/quests/` ne figurent pas sur
docs.discord.food : elles sont prévues par précaution, parce que
l'ancienne construction (toute valeur recollée derrière
`/assets/quests/{id}/`) n'affichait aucune image sur un vrai compte et que
la forme réelle des valeurs n'a pas pu être relevée. Dans tous les cas
l'hôte est `cdn.discordapp.com`, le chemin ne contient que des lettres, des
chiffres, `_`, `-`, `.` et `/` (ni `..`, ni paramètres), et quatre adresses
au plus sont essayées par image.

### Jeux d'essai

`tests/fixtures/nd/rest_quests.json` (noms de fichiers),
`…/rest_quests_assets.json` (chemins complets, adresses entières, vidéos,
versions de thème, liens `cta_config`), `…/rest_quest_status.json` ; tests
`net::quests`, `state::quests`, `ui::quests`, `tests/e2e/nd.rs`. Aperçu :
`--view amis --scene quetes` (images générées localement pour chaque
forme ; la troisième quête n'y répond que sous la racine `/quests/`, pour
montrer le repli).

### À vérifier sur un vrai compte

- acceptation de ces requêtes sans en-têtes du client officiel (un refus
  est affiché, jamais contourné) ;
- `location: 11` accepté pour accepter et réclamer ;
- forme réelle des valeurs d'images (`assets.*`, `rewards[].asset`) et
  racine du CDN qui répond : si aucune des formes prévues ne correspond,
  la carte garde son fond neutre, le nom du jeu à la place du logotype et
  une icône à la place de la vignette ;
- taille des images d'ambiance (refusées au-delà de 4096 px de côté ou de
  10 Mo) ;
- `cta_config.link` : présent et utile pour « Jouer » / « Rejoindre le
  serveur » (sinon le bouton dit « Ouvrir dans Discord ») ;
- arrivée de `VIRTUAL_CURRENCY_BALANCE_UPDATE` sans `capabilities`.

## Choix du tag de serveur (lot ND)

Sources (lues le 15 novembre 2026) : https://docs.discord.food/resources/user,
« Set Guild Identity » et « Primary Guild Structure » ;
https://docs.discord.food/resources/guild, « Guild Identity Structure »
(`profile.tag`, `profile.badge`, présents dans les serveurs reçus par la
passerelle).

| Requête | Corps | Quand | Documentée |
|---|---|---|---|
| `PUT /users/@me/clan` | `{ "identity_guild_id": "<id>", "identity_enabled": true }` ; pour ne plus afficher de tag : `{ "identity_guild_id": null, "identity_enabled": false }` | clic sur un tag (ou sur « Aucun ») dans « Modifier le profil » ; une requête à la fois | non (discord.food, Set Guild Identity) |

Seuls les serveurs dont l'objet reçu porte un `profile.tag` sont proposés ;
sans aucun, la rangée n'est pas affichée et rien ne part. La réponse n'est
pas lue : le tag affiché est celui du serveur choisi, et Discord l'annonce
aussi par la passerelle. Captcha ou refus : dit sous la rangée, rien n'est
retenté. Compte bot : aucune requête. Jeu d'essai : `tests/e2e/nd.rs`
(`guild_tag_is_read_and_chosen_by_one_request`), `state::tags`, `net::tags`.
Aperçu : `--scene profil-edition`.

**À vérifier sur un vrai compte** : la place de `profile` dans les serveurs
du READY d'un compte utilisateur (racine ou `properties` : les deux sont
lus) ; un tag changé par les administrateurs après la connexion n'est pas
suivi (`GUILD_UPDATE` n'est pas relu pour ce champ) ; Discord peut exiger
de « réaffirmer » le tag après un changement.

## Boutique en consultation (lot ND)

Source (lue le 15 novembre 2026) : https://docs.discord.food/resources/collectibles,
« Get Collectibles Categories », « Collectible Category Object »,
« Collectible Product Object », « Item Assets Structure ».

| Requête | Corps | Quand | Documentée |
|---|---|---|---|
| `GET /collectibles-categories` | aucun, aucun paramètre | ouverture de la page « Boutique » (ligne de la colonne des MP) si la liste n'est pas connue ; bouton « Actualiser » ; une seule à la fois | non (discord.food, Get Collectibles Categories) |

Lu : nom et résumé des catégories ; nom, résumé, type et image fixe des
produits (`items[].assets.static_image_url`, ou `thumbnailPreviewSrc` pour
un effet). Au plus 40 catégories de 60 produits.

**Aucun achat.** Ferrocord n'envoie aucune requête de paiement, de panier,
de cadeau ou de dépense d'Orbes ; « Voir dans Discord » ouvre
`https://discord.com/shop` dans le navigateur. Le prix de base est affiché
à titre indicatif : clé `0` de `prices`, puis `country_prices.prices[]` (`amount`, `exponent`, `currency` ; `discord_orb` pour les Orbes), d'après https://docs.discord.food/resources/store, « Subscription Prices Structure ». Le prix réel (taxes, réduction Nitro) reste celui de Discord.
Compte bot :
aucune requête. Captcha : traité comme un refus, rien n'est retenté.

Jeu d'essai : `tests/fixtures/nd/rest_collectibles_categories.json` ; tests
`net::shop`, `state::shop`, `ui::shop`, `tests/e2e/nd.rs`. Aperçu :
`--view amis --scene boutique`.

**À vérifier sur un vrai compte** : la réponse sans paramètre (le client
officiel envoie `include_bundles`, `variants_return_style`, etc.) ; les
produits rendus en groupes de variantes (`items` vide : pas d'image) ; le
poids de la réponse (toute la boutique en une fois).

## Inventaire du compte, en lecture (lot ND)

Source (lue le 15 novembre 2026) : https://docs.discord.food/resources/entitlement,
« List User Entitlements », « List User Giftable Entitlements »,
« Entitlement Object ».

| Requête | Corps | Quand | Documentée |
|---|---|---|---|
| `GET /users/@me/entitlements?with_sku=true&exclude_ended=true` | aucun | ouverture de l'onglet « Inventaire » de la boutique si la liste manque ; « Actualiser » | non (discord.food, List User Entitlements) |
| `GET /users/@me/entitlements/gifts` | aucun | à la suite de la précédente, si elle a réussi | non (List User Giftable Entitlements) |

Lu : identifiant, produit (`sku_id`, `sku.name`), type regroupé (achat,
abonnement, cadeau reçu, récompense de quête, échange d'Orbes), dates de
début et de fin, `consumed` ; les droits `deleted` sont écartés ; 300 au
plus. **Lecture seule** : aucun cadeau n'est créé, offert ni accepté, aucun
droit n'est consommé. Compte bot : aucune requête. Captcha : traité comme
un refus. Jeu d'essai : `tests/fixtures/nd/rest_entitlements.json` ; tests
`net::inventory`, `state::inventory`, `tests/e2e/nd.rs`. Aperçu :
`--view amis --scene inventaire`.

**À vérifier sur un vrai compte** : `sku.name` est-il toujours joint avec
`with_sku=true` (sinon la ligne montre le numéro du produit) ; le volume de
la liste sur un vieux compte.

## Choix de la décoration d'avatar et de la plaque (lot ND)

Source (lue le 15 novembre 2026) : https://docs.discord.food/resources/user,
« Modify Current User » (`avatar_decoration_sku_id`, `nameplate_sku_id`,
nuls pour retirer ; aucun mot de passe n'est demandé pour ces champs) ;
https://docs.discord.food/resources/collectibles pour les éléments des
produits possédés (`asset`, `label`, `palette`).

| Requête | Corps | Quand | Documentée |
|---|---|---|---|
| `PATCH /users/@me` | `{ "avatar_decoration_sku_id": "<produit>" }` ou `null` | clic sur une décoration possédée (ou « Aucune ») dans « Modifier le profil » | non (discord.food, Modify Current User) |
| `PATCH /users/@me` | `{ "nameplate_sku_id": "<produit>" }` ou `null` | clic sur une plaque possédée (ou « Aucune ») | non (idem) |

Les objets proposés viennent de `GET /users/@me/collectibles-purchases`,
déjà demandé à l'ouverture de « Modifier le profil » pour les effets :
aucune lecture de plus. Une rangée sans objet possédé n'est pas affichée ;
un produit qui n'est pas dans les achats ne peut pas être envoyé. Une
écriture à la fois (commune avec le tag de serveur) ; captcha ou refus :
dit sous les rangées, rien n'est retenté. Compte bot : aucune requête.
Tests : `net::tags`, `state::tags`, `tests/e2e/nd.rs`
(`owned_decoration_and_nameplate_are_worn_by_one_request`).

**À vérifier sur un vrai compte** : une décoration ou une plaque reçue par
Nitro (non achetée) n'apparaît peut-être pas dans les achats ; elle reste
portée et affichée, mais « Aucune » paraît alors cochée.

## Suggestions d'amis (lot ND)

Source (lue le 15 novembre 2026) : https://docs.discord.food/resources/relationships,
« List Friend Suggestions », « Remove Friend Suggestion », « Friend
Suggestion Object » (`suggested_user`, `reasons[]` avec `platform` — ou
`platform_type` dans l'exemple — et `name`, `mutual_friends_count`).

| Requête | Corps | Quand | Documentée |
|---|---|---|---|
| `GET /friend-suggestions` | aucun | première ouverture de l'onglet « En attente » de la page Amis, une fois par session | non (discord.food) |
| `DELETE /friend-suggestions/{user}` | aucun | clic sur « Ignorer » | non (idem) |

« Ajouter » envoie la demande d'ami déjà consignée
(`PUT /users/@me/relationships/{user}`, corps `{}`) ; le champ facultatif
`from_friend_suggestion` n'est pas envoyé. Quatre suggestions au plus sont
montrées ; une personne déjà en relation avec le compte est retirée. Un
échec de lecture n'est pas retenté dans la session ; captcha : traité
comme un refus, rien n'est retenté. Aucune requête sur les contacts du
compte (synchronisation de carnet d'adresses : non faite). Compte bot :
aucune requête. Tests : `net::suggestions`, `state::suggestions`,
`tests/e2e/nd.rs` (`friend_suggestions_are_read_once_dismissed_and_added_by_a_click`).
Aperçu : `--scene suggestions`.

**À vérifier sur un vrai compte** : la liste est souvent vide (elle dépend
des comptes liés) ; la clé réelle de la plateforme (`platform` ou
`platform_type`).

## Découverte de serveurs (lot ND)

Source (lue le 15 novembre 2026) : https://docs.discord.food/resources/discovery,
« List Discoverable Guilds » (`guilds`, `total`, `limit`, `offset`),
« Search Discoverable Guilds » (liste d'objets), « List Discovery
Categories » (`id`, `name`, `is_primary`), « Discoverable Guild Object »
(`id`, `name`, `icon`, `description`, `vanity_url_code`,
`approximate_member_count`, `approximate_presence_count`).

| Requête | Quand | Documentée |
|---|---|---|
| `GET /discovery/categories?locale=<fr ou en-US>&primary_only=true` | ouverture de la page (boussole du rail), une fois par session | non (discord.food) |
| `GET /discoverable-guilds?limit=24[&categories=<id>]` | ouverture de la page ; clic sur une catégorie sans recherche | non (idem) |
| `GET /discoverable-guilds/search?query=<texte>&limit=24[&category_id=<id>]` | recherche validée (Entrée ou bouton), ou catégorie choisie avec une recherche | non (idem) |

Aucune requête par frappe ; une demande identique à celle affichée ne
repart pas ; une réponse dépassée est ignorée. Lecture seule : rejoindre
ne passe pas par ces routes. « Rejoindre… » préremplit la fenêtre
« Rejoindre un serveur » existante avec `vanity_url_code` (le clic final
et l'invitation sont ceux déjà consignés) ; sans adresse personnalisée,
« Voir la page » ouvre `https://discord.com/servers/<nom>-<id>` dans le
navigateur. Les icônes viennent du CDN (`/icons/{id}/{hash}.png`).
Captcha ou refus : dit dans la page, rien n'est retenté. Compte bot :
aucune requête. Tests : `net::discovery`, `state::discovery`,
`tests/e2e/nd.rs` (`discovery_is_read_on_open_and_searched_by_a_gesture`).
Aperçu : `--scene decouverte`.

**À vérifier sur un vrai compte** : l'enveloppe de la recherche (liste nue
selon la source, `{ guilds }` aussi lue) ; l'adresse de la page publique
d'un serveur sans adresse personnalisée ; la liste sans recherche ne
contient que des serveurs vérifiés ou partenaires (dit par la source).

## Résumés de salon (lot ND)

Source (lue le 15 novembre 2026) : https://docs.discord.food/resources/message,
« Get Conversation Summaries » (réponse `{ summaries }`, 50 au plus,
permission de lire l'historique) et « Conversation Summary Object »
(`id`, `topic`, `summ_short`, `people`, `unsafe`, `start_id`, `count`).

| Requête | Quand | Documentée |
|---|---|---|
| `GET /channels/{salon}/summaries` | clic sur « Résumés du salon » dans le menu d'un salon de serveur (une fois par salon), puis « Actualiser » | non (discord.food) |

Lecture seule : Ferrocord ne demande jamais à Discord de produire un
résumé et ne s'abonne à aucun événement de résumé. Les résumés marqués
`unsafe` ne sont pas montrés. Un échec n'est pas retenté tout seul ;
captcha : traité comme un refus. Les résumés de huit salons au plus sont
gardés en mémoire. Compte bot : aucune requête. Tests : `net::summaries`,
`state::summaries`, `tests/e2e/nd.rs`
(`channel_summaries_are_one_read_per_gesture`). Aperçu : `--scene resumes`.

**À vérifier sur un vrai compte** : la plupart des serveurs n'ont pas
activé les résumés (liste vide ou refus) ; l'entrée du menu est proposée
pour tout salon, faute de connaître la fonction du serveur à l'avance.

## Vérification des membres et demandes d'adhésion (lot ND)

Source (lue le 15 novembre 2026) : https://docs.discord.food/resources/guild,
« Member Verification Object », « Member Verification Form Field
Structure » (`response` : `true` pour TERMS, indice pour MULTIPLE_CHOICE,
texte sinon), « Get Guild Member Verification », « Get Current User Guild
Join Request », « Create Guild Join Request » (tous les champs du
formulaire, chacun avec sa réponse), « List Guild Join Requests »,
« Action Guild Join Request », « Guild Join Request Object ».

| Requête | Corps | Quand | Documentée |
|---|---|---|---|
| `GET /guilds/{id}/member-verification` | aucun | clic sur « Règles et vérification » dans le menu du serveur | non (discord.food) |
| `GET /guilds/{id}/requests/@me` | aucun | même clic, si le serveur a un formulaire | non (idem) |
| `PUT /guilds/{id}/requests/@me` | `{ "version": "<date>", "form_fields": [<champ reçu> + "response"] }` | clic sur « Envoyer », champs obligatoires remplis | non (idem) |
| `GET /guilds/{id}/requests?status=SUBMITTED&limit=25` | aucun | clic sur « Voir les demandes » ou « Actualiser » (droit d'expulser) | non (idem) |
| `PATCH /guilds/{id}/requests/id/{demande}` | `{ "action": "APPROVED" }` ou `{ "action": "REJECTED", "rejection_reason"?: "<motif>" }` | clic sur « Accepter » ou « Refuser la demande » | non (idem) |

Les champs sont renvoyés tels que reçus (clés inconnues comprises), sans
jamais reprendre une réponse venue du serveur ; les réponses sont celles
que l'utilisateur a saisies. Un envoi à la fois ; une demande déjà déposée
ou acceptée ne peut pas être renvoyée ; un champ d'un type inconnu rend
l'envoi impossible ici (« à terminer dans l'application officielle »).
Captcha ou refus : dit dans la fenêtre, rien n'est retenté. Côté
modération : une décision par clic, pas d'action groupée (`PATCH
/guilds/{id}/requests` n'est pas appelé), pas d'entretien. Compte bot :
aucune requête. Tests : `net::verification`, `state::verification`,
`tests/e2e/nd.rs` (`member_verification_is_read_then_answered_by_one_request`,
`join_requests_are_listed_and_decided_one_click_each`). Aperçu :
`--scene verification`.

**À vérifier sur un vrai compte** : Ferrocord ne sait pas à l'avance si le
compte est encore « en attente » sur un serveur (l'entrée du menu est
toujours proposée) ; la réponse de Discord à un envoi par un membre déjà
vérifié ; les demandes qui passent par un entretien.

## Répertoire d'applications (lot ND)

Source (lue le 15 novembre 2026) : https://docs.discord.food/resources/application-directory,
« List Application Directory Categories » (`[{ id, name }]`), « Search
Applications Directory » (`results[{ type, data }]`, `num_pages`,
`counts_by_category`, `load_id` ; `type` 1 = application), « Application
Directory Entry Object » (`short_description`, `guild_count`).

| Requête | Quand | Documentée |
|---|---|---|
| `GET /application-directory-static/categories?locale=<fr ou en-US>` | première ouverture de l'onglet « Applications » de la page « Découvrir », une fois par session | non (discord.food) |
| `GET /application-directory-static/search?[query=<texte>&]page=1&page_size=24[&category_id=<id>]&locale=<…>` | ouverture de l'onglet ; recherche validée (Entrée ou bouton) ; clic sur une catégorie | non (idem) |

Lecture seule, aucune requête par frappe ; une demande identique ne
repart pas, une réponse dépassée est ignorée. Les liens et bannières
promotionnelles (`type` 2 et 3) sont ignorés. Ajouter une application
n'est pas fait ici : « Voir la fiche » ouvre
`https://discord.com/application-directory/<id>` dans le navigateur
(l'autorisation OAuth2 reste celle de Discord). Icônes : CDN
(`/app-icons/{id}/{hash}.png`). Captcha ou refus : dit dans la page.
Compte bot : aucune requête. Tests : `net::appdir`, `state::appdir`,
`tests/e2e/nd.rs` (`app_directory_is_read_on_open_and_searched_by_a_gesture`).
Aperçu : `--scene repertoire`.

**À vérifier sur un vrai compte** : la présence de `directory_entry` dans
`data` (sinon la description longue est montrée, sans nombre de serveurs).

## Centre de sécurité : état du compte (lot ND)

Source (lue le 15 novembre 2026) : https://docs.discord.food/resources/safety-hub,
« Get Safety Hub » (`classifications`, `guild_classifications`,
`account_standing.state` de 100 à 500, `is_appeal_eligible`) et
« Classification Object » (`description`, `explainer_link`,
`actions[].descriptions`, `max_expiration_time`).

| Requête | Quand | Documentée |
|---|---|---|
| `GET /safety-hub/@me` | clic sur « Vérifier auprès de Discord » (Paramètres, Confidentialité et sécurité, section « État du compte ») | non (discord.food) |

Rien n'est lu sans ce clic, ni à la connexion ni à l'ouverture de la
page. Lecture seule : aucune route de recours, de vérification d'âge ou
de demande de révision n'est appelée ; l'interface renvoie vers
l'application officielle. Le lien d'explication n'est gardé que s'il est
en `https://` et passe par la confirmation des liens externes. Captcha ou
refus : dit dans la section. Compte bot : aucune requête. Tests :
`net::safety`, `state::safety`, `tests/e2e/nd.rs`
(`safety_hub_is_one_read_per_click`). Aperçu : `--scene etat-compte`.

**À vérifier sur un vrai compte** : la forme réelle d'un compte sans
infraction (listes vides attendues) et celle de `actions[].descriptions`.

## Messages enregistrés et rappels (lot ND)

Source (lue le 15 novembre 2026) : https://docs.discord.food/resources/user,
« List Saved Messages » (`results[{ message, save_data }]`, `save_data`
avec `channel_id`, `message_id`, `guild_id?`, `saved_at`, `due_at`),
« Save Message » (corps `due_at?`, date ISO 8601 ou nul ; 200 messages
sans rappel au plus), « Unsave Message » (204). L'ancienne adresse
`/users/@me/message-reminders` n'est décrite nulle part : elle n'est pas
utilisée.

| Requête | Corps | Quand | Documentée |
|---|---|---|---|
| `PUT /users/@me/saved-messages/{salon}/{message}` | `{ "due_at": null }` ou `{ "due_at": "<date>" }` | clic sur « Enregistrer le message », « Me le rappeler dans 1 heure » ou « Me le rappeler demain » (menu d'un message) | non (discord.food) |
| `GET /users/@me/saved-messages` | aucun | ouverture de l'onglet « Enregistrés » de la boîte de réception (si la liste n'est pas connue), puis « Actualiser » | non (idem) |
| `DELETE /users/@me/saved-messages/{salon}/{message}` | aucun | clic sur « Retirer » | non (idem) |

Une écriture à la fois ; après un enregistrement, la liste n'est relue
qu'à la prochaine ouverture de l'onglet. C'est Discord qui tient
l'échéance côté serveur, et Ferrocord ne lit pas les événements
`SAVED_MESSAGE_*` de la passerelle ; un rappel échu est marqué dans l'onglet.
Depuis le lot FF2, les rappels connus (posés dans la session ou lus à
l'ouverture de l'onglet) donnent une notification locale à l'échéance : un
seul réveil programmé pour le prochain, aucune minuterie périodique, aucune
requête. Captcha ou refus : annoncé par un message bref, rien n'est
retenté. Compte bot : aucune requête. Tests : `net::saved`,
`state::saved`, `tests/e2e/nd.rs`
(`saved_messages_are_written_and_listed_one_request_per_gesture`).
Aperçu : `--scene enregistres`.

**À vérifier sur un vrai compte** : que la fonction est ouverte au compte
(elle a été réservée à des groupes d'essai) ; la réponse exacte de
« Save Message » ; le rappel n'est pas notifié par Ferrocord à l'échéance.

## Manques fonctionnels de la vérification finale (lot FF)

Aucune requête nouvelle hors documentation officielle. Ce que le lot envoie
ou lit de plus :

| Requête ou champ | Corps | Quand | Documentée |
|---|---|---|---|
| op 3 (Update Presence) : `status: "idle"`, `since` (ms), `afk: true` | les mêmes champs qu'avant ; seuls ces trois changent | après 10 min sans clavier ni souris (horloge de dernière saisie de Windows, lue toutes les 20 s hors du fil de l'interface), et au retour ; jamais si le statut choisi n'est pas « en ligne » ; même limiteur (4 s entre deux op 3) | oui (Gateway, Update Presence) |
| `POST /channels/{id}/threads` | `{ name, type: 11 }` (10 dans un salon d'annonces) | « Créer un fil » du menu « + » de la saisie, puis « Créer » ; un geste, une requête | oui (Start Thread without Message) |
| `POST /channels/{id}/messages` : `tts: true` | préfixe `/tts ` retiré du texte | envoi d'un message commençant par `/tts ` | oui (Create Message, `tts`) |
| `POST /channels/{id}/messages` : `flags: 4096` | préfixe `@silent ` retiré du texte | envoi d'un message commençant par `@silent ` | oui (Create Message, `flags`, SUPPRESS_NOTIFICATIONS) |
| `PATCH /guilds/{id}/members/@me` | `{ nick }` | commande `/nick` (même requête que la page « Profils ») | oui (Modify Current Member) |

Lu sans requête, dans ce qui est déjà reçu :

- `nsfw` de l'objet Channel (READY, GUILD_CREATE, CHANNEL_CREATE,
  CHANNEL_UPDATE ; documenté) : écran d'entrée d'un salon à limite d'âge. Le
  choix « Continuer » vaut pour la session et n'est envoyé nulle part.
- `flags` d'une pièce jointe, bit 4 (`CONTAINS_EXPLICIT_MEDIA`,
  docs.discord.food, « Attachment Flags », lu le 15 novembre 2026 ; non
  documenté officiellement) : le média est masqué selon le réglage du compte
  « Filtre de contenu explicite ». Lecture seule, rien n'est envoyé.
- `flags` d'un message, bit 12 (`SUPPRESS_NOTIFICATIONS`, documenté) : cloche
  barrée après l'heure, pas de notification de bureau.

Restent locaux, jamais envoyés : l'usage des emojis (fichier
`emojis-<compte>.txt` du dossier de configuration ; la fréquence du compte
n'est toujours pas écrite), les sons coupés (`notify.toml`), les messages
bloqués dépliés, la liste des membres d'un rôle mentionné (membres déjà
connus, aucune demande à la passerelle).

**À vérifier sur un vrai compte** : le passage à « inactif » vu par les
autres et son effet sur les notifications mobiles (`afk: true`) ; le fil
créé sans message de départ dans un salon d'annonces ; la présence réelle
du bit 4 sur les pièces jointes analysées.

## Seconde passe sur les manques fonctionnels (lot FF2)

Source des lignes non documentées officiellement : docs.discord.food, lu le
15 novembre 2026. Chaque requête part d'un geste de l'utilisateur, passe par
le limiteur commun, et sa forme est testée dans `src/net`.

| Requête | Corps ou paramètres | Geste | Documentée |
| --- | --- | --- | --- |
| `POST /users/@me/relationships` : `note` | `{ username, discriminator: null, note }`, `note` absent si la zone est vide, coupé à 120 caractères | bouton « Envoyer une demande d'ami » de l'onglet « Ajouter », zone « Personnalise ta demande » remplie | non (docs.discord.food, « Send Friend Request ») ; test `net::relations_rest` |

Un captcha demandé par Discord arrête la demande : le refus s'affiche sous la
carte, rien n'est retenté. L'onglet « Implicite » de Discord (relation de
type 5, « affinité ») n'est pas repris : Ferrocord range ce type dans
« autre » sans le distinguer.

Autres requêtes du lot, toutes sur un geste :

| Requête | Corps ou paramètres | Geste | Documentée |
| --- | --- | --- | --- |
| `PATCH /users/@me/guilds/{id}/settings` | `notify_highlights` (1 coupées, 2 actives), `mute_scheduled_events`, `mobile_push`, un champ par requête | interrupteurs de la fenêtre de notifications du serveur (F12) | non (docs.discord.food, « User Guild Settings ») ; test `net::account` |
| `PATCH /channels/{fil}` | `{ locked }` | « Verrouiller » ou « Déverrouiller » du menu d'un fil (F13) | oui (Modify Channel, fil) |
| `POST /channels/{id}/threads` | `{ name, type: 12, invitable: true }` | case « Fil privé » de la création d'un fil (F13) | oui (Start Thread without Message) |
| `GET /channels/{fil}/thread-members` | `with_member=true&limit=100` | ouverture de « Membres du fil », une lecture par ouverture (F13) | oui (List Thread Members) |
| `GET …/messages/search` | filtre `has` ajouté (`image`, `video`, `link`, `file`) | clic sur un onglet des résultats (F16) | non (requête de recherche déjà utilisée, même forme) |
| `PATCH /channels/{groupe}` | `{ icon }` (image `data:` de 128 × 128) | « Changer l'icône » du panneau du groupe (F20) | oui (Modify Channel, Group DM) |
| `PATCH /channels/{groupe}` | `{ owner }` | « Transférer la propriété du groupe », confirmé (F20) | non (docs.discord.food, « Modify Channel », Group DM) ; test `net::relations_rest` |
| `POST /channels/{groupe}/invites` | `{ max_age: 86400 }` | « Lien d'invitation » du panneau du groupe (F20) | non pour un groupe (docs.discord.food, « Create Channel Invite » : seul `max_age`) ; test `net::guilds` |
| `POST /channels/{id}/messages` | message ordinaire | Entrée dans le champ de la carte de profil (F24) ; la conversation est d'abord ouverte par `POST /users/@me/channels` si elle n'existe pas | oui |
| `PATCH /guilds/{id}/members/{user}` : `channel_id` | déplacement vocal | membre glissé sur un autre salon vocal, puis confirmation (F21) | oui (Modify Guild Member, même requête que le menu) |

Lu sans requête, dans ce qui est déjà reçu :

- `message_author_id` et `member` de MESSAGE_REACTION_ADD (documentés) :
  notification d'une réaction à l'un de mes messages (F11), jamais pour le
  salon affiché ni pour un salon muet.
- `tts` d'un message (documenté) : lu par la synthèse vocale de Windows dans
  le salon affiché, si le réglage du compte « Commande /tts » le demande
  (F27). Rien n'est envoyé.

Restent locaux : les recherches récentes (mémoire de la session), les
domaines de confiance (fichier du dossier de configuration), le volume par
personne, le débit de la voix de synthèse.

Non fait faute de source publique : la recherche dans tous les MP à la fois,
l'onglet « Implicite » des amis.

**À vérifier sur un vrai compte** : `note` d'une demande d'ami (vue par le
destinataire), `owner` et l'invitation d'un groupe, les trois réglages de
notification du serveur, le filtre `has` répété quand la requête en porte
déjà un.
## Éditeur de profil : style du nom et widgets de jeux (lot éditeur)

La fenêtre « Modifier le profil » (`ui/profile_edit*.rs`) garde tout dans
un brouillon : rien ne part avant « Enregistrer » (barre du bas). Les
écritures partent alors une à la fois, dans l'ordre : profil
(`PATCH /users/@me/profile`), pseudo, profil du serveur, images, tag de
serveur (`PUT /users/@me/clan`), décoration, plaque, style du nom
(`PATCH /users/@me`), widgets. Le tag, la décoration et la plaque ne
partent donc plus au clic (sections « Choix du tag de serveur » et « Choix
de la décoration d'avatar et de la plaque » ci-dessus : mêmes requêtes,
mêmes corps), mais à « Enregistrer ». Un refus arrête la suite et s'affiche
dans la barre ; un captcha arrête tout et le dit ; rien n'est retenté.

Sources (lues le 15 novembre 2026) :

- https://docs.discord.food/resources/user, « Modify Current User » :
  `display_name_font_id`, `display_name_effect_id` (entiers annulables),
  `display_name_colors` (liste annulable d'entiers 0xRRGGBB, **deux au
  plus**), tous trois réservés aux abonnés ; « Display Name Font »
  (11 gg Sans, 3 Sakura, 4 Jellybean, 6 Modern, 7 Medieval, 8 8Bit,
  10 Vampyre, 12 Tempo, 13 Monkey Bars, 14 Mainframe, 15 Headbang,
  16 Journal) ; « Display Name Effect » (1 uni, 2 dégradé, 3 néon, 4 toon,
  5 pop, 6 lueur : les seules valeurs décrites à l'envoi ; 7 prisme et
  8 « gummy » ne sont décrits qu'en lecture : le lot profil les rend, le
  sélecteur ne les propose pas et `Store::set_name_style` refuse de les
  envoyer tant qu'aucune source publique ne donne leur valeur à l'écriture,
  `state::cosmetics::NameEffect::writable`) ; « Get User Profile » : champ `widgets` ;
- https://docs.discord.food/resources/widgets : « Game Widget Object »
  (`id`, `updated_at`, `data` : `type`, `games`, `application_id`), « Game
  Widget Types » (`favorite_games` 1 jeu, `played_games` 20,
  `current_games` 5, `want_to_play_games` 20, `application`), « Game Widget
  Game Structure » (`game_id`, `comment`, `tags`), `PUT /users/@me/widgets`
  (un widget par type, `id` facultatif, `updated_at` ignoré ; réponse : la
  liste gardée), `GET /users/@me/widgets/suggested-games`
  (`suggested_games`, `suggested_wishlist_games`) ;
- https://docs.discord.food/resources/game : « Get Games »
  (`GET /games`, `game_ids` de 1 à 25 identifiants ; `id`, `name`,
  `icon_hash`, `cover_image_hash`) ;
- documentation officielle, « Image Formatting » : Application Icon et
  Application Cover (`app-icons/{application_id}/{hash}.png`).

| Requête | Corps ou paramètres | Quand | Documentée |
|---|---|---|---|
| `PATCH /users/@me` | `{ "display_name_font_id": 11, "display_name_effect_id": 2, "display_name_colors": [16711680, 255] }` ; style retiré : les trois à `null` | « Enregistrer » de l'éditeur, si le style du nom a changé (sélecteur « Style du pseudonyme ») ; `display_name_effect_id` de 1 à 6 seulement (jamais 7 ni 8) | non (discord.food, Modify Current User) |
| `GET /games?game_ids=<id>&game_ids=<id>…` | 25 identifiants au plus par requête | ouverture de l'éditeur de profil : les jeux de mes widgets dont le nom n'est pas connu ; ouverture du choix d'un jeu : les jeux suggérés. Une fois par jeu et par session (512 jeux gardés au plus) ; un jeu resté sans réponse n'est redemandé qu'à l'ouverture suivante de l'éditeur | non (discord.food, Get Games) |
| `GET /users/@me/widgets/suggested-games` | aucun | ouverture du choix d'un jeu (« + » d'un widget) ; une fois par session, de nouveau après un échec | non (discord.food, widgets) |
| `PUT /users/@me/widgets` | `{ "widgets": [ { "id": "<id>", "data": { "type": "favorite_games", "games": [ { "game_id": "<id>", "comment": "…", "tags": ["expert"] } ] } }, { "data": { "type": "application", "application_id": "<id>" } } ] }` (`id` absent pour un widget créé ici) | « Enregistrer » de l'éditeur, si les widgets ont changé ; une écriture à la fois | non (discord.food, widgets) |

Lecture : aucune requête de plus, les widgets viennent du champ `widgets`
de mon profil complet (`GET /users/{id}/profile`, déjà demandé à
l'ouverture). Un widget de type inconnu (ou un doublon de type) rend la
liste **non modifiable** : la réécrire le perdrait ; elle est alors montrée
en lecture seule. Le widget `application` est gardé et renvoyé tel quel,
sans être affiché. Les étiquettes (`tags`) d'un jeu sont gardées et
renvoyées, pas modifiables ici. Un widget de jeux vidé n'est pas envoyé.
Bornes locales : commentaire de 140 caractères (la source n'en donne pas),
16 étiquettes par jeu.

**Recherche d'un jeu** : la source ne décrit aucune recherche par nom. Le
choix propose les suggestions de Discord et filtre sur place la liste des
jeux détectables (`GET /games/detectable`, déjà chargée quand l'activité
de jeu est activée) : aucune requête de recherche n'est envoyée.

Écartés, faute de source publique (règle 1) : l'onglet « Activité » de
l'éditeur officiel (`content-inventory`), le « cadre » de profil (aucune
écriture décrite), une liste de souhaits de la boutique (l'onglet « Liste
de souhaits » est le widget `want_to_play_games`, seul décrit). Les
polices du nom ne sont pas embarquées : le choix part, le nom reste
dessiné avec la police de Ferrocord. Aucun achat : ce que le compte n'a
pas le droit d'utiliser est verrouillé, « Voir dans Discord » ouvre le
navigateur (`https://discord.com/shop`, `https://discord.com/nitro`).

Compte bot : aucune de ces requêtes. Jeux d'essai :
`tests/fixtures/nd/rest_widgets.json`, `rest_games.json`,
`rest_widgets_suggested.json` ; tests `net::widgets`, `net::tags`,
`state::widgets`, `ui::profile_edit::tests`, `tests/e2e/nd.rs`
(`profile_widgets_are_read_named_and_replaced`,
`name_style_is_written_by_one_request`). Aperçu : `--scene profil-edition`.

**À vérifier sur un vrai compte** : la forme exacte acceptée par
`PUT /users/@me/widgets` (un widget vidé doit-il être envoyé vide ou
omis ; `tags: []` et `comment: null` sont-ils acceptés) ; le paramètre
répété `game_ids` (ou une liste séparée par des virgules) ; la présence de
`cover_image_hash` pour les jeux courants ; le retrait du style du nom par
trois `null` ; l'accord de Discord pour un style sans abonnement (refus
attendu, affiché dans la barre) ; l'ordre des cartes de l'onglet
« Tableau » dans le client officiel.

## Mises à jour de Ferrocord (lot distrib) : aucune requête vers Discord

Ces requêtes ne concernent pas Discord : elles vont à l'adresse de mise à
jour, c'est-à-dire aux « Releases » du dépôt GitHub public nommé dans
`src/install/update/depot.txt` (ou à l'adresse écrite dans `local.toml`,
section `[mises_a_jour]`). Sans l'un ni l'autre, ou sans clé publique
intégrée, **aucune** n'est faite. Code : `src/install/update/fetch.rs`,
rythme : `src/install/service.rs`.

| Requête | Quand | Ce qui part |
|---|---|---|
| `GET https://github.com/<propriétaire>/<dépôt>/releases/latest/download/ferrocord-manifeste.json` | au lancement puis une fois par jour au plus si « Vérifier automatiquement » est activé, jamais à moins d'une heure de la précédente (heure gardée dans `installation.json`) ; ou clic sur « Rechercher maintenant » | `User-Agent: Ferrocord/<version>`, `Accept: */*`, rien d'autre |
| `GET` de l'adresse de l'exécutable nommée par le manifeste signé (`…/releases/download/v<version>/Ferrocord-<version>-x64.exe`) | clic sur « Télécharger et vérifier » seulement | idem |

- **Jamais de jeton** (ni celui de Discord, ni un jeton GitHub : le dépôt est
  public), pas de témoin, pas de `Referer`, aucun identifiant de
  l'utilisateur, du compte ou de la machine, aucun paramètre dans l'adresse.
  Ce client HTTP est distinct de celui de l'API de Discord et n'en partage
  aucun en-tête.
- **Redirections** : GitHub renvoie vers son stockage de fichiers. Elles ne
  sont suivies qu'en HTTPS, port 443, vers `github.com`,
  `objects.githubusercontent.com` ou `release-assets.githubusercontent.com`
  (`fetch::REDIRECT_HOSTS`), ou vers l'hôte même qui redirige ; cinq au
  plus. Toute autre est refusée avant la moindre connexion.
- **Ce qui est lu** : le manifeste (64 Kio au plus) n'est cru qu'après
  vérification de sa signature Ed25519 par la clé publique intégrée ; il
  doit être pour Ferrocord, pour le canal « stable », et d'une version plus
  récente que celle installée. L'exécutable (256 Mio au plus) doit avoir la
  taille et le SHA-256 du manifeste, revérifiés au moment du remplacement.
- GitHub voit donc l'adresse IP, la version de Ferrocord (dans le
  `User-Agent`) et l'heure, comme pour tout téléchargement.

Essais : `install::update::bout_en_bout` (faux serveur sur 127.0.0.1 : les
en-têtes reçus y sont vérifiés un par un).

## Activité détectée : détection sans ouvrir de processus (lot « activité »)

Complète « Activité de jeu détectée et présence enrichie (lot C3) » plus
haut ; ce qui suit remplace ce que cette section dit du relevé.

### Sources (relues le 15 novembre 2026)

- docs.discord.food, page « Game » : « List Detectable Games »,
  `GET /games/detectable`, sans paramètre ; objet « Detectable Application »
  (`id`, `name`, `icon_hash`, `aliases`, `executables[]`, `themes`, `hook`,
  `overlay`…) ; « Application Executable » : `os`, `name`, `is_launcher`.
- docs.discord.food, page « Reference », « CDN Endpoints » : icône d'une
  application, `app-icons/{application_id}/{application_icon}.png`.
- docs.discord.food, page « Gateway Events », « Update Presence » (op 3) :
  inchangé, 5 mises à jour par 20 s au plus.

### Ce qui part chez Discord

| Quand | Quoi | Rythme |
|---|---|---|
| Connexion, « Partager l'activité détectée avec les autres » actif, cache absent, de plus d'un jour ou d'une forme plus ancienne | `GET /games/detectable` (inchangée) | 60 s après la connexion, au plus une fois par jour ; limiteur commun. Un cache périmé sert tout de suite, sans attendre la réponse |
| Encart du jeu détecté affiché | image `app-icons/{id}/{icon_hash}` sur le CDN | une fois, puis cache des images |
| Le jeu annoncé change (début, fin, autre jeu au premier plan, jeu masqué) | op 3 : statut personnalisé (type 4) puis le jeu (type 0, `name`, `application_id`, `timestamps.start` = premier relevé où il tournait) | un envoi par changement ; 4 s au moins entre deux op 3 |

Aucune autre requête : ni télémétrie, ni événement « science », ni
`/games/detectable/exclusions`, ni `/applications/non-games/detectable`.
Un jeu masqué (Paramètres › Activités › « Jeux vus récemment ») n'est
jamais envoyé ; la liste des jeux vus et leurs masques restent dans
`local.toml` (section `[jeux]`, clé `recents`).

### Sur cet ordinateur

- Relevé toutes les 10 s, sur un fil d'arrière-plan : un instantané
  ToolHelp (identifiant et nom de fichier de chaque programme) et le
  processus de la fenêtre de premier plan (`GetForegroundWindow`,
  `GetWindowThreadProcessId`). Aucun processus n'est ouvert ni lu : le
  relevé précédent ouvrait chaque candidat pour lire son chemin, ce qu'un
  anti-triche ou un programme lancé en administrateur refuse — le jeu
  n'était alors jamais reconnu.
- Conséquence : la correspondance se fait sur le nom de fichier seul. Un
  motif sans dossier (`jeu.exe`) correspond tel quel ; un motif à dossier
  (`win64/jeu.exe`) n'est accepté que si une seule application de la liste
  revendique ce nom de fichier ; les lanceurs (`is_launcher`), les
  exécutables d'un autre système et ceux qui exigent des arguments sont
  ignorés. L'heure de début est celle de la détection, pas celle du
  lancement du processus.
- Plusieurs jeux : celui qui a eu le premier plan le plus récemment, puis
  le dernier lancé.
- Rien n'est signalé à l'interface tant que les jeux ne changent pas : aucun
  redessin au repos. Avec un jeu en cours, l'encart demande une image par
  minute (le temps écoulé).

Essais : `net::games::tests` (cause du défaut, noms d'exécutables, premier
plan, coût), `state::activity::tests` (masques, présence), `ui::game_card`
(encart, redessins), `tests/e2e/activity.rs` (op 3 face au faux serveur).

## Fiabilité et cohérence (lot « fiable »)

Audit complet, état avant et tests : `docs/fiabilite.md`. Ce qui suit ne
consigne que ce qui change **sur le fil**. Aucune requête ni aucun champ
nouveau n'est envoyé à Discord par ce lot.

### Sources (lues le 15 novembre 2026)

- docs.discord.food, « Gateway » (connexion, heartbeat, reprise, codes de
  fermeture, `capabilities`, propriétés du client, `has_client_mods`,
  `launch_signature`) et « Gateway Events » (exemple d'Identify) ;
- documentation officielle des développeurs, « Gateway » : attente aléatoire
  de 1 à 5 s après Invalid Session, premier heartbeat à
  `heartbeat_interval × jitter`, codes de fermeture. L'attente de 1 à 5 s
  ne figure pas sur docs.discord.food : elle vient de la doc officielle
  seule.

### Passerelle

| Situation | Ce que Ferrocord fait désormais |
|---|---|
| Premier heartbeat | à `heartbeat_interval × aléa(0..1)`, puis toutes les `heartbeat_interval` (avant : toujours à la moitié de l'intervalle, donc au même instant à chaque connexion) ; un battement manqué ne se rattrape plus en rafale (`MissedTickBehavior::Delay`) |
| Heartbeat sans accusé (connexion zombie), op 7, saut d'horloge (veille de l'ordinateur) | fermeture avec le code **4000** (toute fermeture autre que 1000 et 1001 garde la session reprenable), puis RESUME sur `resume_gateway_url` |
| Op 9 (Invalid Session) | attente aléatoire de **1 à 5 s**, puis fermeture 4000 et RESUME si `d: true`, fermeture **1000** et Identify neuf sinon |
| Codes 4008 (trop de commandes) et 4015 | session gardée, attente d'une fenêtre entière (60 s) avant de revenir |
| Codes 4004, 4010 à 4014 | arrêt définitif, aucune reconnexion (inchangé) |
| Reconnexion | recul exponentiel (1 s, 2 s, 4 s… plafond 60 s) avec gigue (entre la moitié et la totalité du délai) ; le compteur ne revient à zéro qu'après 30 s de session établie, plus à chaque READY |
| RESUME refusé 3 fois de suite | Identify neuf (plus de reprise en boucle) |
| Adresse de reprise injoignable 3 fois de suite | retour à l'adresse de passerelle par défaut |
| Connexion, attente du Hello, envoi d'une trame | 20 s au plus chacun |
| Identify | toujours par `gwlimit` : 5 s entre deux, 1000 par jour (inchangé) |

Codes de fermeture **envoyés** par Ferrocord : 1000 (session abandonnée) et
4000 (session à reprendre) seulement.

### REST

- **Nouveaux essais** : seulement pour les lectures (GET, HEAD), après une
  coupure réseau (pas un dépassement de délai) ou un 500, 502, 503, 504 ;
  deux au plus, après 0,5 s puis 1 s environ (gigue, plafond 4 s),
  annulables. Une écriture n'est **jamais** renvoyée d'elle-même (l'envoi de
  message garde son propre mécanisme à `nonce`, voir plus haut). 401, 403 et
  404 ne sont jamais retentés.
- **Mémoire des refus** : un GET qui a reçu 403 ou 404 n'est pas renvoyé au
  serveur avant 10 s, puis 20 s, 40 s… jusqu'à 5 min (oubli au bout de
  15 min sans nouvelle demande, 128 adresses au plus) ; la réponse gardée
  est rendue à la place. Une écriture réussie vide cette mémoire (les droits
  ont pu changer).
- **Corps des réponses** : 16 Mio au plus, lus par morceaux. Message d'erreur
  du serveur : 300 caractères au plus, jeton retiré s'il y figurait.
- **Connexion** : 10 s au plus (30 s pour la requête entière, inchangé).
- **Redirections** : suivies seulement vers le même hôte (schéma et port
  compris), cinq au plus ; vers un autre hôte, la réponse 3xx est rendue
  telle quelle. Les en-têtes d'identité ne partent donc jamais chez un tiers
  (reqwest ne retire de lui-même que `Authorization`).

### En-têtes et identité

- `X-Discord-Locale` : langue choisie dans Ferrocord (`fr`, `en-US`), sinon
  langue du système ; figé pour la connexion (voir « En-têtes »).
- Sons et fichiers joints lus par `voice::clips` (CDN de Discord) : envoient
  désormais le `User-Agent` de la session ; ils n'en envoyaient aucun.
- Connexion par QR code : le WebSocket envoie désormais `Accept-Language`,
  comme la passerelle principale (mode bureau seulement).
- `has_client_mods` : documenté sur docs.discord.food comme propriété du
  client (« le client a été modifié ») ; Ferrocord envoie `false`, à
  l'identique dans l'Identify et dans `X-Super-Properties` (même structure
  sérialisée, test `super_properties_match_the_identify…`). Les bits de
  détection de `launch_signature` restent à zéro. Rien d'autre n'est
  documenté à ce sujet, rien d'autre n'est envoyé.

### `capabilities`, `presence`, `client_state` : décision

**Non envoyés, et aucune option pour les activer.** Bits documentés
(docs.discord.food, « Gateway Capabilities ») :

| Bit | Nom | Effet |
|---|---|---|
| 0 | LAZY_USER_NOTES | retire `notes` du READY |
| 1 | NO_AFFINE_USER_IDS | pas de synchronisation pour les relations implicites |
| 2 | VERSIONED_READ_STATES | `read_state` devient un objet versionné |
| 3 | VERSIONED_USER_GUILD_SETTINGS | `user_guild_settings` devient un objet versionné |
| 4 | DEDUPE_USER_OBJECTS | READY déshydraté (`users`, `merged_members`) |
| 5 | PRIORITIZED_READY_PAYLOAD | READY puis READY_SUPPLEMENTAL (exige le bit 4) |
| 6 | MULTIPLE_GUILD_EXPERIMENT_POPULATIONS | `populations` devient un tableau |
| 7 | NON_CHANNEL_READ_STATES | états de lecture hors salons |
| 8 | AUTH_TOKEN_REFRESH | `auth_token` possible dans le READY |
| 9 | USER_SETTINGS_PROTO | plus de `user_settings` dans le READY |
| 10 | CLIENT_STATE_V2 | cache d'état du client, version 2 |
| 11 | PASSIVE_GUILD_UPDATE | PASSIVE_UPDATE_V1 pour les serveurs non suivis |
| 12 | AUTO_CALL_CONNECT | CALL_CREATE sans demande |
| 13 | DEBOUNCE_MESSAGE_REACTIONS | MESSAGE_REACTION_ADD_MANY |
| 14 | PASSIVE_GUILD_UPDATE_V2 | PASSIVE_UPDATE_V2 (remplace le bit 11) |
| 15 | CHANNEL_OBFUSCATION | salons invisibles masqués |
| 16 | AUTO_LOBBY_CONNECT | `lobbies` dans le READY |

Raisons de ne pas les envoyer :

1. la page ne donne pas la valeur du client officiel ; son seul exemple
   (`1734653`) contient les bits 17, 19 et 20, **non documentés** : les
   envoyer serait annoncer des comportements inconnus ;
2. une valeur partielle (les seuls bits que Ferrocord saurait lire) est une
   valeur qu'aucun client officiel n'envoie : aussi distinguable que
   l'absence du champ ;
3. chaque bit change la forme de ce qui est reçu (READY déshydraté, READY
   en deux temps, PASSIVE_UPDATE_V2, réactions groupées, `client_state`
   à tenir à jour) : sans capture d'un vrai compte, la lecture casserait,
   et un client qui demande des formes qu'il lit mal est moins fiable.

À reprendre seulement avec une capture d'un vrai compte (forme du READY
pour une valeur donnée) et la gestion complète des événements concernés.

### Numéros de version : péremption

Aucune requête ajoutée. Paramètres › Avancés affiche les versions annoncées
(lues dans le cache et `local.toml`) et avertit :

- relevé embarqué de plus de **56 jours** (Electron et Chromium ne se
  renouvellent que par une mise à jour de Ferrocord) ;
- numéro de build non renouvelé depuis plus de **21 jours**
  (`identite.json` garde désormais `build_at`, date du dernier relevé
  réussi).

Pas d'avertissement si `local.toml` impose une valeur.

**À arbitrer** : le rafraîchissement quotidien décrit plus haut
(`GET https://discord.com/login` sans jeton avec le `User-Agent` du client
de bureau, et le manifeste avec un `install_id`) est une requête que le
client officiel ne fait pas sous cette forme (il charge son application,
avec ses en-têtes de navigation). Elle part au plus une fois par jour, sans
jeton ; elle n'a pas été retirée par ce lot (décision du lot
identité-bureau), mais elle est signalée dans `docs/fiabilite.md` comme
candidate au retrait au profit des seules valeurs embarquées.

### Rythme

- Export d'une conversation (`ui::export`) : la page suivante est demandée
  après une pause aléatoire de 0,6 à 1,5 s (avant : 600 ms fixes, jusqu'à
  1000 pages à cadence parfaitement régulière).
- « Marquer tout comme lu » (palette) : un seul `POST /read-states/ack-bulk`
  par 100 salons, serveurs et conversations confondus (avant : un `ack-bulk`
  par serveur et un `/ack` par conversation, lancés ensemble). De même pour
  « Marquer comme lu » sur un dossier de serveurs : un seul `ack-bulk`.
- Présence (op 3) : l'écart entre deux envois est tiré entre 4 et 7 s
  (avant : 4 s fixes).
- `GET /games/detectable` : 60 à 90 s après la connexion (avant : 60 s
  fixes) ; après un échec sans cache, pas redemandée dans l'heure par un
  nouveau READY de la même connexion.
- Navigation au clavier, touche maintenue : au plus un changement de salon
  (donc un `GET …/messages`) par 250 ms.
- `GET /invites/{code}` des cartes visibles : 4 en vol au plus (avant :
  jusqu'à 64 d'un coup).
- Suppression d'une sélection de messages (compte utilisateur, un `DELETE`
  par message) : pause tirée entre 0,35 et 0,9 s entre deux.
- Session vocale expirée (fermetures 4006, 4009 de la passerelle vocale) :
  l'op 4 de reprise repart au plus 3 fois par 5 minutes, puis l'appel
  s'arrête sur un message.
- Inchangés et vérifiés : saisie (au plus un POST `/typing` par 8 s, en
  tapant seulement), accusés de lecture (groupés sur 3 s), présence op 3
  (au moins 4 s entre deux), 120 commandes de passerelle par minute au plus.

### Compteurs

`net::health` compte, par connexion : requêtes, 4xx, 429, 5xx, nouveaux
essais, requêtes épargnées par la mémoire des refus, connexions à la
passerelle, Identify. Affichés dans Paramètres › Avancés › Débogage. Rien
n'est envoyé nulle part.

### Tests

`tests/e2e/fiable.rs` (faux serveur local) : session coupée après chaque
READY, 4008, RESUME refusé trois fois, adresse de reprise morte, zombie fermé
en 4000, attente bornée après l'op 9, mémoire des refus, 502 retenté en
lecture et jamais en écriture, panne durable arrêtée à trois requêtes, 401
jamais retenté, session ordinaire sans aucun refus, aucun jeton dans ce qui
peut être journalisé, redirection vers un autre hôte jamais suivie, pause
entre deux suppressions. `tests/remote_auth.rs` : en-têtes du WebSocket de
la connexion par QR code. Unitaires : `net::backoff`, `net::health`,
`net::gateway::tests`, `net::identity::tests` (langue, péremption),
`voice::clips::fiable_tests`, `net::voice::robust`, `net::voice::dave::robust`.

### À vérifier sur un vrai compte

- qu'une reprise après veille de l'ordinateur donne bien RESUMED et non un
  READY neuf ;
- que la fermeture 4000 côté client garde la session (documenté, non
  observé) ;
- le compteur de Paramètres › Avancés après une journée d'usage : 4xx et 429
  attendus à zéro ; tout autre chiffre désigne une requête à corriger ;
- que `X-Discord-Locale: en-US` avec un système en français ne change rien
  aux textes renvoyés par Discord que Ferrocord affiche.
