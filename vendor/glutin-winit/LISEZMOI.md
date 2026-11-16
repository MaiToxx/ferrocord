# glutin-winit 0.5.0, copie modifiée

Copie de la caisse `glutin-winit` 0.5.0 (licence MIT, fichier `LICENSE`,
https://github.com/rust-windowing/glutin), avec **une** différence : la
variable `PREFER_EGL` de `src/lib.rs`. Quand Ferrocord la met à vrai (lot
« gl », `src/rendu/angle.rs`), le contexte est demandé à EGL (ANGLE, donc
Direct3D 11) avant WGL (le pilote OpenGL du fabricant) ; eframe 0.37 ne
permet pas de le choisir. Sans cela, le comportement est celui de l'original.

`Cargo.toml` la branche par `[patch.crates-io]`. À une montée de version de
glutin-winit : recopier la nouvelle version et refaire ce seul changement.
