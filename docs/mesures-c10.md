# Lot C10 — optimisation générale (15 novembre 2026)

Mesures sur `cargo run --release --example preview -- --bench` (hors ligne,
100 serveurs, fenêtre 1375 × 900). Rien n'est mesuré sur un compte réel.

**Machine pas au calme** : un navigateur de l'utilisateur tournait pendant
toutes les mesures (charge du processeur de 4 à 100 % selon les moments), et un
`ferrocord.exe` de l'utilisateur au début. Une passe isolée varie de ± 10 % ;
les chiffres « avant / après » ci-dessous sont donc des médianes de 8 passes
alternées (avant, après, après, avant…) des deux exécutables release, banc
court (`--bench-deep 300 --bench-quiet 2`).

## Ce qui a changé

1. **Rail des serveurs** (`src/ui/rail.rs`) : une pastille à plus de quatre
   hauteurs de la vue garde sa place, son identifiant et son nom pour les
   lecteurs d'écran, mais n'est plus préparée (non-lus, icône, infobulle, menu).
   Zone chronométrée : 0,29 → 0,18 ms par image avec 100 serveurs.
2. **Liste des salons** (`src/ui/channels.rs`) : même chose pour une ligne à
   plus de six hauteurs de la vue. Zone : 0,16 → 0,10 ms.
3. **Icônes** (`src/theme/icons.rs`) : le tracé d'une icône (un « # » : douze
   formes) est triangulé une fois par icône, couleur et emplacement exact, puis
   reposé tel quel. Formes par image au repos : 1 219 → 504 ; tessellation
   0,60 → 0,45 ms. Le maillage est celui que le tracé direct aurait donné, au
   triangle près (test `cached_mesh_matches_direct_drawing`).
4. **Banc** (`examples/bench/mod.rs`, `src/perf/threads.rs`) : processeur par
   image et par phase (fil d'interface + autres fils), paires de vérification
   comparées au fil de l'eau, relevés mémoire corrigés (voir plus bas).

## Avant / après (release, médianes de 8 passes alternées, ms)

| Phase | image méd. avant | après | image p95 avant | après |
|---|---|---|---|---|
| repos (redessin forcé) | 1,73 | 1,56 (−10 %) | 2,77 | 2,39 |
| défilement | 1,79 | 1,73 (−4 %) | 2,83 | 2,66 |
| arrivée de 50 messages/s | 2,05 | 1,87 (−9 %) | 3,34 | 3,07 |
| changement de salon | 2,14 | 1,91 (−10 %) | 3,38 | 3,06 |
| historique (remontée, redescente) | 1,84 | 1,68 (−9 %) | 2,93 | 2,60 |

Mise en page seule (sans tessellation ni pilote), médiane : repos 1,05 → 0,89 ;
changement de salon 1,45 → 1,29.

Inchangés, dans le bruit : démarrage (fenêtre créée 180 à 350 ms selon la
charge, première image 32 à 97 ms), mémoire après chargement (125 à 140 Mo
privés), mémoire en fin de banc (177 à 195 Mo). Redessins en 10 s de repos :
0 avant, 0 après. Taille de l'aperçu : 27 061 248 → 27 093 504 octets (+32 Ko).

Captures `preview --screenshot` avant / après, identiques à l'octet sur neuf
scènes : écran principal, `--gallery icones`, `--gallery composants`,
`--settings apparence`, `--settings notifications`,
`--view connexion --scene hors-ligne`, `--mobile salons`, `--theme clair`,
`--panel`.

## Les « pics » de 450 à 520 Mo étaient un artefact du banc

Le banc gardait en mémoire toutes ses captures d'écran (seize, puis celles de
l'historique) jusqu'au rapport final. Ce n'est pas l'application. Les paires
sont maintenant comparées et rendues dès qu'elles sont complètes :

| Relevé (privé, Mo) | banc d'avant | banc corrigé |
|---|---|---|
| après les changements de salon | 465 | 155 à 168 |
| après l'historique | 536 à 542 | 183 à 195 |
| pic vu de l'extérieur | 537 à 542 | 198 à 210 |

La vérification « image du changement = image suivante » passait de 0 sur 8 à
8 sur 8 : le banc comparait pendant les fondus de survol et de barre de
défilement ; il attend maintenant qu'ils soient finis.

## Où part le temps d'une image (repos, profil sans LTO)

Mise en page 1,3 ms : rail 0,37, salons 0,37, liste des messages 0,18,
en-tête 0,14, membres 0,12, saisie 0,07. Tessellation 0,6 ms. Pilote OpenGL,
sur d'autres fils : 1,0 à 1,4 ms de processeur par image, à peu près fixe.
Les images et les textures ne coûtent rien de mesurable.

## Essayé puis retiré

- Maillage d'icône réutilisé à un autre emplacement en le décalant : l'accroche
  aux pixels dépend de la position, le résultat différait du tracé direct
  (jusqu'à un pixel sur `Screen`, `Gear`, `Download`). Remplacé par la clé à
  l'emplacement exact ; une liste qui défile refait ses icônes comme avant.

## Pas fait, et pourquoi

- Atlas commun pour les avatars : écarté, les images et les textures ne coûtent
  rien de mesurable par image.

- `panic = "abort"` : l'application rattrape les paniques des décodeurs.
- Retrait des polices par défaut d'egui (environ 1,4 Mo) : risque visuel.
- `opt-level = "s"` par crate : non essayé.
- wgpu / D3D à la place d'OpenGL (les 91 Mo du pilote) : les crates ne sont pas
  dans le cache local, il aurait fallu télécharger ; non mesuré.
- Démarrage : le temps est dans la création de la fenêtre et du contexte
  OpenGL, hors du code de l'application.
- Ouverture des paramètres, sélecteurs et menus : pas de phase au banc.
- Virtualisation complète du rail (ne plus allouer les pastilles lointaines) :
  elle ferait perdre le parcours au clavier et aux lecteurs d'écran.

## Refaire les mesures

```
cargo build --release --example preview
preview --bench --bench-label nom --bench-out fichier.json
```

Comparer deux exécutables en passes alternées, machine au calme, et lire les
médianes : une passe seule ne départage pas un écart de 10 %.
