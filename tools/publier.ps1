# Fabrique une version publiable de Ferrocord dans dist\<version>\, SANS RIEN
# ENVOYER : ni `git push`, ni `gh`, ni aucune requête. La commande de
# publication est affichée à la fin ; c'est vous qui la lancez.
#
#   powershell -ExecutionPolicy Bypass -File tools\publier.ps1 -ClePrivee <fichier hors du dépôt>
#   powershell -ExecutionPolicy Bypass -File tools\publier.ps1 -CreerCle -ClePrivee <fichier hors du dépôt>
#
# La clé privée peut aussi être nommée par la variable FERROCORD_CLE_PRIVEE.
# Elle n'est jamais affichée, jamais copiée, et refusée si elle se trouve
# dans un dépôt git. Marche à suivre complète : docs\distribution.md.
#
# Ce que fait une fabrication :
#   1. vérifie que l'arbre git est propre, que la clé privée est bien celle
#      dont l'exécutable intègre la clé publique, que la version est nouvelle ;
#   2. compile `cargo build --release` ;
#   3. écrit dans dist\<version>\ : Ferrocord-<version>-x64.exe, le manifeste
#      signé ferrocord-manifeste.json, SHA256SUMS, NOTES.md (git log depuis la
#      dernière étiquette) ;
#   4. affiche la commande `gh release create …`.
param(
    # Fichier de la clé privée (PKCS#8 Ed25519), hors de tout dépôt git.
    [string]$ClePrivee = $env:FERROCORD_CLE_PRIVEE,
    # Crée la paire de clés (une seule fois) au lieu de fabriquer une version.
    [switch]$CreerCle,
    # propriétaire/dépôt GitHub ; par défaut, celui de src\install\update\depot.txt.
    [string]$Depot = '',
    # Dossier de sortie ; par défaut dist\ à la racine du dépôt (ignoré par git).
    [string]$Sortie = '',
    # Essais seulement : exécutable déjà compilé (pas de `cargo build --release`).
    [string]$Exe = '',
    # Essais seulement : fichier de clé publique à comparer et à écrire, à la
    # place de src\install\update\cle_publique.txt.
    [string]$ClePublique = '',
    # Essais seulement : ne pas exiger un arbre git propre.
    [switch]$ArbreNonPropre,
    # Clippy et tests viennent de passer sur ce même commit : ne pas les relancer.
    [switch]$TestsDejaPasses,
    # Notes de version rédigées (fichier Markdown, UTF-8, hors du dépôt ou ignoré
    # par git) : elles remplacent le `git log` dans NOTES.md et dans le manifeste
    # signé, donc dans le bandeau « nouveautés » de l'application.
    [string]$Notes = ''
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$racine = Split-Path -Parent $PSScriptRoot
$sansBom = New-Object Text.UTF8Encoding $false
if (-not $ClePublique) { $ClePublique = Join-Path $racine 'src\install\update\cle_publique.txt' }
$manifesteSigner = Join-Path $PSScriptRoot 'signer\Cargo.toml'

function Echec([string]$message) {
    Write-Host "ERREUR : $message" -ForegroundColor Red
    exit 1
}

# Lance l'outil de signature ; rend sa sortie standard (jamais la clé privée,
# que l'outil n'affiche pas).
function Signer {
    $sortie = & cargo run --quiet --release --manifest-path $manifesteSigner -- @args
    if ($LASTEXITCODE -ne 0) { Echec "tools\signer a échoué (voir le message ci-dessus)." }
    return $sortie
}

# Première ligne utile d'un fichier dont les commentaires commencent par #.
function LigneUtile([string]$fichier) {
    if (-not (Test-Path -LiteralPath $fichier -PathType Leaf)) { return '' }
    $ligne = Get-Content -LiteralPath $fichier -Encoding UTF8 | ForEach-Object { $_.Trim() } | Where-Object { $_ -and -not $_.StartsWith('#') } | Select-Object -First 1
    if ($ligne) { return $ligne } else { return '' }
}

# --- La clé privée : nommée, et hors de tout dépôt git ----------------------
if (-not $ClePrivee) {
    Echec "nommez le fichier de la clé privée : -ClePrivee <fichier>, ou la variable FERROCORD_CLE_PRIVEE. Il doit être HORS du dépôt."
}
$ClePrivee = [IO.Path]::GetFullPath($ClePrivee)
$dossier = Split-Path -Parent $ClePrivee
while ($dossier) {
    if (Test-Path -LiteralPath (Join-Path $dossier '.git')) {
        Echec "la clé privée serait dans un dépôt git ($dossier). Gardez-la ailleurs (clé USB chiffrée, dossier personnel hors projet)."
    }
    $parent = Split-Path -Parent $dossier
    if ($parent -eq $dossier) { break }
    $dossier = $parent
}

# --- Création de la paire de clés (commande à part, explicite) ---------------
if ($CreerCle) {
    if (Test-Path -LiteralPath $ClePrivee) { Echec "ce fichier existe déjà : rien n'est remplacé. Pour renouveler la clé, voir docs\distribution.md." }
    $dossierCle = Split-Path -Parent $ClePrivee
    if (-not (Test-Path -LiteralPath $dossierCle -PathType Container)) { Echec "le dossier de la clé privée n'existe pas : $dossierCle" }
    Signer cle --privee $ClePrivee --publique $ClePublique | Out-Null
    Write-Host ''
    Write-Host 'Paire de clés créée.' -ForegroundColor Green
    Write-Host "  Clé privée  : $ClePrivee"
    Write-Host '                Sauvegardez-la sur un support à part. Qui la possède peut signer des mises à jour.'
    Write-Host "  Clé publique : $ClePublique"
    Write-Host '                À commiter : elle est intégrée à l''exécutable (git add, git commit).'
    exit 0
}

if (-not (Test-Path -LiteralPath $ClePrivee -PathType Leaf)) {
    Echec "clé privée introuvable : $ClePrivee. Pour en créer une : tools\publier.ps1 -CreerCle -ClePrivee <fichier>."
}

# --- Arbre git propre --------------------------------------------------------
Push-Location $racine
try {
    $sale = & git status --porcelain
    if ($LASTEXITCODE -ne 0) { Echec "git status a échoué : ce dossier n'est pas un dépôt git ?" }
    if ($sale -and -not $ArbreNonPropre) {
        Echec "l'arbre git n'est pas propre. Commitez ou retirez ces changements, puis recommencez :`n$($sale -join "`n")"
    }
    $commit = (& git rev-parse HEAD).Trim()

    # --- Version, dépôt, clé publique ---------------------------------------
    $m = Select-String -LiteralPath (Join-Path $racine 'Cargo.toml') -Pattern '^version\s*=\s*"(\d+\.\d+\.\d+)"' | Select-Object -First 1
    if (-not $m) { Echec "version introuvable dans Cargo.toml (attendue : majeur.mineur.correctif)." }
    $version = $m.Matches[0].Groups[1].Value
    $etiquette = "v$version"
    if (& git tag --list $etiquette) {
        Echec "l'étiquette $etiquette existe déjà : augmentez la version dans Cargo.toml (une version publiée ne se refait pas)."
    }

    $depotIntegre = LigneUtile (Join-Path $racine 'src\install\update\depot.txt')
    if (-not $Depot) { $Depot = $depotIntegre }
    if ($Depot -notmatch '^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$') {
        Echec "dépôt GitHub inconnu. Écrivez propriétaire/dépôt dans src\install\update\depot.txt (et commitez), ou passez -Depot."
    }
    if ($depotIntegre -ne $Depot) {
        Write-Host "ATTENTION : src\install\update\depot.txt ne nomme pas $Depot. L'exécutable fabriqué ne cherchera pas ses mises à jour dans ce dépôt." -ForegroundColor Yellow
    }

    $publiqueIntegree = LigneUtile $ClePublique
    if (-not $publiqueIntegree) {
        Echec "aucune clé publique dans $ClePublique : l'exécutable refuserait toute mise à jour. Créez la paire : tools\publier.ps1 -CreerCle -ClePrivee <fichier>, puis commitez la clé publique."
    }
    $publique = (Signer publique --privee $ClePrivee | Select-Object -Last 1).Trim()
    if ($publique -ne $publiqueIntegree.ToLower()) {
        Echec "cette clé privée n'est pas celle de la clé publique intégrée ($ClePublique). Les installations refuseraient le manifeste."
    }

    # --- Compilation ----------------------------------------------------------
    if (-not $Exe) {
        # Rien ne part chez les utilisateurs sans clippy ni tests (audit 0.1.33).
        if (-not $TestsDejaPasses) {
            $cibleAvant = $env:CARGO_TARGET_DIR
            try {
                Write-Host "Clippy (avertissements refusés)…"
                $env:CARGO_TARGET_DIR = Join-Path $racine 'target\clippy'
                & cargo clippy --all-targets --locked -- -D warnings
                if ($LASTEXITCODE -ne 0) { Echec "clippy signale des défauts : rien n'est publié." }
                Write-Host "Tests…"
                $env:CARGO_TARGET_DIR = Join-Path $racine 'target\tests'
                & cargo test --locked
                if ($LASTEXITCODE -ne 0) { Echec "des tests échouent : rien n'est publié." }
            } finally {
                $env:CARGO_TARGET_DIR = $cibleAvant
            }
        }
        Write-Host "Compilation de Ferrocord $version (cargo build --release --locked)…"
        & cargo build --release --locked
        if ($LASTEXITCODE -ne 0) { Echec "la compilation a échoué." }
        $cible = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $racine 'target' }
        $Exe = Join-Path $cible 'release\ferrocord.exe'
    }
    if (-not (Test-Path -LiteralPath $Exe -PathType Leaf)) { Echec "exécutable introuvable : $Exe" }

    # --- Dossier de la version -------------------------------------------------
    if (-not $Sortie) { $Sortie = Join-Path $racine 'dist' }
    $dist = Join-Path ([IO.Path]::GetFullPath($Sortie)) $version
    New-Item -ItemType Directory -Force -Path $dist | Out-Null
    $nomExe = "Ferrocord-$version-x64.exe"
    $nomManifeste = 'ferrocord-manifeste.json'
    $cheminExe = Join-Path $dist $nomExe
    $cheminManifeste = Join-Path $dist $nomManifeste
    $cheminNotes = Join-Path $dist 'NOTES.md'
    $cheminSommes = Join-Path $dist 'SHA256SUMS'
    Copy-Item -LiteralPath $Exe -Destination $cheminExe -Force

    # --- Notes de version : git log depuis la dernière étiquette ---------------
    $derniere = & git tag --list 'v*' --sort=-v:refname --merged HEAD | Select-Object -First 1
    if (-not $derniere) { $derniere = '' }
    $plage = if ($derniere) { "$derniere..HEAD" } else { 'HEAD' }
    # Première version (aucune étiquette) : les derniers changements seulement.
    $lignes = if ($derniere) { @(& git -c i18n.logOutputEncoding=utf-8 log --no-merges --pretty=format:'- %s' $plage) } else { @(& git -c i18n.logOutputEncoding=utf-8 log --no-merges --pretty=format:'- %s' -n 30 $plage) }
    if (-not $lignes) { $lignes = @('- Corrections.') }
    $titre = if ($derniere) { "Ferrocord $version (depuis $derniere)" } else { "Ferrocord $version" }
    if ($Notes) {
        if (-not (Test-Path -LiteralPath $Notes -PathType Leaf)) { Echec "Fichier de notes introuvable : $Notes" }
        # Rédigées : reprises telles quelles ; le manifeste les reçoit sans leur titre.
        $lignes = @([IO.File]::ReadAllLines((Resolve-Path -LiteralPath $Notes).Path, [Text.Encoding]::UTF8) | ForEach-Object { $_.TrimEnd() })
        [IO.File]::WriteAllText($cheminNotes, ($lignes -join "`n").Trim() + "`n", $sansBom)
        $lignes = @($lignes | Where-Object { $_ -notmatch '^# ' })
        $court = (($lignes -join "`n") -replace '(?m)^## ', '').Trim()
    } else {
        [IO.File]::WriteAllText($cheminNotes, "# $titre`n`n" + ($lignes -join "`n") + "`n", $sansBom)
        # Dans le manifeste (affiché dans Paramètres > Ferrocord) : les premières lignes seulement.
        $court = ($lignes | Select-Object -First 12) -join "`n"
    }
    if ($court.Length -gt 1500) { $court = $court.Substring(0, 1500) }
    $notesCourtes = Join-Path $dist 'notes-manifeste.tmp'
    [IO.File]::WriteAllText($notesCourtes, $court, $sansBom)

    # --- Manifeste signé ---------------------------------------------------------
    $url = "https://github.com/$Depot/releases/download/$etiquette/$nomExe"
    try {
        Signer signe --privee $ClePrivee --exe $cheminExe --version $version --url $url --notes-fichier $notesCourtes --sortie $cheminManifeste | Out-Null
    } finally {
        Remove-Item -LiteralPath $notesCourtes -ErrorAction SilentlyContinue
    }

    # --- SHA256SUMS (format de sha256sum : empreinte, deux espaces, nom) --------
    $sommes = foreach ($f in @($cheminExe, $cheminManifeste)) {
        '{0}  {1}' -f (Get-FileHash -LiteralPath $f -Algorithm SHA256).Hash.ToLower(), (Split-Path -Leaf $f)
    }
    [IO.File]::WriteAllText($cheminSommes, ($sommes -join "`n") + "`n", $sansBom)

    # --- Bilan, et la commande à lancer soi-même ---------------------------------
    $taille = [Math]::Round((Get-Item -LiteralPath $cheminExe).Length / 1MB, 1)
    Write-Host ''
    Write-Host "Ferrocord $version est prêt dans $dist" -ForegroundColor Green
    Write-Host "  $nomExe  ($taille Mo)"
    Write-Host "  $nomManifeste  (signé ; clé publique $($publique.Substring(0, 16))…)"
    Write-Host '  SHA256SUMS'
    Write-Host '  NOTES.md  (relisez-les : elles seront publiques)'
    Write-Host ''
    Write-Host 'Rien n''a été envoyé. Pour publier (dépôt PUBLIC, version ni brouillon ni préversion) :' -ForegroundColor Cyan
    Write-Host ''
    Write-Host "  git push origin HEAD"
    Write-Host "  gh release create $etiquette --repo $Depot --target $commit --title `"Ferrocord $version`" --notes-file `"$cheminNotes`" `"$cheminExe`" `"$cheminManifeste`" `"$cheminSommes`""
    Write-Host ''
    Write-Host "Les installations verront la version à : https://github.com/$Depot/releases/latest/download/$nomManifeste"
} finally {
    Pop-Location
}
