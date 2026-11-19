# Fiabilité et cohérence (lot « fiable »)

But du lot : que Ferrocord se comporte comme un client normal, propre et
cohérent, sans les signaux d'un client bancal ou d'un robot (rafales,
erreurs répétées, requêtes mal formées, en-têtes incohérents, reconnexions
en boucle).

**Ce document ne promet rien.** Ferrocord reste un client non officiel :
Discord peut le reconnaître (voir « Ce qui distingue encore Ferrocord »), et
l'usage d'un client tiers sur un compte utilisateur reste contraire à ses
conditions. Le risque demeure ; ce lot réduit seulement ce qui, dans le
comportement de Ferrocord, le rendait plus fragile ou plus remarquable qu'il
n'a besoin de l'être.

Limites tenues : aucune imitation d'empreinte TLS ou HTTP/2, aucun
contournement de captcha, aucun achat, aucune automatisation, aucune fausse
télémétrie, aucune requête nouvelle vers Discord. Aucun test ne contacte autre
chose que `127.0.0.1`. Rien de ce qui suit n'a été essayé sur un vrai compte.

Forme sur le fil et sources : `docs/passerelle-utilisateur.md`, section
« Fiabilité et cohérence (lot « fiable ») ».

## 1. Audit

« e2e » : `tests/e2e/…` contre le faux serveur local. Les autres tests sont
unitaires, dans le fichier nommé.

### Passerelle

| Point | État avant | Correction | Test |
|---|---|---|---|
| Code 4004 (jeton refusé) | déjà : arrêt définitif, retour à l'écran de connexion | aucune | e2e `gateway::close_4004_is_fatal_bad_token`, `net::gateway::tests::close_4004_is_fatal_bad_token` |
| Codes 4010 à 4014 | déjà : arrêt définitif, sans reconnexion | aucune | `gateway::tests::close_4010_to_4014_is_fatal_without_bad_token` |
| Codes 4007, 4009 | déjà : session abandonnée, Identify neuf | aucune | `gateway::tests::close_4007_4009_reconnect_without_resume` |
| Codes 4008, 4015 | reconnexion immédiate | session gardée, attente d'une fenêtre entière (60 s) | e2e `fiable::close_4008_waits_a_full_window_then_resumes`, `gateway::tests::close_4008_and_4015_slow_down_but_keep_the_session` |
| Op 9 (Invalid Session) | Identify ou reprise aussitôt | attente tirée entre 1 et 5 s (doc officielle), puis reprise si `d: true`, Identify neuf sinon | e2e `fiable::invalid_session_waits_a_random_time_within_bounds`, `gateway::op9_resumable_resumes`, `gateway::op9_not_resumable_identifies_again`, `gateway::repeated_invalid_session_backs_off` |
| Op 7 (Reconnect) | déjà : reprise | fermeture en 4000 (session gardée) | e2e `gateway::op7_reconnects_and_resumes`, `gateway::repeated_reconnect_request_before_ready_backs_off` |
| Compteur d'échecs | remis à zéro à chaque READY : une session coupée juste après READY revenait sans jamais ralentir | remis à zéro seulement après 30 s de session établie | e2e `fiable::a_session_cut_right_after_ready_backs_off` |
| Attentes de reconnexion | 1 s, 2 s, 4 s pile | recul exponentiel avec gigue (moitié à totalité), plafond 60 s | `net::backoff::tests`, `gateway::tests::reconnect_delay_doubles_and_caps_at_60s`, e2e `gateway::repeated_close_after_hello_backs_off` |
| Reprise refusée en boucle | RESUME sans fin | après 3 refus : Identify neuf | e2e `fiable::a_resume_refused_three_times_gives_way_to_a_fresh_identify` |
| Adresse de reprise morte | reprise sans fin sur la même adresse | après 3 échecs : passerelle par défaut | e2e `fiable::a_dead_resume_address_falls_back_to_the_default_gateway` |
| Premier heartbeat | toujours à la moitié de l'intervalle | à `intervalle × aléa`, sans rattrapage en rafale | e2e `gateway::periodic_heartbeat_carries_seq` |
| Heartbeat sans accusé, veille de l'ordinateur | fermeture sans code choisi | fermeture en 4000, puis reprise ; saut d'horloge détecté | e2e `fiable::a_zombie_connection_is_closed_with_a_resumable_code`, `gateway::unacked_heartbeat_reconnects_and_resumes` |
| Délais | aucun sur l'ouverture, le Hello, l'envoi | 20 s chacun | pas de test propre (constantes) ; saut d'horloge : `gateway::tests::a_clock_jump_is_told_from_an_ordinary_beat` |
| Identify | déjà : par `gwlimit` (5 s entre deux, 1000 par jour) | compté dans `net::health` | `gwlimit::tests::identify_spacing_and_daily_cap`, e2e `fiable::an_ordinary_session_produces_no_refusal_at_all` (un seul Identify) |
| Commandes | déjà : 120 par minute au plus | aucune | `gwlimit::tests::command_121_is_delayed_until_the_window_frees` |
| Intervalle de heartbeat absurde | déjà : erreur de session | aucune | e2e `gateway::short_heartbeat_interval_is_a_session_error` |

### REST

| Point | État avant | Correction | Test |
|---|---|---|---|
| 401 | déjà : arrêt, jamais renvoyé | aucune | e2e `fiable::a_401_is_never_retried`, `rest::unauthorized_history_is_fatal`, `rest::unauthorized_send_is_fatal` |
| 403, 404 sur une lecture | redemandée à chaque passage (martèlement d'une ressource refusée) | mémoire des refus : 10 s, 20 s, 40 s… 5 min au plus ; vidée par une écriture réussie | e2e `fiable::a_refused_read_is_not_asked_again_right_away` |
| 5xx, coupure | aucun nouvel essai, ou sans borne selon la route | lectures seulement : deux nouveaux essais avec recul et gigue ; une écriture ne repart jamais seule | e2e `fiable::a_read_is_retried_after_a_502_but_a_write_never_is`, `fiable::a_lasting_outage_stops_after_two_retries` |
| Doublon de message | déjà : `nonce`, réponse perdue non renvoyée | aucune | e2e `rest::lost_response_is_unconfirmed_until_the_gateway_echoes_the_nonce`, `rest::message_of_someone_else_with_same_nonce_confirms_nothing` |
| 429, limite globale, 429 sans corps ou mal formé | déjà : attente, trois essais au plus, pause générale | aucune | e2e `rest::rate_limit_waits_then_retries`, `rest::rate_limit_gives_up_after_three_attempts`, `rest::bad_retry_after_values_do_not_break_sending`, `rest::global_limit_pauses_all_requests`, `rest::firewall_429_pauses_everything_without_retry`, `rest::exhausted_bucket_waits_before_sending_without_429` |
| Corps de réponse | lu en entier, sans borne | 16 Mio au plus, par morceaux ; ouverture de connexion 10 s | `net::rest::tests::body_of_429_is_bounded` |
| Redirection | suivie jusqu'à dix fois, vers n'importe quel hôte, avec les en-têtes d'identité (reqwest ne retire que `Authorization`) | suivie seulement vers le même hôte (5 au plus) ; sinon la réponse 3xx est rendue telle quelle | e2e `fiable::a_redirect_to_another_host_is_never_followed` |
| Jeton dans une erreur | le texte d'erreur du serveur était repris tel quel | texte borné à 300 caractères, jeton retiré | e2e `fiable::nothing_loggable_ever_holds_the_token` |
| Usage normal | non mesuré | compteurs par connexion (`net::health`), affichés dans Paramètres › Avancés | e2e `fiable::an_ordinary_session_produces_no_refusal_at_all` : démarrage, trois salons, un message : aucun 4xx, aucun 429, aucun nouvel essai |

### Données reçues

| Point | État avant | Correction | Test |
|---|---|---|---|
| Analyseurs binaires du vocal (RTP, RTCP, DAVE, MLS) | lus avec vérifications, non éprouvés | paquets hostiles (tronqués, longueurs fausses, aléatoires) : aucun ne panique | `net::voice::robust`, `net::voice::dave::robust` |
| Passerelle texte, REST | relu dans ce lot : accès par `get`, durées bornées (`ratelimit::seconds`), aucune indexation sur une donnée reçue | aucune nécessaire | e2e `rest::bad_retry_after_values_do_not_break_sending`, `gateway::short_heartbeat_interval_is_a_session_error` |
| Son téléchargé (décodeur tiers `symphonia`) | une panique du décodeur arrêtait l'application | décodage sous `panics::contain` : son illisible, rien de plus | pas de test propre : aucun fichier connu ne fait paniquer le décodeur ; `panics::tests` éprouve `contain` |
| Vidéo WebM jointe (`video::mkv`) | additions sur des tailles et des dates lues dans le fichier (débordement : panique en build de développement) ; analyse hors de `contain` | additions vérifiées ou saturées ; analyse sous `panics::contain` | `video::mkv` (tests existants) |
| Époque MLS (`epoch + 1`) | débordement possible en build de développement | `wrapping_add` (l'échange échoue ensuite proprement) | `net::voice::dave` (tests existants) |
| Flux H.264 reçu en appel (`openh264`, code C) | non protégé | **laissé** : une panique Rust n'y est pas le risque, et `contain` ne protège pas d'une faute dans du code C | aucun |

### Passerelle vocale

| Point | État avant | Correction | Test |
|---|---|---|---|
| Codes 4014, 4022 | déjà : fin de l'appel, sans reconnexion | aucune | `net::voice::tests`, e2e `voice_transport` |
| Code 4015, coupures | déjà : reprise (op 7) avec recul | délais d'ouverture et de Hello, gigue | `net::voice::tests`, e2e `voice_transport` |
| Codes 4006, 4009 (session expirée) | une op 4 repartait à chaque fois, sans compteur : boucle possible | 3 reprises automatiques au plus par 5 minutes ; au-delà, message et arrêt (le bouton reste) | `state::voice::tests::a_voice_session_that_keeps_expiring_stops_rejoining`, e2e `voice_call` |

### Identité

| Point | État avant | Correction | Test |
|---|---|---|---|
| `X-Discord-Locale` | langue du système, alors que l'interface en affiche une autre | langue choisie dans Ferrocord, figée pour la connexion | `net::identity::tests` |
| Sons et fichiers lus (`voice::clips`) | partaient vers le CDN sans `User-Agent` | `User-Agent` de la session | `voice::clips::fiable_tests` |
| Connexion par QR code | WebSocket sans `Accept-Language`, contrairement à la passerelle principale | même en-tête | `tests/remote_auth.rs` : `the_qr_socket_announces_what_the_main_gateway_announces` |
| Numéros de version | pouvaient vieillir sans que rien ne le dise | avertissement dans Paramètres › Avancés (relevé embarqué de plus de 56 jours, build de plus de 21 jours) | `net::identity::tests` |
| Identify ↔ `X-Super-Properties` | déjà : une seule structure sérialisée | aucune | e2e `identity::desktop_identity_is_the_same_on_the_gateway_the_api_and_the_cdn` |

## 2. Inventaire des en-têtes et des champs d'identité

Mode « bureau » (défaut sous Windows), compte utilisateur.

| Requête | `User-Agent` | Autres en-têtes d'identité | Jeton |
|---|---|---|---|
| API REST (toutes les routes, envoi de fichiers compris : un seul point de sortie, `rest::api_req`) | Electron de la session | `X-Super-Properties`, `X-Discord-Locale`, `X-Discord-Timezone`, `Accept-Language`, `Referer`, `Sec-CH-UA*`, `Sec-Fetch-*`, `Origin` hors GET et HEAD | `Authorization`, tel que saisi |
| Passerelle (WebSocket) | le même | `Origin`, `Accept-Language` ; Identify : mêmes propriétés que l'en-tête, plus `is_fast_connect` et `gateway_connect_reasons` | dans l'Identify et le Resume |
| Passerelle vocale (WebSocket) | le même | `Origin`, `Accept-Language` ; Identify vocal sans propriétés | jeton vocal seulement |
| Voix (UDP) | sans objet | aucun | aucun |
| CDN (images, émojis, autocollants, enregistrer une image, export) | le même | aucun | jamais |
| CDN (sons, messages vocaux, audio et vidéo joints) | le même | aucun | jamais |
| Connexion par QR code | le même | WebSocket : `Origin`, `Accept-Language` ; échange du ticket : comme l'API | aucun |
| Relevé des versions (une fois par jour) | le même | aucun | jamais |
| Mises à jour (GitHub) | `Ferrocord/<version>` (voulu) | aucun | jamais |
| Traduction (service choisi par l'utilisateur, plugin coupé par défaut) | `Ferrocord/<version>` | aucun | jamais |

Constats :

- aucun `User-Agent` de bibliothèque : reqwest n'en pose pas par défaut, et
  chaque client en reçoit un explicitement ; tungstenite n'en ajoute pas ;
- le préfixe `Bot ` n'est jamais ajouté par le code : il n'existe que si le
  jeton saisi le porte (compte de bot) ;
- le jeton ne part que vers l'API et les passerelles, sur des hôtes vérifiés ;
- les valeurs sont figées par connexion (`Identity`, partagée par la
  passerelle et l'API) : `User-Agent`, `client_launch_id`,
  `launch_signature`, langue ; `client_heartbeat_session_id` change toutes
  les 30 minutes, comme documenté, et l'Identify et l'en-tête lisent le même.

Écarts connus, laissés :

- **changement de mode d'identité sans redémarrage** : la passerelle et
  l'API suivent le nouveau mode à la reconnexion ; le CDN, le vocal et les
  sons gardent celui du lancement. Redémarrer après avoir changé de mode ;
- **compte de bot avec le mode bureau** : l'API et la passerelle restent en
  identité Ferrocord, mais le CDN et le vocal annoncent l'`User-Agent`
  d'Electron. Sans effet sur un compte utilisateur ;
- le `User-Agent` d'Electron part aussi vers `cdn.jsdelivr.net` (Twemoji) et
  `media.tenor.com` (GIF), sans jeton ni propriétés : invisible pour Discord.

## 3. Décisions

### `capabilities`, `presence`, `client_state` : non envoyés, aucun réglage

Relu sur docs.discord.food (« Gateway », « Gateway Events ») le 15 novembre
2026. Les trois champs sont **optionnels** dans l'Identify. La page décrit 17
bits de `capabilities` (0 à 16) et ne dit pas quelle valeur le client officiel
envoie ; son seul exemple (`1734653`, avec des propriétés factices) contient
les bits 17, 19 et 20, qui ne sont pas documentés.

Choix : ne rien envoyer, et ne pas écrire de réglage pour l'activer.

1. Chaque bit change la forme de ce que Discord renvoie (READY déshydraté,
   READY en deux temps, `read_state` et `user_guild_settings` versionnés,
   PASSIVE_UPDATE_V2, réactions groupées, réglages en protobuf seulement).
   Sans capture d'un vrai compte, la lecture ne peut pas être vérifiée, et un
   client qui demande des formes qu'il lit mal est moins fiable.
2. Une valeur partielle (les seuls bits que Ferrocord saurait lire) est une
   valeur qu'aucun client officiel n'envoie : elle distingue autant que
   l'absence du champ.
3. Un réglage désactivé, testé seulement contre un faux serveur écrit d'après
   la même page, ne prouverait rien de plus et laisserait du code mort
   capable de casser la session le jour où on l'active.

Ce qu'il faudrait pour le faire : une capture du READY d'un vrai compte pour
la valeur exacte du client officiel du moment ; la lecture du READY
déshydraté (`users`, `merged_members`) et de READY_SUPPLEMENTAL ; les états de
lecture et réglages de serveur versionnés ; PASSIVE_UPDATE_V2 ;
MESSAGE_REACTION_ADD_MANY ; `client_state` tenu à jour (versions par
serveur) ; les bits 17, 19 et 20 compris.

### Champ « client modifié »

docs.discord.food (« Client Properties ») documente `has_client_mods`
(booléen : le client porte des modifications, BetterDiscord par exemple) ;
ses trois exemples de client de bureau le donnent à `false`. La même page
décrit `launch_signature` : un UUID dont certains bits signalent des
modifications détectées (BetterDiscord, Vencord, OpenAsar, Shelter,
Moonlight…), à zéro pour un client non modifié.

Ferrocord envoie déjà `has_client_mods: false` et une `launch_signature` aux
bits de détection à zéro, à l'identique dans l'Identify et dans
`X-Super-Properties` (même structure). Rien à changer. Rien d'autre n'est
documenté à ce sujet : rien d'autre n'est envoyé.

Deux champs à signaler :

- `client_app_state` (focus de la fenêtre) est documenté et **absent** des
  propriétés de Ferrocord (choix du lot identité-bureau : l'exemple Windows
  de la page ne le contient pas non plus) ;
- `gateway_connect_reasons` est envoyé dans l'Identify mais ne figure pas
  dans le tableau de la page relue : à vérifier sur un vrai compte.

### Valeurs périmées

Les versions d'Electron et de Chromium ne se renouvellent que par une mise à
jour de Ferrocord ; le numéro de build est relevé une fois par jour. Un
avertissement apparaît dans Paramètres › Avancés quand elles vieillissent
(56 jours et 21 jours). Aucune requête n'a été ajoutée.

**À arbitrer par l'utilisateur** : le relevé quotidien (`GET
https://discord.com/login` et le manifeste des mises à jour, sans jeton, avec
l'`User-Agent` du client de bureau) est une requête que le client officiel ne
fait pas sous cette forme. Elle part au plus une fois par jour. La retirer
laisserait le numéro de build vieillir entre deux versions de Ferrocord ; la
garder ajoute une requête atypique. Non tranché par ce lot.

## 4. Rythme

| Flux | Déclencheur | Avant | Maintenant |
|---|---|---|---|
| Souscription au serveur (op 37) | salon affiché, défilement de la liste des membres | un seul serveur à la fois, 250 ms puis 1,5 s entre deux ; aucune rafale au démarrage | inchangé |
| Membres (op 8, op 14) | — | jamais envoyées | inchangé |
| Accusé de lecture | bas de la liste visible, fenêtre active | groupé, 3 s après la lecture | inchangé |
| « Marquer tout comme lu », « Marquer comme lu » d'un dossier de serveurs | commande de la palette, menu du dossier | une requête par serveur et une par conversation, lancées ensemble | **un seul accusé groupé** (par lots de 100) |
| Saisie (`/typing`) | frappe | au plus une par 8 s et par salon | inchangé |
| Présence (op 3) | jeu détecté, Rich Presence, inactivité | au moins 4 s entre deux : une application bavarde donnait une op 3 toutes les 4 s pile | **écart tiré entre 4 et 7 s** |
| Liste des jeux (`/games/detectable`) | connexion, une fois par jour | 60 s pile après la connexion ; redemandée à chaque READY après un échec | **60 à 90 s** ; **pas redemandée dans l'heure qui suit un échec** |
| Historique | ouverture d'un salon, défilement vers le haut | une demande à la fois par sens, une page par seconde au plus | inchangé |
| Navigation au clavier, touche maintenue | répétition du système | un salon (et son historique) par répétition, une trentaine par seconde | **4 par seconde au plus** ; un nouvel appui passe toujours |
| Cartes d'invitation | carte visible | jusqu'à 64 requêtes d'un coup | **4 en vol au plus**, les suivantes à mesure |
| Profil | ouverture d'une carte ou d'un MP (jamais au survol) | une fois par personne et serveur, 5 min de cache, jamais deux en vol | inchangé |
| Suppression d'une sélection | modération, après confirmation | jusqu'à 100 suppressions dos à dos | **pause tirée entre 0,35 et 0,9 s** entre deux |
| Export d'une conversation | clic « Exporter… » | 600 ms pile entre deux pages | 0,6 à 1,5 s |
| Images | à l'écran | 6 à la fois, dédupliquées, cache disque, pause après un 429 | inchangé |
| Reprise vocale après session expirée | code 4006 ou 4009 | sans borne | 3 par 5 minutes |

Tests : `state::store_relations::tests::mark_all_read_is_one_grouped_ack`,
`net::games::tests::gate_sends_on_change_with_a_gap`,
`state::activity::tests::a_failed_list_is_not_asked_again_at_every_ready`,
`ui::keys::tests::a_held_key_does_not_race_through_channels`,
`state::invites::tests::only_a_few_invites_are_asked_at_once`,
e2e `fiable::selected_messages_are_not_deleted_back_to_back`.

Laissés tels quels : les délais fixes qui suivent un geste (3 s avant un
accusé, 8 s entre deux `/typing`, 1,5 s entre deux op 37 pendant un
défilement) : leur instant dépend du geste, pas d'une horloge ; plusieurs
accusés à la même échéance quand plusieurs vues sont ouvertes ; les pièces
jointes d'un export, téléchargées une à une sans pause.

## 5. Extras : ce qui part vers Discord sans geste immédiat

Repère : le dépôt public de Vencord refuse les « selfbots or API spam
(animated status, message pruner, auto reply, nitro snipers, etc) », ainsi
que FakeDeafen et FakeMute ; son plugin NoTrack, obligatoire, coupe la
télémétrie.

Absents du code (cherchés) : statut animé ou programmé, réponse automatique,
réactions en série, envoi répété, changement automatique d'avatar ou de
pseudo, faux état du micro ou du casque (l'op 4 porte toujours l'état réel),
progression automatique de quête (`/heartbeat`, `/video-progress` jamais
appelés), envoi automatique d'un message programmé.

| Fonction | Ce qui part | Rythme | Déclencheur | Défaut | Classement |
|---|---|---|---|---|---|
| Messages programmés | un `POST` de message | un par clic | l'échéance n'affiche qu'un rappel ; l'envoi demande un clic | activé | sans risque |
| Brouillons, journal des messages, mots-clés, recherche locale, onglets, zoom, thèmes | rien | — | — | variable | sans risque |
| Traduction | rien vers Discord (service choisi par l'utilisateur) | un par clic | clic | coupé | sans risque |
| Quêtes | liste, acceptation, récompense | sur geste ; liste relue si plus de 5 min à l'ouverture de la page | clic | — | sans risque |
| Progression des quêtes (0.1.13) | `POST …/heartbeat` (1/min tant que le jeu tourne), `…/video-progress` (toutes les 10 s lues) | activité réelle seulement | jeu détecté, vidéo en lecture | **coupé** | à connaître : requêtes du client officiel, envoyées sans ses empreintes de programme |
| Accusés de lecture | `POST …/ack` | 3 s après lecture, groupés | lecture à l'écran | activé | sans risque (le client officiel fait de même) |
| Saisie | `POST …/typing` | une par 8 s | frappe | activé | sans risque |
| Inactivité automatique | op 3 `idle`, `afk: true`, puis retour | à 10 min sans saisie, et au retour | horloge locale de dernière saisie | toujours | sans risque (le client officiel fait de même) |
| Jeu détecté | op 3 avec le jeu ; `GET /games/detectable` une fois par jour | à chaque changement de jeu, 4 à 7 s au moins entre deux | relevé local des programmes toutes les 20 s | activé (réglage Discord « afficher le jeu ») | sans risque |
| Rich Presence locale | op 3 avec l'activité donnée par une application locale | 4 à 7 s au moins entre deux | l'application tierce | activé | **à surveiller** : une application qui change son activité sans arrêt fait partir une op 3 toutes les 4 à 7 s, soit le rythme d'un statut animé. Le limiteur le borne, mais c'est l'application qui décide |
| Mode discret | **retient** `/typing` et les accusés automatiques (accusé au clic) ; ne change ni la présence ni le chargement | — | réglage | coupé | **à surveiller**, à laisser coupé par défaut : l'absence de ces signaux n'est pas une requête, mais un compte qui lit sans jamais accuser ni écrire « en train d'écrire » s'écarte du profil ordinaire |
| Suppression d'une sélection | `DELETE` de message, un par un | jusqu'à 100, pause de 0,35 à 0,9 s | sélection et confirmation, permission de gérer les messages | — | **à surveiller** : c'est la forme d'un « message pruner », même déclenché à la main. Recommandation : abaisser le plafond à 25 par geste pour un compte utilisateur |
| « Marquer tout comme lu » | `POST /read-states/ack-bulk` | une requête par 100 salons | commande de la palette | — | sans risque depuis ce lot |
| Export d'une conversation | `GET` d'historique en série, pièces jointes sur demande | une page par 0,6 à 1,5 s, 1000 pages au plus | clic | activé | **à surveiller** : 1000 pages, c'est 50 000 messages lus d'affilée, ce qu'aucun usage ordinaire ne produit. Recommandation : plafond par défaut à 200 pages, le reste sur confirmation |
| Cartes d'invitation | `GET /invites/{code}` | 4 en vol | carte visible | — | sans risque |
| Relevé des versions | deux `GET` sans jeton | une fois par jour | connexion | activé en mode bureau | **à désactiver par défaut ?** voir « Valeurs périmées » : à arbitrer |
| Reprise vocale | op 4 | 3 par 5 min | session vocale expirée | — | sans risque depuis ce lot |
| Enregistrement d'appel, incrustation, égaliseur | rien | — | — | coupés ou locaux | sans risque |

Aucune fonction n'a été retirée ni désactivée par ce lot. Les trois
recommandations (plafond de suppression, plafond d'export, relevé des
versions) sont soumises à l'utilisateur.

## 6. Ce qui distingue encore Ferrocord du client officiel

Par importance décroissante. Aucun de ces points n'est masqué par ce lot.

| # | Signal | État | Pourquoi |
|---|---|---|---|
| 1 | **Empreinte TLS et HTTP/2** : rustls et hyper ne négocient pas comme Chromium (suites, extensions, ordre, réglages HTTP/2, ordre des pseudo-en-têtes). Visible dès la première connexion, avant toute requête | volontairement non traité | imiter une empreinte est un contournement de détection : refusé. C'est le signal le plus net, et il contredit l'`User-Agent` annoncé |
| 2 | **Ordre et casse des en-têtes HTTP**, `Accept: */*`, absence de cookies (`__dcfduid`, `__sdcfduid`, `__cf_bm`), de `X-Debug-Options`, `X-Installation-ID`, `X-Context-Properties` | non traité | l'ordre tient à la bibliothèque (même famille que le point 1) ; les cookies et en-têtes de contexte ne sont pas documentés assez précisément pour être reproduits sans les inventer |
| 3 | **Télémétrie absente** (`/science`, métriques) | volontairement non traité | pas de fausse télémétrie. L'absence n'a rien de rare : le plugin NoTrack de Vencord la coupe pour tous ses utilisateurs |
| 4 | **Identify sans `capabilities`, `presence` ni `client_state`**, compression du transport en `zlib-stream` (celle du client officiel du moment n'a pas été vérifiée) | volontairement non traité | voir « Décisions » : change la forme de tout ce qui est reçu, invérifiable sans vrai compte |
| 5 | **Profil de trafic au démarrage** : le client officiel fait des dizaines de requêtes que Ferrocord ne fait pas (expériences, bibliothèque, facturation, boutique…) et charge ses scripts | non traité | les faire sans s'en servir serait du trafic factice |
| 6 | **Relevé quotidien des versions** : `GET /login` sans en-têtes de navigation | à arbitrer | voir « Valeurs périmées » |
| 7 | **Numéros de version** qui vieillissent entre deux versions de Ferrocord | traité en partie | avertissement ; le build est relevé chaque jour |
| 8 | `client_app_state` absent ; `gateway_connect_reasons` non confirmé par la page relue | à vérifier sur un vrai compte | une capture trancherait |
| 9 | **Langue** : `X-Discord-Locale` suit Ferrocord, pas la langue enregistrée dans les réglages Discord du compte ; `Accept-Language` suit le système | traité en partie | cohérent au sein de Ferrocord ; peut différer du compte |
| 10 | **Mode d'identité changé sans redémarrage**, compte de bot en mode bureau | laissé, documenté | voir l'inventaire |
| 11 | **Rythmes** : heartbeats, accusés, saisie, présence | traité | gigue et bornes ; les valeurs exactes du client officiel ne sont pas connues |
| 12 | **Fonctions que le client officiel n'a pas** : export en série, suppression par sélection, mode discret | à surveiller | voir « Extras » |

## 7. Prouvé, et ce qui ne peut pas l'être ici

Prouvé par test contre le faux serveur local : tout le tableau d'audit
(reconnexions, reprises, attentes, nouveaux essais, mémoire des refus,
redirections, absence du jeton dans ce qui se journalise, session ordinaire
sans aucun refus, pause entre les suppressions), l'égalité de l'identité
entre la passerelle, l'API et le CDN, et les bornes de rythme.

À vérifier sur un vrai compte, et seulement là :

- qu'une reprise après veille donne RESUMED et non un READY neuf ;
- que la fermeture 4000 côté client garde bien la session ;
- le compteur de Paramètres › Avancés après une journée d'usage : 4xx et 429
  attendus à zéro ; tout autre chiffre désigne une requête à corriger ;
- que Discord accepte un Identify sans `capabilities` sans changer de
  comportement (c'est le cas aujourd'hui dans l'usage, non garanti) ;
- `gateway_connect_reasons`, `client_app_state` ;
- que `X-Discord-Locale` différent de la langue du compte ne change rien aux
  textes renvoyés ;
- que la mémoire des refus (403, 404) ne masque pas trop longtemps un droit
  tout juste accordé (elle est vidée par toute écriture réussie).
