# Builds a release of Soundesti and the "Soundesti Installer Wizard.exe" that contains it.
# Output: dist\Soundesti Installer Wizard.exe (plus dist\payload\ staging and SHA256SUMS.txt)
# Usage: scripts\package.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$cargo = "$PSScriptRoot\cargo.ps1"
$dist = Join-Path $root "dist"
$stage = Join-Path $dist "payload"

Write-Host "1/4 Building Soundesti (release)..."
& powershell -NoProfile -ExecutionPolicy Bypass -File $cargo build --release -p soundesti-app | Select-String -Pattern "^(error|warning: unused)|Finished"
if ($LASTEXITCODE -ne 0) { throw "app build failed" }

Write-Host "2/4 Staging payload..."
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item (Join-Path $root "target\release\soundesti.exe") $stage
Copy-Item (Join-Path $root "LICENSE.txt") $stage
Copy-Item (Join-Path $root "LICENSE") (Join-Path $stage "LICENSE-GPL-3.0.txt")
Copy-Item (Join-Path $root "README.md") (Join-Path $stage "README.txt")
# Original sample pack rendered by `cargo run -p soundesti-session --example make_pack`.
Copy-Item -Recurse (Join-Path $root "assets\packs") (Join-Path $stage "Packs")

Write-Host "3/4 Building installer with embedded payload..."
$env:SOUNDESTI_PAYLOAD_DIR = $stage
& powershell -NoProfile -ExecutionPolicy Bypass -File $cargo build --release -p soundesti-installer | Select-String -Pattern "^(error|warning: unused)|Finished"
if ($LASTEXITCODE -ne 0) { throw "installer build failed" }
Remove-Item Env:\SOUNDESTI_PAYLOAD_DIR

Write-Host "4/4 Writing dist..."
$out = Join-Path $dist "Soundesti Installer Wizard.exe"
Copy-Item (Join-Path $root "target\release\soundesti-setup.exe") $out -Force
$hash = (Get-FileHash $out -Algorithm SHA256).Hash.ToLower()
"$hash  Soundesti Installer Wizard.exe" | Set-Content -Encoding ascii (Join-Path $dist "SHA256SUMS.txt")
$size = [math]::Round((Get-Item $out).Length / 1MB, 1)
Write-Host "Done: $out ($size MB, sha256 $hash)"
