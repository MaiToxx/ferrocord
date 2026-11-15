# Lot « d3d » — Direct3D 12 par wgpu contre OpenGL (15 novembre 2026)

**Décision : garder OpenGL.** Sur la machine de l'utilisateur, le rendu `wgpu`
d'eframe sur Direct3D 12 est plus lourd et plus lent qu'OpenGL sur tous les
points mesurés sauf un (le travail du pilote hors du fil d'interface) :
+158 Mo de mémoire privée fenêtre vide, +180 Mo en fin de banc, démarrage
3,3 fois plus long (+400 ms), images 20 à 35 % plus lentes, exécutable
+4,5 Mo. L'hypothèse de `Cargo.toml` (« glow : beaucoup plus léger que wgpu
pour une interface 2D simple ») est confirmée par les chiffres.

## Conditions

- Machine : AMD Ryzen 7 4800H, 15,4 Go, Windows 11 26300 ; deux cartes :
  Radeon intégrée (pilote 31.0.21925.1001) et Radeon RX 5600M (32.0.21045.16010).
- Charge : aucune compilation (`cargo`, `rustc`) pendant les bancs ; processeur
  relevé avant chaque passe : 3 à 32 % (un navigateur de l'utilisateur, par à-coups).
- `preview --bench` par défaut (100 serveurs, 300 images par phase, 10 s de
  repos, phase « trafic »), release, fenêtre 1375 × 900, **3 passes alternées**
  par configuration (OpenGL, D3D12, OpenGL-double, OpenGL…) ; médiane des trois.
  `preview --bench-mem vide` et `app` : 3 passes aussi.
- Trois configurations :
  - **OpenGL** : exécutable d'aujourd'hui (sans la fonctionnalité `rendu-wgpu`) ;
  - **D3D12** : exécutable avec `rendu-wgpu`, `FERROCORD_RENDU=d3d12`, carte intégrée ;
  - **OpenGL-double** : le même exécutable à deux rendus, `FERROCORD_RENDU=opengl`
    (ce que coûte la cohabitation quand on ne s'en sert pas).
- wgpu 30.0.2, dorsal DX12 seul (ni Vulkan, ni Metal, ni GLES, ni WebGPU),
  compilateur de shaders FXC de Windows, sans validation,
  `MemoryHints::MemoryUsage`, carte « économe » (l'intégrée).
- **Direct3D 11 : non mesurable.** wgpu n'a plus de dorsal D3D11 (seulement
  D3D12, Vulkan, Metal, OpenGL) ; eframe 0.37 n'offre que glow ou wgpu.
  `FERROCORD_RENDU=d3d11` est traité comme une valeur inconnue (OpenGL).

## Mesures

| Mesure | OpenGL | D3D12 | OpenGL-double |
|---|---:|---:|---:|
| Fenêtre créée (ms depuis le début du banc) | 174 | 572 | 163 |
| Première image construite (ms) | 179 | 578 | 168 |
| Début de l'image suivante, première présentée (ms) | 217 | 631 | 205 |
| Image au repos, redessin forcé : médiane / p95 / max (ms) | 1,48 / 1,95 / 2,3 | 1,90 / 2,33 / 2,8 | 1,40 / 1,94 / 3,8 |
| Défilement : médiane / p95 / max | 1,65 / 2,09 / 2,8 | 1,97 / 2,44 / 4,5 | 1,55 / 2,17 / 3,5 |
| Arrivée de 50 messages/s | 1,79 / 2,49 / 3,0 | 2,15 / 2,85 / 3,8 | 1,75 / 2,48 / 4,3 |
| Changement de salon | 1,84 / 2,35 / 2,7 | 2,11 / 2,58 / 4,2 | 1,74 / 2,27 / 3,7 |
| Historique (remontée, redescente) | 1,65 / 2,20 / 7,5 | 2,03 / 2,59 / 15,4 | 1,52 / 2,20 / 5,3 |
| Trafic (paquets de la passerelle) | 1,47 / 2,07 / 3,3 | 2,01 / 2,73 / 5,8 | 1,48 / 2,13 / 3,6 |
| Processeur par image au repos : fil d'interface + autres fils (ms) | 1,93 + 1,09 | 3,18 + 0,00 | 1,88 + 0,89 |
| Mémoire après chargement : privée / de travail (Mo) | 126 / 126 | 274 / 231 | 125 / 126 |
| Mémoire en fin de banc : privée / de travail (Mo) | 178 / 147 | 358 / 264 | 178 / 148 |
| Fenêtre vide, avant toute image (`--bench-mem vide`) : privée / travail | 91–97 / 68 | 209 / 159 | 97 / 68 |
| Fenêtre vide après 3 s : privée / travail | 99 / 82 | 257 / 207 | 99 / 82 |
| Application chargée après 3 s (`--bench-mem app`) : privée / travail | 116 / 116 | 270 / 226 | 116 / 112 |
| Redessins en 10 s de repos | 0 | 0 | 0 |
| Images au repos sous trafic | 0 | 0 | 0 |
| Verdicts du banc (10) | tous tenus | tous tenus | tous tenus |
| Taille de `preview.exe` (octets) | 35 054 592 | 39 538 688 | 39 538 688 |

Le maximum est le plus grand des trois passes ; médiane et p95 sont la
médiane des trois passes. La phase « préchargement » (une image de 23 à 33 ms
dans les trois cas) n'est pas dans le tableau : elle ne dépend pas du rendu.

Contrôle sur la carte dédiée (`WGPU_POWER_PREF=high`, RX 5600M, banc court
`--bench-frames 100 --bench-quiet 2 --bench-deep 300`, une passe) : fenêtre
créée à 659 ms (628 sur l'intégrée dans le même banc court, 193 en OpenGL),
fenêtre vide 271 à 278 Mo privés, fin de banc 369 Mo. Pas mieux.

### Part du pilote

Fenêtre vide, avant la première image : le processus part de 2 Mo privés.
OpenGL l'amène à 91–97 Mo, Direct3D 12 à 209 Mo. Modules chargés
(`Get-Process … .Modules`, fenêtre vide) :

- OpenGL : `atio6axx.dll` (62 Mo d'image) et rien d'autre de notable ;
- D3D12 : **deux** `amdxc64.dll` (70 et 78 Mo : le pilote D3D12 de chacune des
  deux cartes), `d3d10warp.dll` (le rendu logiciel de Windows, 6 Mo),
  `d3dcompiler_47.dll` (4,5 Mo), `D3D12Core.dll` (3,4 Mo), `dcomp.dll`.

wgpu, pour choisir une carte, crée un périphérique Direct3D 12 sur **chaque**
carte du système, WARP compris ; son API n'offre pas de s'en tenir à une
seule. C'est ce qui explique à la fois la mémoire et les 400 ms de démarrage
(la carte dédiée est réveillée même si elle ne dessine rien). Sur une machine
à une seule carte le surcoût serait moindre, mais il resterait un pilote
D3D12 de 70 à 78 Mo plus WARP et le compilateur de shaders, contre 62 Mo
pour le pilote OpenGL : aucun gain à en attendre.

Les « 91 Mo du pilote OpenGL » de `docs/mesures-c10.md` sont donc bien réels,
mais ce n'est pas OpenGL qui est lourd : c'est le pilote AMD, et son
équivalent Direct3D 12 l'est davantage.

### Taille et compilation

- `preview.exe` : +4 484 096 octets (+12,8 %), que D3D12 serve ou non.
  `ferrocord.exe` (43 506 688 octets aujourd'hui) contient le même code de
  rendu : estimation 48,0 Mo, soit +4,5 Mo.
- 26 crates ajoutées à `Cargo.lock` (compilées seulement avec la
  fonctionnalité) : `wgpu`, `wgpu-core`, `wgpu-hal`, `wgpu-types`,
  `wgpu-core-deps-windows-linux-android`, `wgpu-naga-bridge`, `naga`,
  `naga-types`, `egui-wgpu`, `gpu-allocator`, `range-alloc`, `presser`,
  `allocator-api2`, `bit-set`, `bit-vec`, `codespan-reporting`, `hashbrown`
  0.16.1 (seconde version), `rustc-hash` 1.1.0 (seconde version), `libm`,
  `ordered-float`, `pollster`, `portable-atomic`, `portable-atomic-util`,
  `renderdoc-sys`, `type-map`, `unicode-width`. Licences : toutes
  « MIT OR Apache-2.0 », sauf `codespan-reporting` (Apache-2.0), `libm` et
  `ordered-float` (MIT). `portable-atomic*` et `renderdoc-sys` ne sont pas
  compilées pour Windows.
- `src/install/licences.json.gz` **n'est pas régénéré** : `tools/licences.ps1`
  suit les dépendances des fonctionnalités par défaut, et ces crates n'entrent
  pas dans l'exécutable livré.
- Compilation release (LTO « fat ») de l'aperçu : 855 s à froid sans la
  fonctionnalité (une autre compilation tournait au début) ; avec, 708 à
  712 s pour recompiler les crates ajoutées, eframe et l'édition de liens.
  Le surcoût à froid n'a pas été isolé (il aurait fallu deux compilations
  complètes de plus) ; l'édition de liens LTO domine dans les deux cas.

## Ce qui dépend du rendu, point par point

Inventaire du code : rien dans Ferrocord n'appelle OpenGL directement. Pas de
`PaintCallback`, pas d'usage de `glow::Context` (seule la signature
d'`App::on_exit` le nomme) ; les images, les avatars et les images de la vidéo
sont des textures d'egui ; les captures passent par
`ctx.request_screenshot`. Le changement de rendu ne touche donc que les
options de lancement.

| Point | Sous D3D12 |
|---|---|
| Texte, icônes, coins arrondis, dégradés, images, emojis | identiques à l'œil sur neuf scènes capturées dans les deux rendus (écran principal, thème clair, `emojis`, `theme-degrade`, `mica`, galerie de composants, `videos`, `parures`, Paramètres → Apparence) ; fichiers PNG différents à l'octet (arrondis du GPU) |
| Capture `--screenshot` et captures du banc | marchent (paires du changement de salon et aller-retour de l'historique : identiques) |
| Lecteur vidéo, vidéo des appels | textures d'egui : même chemin ; scène `videos` identique ; flux réel non essayé |
| Barre de titre intégrée, redimensionnement, ancrage | marchent (Win32, indépendants du rendu) |
| Redessins au repos | 0, comme en OpenGL |
| Fond Mica / acrylique (fenêtre transparente) | **non vérifié à l'écran.** La chaîne d'échange ordinaire de D3D12 est opaque ; le code demande alors celle posée sur un visuel DirectComposition, qui garde l'alpha, mais une capture ne montre pas la matière du système et rien n'a été regardé sur l'écran réel |
| Incrustation vocale (seconde fenêtre transparente) | **ne peut pas être transparente** quand la fenêtre principale ne l'est pas : eframe décide de la transparence d'après la fenêtre principale, et la chaîne d'échange D3D12 sur une fenêtre ordinaire n'a que le mode opaque. Fond noir attendu |
| `--bench-mem vide-logiciel` | n'a de sens qu'en OpenGL |
| Repli | périphérique D3D12 créé avant la fenêtre ; en cas d'échec, OpenGL sans rien avoir affiché (`rendu::repli()` dit pourquoi). Une panne après la création de la fenêtre (surface refusée) n'est pas rattrapée |

## Recommandation

Garder OpenGL, sans option dans Paramètres → Avancés : proposer un réglage
qui triple le démarrage, double la mémoire et grossit l'exécutable de tous de
4,5 Mo n'apporte rien à personne, et l'incrustation vocale y perdrait sa
transparence. Le critère (« plus léger et plus rapide que Discord ») est
mieux servi par l'état actuel.

Le seul avantage mesuré de D3D12 : le pilote ne travaille plus sur d'autres
fils (0,9 à 1,3 ms de processeur par image en OpenGL, 0 en D3D12), mais le fil
d'interface en fait alors davantage (3,2 ms contre 1,9) : le total par image
est le même, et l'image est plus longue.

## État du code

- Fonctionnalité Cargo `rendu-wgpu`, **désactivée par défaut** : sans elle,
  l'exécutable est celui d'avant, à ceci près que `launch` passe par
  `rendu::apply`, qui ne fait alors rien (colonne « OpenGL » du tableau).
- Avec elle : `FERROCORD_RENDU=d3d12|opengl` au lancement (`src/rendu.rs`,
  `ferrocord::with_rendu`), OpenGL par défaut, repli automatique sur OpenGL.
  `WGPU_POWER_PREF=high` choisit la carte dédiée. Le banc écrit le rendu et la
  carte utilisés dans son rapport (`rendu`).
- Pas de réglage dans les Paramètres, pas de mémorisation d'un échec : ils
  n'avaient de sens que si le rendu était proposé.
- Le code est gardé plutôt que retiré : il tient en un fichier de 200 lignes
  et trois lignes d'appel, ne change rien sans la fonctionnalité, et permet de
  refaire la mesure en dix minutes avec un autre wgpu, un autre pilote ou une
  machine à une seule carte :

  ```
  cargo build --release --example preview --features rendu-wgpu
  $env:FERROCORD_RENDU = 'd3d12'; preview --bench
  ```

  S'il gêne (montée de version d'eframe qui casse `rendu.rs`), le retirer est
  sans risque : supprimer `src/rendu.rs`, `with_rendu`, la fonctionnalité et
  la ligne `wgpu` de `Cargo.toml`.

## Pistes non essayées

- Créer soi-même le périphérique D3D12 sur une seule carte (DXGI, `unsafe`)
  et le donner à wgpu : éviterait le second pilote et WARP, pas le premier
  (70 à 78 Mo contre 62 en OpenGL). Gain possible au démarrage, pas en mémoire.
- Direct3D 11 : il faudrait un rendu egui écrit à la main (aucun n'est
  maintenu pour egui 0.37). Hors de proportion.
- Le vrai levier contre les 91 Mo du pilote reste hors du rendu : rien à
  gagner en changeant d'API tant que le pilote AMD pèse ce poids.
