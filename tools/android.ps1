<#
.SYNOPSIS
    Fabrique l'APK Android de Ferrocord (arm64-v8a), signée par la clé de
    débogage locale.

.DESCRIPTION
    Outils attendus (voir README, section Android) : JDK 17, SDK Android
    (platform-tools, une plateforme, build-tools, NDK, cmake du SDK pour
    Ninja), la cible Rust aarch64-linux-android et cargo-ndk.

    Étapes : bibliothèque native (cargo ndk, crate android/), ressources et
    manifeste (aapt2), ajout de la bibliothèque, alignement (zipalign),
    signature (apksigner). La clé de débogage est celle d'Android Studio,
    `%USERPROFILE%\.android\debug.keystore`, créée si elle manque : jamais
    dans le dépôt.

    Sortie : <dossier cible>\android\ferrocord-debug.apk.

.PARAMETER Check
    Seulement vérifier que la crate compile pour Android (cargo ndk check).
#>
param([switch]$Check)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

function Fail($msg) { Write-Host "ERREUR : $msg" -ForegroundColor Red; exit 1 }
function Run($exe, [string[]]$argv) {
    & $exe @argv
    if ($LASTEXITCODE -ne 0) { Fail "$exe a échoué (code $LASTEXITCODE)" }
}
# Dernier sous-dossier par numéro de version (28.2.13676358 > 27.3.…).
function Latest($dir, $filter = '*') {
    if (-not (Test-Path $dir)) { return $null }
    Get-ChildItem $dir -Directory -Filter $filter |
        Where-Object { ($_.Name -replace '^android-', '') -match '^\d+(\.\d+)*' } |
        Sort-Object { $v = ($_.Name -replace '^android-', '') -replace '^(\d+(\.\d+)*).*$', '$1'; if ($v -notmatch '\.') { $v += '.0' }; [version]$v } |
        Select-Object -Last 1
}

# --- Outils -----------------------------------------------------------------
$sdk = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } else { Join-Path $env:LOCALAPPDATA 'Android\Sdk' }
if (-not (Test-Path $sdk)) { Fail "SDK Android introuvable ($sdk)" }
$ndk = Latest (Join-Path $sdk 'ndk')
$bt = Latest (Join-Path $sdk 'build-tools')
$plat = Latest (Join-Path $sdk 'platforms') 'android-*'
$cmake = Latest (Join-Path $sdk 'cmake')
if (-not $ndk) { Fail 'NDK absent : sdkmanager "ndk;<version>"' }
if (-not $cmake) { Fail 'CMake du SDK absent (il fournit Ninja) : sdkmanager "cmake;<version>"' }
if (-not $env:JAVA_HOME) {
    $jdk = Get-ChildItem 'C:\Program Files\Microsoft' -Directory -Filter 'jdk-17*' -ErrorAction SilentlyContinue | Select-Object -Last 1
    if ($jdk) { $env:JAVA_HOME = $jdk.FullName }
}
$env:ANDROID_HOME = $sdk
$env:ANDROID_NDK_HOME = $ndk.FullName
# Les bibliothèques C (opus, aws-lc) passent par CMake : Ninja, pas Visual Studio.
$env:CMAKE_GENERATOR = 'Ninja'
$env:Path = (Join-Path $cmake.FullName 'bin') + ';' + $env:Path
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
$out = Join-Path $target 'android'
New-Item -ItemType Directory -Force $out | Out-Null
Set-Location $root

# Android 8 (API 26) au moins : AAudio, canaux de notification.
$minSdk = 26
if ($Check) {
    Run cargo @('ndk', '-t', 'arm64-v8a', '-P', "$minSdk", 'check', '-p', 'ferrocord-android')
    exit 0
}

# --- Bibliothèque native ----------------------------------------------------
$libs = Join-Path $out 'lib'
Remove-Item -Recurse -Force $libs -ErrorAction SilentlyContinue
# --link-libcxx-shared : OpenH264 (C++) a besoin de libc++_shared.so, qui
# n'est pas une bibliothèque du système : elle doit voyager dans l'APK.
Run cargo @('ndk', '-t', 'arm64-v8a', '-P', "$minSdk", '--link-libcxx-shared', '-o', $libs, 'build', '--release', '-p', 'ferrocord-android')
$abiDir = Join-Path $libs 'arm64-v8a'
$so = Join-Path $abiDir 'libferrocord_android.so'
if (-not (Test-Path $so)) { Fail "bibliothèque absente : $so" }

# Chaque dépendance (NEEDED) doit être dans l'APK ou parmi les bibliothèques
# publiques du NDK présentes dès l'API 26 ; sinon le chargement échoue avant android_main.
$llvm = Join-Path $ndk.FullName 'toolchains\llvm\prebuilt\windows-x86_64\bin'
# libc++_shared.so du NDK garde ses symboles de débogage : inutiles sur le téléphone.
Run (Join-Path $llvm 'llvm-strip.exe') @('--strip-unneeded', (Join-Path $abiDir 'libc++_shared.so'))
$readelf = Join-Path $llvm 'llvm-readelf.exe'
$public = @('libc.so', 'libm.so', 'libdl.so', 'liblog.so', 'libandroid.so', 'libaaudio.so', 'libEGL.so', 'libGLESv2.so', 'libGLESv3.so', 'libjnigraphics.so', 'libmediandk.so', 'libz.so', 'libOpenSLES.so', 'libvulkan.so')
$shipped = Get-ChildItem $abiDir -Filter '*.so' | ForEach-Object Name
foreach ($lib in $shipped) {
    $needed = & $readelf -d (Join-Path $abiDir $lib) | Select-String 'NEEDED' | ForEach-Object { if ($_.Line -match '\[(.+)\]') { $Matches[1] } }
    foreach ($n in $needed) {
        if ($public -notcontains $n -and $shipped -notcontains $n) { Fail "$lib a besoin de $n, absente de l'APK" }
    }
}

# --- Paquet -----------------------------------------------------------------
if (-not $bt -or -not $plat) { Fail 'build-tools ou plateforme absents : sdkmanager "build-tools;<v>" "platforms;android-<n>"' }
$aapt2 = Join-Path $bt.FullName 'aapt2.exe'
$zipalign = Join-Path $bt.FullName 'zipalign.exe'
$apksigner = Join-Path $bt.FullName 'apksigner.bat'
$jar = Join-Path $plat.FullName 'android.jar'
# API 34 : à partir de 35, Android impose l'affichage bord à bord, et
# l'interface passerait sous la barre d'état et la barre de navigation.
$targetSdk = 34
$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$parts = $version.Split('.') | ForEach-Object { [int]$_ }
$code = $parts[0] * 10000 + $parts[1] * 100 + $parts[2]

$res = Join-Path $out 'res.zip'
$base = Join-Path $out 'base.apk'
$aligned = Join-Path $out 'aligned.apk'
$apk = Join-Path $out 'ferrocord-debug.apk'
Remove-Item -Force $res, $base, $aligned, $apk -ErrorAction SilentlyContinue
Run $aapt2 @('compile', '--dir', (Join-Path $root 'android\res'), '-o', $res)
Run $aapt2 @('link', '-o', $base, '-I', $jar, '--manifest', (Join-Path $root 'android\AndroidManifest.xml'),
    '--min-sdk-version', "$minSdk", '--target-sdk-version', "$targetSdk",
    '--version-code', "$code", '--version-name', $version, $res)

# Les bibliothèques, compressées : le manifeste demande leur extraction à l'installation.
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
$zip = [System.IO.Compression.ZipFile]::Open($base, 'Update')
try {
    foreach ($lib in $shipped) {
        [void][System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, (Join-Path $abiDir $lib), "lib/arm64-v8a/$lib", 'Optimal')
    }
} finally { $zip.Dispose() }
Run $zipalign @('-P', '16', '-f', '4', $base, $aligned)

# --- Signature de débogage --------------------------------------------------
$keystore = Join-Path $env:USERPROFILE '.android\debug.keystore'
if (-not (Test-Path $keystore)) {
    New-Item -ItemType Directory -Force (Split-Path $keystore) | Out-Null
    $keytool = if ($env:JAVA_HOME) { Join-Path $env:JAVA_HOME 'bin\keytool.exe' } else { 'keytool' }
    # Mots de passe publics et conventionnels de la clé de débogage d'Android.
    Run $keytool @('-genkeypair', '-keystore', $keystore, '-storepass', 'android', '-alias', 'androiddebugkey',
        '-keypass', 'android', '-keyalg', 'RSA', '-keysize', '2048', '-validity', '10000',
        '-dname', 'CN=Android Debug,O=Android,C=US')
}
Run $apksigner @('sign', '--ks', $keystore, '--ks-pass', 'pass:android', '--ks-key-alias', 'androiddebugkey', '--out', $apk, $aligned)
Run $apksigner @('verify', $apk)
Remove-Item -Force $base, $aligned, $res -ErrorAction SilentlyContinue

$mb = [math]::Round((Get-Item $apk).Length / 1MB, 2)
Write-Host "APK : $apk ($mb Mo)"
