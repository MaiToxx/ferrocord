# Installe Ferrocord pour l'utilisateur courant, sans droits d'administrateur
# ni dépendance. L'installation est faite par l'exécutable lui-même
# (`ferrocord.exe --installer`, src\install\setup.rs) ; ce script ne fait que
# le lancer sur l'exécutable compilé, pour qui travaille depuis les sources :
# - copie de ferrocord.exe dans %LOCALAPPDATA%\Programs\Ferrocord ;
# - raccourci dans le menu Démarrer (et sur le bureau avec -Bureau) ;
# - entrée dans « Applications installées » (HKCU) ; la désinstallation
#   (`ferrocord.exe --desinstaller`) garde vos réglages et votre cache, sauf
#   si vous demandez de les effacer ;
# - démarrage avec Windows avec -Demarrage.
#
#   powershell -ExecutionPolicy Bypass -File tools\installer.ps1 [-Exe <chemin de ferrocord.exe>] [-Bureau] [-Demarrage]
#
# Par défaut, l'exécutable est celui de `cargo build --release`.

param(
    [string]$Exe = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\release\ferrocord.exe'),
    [switch]$Bureau,
    [switch]$Demarrage
)

$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $Exe -PathType Leaf)) {
    Write-Error "Exécutable introuvable : $Exe (compilez d'abord avec cargo build --release, ou passez -Exe)."
}
$arguments = @('--installer', '--silencieux')
if ($Bureau) { $arguments += '--bureau' }
if ($Demarrage) { $arguments += '--demarrage' }
$p = Start-Process -FilePath $Exe -ArgumentList $arguments -Wait -PassThru
if ($p.ExitCode -ne 0) {
    Write-Error 'Installation impossible. Si Ferrocord est ouvert, quittez-le (menu de l''icône > Quitter), puis recommencez.'
}
$dir = Join-Path $env:LOCALAPPDATA 'Programs\Ferrocord'
Write-Output "Ferrocord installé dans $dir (menu Démarrer : Ferrocord)."
Write-Output 'Démarrage avec Windows, liens discord:// et mises à jour : Paramètres > Ferrocord.'
Write-Output 'Désinstaller : Paramètres de Windows > Applications installées > Ferrocord.'
