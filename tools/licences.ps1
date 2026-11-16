# Textes complets des licences des dépendances de Ferrocord, pour
# Paramètres > À propos. Sans dépendance : `cargo metadata`, puis les fichiers
# de licence de chaque paquet (LICENSE*, COPYING*, NOTICE*…), plus ceux des
# bibliothèques C compilées dans l'exécutable (libopus, OpenH264, aws-lc).
# Les polices embarquées (assets\fonts) y sont ajoutées avec leur licence.
# Seules les dépendances normales pour Windows comptent (ni tests, ni outils
# de compilation : ils ne sont pas dans l'exécutable).
#
# Résultat : src/install/licences.json.gz (JSON compressé, textes identiques
# partagés). À relancer après tout changement de Cargo.lock :
#
#   powershell -ExecutionPolicy Bypass -File tools\licences.ps1

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$out = Join-Path $root 'src\install\licences.json.gz'

Push-Location $root
try {
    $json = cargo metadata --format-version 1 --filter-platform x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw 'cargo metadata a échoué' }
} finally {
    Pop-Location
}
$meta = ($json -join "`n") | ConvertFrom-Json

$byId = @{}
foreach ($p in $meta.packages) { $byId[$p.id] = $p }
$nodes = @{}
foreach ($n in $meta.resolve.nodes) { $nodes[$n.id] = $n }

# Parcours depuis Ferrocord, par les dépendances normales seulement.
$seen = @{}
$queue = New-Object System.Collections.Queue
$queue.Enqueue($meta.resolve.root)
while ($queue.Count -gt 0) {
    $id = $queue.Dequeue()
    if ($seen.ContainsKey($id)) { continue }
    $seen[$id] = $true
    foreach ($d in $nodes[$id].deps) {
        $normal = $false
        foreach ($k in $d.dep_kinds) { if ($null -eq $k.kind) { $normal = $true } }
        if ($normal) { $queue.Enqueue($d.pkg) }
    }
}

# Fichiers de licence des bibliothèques C, dans les sources des paquets *-sys.
$extra = @{
    'opusic-sys'    = @('opus\COPYING')
    'openh264-sys2' = @('upstream\LICENSE')
}

function Read-Text([string]$path) {
    $t = [IO.File]::ReadAllText($path)
    return ($t -replace "`r`n", "`n").Trim()
}

# Textes types (paquet sans fichier) : MIT et Apache-2.0 pris dans un autre
# paquet, les autres dans tools/licences/<identifiant SPDX>.txt.
$generic = @{}
foreach ($f in Get-ChildItem -LiteralPath (Join-Path $PSScriptRoot 'licences') -Filter '*.txt') { $generic[$f.BaseName] = Read-Text $f.FullName }
foreach ($id in $seen.Keys) {
    $dir = Split-Path -Parent $byId[$id].manifest_path
    foreach ($f in Get-ChildItem -LiteralPath $dir -File) {
        if ($f.Name -match '^LICEN[CS]E[-_]MIT(\.md|\.txt)?$' -and -not $generic['MIT']) { $generic['MIT'] = Read-Text $f.FullName }
        if ($f.Name -match '^LICEN[CS]E[-_]APACHE(2)?(\.md|\.txt)?$' -and -not $generic['Apache-2.0']) { $generic['Apache-2.0'] = Read-Text $f.FullName }
    }
}

$texts = New-Object System.Collections.Generic.List[string]
$index = @{}
function Add-Text([string]$t) {
    if (-not $index.ContainsKey($t)) {
        $index[$t] = $texts.Count
        $texts.Add($t)
    }
    return $index[$t]
}

$packages = New-Object System.Collections.Generic.List[object]
$missing = @()
foreach ($id in ($seen.Keys | Sort-Object { $byId[$_].name }, { $byId[$_].version })) {
    $p = $byId[$id]
    if ($id -eq $meta.resolve.root) { continue }
    $dir = Split-Path -Parent $p.manifest_path
    $ids = New-Object System.Collections.Generic.List[int]
    $files = Get-ChildItem -LiteralPath $dir -File | Where-Object { $_.Name -match '^(LICEN[CS]E|COPYING|NOTICE|COPYRIGHT|UNLICENSE)' } | Sort-Object Name
    foreach ($f in $files) { $ids.Add((Add-Text (Read-Text $f.FullName))) }
    foreach ($rel in $extra[$p.name]) {
        $path = Join-Path $dir $rel
        if (Test-Path -LiteralPath $path) { $ids.Add((Add-Text ("Bibliothèque C incluse ($rel) :`n`n" + (Read-Text $path)))) }
    }
    if ($files.Count -eq 0) {
        $license = [string]$p.license
        $done = $false
        foreach ($spdx in @($generic.Keys | Sort-Object)) {
            if ($license -match [regex]::Escape($spdx) -and $generic[$spdx]) {
                $authors = ($p.authors -join ', ')
                $ids.Add((Add-Text ("Texte type de la licence $spdx (le paquet n'en fournit pas de copie) ; auteurs : $authors.`n`n" + $generic[$spdx])))
                $done = $true
            }
        }
        if (-not $done) { $missing += "$($p.name) $($p.version) ($license)" }
    }
    $packages.Add([ordered]@{ nom = $p.name; version = $p.version; licence = [string]$p.license; textes = [int[]]$ids.ToArray() })
}

# Polices embarquées (assets\fonts) : leur licence est montrée avec celles des paquets.
$fonts = @(
    @{ nom = 'Figtree (police)'; version = '2.002'; licence = 'OFL-1.1'; fichier = 'assets\fonts\OFL.txt' }
    # Polices des styles de nom (assets\fonts\noms) : nom, auteur d'après le fichier de licence.
    @{ nom = 'Bangers (police, The Bangers Project Authors)'; version = '2.100'; licence = 'OFL-1.1'; fichier = 'assets\fonts\noms\bangers\OFL.txt' }
    @{ nom = 'BioRhyme (police, The BioRhyme Project Authors)'; version = '1.600'; licence = 'OFL-1.1'; fichier = 'assets\fonts\noms\biorhyme\OFL.txt' }
    @{ nom = 'Cherry Bomb One (police, The Cherry Bomb Project Authors)'; version = '4.100'; licence = 'OFL-1.1'; fichier = 'assets\fonts\noms\cherrybombone\OFL.txt' }
    @{ nom = 'Chicle (police, Angel Koziupa et Alejandro Paul)'; version = '1.000'; licence = 'OFL-1.1'; fichier = 'assets\fonts\noms\chicle\OFL.txt' }
    @{ nom = 'Compagnon (police, Velvetyne Type Foundry)'; version = '1.000'; licence = 'OFL-1.1'; fichier = 'assets\fonts\noms\compagnon\LICENSE.txt' }
    @{ nom = 'Kalam (police, Indian Type Foundry)'; version = '2.001'; licence = 'OFL-1.1'; fichier = 'assets\fonts\noms\kalam\OFL.txt' }
    @{ nom = 'MuseoModerno (police, The MuseoModerno Project Authors)'; version = '1.003'; licence = 'OFL-1.1'; fichier = 'assets\fonts\noms\museomoderno\OFL.txt' }
    @{ nom = 'New Rocker (police, Pablo Impallari, Brenda Gallo et Rodrigo Fuenzalida)'; version = '1.000'; licence = 'OFL-1.1'; fichier = 'assets\fonts\noms\newrocker\OFL.txt' }
    @{ nom = 'Orbitron (police, The Orbitron Project Authors)'; version = '2.001'; licence = 'OFL-1.1'; fichier = 'assets\fonts\noms\orbitron\OFL.txt' }
    @{ nom = 'Pixelify Sans (police, The Pixelify Sans Project Authors)'; version = '1.000'; licence = 'OFL-1.1'; fichier = 'assets\fonts\noms\pixelifysans\OFL.txt' }
    @{ nom = 'Playpen Sans (police, The Playpen Sans Project Authors)'; version = '2.000'; licence = 'OFL-1.1'; fichier = 'assets\fonts\noms\playpensans\OFL.txt' }
    @{ nom = 'Ribes (police, Collletttivo)'; version = '2.100'; licence = 'OFL-1.1'; fichier = 'assets\fonts\noms\ribes\LICENSE.txt' }
    @{ nom = 'Sinistre (police, Collletttivo)'; version = '69.420'; licence = 'OFL-1.1'; fichier = 'assets\fonts\noms\sinistre\LICENSE.txt' }
    @{ nom = 'Zilla Slab (police, The Mozilla Foundation)'; version = '1.1'; licence = 'OFL-1.1'; fichier = 'assets\fonts\noms\zillaslab\OFL.txt' }
    # Bibliothèques ANGLE embarquées (assets\angle, lot « gl ») : rendu par Direct3D 11.
    @{ nom = 'ANGLE (libEGL.dll et libGLESv2.dll, The ANGLE Project Authors)'; version = '2.1.28055'; licence = 'BSD-3-Clause'; fichier = 'assets\angle\LICENSE.txt' }
    # Bibliothèques tierces liées dans ces deux fichiers : mentions extraites de LICENSES.chromium.html,
    # dans l'archive Electron dont ils viennent (assets\angle\LISEZMOI.md).
    @{ nom = 'ANGLE, bibliothèque liée : Abseil'; version = '2.1.28055'; licence = 'Apache-2.0'; fichier = 'assets\angle\tiers\abseil.txt' }
    @{ nom = 'ANGLE, bibliothèque liée : compiler-rt (LLVM)'; version = '2.1.28055'; licence = 'Apache-2.0 WITH LLVM-exception'; fichier = 'assets\angle\tiers\compiler-rt.txt' }
    @{ nom = 'ANGLE, bibliothèque liée : EGL-Registry (Khronos)'; version = '2.1.28055'; licence = 'Apache-2.0'; fichier = 'assets\angle\tiers\egl-registry.txt' }
    @{ nom = 'ANGLE, bibliothèque liée : en-têtes Khronos'; version = '2.1.28055'; licence = 'MIT'; fichier = 'assets\angle\tiers\khronos-header-files.txt' }
    @{ nom = 'ANGLE, bibliothèque liée : libc++ (LLVM)'; version = '2.1.28055'; licence = 'Apache-2.0 WITH LLVM-exception'; fichier = 'assets\angle\tiers\libcxx.txt' }
    @{ nom = 'ANGLE, bibliothèque liée : libc++abi (LLVM)'; version = '2.1.28055'; licence = 'Apache-2.0 WITH LLVM-exception'; fichier = 'assets\angle\tiers\libcxxabi.txt' }
    @{ nom = 'ANGLE, bibliothèque liée : llvm-libc'; version = '2.1.28055'; licence = 'Apache-2.0 WITH LLVM-exception'; fichier = 'assets\angle\tiers\llvm-libc.txt' }
    @{ nom = 'ANGLE, bibliothèque liée : murmur3'; version = '2.1.28055'; licence = 'Apache-2.0'; fichier = 'assets\angle\tiers\murmur3.txt' }
    @{ nom = 'ANGLE, bibliothèque liée : OpenGL-Registry (Khronos)'; version = '2.1.28055'; licence = 'Apache-2.0'; fichier = 'assets\angle\tiers\opengl-registry.txt' }
    @{ nom = 'ANGLE, bibliothèque liée : SMHasher'; version = '2.1.28055'; licence = 'MIT'; fichier = 'assets\angle\tiers\smhasher.txt' }
    @{ nom = 'ANGLE, bibliothèque liée : SPIRV-Headers'; version = '2.1.28055'; licence = 'MIT'; fichier = 'assets\angle\tiers\spirv-headers.txt' }
    @{ nom = 'ANGLE, bibliothèque liée : SPIRV-Tools'; version = '2.1.28055'; licence = 'Apache-2.0'; fichier = 'assets\angle\tiers\spirv-tools.txt' }
    @{ nom = 'ANGLE, bibliothèque liée : The Chromium Project (base)'; version = '2.1.28055'; licence = 'BSD-3-Clause'; fichier = 'assets\angle\tiers\the-chromium-project.txt' }
    @{ nom = 'ANGLE, bibliothèque liée : volk'; version = '2.1.28055'; licence = 'MIT'; fichier = 'assets\angle\tiers\volk-meta-loader-for-vulkan-api.txt' }
    @{ nom = 'ANGLE, bibliothèque liée : Vulkan-Headers'; version = '2.1.28055'; licence = 'Apache-2.0'; fichier = 'assets\angle\tiers\vulkan-api-headers.txt' }
    @{ nom = 'ANGLE, bibliothèque liée : Vulkan Deps'; version = '2.1.28055'; licence = 'BSD-3-Clause'; fichier = 'assets\angle\tiers\vulkan-deps.txt' }
    @{ nom = 'ANGLE, bibliothèque liée : VulkanMemoryAllocator'; version = '2.1.28055'; licence = 'MIT'; fichier = 'assets\angle\tiers\vulkanmemoryallocator.txt' }
    @{ nom = 'ANGLE, bibliothèque liée : xxHash'; version = '2.1.28055'; licence = 'BSD-2-Clause'; fichier = 'assets\angle\tiers\xxhash.txt' }
    @{ nom = 'ANGLE, bibliothèque liée : zlib (portions de Chromium)'; version = '2.1.28055'; licence = 'Zlib'; fichier = 'assets\angle\tiers\zlib-2.txt' }
    @{ nom = 'ANGLE, bibliothèque liée : zlib'; version = '2.1.28055'; licence = 'Zlib'; fichier = 'assets\angle\tiers\zlib.txt' }
)
foreach ($f in $fonts) {
    $id = Add-Text (Read-Text (Join-Path $root $f.fichier))
    $packages.Add([ordered]@{ nom = $f.nom; version = $f.version; licence = $f.licence; textes = [int[]]@($id) })
}

$doc = [ordered]@{ paquets = $packages.ToArray(); textes = $texts.ToArray() }
$bytes = [Text.UTF8Encoding]::new($false).GetBytes(($doc | ConvertTo-Json -Depth 5 -Compress))
$ms = New-Object IO.MemoryStream
$gz = New-Object IO.Compression.GZipStream($ms, [IO.Compression.CompressionLevel]::Optimal)
$gz.Write($bytes, 0, $bytes.Length)
$gz.Close()
[IO.File]::WriteAllBytes($out, $ms.ToArray())

Write-Output ("{0} paquets, {1} textes distincts, {2:N0} octets ({3:N0} compressés) -> {4}" -f $packages.Count, $texts.Count, $bytes.Length, (Get-Item $out).Length, $out)
if ($missing.Count -gt 0) {
    Write-Output 'Sans texte de licence (à compléter à la main) :'
    $missing | ForEach-Object { Write-Output "  $_" }
}
