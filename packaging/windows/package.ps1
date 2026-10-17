<#
.SYNOPSIS
  Build, sign and package PdfCraft for Windows.

.DESCRIPTION
  Produces, in $env:DIST (default: dist/release):
    pdfcraft-<version>-windows-<arch>.msi            per-machine installer (WiX v5)
    pdfcraft-<version>-windows-<arch>-portable.zip   pdfcraft.exe + pdfcraft-cli.exe + models\ + portable.txt

  The binaries link the C runtime statically (+crt-static), so neither the MSI nor the portable
  zip needs the Visual C++ redistributable. Signing is delegated to sign.ps1 (skipped with a
  warning when no signing secrets are set).

  Needs: Rust (MSVC toolchain + the target), curl and the network (the OCR models), the Windows SDK (rc.exe, signtool.exe),
  and WiX v5: dotnet tool install --global wix --version 5.0.2

.EXAMPLE
  pwsh packaging/windows/package.ps1 -Arch x64
  pwsh packaging/windows/package.ps1 -Arch x86 -SkipBuild
  pwsh packaging/windows/package.ps1 -Arch arm64     # cross-compiled; needs the MSVC ARM64 build tools
#>
param(
  [ValidateSet('x64', 'x86', 'arm64')] [string] $Arch = 'x64',
  [switch] $SkipBuild
)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

function Invoke-Native([string] $What, [scriptblock] $Block) {
  Write-Output "==> $What"
  & $Block
  if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

# The version lives in one place: [workspace.package] version in the root Cargo.toml.
$Version = $null
if (-not $Version) {
  $inPkg = $false
  foreach ($line in Get-Content (Join-Path $Root 'Cargo.toml')) {
    if ($line -match '^\s*\[') { $inPkg = ($line.Trim() -eq '[workspace.package]'); continue }
    if ($inPkg -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $Version = $Matches[1]; break }
  }
}
if (-not $Version) { throw 'could not read [workspace.package] version from Cargo.toml' }
$env:LHTPDF_VERSION = $Version
$ReleaseLabel = $Version
# MSI ProductVersion is numeric (major.minor.build); pre-release tags are dropped there.
$MsiVersion = ($Version -split '-')[0]

$Target = switch ($Arch) { 'x64' { 'x86_64-pc-windows-msvc' } 'x86' { 'i686-pc-windows-msvc' } 'arm64' { 'aarch64-pc-windows-msvc' } }
$Dist = if ($env:DIST) { $env:DIST } else { Join-Path $Root 'dist\release' }
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
New-Item -ItemType Directory -Force -Path $Dist | Out-Null

if (-not $env:PDFCRAFT_BUILD_SHA) { $env:PDFCRAFT_BUILD_SHA = (git -C $Root rev-parse HEAD 2>$null) }
if (-not $env:PDFCRAFT_BUILD_DATE) { $env:PDFCRAFT_BUILD_DATE = (Get-Date).ToUniversalTime().ToString('yyyy-MM-dd') }

Write-Output "PdfCraft $Version for Windows $Arch ($Target)"

# One canonical binary source for Windows x64 releases.
# MSI and portable ZIP always use the same Release executables.
if ($Arch -eq 'x64') {
  $Bin = Join-Path $TargetDir 'release'
} else {
  $Bin = Join-Path $TargetDir "$Target\release"
}

if (-not $SkipBuild) {
  # Static CRT: no VC++ redistributable needed.
  # Apply target-specific flags without changing host build scripts.
  $flagVar = 'CARGO_TARGET_' + ($Target.ToUpper() -replace '-', '_') + '_RUSTFLAGS'
  [Environment]::SetEnvironmentVariable($flagVar, '-C target-feature=+crt-static')
  $env:PDFCRAFT_REQUIRE_WINRES = '1'

  if ($Arch -eq 'x64') {
    Invoke-Native 'cargo build (canonical x64 Release)' {
      cargo build --release --locked -j 20 -p pdfcraft -p pdfcraft-cli
    }
  } else {
    Invoke-Native "cargo build ($Target)" {
      cargo build --release --locked -j 20 -p pdfcraft -p pdfcraft-cli --target $Target
    }
  }
}

# Never package an executable reporting a different release version.
if ($Arch -eq 'x64') {
  $CliPath = Join-Path $Bin 'pdfcraft-cli.exe'
  if (-not (Test-Path -LiteralPath $CliPath -PathType Leaf)) {
    throw "Missing Release CLI: $CliPath"
  }

  $CliVersionOutput = & $CliPath --version
  if ($LASTEXITCODE -ne 0) {
    throw 'Release CLI version check failed'
  }

  $ExpectedVersionLine = "lhtPDF $Version"
  if (@($CliVersionOutput) -notcontains $ExpectedVersionLine) {
    throw "Release CLI version mismatch: expected $ExpectedVersionLine; got $($CliVersionOutput -join '; ')"
  }

  Write-Output "ok Release CLI version: $ExpectedVersionLine"
}

# Check both binaries' PE headers before packaging. Machine (COFF header) must match -Arch, so an
# x64 build can never ship labelled arm64 (#61). Subsystem (optional header): 2 = Windows GUI,
# 3 = console. The app must be GUI (no console window opens with it, #57); the CLI must stay
# console so its output reaches the terminal.
function Get-PeHeader([string] $Path) {
  $bytes = [System.IO.File]::ReadAllBytes($Path)
  $pe = [BitConverter]::ToInt32($bytes, 0x3C)
  return @{ Machine = [BitConverter]::ToUInt16($bytes, $pe + 4); Subsystem = [BitConverter]::ToUInt16($bytes, $pe + 0x5C) }
}
$Machine = switch ($Arch) { 'x64' { 0x8664 } 'x86' { 0x14C } 'arm64' { 0xAA64 } }
foreach ($check in @(@('pdfcraft.exe', 2), @('pdfcraft-cli.exe', 3))) {
  $h = Get-PeHeader (Join-Path $Bin $check[0])
  if ($h.Machine -ne $Machine) { throw "$($check[0]) is for machine 0x$('{0:X}' -f $h.Machine), expected 0x$('{0:X}' -f $Machine) ($Arch)" }
  if ($h.Subsystem -ne $check[1]) { throw "$($check[0]) has PE subsystem $($h.Subsystem), expected $($check[1])" }
  Write-Output "ok $($check[0]): $Arch, PE subsystem $($h.Subsystem)"
}
$Stage = Join-Path $TargetDir "windows-package\$Arch"
Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
Copy-Item (Join-Path $Bin 'pdfcraft.exe') (Join-Path $Stage 'lhtPDF.exe')
Copy-Item (Join-Path $Bin 'pdfcraft-cli.exe') (Join-Path $Stage 'lhtPDF-cli.exe')
# The document icon embeds Lucide artwork: carry its ISC notice and the project licences
# beside the binaries in both distributions.
foreach ($f in 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE') {
  Copy-Item (Join-Path $Root $f) $Stage
}
Copy-Item (Join-Path $Root 'assets\app-icon\LICENSE-document.txt') $Stage
# OCR fonts are embedded from the optional craft-fonts checkout, never stored in this repository.
# The MSI expects these two licence filenames in the staging directory.
if ($env:CRAFT_FONTS_DIR) {
  foreach ($entry in @(
    @('be-vietnam-pro', 'OFL-BeVietnamPro.txt'),
    @('gowun-dodum', 'OFL-GowunDodum.txt')
  )) {
    $licence = Join-Path $env:CRAFT_FONTS_DIR "fonts\$($entry[0])\OFL.txt"
    if (-not (Test-Path -LiteralPath $licence -PathType Leaf)) {
      throw "Missing OCR font licence: $licence"
    }
    Copy-Item -LiteralPath $licence -Destination (Join-Path $Stage $entry[1])
  }
} else {
  throw 'Windows release packaging requires CRAFT_FONTS_DIR with the OCR Unicode fonts'
}

& (Join-Path $PSScriptRoot 'sign.ps1') (Join-Path $Stage 'lhtPDF.exe') (Join-Path $Stage 'lhtPDF-cli.exe')

# OCR models (#103): the app looks in models\ beside pdfcraft.exe. `cargo xtask models` fetches every
# ATTRIBUTION.toml `kind = "model"` file (verified by SHA-256) with its licence and ATTRIBUTION.txt;
# the MSI and the portable zip take the whole folder, so new models ship without naming them here.
$Models = Join-Path $Stage 'models'
New-Item -ItemType Directory -Force -Path $Models | Out-Null
# Reuse previously verified local OCR assets. xtask still checks each
# model and licence against ATTRIBUTION.toml before packaging.
$LocalModels = Join-Path $Root 'assets\models'
if (Test-Path -LiteralPath $LocalModels -PathType Container) {
  foreach ($f in @(
    'text-detection.rten',
    'text-detection.rten.LICENCE.txt',
    'text-recognition.rten',
    'text-recognition.rten.LICENCE.txt'
  )) {
    $src = Join-Path $LocalModels $f
    if (Test-Path -LiteralPath $src -PathType Leaf) {
      Copy-Item -LiteralPath $src -Destination $Models
    }
  }
}
Push-Location $Root
try { Invoke-Native 'cargo xtask models' { cargo xtask models $Models } } finally { Pop-Location }
Remove-Item -Force (Join-Path $Models '*.part') -ErrorAction SilentlyContinue
if (-not (Test-Path (Join-Path $Models 'ATTRIBUTION.txt')) -or -not (Get-ChildItem (Join-Path $Models '*.LICENCE.txt'))) {
  throw "no OCR models in $Models after cargo xtask models"
}

# Offline Tesseract OCR for English, Vietnamese and Korean.
# Source directory is explicitly supplied for reproducible packaging.
$TesseractSource = $env:LHTPDF_TESSERACT_RUNTIME
if (-not $TesseractSource -or -not (Test-Path (Join-Path $TesseractSource 'tesseract.exe'))) {
  throw "Set LHTPDF_TESSERACT_RUNTIME to a verified Tesseract runtime directory"
}
$TesseractDir = Join-Path $Stage 'tesseract'
New-Item -ItemType Directory -Force -Path $TesseractDir | Out-Null
Copy-Item (Join-Path $TesseractSource 'tesseract.exe') $TesseractDir
Get-ChildItem -LiteralPath $TesseractSource -Filter '*.dll' -File |
  Copy-Item -Destination $TesseractDir
Copy-Item -Recurse (Join-Path $TesseractSource 'tessdata') $TesseractDir
$TesseractLicense = Join-Path $TesseractSource 'LICENSE-Tesseract.txt'
if (-not (Test-Path -LiteralPath $TesseractLicense -PathType Leaf)) {
  throw "Missing Tesseract license in LHTPDF_TESSERACT_RUNTIME"
}
Copy-Item -LiteralPath $TesseractLicense `
  -Destination (Join-Path $TesseractDir 'LICENSE-Tesseract.txt')
# ---- MSI ---------------------------------------------------------------------------------------
$Msi = Join-Path $Dist "lhtPDF-$ReleaseLabel-windows-$Arch.msi"
Invoke-Native 'wix build' {
  wix build (Join-Path $PSScriptRoot 'pdfcraft.wxs') -arch $Arch `
    (Join-Path $PSScriptRoot 'installer-ui.wxs') `
    -d "Version=$MsiVersion" -d "BinDir=$Stage" -d "ModelsDir=$Models" -d "TesseractDir=$TesseractDir" -d "IconPath=$(Join-Path $Root 'assets\app-icon\pdfcraft.ico')" `
    -o $Msi
}
# Inspect the built MSI, not just the XML, before signing/publishing it. In a child process, so
# its Windows Installer database handle is gone before signtool opens the MSI.
Invoke-Native 'test-msi' { & (Get-Process -Id $PID).Path -NoProfile -File (Join-Path $PSScriptRoot 'test-msi.ps1') $Msi }
# wix writes its debug symbols (.wixpdb) next to the MSI; keep them out of the release assets.
Remove-Item -Force -ErrorAction SilentlyContinue ([IO.Path]::ChangeExtension($Msi, '.wixpdb'))
& (Join-Path $PSScriptRoot 'sign.ps1') $Msi

# ---- portable zip ------------------------------------------------------------------------------
$Portable = Join-Path $TargetDir "windows-package\lhtPDF-$ReleaseLabel-windows-$Arch-portable"
Remove-Item -Recurse -Force $Portable -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Portable | Out-Null
Copy-Item (Join-Path $Stage '*.exe') $Portable
foreach ($f in 'NOTICE', 'LICENSE-document.txt', 'OFL-BeVietnamPro.txt', 'OFL-GowunDodum.txt') {
  Copy-Item (Join-Path $Stage $f) $Portable
}
Copy-Item -Recurse $Models (Join-Path $Portable 'models')
Copy-Item -Recurse $TesseractDir (Join-Path $Portable 'tesseract')
foreach ($f in 'README.md', 'LICENSE', 'LICENSE-MIT', 'LICENSE-APACHE') {
  $p = Join-Path $Root $f
  if (Test-Path $p) { Copy-Item $p $Portable }
}
# Builds made with craft-fonts (CRAFT_FONTS_DIR, set for every release) embed its fonts: ship their
# licences, fonts\<family>\OFL.txt -> OFL-<family>.txt.
if ($env:CRAFT_FONTS_DIR) {
  Get-ChildItem -Path (Join-Path $env:CRAFT_FONTS_DIR 'fonts') -Directory -ErrorAction SilentlyContinue | ForEach-Object {
    $ofl = Join-Path $_.FullName 'OFL.txt'
    if (Test-Path $ofl) { Copy-Item $ofl (Join-Path $Portable "OFL-$($_.Name).txt") }
  }
}
# portable.txt beside pdfcraft.exe switches on portable mode: settings, logs, recovery files and new
# digital IDs go to PdfCraftData\ next to the exe instead of %APPDATA% (#157; see
# crates/ui-egui/src/portable.rs).
Copy-Item (Join-Path $PSScriptRoot 'portable.txt') $Portable
$Zip = Join-Path $Dist "lhtPDF-$ReleaseLabel-windows-$Arch-portable.zip"
Remove-Item -Force $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path $Portable -DestinationPath $Zip

# Smoke-test the CLI when this machine can run it. An ARM64 build made on an x64 runner can't run
# here; .github/workflows/windows-arm64.yml installs and runs it on ARM64 instead.
$HostArch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
if ($Arch -ne 'arm64' -or $HostArch -eq 'arm64') {
  Invoke-Native 'pdfcraft-cli --version' { & (Join-Path $Stage 'lhtPDF-cli.exe') --version }
} else {
  Write-Output "skipping pdfcraft-cli --version: an $Arch build doesn't run on this $HostArch machine"
}
Get-Item $Msi, $Zip | Format-Table Name, Length
