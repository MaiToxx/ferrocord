# Captures de tous les écrans avec l'aperçu hors ligne (`examples/preview.rs`),
# pour l'audit d'alignement (`docs/audit-alignement.md`).
#
#   tools\captures.ps1 -Out C:\fc-t\align\caps\avant [-Sizes 1024x576,1280x720] [-Themes cendre,clair] [-Only salons,amis] [-Jobs 4]
#
# Les tailles sont en points : à 125 %, 1024x576 donne une image de 1280x720
# pixels. Aucune connexion, aucun jeton : seul l'exemple `preview` est lancé.
param(
    [Parameter(Mandatory = $true)][string]$Out,
    [string[]]$Sizes = @('1024x576'),
    # Thèmes en plus de celui par défaut : un dossier « taille-thème » chacun.
    [string[]]$Themes = @(),
    [string[]]$Only = @(),
    [int]$Jobs = 4,
    [string]$Exe = ''
)
$ErrorActionPreference = 'Stop'
# Avec `powershell -File`, une liste arrive en une seule chaîne.
$Sizes = @($Sizes | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
$Only = @($Only | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
$Themes = @($Themes | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
if (-not $Exe) {
    $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $PSScriptRoot '..\target' }
    $Exe = Join-Path $target 'release\examples\preview.exe'
}
if (-not (Test-Path $Exe)) { throw "aperçu introuvable : $Exe (cargo build --release --example preview)" }

# Nom de la capture, puis arguments de l'aperçu.
$shots = [ordered]@{
    'salon'                 = '--view guild'
    'salon-survol'          = '--view guild --pointer 600,300'
    'salon-survol-ligne'    = '--view guild --pointer 150,200'
    'salon-notification'    = '--view guild --notice'
    'salon-clair'           = '--view guild --theme clair'
    'salon-compact'         = '--view guild --theme compact'
    'salon-aere'            = '--view guild --theme aere'
    'salon-sans-images'     = '--view guild --no-images'
    'mp'                    = '--view dm'
    'amis'                  = '--view amis'
    'groupe'                = '--view groupe'
    'masque'                = '--view masque'
    'lecture-seule'         = '--view lecture-seule'
    'sans-historique'       = '--view sans-historique'
    'exclusion'             = '--view exclusion'
    'profil'                = '--view guild --profile'
    'nouveaux'              = '--scene nouveaux'
    'bouton'                = '--scene bouton'
    'present'               = '--scene present'
    'non-envoyes'           = '--scene non-envoyes'
    'saisie'                = '--scene saisie'
    'saisie-longue'         = '--scene saisie-longue'
    'reponse'               = '--scene reponse'
    'fichiers'              = '--scene fichiers'
    'messages-vocaux'       = '--scene messages-vocaux'
    'lecteur'               = '--scene lecteur'
    'lecteur-plein'         = '--scene lecteur-plein'
    'entete'                = '--scene entete'
    'entete-sujet'          = '--scene entete-sujet'
    'entete-fils'           = '--scene entete-fils'
    'entete-archives'       = '--scene entete-archives'
    'entete-cloche'         = '--scene entete-cloche'
    'menu-serveur'          = '--scene menu-serveur'
    'categories'            = '--scene categories'
    'categories-muets'      = '--scene categories-muets'
    'salon-vocal'           = '--scene salon-vocal'
    'mon-compte'            = '--scene mon-compte'
    'profil-edition'        = '--scene profil-edition'
    'profil-edition-serveur' = '--scene profil-edition-serveur'
    'fils'                  = '--scene fils'
    'fil'                   = '--scene fil'
    'forum'                 = '--scene forum'
    'emojis'                = '--scene emojis'
    'gifs'                  = '--scene gifs'
    'gifs-recherche'        = '--scene gifs-recherche'
    'gifs-favoris'          = '--scene gifs-favoris'
    'super-reaction'        = '--scene super-reaction'
    'sondages'              = '--scene sondages'
    'applications'          = '--scene applications'
    'modale'                = '--scene modale'
    'recherche'             = '--scene recherche'
    'selecteur-rapide'      = '--scene selecteur'
    'boite'                 = '--scene boite'
    'raccourcis'            = '--scene raccourcis'
    'accessibilite'         = '--scene accessibilite'
    'activite'              = '--scene activite'
    'serveurs'              = '--scene serveurs'
    'serveurs-ajouter'      = '--scene serveurs-ajouter'
    'serveurs-inviter'      = '--scene serveurs-inviter'
    'serveurs-invitations'  = '--scene serveurs-invitations'
    'evenements'            = '--scene evenements'
    'evenement-creer'       = '--scene evenement-creer'
    'bienvenue'             = '--scene bienvenue'
    'integration'           = '--scene integration'
    'moderation-membre'     = '--scene moderation-membre'
    'membre-roles'          = '--scene membre-roles'
    'salons-creer'          = '--scene salons-creer'
    'vocal'                 = '--scene vocal'
    'vocal-codes'           = '--scene vocal-codes'
    'appel'                 = '--scene appel'
    'appel-direct'          = '--scene appel-direct'
    'appel-partage'         = '--scene appel-partage'
    'sonnerie'              = '--scene sonnerie'
    'conference'            = '--scene conference'
    'sons'                  = '--scene sons'
    'partage'               = '--scene partage'
    'ps-vue'                = '--scene parametres-serveur'
    'ps-webhooks'           = '--scene parametres-serveur-webhooks'
    'ps-integrations'       = '--scene parametres-serveur-integrations'
    'ps-boosts'             = '--scene parametres-serveur-boosts'
    'ps-emojis'             = '--scene parametres-emojis'
    'ps-autocollants'       = '--scene parametres-autocollants'
    'ps-sons'               = '--scene parametres-sons'
    'ps-salons'             = '--scene salons-serveur'
    'ps-roles'              = '--scene roles'
    'ps-role'               = '--scene role'
    'ps-role-permissions'   = '--scene role-permissions'
    'ps-membres'            = '--scene membres'
    'ps-securite'           = '--scene moderation-securite'
    'ps-automod'            = '--scene moderation-automod'
    'ps-journal'            = '--scene moderation-journal'
    'ps-bannissements'      = '--scene moderation-bannissements'
    'ps-integration'        = '--scene parametres-integration'
    'ps-bienvenue'          = '--scene parametres-bienvenue'
    'salon-vue'             = '--scene salons'
    'salon-permissions'     = '--scene salons-permissions'
    'connexion'             = '--view login'
    'connexion-erreur'      = '--view login --notice'
    'connexion-qr'          = '--view login --scene qr'
    'connexion-qr-lu'       = '--view login --scene qr-lu'
    'comptes'               = '--view comptes'
    'comptes-selecteur'     = '--view selecteur'
    'comptes-connexion'     = '--view connexion'
    'comptes-retrait'       = '--view retrait'
    'comptes-ancien'        = '--view ancien-fichier'
    'comptes-migration'     = '--view migration'
    'comptes-doublon'       = '--view doublon'
    'comptes-remplacement'  = '--view remplacement'
    'hors-ligne'            = '--view connexion --scene hors-ligne'
    # Fondations visuelles (lot V1) : icônes, composants partagés, fenêtre modale, barre de titre.
    'v1-icones'             = '--gallery icones'
    'v1-composants'         = '--gallery composants --pointer 60,470'
    'v1-fenetre'            = '--gallery fenetre'
    'v1-barre-fermer'       = '--view guild --pointer 1265,10'
    'v1-barre-windows'      = '--view guild --set mise_en_page.titlebar=windows'
}
foreach ($p in 'compte', 'profil', 'statut', 'confidentialite', 'applications', 'appareils', 'comptes-lies', 'facturation', 'apparence', 'texte', 'notifications', 'langue', 'dossiers', 'ferrocord', 'accessibilite', 'streamer', 'avances', 'donnees', 'activite', 'a-propos', 'raccourcis', 'voix') {
    $shots["pu-$p"] = "--settings $p"
}
# Scènes ajoutées à l'aperçu depuis : la capture porte le nom de la scène.
foreach ($s in 'alerte-automod', 'amis-ajouter', 'apparence-editeur', 'bloques', 'boutique', 'decouverte', 'demandes', 'densites', 'densites-spacieuse', 'detache', 'discret', 'ecriture', 'ecriture-deux', 'ecriture-plusieurs', 'effet', 'effet-fixe', 'enregistrement', 'enregistrement-demarrer', 'enregistres', 'etat-compte', 'export', 'export-lecture', 'fichier-details', 'fil-membres', 'fond-image', 'galerie', 'glyphes', 'incrustation', 'inventaire', 'journal', 'lien-externe', 'limite-age', 'mention-role', 'menu-plus', 'menu-utilisateur', 'mica', 'mica-acrylique', 'mon-compte-clair', 'mon-compte-sobre', 'mon-compte-statut', 'mon-compte-statut-perso', 'mots-cles', 'mots-cles-reglages', 'notifs-serveur', 'onglets', 'palette', 'panneau', 'panneau-menu', 'parcourir', 'parametres-serveur-elagage', 'parametres-serveur-modele', 'parametres-serveur-widget', 'parures', 'parures-animees', 'plantage', 'polices-nom', 'pour-vous', 'profil-carte', 'profil-edition-sans-nitro', 'profil-edition-style', 'programme', 'programmer', 'programmes', 'pseudo-ami', 'quetes', 'recherche-locale', 'repertoire', 'resumes', 'rich-presence', 'rnnoise', 'sans-rail', 'scinde', 'suggestions', 'theme-degrade', 'theme-import', 'traduction', 'traduction-reglages', 'transitions', 'verification', 'videos', 'videos-liens', 'zoom') {
    if (-not $shots.Contains($s)) { $shots[$s] = "--scene $s" }
}

$todo = @()
foreach ($size in $Sizes) {
    $wh = $size -replace 'x', ','
    foreach ($theme in @('') + $Themes) {
        $sub = if ($theme) { "$size-$theme" } else { $size }
        $dir = Join-Path $Out $sub
        New-Item -ItemType Directory -Force $dir | Out-Null
        foreach ($name in $shots.Keys) {
            if ($Only.Count -gt 0 -and -not ($Only | Where-Object { $name -like $_ })) { continue }
            $extra = $shots[$name]
            # Une capture qui fixe déjà son thème n'est prise qu'une fois, dans le dossier par défaut.
            if ($theme -and $extra -match '--theme') { continue }
            if ($theme) { $extra += " --theme $theme" }
            $png = Join-Path $dir "$name.png"
            if ($extra -notmatch '--pointer') { $extra += ' --hover-none' }
            $todo += [pscustomobject]@{ Name = "$sub/$name"; Png = $png; Args = "$extra --lang fr --size $wh --screenshot `"$png`"" }
        }
    }
}

$running = @()
$failed = @()
function Wait-One {
    $script:running = @($script:running | Where-Object {
        if ($_.Proc.HasExited) {
            if (-not (Test-Path $_.Job.Png)) { $script:failed += $_.Job.Name }
            $false
        } elseif (((Get-Date) - $_.Start).TotalSeconds -gt 60) {
            # Ne tue que le processus lancé ici.
            try { $_.Proc.Kill() } catch {}
            $script:failed += $_.Job.Name
            $false
        } else { $true }
    })
}
foreach ($job in $todo) {
    while ($running.Count -ge $Jobs) { Start-Sleep -Milliseconds 100; Wait-One }
    if (Test-Path $job.Png) { Remove-Item $job.Png -Confirm:$false }
    $p = Start-Process -FilePath $Exe -ArgumentList $job.Args -PassThru -WindowStyle Normal -RedirectStandardError ([IO.Path]::GetTempFileName())
    $running += [pscustomobject]@{ Proc = $p; Job = $job; Start = Get-Date }
}
while ($running.Count -gt 0) { Start-Sleep -Milliseconds 100; Wait-One }
"$($todo.Count - $failed.Count) captures sur $($todo.Count) dans $Out"
if ($failed.Count -gt 0) { "échecs : $($failed -join ', ')" }
