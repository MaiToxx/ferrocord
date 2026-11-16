# Fonctions de Discord qui reposent sur l'API non documentée (lot ND)

Inventaire du client de bureau officiel, fonction par fonction : ce que
Ferrocord a déjà, ce que le lot ND ajoute, ce qui est écarté et pourquoi.
Le détail de chaque requête envoyée (corps, moment, jeu d'essai) est dans
`docs/passerelle-utilisateur.md`.

Source des formes : https://docs.discord.food (dépôt public
`discord-userdoccers`), lu le 15 novembre 2026 pour ce lot. « Risque » est
le risque pour le compte : **faible** (lecture à la demande, forme connue),
**moyen** (écriture, ou route que seul le client officiel appelle
d'ordinaire), **fort** (touche à l'anti-abus, à l'argent ou à l'identité
du client).

## Règles appliquées à toute requête non documentée

1. forme vérifiée sur une source publique citée ;
2. jeu d'essai synthétique et test qui ne contacte que `127.0.0.1` ;
3. déclenchée par un geste de l'utilisateur, ou à un rythme humain ;
4. passe par le limiteur de débit commun ;
5. consignée dans `docs/passerelle-utilisateur.md`.

Interdits, sans exception : aucune automatisation (jamais de progression
de quête simulée), aucun achat (le navigateur s'ouvre), aucun
contournement de captcha (l'action s'arrête et le dit), pas d'imitation
d'empreinte TLS. Une fonction qui ne peut pas respecter ces règles est
écartée ci-dessous avec sa raison.

## 1. Profils et cosmétiques

| Fonction | Source | Requêtes | Risque | État |
|---|---|---|---|---|
| Profil complet, serveurs et amis en commun, badges, comptes liés | resources/user, Get User Profile | `GET /users/{id}/profile` | faible | déjà fait (lot profil-complet) |
| Notes | resources/user, Get/Modify User Note | `GET`/`PUT /users/@me/notes/{id}` | faible | déjà fait |
| Décorations d'avatar, plaques, tag affiché, couleurs du nom, bannières et avatars animés | resources/user (objets `user`) | aucune (champs lus) | nul | déjà fait (lot P) |
| **Effets de profil** : lecture sur la carte et le profil complet | resources/collectibles, Get Collectibles Product et Get Collectibles Categories ; liste publique npm `@discord-user-card/profile-effects` (`id` ≠ `sku_id`) | `GET /collectibles-products/{sku}`, `GET /collectibles-categories` (une fois par session, pour relier l'effet à son SKU) | faible | **fait (lot ND, chaîne corrigée par le lot profil)** |
| **Choix de l'effet** dans « Modifier le profil » | resources/collectibles, List User Purchased Collectibles ; resources/user, Modify User Profile | `GET /users/@me/collectibles-purchases`, `PATCH /users/@me/profile` (`profile_effect_id`) | moyen (écriture ; Discord refuse sans droit) | **fait (lot ND)** |
| **Choix de la décoration d'avatar et de la plaque** possédées | resources/user, Modify Current User (`avatar_decoration_sku_id`, `nameplate_sku_id`) | `PATCH /users/@me` | moyen (écriture) | **fait (lot ND)**, au clic dans « Modifier le profil » |
| **Choix du tag de serveur** | resources/user, Set Guild Identity ; resources/guild, Guild Identity Structure | `PUT /users/@me/clan` (`identity_guild_id`, `identity_enabled`) | moyen (écriture) | **fait (lot ND)**, au clic dans « Modifier le profil » |
| Polices et effets du nom (néon, pop…) | resources/user, Display Name Style | aucune (champs lus) | nul | couleurs et effets faits (dégradé, lueur, contour, ombre : lot profil) ; polices : les quatorze polices libres (OFL 1.1) sont embarquées et chargées à la demande (lots polices et polices2, `ui/fonts.rs`, table `NAMES`) ; Néo-Castel (7), publiée sur Gumroad seulement et sans licence libre, garde la police de Windows la plus proche, ou le fichier `polices/7.otf` posé par l'utilisateur dans le dossier des réglages ; l'association identifiant → police a été relue sur la source le 2026-11-15 (docs.discord.food/resources/user) et correspond à la table `NAMES` pour les seize identifiants |
| Infobulle détaillée des badges (ancienneté du compte, de l'abonnement, du boost ; palier Nitro) | identifiants de badges : gist public XYZenix/c45156b7c883b5301c9028e39d71b479 ; dates : `premium_since`, `premium_guild_since`, identifiant du compte | aucune (champs lus) | nul | **fait (lot profil)** ; le nom du palier d'ancienneté du compte (« Grand pin »…) n'a aucune source publique : non affiché |
| « Collection de jeux », « Liste de souhaits » sur la carte | resources/widgets ne décrit que des widgets de 20 jeux au plus ; rien sur la collection complète ni sur la liste de souhaits de la boutique | aucune | — | **écarté (lot profil)** : pas de source publique |
| **Style du nom** : effet de couleur, police, une ou deux couleurs | resources/user, Modify Current User (`display_name_font_id`, `display_name_effect_id`, `display_name_colors`) | `PATCH /users/@me` | moyen (écriture ; réservé à Nitro, Discord refuse sinon) | **fait (lot éditeur)** : sélecteur « Style du pseudonyme » de l'éditeur de profil, envoyé à « Enregistrer » ; aperçu rendu comme sur la carte (`ui/cosmetics.rs`, `styled_job` : effet, police du style) ; effets proposés à l'écriture : 1 à 6 seulement (7 prisme et 8 « gummy » ne sont décrits qu'en lecture : rendus, jamais envoyés, `NameEffect::writable`) |
| **Widgets de jeux du profil** (« Tableau », « Liste de souhaits ») | resources/widgets ; resources/game, Get Games | `PUT /users/@me/widgets`, `GET /users/@me/widgets/suggested-games`, `GET /games?game_ids=` ; lecture dans `GET /users/{id}/profile` | moyen (écriture) | **fait (lot éditeur)** : colonne de droite de l'éditeur de profil ; recherche de jeu sur la liste des jeux détectables (aucune recherche n'est décrite) |
| Onglet « Activité » de l'éditeur de profil, cadre de profil | aucune | — | — | écarté : aucune source publique (règle 1) |
| Plaques animées | docs.discord.food, reference, « CDN Formatting », Nameplate (`assets/collectibles/{asset}asset.webm`, à côté de `static.png`) | aucune (un fichier du CDN de plus, sans identification) | nul | **fait (lot plaques)** : jouée sous le pointeur seulement, image fixe au repos (`images/webm.rs`) ; décodée par Windows (VP8/VP9, « Extensions vidéo VP9 »), image fixe gardée sans ce décodeur ; adresse et lecture confirmées sur un vrai compte ; transparence (lot plaques2) : piste `AlphaMode`, alpha de chaque image dans les `BlockAdditions` (RFC 9559), décodé par un second décodeur et composé, fichier refusé (image fixe gardée) si l'alpha manque ; **non vérifié sur un vrai fichier** : que la transparence y est bien rangée ainsi |
| Thème et couleurs du profil, bannière, bio, pronoms, profil par serveur | resources/user | `PATCH /users/@me/profile`, `PATCH /guilds/{id}/profile/@me` | moyen | déjà fait |
| Activité récente d'un profil (« a joué à… ») | aucune (resources/content-inventory n'existe pas sur docs.discord.food, 15 novembre 2026) | `GET /content-inventory/users/{id}/outbox` | faible | écarté : aucune source publique ne décrit la forme (règle 1) |
| Statut de jeu détaillé, Rich Presence, jeux détectés | topics/rpc, resources/application | `GET /applications/detectable`, `…/assets` | faible | déjà fait (lots C3 et R) |

## 2. Boutique, Orbes, Nitro, cadeaux

| Fonction | Source | Requêtes | Risque | État |
|---|---|---|---|---|
| Solde d'Orbes | topics/gateway-events, Virtual Currency Balance Update | aucune (événement) | nul | **fait (lot ND)**, affiché sur la page des quêtes quand Discord l'annonce |
| **Boutique en consultation** (catégories, produits, images) | resources/collectibles, Get Collectibles Categories | `GET /collectibles-categories` | faible | **fait (lot ND)** : page « Boutique » ; « Voir dans Discord » ouvre le navigateur ; prix de base affiché à titre indicatif (resources/store, Subscription Prices Structure) |
| Achat (boutique, Nitro, boosts, cadeaux à offrir, Orbes dépensées) | resources/store, billing | `POST …/purchase`, moyens de paiement | fort | **écarté** : aucune requête de paiement, jamais |
| Inventaire : droits possédés, cadeaux reçus | resources/entitlement | `GET /users/@me/entitlements`, `GET /users/@me/entitlements/gifts` | faible | **fait (lot ND)** : onglet « Inventaire » de la boutique, lecture seule |
| Accepter un lien cadeau | resources/entitlement, Redeem Gift Code | `POST /entitlements/gift-codes/{code}/redeem` | moyen à fort (souvent derrière un captcha) | à trancher ; par défaut la carte du lien ouvre le navigateur |
| État de l'abonnement Nitro, emplacements de boost | resources/billing, premium | `GET /users/@me/billing/subscriptions`, `GET /users/@me/guilds/premium/subscription-slots` | moyen (pages de facturation) | écarté pour l'instant : lien vers les réglages de Discord ; la liste des boosts d'un serveur existe déjà (lot A8) |
| Promotions partenaires, parrainage Nitro | resources/promotion, premium-referral | `GET /outbound-promotions`, … | moyen | écarté : sans intérêt hors du client officiel |

## 3. Quêtes

| Fonction | Source | Requêtes | Risque | État |
|---|---|---|---|---|
| Page des quêtes : liste, image d'ambiance, logotype, récompense, échéance, progression ; tri, filtre et masquage locaux | resources/quests, Get User Quests ; reference, CDN Endpoints (images, sans jeton) | `GET /quests/@me` | faible | **fait (lot ND, refonte « quêtes »)** |
| Accepter | Enroll in Quest | `POST /quests/{id}/enroll` | moyen | **fait (lot ND)**, au clic |
| Réclamer, afficher le code | Claim Quest Reward, Get Quest Reward Code | `POST /quests/{id}/claim-reward`, `GET /quests/{id}/reward-code` | moyen | **fait (lot ND)**, au clic, seulement si Discord dit la quête terminée |
| Progression (jeu, vidéo, console, activité) | Send Quest Heartbeat, Video Progress, Console Start | `POST /quests/{id}/heartbeat`, `/video-progress`, `/console/start` | fort | **écarté, définitivement** : ce serait déclarer une activité qui n'a pas lieu. La carte dit « Commencer la quête » (ou la progression reçue) et ouvre la quête dans Discord |
| Récompense avec choix de console | Claim Quest Reward (`platform`) | — | moyen | renvoyé vers l'application officielle |

## 4. Serveurs : découverte, adhésion, accueil

| Fonction | Source | Requêtes | Risque | État |
|---|---|---|---|---|
| Découverte de serveurs (recherche, catégories) | resources/discovery | `GET /discoverable-guilds`, `GET /discoverable-guilds/search`, `GET /discovery/categories` | faible | fait (lot ND) : page en consultation ; rejoindre passe par la fenêtre d'invitation ou le navigateur |
| Rejoindre un serveur trouvé | resources/invite, guild | `POST /invites/{code}`, `PUT /guilds/{id}/members/@me` | moyen (captcha fréquent) | par invitation : déjà fait ; captcha : arrêt |
| Vérification des membres (règles à accepter) | resources/guild, Member Verification | `GET /guilds/{id}/member-verification`, `PUT /guilds/{id}/requests/@me` | moyen | fait (lot ND) : fenêtre « Règles et vérification », lecture puis envoi au clic |
| Demandes d'adhésion (côté modération) | resources/guild, Join Requests | `GET /guilds/{id}/requests`, `PATCH …/requests/id/{id}` | moyen | fait (lot ND) : même fenêtre, une décision par clic, pas d'action groupée |
| Onboarding, guide du serveur, écran de bienvenue | documentés par Discord en partie | `GET /guilds/{id}/onboarding`, … | faible | déjà fait (lots A7, intégration) ; guide du serveur (« home ») : non fait (hors de la liste du lot ND) |
| Parcourir les salons, salons suivis | resources/user-settings | `PATCH /users/@me/guilds/{id}/settings` | moyen | déjà fait |
| Dossiers de serveurs, ordre, réglages synchronisés | resources/user-settings-proto | `GET`/`PATCH /users/@me/settings-proto/{1,2}` | moyen | déjà fait |
| Tag de serveur (côté administration) | resources/guild, clan | `PATCH /guilds/{id}/…` | moyen | non fait : écriture d'administration, hors de la liste du lot ND |
| Hubs étudiants, répertoire de serveurs d'un hub | resources/directory | `GET /channels/{id}/directory-entries` | faible | écarté : fonction retirée ou marginale |

## 5. Applications

| Fonction | Source | Requêtes | Risque | État |
|---|---|---|---|---|
| Commandes, composants, fenêtres modales | interactions (comptes utilisateur) | `GET …/application-command-index`, `POST /interactions` | moyen | déjà fait (lot T11) |
| Répertoire d'applications (recherche, fiche) | resources/application-directory | `GET /application-directory-static/categories`, `GET /application-directory-static/search` | faible | fait (lot ND) : onglet « Applications » de la page « Découvrir », consultation ; l'ajout ouvre le navigateur |
| Ajouter une application à un serveur ou au compte | oauth2 authorize | `POST /oauth2/authorize` | fort (captcha, autorisation) | **écarté** : ouvre le navigateur |
| Activités intégrées, liens d'activité | resources/application, activities | iframe + RPC | fort | **écarté** : il faudrait embarquer un navigateur |
| Applications autorisées (liste, révoquer) | resources/oauth2, tokens | `GET /oauth2/tokens`, `DELETE /oauth2/tokens/{id}` | moyen | déjà fait (Paramètres, `net/account.rs`) |

## 6. Messages

| Fonction | Source | Requêtes | Risque | État |
|---|---|---|---|---|
| Messages vocaux | lot phase 6 | envoi par pièces jointes | moyen | déjà fait |
| Super réactions | resources/message (`burst`) | `PUT …/reactions/…?type=1` | moyen | déjà fait (lot G) |
| Sondages, transferts de message | documentés par Discord | — | faible | déjà fait |
| Sélecteur de GIF, favoris | lot G | `/gifs/*`, `settings-proto/2` | faible | déjà fait |
| Emojis favoris et fréquents, tableau de sons | settings-proto/2, soundboard | — | moyen | déjà fait |
| Accusés de lecture, marquer comme non lu, mentions récentes, « Pour vous » | lots S3, navigation | `POST …/ack`, `GET /users/@me/mentions`, notification-center | moyen | déjà fait |
| Recherche de messages | lot T12 | `GET …/messages/search` | faible | déjà fait |
| Résumés de salon (IA) | resources/message, Get Conversation Summaries | `GET /channels/{id}/summaries` | faible | fait (lot ND) : panneau ouvert par le menu du salon, lecture seule |
| Rappels de message (« Me le rappeler ») | resources/user, Saved Messages | `GET /users/@me/saved-messages`, `PUT`/`DELETE /users/@me/saved-messages/{salon}/{message}` | moyen | fait (lot ND) : menu du message et onglet « Enregistrés » ; pas de notification locale à l'échéance |
| Traduction, titres et résumés par IA | resources/ai | `POST /ai/translate`, … | moyen | écarté : expérimental, réservé à des groupes d'essai |
| Demandes de message, spam | lot U | — | moyen | déjà fait |
| Signalement | resources/report | menus à étapes | fort | écarté : formulaire dans le navigateur |

## 7. Amis et relations

| Fonction | Source | Requêtes | Risque | État |
|---|---|---|---|---|
| Amis, blocages, demandes, pseudos d'amis, MP de groupe | lot S3 + T9 | `/users/@me/relationships…` | moyen | déjà fait |
| Suggestions d'amis | resources/relationships, Get Friend Suggestions | `GET /friend-suggestions`, `DELETE /friend-suggestions/{id}` | faible | fait (lot ND) : onglet « En attente », Ajouter / Ignorer |
| Synchronisation des contacts | resources/connected-accounts | envoi du carnet d'adresses | fort (données de tiers) | **écarté** |
| Amis et serveurs en commun | Get User Profile | — | faible | déjà fait |
| Ignorer un utilisateur | resources/relationships | `PUT /users/@me/relationships/{id}/ignore` | moyen | déjà fait (menu de l'utilisateur) |

## 8. Compte, sécurité, confidentialité

| Fonction | Source | Requêtes | Risque | État |
|---|---|---|---|---|
| Appareils et sessions | resources/auth, sessions | `GET /auth/sessions`, `POST /auth/sessions/logout` | moyen | déjà fait |
| Connexions de compte (liste, visibilité) | resources/connected-accounts | `GET /users/@me/connections` | faible | déjà fait ; ajouter une connexion : navigateur (OAuth tiers) |
| Connexion par QR code | lot C2 | passerelle `remote-auth` | moyen | déjà fait |
| Mot de passe, e-mail, A2F, suppression du compte | resources/user, auth | écritures sensibles, captcha | fort | **écarté** : navigateur |
| Demande de ses données, consentements | resources/user, harvest ; consent | `GET`/`POST /users/@me/harvest`, `GET /users/@me/consent` | moyen (déclenche un e-mail) | à trancher ; par défaut lien vers les réglages de Discord |
| Centre familial | resources/family-center | `GET /family-center/@me`, … | moyen | écarté : renvoyé vers Discord (demande explicite du document d'écarts) |
| Centre de sécurité (état du compte, sanctions) | resources/safety-hub | `GET /safety-hub/@me` | faible | fait (lot ND) : section « État du compte », lue au clic ; recours dans Discord |
| Filtres de contenu sensible, MP des serveurs | settings-proto/1 | `PATCH /users/@me/settings-proto/1` | moyen | déjà fait (page Confidentialité et sécurité : filtre de contenu explicite par origine, MP des membres des serveurs, filtre des MP indésirables), par les réglages synchronisés déjà consignés |

## 9. Passerelle

| Fonction | Source | Risque | État |
|---|---|---|---|
| Identité de client de bureau | décision du 14 novembre 2026 | accepté par l'utilisateur | déjà fait |
| Liste des membres (op 37), souscriptions (op 14) | topics/gateway | moyen | déjà fait |
| `capabilities` dans l'Identify | topics/gateway, Capabilities | **fort** : change la forme du READY (utilisateurs dédupliqués, `merged_members`, réglages en protobuf seulement) et donc tout le décodage de connexion ; une erreur ferait échouer la connexion | **écarté pour ce lot** : à traiter seul, avec capture de READY sur le vrai compte (voir « À capturer pour les capacités modernes ») |
| Événements des quêtes et du solde d'Orbes | topics/gateway-events | nul | **fait (lot ND)** |
| Statistiques, expériences, `science` | topics/experiments, science | fort (télémétrie imitée) | **écarté** : Ferrocord n'envoie aucune télémétrie |
| Empreinte TLS, en-têtes `X-Super-Properties` du client officiel | — | fort | **écarté** (interdit) |

## État au terme du lot ND

Faits, chacun relu sur sa page de documentation avant d'écrire la requête
(règle 1) et consigné dans `docs/passerelle-utilisateur.md` : inventaire
(droits et cadeaux reçus) ; choix de la décoration et de la plaque ;
suggestions d'amis ; découverte de serveurs ; résumés de salon ;
vérification des membres et demandes d'adhésion ; répertoire
d'applications ; centre de sécurité ; messages enregistrés et rappels.
Les filtres de contenu étaient déjà en place (réglages synchronisés).

Écarté faute de source : l'activité récente d'un profil (aucune page
publique ne décrit `content-inventory`).

Reste, hors de la liste traitée : le guide du serveur (« home », ligne de
la section 4) et le tag de serveur côté administration. Plus aucune ligne
n'est marquée « à relire ».
