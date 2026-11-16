# Lot « d3d » — Direct3D 12 par wgpu contre OpenGL (15 novembre 2026)

> **Mise à jour du 16 novembre (lot « gl »)** : « garder OpenGL » vaut toujours contre
> wgpu, mais le contexte OpenGL vient désormais d'ANGLE (Direct3D 11) : 60 à 130 Mo de
> moins que le pilote OpenGL d'AMD sur cette machine. Voir la dernière section.

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

## Lot « gl » (16 novembre 2026) : wgpu remesuré, Direct3D 11 par ANGLE retenu

Deux choses ont changé depuis ce document.

**Direct3D 11 est mesurable, et c'est lui qui gagne** : non par wgpu, mais
par ANGLE (la traduction OpenGL ES → Direct3D 11 de Chromium), qui donne à
`glow` le même contexte qu'avant. Le rendu d'egui ne change pas ; seul le
pilote chargé change (`atidxx64.dll`, 31 Mo d'image, au lieu de
`atio6axx.dll`, 62 Mo, ou des deux `amdxc64.dll` de Direct3D 12). Détail,
méthode et réserves : `docs/mesures.md`, lot « gl ».

**wgpu a été remesuré** avec les réglages qui comptent, dans une fenêtre
d'essai minimale (egui seul, 1100 × 720, 200 images forcées puis 3 s de
repos ; médiane de 3 passes ; la machine compilait en même temps, les temps
sont donc indicatifs, la mémoire non) :

| Rendu | Carte | Privée (Mo) | De travail privée (Mo) | Première image (ms) | Image médiane / p95 (ms) | Processeur par image (ms) |
|---|---|---:|---:|---:|---:|---:|
| glow, pilote OpenGL (avant) | dédiée¹ | 99,7 | 68,2 | 207 | 2,17 / 2,74 | 3,1 |
| glow, pilote OpenGL | intégrée | 167,5 | 145,4 | 611 | 2,10 / 2,63 | 4,6 |
| **glow, ANGLE (Direct3D 11)** | dédiée | 60,5 | 29,9 | 207 | 2,22 / 2,76 | 3,1 |
| **glow, ANGLE (Direct3D 11)** | **intégrée** | **39,1** | **18,0** | **157** | 2,15 / 2,81 | 2,5 |
| wgpu, Direct3D 12, `LowPower` | intégrée | 263,3 | 173,6 | 682 | 2,19 / 2,71 | 7,4 |
| wgpu, Direct3D 12, `HighPerformance` | dédiée | 276,9 | 182,1 | 708 | 2,17 / 2,81 | 11,3 |
| wgpu, Direct3D 12, latence maximale 1 | intégrée | 264,2 | 173,1 | 663 | 2,17 / 2,91 | 7,5 |
| wgpu, Vulkan, `LowPower` | intégrée | 91,0 | 64,6 | 337 | 1,87 / 2,47 | 3,5 |
| wgpu, Vulkan, `HighPerformance` | dédiée | 209,7 | 176,1 | 425 | 1,88 / 2,56 | 8,3 |
| wgpu, OpenGL (WGL) | dédiée | 122,0 | 61,9 | 299 | 2,38 / 3,20 | 4,7 |

¹ Sur cette machine, branchée au secteur, le pilote AMD envoie par défaut un
programme inconnu sur la carte **dédiée**, alors que l'écran est branché sur
l'intégrée.

Tous les essais wgpu : `MemoryHints::MemoryUsage`, sans validation, FXC pour
Direct3D 12, pas de MSAA, présentation `Fifo`. `desired_maximum_frame_latency`
ne change rien à la mémoire. Le premier essai n'était donc pas mal réglé :
Direct3D 12 charge toujours le pilote de chaque carte, WARP compris. Vulkan
est le meilleur wgpu (91 Mo sur l'intégrée), mais reste deux fois plus lourd
qu'ANGLE sur la même carte, deux fois plus lent à la première image, et
coûterait les 4,5 Mo d'exécutable et les réserves de transparence relevées
plus haut. **wgpu reste écarté** ; la fonctionnalité `rendu-wgpu` est gardée
telle quelle pour refaire la mesure.

`FERROCORD_RENDU` accepte maintenant `angle` (ou `d3d11`), `opengl` et
`d3d12` ; sans la variable : ANGLE s'il est posé à côté de l'exécutable,
OpenGL sinon.
