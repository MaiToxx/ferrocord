# Mesures : Ferrocord face au client Discord officiel

Ce document compare, chiffres à l'appui, la rapidité, la réactivité et la
consommation de ressources de Ferrocord et du client Discord officiel. Il a
trois parties :

1. ce qui a été mesuré sans compte ni connexion (aperçu hors ligne, disque) ;
2. le script `tools/mesures.ps1`, que vous lancez vous-même sur vos deux
   applications connectées ;
3. les limites de ces mesures.

## 1. Mesures hors ligne (13 novembre 2026)

Machine : Windows 11 Pro 10.0.26300, AMD Ryzen 7 4800H (16 cœurs logiques),
15,4 Go de mémoire. Machine au repos pendant la mesure (charge ≈ 3 %, aucune
compilation en cours). Ferrocord : branche `lot/mesures` (base `81449aa`),
profil `release`.

### Aperçu hors ligne : banc `preview --bench`

Commande :

```
cargo run --release --example preview -- --bench --bench-out banc.json
```

Le banc dessine la vraie interface (`ui::App`, rendu OpenGL d'eframe) sur un
magasin synthétique : 100 serveurs, 5 000 salons, 4 salons de 500 messages
variés (texte mis en forme, liens, embeds, pièces jointes) et un salon de
2 000 messages servis page par page. La fenêtre se ferme seule. « Image » :
temps qu'eframe mesure pour une image (entrées, mise en page, tessellation,
envoi des commandes OpenGL ; sans l'attente de la synchro verticale).

| Mesure | Médiane | 95e centile | Maximum |
|---|---:|---:|---:|
| Image au repos, salon de 500 messages | 1,49 ms | 2,00 ms | 3,28 ms |
| Image pendant le défilement (molette, 60 points par image) | 1,58 ms | 2,13 ms | 2,54 ms |
| Image pendant l'arrivée de 50 messages par seconde | 1,73 ms | 2,27 ms | 2,75 ms |
| Image d'un changement de salon (500 messages chacun) | 3,70 ms | 4,31 ms | 4,79 ms |
| Image pendant le parcours de l'historique (2 000 messages, 71 pages) | 1,60 ms | 2,31 ms | 10,09 ms |

| Mesure | Résultat |
|---|---|
| Redessins au repos (10 s, après 1 s de stabilisation), avec 1 000 présences par seconde injectées | **0 image** ; 0 présence sur 9 993 n'a réveillé l'interface |
| Démarrage de l'aperçu jusqu'à la fenêtre créée | 163 ms |
| Démarrage jusqu'à la première image construite | 182 ms (dont 45 ms pour cette image) |
| Démarrage du processus jusqu'à la première fenêtre visible (mesure extérieure, `mesures.ps1 -Essai`) | 0,76 s au premier lancement, 0,40 s au second |
| Démarrage jusqu'au processeur au calme (« prêt », même mesure) | 1,04 s, puis 0,67 s |
| Historique : remontée puis redescente des 2 000 messages | 39,4 s puis 31,6 s à 400 points par image ; capture finale identique à l'initiale ; jamais plus de 500 messages en mémoire |
| Mémoire après chargement (5 000 salons, 2 500 messages, avatars) | privée 123 Mo, de travail 126 Mo |
| Mémoire à la fin du banc (après 71 pages d'historique et les captures) | privée 234 Mo, de travail 137 Mo |
| Aperçu par défaut au repos (mesure extérieure, `-Essai`) | privée 113 Mo, de travail 106 Mo, 1 processus, 25 fils, 323 handles, 0 % de processeur |

Deux verdicts du banc n'étaient **pas** tenus ce jour-là (changement de
salon au-dessus de 4 ms, défilement bloqué au demi-tour) ; le lot C8
ci-dessous les corrige.

### Lot C8 : optimisation (14 novembre 2026)

Même machine, au calme, même commande ; « avant » = aperçu compilé depuis la
base du lot (`preview-avant.exe`), « après » = branche `lot/optimisation`,
passés l'un après l'autre. Valeurs : 95e centile (maximum).

| Mesure | Avant | Après |
|---|---:|---:|
| Image d'un changement de salon | 4,15 ms (4,46) | **2,12 ms** (2,64) |
| Mise en page du changement de salon (médiane) | 2,96 ms | **1,15 ms** |
| Image au repos | 2,16 ms (3,39) | 2,04 ms (2,38) |
| Image pendant le défilement | 2,12 ms (2,56) | 2,07 ms (2,65) |
| Image pendant l'arrivée de 50 messages par seconde | 2,41 ms (4,29) | 2,15 ms (2,51) |
| Image pendant le parcours de l'historique | 2,33 ms (7,87) | 2,12 ms (5,38) |
| Paires « image du changement = image suivante » | 0 sur 8 | **8 sur 8** |
| Images de la seconde de stabilisation (`area.rs:708`) | 23 | **1** (celle qu'egui accorde après la dernière demande du banc) |
| Défilement effectif (la vue redescend au demi-tour) | non | **oui** |
| Redessins au repos (10 s, 1 000 présences par seconde) | 0 | 0 |
| Première image, temps mesuré par eframe | 95 à 99 ms | **45 à 48 ms** |
| Fenêtre visible (mesure extérieure, médiane de 8 lancements, deux séries) | 450 et 452 ms | 435 et 441 ms |
| Mémoire après chargement (privée / de travail) | 122,6 / 125,3 Mo | 122,4 / 125,6 Mo |
| Mémoire à la fin du banc (privée / de travail) | 226,3 / 137,1 Mo | 255,4 / 162,5 Mo |

Ce qui a changé :

- **Changement de salon.** Les mises en page des messages sont gardées d'une
  image à l'autre (`src/ui/text_cache.rs`, 1 000 au plus, vidé si l'atlas
  des glyphes ou les polices changent) : le cache d'egui ne garde que celles
  de l'image précédente. La liste de messages ne mesure tout de suite que
  l'écran visible (plus un quart d'écran) ; les marges d'un écran au-dessus
  et au-dessous ont un budget de 1,5 ms par image, le reste suit aux images
  suivantes. Attention : environ 0,9 ms du gain vient du banc lui-même, dont
  le faux serveur fabriquait et décodait la liste des membres sur le fil
  d'interface à chaque changement ; il le fait maintenant dans un fil à part,
  comme le fil réseau de l'application.
- **Zone qui s'animait (`egui/containers/area.rs:708`).** C'était le fondu
  d'apparition de la barre d'actions d'un message, refait à chaque
  changement de salon sous la souris (une dizaine d'images) et visible dans
  les captures. La barre a maintenant un seul identifiant et apparaît sans
  fondu, comme dans Discord.
- **Défilement bloqué au demi-tour.** Le pointeur immobile du banc tombait
  sur la barre d'actions, une zone egui au premier plan : la molette allait
  à la barre, pas à la liste. Elle est maintenant rendue à la liste.
  Test : `ui::chat::tests::wheel_over_the_action_bar_still_scrolls`.
- **Démarrage.** Cinq bibliothèques Windows dont la première image n'a pas
  besoin (`d3d11`, `mfplat`, `uiautomationcore`, `mmdevapi`,
  `coremessaging`) sont chargées à la première utilisation (`build.rs`,
  `/DELAYLOAD`). Les étapes de `run` sont écrites dans le journal
  `FERROCORD_PERF` (`etape.*`). Hors ligne, l'aperçu devient visible en
  0,44 s ; le 1,29 s mesuré sur le compte réel reste à revérifier.
- **Redessins au repos.** Chaque événement de la passerelle réveillait
  l'interface (deux images par réveil, d'après egui). Un filtre
  (`src/state/interest.rs`) décide maintenant pour chaque événement :
  réveil ; mis en file sans réveil (message ordinaire dans un salon déjà non
  lu, ou muet ; modification, réaction ou média hors du salon ouvert ; file
  vidée au prochain réveil ou au-delà de 512) ; ou ignoré (« … écrit » hors
  du salon ouvert). Test : `ui::chat::tests::idle_app_asks_for_no_frame`
  (aucune image demandée après 240 images sans événement).

Mémoire à la fin du banc : +29 Mo privés, +25 Mo de travail. La cause n'est
pas vérifiée. Le plafond du cache de mises en page (3 000 puis 1 000) ne
l'a presque pas changée. Le banc défile maintenant vraiment au demi-tour et
parcourt donc plus de contenu. La mémoire après chargement ne change pas.

### Lot M : mémoire (14 novembre 2026)

Le rapport du banc donne maintenant aussi la mémoire après les changements
de salon et après l'historique (`memoire.phases`, mesurées pendant les
captures d'écran : elles comptent les captures gardées) et ce que tiennent
les caches à la fin (`memoire.caches_a_la_fin` : mises en page, textures des
images, textures d'egui et atlas des polices).

Ce que tient la mémoire :

- **Fenêtre OpenGL vide** (`--bench-mem vide`) : 91 Mo privés dès la
  création du contexte, presque tout dans le pilote OpenGL d'AMD
  (`atio6axx.dll`, processeur graphique dédié sur cette machine). L'application
  elle-même ajoute environ 25 Mo après chargement (magasin, polices, images).
- **Textures des images** : le pilote en garde une copie dans la mémoire du
  processus. Plafond du cache 48 → 16 Mo : environ 64 Mo privés de moins en fin
  de banc. Un écran plein d'images tient environ 6 Mo ; l'image en cours
  n'est jamais évincée ; la visionneuse garde ses 12 Mo par image.
- **Mises en page de messages** (`ui/text_cache.rs`) : 1 000 mises en page
  tenaient 20 Mo (environ 170 octets par caractère : glyphe, sommets,
  indices). C'était la cause des +29 Mo après C8. Le plafond est maintenant de
  12 Mo (environ 430). À 6 Mo, le changement de salon refaisait des mises en
  page (médiane 1,2 → 2,5 ms) : écarté.
- **Écartés** : le tas segmenté de Windows (manifeste `heapType`) : pas de gain
  net en fin de banc (171 contre 182 Mo dans une paire, 180 contre 169 dans
  une autre), images 0,2 à 0,4 ms plus lentes. Membres et présences : tous bornés (5 000
  entrées), quelques centaines de Ko. Atlas des polices : 2 Mo.

Banc, séries alternées avant / après (même machine, chargée par d'autres
compilations : les temps se comparent seulement deux à deux).

| Mesure | Avant (2 séries) | Après (2 séries) |
|---|---:|---:|
| Mémoire après chargement (privée) | 123,0 et 121,4 Mo | 139,1¹ et 122,4 Mo |
| Mémoire à la fin du banc (privée) | 265,4 et 251,1 Mo | **194,7 et 185,3 Mo** |
| Mémoire à la fin du banc (de travail) | 163,7 et 157,8 Mo | 171,2 et 148,0 Mo |
| Mises en page gardées à la fin | 811 (19,9 Mo) | 425 (10,9 Mo) |
| Textures des images à la fin | 43,4 et 45,7 Mo | 14,7 et 15,8 Mo |
| Mise en page du changement de salon (médiane) | 2,62 et 1,78 ms | 2,06 et 1,20 ms |
| Image d'un changement de salon (p95) | 4,28 et 4,38 ms | 3,65 et 2,09 ms |

¹ La mémoire après chargement varie d'une série à l'autre (122 ou 139 Mo,
avant comme après, pour le même exécutable) : c'est le pilote, pas le code.

Ce qui reste : la mémoire au repos est d'abord celle du pilote OpenGL. La
réduire demanderait un autre moteur de rendu (wgpu avec Direct3D, à mesurer :
plusieurs Mo d'exécutable en plus), hors de ce lot.

### Lot C9 : reculs mesurés sur le vrai compte (14 novembre 2026)

Point de départ : `mesures/resultats-2026-11-14_1450.md` et son journal
`FERROCORD_PERF` (215 891 lignes, lu en flux). Ce qu'il montre, et ce qui
a été fait :

- **« Prêt » 1,56 → 5,17 s.** La fenêtre est visible à 0,19 s (0,10 s vue
  de l'extérieur, 1,29 s avant C8) : le calme est donc cherché beaucoup plus
  tôt, pendant la connexion, alors qu'avant il commençait après. Le journal
  montre trois choses entre 0,2 et 5,2 s : (1) l'indicateur de chargement
  de la liste de messages, de 0,23 à 0,66 s puis de 1,6 à 2,0 s, qui
  demandait une image à chaque image (120 par seconde, 88 images,
  soit plus que les 100 ms permises par la règle de 5 % sur 2 s) ; (2) le
  READY à 1,58 s (525 événements, lus en une image de 4,6 ms) ; (3) de 3,25
  à 5,25 s, 156 images : 68 dues à des entrées (`cause.entree`), les autres aux
  animations de survol des barres de défilement et à l'attente d'arrêt du
  pointeur des info-bulles (`tooltip.rs_424`) : très probablement la souris sur la
  fenêtre (le journal ne le disait pas encore). Ce troisième point est la fin du « prêt » mesuré ; il ne vient
  pas du programme. Corrigé : l'indicateur est redessiné 30 fois par
  seconde au plus (`src/ui/spinner.rs`, même dessin). Pour le prouver la
  prochaine fois, le journal note la sorte d'entrée (`entree.souris`…) et le
  temps processeur de chaque fil (`fil.*`), et le script en fait deux
  tableaux du démarrage.
- **Pic du « changement de salon » 9,4 → 14 ms.** Les 14 ms ne sont pas un
  changement : c'est l'image où le premier salon s'affiche au démarrage
  (historique hors ligne, à 231 ms), écran entier froid. Les cinq vrais
  changements font 0,8 à 4,8 ms. Le journal la note maintenant à part
  (`premier_salon`). Reproduite dans l'aperçu (`--view connexion --scene
  hors-ligne`, qui l'écrit) : 11,7 à 13,0 ms. Corrigé : l'instantané est
  converti en modèles dans le fil de l'historique (`Snapshot::prepare`), les
  glyphes courants sont préparés une police par image pendant l'attente
  (`src/ui/warm.rs`), et la toute première liste de messages est dessinée à
  l'image suivante. Résultat : 5,2 à 8,1 ms, puis 9,7 ms (la liste et la
  liste des membres, qui arrivait déjà à cette image-là). Moins de 4 ms
  n'est pas atteint pour cette image froide.
- **Réveils au repos : 54 images en 60 s.** `cause.ui_mod.rs_406` est le
  réveil de la passerelle (`backend.set_wake`) : un par événement réveillant,
  donc les 66 `passerelle.reveil.message_update`, 5 `channel_update`, 2
  messages et 1 état vocal. Les MESSAGE_UPDATE du salon ouvert réveillaient
  tous. Maintenant, une modification, un média ou une réaction sur un
  message du salon ouvert **hors de la partie affichée** attend l'image
  suivante (`Interest::show_messages`, publié à chaque image par la liste).
  Test : `state::interest::tests::open_channel_updates_wake_only_when_shown`.
  Banc : 20 MESSAGE_UPDATE par seconde sur des messages hors de l'écran
  pendant l'observation au repos : 0 image.
- **Mémoire au repos 122 → 130 Mo.** Rien n'était gardé deux fois (la police
  est partagée par egui, l'instantané hors ligne est rendu après usage). Les
  cinq fichiers de Segoe UI (4,8 Mo, la demi-grasse `seguisb.ttf` venue avec
  la refonte comprise) étaient copiés dans le tas : ils sont maintenant
  projetés en mémoire (`src/platform/mapped.rs`), pages du cache du système.

Banc (`--bench`, machine chargée par d'autres compilations : les temps se
comparent deux à deux ; « avant » = `e8ccfb4`, la première série « avant »,
froide et sous charge, est écartée). Image : médiane / 95e centile /
maximum (ms).

| Mesure | Avant | Après (2 séries) |
|---|---:|---:|
| Image au repos | 1,69 / 2,00 / 2,18 | 1,63 / 2,00 / 2,30 ; 1,61 / 1,96 / 2,48 |
| Image pendant le défilement | 1,59 / 2,03 / 2,48 | 1,80 / 2,15 / 2,76 ; 1,79 / 2,07 / 2,79 |
| Image pendant l'arrivée de 50 messages par seconde | 1,86 / 2,28 / 2,48 | 1,92 / 2,43 / 3,06 ; 1,83 / 2,39 / 2,67 |
| Image d'un changement de salon | 1,81 / 2,25 / 2,49 | 2,26 / 2,87 / 4,02 ; 1,77 / 2,27 / 2,45 |
| Redessins au repos (10 s, 1 000 présences par seconde) | 0 | 0 ; 0 |
| MESSAGE_UPDATE hors de l'écran ayant réveillé (20 par seconde, 10 s) | 199 sur 199¹ | **0 sur 199** ; **0 sur 199** |
| Mémoire après chargement (privée / de travail) | 123,9 / 124,4 Mo | **119,7 / 118,9 ; 118,7 / 119,8 Mo** |
| Image du premier salon, scène `hors-ligne` (3 lancements) | 11,7 ; 13,0 ; 11,7 | 8,1 puis 16,0² ; 5,4 puis 9,7 ; 5,2 puis 9,7 |

¹ Le banc d'avant n'injecte pas ces événements ; avant C9, le filtre
réveillait pour toute modification du salon ouvert (test
`only_invisible_events_wait`), donc toutes.
² Premier lancement, disque et polices froids.

À vérifier sur le vrai compte : « prêt » sans toucher la souris (le
tableau des entrées du démarrage le dira), temps par fil au démarrage,
`premier_salon`, images au repos.

### Lot « perf » : « prêt » et repos sur le vrai compte (15 novembre 2026)

Point de départ : `mesures/resultats-2026-11-15_1540.md` et son journal
(214 793 lignes). Ferrocord y perd sur deux lignes : « prêt » à 19,68 s
contre 7,22 s, et un repos pas silencieux (83 images en 60 s, processeur
maximal sur 1 s de 6,16 % contre 4,67 %).

- **« Prêt » à 19,68 s : c'était l'instrument de mesure.** Le relevé des
  fils (`fil.*`, lot C9) refaisait à chaque relevé la liste de tous les fils
  du système (`CreateToolhelp32Snapshot`), quatre fois par seconde pendant
  20 s. Le journal le dit lui-même : `fil.ferrocord_perf_fils` consomme 62 à
  94 ms par seconde de la 1re à la 20e seconde (1 719 ms en tout, premier
  poste du tableau), même pendant les secondes où rien d'autre ne tourne
  (entre 12 et 13 s, entre 19 et 20 s, par exemple). À lui seul il dépasse le seuil de calme
  (5 % d'un cœur sur 2 s), et le calme arrive quand le rythme serré s'arrête.
  Recalculé sur le journal avec la règle du script (première fenêtre de 2 s
  sous 100 ms, relevés de 250 ms) : 19,25 s avec le relevé (19,68 s
  mesurés), **2,75 s sans lui**. Entre 3 et 20 s, le reste est la souris
  sur la fenêtre (95 des 337 images, entre 2 et 3 s, 6 et 8 s, 11 et 12 s) et les
  réveils de la passerelle traités plus bas.
  Corrigé (`src/perf/threads.rs`) : les fils connus restent ouverts et un
  relevé ne lit que leurs compteurs ; la liste n'est refaite que si du temps
  processeur du processus n'est attribué à aucun fil connu (au plus toutes
  les 2 s), et sinon une fois par minute. Sur cette machine : liste 30,9 ms,
  relevé des fils connus 3 µs. Test de bout en bout
  (`the_sampler_stays_far_below_the_calm_threshold`) : le relevé tourne 3 s
  au rythme du démarrage et doit avoir consommé moins de 5 % d'un cœur
  (plus de 10 % avant).
- **`sans_nom` (469 ms).** Aucun fil de Ferrocord n'est sans nom : ce sont
  ceux que créent le pilote OpenGL et la réserve de fils de Windows. Leur
  temps suit les images (109 ms à la 1re seconde, 125 à la 3e, 94 à la 8e,
  62 à la 12e : les secondes où l'on dessine). Ils portent maintenant le nom
  du module où ils démarrent (`fil.sans_nom.atio6axx`, `fil.sans_nom.ntdll`…) :
  la prochaine mesure dira lequel.
- **83 images au repos : trois réveils par MESSAGE_UPDATE.** Les 114
  `cause.ui_mod.rs_540` sont exactement les 114 événements « réveil » (96 +
  9 + 4 + 3 + 1 + 1). Un MESSAGE_UPDATE complet (Discord en envoie un quand
  il ajoute l'aperçu d'un lien) donne cinq événements : texte et médias, qui
  attendaient déjà hors de l'écran, mais aussi épinglage, drapeaux d'annonce
  et composants d'application, qui réveillaient toujours. Le journal montre
  ce motif 32 fois : « 2 réveils, 2 différés, 1 réveil ». Maintenant ces
  trois-là attendent aussi quand le message n'est pas à l'écran (autre salon,
  ou salon ouvert hors de la partie affichée), comme les votes de sondage ;
  une suppression attend dans un salon qui n'est pas à l'écran ; un
  CHANNEL_UPDATE attend en entier (jusqu'à huit événements par paquet) quand
  son serveur n'est pas à l'écran (colonne des salons, panneau vocal, salons
  secondaires) et que ni le type, ni la catégorie, ni les permissions du
  salon ne changent : le premier paquet d'un salon et tout changement de
  visibilité réveillent, parce qu'ils peuvent changer une pastille du rail.
  Restent des réveils, à dessein : un nouveau message qui fait apparaître un
  non-lu ou une mention, les états vocaux, et tout ce qui touche un message,
  un salon ou un serveur affiché. La réponse à « Publier » réveille toujours.
  Code : `src/state/interest.rs`, `src/net/mod.rs` (`Out::calm`),
  `src/net/gateway.rs`, `src/ui/interest.rs`.
- **Compteurs et notifications.** Aucun événement mis en attente ne change
  un compteur de non-lus ou de mentions ni ne produit de notification ou de
  son : ceux qui le font réveillent toujours (test
  `state::interest::tests::deferred_events_change_no_counter`, qui applique
  au magasin chaque événement différé et compare les compteurs).
- **Banc : phase « trafic ».** Ce recul ne se voyait que sur un vrai
  compte. Le banc rejoue maintenant des paquets de la passerelle en JSON par
  le décodage et le filtre de la connexion (`net::Replay`, sans réseau) : une
  rafale de démarrage (122 paquets), puis `--bench-quiet` secondes à 3 paquets
  par seconde dans des salons qui ne sont pas à l'écran (MESSAGE_UPDATE
  complet et partiel, MESSAGE_DELETE, CHANNEL_UPDATE, MESSAGE_CREATE d'un
  salon déjà non lu), puis cinq paquets qui changent l'écran. Deux verdicts :
  `zero_image_au_repos_sous_trafic` et `paquets_visibles_reveillent`.

Banc (`--bench`, même exécutable à la règle près ; « avant » : la phase
« trafic » avec l'ancienne règle ; deux séries chacune, machine occupée par
ailleurs à 1 à 40 %).

| Mesure | Avant (2 séries) | Après (2 séries) |
|---|---:|---:|
| Repos sous trafic (10 s, 29 paquets, 84 événements) : événements ayant réveillé | 55 ; 55 | **0 ; 0** |
| Repos sous trafic : images dessinées | 40 ; 40 | **0 ; 0** |
| Rafale de démarrage (122 paquets, 402 événements) : événements ayant réveillé | 322 ; 322 | 202 ; 202 |
| Rafale de démarrage : images dessinées (2 s) | 23 ; 24 | 23 ; 23 |
| Paquets qui changent l'écran : réveillés | 5 sur 5 | 5 sur 5 |
| Redessins au repos (10 s, 1 000 présences par seconde) | 0 ; 0 | 0 ; 0 |
| Image au repos (médiane / p95, ms) | 1,56 / 1,97 ; 1,56 / 1,94 | 1,58 / 2,19 ; 1,49 / 1,86 |
| Image d'un changement de salon (médiane / p95, ms) | 1,81 / 2,27 ; 1,92 / 2,44 | 1,89 / 2,42 ; 1,88 / 2,35 |

La rafale donne le même nombre d'images avant et après : egui regroupe déjà
les réveils rapprochés (une image pour tout ce qui est arrivé depuis la
précédente) ; ses 23 images sont les animations des pastilles qui
apparaissent. Les 202 réveils qui restent sont dus : premier message non lu
de 40 serveurs et premier CHANNEL_UPDATE de 40 salons.

La seconde de stabilisation avant le repos compte maintenant une vingtaine
d'images (animations de survol et de barre de défilement à la sortie de la
phase « historique », `motion.rs` et `scroll_area.rs`), avant comme après ce
lot : elles viennent de la base du lot, pas de lui, et s'arrêtent seules.

À confirmer par une nouvelle passe de `tools\mesures.ps1` sur le vrai compte
(rien de ceci n'a pu y être mesuré) : « prêt » vers 3 s si la souris reste
hors de la fenêtre ; `fil.ferrocord_perf_fils` de l'ordre de 100 ms sur le
démarrage au lieu de 1 719 ; images au repos : de l'ordre de 10 à 20 au lieu de 83
pour le même trafic, par estimation (les 32 MESSAGE_UPDATE ne réveillent plus ; restent les
nouveaux non-lus, les états vocaux, et les CHANNEL_UPDATE du serveur affiché
ou vus pour la première fois) ; processeur maximal sur 1 s au repos.

### Lot « ram » : mémoire juste après le démarrage (16 novembre 2026)

Demande : 160 Mo « à peine lancé », c'est trop. Le banc rejoue maintenant le
démarrage d'un compte **simulé** (`preview --bench-mem demarrage` : READY de
100 serveurs de 50 salons, 30 rôles et 80 emojis par serveur, 150 MP, 300
relations, 10 000 jeux détectables, un salon affiché) et dit où va la
mémoire. Trois compteurs, à ne pas confondre :

- **privée** : mémoire engagée par le processus, touchée ou non (ce que
  `tools\mesures.ps1` relève) ;
- **de travail privée** : la part réellement en mémoire vive, la colonne
  « Mémoire » du Gestionnaire des tâches ;
- **tas vivant** : les octets que le programme tient lui-même (allocateur
  compteur de l'aperçu, `examples/bench/alloc.rs`). C'est la seule mesure
  exacte au Mo près : les deux autres bougent de ± 5 Mo d'un lancement à
  l'autre à cause du pilote graphique.

Répartition de la mémoire privée 3 s après la première image (Mo, machine
calme : charge de 2 à 7 %, 4 passes alternées avant / après, exécutable
optimisé ; premier lancement de chaque exécutable écarté, il coûte 17 Mo de
plus) :

| Poste | Avant | Après |
|---|---:|---:|
| Fenêtre OpenGL vide (`--bench-mem vide`) : pilote `atio6axx.dll` | 99,2 à 99,3 | 98,3 à 98,9 |
| – dont tas du pilote (il passe par le tas du processus) | 56 à 59 | 56 à 59 |
| – dont réservations directes du pilote | 32 | 32 |
| READY décodé et magasin (tas vivant) | 8,06 | **5,03** |
| Jeux détectables (tas vivant) | 6,33 en 60 006 blocs | **1,17 en 6 blocs** |
| Première image : polices, atlas, mises en page (tas vivant) | 3,57 | 2,97 |
| 3 s d'images suivantes (tas vivant) | 1,12 | 0,71 |
| **Tas vivant, total** | **20,3** (109 000 blocs) | **11,1** (49 000 blocs) |
| Pointe du tas pendant le READY | 19,7 | 16,7 |
| **Mémoire privée** | **131,4 à 132,2** | **121,2 à 121,8** |
| Mémoire de travail | 126,3 à 129,4 | 116,1 à 117,4 |
| Mémoire de travail privée (Gestionnaire des tâches) | 86,9 à 94,4¹ | 74,0 à 75,3 |

¹ Relevée sur l'exécutable d'itération (sans optimisation à l'édition de
liens) : l'exécutable « avant » optimisé n'avait pas encore ce compteur.

Gain : **10,3 Mo privés** (dispersion ± 0,4 de chaque côté), environ 13 Mo
de travail privé, tas vivant presque divisé par deux. L'objectif « moins de
100 Mo privés » n'est **pas** atteint et ne peut pas l'être sans changer de
moteur de rendu : la fenêtre vide en tient déjà 98, tout dans le pilote
OpenGL. En mémoire de travail privée (ce que montre le Gestionnaire des
tâches), on est à 75 Mo.

Corrections gardées, dans l'ordre (chacune remesurée, 3 passes) :

| Correction | Tas vivant | Mémoire privée | De travail privée |
|---|---:|---:|---:|
| Départ | 20,3 | 131,7 à 131,9 | 86,9 à 94,4 |
| Jeux détectables en index compact (`state/activity.rs`, `net/games.rs`) : un texte unique et des tables triées au lieu d'une `String` par texte et de deux tables de hachage ; décodage au fil de l'eau | 15,1 | 124,7 à 125,3 | ≈ 81 |
| Atlas des polices borné à 8 192 pixels de large (`ui/fonts.rs`) : egui l'ouvrait à 16 384 × 32 | 14,1 | 123,7 à 124,4 | 77,6 à 78,3 |
| Listes des serveurs ramenées à leur taille après le READY (`Guild::shrink_to_fit`) : la collecte gardait la capacité de la forme brute | 11,1 | 121,1 à 121,7 | 73,9 à 74,5 |

Essayé puis retiré (aucun gain mesuré) :

- `HeapCompact` après le READY : tas engagé 68,6 → 68,6 Mo ;
- recopie des serveurs dans des blocs neufs après le READY (contre la
  fragmentation) : rien ;
- atlas à 4 096 : identique à 8 192 ; à 2 048 : 0,5 Mo de mieux, mais sous
  la taille maximale des images (egui le refuse en débogage) : écarté.

Vitesse (banc complet, 3 séries alternées avant / après, médianes) :

| Mesure | Avant | Après |
|---|---:|---:|
| Première image construite | 198,5 / 181,7 / 194,5 ms | 185,9 / 195,8 / 199,3 ms |
| Image d'un changement de salon, médiane | 2,01 / 1,77 / 1,70 ms | 2,03 / 2,02 / 1,98 ms |
| Image d'un changement de salon, p95 | 2,39 / 2,22 / 2,20 ms | 2,48 / 2,37 / 2,45 ms |
| Image du défilement, médiane | 1,89 / 1,64 / 1,58 ms | 1,80 / 1,78 / 1,82 ms |
| Image du défilement, p95 | 2,70 / 2,24 / 2,14 ms | 2,43 / 2,28 / 2,37 ms |
| Redessins en 10 s de repos | 0 / 0 / 0 | 0 / 0 / 0 |
| Images au repos sous trafic | 0 | 0 |

Deux séries « avant » sur trois sont plus rapides de 0,2 ms par image ; la
troisième est égale. Les mêmes étapes compilées sans optimisation à l'édition
de liens donnent l'inverse (1,98 ms par image au repos avant, 1,65 après,
deux séries chacune) : l'écart suit la compilation, pas les corrections, dont
aucune ne touche au chemin d'une image. Tous les budgets restent tenus avec
la même marge (4 ms). Captures d'écran avant / après identiques à l'octet
près (vues `guild`, `amis`, Paramètres ; la vue `dm` diffère d'un lancement
à l'autre du même exécutable, avant comme après).

Après un long usage : le banc à 300 puis à 1 500 images par phase finit avec
le même tas vivant (23,7 Mo dans les deux cas, 42 900 et 43 500 blocs), les
mêmes caches (mises en page 10 Mo, textures des images 15 Mo, tous deux
plafonnés) et la même mémoire privée (176 à 188 Mo selon la série, avant
comme après) : pas de croissance lente. La différence entre le démarrage
(121 Mo) et la fin du banc vient des caches d'images et de mises en page
(plafonds du lot M, inchangés : la qualité des images n'est pas touchée) et
de la copie que le pilote garde de chaque texture.

Ce qui dépend de la taille du compte (compte simulé, tas vivant) : moitié
moins de serveurs, 8,6 Mo (privée 117,5 à 117,8) ; le double, 16,2 Mo
(privée 127,8 à 134,8). Soit environ 5 Mo par centaine de serveurs de cette
taille ; les jeux détectables (1,2 Mo) et le socle (polices, atlas, première
image : 5 Mo) n'en dépendent pas. **Le vrai compte n'a pas été mesuré** (les
agents ne s'y connectent pas) : ses serveurs n'ont ni le même nombre de
salons, ni de rôles, ni d'emojis que ceux du banc, et les 160 Mo relevés
comprennent sans doute des images déjà chargées. À confirmer par
`tools\mesures.ps1`, qui donne maintenant la mémoire à chaque étape du
démarrage et sa répartition (voir le journal, plus bas).

Garde-fous ajoutés :

- `--bench` imprime « mémoire au repos après démarrage » (privée, de
  travail, de travail privée, tas vivant) et sa répartition (tas, tas alloué,
  réservations directes, modules, piles), et un verdict
  `memoire_au_repos_apres_demarrage` : le tas vivant (11,1 Mo mesurés) doit
  rester sous 16 Mo ;
- `--bench-mem demarrage` sort en code 1 si le tas vivant du compte simulé
  (11,1 Mo) dépasse 16 Mo ; `demarrage-parties` pèse chaque partie du
  magasin, `FERROCORD_BANC_TRACE=<octets>` nomme les blocs survivants par
  pile d'appels ;
- tests : `index_stays_compact_and_keeps_the_list`,
  `ready_guilds_keep_no_spare_capacity`, `atlas_width_is_capped_not_raised`,
  `perf::memory::tests`.

Ce qui reste, non fait :

- **Le pilote OpenGL : 98 Mo sur 121.** Seul un autre moteur de rendu le
  retirerait (wgpu sur Direct3D 12 : à mesurer, plusieurs Mo d'exécutable et
  des jours de travail ; le contexte OpenGL d'eframe ne se règle pas plus
  bas). **Fait au lot « gl », plus bas** : ANGLE (Direct3D 11) à la place
  du pilote OpenGL.
- Un autre allocateur (mimalloc) : notre tas ne pèse que 11 Mo vivants et
  environ 4 Mo de fragmentation après le READY ; le pilote garderait le tas
  de Windows. Gain estimé : 0 à 4 Mo, contre une dépendance et une arène
  réservée d'avance. Non essayé (aucun téléchargement dans ce lot).
- Décoder le READY serveur par serveur au lieu d'un arbre entier, et ne le
  relire qu'une fois (plusieurs décodeurs le reparcourent : communauté,
  social, fils, relations) : pointe de 16,7 Mo et fragmentation en moins,
  2 à 4 Mo privés estimés, et du temps processeur au démarrage.
- Emojis tenus deux fois (serveur et social), table des rôles (0,5 Mo),
  index des emojis intégrés (0,5 Mo), icône par défaut d'eframe décodée au
  démarrage (1 Mo ; la remplacer changerait l'icône) : 2 à 3 Mo en tout.

### Lot « cpu » : processeur et démarrage (16 novembre 2026)

Demande : réduire l'impact sur le processeur et le temps de démarrage, sans
changer un pixel. Méthode : mesurer d'abord, ne garder que ce qui gagne.

**Nouveau banc : `preview --bench-cpu <secondes>`** (`examples/bench/cpu.rs`).
Il rejoue le démarrage d'un compte **simulé** comme le fait la connexion : la
fenêtre s'ouvre sur un magasin vide, un fil joue la passerelle (READY de 100
serveurs, 4,76 Mo, décodé par le vrai décodage), le salon s'ouvre, sa
dernière page arrive. Il mesure, pour tout le processus (`GetProcessTimes` :
interface, pilote graphique, décodage) : le processeur consommé du lancement
jusqu'au calme et le délai jusqu'au calme (règle de `tools\mesures.ps1` :
première fenêtre de 2 s sous 5 % d'un cœur), le READY (durée, allocations,
image qui l'applique), les jeux détectables, le repos sans trafic puis sous
trafic simulé (3 paquets par seconde hors de l'écran : moyenne, pointe sur
1 s, images, fils qui ont travaillé), le coût d'un paquet de chaque sorte
courante, et le coût d'un relevé de la surveillance des fichiers. L'horloge
du système compte par pas de 15,6 ms : un écart de moins de 30 ms sur une
seule passe ne veut rien dire, d'où les six passes.

**Profil avant** (banc, machine calme : charge de 2 à 8 %, aucun `rustc`) :

| Poste | Mesure |
|---|---:|
| Du lancement à la première image | 212 à 228 ms, 203 à 312 ms de processeur |
| – dont fenêtre et contexte OpenGL (pilote) | ≈ 160 ms (journal du vrai compte) |
| READY (4,76 Mo) : décodage, hors du fil d'interface | 138 à 150 ms, 605 000 à 611 000 allocations, 1 207 événements |
| READY : image qui l'applique au magasin | 7,4 à 10 ms |
| Jeux détectables (2,55 Mo), hors du fil d'interface | 34 à 36 ms, 300 531 allocations |
| Processeur du lancement au calme | 562 à 672 ms |
| – fil d'interface | 360 à 390 ms |
| – décodage du READY (fil de la passerelle) | 156 à 172 ms |
| – pilote OpenGL (`sans_nom.atio6axx`) | 62 à 109 ms |
| – tokio (images) | 16 à 47 ms |
| Repos sans trafic, 10 s | 0 image, 0 à 16 ms de processeur (un pas d'horloge) |
| Repos sous trafic (29 paquets en 10 s) | 0 image, 0 à 31 ms, aucun réveil |
| Un paquet : MESSAGE_CREATE / MESSAGE_UPDATE complet / MESSAGE_DELETE | 2,7 / 6,5 / 0,5 µs ; 15 / 32 / 3 allocations |
| Un paquet : TYPING_START / PRESENCE_UPDATE / MESSAGE_REACTION_ADD | 1,7 / 1,1 / 3,1 µs ; 13 / 8 / 28 allocations |

(La répartition par fil et le repos viennent des passes « après », dont le
banc est plus fin ; aucune correction ne touche ces postes.)

Ce que le profil dit : au repos, le banc ne trouve plus rien à retirer dans
le traitement des paquets (trois paquets par seconde coûtent 20 µs par
seconde). Ce qui restait au repos était ailleurs, et se lit dans le journal
du vrai compte du 15 novembre (`fil.*` entre 30 et 95 s, 65 s de repos) :
relevé lui-même 328 ms (corrigé par le lot « perf »), fil d'interface 203 ms
et pilote 109 ms (les 83 images, corrigées par le lot « perf »), puis
**`theme` 78 ms et `settings` 31 ms** : les deux fils qui surveillent
`theme.toml` et `settings.toml`, soit 0,17 % d'un cœur en permanence, le
premier poste une fois les images retirées.

**Corrections gardées**

1. **Surveillance de `theme.toml` et `settings.toml` par signal du système**
   (`src/platform/dirwatch.rs`, `src/theme/watch.rs`). La date du fichier
   était relevée quatre fois par seconde (thème) et une fois par seconde
   (réglages). Windows prévient maintenant d'un changement dans le dossier
   (`FindFirstChangeNotificationW`) : interroger ce signal ne touche pas au
   système de fichiers, et le fichier n'est relevé que lorsqu'il est levé
   (plus un relevé de sûreté toutes les 5 s pour le thème, 20 s pour les
   réglages, au cas où le système n'aurait pas prévenu). Le délai de prise
   en compte d'une modification ne change pas (même rythme d'interrogation).
   Sans signal (dossier absent, autre système), l'ancien relevé reste.
   Coût d'une interrogation, mesuré par le banc : 31 µs pour le relevé du
   fichier (dossier temporaire ; environ 300 µs sur le vrai compte d'après le
   journal), **0,2 µs** pour le signal. Sur le vrai compte, attendu : 109 ms
   de processeur par minute de repos → moins de 5 ms.
2. **Icône de la fenêtre réduite d'avance** (`src/platform/app_icon.rs`,
   `assets/icone/`). Sans icône fournie, eframe décodait la sienne (PNG de
   512 × 512) avant la fenêtre, en donnait une copie d'un Mo à winit, puis la
   réduisait deux fois (filtre de Lanczos) et recodait chaque réduction en
   PNG, sur le fil d'interface, juste après la première image (5 à 6 ms par
   réduction, mesuré à part). Les réductions sont maintenant dans
   l'exécutable (onze tailles : grande et petite icône de 100 à 300 %
   d'échelle, 28 Ko en tout) et posées par le même appel de Windows ; une
   taille imprévue est réduite au vol, comme avant. **L'icône est la même** :
   un test refait chaque réduction avec la chaîne d'eframe et compare les
   pixels.
   *Depuis le lot « icône »* : l'icône est le logo de Ferrocord, plus le
   « e » d'eframe. Elle est rangée une seule fois, dans les ressources de
   l'exécutable (`assets/icone/ferrocord.ico`, produit par le code et lié
   par `build.rs`), et la fenêtre la prend là (`LoadImageW`) ; une taille
   absente du fichier est dessinée au vol (quelques microsecondes). Plus
   rien n'est décodé ni réduit au lancement, et les réductions PNG ont
   quitté le dépôt.

**Avant / après** (banc `--bench-cpu 10`, six passes alternées avant / après,
première passe de chaque exécutable écartée, charge de 2 à 8 %) :

| Mesure | Avant (min – médiane – max) | Après (min – médiane – max) |
|---|---:|---:|
| Processeur du lancement à la première image (ms) | 203 – 250 – 312 | 219 – 234 – 250 |
| Délai de la première image (ms) | 212 – 216 – 228 | 216 – 220 – 229 |
| Processeur du lancement au calme (ms) | 562 – 617 – 672 | 406 – 508 – 578 |
| Délai jusqu'au calme (ms) | 614 – 620 – 635 | 367 – 618 – 623 |
| Images jusqu'au calme | 36 à 38 | 37 (9 et 10 quand le calme arrive plus tôt) |
| Décodage du READY (ms) | 138 – 140 – 150 | 133 – 137 – 140 |
| Images au repos, sans et sous trafic | 0 | 0 |
| Un relevé de la surveillance des fichiers (µs) | 31 | 0,2 |

Le processeur au repos ne se compare pas entre les deux exécutables : celui
d'« avant » porte la première version du banc, dont la boucle d'observation
se réveillait 200 fois par seconde (0,6 à 0,9 % d'un cœur à elle seule).
Avec le banc corrigé : 0 à 0,16 % sans trafic, 0 à 0,31 % sous trafic, pointe
sur 1 s de 0 à 1,6 % (un pas d'horloge), et le seul fil qui travaille est
celui du banc.

Lecture honnête : le processeur jusqu'au calme baisse d'environ 70 à 110 ms
sur 620 (les quatre passes « après » au même point de calme : 469 à 578 ms,
médiane 547). C'est plus que les 12 ms de réductions mesurées à part : le
reste est la copie d'un Mo, l'icône de 512 × 512 que winit créait, et du
bruit (pas d'horloge de 15,6 ms). Le délai de la première image ne bouge pas
(eframe posait l'icône après elle). Le calme arrive plus tôt dans deux passes
sur six seulement : le travail restant frôle le seuil de 5 %.

**Examiné, puis laissé** (aucun gain à attendre d'après le profil) :

- décodage partiel des paquets ignorés, regroupement des minuteries, nombre
  de fils tokio : un paquet coûte 0,5 à 6,5 µs ; tokio a déjà deux fils ;
  aucun fil ne se réveille en boucle hors des deux surveillances de fichiers
  corrigées ;
- fenêtre réduite ou masquée : eframe 0.37 ne dessine déjà plus et endort sa
  boucle (`sleep_if_invisible_or_minimized`) ; non mesuré ici (le banc ne
  réduit pas la fenêtre) ;
- envoi des textures par lots, cache des mises en page : déjà en place
  (lots C8 et M), le banc `--bench` reste sous 4 ms par image.

**Non fait, par gain estimé** :

- **READY décodé en une passe** : c'est le premier poste du démarrage
  (140 ms sur 620, 610 000 allocations). Le texte est relu en entier par cinq
  décodeurs (principal, relations et états de lecture, communauté, vocal,
  appels), et chaque champ « tolérant » (`Item`, `de_lenient`, très nombreux
  dans `src/net`) recopie son texte avant de le relire. Une structure commune
  par serveur, décodée une fois, et un `Item` sans copie : 60 à 80 ms et
  300 000 allocations de moins, estimés ; plusieurs heures et un risque sur
  la tolérance aux formes inattendues, d'où le report. Ce temps est hors du
  fil d'interface : il retarde l'affichage des serveurs, pas la fenêtre.
- Jeux détectables : 300 000 allocations transitoires (35 ms, une fois, 60 à
  90 s après la connexion, hors du fil d'interface). Décodeur au fil de l'eau
  sans `String` par champ : 20 ms estimés.
- Fenêtre et contexte OpenGL : 160 ms sur les 216 de la première image, dans
  le pilote. Hors de ce lot (rendu).
- Profil de compilation : rien à proposer (`lto = "fat"`, une unité de
  compilation : déjà le plus rapide à l'exécution).

**Garde-fous** : captures de l'aperçu identiques à l'octet près avant / après
(`salon`, `amis`, Paramètres : `pu-compte`, `pu-apparence`,
`pu-notifications`) ; `--bench-mem demarrage` : 11,1 Mo vivants avant comme
après (seuil 16 Mo, tenu) ; 0 image au repos, sans et sous trafic ; tests :
`platform::dirwatch::tests`, `platform::app_icon::tests`, et ceux de
`theme::watch` et `settings`, qui passent maintenant par le signal.

**À confirmer sur le vrai compte** (`tools\mesures.ps1`, les agents ne s'y
connectent pas) : dans « temps processeur par fil », `fil.theme` et
`fil.settings` proches de zéro au repos (78 et 31 ms en 65 s avant) ;
processeur moyen au repos ; qu'une modification de `theme.toml` dans un
éditeur s'applique toujours en moins d'une seconde ; l'icône de la fenêtre et
de la barre des tâches inchangée ; la durée réelle du décodage du READY
(taille du vrai READY inconnue : le banc en simule un de 4,76 Mo).
### Lot « gl » : le pilote graphique (16 novembre 2026)

Demande : réduire les 98 Mo que la fenêtre OpenGL vide prend sur les 121 du
démarrage, sans changer un pixel. Résultat : **le contexte OpenGL est demandé
à ANGLE** (la traduction OpenGL ES → Direct3D 11 de Chromium) au lieu du
pilote OpenGL du fabricant de la carte. egui dessine par le même `glow` ;
seul le pilote chargé change.

| `preview --bench-mem`, 3 s après la première image (Mo, 3 passes) | Avant (pilote OpenGL, carte dédiée¹) | ANGLE, carte dédiée | ANGLE, carte intégrée² |
|---|---:|---:|---:|
| Fenêtre vide : privée | 98,1 à 98,6 | 52,3 à 52,8 | **27,3 à 28,0** |
| Fenêtre vide : de travail privée (Gestionnaire des tâches) | 46,3 à 46,7 | 26,9 à 27,0 | **11,0 à 11,3** |
| Démarrage du compte simulé (`demarrage`) : privée | 120,4 à 121,7 | 86,5 à 86,6 | **61,0 à 61,7** |
| Démarrage du compte simulé : de travail privée | 74,2 à 74,7 | 48,2 à 48,6 | **30,5 à 31,1** |
| Démarrage du compte simulé : tas vivant | 11,1 | 11,1 | 11,1 |

Soit, au démarrage du compte simulé, **60 Mo privés de moins** (121,5 → 61,5)
et **44 Mo de moins dans le Gestionnaire des tâches** (74,4 → 30,6). Le nouvel
exécutable **sans** ANGLE à côté de lui (le repli) mesure comme l'ancien :
98,2 à 99,4 Mo privés fenêtre vide, 113,9 à 121,8 au démarrage. Charge de la
machine pendant ces passes : 2 à 9 %.

¹ Ce que fait la machine aujourd'hui, branchée au secteur : le pilote AMD
envoie un programme inconnu sur la carte **dédiée** (RX 5600M), bien que
l'écran soit branché sur l'intégrée. Sur l'intégrée, le pilote OpenGL est
pire encore : 167 Mo privés et 145 Mo de travail privé pour une fenêtre vide,
611 ms avant la première image (c'est sans doute ce que montre le
Gestionnaire des tâches sur batterie : les « 160 Mo » de départ).
² Ce que fera l'application installée : elle demande à Windows la carte
économe (voir plus bas). L'aperçu, lui, ne touche pas au registre : cette
colonne est mesurée avec une préférence posée pour le seul exécutable
d'essai, puis retirée.

#### Méthode : une fenêtre d'essai minimale

Un petit programme hors dépôt (egui seul sur un contexte `glutin`, 1100 × 720,
200 images forcées puis 3 s de repos, médiane de 3 passes) a isolé chaque
réglage. La machine compilait par moments : les temps sont indicatifs
(± 15 ms sur la première image), la mémoire est stable à ± 1 Mo.

| Piste | Privée (Mo) | De travail privée (Mo) | Première image (ms) | Image médiane / p95 (ms) | Processeur par image (ms) | Pixels |
|---|---:|---:|---:|---:|---:|---|
| Pilote OpenGL, réglages d'eframe (référence) | 99,7 | 68,2 | 207 | 2,17 / 2,74 | 3,1 | référence |
| Tampon de profondeur 24 bits + stencil 8 bits | 107,3 | 68,3 | 221 | 2,23 / 2,80 | 3,1 | – |
| MSAA × 4 | 145,2 | 69,1 | 215 | 2,21 / 2,68 | 3,0 | – |
| Sans synchronisation verticale | 107,4 | 68,7 | 223 | 1,59 / 2,05 | 2,6 | – |
| Fenêtre transparente (Mica, incrustation) | 100,1 | 68,3 | 210 | 1,98 / 2,64 | 2,8 | – |
| Profil de compatibilité, OpenGL 2.1, 4.6 | 99,7 à 100,5 | 68,1 à 68,6 | 203 à 220 | 2,02 à 2,24 | 2,9 à 3,3 | – |
| Simple tampon | 100,2 | 68,2 | 226 | 2,22 / 2,81 | 3,5 | – |
| Rendu logiciel (`hardware_acceleration: Off`) | – | – | – | – | – | pas de contexte : l'OpenGL 1.1 de Windows ne suffit pas à egui |
| Pilote OpenGL sur la carte intégrée | 167,5 | 145,4 | 611 | 2,10 / 2,63 | 4,6 | – |
| **ANGLE (Direct3D 11), carte dédiée** | **60,5** | **29,9** | 207 | 2,22 / 2,76 | 3,1 | 1/255 sur < 0,01 % |
| ANGLE, OpenGL ES 2 au lieu de 3 | 59,8 | 29,4 | 203 | 2,03 / 2,66 | 3,3 | – |
| **ANGLE, carte intégrée** | **39,1** | **18,0** | **157** | 2,15 / 2,81 | 2,5 | 1/255 sur < 0,01 % |
| wgpu : Direct3D 12, Vulkan, OpenGL | 91 à 277 | 62 à 182 | 299 à 708 | 1,87 à 2,38 | 3,5 à 11,3 | voir `docs/mesures-d3d.md` |

« – » : pixels non comparés (piste écartée avant). Ce que disent les premières lignes : **eframe demande déjà le contexte le
plus léger** (ni profondeur, ni stencil, ni MSAA ; l'anticrénelage d'egui est
fait par dégradé des bords, pas par le GPU). Aucun réglage du contexte ne
gagne quoi que ce soit ; la transparence est gratuite ; la synchronisation
verticale ne coûte pas de mémoire et sa suppression ferait dessiner pour
rien. Le poids est dans le pilote lui-même.

Bibliothèques chargées par la fenêtre vide (58 modules) : `atio6axx.dll`
(62 Mo d'image, le pilote OpenGL) et rien d'autre de lourd ; ni Media
Foundation, ni audio, ni capture, ni codec avant leur premier usage. Sous
ANGLE : `atidxx64.dll` (31 Mo, le pilote Direct3D 11), `libGLESv2.dll`
(7,8 Mo), `d3dcompiler_47.dll` (4,5 Mo, celui de Windows).

#### Ce qui est fait

- **ANGLE, par défaut sous Windows x64**, quand ses deux bibliothèques sont à
  côté de l'exécutable (`src/rendu/angle.rs`). Elles sont embarquées
  compressées dans l'exécutable (`assets/angle`, + 3,45 Mo) et posées dans le
  dossier d'installation au lancement de l'exécutable **installé** (8,5 Mo
  sur disque) ; jamais à côté d'un exécutable téléchargé ou de compilation.
  Reconnues par leur taille et leur en-tête : une mise à jour qui en change
  la version les repose.
- **Repli sur le pilote OpenGL, sans rien d'autre de changé**, si les
  bibliothèques manquent, si EGL refuse de s'initialiser (c'est `glutin` qui
  retombe sur WGL), si `FERROCORD_RENDU=opengl`, ou si le démarrage précédent
  sous ANGLE n'a pas atteint l'affichage : un témoin `angle.essai` est posé
  avant la fenêtre et retiré à la troisième image ; s'il est encore là au
  lancement suivant, c'est OpenGL (jusqu'à ce qu'on efface ce fichier ou
  qu'une mise à jour repose les bibliothèques).
- eframe 0.37 impose WGL sous Windows : `vendor/glutin-winit` est une copie
  de `glutin-winit` 0.5.0 avec une seule ligne de plus (« EGL d'abord » quand
  l'application le demande), branchée par `[patch.crates-io]`.
- **Carte économe** : l'exécutable installé écrit pour lui-même
  `GpuPreference=1;` dans les préférences graphiques de Windows (celles de
  Paramètres › Système › Écran › Graphiques), seulement s'il sert ANGLE et
  seulement si rien n'y est déjà : un choix fait par l'utilisateur n'est
  jamais remplacé, et la désinstallation ne retire que notre valeur. Pas de
  réglage dans les Paramètres de Ferrocord : celui de Windows existe déjà.
  Sur une machine à une seule carte, cela ne change rien.
- L'aperçu : `--angle` pose les bibliothèques à côté de lui ; les rapports
  des bancs nomment le rendu (`rendu`) et les bibliothèques graphiques
  réellement chargées (`pilotes`).

#### Pixels

Captures de l'aperçu (image dessinée, `--screenshot`), huit scènes (`guild`,
`amis`, Paramètres › Apparence, `mp-chat`, `emojis`, `galerie`,
`theme-degrade`, `parures`), 1375 × 900 :

- nouvel exécutable **sans** ANGLE contre l'ancien : identiques pixel pour
  pixel, les huit ;
- ANGLE contre pilote OpenGL : 23 à 88 pixels sur 1 237 500 (moins de
  0,01 %) diffèrent, **de 1/255 sur un canal**, jamais plus. C'est l'arrondi
  du mélange des couleurs par le GPU, qui n'est pas le même en Direct3D 11
  et en OpenGL (entre les deux cartes, sous ANGLE : 2 à 14 pixels, de 1/255
  aussi). Texte, coins, ombres, dégradés, emojis : mêmes pixels.

Ces comparaisons ont été faites sur un exécutable compilé sans optimisation
à l'édition de liens, les trois variantes capturées dans le même ordre.
Refaites sur l'exécutable final : `guild`, `mp-chat`, `galerie` et `parures`
donnent les mêmes chiffres (0 pixel sans ANGLE ; 23 à 95 pixels à 1/255 avec) ;
les quatre autres scènes diffèrent d'un lancement à l'autre **du même rendu**
(heure affichée, état de l'aperçu), autant sans ANGLE qu'avec : elles ne
disent rien du rendu, et la série précédente vaut pour elles.

À l'écran (capture de l'écran lui-même, fenêtre à barre de titre intégrée,
affichage à 125 %), fond Mica et acrylique compris : la moitié haute est
identique, et la moitié basse l'est aussi **décalée d'une ligne de pixels**.
C'est le pilote OpenGL qui **supprime des lignes** de l'image (un trait d'un
pixel peut disparaître : le « barré » d'un message n'était pas visible à cet
endroit), alors que la chaîne d'échange d'ANGLE montre l'image pixel pour
pixel. C'est la seule différence visible, et c'est un défaut d'avant qui
disparaît. La transparence (fenêtre vue à travers, incrustation vocale,
matière Mica) est la même. L'explication donnée d'abord ici (« winit retire
une ligne à la zone cliente ») était fausse : la zone cliente et la surface de
rendu ont la même taille ; la cause est mesurée au lot « gl2 », plus bas.

#### Vitesse (banc complet, 3 passes alternées, médianes)

`preview --bench` par défaut, exécutables optimisés, les trois configurations
à tour de rôle. **La machine n'était pas calme** (un autre lot compilait par
moments : charge relevée avant chaque passe de 2 à 15 %, un `rustc` actif
avant la deuxième passe « avant ») : les passes sont données une par une.

| Mesure (3 passes) | Avant (pilote OpenGL, dédiée) | ANGLE, dédiée | ANGLE, intégrée |
|---|---:|---:|---:|
| Fenêtre créée (ms) | 198 / 279 / 223 | 296 / 154 / 141 | 203 / 210 / 120 |
| Première image construite (ms) | 203 / 286 / 271 | 344 / 160 / 147 | 251 / 261 / 126 |
| Début de l'image suivante, première présentée (ms) | 231 / 324 / 299 | 441 / 242 / 233 | 300 / 307 / 173 |
| Image au repos, redessin forcé : médiane (ms) | 1,44 / 1,72 / 1,42 | 1,81 / 1,98 / 1,79 | 1,82 / 1,96 / 2,00 |
| Image au repos : p95 | 1,88 / 2,10 / 1,85 | 2,10 / 2,29 / 2,15 | 2,27 / 2,32 / 2,31 |
| Défilement : médiane | 1,59 / 1,99 / 1,63 | 2,02 / 2,15 / 1,89 | 2,06 / 2,19 / 2,14 |
| Défilement : p95 | 2,14 / 2,45 / 2,04 | 2,50 / 2,50 / 2,38 | 2,56 / 2,70 / 2,57 |
| Arrivée de 50 messages/s : médiane | 1,77 / 2,12 / 1,82 | 2,13 / 2,19 / 2,18 | 2,23 / 2,39 / 2,36 |
| Changement de salon : médiane | 1,69 / 2,23 / 1,96 | 2,34 / 2,22 / 2,25 | 2,55 / 2,50 / 2,35 |
| Changement de salon : p95 | 2,13 / 2,76 / 2,32 | 2,90 / 2,72 / 2,63 | 3,22 / 2,95 / 2,66 |
| Historique : médiane | 1,67 / 1,68 / 1,68 | 1,95 / 1,99 / 1,92 | 2,33 / 2,17 / 2,10 |
| Trafic : médiane | 1,62 / 1,59 / 1,79 | 1,95 / 2,18 / 1,88 | 2,26 / 2,19 / 2,10 |
| Processeur par image au repos : fil d'interface + autres fils (ms) | 1,77 + 1,72 / 2,03 + 1,77 / 1,88 + 1,41 | 2,60 + 0,57 / 2,45 + 0,00 / 2,45 + 0,21 | 2,34 + 0,16 / 2,50 + 0,21 / 2,92 + 0,21 |
| Processeur par image au défilement : fil d'interface + autres fils | 1,82 + 1,51 / 2,45 + 1,67 / 1,93 + 1,62 | 3,02 + 0,73 / 2,71 + 0,31 / 2,60 + 0,78 | 2,71 + 0,73 / 2,87 + 0,47 / 3,02 + 0,37 |
| Mémoire après chargement : privée (Mo) | 125,8 / 125,7 / 125,2 | 90,1 / 89,7 / 90,9 | 61,4 / 62,3 / 61,5 |
| Mémoire après chargement : de travail privée | 80,3 / 81,2 / 79,8 | 46,8 / 46,7 / 47,4 | 29,1 / 29,6 / 29,6 |
| Mémoire en fin de banc : privée | 176,9 / 176,5 / 177,2 | 140,2 / 140,3 / 138,9 | 130,1 / 130,0 / 129,8 |
| Redessins en 10 s de repos, images au repos sous trafic | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 |
| Verdicts du banc (11) | tous tenus | tous tenus | tous tenus |

Ce qu'il faut en retenir, sans l'enjoliver :

- **Une image dure 0,3 à 0,5 ms de plus sur le fil d'interface** (médiane
  1,8 à 2,0 ms au repos au lieu de 1,4 à 1,7 ; p95 de 2,1 à 3,2 ms selon la
  phase au lieu de 1,9 à 2,8). C'est au-delà du bruit et c'est le prix
  d'ANGLE : le pilote OpenGL d'AMD fait une partie de son travail sur un fil
  à lui, la traduction vers Direct3D 11 se fait sur le nôtre. Tous les
  budgets (4 ms) restent tenus, avec 1 ms de marge au pire.
- **Le processeur consommé par image baisse** : 3,3 à 4,1 ms (fil d'interface
  + fils du pilote) avant, 2,5 à 3,8 ms sous ANGLE, soit en moyenne 0,7 ms de moins au
  repos et 0,3 au défilement. L'image est un peu plus longue, mais coûte moins.
- **Démarrage** : la fenêtre est créée aussi vite ou plus vite (120 à 210 ms
  contre 198 à 279, hors première passe perturbée) et la première image est
  présentée dans les mêmes temps (173 à 307 ms contre 231 à 324). Aucun
  recul mesurable dans le bruit de cette série ; à refaire machine calme.
  Contre le pilote OpenGL sur la carte intégrée (611 ms dans la fenêtre
  d'essai), le démarrage est deux fois plus court.
- Au repos, rien ne change : 0 image dessinée.

Si ces 0,3 à 0,5 ms par image devaient compter davantage que 60 Mo, le défaut
se renverse en une ligne (`Rendu::default` dans `src/rendu.rs`) ; ANGLE reste
alors disponible par `FERROCORD_RENDU=angle`.

#### Écarté, et pourquoi

- **Réglages du contexte OpenGL** (profondeur, stencil, MSAA, profil,
  version, simple tampon, sRGB, transparence) : déjà au plus bas, voir le
  tableau.
- **wgpu** (Direct3D 12, Vulkan, OpenGL) : remesuré, toujours plus lourd et
  plus lent (`docs/mesures-d3d.md`).
- **Forcer la carte dédiée ou intégrée avec le pilote OpenGL** : l'intégrée
  coûte 68 Mo de plus et triple le démarrage ; la dédiée est déjà le choix
  du pilote AMD.
- **Libérer le contexte quand la fenêtre est réduite dans la zone de
  notification** : eframe ne sait pas détruire puis recréer le contexte sans
  recréer la fenêtre ; aucune image n'est déjà dessinée fenêtre masquée
  (0 redessin au repos). Avec ANGLE il reste une vingtaine de Mo à y gagner :
  non fait.
- **Profil de compilation** : il est déjà au plus serré (`opt-level = 3`,
  LTO « fat », une unité de génération, symboles retirés). `panic = "abort"`
  casserait le rapport de plantage et le rattrapage des décodeurs ;
  `opt-level = "s"` ne toucherait que des pages de code, partagées et non
  comptées dans la mémoire privée. PGO : l'outil (`llvm-profdata`) n'est pas
  installé ; à proposer plus tard, gain attendu sur le temps par image, pas
  sur la mémoire.
- **mimalloc** : non essayé. Le tas de Ferrocord ne pèse que 11 Mo vivants ;
  le gain estimé (0 à 4 Mo) est vingt fois plus petit que celui du pilote, et
  chaque essai coûte une compilation complète.

#### Ce qui dépend de la machine, et ce qui reste à confirmer

- Les chiffres sont ceux d'**un** ordinateur : pilotes AMD 31.0.21925 (carte
  intégrée) et 32.0.21045 (RX 5600M). Le sens du gain est général (le pilote
  Direct3D 11 est plus léger que le pilote OpenGL chez tous les fabricants,
  c'est pourquoi Chromium et Firefox font de même), pas son ampleur : avec un
  pilote Intel ou NVIDIA, le pilote OpenGL part de moins haut. Rien dans le
  code n'est propre à AMD.
- **Le vrai `ferrocord.exe` installé n'a pas été lancé** (les agents ne le
  lancent pas) : la pose des bibliothèques dans le dossier d'installation, la
  préférence de carte et le témoin de démarrage sont couverts par des tests
  (dossier temporaire, registre simulé), pas par un essai réel. À vérifier
  après installation : `libEGL.dll` et `libGLESv2.dll` à côté de
  `ferrocord.exe`, la colonne « Mémoire » du Gestionnaire des tâches, et
  « Moteur GPU » qui doit nommer la carte intégrée.
- À regarder une fois à l'œil : l'incrustation vocale, le fond Mica, la
  lecture d'une vidéo, un partage d'écran (la préférence de carte vaut aussi
  pour l'encodeur vidéo matériel de Windows), la sortie de veille et le
  bureau à distance (perte du périphérique Direct3D).
- Les mentions de licence des bibliothèques qu'ANGLE lie lui-même : recopiées
  dans Paramètres › À propos au lot « gl2 » (`assets/angle/tiers`).
- Retour en arrière sans recompiler : la variable d'environnement
  `FERROCORD_RENDU=opengl` (supprimer les deux bibliothèques ne suffit pas :
  l'exécutable installé les repose au lancement suivant).

Taille : `preview.exe` passe de 39 333 376 à 42 805 760 octets
(+ 3 472 384, soit + 8,8 % : les deux bibliothèques compressées et 2 Kio de
code). `ferrocord.exe` contient le même ajout et n'a pas été recompilé dans
ce lot : + 3,5 Mo attendus. Sur disque, l'installation gagne en plus les deux
bibliothèques posées (8,5 Mo). Compilation : inchangée (12 min 51 s pour
l'aperçu en release, LTO « fat », machine occupée).

Téléchargement fait pour ce lot : l'archive officielle
`electron-v43.8.10-win32-x64.zip` (GitHub d'Electron, sha256 vérifié contre
`SHASUMS256.txt`), dont seuls `libEGL.dll` et `libGLESv2.dll` ont été gardés
(`assets/angle/LISEZMOI.md` : source, version, sommes de contrôle, licence).
Aucun exécutable de l'archive n'a été lancé.

### Lot « long » : usage prolongé, coût par image, READY (16 novembre 2026)

Demande : réduire la mémoire et le processeur là où les lots « ram » et
« cpu » n'avaient pas regardé (après des heures d'usage, par image en
activité, décodage du READY), sans changer un pixel. Méthode inchangée :
mesurer d'abord, ne garder que ce qui gagne, passes alternées avant / après.

**Nouveaux outils**

- `preview --bench-long <visites>` (`examples/bench/long.rs`) : usage
  prolongé rejoué en accéléré sur le compte **simulé** (READY de 100 serveurs
  par le vrai décodage). Cinq tours identiques de : `visites` salons ouverts à
  la suite, tous différents (500 pour la référence ; pages servies à la
  demande par `Cmd::History`, une remontée dans l'historique par salon,
  messages avec images et embeds), deux heures d'événements de la passerelle
  au rythme d'un vrai compte (21 600 paquets : messages dans le salon affiché,
  dans des salons déjà visités et jamais ouverts, modifications, suppressions,
  réactions, « écrit… », présences de 20 000 personnes, salons renommés),
  25 reconnexions (RESUMED, et un READY complet une fois sur cinq). Après
  chaque tour : tas vivant (exact), mémoire privée, tas engagé, caches,
  handles, fils. Les trois premiers tours remplissent les caches ; verdicts
  entre le troisième et le dernier : tas vivant + 3 Mo au plus, mémoire privée
  + 16 Mo, handles + 24, fils + 2, tas vivant final sous 40 Mo (code de
  sortie 1 sinon). La synchronisation verticale est coupée : le banc dure
  moins d'une minute.
- `--bench` donne les **allocations par image** de chaque phase, et
  `FERROCORD_BANC_PROFIL=<phase>` les attribue à leur site d'appel
  (exécutable `dev`, pour les symboles) : c'est le profil du travail refait à
  chaque image. `FERROCORD_BANC_TRACE=<octets>` sait ne rendre que les blocs
  demandés après le premier tour du banc long : ce qu'un tour de plus laisse.
- `--bench-atlas-libre` : le banc sans la borne de l'atlas des polices, dans
  le même exécutable.

**Chantier 1 : usage prolongé.** Inventaire d'abord : tout ce qui grossit
avec l'usage est déjà plafonné (20 salons de 500 messages en LRU, 5 000
personnes, 16 Mo de textures, 12 Mo et 1 000 entrées de mises en page, etc.).
Restent sans plafond de petits états seulement : réponses en cours par salon
(`ui/drafts.rs`), spoilers dévoilés, vues de salon périmées, restes d'un
serveur quitté. Puis le banc (`--bench-long 500`, exécutable de ce lot,
relevés après chaque tour) :

| Relevé | Départ | Tour 1 | Tour 2 | Tour 3 | Tour 4 | Tour 5 |
|---|---:|---:|---:|---:|---:|---:|
| Tas vivant (Mo) | 13,0 | 24,1 | 25,5 | 27,4 | 27,9 | 28,1 |
| Mémoire privée (Mo) | 136,6 | 169,1 | 179,7 | 178,6 | 180,8 | 181,8 |
| Tas engagé (Mo) | 85,7 | 116,4 | 120,2 | 121,0 | 123,2 | 124,4 |
| Textures des images (Mo, plafond 16) | 0,9 | 14,8 | 15,3 | 15,2 | 15,8 | 15,1 |
| Mises en page (Mo) | 0,1 | 3,9 | 3,9 | 3,9 | 3,9 | 3,9 |
| Handles | 364 | 365 | 365 | 365 | 365 | 366 |
| Fils | 24 | 25 | 25 | 23 | 23 | 23 |

Cinq passes en tout (deux sur la base, trois sur ce lot, exécutables
d'itération compris) : tas vivant final de 27,3 à 28,4 Mo, mémoire privée
finale de 180,0 à 182,6 Mo ; entre le troisième et le dernier tour, tas
vivant de − 0,8 à + 1,3 Mo, mémoire privée de + 1 à + 12 Mo (les textures
n'atteignent leur plafond de 16 Mo qu'au deuxième, troisième ou quatrième tour
selon la passe). Tous les verdicts sont tenus à chaque passe, avant comme
après : ce lot ne change pas la mémoire d'usage prolongé, il la mesure.
Images pendant les visites de salons : médiane 2,3 ms, p95 4,2 ms ; maximum
de 8,4 à 9,2 ms sur quatre passes et 18,4 ms sur la cinquième (une image sur
10 005, à l'ouverture d'un salon).

Lecture : pas de fuite. Le tas vivant se remplit pendant les trois premiers
tours puis ne bouge plus (les tables atteignent leur plafond : la trace des
blocs demandés après le premier tour et encore vivants à la fin ne montre que
elles, la plus grosse étant la table des membres, 1,26 Mo, et les
remplacements dus aux READY complets). Aucun pixel n'est gardé côté Rust. La
mémoire privée dépasse le tas vivant de la copie des textures par le pilote
graphique et du tas engagé après les pointes (121 à 124 Mo engagés pour 28
vivants) : ce sont les postes déjà décrits par le lot « ram ». Handles et fils
stables sur 125 reconnexions (25 par tour). Aucune correction mémoire n'était
donc à faire ; le banc reste comme garde-fou (verdicts ci-dessus).

À noter : `App::logic` d'eframe 0.37 tourne même fenêtre masquée ou réduite ;
la file des événements de la passerelle est donc vidée, elle ne grossit pas
pendant des heures de fenêtre réduite (vérifié dans le code d'eframe, pas au
banc, qui ne réduit pas la fenêtre).

**Chantier 2 : coût par image.** Profil des allocations
(`FERROCORD_BANC_PROFIL`) : 1 320 allocations par image au repos, 2 455 au
défilement. Par site :

- `Popup::style` par `context_menu` : environ 254 par image, une par ligne du
  rail et de la liste des salons, visible ou non ;
- tessellation d'egui : les mailles de texte, environ 430 Ko copiés par image ;
- `emoji::truncated`, `job` et `split` pour les noms de salons et de membres,
  `guild_acronym` (59) : refaits à chaque image ;
- le rail construit ses 100 serveurs et la liste ses 50 salons à chaque image.

Les 17 225 allocations par image du changement de salon viennent du faux
serveur du banc (liste des membres fabriquée dans un fil), pas de
l'application.

**L'écart de 0,2 ms du lot « ram » : tranché, ce n'est pas le code.** Six
passes alternées de cinq séries (`--bench --bench-frames 400 --bench-deep 300
--bench-quiet 3`, charge de 1 à 9 % avant chaque mesure, aucun `rustc`) :
`avant` (main), `après` (ce lot), `libre` (l'exécutable d'après avec
`--bench-atlas-libre`), et les deux exécutables du lot « ram », `base` et
`fin`, où l'écart avait été vu. Passes 1 à 3 dans l'ordre avant, après,
libre, base, fin ; passes 4 à 6 dans l'ordre inverse. Médiane du temps
d'image au repos, par passe (ms) :

| Série | 1 | 2 | 3 | 4 | 5 | 6 | min – médiane – max |
|---|---:|---:|---:|---:|---:|---:|---:|
| avant | 1,69 | 1,48 | 1,79 | 1,79 | 1,68 | 1,81 | 1,48 – 1,74 – 1,81 |
| après | 1,77 | 1,73 | 1,68 | 1,61 | 1,71 | 1,57 | 1,57 – 1,69 – 1,77 |
| libre | 1,81 | 1,61 | 1,61 | 1,79 | 1,78 | 1,74 | 1,61 – 1,76 – 1,81 |
| base (ram) | 1,81 | 1,87 | 1,71 | 1,81 | 1,71 | 1,81 | 1,71 – 1,81 – 1,87 |
| fin (ram) | 1,75 | 1,89 | 1,82 | 1,58 | 1,46 | 1,80 | 1,46 – 1,78 – 1,89 |

- Un même exécutable varie de 0,3 à 0,4 ms d'une passe à l'autre (avant :
  1,48 à 1,81 ; fin : 1,46 à 1,89) : plus que l'écart à expliquer.
- `après` et `libre` sont le même exécutable, avec et sans la borne de
  l'atlas : 1,69 contre 1,76, dans le bruit. La borne de l'atlas ne coûte rien.
- `base` et `fin` du lot « ram » : 1,81 contre 1,78. L'écart ne se reproduit
  pas.
- La place dans la passe compte : la série mesurée en premier après une pause
  est la plus rapide (avant en tête : 1,69 1,48 1,79 ; en queue : 1,79 1,68
  1,81 ; fin en queue : 1,75 1,89 1,82 ; en tête : 1,58 1,46 1,80), sans doute
  la fréquence du processeur. C'est l'ordre fixe des passes du lot « ram »
  qui avait fabriqué l'écart.

Avant / après sur les autres phases (min – médiane – max des six passes, ms) :

| Phase | Médiane avant | Médiane après | Maximum avant | Maximum après |
|---|---:|---:|---:|---:|
| Repos | 1,48 – 1,74 – 1,81 | 1,57 – 1,69 – 1,77 | 2,25 – 2,49 – 2,73 | 2,34 – 2,44 – 2,76 |
| Défilement | 1,73 – 1,88 – 2,04 | 1,82 – 1,89 – 2,03 | 2,74 – 2,90 – 3,29 | 2,61 – 3,09 – 4,54 |
| Arrivée de 50 messages par seconde | 1,86 – 2,03 – 2,20 | 1,94 – 2,03 – 2,15 | 2,94 – 3,08 – 3,40 | 2,98 – 3,12 – 3,30 |
| Changement de salon | 1,86 – 2,15 – 2,26 | 1,89 – 2,13 – 2,24 | 2,72 – 2,90 – 3,16 | 2,75 – 2,97 – 3,46 |
| Historique | 1,87 – 1,90 – 1,95 | 1,67 – 1,80 – 1,92 | 4,43 – 4,66 – 5,13 | 4,74 – 5,03 – 5,48 |

Aucune différence au-delà du bruit, et c'est attendu : ce lot ne change rien
de ce qui tourne à chaque image. L'objectif « maximum sous 8 ms » est tenu au
banc principal avant comme après (5,5 ms au plus) ; il ne l'est pas au banc
long (voir plus haut : 8,4 à 18,4 ms à l'ouverture d'un salon). Mémoire
privée à la fin du banc : 160,6 – 161,3 – 161,7 Mo avant, 157,5 – 159,3 –
162,7 Mo après.

**Essayé pour écrêter les maximums, puis retiré** : une borne en octets sur
les textures envoyées au processeur graphique par image (2 Mo, en plus des
32 textures). Maximum d'image pendant les visites du banc long : 8,4 et
8,8 ms sans la borne, 9,2 et 8,6 ms avec ; phase « historique » du banc
principal : 4,7 ms sans, 5,0 ms avec. Aucun gain : retirée. Les exécutables
« après » mesurés ici la contenaient encore ; le code livré est le même sans
elle (les mesures du READY et des paquets n'en dépendent pas).

**Chantier 3 : READY en une passe.** Deux causes, deux corrections
(`src/net/raw.rs`) :

1. *Tolérance sans copie.* `Item<T>`, `de_lenient` (580 sites), les
   identifiants et permissions « chaîne ou nombre » recopiaient le texte de
   chaque champ dans un `Box<RawValue>` avant de le relire. Le texte brut est
   maintenant prêté par serde_json (`with_raw`) : même relecture, sur le
   texte d'origine. Les anciens `untagged` (qui passaient par un arbre
   `Content`) sont lus par un visiteur direct (`Loose`).
2. *Serveurs découpés une fois.* Le READY et chaque GUILD_CREATE étaient
   relus en entier par cinq décodeurs (principal, relations, communauté,
   vocal, appels). Le tableau des serveurs est découpé une seule fois en
   champs bruts (`Parts`, `Shared`), et chaque décodeur ne lit que les champs
   qu'il déclare. Le READY principal reste lu d'un bloc (son message d'erreur
   est inchangé).

La tolérance est la même, et c'est vérifié : chaque lecteur réécrit est
comparé à l'ancien (gardé dans les tests, module `old`) sur toutes les formes
(absent, `null`, mauvais type, nombre pour chaîne, objet à la place d'un
tableau, clés en double, échappements dans les clés) ; chaque décodeur
partagé est comparé à sa lecture du texte entier
(`shared_split_reads_like_the_whole_text` dans `community`, `voicegw`,
`calls` ; `parts_read_like_the_whole_text`,
`lenient_readers_match_the_copying_ones_on_every_shape`).

**Avant / après** (`--bench-cpu 3`, six passes alternées, charge de 5 à 9 %,
compte simulé : READY de 4,76 Mo, 100 serveurs) :

| Mesure | Avant (min – médiane – max) | Après (min – médiane – max) |
|---|---:|---:|
| Décodage du READY (ms) | 134,9 – 136,3 – 137,0 | 89,0 – 89,8 – 91,4 |
| Allocations du READY | 603 884 – 605 749 – 606 647 | 173 125 – 173 864 – 179 121 |
| Image qui applique le READY (ms) | 7,6 – 8,3 – 9,3 | 8,3 – 9,0 – 9,9 |
| Décodage des jeux détectables (ms) | 34,0 – 34,4 – 35,9 | 27,0 – 27,7 – 29,1 |
| Allocations des jeux détectables | 300 531 | 185 763 |
| Processeur du lancement au calme (ms) | 469 – 570 – 609 | 406 – 508 – 625 |
| Délai jusqu'au calme (ms) | 394 – 636 – 650 | 338 – 587 – 594 |
| MESSAGE_CREATE (µs ; allocations) | 2,75 ; 15 | 2,23 ; 6 |
| MESSAGE_UPDATE | 6,68 ; 32 | 5,47 ; 13 |
| MESSAGE_DELETE | 0,55 ; 3 | 0,34 ; 0 |
| TYPING_START | 1,77 ; 13 | 1,43 ; 7 |
| PRESENCE_UPDATE | 1,12 ; 8 | 0,83 ; 2 |
| MESSAGE_REACTION_ADD | 3,09 ; 28 | 2,30 ; 14 |

Lecture honnête : le READY gagne 46 ms (34 %) et 432 000 allocations, moins
en temps que les 60 à 80 ms estimés (le READY principal est encore relu d'un
bloc), plus en allocations. Les paquets courants gagnent 19 à 38 % et la
moitié de leurs allocations ou plus. Le processeur jusqu'au calme baisse de
60 ms en médiane, dans un bruit de 150 ms (pas d'horloge de 15,6 ms).
L'image qui applique le READY ne change pas (0,7 ms d'écart, dans le bruit).
Mémoire du démarrage (`--bench-mem demarrage`, trois passes alternées) :
pointe du tas pendant le READY 16,7 → 14,3 Mo, mémoire privée après 3 s
122,1 à 122,7 → 119,0 à 119,5 Mo, tas vivant 11,1 Mo avant comme après.

**Essayé puis retiré ou laissé**

- Virtualiser le rail et la liste des salons (ne construire que les lignes
  visibles) : c'est le premier poste par image, mais le glisser-déposer, les
  menus contextuels, le focus clavier et les identifiants d'egui en dépendent ;
  risque de changement visible, non fait dans ce lot.
- 16,5 Mo de `egui::id::id_source` vus dans le profil : table de débogage
  d'egui, absente de l'exécutable optimisé. Rien à corriger.
- Plafonds de caches plus bas : aucun gain au banc long (le tas vivant tient
  sous 29 Mo), et un plafond plus bas referait des requêtes.

**Garde-fous** : captures de l'aperçu identiques à l'octet près avant / après
(`salon`, `amis`, `pu-compte`, `pu-apparence`, `pu-notifications`) ;
`cargo clippy --all-targets` sans avertissement ; `cargo test -- --skip
state_table` : aucun échec ; `--bench-long 500` :
tous les verdicts tenus ; aucun changement de ce que Discord voit (mêmes
paquets, mêmes requêtes : seul le décodage change).

**À confirmer sur le vrai compte** (le banc joue un compte simulé, READY de
4,76 Mo et 100 serveurs) : la durée réelle du décodage du READY
(`FERROCORD_PERF`) ; qu'aucun serveur, salon, rôle, état vocal ou appel ne
manque après connexion (la tolérance est testée sur des formes fabriquées, pas
sur celles du vrai compte) ; la mémoire privée après plusieurs heures
(`tools\mesures.ps1`), que le banc long ne fait que rejouer en accéléré.

**Non fait, par gain estimé**

- Rail et liste des salons ne construisant que les lignes visibles : 300 à
  500 allocations et 0,2 à 0,4 ms de moins par image, estimés.
- Jeux détectables sans `String` par champ : environ 100 000 allocations de
  moins, une fois par session, hors du fil d'interface.
- READY principal lu par `Parts` lui aussi : une relecture du texte de moins
  (10 à 15 ms estimés), au prix d'un message d'erreur différent.
- Tables de personnes : environ 5 Mo quand les plafonds sont atteints ; des
  chaînes partagées en gagneraient 1 à 2.
- Plafonner les petits états sans borne (réponses en cours, spoilers
  dévoilés, vues périmées) : quelques Ko par heure au pire.
- Copie des textures gardée par le pilote graphique : hors de ce lot (rendu).

### Lot « gl2 » : suites du rendu ANGLE (16 novembre 2026)

Le lot « gl » avait laissé cinq écarts. Ce qui suit dit, pour chacun, ce qui a
été mesuré, ce qui est corrigé et ce qui ne l'est pas. Mesures brutes hors
dépôt ; la machine n'était pas seule (un autre lot compilait par moments) :
la charge relevée avant chaque passe est donnée.

#### 1. La « ligne de pixels » : ce n'est pas la fenêtre, c'est le pilote OpenGL

Méthode : la fenêtre de l'aperçu est capturée par Windows (`PrintWindow`,
contenu composé par le bureau, zone cliente seule) et comparée ligne à ligne à
l'image dessinée (`--screenshot`, lue dans le tampon de rendu). Affichage à
125 %, barre de titre intégrée sauf mention.

| Fenêtre (zone cliente) | ANGLE | Pilote OpenGL, carte dédiée (choix du pilote AMD) | Pilote OpenGL, carte intégrée |
|---|---|---|---|
| 1375 × 900 (taille par défaut) | identique : 900 lignes sur 900 | **3 lignes supprimées** (vers 150, 453 et 756), image tassée sur 897 lignes | identique |
| 1375 × 900, barre de titre de Windows | – | **1 ligne supprimée** (au milieu) | – |
| 1375 × 901, 1375 × 902, 1282 × 841 | identique (841 sur 841 mesuré) | lignes supprimées | identique (1282 × 841) |
| 1182 × 791 | – | identique | – |
| Agrandie (1920 × 1080) | identique | identique | – |
| Réduite puis restaurée ; 60 redimensionnements en rafale puis retour à 1375 × 900 | identique (0 pixel d'écart avec la fenêtre d'avant) | – | – |

« Identique » : même image ligne pour ligne, aux quelques dizaines de pixels
à 1/255 près qui séparent déjà les deux rendus (7 à 47 pixels).

Ce que cela établit :

- **La surface de rendu et la zone cliente ont la même taille** (1375 × 900
  dans les deux cas, `GetClientRect` et tampon lu), avec la barre intégrée
  comme avec celle de Windows. Le traitement de `WM_NCCALCSIZE`
  (`src/platform/caption.rs`) n'y est pour rien : il ne retire rien à la
  hauteur, et le défaut existe aussi avec la barre de titre de Windows.
  L'explication du lot « gl » était fausse.
- **Le défaut est dans la présentation du pilote OpenGL d'AMD quand il
  dessine sur la carte dédiée pour un écran branché sur l'intégrée** : selon
  la taille de la fenêtre, l'image est tassée d'une à trois lignes. Le même
  pilote sur la carte intégrée (préférence posée pour le seul exécutable
  d'essai, puis retirée) montre l'image exacte, et la fenêtre agrandie aussi.
- **Sous ANGLE, le rendu par défaut, l'image est exacte dans tous les états
  essayés** : il n'y avait rien à corriger dans Ferrocord, et rien n'a été
  changé dans la fenêtre. Ce que l'on voyait « décalé » après le lot « gl »,
  c'est l'image enfin entière.
- **Le repli OpenGL garde donc ce défaut sur cette machine** (c'est l'affichage
  d'avant le lot « gl », ni pire ni meilleur). Le seul moyen de l'ôter serait
  de forcer la carte intégrée pour le repli, ce qui coûte 68 Mo et triple le
  démarrage (tableau du lot « gl ») : non fait, le repli est un cas rare.

Non vérifié : les échelles 100 % et 150 % et le passage d'un écran à l'autre
(il faudrait changer un réglage du système), l'ancrage à gauche et à droite
(non essayé à part : c'est un simple changement de taille de la fenêtre, et
les changements de taille sont exacts), et le plein écran. `PrintWindow`
donne trois lignes là où la capture d'écran du lot « gl » en voyait une : le
nombre dépend de la taille de la fenêtre, le sens est le même.

#### 2. Le coût par image sous ANGLE : mesuré, situé, pas réduit

Cinq passes alternées du banc (tableau de la mesure finale, plus bas) :

- **l'image reste plus longue de 0,1 à 0,3 ms sur le fil d'interface**
  (médiane au repos 2,04 ms contre 1,75 ; défilement 2,15 contre 1,90 ;
  changement de salon 2,28 contre 2,18), les plages des deux rendus se
  recouvrant en partie ;
- **l'écart est tout entier dans l'envoi à la carte** : la mise en page dure
  autant (1,02 à 1,25 ms dans les deux cas), le reste de l'image (découpage en
  triangles et appels OpenGL) dure 0,61 à 0,64 ms avec le pilote OpenGL et
  0,77 à 0,92 ms avec ANGLE, soit + 0,25 ms : ANGLE traduit chaque appel sur
  notre fil, le pilote OpenGL les met en file pour un fil à lui ;
- **le processeur total par image reste plus bas sous ANGLE** : 2,50 + 0,26 ms
  (fil d'interface + autres fils) contre 2,24 + 1,30 au repos ; 2,81 + 0,52
  contre 2,34 + 1,25 au défilement ; 2,81 + 0,89 contre 2,50 + 2,03 à
  l'arrivée de messages.

Essai fait et retiré : **un seul envoi de sommets et d'indices par image**
(copie modifiée d'`egui_glow`, tous les maillages mis bout à bout, découpe et
texture redonnées seulement quand elles changent), au lieu de deux envois par
maillage. Image identique au pixel près (13 scènes × 2 rendus : 0 pixel
d'écart), mais **aucun gain sous ANGLE** (médiane au repos 1,82 contre 1,80
dans la passe la plus calme) et **une perte avec le pilote OpenGL**
(1,77 à 1,79 ms contre 1,53 à 1,70 sans). Le code n'a pas été gardé.

Ce qui reste à essayer, et pourquoi ce n'est pas fait ici : le chemin de
présentation direct d'ANGLE (`EGL_EXPERIMENTAL_PRESENT_PATH_FAST_ANGLE`, qui
évite une copie de l'image entière à chaque échange) et les autres attributs
EGL se règlent à la création de l'affichage, dans `glutin`, qu'il faudrait
copier dans `vendor/` comme `glutin-winit`. Ils jouent sur l'échange de
tampons, que la mesure « image » ne compte pas : le gain attendu est sur le
processeur du fil d'interface (les 0,3 à 0,5 ms qu'il consomme de plus), pas
sur la durée d'une image. Pour celle-ci, il faudrait un profil appel par
appel de la traduction d'ANGLE, non fait. Tous les budgets (4 ms) restent
tenus, 11 verdicts sur 11 à chaque passe.

#### 3. Vérifications visuelles (aperçu, image dessinée, 1375 × 900)

Chaque scène capturée sous le pilote OpenGL puis sous ANGLE :

| Scènes | Pixels différents |
|---|---|
| `guild`, `incrustation` (incrustation vocale), `mica`, `mica-acrylique`, `lecteur`, `lecteur-plein`, `videos`, `parures`, `parures-animees`, `polices-nom`, `theme-degrade`, `gifs`, `galerie`, `zoom`, `emojis`, `appel-partage`, `glyphes`, `fond-image`, `partage`, `partage-ecrans` | 3 à 102 sur 1 237 500, **tous de 1/255** |
| `effet-fixe` (effet de profil, image fixe) | 65 à 123 de plus de 2/255, tous dans l'heure affichée (lignes 729 à 738) : autant entre deux lancements du même rendu |
| `effet`, `profil-carte` (effet de profil animé) | 5 500 à 10 200 à l'instant « image stable », qui n'est pas le même d'un rendu à l'autre ; **à instant fixe (`--at 3000`) : 165, dont 71 dans l'heure affichée**, comme entre deux lancements du même rendu |

Aucune différence de rendu : textures vidéo, plaques animées avec alpha,
effets de profil, dégradés et halos des noms, vignettes du sélecteur de
partage (sources factices de l'aperçu), GIF, grandes images au zoom, atlas des
polices. La taille maximale de texture de Direct3D 11 (16 384) dépasse la
borne de l'atlas (8 192) ; les scènes `glyphes`, `zoom` et `galerie` le
confirment.

**Perte du rendu** (pilote réinitialisé ou mis à jour, carte retirée) : un
contexte perdu ne se répare pas dans le processus (toutes les textures
seraient à refaire, et eframe ferme la fenêtre sur la première erreur
d'échange, sans rendre l'erreur). Ferrocord reconnaît maintenant cette sortie
(`src/rendu.rs`, `finish`) et **se relance**, comme après une mise à jour ;
moins de vingt secondes après le lancement, la relance se fait sur le pilote
OpenGL et ANGLE attend la version suivante. Direct3D n'offre aucun moyen de
retirer un périphérique sans toucher au pilote : le chemin est essayé en
détruisant la surface EGL dans le dos d'eframe (`FERROCORD_RENDU_PERTE=40`
dans l'aperçu), ce qui fait échouer l'échange suivant au même endroit. Vu :
la fenêtre se ferme, le témoin reste avec la version, les deux lancements
suivants passent par le pilote OpenGL. Non essayé : une vraie perte (sortie de
veille, bureau à distance, changement de carte) et la relance de
l'application elle-même (l'aperçu ne se relance pas).

À regarder une fois à l'œil, parce qu'une capture ne le montre pas : le
redimensionnement continu à la souris (scintillement, bande noire), la
matière Mica réellement vue à travers la fenêtre, une vraie vidéo, un vrai
partage d'écran.

#### 4. Robustesse de la pose

Tout est décrit dans `docs/distribution.md` (début) et couvert par les tests
de `src/rendu/angle.rs`. Essayé aussi sur l'aperçu : une bibliothèque altérée
d'un octet au-delà de son en-tête n'est pas chargée (rendu OpenGL, fichier
effacé) ; deux exemplaires lancés ensemble démarrent tous deux sous ANGLE et
ne laissent pas de témoin. Coût de la vérification : les deux fichiers
(8,5 Mo) sont lus et hachés à chaque lancement ; la fenêtre est pourtant créée
aussi vite qu'avant (145 à 169 ms ici, 120 à 210 au lot « gl »).

Mentions légales : `assets/angle/tiers` (20 textes), montrés dans Paramètres ›
À propos. Android : `tools\android.ps1 -Check` (`cargo ndk check`, arm64) passe ; la copie
de `glutin-winit` et le nouveau code ne concernent que Windows.

#### 5. Mesure finale (5 passes alternées, 16 novembre 2026)

`preview --bench` et `preview --bench-mem`, exécutable optimisé (LTO), les
deux rendus à tour de rôle ; charge de la machine avant chaque passe : 2 à
15 % (une passe mémoire à 26 %), aucun `rustc` actif. L'exécutable mesuré est
celui de l'essai d'envoi groupé, celui-ci coupé par sa variable
d'environnement : le chemin exécuté est le code d'`egui_glow` d'origine,
c'est-à-dire le code livré. « ANGLE » est mesuré comme le fera l'application
installée, sur la carte économe (préférence posée pour le seul exécutable
d'essai, puis retirée). Médiane des cinq passes (minimum à maximum).

| Mesure | Pilote OpenGL (carte dédiée) | ANGLE (carte économe) |
|---|---:|---:|
| Fenêtre vide : mémoire privée (Mo) | 98,2 (98,1 à 99,4) | **27,2 (27,0 à 27,8)** |
| Fenêtre vide : de travail privée | 46,3 (46,1 à 47,0) | **11,0 (10,9 à 11,3)** |
| Démarrage simulé : privée | 120,9 (114,2 à 121,4) | **61,4 (60,9 à 62,1)** |
| Démarrage simulé : de travail privée | 74,6 (73,4 à 75,3) | **30,8 (30,6 à 31,0)** |
| Démarrage simulé : tas vivant | 11,1 | 11,1 |
| Banc, après chargement : privée | 125,9 (124,0 à 126,2) | **61,1 (60,6 à 62,0)** |
| Banc, après chargement : de travail privée | 80,7 (79,8 à 81,0) | **29,3 (28,6 à 29,4)** |
| Fin de banc : privée | 175,9 (175,6 à 181,6) | **127,7 (127,3 à 129,1)** |
| Fenêtre créée (ms) | 211 (204 à 226) | **156 (145 à 169)** |
| Première image construite (ms) | 217 (210 à 232) | **162 (151 à 175)** |
| Début de l'image suivante, première présentée (ms) | 255 (241 à 265) | **214 (201 à 227)** |
| Image au repos, redessin forcé : médiane (ms) | **1,75 (1,70 à 1,89)** | 2,04 (1,80 à 2,11) |
| Image au repos : p95 | **2,17 (2,13 à 2,29)** | 2,36 (2,15 à 2,58) |
| Image au repos : maximum | 2,58 (2,37 à 2,62) | 2,73 (2,36 à 2,97) |
| Défilement : médiane / p95 / maximum | **1,90 / 2,38 / 2,87** | 2,15 / 2,56 / 3,05 |
| Changement de salon : médiane / p95 / maximum | **2,18 / 2,56 / 2,89** | 2,28 / 2,75 / 3,27 |
| Arrivée de 50 messages/s : médiane / p95 | **1,98 / 2,58** | 2,19 / 2,79 |
| Historique : médiane / maximum | **1,86 / 5,41** | 1,99 / 6,11 |
| Trafic : médiane / p95 | **1,86 / 2,40** | 2,16 / 2,67 |
| Processeur par image au repos : fil d'interface + autres fils (ms) | 2,24 + 1,30 | 2,50 + **0,26** |
| Processeur par image au défilement | 2,34 + 1,25 | 2,81 + **0,52** |
| Processeur par image au changement de salon | 2,66 + 1,04 | 2,97 + **0,36** |
| Images dessinées en 10 s de repos | 0 | 0 |
| Verdicts du banc (11) | tous tenus, 5 passes | tous tenus, 5 passes |

En clair : **60 Mo privés de moins au démarrage** (120,9 → 61,4) et 44 Mo de
moins dans le Gestionnaire des tâches (74,6 → 30,8), **une fenêtre ouverte
50 ms plus tôt**, un processeur total par image plus bas, et une image
**0,1 à 0,3 ms plus longue** sur le fil d'interface, très loin des 16 ms d'un
écran à 60 Hz. Le compromis du lot « gl » tient ; la mémoire, priorité de la
demande, n'a pas bougé.

### Taille sur disque

| | Ferrocord | Discord | Discord / Ferrocord |
|---|---:|---:|---:|
| Programme | 25,8 Mo¹ (un seul fichier) | 661,7 Mo (`%LOCALAPPDATA%\Discord`, 1 673 fichiers) | ≈ 26 × |
| Données et cache | 6,1 Mo (`%LOCALAPPDATA%\ferrocord`, cache d'images) + moins de 0,1 Mo (`%APPDATA%\ferrocord`, réglages) | 172,2 Mo (`%APPDATA%\discord`, dont 137,8 Mo de cache web) | ≈ 28 × |
| Total | ≈ 32 Mo | ≈ 834 Mo | ≈ 26 × |

¹ Exécutable de release compilé sur cette branche (27 076 608 octets,
soit 25,8 Mio ; « Mo » vaut ici 2²⁰ octets partout). `target\release\ferrocord.exe`
n'existait pas dans le dépôt principal au moment de la mesure ; sa dernière
version de release y est `ferrocord.old.exe`, 24,75 Mo.

Détail de Discord 1.0.9268 : `app-1.0.9268` 506,9 Mo (dont `Discord.exe`
224,7 Mo et `modules` 153,3 Mo), `packages` 152,2 Mo (paquet de mise à
jour). Les dossiers ont été seulement parcourus, rien n'a été ouvert ni modifié.

### Processus déjà ouverts

Au moment de la mesure, ni Discord ni Ferrocord ne tournaient : aucune
mesure de processus en cours d'usage n'a pu être faite. C'est le rôle du
script de la partie 2.

## 2. Le script `tools/mesures.ps1`

### Commande

Fermez Discord et Ferrocord complètement (zone de notification comprise),
puis, à la racine du dépôt :

```
cargo build --release
powershell -ExecutionPolicy Bypass -File tools\mesures.ps1
```

Avec un appel vocal : ajoutez `-Vocal`. Autres options :
`-Ferrocord <chemin>`, `-Discord <chemin de Discord.exe>`,
`-Applications Ferrocord` (une seule), `-Repos 60`, `-Activite 30`,
`-Stabilisation 30`, `-SeuilCalme 5`, `-Sortie <dossier>`.

### Durée et ce qu'il faut faire

Environ 3 minutes par application (4 avec `-Vocal`), Ferrocord puis Discord :

1. **Démarrage** : le script lance l'application et mesure le temps jusqu'à
   la première fenêtre visible, puis jusqu'à ce que le processeur retombe
   sous 5 % d'un cœur pendant 2 s (« prêt »). Ne touchez à rien. Pour
   Discord, attendez la fin d'une éventuelle mise à jour, puis Entrée.
2. **Stabilisation** (30 s) puis **repos** (60 s) : ne touchez à rien,
   souris hors de la fenêtre. Mesures : mémoire privée et de travail
   (somme de tous les processus : Discord en a plusieurs), processeur moyen
   et maximal, processus, fils, handles, GPU.
3. **Activité** (30 s) : après Entrée, changez de salon et faites défiler
   sans arrêt ; un bip annonce la fin.
4. **Appel vocal** (`-Vocal`, 60 s) : rejoignez un salon vocal, Entrée,
   restez jusqu'au message de fin.
5. **Fermeture** : le script demande avant de fermer, et ne ferme que le
   processus qu'il a lancé et ses descendants. Si Discord s'est relancé
   lui-même (mise à jour), il vous le dit et vous le fermez.

Si une application est déjà ouverte, le script ne la mesure pas (il ne
pourrait ni mesurer son démarrage ni la fermer) ; il relève seulement sa
taille sur disque.

### Résultat

`mesures\resultats-<date>.md` : un tableau Markdown avec une ligne par
mesure, la valeur pour Ferrocord, pour Discord, et le rapport
Discord / Ferrocord (« 5,0 × » : Discord en consomme cinq fois plus). Le
dossier `mesures\` est ignoré par git. Le script n'envoie rien sur le
réseau, ne lit aucun jeton et ne modifie aucune donnée des applications.

Unités : processeur en % **d'un cœur** (100 % = un cœur entier ; le
Gestionnaire des tâches divise par le nombre de cœurs logiques). GPU : le
moteur le plus occupé, comme le Gestionnaire des tâches, d'après la classe
`Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine` (« n/d » si elle
manque).

### Journal interne de Ferrocord : `FERROCORD_PERF`

La latence de l'interface ne se mesure pas de l'extérieur. Ferrocord peut
donc l'écrire lui-même :

- `FERROCORD_PERF=1` dans l'environnement au lancement active le journal ;
  toute autre valeur, ou son absence, le laisse désactivé (par défaut) ;
- fichier : `%LOCALAPPDATA%\ferrocord\perf.csv`, ou le chemin donné par
  `FERROCORD_PERF_FICHIER` (le script en donne un dans `mesures\`) ;
- contenu : `t_ms,type,ms`, une ligne par image (`image` : construction par
  l'application ; `image_eframe` : temps mesuré par eframe), par changement
  de salon (`changement_de_salon` : l'image où le salon a changé) et une
  pour la première image (`premiere_image` : depuis le lancement). Depuis
  le lot C8, aussi : `etape.<nom>` (étapes du démarrage, en ms depuis le
  début de `run` ; `etape.processus_avant_run` : âge du processus à ce
  moment), `cause.<fichier>_<ligne>` (code qui a demandé une image, d'après
  egui), `cause.entree` (image due à la souris, au clavier ou à la fenêtre)
  et `passerelle.<reveil|differe|ignore>.<paquet>` (sort de chaque
  événement de la passerelle, par type de paquet : `message_create`…) ;
  ces lignes valent 1 dans la colonne `ms` (un compte). Depuis le lot C9 :
  `entree.<souris|clic|molette|clavier|fenetre|autre>` (sorte d'entrée de
  l'image, un compte), `fil.<nom>` (temps processeur de chaque fil depuis le
  relevé précédent, en ms ; toutes les 250 ms pendant 20 s, puis toutes les
  5 s ; `principal` = fil d'interface) et `premier_salon` (image où le
  premier salon s'affiche au démarrage, à part des changements de salon).
  Depuis le lot « perf » : un fil sans nom s'appelle `fil.sans_nom.<module>`,
  d'après le module où il démarre (`ntdll` : réserve de fils de Windows ;
  `atio6axx`, `nvoglv64`… : pilote OpenGL), et le relevé des fils ne refait
  plus la liste des fils du système à chaque fois (voir ce lot : il
  faussait « prêt ») ; le temps d'un fil né entre deux listes est compté à
  la liste suivante, au plus 2 s plus tard.
  Depuis le lot « ram » (Windows) : `mem.etape.<nom>` (mémoire privée du
  processus à chaque étape du démarrage, en Mo dans la colonne `ms`),
  `mem.privee`, `mem.travail` et `mem.travail_privee` (relevés quand ils
  ont bougé d'au moins 1 Mo) et `mem.carte.<poste>` (répartition de la
  mémoire privée à 10, 30 et 60 s puis toutes les 5 minutes : `tas`,
  `tas_alloue`, `autre`, `piles`, `module.<dll>`).
  Rien d'autre : ni nom de personne, ni identifiant, ni texte, ni jeton ;
- désactivé, l'application tourne sans l'enveloppe de mesure : la seule
  dépense est la lecture de la variable au démarrage. Activé, l'écriture se
  fait dans un fil à part, jamais sur le fil d'interface.

Le script en tire une section à part : première image, images dessinées au
repos (attendu : 0), temps d'image pendant l'activité (médiane, 95e
centile, maximum) et temps des changements de salon. Il ajoute deux
petits tableaux : les étapes du démarrage, et les causes d'images pendant la
fenêtre de repos (les 15 plus fréquentes) : ce qui réveille encore
l'interface sur votre compte. Depuis le lot C9, deux de plus pour le
démarrage (du lancement à la fin des 2 s de calme) : temps processeur par
fil, et images et entrées reçues (`entree.souris` dit si la souris a bougé
sur la fenêtre). Depuis le lot « ram », deux encore : la mémoire
privée à chaque étape du démarrage, et les derniers compteurs et la dernière
répartition relevés. Discord n'a pas
d'équivalent : ces lignes ne se comparent pas.

Code : `src/perf.rs`, branché dans `src/lib.rs` (`run`). Tests :
`perf::tests` (désactivé : aucun fichier ; activé : seulement des durées).

## 3. Limites

- **Pas le même travail.** Discord embarque un navigateur complet
  (Electron, Chromium) et des modules que Ferrocord n'a pas (superposition
  de jeux, par exemple) ; Ferrocord n'a pas encore toutes les fonctions. Les chiffres comparent deux clients pour le
  même usage, pas deux implémentations de la même chose.
- **Premier lancement et cache.** Le premier démarrage après un
  redémarrage de Windows lit le disque ; les suivants viennent du cache du
  système (ici 0,76 s puis 0,40 s pour l'aperçu). Pour comparer, lancez le
  script deux fois et gardez la seconde série, ou notez lequel est un
  démarrage à froid. Discord vérifie aussi ses mises à jour au démarrage
  (écran de chargement) : sa « première fenêtre visible » est cet écran,
  pas la fenêtre principale ; « prêt » est l'indicateur plus juste.
- **« Prêt » est approché.** Le calme du processeur ne garantit pas que les
  messages sont affichés ; c'est un indicateur, pas une mesure de
  l'interface.
- **Compte et serveurs.** Les deux clients doivent être sur le même compte,
  avec les mêmes serveurs, et le même salon ouvert ; la mémoire dépend de
  ce qui a été chargé.
- **Activité à la main.** Votre geste n'est pas le même d'un client à
  l'autre ; les 30 s d'activité donnent un ordre de grandeur.
- **Mesure extérieure.** Mémoire privée = `PrivateMemorySize64` (mémoire
  réservée, différente de la colonne « Mémoire » du Gestionnaire des
  tâches, qui montre l'ensemble de travail privé). Le processeur est relevé
  toutes les secondes ; un pic plus court est lissé.
- **Mesures hors ligne.** La partie 1 utilise l'aperçu, sans réseau ni
  passerelle : ni décompression du flux, ni TLS, ni vrais avatars. Les
  chiffres du banc ne remplacent pas ceux du script sur un compte réel.
- **Vocal.** Mesuré seulement si vous lancez `-Vocal` ; le traitement du
  son (suppression de bruit, Opus) ne se compare qu'avec les mêmes réglages.
- **Charge de la machine.** Toute autre tâche lourde (compilation,
  navigateur) fausse les deux séries ; fermez-les.
