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
sur la fenêtre). Discord n'a pas
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
