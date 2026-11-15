# Plugins

Un **plugin** est une fonction propre à Ferrocord (absente de Discord) que
l'utilisateur active ou coupe dans **Paramètres > Plugins**. Le registre est
dans `src/plugins/mod.rs`, la page dans `src/ui/plugins.rs`, les textes dans
`src/i18n/plugins.rs`.

Règle : **un plugin coupé ne coûte rien**. Ni fil, ni minuterie, ni
redessin, ni mémoire notable. Ne pas y inscrire une fonction de base de
Discord.

## Ajouter un plugin en dix lignes

1. `src/i18n/plugins.rs` : le nom et la description (une ou deux lignes), en
   français et en anglais.

   ```rust
   COMPACT = "Mode compact IRC", "IRC compact mode";
   COMPACT_ABOUT = "Une ligne par message, sans avatar.", "One line per message, no avatar.";
   ```

2. `src/plugins/mod.rs` : un identifiant **à la fin** de `Id`, et sa ligne
   **à la fin** de `ALL` (même rang ; un test le vérifie).

   ```rust
   pub enum Id { /* … */ CompactIrc }

   Plugin::new(Id::CompactIrc, "mode_compact_irc", pt::COMPACT, pt::COMPACT_ABOUT, Category::Messages),
   ```

3. Là où la fonction commence :

   ```rust
   if crate::plugins::on(crate::plugins::Id::CompactIrc) { /* … */ }
   ```

C'est tout : la carte, l'interrupteur, la recherche, le filtre, le compteur
et l'enregistrement dans `local.toml` viennent du registre.

## La ligne du registre

`Plugin::new(id, clé, nom, description, catégorie)` : activé par défaut,
appliqué tout de suite, sans réglage. Puis, au besoin :

| Suffixe | Effet |
|---|---|
| `.off_by_default()` | Coupé tant que l'utilisateur ne l'active pas. À choisir pour toute fonction nouvelle qui surprendrait, coûte, ou touche à la vie privée. |
| `.needs_restart()` | Le changement ne vaut qu'au prochain lancement : `plugins::on` garde l'état lu au démarrage et la carte affiche « Redémarrage nécessaire ». |
| `.with_settings(&[…])` | Réglages propres (voir plus bas) : la carte gagne une roue dentée. |

- **Clé** (`"mode_compact_irc"`) : nom dans `local.toml`, en minuscules
  ASCII et tirets bas. Ne jamais la changer ni la réutiliser.
- **Catégorie** : `Appearance`, `Messages`, `Privacy`, `Activity`, `Voice`,
  `Navigation`, `System`. Une catégorie sans plugin n'est pas proposée dans
  le filtre.

## Lire l'état

```rust
plugins::on(Id::X) -> bool
```

Une lecture et un masque : pas de verrou, pas d'allocation. À appeler à
chaque image ou dans une boucle sans y penser, depuis l'interface, le magasin
ou un autre fil.

- Le fil de l'interface lit l'état de **son** application ; les autres fils
  lisent l'état partagé du processus. Dans les tests unitaires, l'état
  partagé reste aux valeurs par défaut : chaque test a le sien.
- Avant la lecture de `local.toml` (quelques millisecondes au lancement),
  ce sont les valeurs par défaut. Une fonction qui écrit ou efface sur le
  disque attend `self.local.loaded`, comme l'historique hors ligne.
- Dans un test : `app.set_plugin(Id::X, false)`.

## Ne rien coûter une fois coupé

- Ce qui se décide à chaque image (dessiner une ligne de menu, peindre un
  fond) : un `if plugins::on(..)` suffit.
- Ce qui tient une ressource (fil, tube, texture, fenêtre, lecture en
  cours) : la rendre quand le plugin est coupé. Deux façons :
  - un `tick` qui compare l'état voulu à l'état appliqué (comme
    `look_tick` pour l'image de fond, `offline_tick` pour l'historique) ;
  - `App::plugin_changed(id, on)` dans `src/ui/plugins.rs`, appelé une fois
    à chaque bascule (comme l'arrêt de la lecture à voix haute).
- Ne rien démarrer « au cas où » : le fil ou la minuterie naît à la
  première utilisation, plugin activé.

## Réglages propres

Le plugin **déclare** ses réglages ; la fenêtre (modale à pied : Enregistrer,
Annuler, Valeurs par défaut) est dessinée d'après la déclaration.

```rust
pub const TRANSLATE_ENGINE: &str = "moteur";
const ENGINES: &[(&str, Text)] = &[("local", pt::ENGINE_LOCAL), ("service", pt::ENGINE_SERVICE)];

Plugin::new(Id::Translate, "traduction", pt::TRANSLATE, pt::TRANSLATE_ABOUT, Category::Messages).with_settings(&[
    Setting { key: "garder_original", label: pt::KEEP, detail: pt::KEEP_TIP, kind: Kind::Bool(true) },
    Setting { key: TRANSLATE_ENGINE, label: pt::ENGINE, detail: pt::ENGINE_TIP, kind: Kind::Choice(ENGINES, "local") },
    Setting { key: "delai_ms", label: pt::DELAY, detail: pt::DELAY_TIP, kind: Kind::Number { min: 0, max: 2000, default: 300 } },
    Setting { key: "adresse", label: pt::URL, detail: pt::URL_TIP, kind: Kind::Text { default: "", max: 200 } },
]),
```

| Forme | Contrôle | Lecture |
|---|---|---|
| `Kind::Bool(défaut)` | interrupteur | `plugins::flag(id, clé) -> bool` |
| `Kind::Choice(options, défaut)` | liste | `plugins::choice(id, clé) -> &'static str` (la clé de l'option) |
| `Kind::Number { min, max, default }` | curseur | `plugins::number(id, clé) -> i64` (toujours dans les bornes) |
| `Kind::Text { default, max }` | champ | `plugins::text(id, clé) -> String` |

Ces lectures parcourent une petite liste (et `text` alloue) : pour un
chemin chaud, lire la valeur une fois par image ou au changement, pas par
message. Un réglage jamais touché rend sa valeur par défaut ; une valeur
illisible ou hors bornes dans le fichier est ignorée ou ramenée aux bornes.
La roue est grisée tant que le plugin est coupé.

Un réglage riche (aperçu, liste éditable, sélecteur de fichier) ne passe
pas par là : il garde sa propre section dans la page qui convient, et la
description du plugin dit où le trouver.

## Fichier

```toml
[plugins]
historique_hors_ligne = true
lecture_voix_haute = false

[plugins.reglages.lecture_voix_haute]
annoncer_auteur = false
```

Une clé absente vaut l'état par défaut (un plugin ajouté plus tard apparaît
donc dans son état par défaut) ; une clé inconnue est ignorée.

## Plugins inscrits par le lot P0

| Identifiant | Clé | Où il est branché |
|---|---|---|
| `OfflineHistory` | `historique_hors_ligne` | `ui/offline.rs` : coupé, l'historique est effacé et plus rien n'est écrit. L'interrupteur de « Données et confidentialité » est le même. Les anciennes clés (`historique_hors_ligne` à la racine, `[rich_presence] actif`) sont encore lues. |
| `RichPresence` | `rich_presence` | `ui/rpc.rs` : coupé, le tube discord-ipc est fermé. Même interrupteur que la page Activités. |
| `ReadAloud` | `lecture_voix_haute` | `ui/msg_actions.rs` : la ligne du menu n'existe plus ; une lecture en cours est arrêtée. Réglage : annoncer l'auteur. |
| `Backdrop` | `fond_personnalise` | `ui/look.rs` : ni image (texture rendue) ni dégradé. |
| `Animations` | `animations` | `LocalUi::motion_level` : cran « aucune ». |
| `AutoTheme` | `theme_jour_nuit` | `ui/look.rs` : aucune bascule, aucun réveil programmé. |
| `CustomFonts` | `polices_installees` | `ui/look.rs` : polices du thème, fichiers de police rendus. |

## Vérifier

`cargo test plugins` : registre (rangs, clés uniques, réglages cohérents),
aller-retour du fichier, page (liste, recherche, filtre, interrupteur,
« Tout désactiver », fenêtre de réglages). Aperçu :
`cargo run --example preview -- --settings plugins --screenshot p.png`
(`plugins-reglages` pour la fenêtre de réglages).

## Plugins inscrits par le lot E1

| Identifiant | Clé | Où il est branché |
|---|---|---|
| `MessageLog` | `journal_messages` | `ui/journal.rs` (modèle : `state/journal.rs`, fichiers : `offline/vault.rs`). **Coupé par défaut.** Activé, une suppression ou une modification reçue garde la version que le client avait en mémoire (jamais mes messages ni ceux que je supprime) ; coupé, `journal_before` et `journal_tick` rendent la main à la première ligne, le journal est effacé (mémoire et disque) et le fil des fichiers s'arrête. Réglages : jours de conservation, pièces jointes, bots. « Effacer le journal » : Données et confidentialité. Aperçu : `--scene journal`. |
| `Keywords` | `mots_cles_surveilles` | `ui/keywords.rs` (modèle : `state/keywords.rs`, liste dans `local.toml`, section `[mots_cles]`). À la réception d'un message (`keywords_before`/`keywords_after` dans `poll_events`) : notification par `desk_alert` si le message n'en donne pas déjà une, ligne dans « Pour vous », surlignage dans le fil (`keywords::marked`). Jamais pour mes messages, une personne bloquée, le salon lu ; « Ne pas déranger » : la ligne sans notification. Coupé : rien n'est regardé ni gardé. Liste : Paramètres > Notifications ; exclusion : menu du salon. Aperçu : `--scene mots-cles`, `mots-cles-reglages`. |
| `LocalSearch` | `recherche_locale` | `ui/local_search.rs` (index : `state/local_index.rs`, au plus 20 000 messages et 4 Mio en mémoire, 6 Mio sur le disque). `local_search_tick` range les salons du magasin dont la version a changé ; `local_search_before` retire un message supprimé. Bouton « Local » à côté du champ de recherche, onglets « Discord » / « Sur cet ordinateur » dans le panneau (`search::Active::local`) : en mode local, résultats pendant la frappe, pages et tri sur place, aucune commande. Enregistré (dossier `recherche`) seulement si `OfflineHistory` est activé ; « Effacer l'historique » efface aussi l'index. Coupé : rien n'est indexé ni écrit, le fichier est effacé. Aperçu : `--scene recherche-locale`. |
| `Export` | `export_conversation` | `ui/export.rs` (fichier : `state/export.rs`). Ligne « Exporter la conversation… » du menu d'un salon ou d'un MP (`local_plugins_menu`). Après le clic sur « Exporter… » : messages en mémoire, puis `Cmd::History` (la requête d'historique du client) une page à la fois par le limiteur du réseau, 400 ms entre deux pages, au plus 1000 pages ; `export_before` range la page dans l'export sans la donner au magasin. Pièces jointes : seulement case cochée et après la confirmation qui annonce leur taille (`Images::fetch_raw`, un fichier à la fois, dossier `<nom>_fichiers`). Le fichier (HTML autonome ou texte) ne contient que titre, période, auteurs, heures, textes, fichiers. Coupé : pas de ligne, rien. Aperçu : `--scene export`, `export-lecture`. |

Les fichiers des plugins locaux passent par `offline::vault::Vault` : un
dossier par sorte à côté de `historique` (`journal`, `recherche`), un
fichier chiffré par compte, la clé protégée par DPAPI, et un fil qui ne
naît qu'à la première demande (`Vault::files`) et s'arrête quand plus aucun
plugin ne s'en sert (`App::vault_release_if_idle`).

## Plugins inscrits par le lot E4 (voix)

Les textes sont dans `src/i18n/e4.rs`. Les réglages des plugins vocaux sont lus
sur le fil de l'interface (`audio::dsp::Extras::current`) et portés aux fils
audio avec le reste des réglages de l'appel (`voice::Settings::extras`) : les
fils audio ne consultent jamais le registre.

| Identifiant | Clé | Où il est branché |
|---|---|---|
| `NoiseRnn` | `suppression_bruit_rnnoise` | `audio/dsp/rnn.rs` (RNNoise, crate `nnnoiseless`). **Coupé par défaut.** Activé, RNNoise remplace l'étage spectral de `VoiceChain` (appel et « Test du micro ») tant que « Suppression du bruit » est activée ; retard ajouté : 10 ms. Coupé : l'état de RNNoise n'existe pas (`VoiceChain::has_rnn`). Réglage : intensité (10 à 100, mélange avec l'origine retardée d'autant) ; curseur et bouton « S'entendre » (le test du micro) dans Paramètres > Voix et vidéo, section Traitement. Mesuré par `cargo test audio::dsp::rnn -- --nocapture` : bruit de ventilateur -27 dB, bruit blanc -12,6 dB, voix d'essai gardée à 0,3 dB près, 0,5 à 1 % d'un cœur. |
| `VoiceEq` | `egaliseur_vocal` | `audio/dsp/eq.rs` : trois bandes (plateau grave 200 Hz, cloche 1,2 kHz, plateau aigu 5 kHz, -12 à +12 dB) sur le mixage des voix reçues, dans `engine::Playback::render_mix`, **avant** le limiteur (une bande poussée ne sature pas). Réglages : préréglage (Plat, Voix claires, Graves renforcés, Aigus adoucis, Radio, Nuit, Personnalisé) et les trois bandes de « Personnalisé ». Activé par défaut sur « Plat » : à plat ou coupé, `Eq3::process` rend la main sans rien traiter et rien n'est alloué. Le volume par personne (0 à 200 %, menu d'une personne en vocal) était déjà retenu entre les sessions dans `local.toml` (`state::voice::VoicePrefs`) et passe par le même limiteur : il ne dépend pas du plugin. |
| `CallRecorder` | `enregistrement_appel` | `ui/voice/record.rs` (interface), `voice/recorder.rs` (fil d'écriture), `state/call_rec.rs` (commandes). **Coupé par défaut.** Bouton à disque du panneau vocal → fenêtre d'avertissement à chaque fois (Discord ne prévient pas les autres, la loi peut exiger leur accord), dossier à choisir, « une piste par personne » ; rien ne démarre sans le clic sur « Démarrer l'enregistrement ». Le même avertissement s'affiche à l'activation du plugin. Pendant l'enregistrement : bandeau rouge dans le panneau vocal et pastille rouge dans la barre de titre (durée, arrêt d'un clic ; une image par seconde tant que ça enregistre). Arrêt seul : salon quitté ou changé, appel fini, plugin coupé. Audio : `engine::Capture::set_tap` (le micro tel qu'il est émis) et `engine::Playback::set_tap` (chaque voix décodée, avant volume et sourdine), posées par les deux fils audio quand `voice::media::Shared::set_rec` change ; un fil `ferrocord-enregistrement` mixe et écrit en Ogg/Opus mono, page par page (`audio::clip::ogg::OpusFile`), 64 kbit/s le mixage, 40 kbit/s par piste. Hors enregistrement : ni fil, ni file, ni prise (une lecture d'atomique par rappel audio). |
| `SpeakerOverlay` | `incrustation_qui_parle` | `ui/voice/overlay.rs`. **Coupé par défaut.** Pendant un appel, une fenêtre à part (viewport différé d'egui) : sans bordure, transparente, toujours au premier plan, hors barre des tâches, créée inactive, clics traversants (`with_mouse_passthrough`) ; avatars, noms, halo vert de qui parle, micro ou casque coupé. Fenêtre ordinaire du système, sans injection : elle ne s'affiche pas sur un jeu en plein écran exclusif. « Mode déplacer » : elle reçoit la souris, se glisse, se redimensionne par son coin ; « Terminé » retient position et taille dans les réglages du plugin. La fenêtre principale lui donne un modèle (`Arc<Mutex<Model>>`) et ne lui demande une image (`request_repaint_of`) que si ce modèle change ; elle-même n'en demande jamais. Hors appel ou coupé : ni fenêtre ni modèle. Tant qu'elle existe, `App::clear_color` rend une couleur transparente (la fenêtre principale peint tout son fond). Aucun `unsafe`. |
## Plugins inscrits par le lot E5

| Identifiant | Clé | Où il est branché |
|---|---|---|
| `CrashReport` | `rapport_plantage` | `src/crash.rs` (écriture, filtre, journal circulaire), `ui/crash_banner.rs` (bandeau). Le crochet de panique (`panics::install`) et une erreur fatale de `run` écrivent un fichier texte dans `%LOCALAPPDATA%\ferrocord\plantages\` : date, version, endroit, message filtré (`crash::scrub`), pile d'appels, derniers événements de `crash::note` (noms écrits dans le code, jamais un texte reçu). Dix rapports gardés. Au lancement suivant, un bandeau propose « Ouvrir le rapport », « Ouvrir le dossier », « Ignorer ». Rien n'est envoyé. Coupé : `note` rend la main après une lecture atomique, rien n'est écrit ni lu. Aperçu : `--scene plantage`. |
| `ShareThemes` | `themes_partageables` | Format et lecture stricte : `theme/share.rs` ; fichiers : `look.rs` (fil « apparence ») ; interface : `ui/theme_share.rs`. Un `.ferrotheme` est un fichier TOML : `ferrotheme = 1`, `nom`, `auteur` facultatif, le thème comme dans `theme.toml`, puis `[apparence]` (dégradé, réglages de l'image, polices) et `[image]` (base64, 4 Mio au plus). Lecture : fichier de 8 Mio au plus, thème lu par l'analyseur de `theme.toml` (tout écart refuse le fichier), image reconnue à ses octets et décodée par `images::decode`, aucun chemin repris ni suivi. Export : Apparence > « Thèmes partageables » (case « Inclure l'image de fond » : image réduite, réencodée en JPEG ; son chemin n'est jamais écrit). Import : bouton, fichier déposé sur la fenêtre (`theme_share_drop`, avant les pièces jointes) ou galerie « Thèmes fournis » (`assets/themes/`) ; toujours un aperçu, puis « Appliquer » : image copiée dans `<réglages>/fonds/`, profil d'apparence ajouté et appliqué ; zoom, largeurs, barre de titre et réglages personnels gardés. Coupé : ni section, ni lecture d'un fichier déposé. Aperçu : `--scene theme-import`, `--settings apparence`. |
| `Mica` | `mica_windows` | `platform/backdrop.rs` (DWM, tout l'`unsafe`), `ui/mica.rs`, `lib::with_backdrop`. **Coupé par défaut, redémarrage nécessaire** : la matière ne se voit que dans une fenêtre créée transparente, ce qui se décide avant sa création (`backdrop_at_boot` lit l'état du plugin dans `local.toml` ; le fichier n'est analysé que s'il nomme le plugin). `mica_tick` pose `DWMWA_SYSTEMBACKDROP_TYPE` (Mica ou acrylique, réglage `matiere`), la teinte sombre ou claire du thème et le cadre étendu une fois, puis seulement quand l'un d'eux change. Le rail et la colonne des salons sont peints en voile de la couleur du thème (`mica_fill`, réglage `voile`, de 80 à 95 % ; les parties internes de la colonne passent par `mica_inner` pour ne pas doubler le voile) ; tout le reste reste plein. Avant Windows 11 22H2, ou si DWM refuse : la fenêtre n'est pas créée transparente (ou est effacée en couleur pleine), les fonds restent ceux du thème, et la carte du plugin affiche « Non disponible sur ce système ». Aperçu : `--scene mica`, `mica-acrylique` (une capture `--screenshot` ne montre pas la matière du système). |

## Plugins inscrits par le lot E3 (discret et saisie)

Les textes sont dans `src/i18n/e3.rs`. Aucun de ces plugins n'ajoute de
requête à Discord, et aucun n'envoie quoi que ce soit sans un clic.

| Identifiant | Clé | Où il est branché |
|---|---|---|
| `Stealth` | `mode_discret` | `ui/stealth.rs`. **Coupé par défaut.** Trois réglages indépendants, relus seulement quand les plugins changent (`stealth_apply`, appelé par `plugins_sync`) : pas de « en train d'écrire » (`Composer::signal_typing` rend la main) ; lecture au clic (`state::reads::Reads::set_manual` : plus aucun accusé prévu, `select_channel` garde les pastilles, un message reçu salon ouvert le rend non lu, bouton « Marquer comme lu » de l'en-tête et du bandeau des nouveaux messages) ; médias floutés (`stealth::veiled` et `stealth::mosaic`, appelés par `attachments::tile_ui` et l'affiche de `player::video_tile` : une mosaïque de la texture déjà chargée, dévoilée au survol ou au clic, option « seulement dans les serveurs »). Des requêtes en moins, jamais en plus. Aperçu : `--scene discret`. |
| `Drafts` | `brouillons_persistants` | `ui/drafts.rs` (fichier : `state/drafts.rs`, dossier `brouillons` du coffre). Activé par défaut. La saisie avance un compteur (`Composer::rev`) ; `drafts_tick` arme une seule échéance cinq secondes après la première frappe non enregistrée et n'écrit qu'à cette échéance, à la fermeture de la session et à la sortie : jamais une écriture par frappe. Lecture une fois par compte ; rien n'est écrit avant qu'elle soit revenue. La réponse en préparation est gardée par salon ; les pièces jointes en attente ne le sont pas. Réglage : jours de conservation. Coupé : fichiers effacés, les brouillons restent en mémoire comme avant. Le crayon de la liste des salons existait déjà (`Composer::has_draft`). |
| `Scheduled` | `messages_programmes` | `ui/scheduled.rs` (modèle : `state/scheduled.rs`, dossier `programmes`). Activé par défaut. « Programmer… » et « Messages programmés » dans le menu « + » de la saisie. **L'envoi est toujours confirmé** : à l'échéance, `scheduled_tick` donne une notification locale (`desk_alert`) et le bandeau `scheduled_banner` (Envoyer, Reporter, Supprimer) ; la requête d'envoi ordinaire (`Store::send_message`) ne part que de `scheduled_send`, appelé par un bouton. Application fermée à l'échéance : le bandeau est là au lancement suivant. Un seul réveil demandé, pour la prochaine échéance à venir. Coupé : ni bandeau ni menu ; le fichier reste. Aperçu : `--scene programme`, `programmes`, `programmer`. |
| `Translate` | `traduction_message` | `ui/translate.rs` (modèle : `state/translate.rs`, requête : `net/translate.rs`). **Coupé par défaut.** Clic droit → « Traduire » ; la traduction se pose sous le message (`MessageMeta::translated`, `translate::under_ui`), avec « Afficher l'original ». Aucun service intégré : l'utilisateur saisit l'adresse d'un service compatible LibreTranslate (`POST <adresse>/translate`, API publique documentée), `https://` exigé sauf vers l'ordinateur ou le réseau local, et rien n'est contacté tant que « J'ai lu l'avertissement » n'est pas activé (`translate_config`). Une requête par clic, deux au plus à la fois, délai de 15 s, réponse bornée à 256 Kio, aucune redirection suivie, cache borné en mémoire. Clé d'API facultative : chiffrée dans le coffre (dossier `secrets`), jamais dans `local.toml` ni dans un journal. Tests avec un faux service sur 127.0.0.1. Aperçu : `--scene traduction`, `traduction-reglages`. |

Le coffre des plugins (`offline::vault`) range aussi un petit contenu déjà
encodé par son plugin : `Payload::Raw(Kind, octets)`, relu par
`Loaded::Raw` et rendu à `App::e3_loaded`. Un plugin qui ajoute un bout
d'interface à sa fenêtre de réglages déclarée le fait dans
`plugin_settings_modal` (comme la clé de la traduction).

## Plugins inscrits par le lot E2 (fenêtres)

Les textes sont dans `src/i18n/e2.rs`, ce que `local.toml` retient dans
`src/state/windows.rs` (section `[fenetres]` : onglets par compte, séparation
de la vue scindée, zoom et place de fenêtre par salon).

| Identifiant | Clé | Où il est branché |
|---|---|---|
| `ChannelTabs` | `onglets_salons` | `ui/tabs.rs` (modèle : `state::windows::Tabs`). Un onglet est un endroit (serveur, salon) ; l'onglet actif est toujours l'endroit affiché (`tabs_sync` au début de `main_ui`). La barre (`tabs_bar`, en haut de la zone de discussion) n'existe qu'à partir de deux onglets : avec un seul, ou le plugin coupé, la mise en page est celle d'avant (test `layout_is_unchanged_without_tabs`). Ouvrir : clic molette ou Ctrl+clic sur un salon ou un MP, « Ouvrir dans un onglet » (`windows_menu`), toujours en arrière-plan ; un onglet d'arrière-plan ne charge rien et ne dessine que sa pastille (non-lu, compte de mentions). Fermer : croix, clic molette, Ctrl+W ; glisser pour réordonner ; épingler par le menu (l'onglet épinglé garde son salon) ; Ctrl+Tab, Ctrl+Maj+Tab ; Ctrl+1 à 9 vont aux onglets tant que la barre est affichée (sinon aux serveurs, comme avant). Fenêtre étroite : les onglets se resserrent jusqu'à 112 points, puis la barre défile (molette) et un chevron donne la liste. Défilement et brouillon : ceux du salon (liste et saisie les gardent déjà par salon). Enregistré par compte ; un simple changement de salon est écrit à la première image deux secondes plus tard ou à la fermeture (aucune minuterie). Aperçu : `--scene onglets`. |
| `CommandPalette` | `palette_commandes` | `ui/palette.rs`. Ctrl+Maj+P (`palette_keys`, pris seulement plugin activé), ou « > » tapé dans le sélecteur rapide Ctrl+K (`palette_from_switcher`). Les actions sont réunies à l'ouverture et rendues à la fermeture ; la recherche (chaque mot tapé doit se trouver dans le titre, sans casse ni accents, score de `state::nav::score`) n'est refaite que quand le texte change. Coupé : ni touche prise, ni état. Aperçu : `--scene palette`. |
| `ChannelZoom` | `zoom_par_salon` | `ui/chat_zoom.rs`. Ctrl+molette sur une liste de messages (`zoom_wheel`, appelé par `message_list`), Ctrl+= et Ctrl+- pour la discussion active, Ctrl+0 pour 100 % (`zoom_keys`, avant le zoom de la fenêtre d'egui) ; de 60 à 250 % par pas de 10, annoncé par une notification. La liste reçoit des jetons et un style dont seules les tailles de texte sont multipliées (`zoomed`, `zoom_style`) ; en-tête, saisie, bouton du bas et bandeau gardent la taille du thème. Les hauteurs mémorisées valent pour une échelle par liste (`ChanView::scale` : zoom et pixels par point) : changer le zoom d'un salon ne refait que les siennes. Retenu par salon (`[fenetres.zoom]`, 200 salons au plus, 100 % oublié), écrit à la première image une seconde après le dernier changement ou à la fermeture. Coupé : facteur 1, rien n'est copié, et Ctrl+= / Ctrl+- / Ctrl+0 règlent de nouveau le zoom de toute l'interface. Sans discussion affichée ou Paramètres ouverts, ces touches gardent ce rôle. Aperçu : `--scene zoom`. |
| `SplitView` | `vue_scindee` | `ui/panes.rs`. « Ouvrir à droite » (`windows_menu`) appelle `split_open` ; `chat_area` remplace `chat` dans la zone centrale de `main_ui` et, sans vue scindée, l'appelle tel quel (mise en page identique). Le salon ouvert reste la discussion complète, avec la saisie ; l'autre est dessinée par `with_place` (magasin, liste, gestes sur les messages et notification échangés le temps du dessin) avec un en-tête à croix et un pied « Cliquez pour écrire ici ». Un clic dans l'autre échange les rôles sans changer de côté (`split_swap`) : la saisie par salon n'est jamais dessinée deux fois, chaque discussion garde son brouillon et sa position. Séparation tirée à la souris, en % dans `[fenetres] scinde` ; sous `2 × PANE_MIN` de large, une seule colonne. |
| `DetachedWindows` | `fenetres_detachees` | `ui/panes.rs`. « Ouvrir dans une fenêtre » appelle `detach_open` (4 au plus, une par salon) ; `detached_ui`, en fin de `main_ui`, montre chacune par `show_viewport_immediate` : en-tête, liste (`with_place`), saisie simple (`window_composer` : Entrée envoie par `send_text`, réponse comprise ; ni complétion ni pièces jointes). Place et taille dans `[fenetres.cadres]`, enregistrées une seconde après le dernier déplacement. Viewport immédiat : il emprunte l'application, donc il est dessiné avec la fenêtre principale et seulement avec elle (rien au repos). Le salon aussi affiché dans la fenêtre principale n'est dessiné qu'une fois (la fenêtre le dit). |

Discussions secondaires et réveils : `with_place` note chaque salon dessiné
hors du salon ouvert ; `panes_publish` les donne au filtre de la passerelle
(`Interest::show_also`, au plus `ALSO_MAX`) quand la liste change. Leurs
événements réveillent comme ceux du salon ouvert ; un onglet d'arrière-plan
n'y est jamais (seule sa pastille change).

Mode compact « type IRC » : ce n'est pas un plugin. Le mode « une ligne par
message » du thème (`[disposition] messages = "compact"`, préréglage
« Compact ») le faisait déjà : heure, pseudo coloré, texte sur une ligne, sans
avatar, réponse au-dessus, pièces jointes, embeds et réactions en dessous
(`ui/message.rs`). Le lot E2 ne le double pas : la palette de commandes le
bascule (« Basculer le mode compact »).

### Palette de commandes : le registre des actions

Toutes les actions de la palette viennent de `ui::palette::PROVIDERS` : une
fonction par module, appelée à l'ouverture, qui ajoute ses `Command` d'après
l'état du moment (elle peut n'en ajouter aucune).

```rust
// Dans le module qui propose l'action.
impl App {
    pub(super) fn mon_module_commands(&self, out: &mut Vec<palette::Command>) {
        let lang = self.lang;
        out.push(palette::Command::new(wt::G_ACTION, xt::FAIRE_CECI.get(lang), |app| app.faire_ceci()).chord(palette::ctrl(lang, "J")));
    }
}
// Dans `ui/palette.rs`, une ligne de plus dans `PROVIDERS` : `App::mon_module_commands`.
```

- `Command::new(famille, titre, action)` : la famille (`wt::G_NAV`, `G_ACTION`,
  `G_VIEW`, `G_SETTINGS`…) s'affiche en gris après le titre ; le titre est ce
  que l'utilisateur lit et cherche, dans sa langue ; `.chord(texte)` montre un
  raccourci à droite (`keys::chord_label` l'écrit dans la langue).
- L'action reçoit `&mut App` après la fermeture de la palette. Elle appelle la
  **même fonction que le bouton** : une action qui demande une confirmation
  (déconnexion, suppression) la demande donc ici aussi. Ne jamais y mettre
  d'action destructrice directe.
- Déjà inscrits : les actions des raccourcis clavier (`palette_key_actions`,
  avec leur combinaison du moment), tout marquer comme lu, page Amis, quêtes,
  mode compact, déconnexion (`palette_view`), les onglets (`tabs_commands`),
  le zoom du salon (`zoom_commands`), les discussions côte à côte et détachées
  (`panes_commands`), chaque page des paramètres, chaque thème, chaque statut,
  chaque plugin (activer ou couper), les autres comptes, et toutes les
  destinations du sélecteur rapide (« Aller à … »).
- `cargo test palette` vérifie que chaque titre existe dans les deux langues
  et qu'aucun n'est en double.
