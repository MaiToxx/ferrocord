<#
.SYNOPSIS
    Compare Ferrocord et le client Discord officiel : démarrage, repos,
    activité, appel vocal facultatif, taille sur disque.

.DESCRIPTION
    Lance chaque application l'une après l'autre, mesure de l'extérieur
    (processus, mémoire, processeur, fils, handles, GPU) et écrit un tableau
    comparatif en Markdown dans mesures\resultats-<date>.md.

    Le script n'envoie rien sur le réseau, ne lit aucun jeton et ne touche
    à aucune donnée des applications (seulement la taille des dossiers).
    Il ne ferme que les processus qu'il a lui-même lancés, après avoir
    demandé. Mode d'emploi complet : docs\mesures.md.

    Pour Ferrocord, il active le journal interne (FERROCORD_PERF=1) : temps
    de chaque image et de chaque changement de salon, dans un fichier local
    qui ne contient que des durées.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File tools\mesures.ps1

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File tools\mesures.ps1 -Vocal

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File tools\mesures.ps1 -Essai -Apercu target\release\examples\preview.exe
#>
[CmdletBinding()]
param(
    # Exécutable de Ferrocord (défaut : target\release\ferrocord.exe du dépôt).
    [string]$Ferrocord,
    # Exécutable du client officiel (défaut : la version la plus récente de %LOCALAPPDATA%\Discord\app-*\Discord.exe).
    [string]$Discord,
    # Applications mesurées, dans cet ordre.
    [ValidateSet('Ferrocord', 'Discord')]
    [string[]]$Applications = @('Ferrocord', 'Discord'),
    # Attente après le « prêt », avant la mesure au repos (s).
    [int]$Stabilisation = 30,
    # Mesure au repos (s).
    [int]$Repos = 60,
    # Mesure pendant que vous changez de salon et faites défiler (s).
    [int]$Activite = 30,
    # Mesure pendant un appel vocal (facultatif).
    [switch]$Vocal,
    [int]$DureeVocal = 60,
    # « Prêt » : processeur moyen sur 2 s sous ce seuil (% d'un cœur).
    [double]$SeuilCalme = 5,
    # Attente maximale de la fenêtre et du calme au démarrage (s).
    [int]$DelaiMax = 120,
    # Dossier des résultats (défaut : mesures\ à la racine du dépôt).
    [string]$Sortie,
    # Vérification du script sur l'aperçu hors ligne de Ferrocord, lancé deux
    # fois (aucune connexion) : durées courtes, aucune question, fermeture automatique.
    [switch]$Essai,
    # Exécutable de l'aperçu pour -Essai (défaut : target\release\examples\preview.exe).
    [string]$Apercu
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2

$Racine = Split-Path -Parent $PSScriptRoot
if (-not $Sortie) { $Sortie = Join-Path $Racine 'mesures' }
$Coeurs = [Environment]::ProcessorCount
$Invariant = [Globalization.CultureInfo]::InvariantCulture

if ($Essai) {
    $Stabilisation = [Math]::Min($Stabilisation, 3)
    $Repos = [Math]::Min($Repos, 5)
    $Activite = [Math]::Min($Activite, 5)
    $DureeVocal = [Math]::Min($DureeVocal, 5)
    $DelaiMax = [Math]::Min($DelaiMax, 60)
}

# --- Outils -----------------------------------------------------------------

function Write-Etape([string]$Texte) {
    Write-Host ''
    Write-Host "== $Texte" -ForegroundColor Cyan
}

function Read-Oui([string]$Question, [bool]$Defaut = $true) {
    if ($Essai) { return $Defaut }
    $suffixe = if ($Defaut) { '(O/n)' } else { '(o/N)' }
    $r = Read-Host "$Question $suffixe"
    if ([string]::IsNullOrWhiteSpace($r)) { return $Defaut }
    return $r.Trim().ToLower().StartsWith('o')
}

function Wait-Entree([string]$Texte) {
    if ($Essai) { return }
    [void](Read-Host $Texte)
}

function Get-Taille([string]$Chemin) {
    if (-not $Chemin -or -not (Test-Path -LiteralPath $Chemin)) { return $null }
    $item = Get-Item -LiteralPath $Chemin -Force
    if (-not $item.PSIsContainer) { return $item.Length / 1MB }
    $s = (Get-ChildItem -LiteralPath $Chemin -Recurse -Force -File -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum
    if ($null -eq $s) { return 0 }
    return $s / 1MB
}

function Get-Centile([double[]]$Valeurs, [double]$P) {
    if (-not $Valeurs -or $Valeurs.Count -eq 0) { return $null }
    $t = @($Valeurs | Sort-Object)
    return [double]$t[[int][Math]::Round(($t.Count - 1) * $P)]
}

function Get-Moyenne($Valeurs) {
    $v = @($Valeurs | Where-Object { $null -ne $_ })
    if ($v.Count -eq 0) { return $null }
    return ($v | Measure-Object -Average).Average
}

function Get-Maximum($Valeurs) {
    $v = @($Valeurs | Where-Object { $null -ne $_ })
    if ($v.Count -eq 0) { return $null }
    return ($v | Measure-Object -Maximum).Maximum
}

# --- Applications -----------------------------------------------------------

function New-Application([string]$Nom, [string]$Exe, [string]$Prefixe, [string[]]$Arguments, [bool]$Journal, [double]$Programme, [double]$Donnees) {
    [pscustomobject]@{
        Nom       = $Nom
        Exe       = $Exe
        # Processus comptés : ceux dont l'exécutable est sous ce chemin.
        Prefixe   = $Prefixe
        Processus = [IO.Path]::GetFileNameWithoutExtension($Exe)
        Arguments = $Arguments
        Journal   = $Journal
        Programme = $Programme
        Donnees   = $Donnees
    }
}

function Find-Ferrocord {
    $exe = if ($Ferrocord) { $Ferrocord } else { Join-Path $Racine 'target\release\ferrocord.exe' }
    if (-not (Test-Path -LiteralPath $exe)) {
        Write-Warning "Ferrocord introuvable ($exe). Compilez-le (cargo build --release) ou passez -Ferrocord <chemin>."
        return $null
    }
    $exe = (Resolve-Path -LiteralPath $exe).Path
    $donnees = 0
    foreach ($d in @((Join-Path $env:APPDATA 'ferrocord'), (Join-Path $env:LOCALAPPDATA 'ferrocord'))) {
        $t = Get-Taille $d
        if ($t) { $donnees += $t }
    }
    New-Application 'Ferrocord' $exe $exe @() $true (Get-Taille $exe) $donnees
}

function Find-Discord {
    $exe = $Discord
    if (-not $exe) {
        $base = Join-Path $env:LOCALAPPDATA 'Discord'
        $dossiers = @(Get-ChildItem -LiteralPath $base -Directory -Filter 'app-*' -ErrorAction SilentlyContinue |
            Sort-Object { try { [version]($_.Name.Substring(4)) } catch { [version]'0.0' } } -Descending)
        if ($dossiers.Count -gt 0) { $exe = Join-Path $dossiers[0].FullName 'Discord.exe' }
    }
    if (-not $exe -or -not (Test-Path -LiteralPath $exe)) {
        Write-Warning "Discord introuvable. Passez -Discord <chemin de Discord.exe>."
        return $null
    }
    $exe = (Resolve-Path -LiteralPath $exe).Path
    # Installation entière (tous les app-*, Update.exe, packages) : une mise à
    # jour peut relancer Discord depuis un autre dossier app-*.
    $installation = Split-Path -Parent (Split-Path -Parent $exe)
    New-Application 'Discord' $exe $installation @() $false (Get-Taille $installation) (Get-Taille (Join-Path $env:APPDATA 'discord'))
}

function Find-Apercu([string]$Nom) {
    $candidats = @()
    if ($Apercu) { $candidats += $Apercu }
    $candidats += Join-Path $Racine 'target\release\examples\preview.exe'
    if ($env:CARGO_TARGET_DIR) { $candidats += Join-Path $env:CARGO_TARGET_DIR 'release\examples\preview.exe' }
    foreach ($c in $candidats) {
        if (Test-Path -LiteralPath $c) {
            $exe = (Resolve-Path -LiteralPath $c).Path
            return New-Application $Nom $exe $exe @() $false (Get-Taille $exe) 0
        }
    }
    throw "Aperçu introuvable : cargo build --release --example preview, ou -Apercu <chemin>."
}

function Get-AppProcesses($App) {
    @(Get-Process -Name $App.Processus -ErrorAction SilentlyContinue | Where-Object {
            $p = $null
            try { $p = $_.Path } catch { }
            $p -and $p.StartsWith($App.Prefixe, [StringComparison]::OrdinalIgnoreCase)
        })
}

# Processus lancés par le script : la racine et ses descendants.
function Get-Lances([int]$Id) {
    $tous = @(Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId)
    $ids = New-Object 'System.Collections.Generic.HashSet[int]'
    if (Get-Process -Id $Id -ErrorAction SilentlyContinue) { [void]$ids.Add($Id) }
    $file = New-Object 'System.Collections.Generic.Queue[int]'
    $file.Enqueue($Id)
    while ($file.Count -gt 0) {
        $parent = $file.Dequeue()
        foreach ($p in $tous) {
            if ($p.ParentProcessId -eq $parent -and $ids.Add([int]$p.ProcessId)) { $file.Enqueue([int]$p.ProcessId) }
        }
    }
    return @($ids)
}

# --- Échantillons -----------------------------------------------------------

function New-Echantillon($Procs, $Chrono) {
    $cpu = @{}
    foreach ($p in $Procs) {
        try { $cpu[$p.Id] = $p.TotalProcessorTime.TotalMilliseconds } catch { }
    }
    [pscustomobject]@{ T = $Chrono.Elapsed.TotalMilliseconds; Cpu = $cpu; Procs = $Procs }
}

# Processeur entre deux échantillons, en % d'un cœur.
function Get-Processeur($A, $B) {
    $d = 0.0
    foreach ($k in $B.Cpu.Keys) {
        if ($A.Cpu.ContainsKey($k)) { $d += [Math]::Max(0, $B.Cpu[$k] - $A.Cpu[$k]) } else { $d += $B.Cpu[$k] }
    }
    $dt = $B.T - $A.T
    if ($dt -le 0) { return 0 }
    return $d / $dt * 100
}

# Utilisation du GPU (classe de compteurs non traduite, contrairement à Get-Counter).
function Start-Gpu([int[]]$Ids, [int]$Secondes) {
    if (-not $Ids -or $Ids.Count -eq 0) { return $null }
    Start-Job -ArgumentList (, $Ids), $Secondes -ScriptBlock {
        param($Ids, $Secondes)
        $fin = (Get-Date).AddSeconds($Secondes)
        $valeurs = @()
        while ((Get-Date) -lt $fin) {
            $lignes = @(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine -ErrorAction SilentlyContinue)
            if ($lignes.Count -eq 0) { return 'indisponible' }
            $parType = @{}
            foreach ($l in $lignes) {
                if ($l.Name -match '^pid_(\d+)_.*engtype_(.+)$' -and $Ids -contains [int]$Matches[1]) {
                    $parType[$Matches[2]] = [double]$parType[$Matches[2]] + [double]$l.UtilizationPercentage
                }
            }
            # Comme le Gestionnaire des tâches : le moteur le plus occupé.
            $max = 0.0
            foreach ($v in $parType.Values) { if ($v -gt $max) { $max = $v } }
            $valeurs += $max
            Start-Sleep -Milliseconds 1000
        }
        $valeurs
    }
}

function Stop-Gpu($Job) {
    if (-not $Job) { return $null }
    [void](Wait-Job $Job -Timeout 30)
    $r = @(Receive-Job $Job -ErrorAction SilentlyContinue)
    Remove-Job $Job -Force
    if ($r.Count -eq 0 -or $r[0] -eq 'indisponible') { return $null }
    return @($r | ForEach-Object { [double]$_ })
}

# Mesure de `Secondes` secondes, un échantillon par seconde.
function Measure-Phase($App, [int]$Secondes, $Chrono) {
    $procs = @(Get-AppProcesses $App)
    $gpu = Start-Gpu @($procs | ForEach-Object { $_.Id }) $Secondes
    $debut = $Chrono.Elapsed.TotalSeconds
    $prec = New-Echantillon $procs $Chrono
    $lignes = @()
    while ($Chrono.Elapsed.TotalSeconds - $debut -lt $Secondes) {
        Start-Sleep -Milliseconds 1000
        $procs = @(Get-AppProcesses $App)
        if ($procs.Count -eq 0) {
            Write-Warning "$($App.Nom) n'a plus de processus : mesure arrêtée."
            break
        }
        $cur = New-Echantillon $procs $Chrono
        $lignes += [pscustomobject]@{
            Cpu     = Get-Processeur $prec $cur
            Privee  = ($procs | Measure-Object PrivateMemorySize64 -Sum).Sum / 1MB
            Travail = ($procs | Measure-Object WorkingSet64 -Sum).Sum / 1MB
            N       = $procs.Count
            Fils    = ($procs | ForEach-Object { $_.Threads.Count } | Measure-Object -Sum).Sum
            Handles = ($procs | Measure-Object HandleCount -Sum).Sum
        }
        $prec = $cur
    }
    $g = Stop-Gpu $gpu
    [pscustomobject]@{
        Debut      = $debut
        Fin        = $Chrono.Elapsed.TotalSeconds
        CpuMoy     = Get-Moyenne ($lignes | ForEach-Object { $_.Cpu })
        CpuMax     = Get-Maximum ($lignes | ForEach-Object { $_.Cpu })
        Privee     = Get-Moyenne ($lignes | ForEach-Object { $_.Privee })
        PriveeMax  = Get-Maximum ($lignes | ForEach-Object { $_.Privee })
        Travail    = Get-Moyenne ($lignes | ForEach-Object { $_.Travail })
        Processus  = Get-Maximum ($lignes | ForEach-Object { $_.N })
        Fils       = Get-Moyenne ($lignes | ForEach-Object { $_.Fils })
        Handles    = Get-Moyenne ($lignes | ForEach-Object { $_.Handles })
        GpuMoy     = if ($g) { Get-Moyenne $g } else { $null }
        GpuMax     = if ($g) { Get-Maximum $g } else { $null }
    }
}

# Démarrage : première fenêtre visible, puis processeur au calme (« prêt »).
function Measure-Demarrage($App, $Chrono) {
    $fenetre = $null
    $pret = $null
    $hist = New-Object System.Collections.ArrayList
    while ($Chrono.Elapsed.TotalSeconds -lt $DelaiMax) {
        $procs = @(Get-AppProcesses $App)
        if (-not $fenetre) {
            foreach ($p in $procs) {
                if ($p.MainWindowHandle -ne [IntPtr]::Zero) { $fenetre = $Chrono.Elapsed.TotalSeconds; break }
            }
        }
        if ($procs.Count -gt 0) { [void]$hist.Add((New-Echantillon $procs $Chrono)) }
        if ($fenetre -and $hist.Count -gt 1) {
            $dernier = $hist[$hist.Count - 1]
            $a = $null
            for ($i = $hist.Count - 2; $i -ge 0; $i--) {
                if ($dernier.T - $hist[$i].T -ge 2000) { $a = $hist[$i]; break }
            }
            if ($a -and ($a.T / 1000) -ge $fenetre -and (Get-Processeur $a $dernier) -lt $SeuilCalme) {
                $pret = $a.T / 1000
                break
            }
        }
        Start-Sleep -Milliseconds $(if ($fenetre) { 250 } else { 50 })
    }
    if (-not $fenetre) { Write-Warning "$($App.Nom) : aucune fenêtre visible après $DelaiMax s (démarrage réduit dans la zone de notification ?)." }
    elseif (-not $pret) { Write-Warning "$($App.Nom) : processeur jamais au calme en $DelaiMax s." }
    [pscustomobject]@{ Fenetre = $fenetre; Pret = $pret }
}

# Journal interne de Ferrocord (FERROCORD_PERF) : t_ms,type,ms.
function Read-Journal([string]$Chemin, $Phases) {
    if (-not (Test-Path -LiteralPath $Chemin)) { return $null }
    $lignes = @(Import-Csv -LiteralPath $Chemin | ForEach-Object {
            [pscustomobject]@{ T = [double]::Parse($_.t_ms, $Invariant) / 1000; Type = $_.type; Ms = [double]::Parse($_.ms, $Invariant) }
        })
    # Âge du processus au début de `run` (lot C8) : le journal compte depuis
    # `run`, le script depuis le lancement ; ce décalage les aligne.
    $age = @($lignes | Where-Object { $_.Type -eq 'etape.processus_avant_run' })
    if ($age.Count -gt 0) {
        $d = $age[0].Ms / 1000
        foreach ($l in $lignes) { if ($l.Type -ne 'etape.processus_avant_run') { $l.T += $d } }
    }
    $r = [ordered]@{}
    $premiere = @($lignes | Where-Object { $_.Type -eq 'premiere_image' })
    if ($premiere.Count -gt 0) { $r['journal_premiere_image_ms'] = $premiere[0].Ms }
    $repos = $Phases['repos']
    if ($repos) {
        $n = @($lignes | Where-Object { $_.Type -eq 'image' -and $_.T -ge $repos.Debut -and $_.T -lt $repos.Fin }).Count
        $r['journal_images_repos'] = $n
        $r['journal_images_repos_par_s'] = $n / [Math]::Max(1, $repos.Fin - $repos.Debut)
    }
    $act = $Phases['activite']
    if ($act) {
        $images = [double[]]@($lignes | Where-Object { $_.Type -eq 'image' -and $_.T -ge $act.Debut -and $_.T -lt $act.Fin } | ForEach-Object { $_.Ms })
        $r['journal_images_activite'] = $images.Count
        $r['journal_image_mediane_ms'] = Get-Centile $images 0.5
        $r['journal_image_p95_ms'] = Get-Centile $images 0.95
        $r['journal_image_max_ms'] = Get-Centile $images 1
    }
    $salons = [double[]]@($lignes | Where-Object { $_.Type -eq 'changement_de_salon' } | ForEach-Object { $_.Ms })
    $r['journal_changements'] = $salons.Count
    $r['journal_changement_mediane_ms'] = Get-Centile $salons 0.5
    $r['journal_changement_p95_ms'] = Get-Centile $salons 0.95
    $r['journal_changement_max_ms'] = Get-Centile $salons 1
    # Premier salon affiché au démarrage (lot C9) : à part, ce n'est pas un changement.
    $premier = @($lignes | Where-Object { $_.Type -eq 'premier_salon' })
    if ($premier.Count -gt 0) { $r['journal_premier_salon_ms'] = $premier[0].Ms }
    # Démarrage (lot C9) : temps processeur de chaque fil et entrées reçues,
    # du lancement à la fin des 2 s de calme (ce qui retarde « prêt »).
    $dem = $Phases['demarrage']
    if ($dem) {
        $fils = [ordered]@{}
        $lignes | Where-Object { $_.Type.StartsWith('fil.') -and $_.T -lt $dem.Fin } | Group-Object Type |
            ForEach-Object { [pscustomobject]@{ Nom = $_.Name; Ms = ($_.Group | Measure-Object Ms -Sum).Sum } } |
            Sort-Object Ms -Descending | Select-Object -First 12 | ForEach-Object { $fils[$_.Nom] = $_.Ms }
        if ($fils.Count -gt 0) { $r['journal_fils_demarrage'] = $fils }
        $entrees = [ordered]@{}
        $lignes | Where-Object { ($_.Type.StartsWith('entree.') -or $_.Type -eq 'image') -and $_.T -lt $dem.Fin } | Group-Object Type |
            Sort-Object Count -Descending | ForEach-Object { $entrees[$_.Name] = $_.Count }
        if ($entrees.Count -gt 0) { $r['journal_entrees_demarrage'] = $entrees }
    }
    # Étapes du démarrage (ms depuis le début de `run`), dans l'ordre.
    $etapes = [ordered]@{}
    foreach ($l in $lignes) {
        if ($l.Type.StartsWith('etape.') -and -not $etapes.Contains($l.Type)) { $etapes[$l.Type.Substring(6)] = $l.Ms }
    }
    if ($etapes.Count -gt 0) { $r['journal_etapes'] = $etapes }
    # Ce qui a demandé les images du repos, et ce que la passerelle a fait de ses événements.
    if ($repos) {
        $causes = [ordered]@{}
        $lignes | Where-Object { ($_.Type.StartsWith('cause.') -or $_.Type.StartsWith('passerelle.') -or $_.Type.StartsWith('entree.')) -and $_.T -ge $repos.Debut -and $_.T -lt $repos.Fin } |
            Group-Object Type | Sort-Object Count -Descending | Select-Object -First 15 | ForEach-Object { $causes[$_.Name] = $_.Count }
        $r['journal_causes_repos'] = $causes
    }
    return $r
}

# Ferme, après accord, la racine lancée et ses descendants, rien d'autre.
function Close-Lances($App, [int]$Id) {
    $ids = @(Get-Lances $Id)
    if ($ids.Count -eq 0) {
        $restants = @(Get-AppProcesses $App)
        if ($restants.Count -gt 0) {
            Write-Host "$($App.Nom) s'est relancé hors du processus lancé par le script ($($restants.Count) processus) : fermez-le vous-même."
        }
        return
    }
    if (-not (Read-Oui "Fermer les $($ids.Count) processus de $($App.Nom) lancés par ce script ?")) {
        Write-Host "$($App.Nom) laissé ouvert. Fermez-le avant la mesure suivante pour ne pas la fausser."
        return
    }
    $racine = Get-Process -Id $Id -ErrorAction SilentlyContinue
    if ($racine) {
        [void]$racine.CloseMainWindow()
        [void]$racine.WaitForExit(5000)
    }
    foreach ($i in $ids) {
        $p = Get-Process -Id $i -ErrorAction SilentlyContinue
        if ($p -and -not $p.HasExited) { Stop-Process -Id $i -Force -ErrorAction SilentlyContinue }
    }
}

# --- Mesure d'une application -----------------------------------------------

function Measure-Application($App) {
    Write-Etape "$($App.Nom) : $($App.Exe)"
    $r = [ordered]@{ disque_programme_mo = $App.Programme; disque_donnees_mo = $App.Donnees }
    if (@(Get-AppProcesses $App).Count -gt 0) {
        Write-Warning "$($App.Nom) est déjà ouvert : le démarrage ne serait pas mesuré, et le script ne doit fermer que ce qu'il a lancé."
        Write-Host "Fermez-le complètement (zone de notification comprise), puis relancez le script. Seule la taille sur disque est relevée."
        return $r
    }
    $journal = $null
    if ($App.Journal) {
        $journal = Join-Path $Sortie ("perf-ferrocord-{0}.csv" -f $Horodatage)
        $env:FERROCORD_PERF = '1'
        $env:FERROCORD_PERF_FICHIER = $journal
    }
    Write-Host "Lancement. Ne touchez à rien jusqu'à la consigne suivante."
    $chrono = [Diagnostics.Stopwatch]::StartNew()
    $params = @{ FilePath = $App.Exe; PassThru = $true; WorkingDirectory = (Split-Path -Parent $App.Exe) }
    if ($App.Arguments.Count -gt 0) { $params['ArgumentList'] = $App.Arguments }
    $proc = Start-Process @params
    Remove-Item Env:FERROCORD_PERF, Env:FERROCORD_PERF_FICHIER -ErrorAction SilentlyContinue
    $phases = @{}
    try {
        $d = Measure-Demarrage $App $chrono
        $r['demarrage_fenetre_s'] = $d.Fenetre
        $r['demarrage_pret_s'] = $d.Pret
        $phases['demarrage'] = [pscustomobject]@{ Debut = 0; Fin = $(if ($d.Pret) { $d.Pret + 2 } else { $DelaiMax }) }
        if (-not $Essai -and $App.Nom -eq 'Discord') {
            Wait-Entree "Si Discord demande de se connecter ou installe une mise à jour, attendez la fin, puis appuyez sur Entrée"
        }
        Write-Host "Stabilisation : $Stabilisation s sans rien toucher."
        Start-Sleep -Seconds $Stabilisation
        Write-Host "Repos : $Repos s de mesure. Ne touchez à rien (souris hors de la fenêtre)."
        $p = Measure-Phase $App $Repos $chrono
        $phases['repos'] = $p
        $r['repos_privee_mo'] = $p.Privee; $r['repos_travail_mo'] = $p.Travail
        $r['repos_cpu_moy'] = $p.CpuMoy; $r['repos_cpu_max'] = $p.CpuMax
        $r['repos_processus'] = $p.Processus; $r['repos_fils'] = $p.Fils; $r['repos_handles'] = $p.Handles
        $r['repos_gpu_moy'] = $p.GpuMoy; $r['repos_gpu_max'] = $p.GpuMax

        Wait-Entree "Activité : pendant $Activite s, changez de salon et faites défiler sans arrêt. Appuyez sur Entrée pour commencer"
        $p = Measure-Phase $App $Activite $chrono
        $phases['activite'] = $p
        if (-not $Essai) { [Console]::Beep(880, 200) }
        Write-Host "Activité terminée, vous pouvez arrêter."
        $r['activite_cpu_moy'] = $p.CpuMoy; $r['activite_cpu_max'] = $p.CpuMax
        $r['activite_privee_mo'] = $p.PriveeMax; $r['activite_travail_mo'] = $p.Travail
        $r['activite_gpu_moy'] = $p.GpuMoy

        if ($Vocal) {
            Wait-Entree "Appel vocal : rejoignez un salon vocal (ou un appel), puis appuyez sur Entrée ; restez $DureeVocal s"
            $p = Measure-Phase $App $DureeVocal $chrono
            $phases['vocal'] = $p
            Write-Host "Mesure de l'appel terminée, vous pouvez le quitter."
            $r['vocal_cpu_moy'] = $p.CpuMoy; $r['vocal_cpu_max'] = $p.CpuMax
            $r['vocal_privee_mo'] = $p.Privee; $r['vocal_travail_mo'] = $p.Travail
        }
    }
    finally {
        Close-Lances $App $proc.Id
    }
    if ($journal) {
        Start-Sleep -Milliseconds 500
        $j = Read-Journal $journal $phases
        if ($j) { foreach ($k in $j.Keys) { $r[$k] = $j[$k] } }
        else { Write-Warning "Journal de Ferrocord absent ($journal) : exécutable trop ancien ?" }
    }
    return $r
}

# --- Rapport ----------------------------------------------------------------

$Lignes = @(
    @('Démarrage : première fenêtre visible (s)', 'demarrage_fenetre_s', 2),
    @('Démarrage : processeur au calme, « prêt » (s)', 'demarrage_pret_s', 2),
    @('Repos : mémoire privée (Mo, tous processus)', 'repos_privee_mo', 1),
    @('Repos : mémoire de travail (Mo, tous processus)', 'repos_travail_mo', 1),
    @("Repos : processeur moyen (% d'un cœur)", 'repos_cpu_moy', 2),
    @("Repos : processeur maximal sur 1 s (% d'un cœur)", 'repos_cpu_max', 2),
    @('Repos : processus', 'repos_processus', 0),
    @('Repos : fils (threads)', 'repos_fils', 0),
    @('Repos : handles', 'repos_handles', 0),
    @('Repos : GPU moyen (%)', 'repos_gpu_moy', 2),
    @('Repos : GPU maximal (%)', 'repos_gpu_max', 2),
    @("Activité : processeur moyen (% d'un cœur)", 'activite_cpu_moy', 1),
    @("Activité : processeur maximal sur 1 s (% d'un cœur)", 'activite_cpu_max', 1),
    @('Activité : mémoire privée maximale (Mo)', 'activite_privee_mo', 1),
    @('Activité : mémoire de travail (Mo)', 'activite_travail_mo', 1),
    @('Activité : GPU moyen (%)', 'activite_gpu_moy', 2),
    @("Appel vocal : processeur moyen (% d'un cœur)", 'vocal_cpu_moy', 1),
    @("Appel vocal : processeur maximal (% d'un cœur)", 'vocal_cpu_max', 1),
    @('Appel vocal : mémoire privée (Mo)', 'vocal_privee_mo', 1),
    @('Appel vocal : mémoire de travail (Mo)', 'vocal_travail_mo', 1),
    @('Disque : programme (Mo)', 'disque_programme_mo', 1),
    @('Disque : données et cache (Mo)', 'disque_donnees_mo', 1)
)

$LignesJournal = @(
    @('Première image construite, depuis le lancement (ms)', 'journal_premiere_image_ms', 1),
    @('Images dessinées pendant le repos', 'journal_images_repos', 0),
    @('Images par seconde au repos', 'journal_images_repos_par_s', 2),
    @("Images pendant l'activité", 'journal_images_activite', 0),
    @("Construction d'une image pendant l'activité, médiane (ms)", 'journal_image_mediane_ms', 3),
    @("Construction d'une image pendant l'activité, 95e centile (ms)", 'journal_image_p95_ms', 3),
    @("Construction d'une image pendant l'activité, maximum (ms)", 'journal_image_max_ms', 3),
    @('Changements de salon relevés', 'journal_changements', 0),
    @('Changement de salon, médiane (ms)', 'journal_changement_mediane_ms', 3),
    @('Changement de salon, 95e centile (ms)', 'journal_changement_p95_ms', 3),
    @('Changement de salon, maximum (ms)', 'journal_changement_max_ms', 3),
    @('Premier salon affiché au démarrage, image (ms)', 'journal_premier_salon_ms', 3)
)

function Format-Valeur($V, [int]$Decimales) {
    if ($null -eq $V) { return 'n/d' }
    return ([double]$V).ToString("N$Decimales")
}

function Format-Rapport($A, $B) {
    if ($null -eq $A -or $null -eq $B) { return 'n/d' }
    if ([double]$A -eq 0) { if ([double]$B -eq 0) { return '=' } else { return '∞' } }
    return (([double]$B) / ([double]$A)).ToString('N1') + ' ×'
}

function Write-Resultats($Noms, $Res, [string]$Fichier) {
    $a = $Noms[0]; $b = $Noms[1]
    $os = Get-CimInstance Win32_OperatingSystem
    $cpu = @(Get-CimInstance Win32_Processor)[0]
    $md = New-Object System.Collections.Generic.List[string]
    $titre = if ($Essai) { 'Mesures (ESSAI du script sur l''aperçu hors ligne)' } else { 'Mesures Ferrocord / Discord' }
    $md.Add("# $titre — $(Get-Date -Format 'yyyy-MM-dd HH:mm')")
    $md.Add('')
    $md.Add("Machine : $($os.Caption) $($os.Version), $($cpu.Name.Trim()), $Coeurs cœurs logiques, $([Math]::Round($os.TotalVisibleMemorySize / 1MB, 1)) Go de mémoire.")
    $md.Add("Processeur en % d'un cœur (100 % = un cœur entier ; le Gestionnaire des tâches divise par $Coeurs). Mémoire : somme de tous les processus de l'application.")
    $md.Add("Durées : stabilisation $Stabilisation s, repos $Repos s, activité $Activite s$(if ($Vocal) { ", appel $DureeVocal s" }). Seuil de calme : $SeuilCalme % d'un cœur sur 2 s.")
    $md.Add('')
    $md.Add("| Mesure | $a | $b | $b / $a |")
    $md.Add('|---|---:|---:|---:|')
    foreach ($l in $Lignes) {
        $va = if ($Res[$a]) { $Res[$a][$l[1]] } else { $null }
        $vb = if ($Res[$b]) { $Res[$b][$l[1]] } else { $null }
        if ($null -eq $va -and $null -eq $vb) { continue }
        $md.Add("| $($l[0]) | $(Format-Valeur $va $l[2]) | $(Format-Valeur $vb $l[2]) | $(Format-Rapport $va $vb) |")
    }
    $journal = if ($Res[$a]) { $Res[$a] } else { $null }
    if ($journal -and $journal.Contains('journal_changements')) {
        $md.Add('')
        $md.Add("## Journal interne de $a (FERROCORD_PERF)")
        $md.Add('')
        $md.Add('Mesuré par l''application elle-même ; Discord n''a pas d''équivalent, ces lignes ne se comparent pas.')
        $md.Add('')
        $md.Add('| Mesure | Valeur |')
        $md.Add('|---|---:|')
        foreach ($l in $LignesJournal) {
            if ($journal.Contains($l[1])) { $md.Add("| $($l[0]) | $(Format-Valeur $journal[$l[1]] $l[2]) |") }
        }
        if ($journal.Contains('journal_etapes')) {
            $md.Add('')
            $md.Add('### Étapes du démarrage (ms depuis le début de `run`)')
            $md.Add('')
            $md.Add('| Étape | ms |')
            $md.Add('|---|---:|')
            foreach ($e in $journal['journal_etapes'].Keys) { $md.Add("| $e | $(Format-Valeur $journal['journal_etapes'][$e] 1) |") }
        }
        if ($journal.Contains('journal_fils_demarrage')) {
            $md.Add('')
            $md.Add('### Démarrage : temps processeur par fil, jusqu''à la fin du calme (ms)')
            $md.Add('')
            $md.Add('`principal` : fil d''interface ; `tokio_rt_worker` : réseau ; `ferrocord_historique` : historique hors ligne ; `ferrocord_perf_fils` : ce relevé lui-même ; `sans_nom.<module>` : fil sans nom, d''après le module où il démarre (`ntdll` : réserve de fils de Windows ; `atio6axx`, `nvoglv64`… : pilote OpenGL).')
            $md.Add('')
            $md.Add('| Fil | ms |')
            $md.Add('|---|---:|')
            foreach ($f in $journal['journal_fils_demarrage'].Keys) { $md.Add("| $f | $(Format-Valeur $journal['journal_fils_demarrage'][$f] 1) |") }
        }
        if ($journal.Contains('journal_entrees_demarrage')) {
            $md.Add('')
            $md.Add('### Démarrage : images et entrées reçues, jusqu''à la fin du calme')
            $md.Add('')
            $md.Add('`entree.souris` : la souris a bougé sur la fenêtre (une image chacune) ; sans `entree.*`, les images viennent du programme.')
            $md.Add('')
            $md.Add('| Type | Nombre |')
            $md.Add('|---|---:|')
            foreach ($e in $journal['journal_entrees_demarrage'].Keys) { $md.Add("| $e | $($journal['journal_entrees_demarrage'][$e]) |") }
        }
        if ($journal.Contains('journal_causes_repos')) {
            $md.Add('')
            $md.Add('### Causes des images et événements de la passerelle pendant le repos')
            $md.Add('')
            $md.Add('`cause.*` : code qui a demandé une image ; `passerelle.reveil.*` : événement qui a réveillé l''interface ; `passerelle.differe.*` : événement lu à l''image suivante, sans réveil ; `passerelle.ignore.*` : abandonné.')
            $md.Add('')
            $md.Add('| Type | Nombre |')
            $md.Add('|---|---:|')
            foreach ($c in $journal['journal_causes_repos'].Keys) { $md.Add("| $c | $($journal['journal_causes_repos'][$c]) |") }
        }
    }
    $md.Add('')
    $md.Add('Limites : voir docs/mesures.md (premier lancement et cache, mises à jour de Discord, compte et serveurs différents, mesure extérieure).')
    [IO.File]::WriteAllLines($Fichier, $md, (New-Object System.Text.UTF8Encoding($true)))
}

# --- Programme --------------------------------------------------------------

$Horodatage = Get-Date -Format 'yyyy-MM-dd_HHmm'
New-Item -ItemType Directory -Force -Path $Sortie | Out-Null

if ($Essai) {
    $apps = @((Find-Apercu 'Aperçu'), (Find-Apercu 'Aperçu (2e lancement)'))
}
else {
    $apps = @()
    foreach ($nom in $Applications) {
        $a = if ($nom -eq 'Ferrocord') { Find-Ferrocord } else { Find-Discord }
        if ($a) { $apps += $a }
    }
    if ($apps.Count -eq 0) { throw 'Aucune application à mesurer.' }
    $total = $apps.Count * ($Stabilisation + $Repos + $Activite + $(if ($Vocal) { $DureeVocal } else { 0 }) + 20)
    Write-Host "Mesure de : $(($apps | ForEach-Object { $_.Nom }) -join ', '). Durée : environ $([Math]::Ceiling($total / 60)) min."
    Write-Host 'Fermez les deux applications avant de commencer, et évitez toute autre tâche lourde pendant la mesure.'
    Wait-Entree 'Appuyez sur Entrée pour commencer'
}

$Resultats = @{}
foreach ($app in $apps) {
    $Resultats[$app.Nom] = Measure-Application $app
}

$noms = @('Ferrocord', 'Discord')
if ($Essai) { $noms = @($apps[0].Nom, $apps[1].Nom) }
$suffixe = if ($Essai) { '-essai' } else { '' }
$fichier = Join-Path $Sortie ("resultats-{0}{1}.md" -f $Horodatage, $suffixe)
Write-Resultats $noms $Resultats $fichier
Write-Etape "Résultats : $fichier"
Get-Content -LiteralPath $fichier -Encoding UTF8 | Write-Host
