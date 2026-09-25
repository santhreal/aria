# Assemble a distributable Windows zip for Aria (alpha packaging).
#
# Decode and encode go through Media Foundation, which is part of the OS: no
# codec libraries are bundled and none are needed. The zip runs in place; the
# Start-menu shortcut, the project file association and the uninstaller come
# from the installer built by package-windows-installer.ps1.
#
# Usage:
#   .\scripts\package-windows.ps1
#   .\scripts\package-windows.ps1 -Arch arm64
#
# Output:
#   dist\Aria-<version>-windows-<arch>.zip
#
# Prerequisites:
#   - Rust stable (see rust-toolchain.toml)
#   - release build: cargo build --release -p aria-desktop

param(
    [ValidateSet("x86_64", "arm64")]
    [string]$Arch = "x86_64"
)

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

$Metadata = cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json
$Version = ($Metadata.packages | Where-Object { $_.name -eq "aria-desktop" }).version
if (-not $Version) {
    throw "aria-desktop is missing from cargo metadata."
}
$TargetDir = $Metadata.target_directory
$Dist = "dist"
$Staging = Join-Path $Dist "staging-windows-$Arch"
$Pkg = "aria-$Version-windows-$Arch"

Write-Host "==> packaging Aria $Version for Windows ($Arch)"

$BinarySrc = Join-Path $TargetDir "release\aria-desktop.exe"
if (-not (Test-Path $BinarySrc)) {
    Write-Host "==> release binary missing; building aria-desktop"
    cargo build --release -p aria-desktop --locked
}

if (Test-Path $Staging) {
    Remove-Item -Recurse -Force $Staging
}
$PkgDir = Join-Path $Staging $Pkg
New-Item -ItemType Directory -Path $PkgDir -Force | Out-Null

Copy-Item $BinarySrc (Join-Path $PkgDir "aria-desktop.exe")
Copy-Item LICENSE-MIT, LICENSE-APACHE -Destination $PkgDir
# The executable carries no icon resource, so the shortcut, the wizard and the
# .aria file type all point at this file. It travels in the payload because the
# installer references it under {app}.
Copy-Item (Join-Path $Root "packaging\windows\aria.ico") -Destination $PkgDir

$Readme = @"
Aria $Version — Windows ($Arch)
===================================

Run:
  .\aria-desktop.exe

Decode and export use Media Foundation, which ships with Windows: nothing
else to install. The H.264 video / AAC audio pair is the only output.

To get a Start-menu entry, a desktop icon and .aria files that open by
double-click, use the Setup.exe installer instead of this zip.

See https://github.com/1Mr-Newton/aria for source and issue tracker.
"@
Set-Content -Path (Join-Path $PkgDir "README-INSTALL.txt") -Value $Readme -Encoding utf8

$Zip = Join-Path $Dist "Aria-${Version}-windows-${Arch}.zip"
if (Test-Path $Zip) {
    Remove-Item $Zip
}
Compress-Archive -Path $PkgDir -DestinationPath $Zip

$Size = (Get-Item $Zip).Length / 1MB
Write-Host ("==> wrote {0} ({1:N1} MB)" -f $Zip, $Size)
