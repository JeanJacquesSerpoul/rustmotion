<#
.SYNOPSIS
    Construit RustMotion pour Windows : installateurs, exécutable portable et CLI.

.DESCRIPTION
    Produit dans release\windows (par défaut) :
      - RustMotion_<version>_x64_en-US.msi      installateur Windows Installer
      - RustMotion_<version>_x64-setup.exe      installateur NSIS
      - RustMotion_<version>_x64_portable.exe   application sans installation (WebView2 requis,
                                                présent d'office sur Windows 10 et 11)
      - rustmotion-cli-<version>-windows-x64.zip  la CLI, avec README et LICENSE

    La version est lue dans src-tauri\tauri.conf.json.
    Prérequis : Rust (MSVC), Trunk, tauri-cli v2. La cible wasm32 est ajoutée si elle manque.
    À l'exécution, RustMotion a besoin de Chrome ou Edge et de ffmpeg.

.PARAMETER Portable
    Exécutable portable seulement, sans installateurs (plus rapide).

.PARAMETER SkipCli
    Ne construit pas la CLI.

.PARAMETER Clean
    Supprime d'abord le frontend compilé (dist) et les installateurs précédents.

.PARAMETER OutDir
    Dossier de sortie (défaut : release\windows à la racine du projet). Il est vidé avant la copie.

.EXAMPLE
    .\scripts\build-windows.ps1

.EXAMPLE
    .\scripts\build-windows.ps1 -Portable -SkipCli
#>
[CmdletBinding()]
param(
    [switch]$Portable,
    [switch]$SkipCli,
    [switch]$Clean,
    [string]$OutDir
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$started = Get-Date
$root = Split-Path -Parent $PSScriptRoot
if (-not $OutDir) { $OutDir = Join-Path $root 'release\windows' }

function Write-Step([string]$Message) {
    Write-Host ''
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Stop-Build([string]$Message) {
    Write-Host ''
    Write-Host "ERREUR : $Message" -ForegroundColor Red
    exit 1
}

# Lance un programme et s'arrête s'il échoue.
function Invoke-Tool([string]$Exe, [string[]]$Arguments, [string]$Where) {
    Push-Location $Where
    try {
        & $Exe @Arguments
        $code = $LASTEXITCODE
    }
    finally {
        Pop-Location
    }
    if ($code -ne 0) { Stop-Build "« $Exe $($Arguments -join ' ') » a échoué (code $code)." }
}

function Test-Tool([string]$Name) {
    return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
}

# ---------------------------------------------------------------- Vérifications
Write-Step 'Vérification des prérequis'

if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    Stop-Build 'ce script construit la version Windows : lancez-le sous Windows.'
}
if (-not (Test-Tool 'cargo')) { Stop-Build 'Rust est introuvable. Installez-le depuis https://rustup.rs' }
if (-not (Test-Tool 'trunk')) { Stop-Build 'Trunk est introuvable. Installez-le : cargo install trunk --locked' }

& cargo tauri --version *> $null
if ($LASTEXITCODE -ne 0) { Stop-Build 'tauri-cli est introuvable. Installez-le : cargo install tauri-cli --version "^2" --locked' }

$targets = & rustup target list --installed
if ($targets -notcontains 'wasm32-unknown-unknown') {
    Write-Host 'Ajout de la cible wasm32-unknown-unknown (frontend Leptos)…'
    Invoke-Tool 'rustup' @('target', 'add', 'wasm32-unknown-unknown') $root
}

# Un « trunk serve » (cargo tauri dev) utilise le même dossier dist : les deux builds se gêneraient.
if (Get-Process -Name 'trunk' -ErrorAction SilentlyContinue) {
    Stop-Build 'un serveur Trunk tourne (cargo tauri dev ?). Fermez-le avant de construire la version release.'
}
# Une instance lancée depuis target\release verrouillerait l'exécutable à reconstruire
# (l'application de dev, dans target\debug, ne gêne pas).
$releaseDir = Join-Path $root 'target\release'
$running = Get-Process -Name 'rustmotion', 'rustmotion-cli' -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -and $_.Path.StartsWith($releaseDir, [StringComparison]::OrdinalIgnoreCase) }
if ($running) { Stop-Build "RustMotion est ouvert depuis $releaseDir : fermez-le d'abord." }

# Par convention NO_COLOR accepte n'importe quelle valeur (souvent 1), mais Trunk n'admet que true/false.
if ($env:NO_COLOR -and @('true', 'false') -notcontains $env:NO_COLOR) { $env:NO_COLOR = 'true' }

$conf = Get-Content (Join-Path $root 'src-tauri\tauri.conf.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$version = $conf.version
Write-Host "RustMotion $version, sortie : $OutDir"

if ($Clean) {
    Write-Step 'Nettoyage'
    foreach ($dir in @((Join-Path $root 'dist'), (Join-Path $root 'target\release\bundle'))) {
        if (Test-Path $dir) { Remove-Item $dir -Recurse -Force; Write-Host "supprimé : $dir" }
    }
}

# ---------------------------------------------------------------- Construction
if ($Portable) {
    Write-Step 'Application (exécutable seul)'
    Invoke-Tool 'cargo' @('tauri', 'build', '--no-bundle') (Join-Path $root 'src-tauri')
}
else {
    Write-Step 'Application et installateurs (MSI + NSIS)'
    Invoke-Tool 'cargo' @('tauri', 'build', '--bundles', 'msi,nsis') (Join-Path $root 'src-tauri')
}

if (-not $SkipCli) {
    Write-Step 'CLI'
    Invoke-Tool 'cargo' @('build', '--release', '--locked', '-p', 'rustmotion-engine') $root
}

# ---------------------------------------------------------------- Rassemblement
Write-Step 'Rassemblement des fichiers'
if (Test-Path $OutDir) { Remove-Item (Join-Path $OutDir '*') -Recurse -Force }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$release = Join-Path $root 'target\release'
Copy-Item (Join-Path $release 'rustmotion.exe') (Join-Path $OutDir "RustMotion_${version}_x64_portable.exe")

if (-not $Portable) {
    $installers = @(
        Get-ChildItem (Join-Path $release 'bundle\msi') -Filter "*_${version}_*.msi" -ErrorAction SilentlyContinue
        Get-ChildItem (Join-Path $release 'bundle\nsis') -Filter "*_${version}_*-setup.exe" -ErrorAction SilentlyContinue
    )
    if ($installers.Count -eq 0) { Stop-Build "aucun installateur $version trouvé dans $release\bundle." }
    $installers | Copy-Item -Destination $OutDir
}

if (-not $SkipCli) {
    $name = "rustmotion-cli-$version-windows-x64"
    $stage = Join-Path ([IO.Path]::GetTempPath()) $name
    if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
    New-Item -ItemType Directory -Path $stage | Out-Null
    Copy-Item (Join-Path $release 'rustmotion-cli.exe') $stage
    foreach ($doc in 'README.md', 'README.en.md', 'LICENSE') { Copy-Item (Join-Path $root $doc) $stage }
    Compress-Archive -Path $stage -DestinationPath (Join-Path $OutDir "$name.zip") -Force
    Remove-Item $stage -Recurse -Force
}

# ---------------------------------------------------------------- Bilan
$elapsed = (Get-Date) - $started
Write-Step ("Terminé en {0:mm} min {0:ss} s" -f $elapsed)
Get-ChildItem $OutDir | Sort-Object Name | ForEach-Object {
    '{0,9:N1} Mo  {1}' -f ($_.Length / 1MB), $_.Name
} | Write-Host
Write-Host ''
Write-Host "Dossier : $OutDir"
