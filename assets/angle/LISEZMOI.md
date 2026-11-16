# ANGLE embarqué (lot « gl »)

`libEGL.dll.gz` et `libGLESv2.dll.gz` : les deux bibliothèques d'ANGLE (Google,
OpenGL ES traduit en Direct3D 11) pour Windows x64, compressées par
`gzip -9 -n`. `src/rendu/angle.rs` les embarque dans l'exécutable et les pose
à côté de lui dans le dossier d'installation ; pourquoi : `docs/mesures.md`,
lot « gl ».

Google ne publie pas de binaires d'ANGLE : ceux-ci viennent de la publication
officielle d'Electron, qui les compile avec Chromium.

| | |
|---|---|
| Source | `https://github.com/electron/electron/releases/download/v43.8.10/electron-v43.8.10-win32-x64.zip` |
| sha256 de l'archive (égal à celui de `SHASUMS256.txt` de la publication) | `19441bb2a64246c2cd19f085c7deb747de442840effda0fc54ba6d640196c28c` |
| Version d'ANGLE (ressource du fichier) | 2.1.28055, révision `376a0443570f` |
| `libEGL.dll` | 467 968 octets, sha256 `ef6b85b6aaf7126aecad3b9d9c73c2017273a1d474cb7445c56fb946218516fe` |
| `libGLESv2.dll` | 8 076 800 octets, sha256 `cc1fcfa494b64ccba3aefbe466009103e96a709affa1dbfbe23123dffe55663d` |

Electron 44 et suivants ne livrent plus ces deux fichiers à part (ANGLE y est
lié dans l'exécutable) : la branche 43 est la dernière où les prendre.

Licence : BSD à trois clauses (`LICENSE.txt`, copie du fichier `LICENSE` du
dépôt d'ANGLE). ANGLE lie lui-même des bibliothèques tierces ; leurs mentions
sont dans `tiers/`, un fichier par bibliothèque : Abseil, libc++, libc++abi,
llvm-libc et compiler-rt (LLVM), zlib, xxHash, SMHasher, murmur3,
SPIRV-Tools, SPIRV-Headers, Vulkan-Headers, VulkanMemoryAllocator, volk,
Vulkan Deps, en-têtes et registres Khronos (EGL, OpenGL), base de Chromium.
Ces textes sont extraits tels quels de `LICENSES.chromium.html`, le fichier de
mentions livré dans la même archive Electron (retéléchargée au lot « gl2 »,
même sha256). La liste est volontairement large : toute bibliothèque de ce
fichier qu'ANGLE peut lier dans sa compilation pour Windows y est. Tout est
montré dans Paramètres › À propos par `tools\licences.ps1`.

Mettre à jour : remplacer les deux `.gz`, puis les tailles et les sha256 dans
`src/rendu/angle.rs` (le test `bibliotheques_posees_…` les vérifie), ce
tableau, et refaire `tiers/` depuis le `LICENSES.chromium.html` de la nouvelle
archive, puis relancer `tools\licences.ps1`.
