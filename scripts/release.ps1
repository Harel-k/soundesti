# Publishes a Soundesti release on GitHub so installed copies can auto-update.
#   scripts\release.ps1 -Notes "What changed"
# Steps: checks the working tree is clean and pushed, builds the Installer Wizard,
# writes update.json (version, download URL, SHA-256), creates the GitHub release
# v<version> with both files. The version comes from [workspace.package] in Cargo.toml;
# bump it there (and commit) before releasing a new version.
param([string]$Notes = "", [switch]$Draft)
# Native tools (git, gh) write progress to stderr; PowerShell 5.1 would treat that as
# fatal under "Stop", so failures are detected through $LASTEXITCODE instead.
$ErrorActionPreference = "Continue"
$root = Split-Path $PSScriptRoot -Parent
$repo = "Harel-k/soundesti"
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version = "([^"]+)"').Matches[0].Groups[1].Value
$tag = "v$version"
if (git status --porcelain) { throw "Working tree is not clean; commit first." }
git push origin main 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { throw "git push failed" }
gh release view $tag --repo $repo 2>&1 | Out-Null
if ($LASTEXITCODE -eq 0) { throw "Release $tag already exists; bump the version in Cargo.toml." }

& powershell -NoProfile -ExecutionPolicy Bypass -File "$PSScriptRoot\package.ps1"
if ($LASTEXITCODE -ne 0) { throw "packaging failed" }

$out = Join-Path $root "dist\release"
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory $out | Out-Null
$asset = "Soundesti-Installer-Wizard.exe"   # no spaces: GitHub would rename them
Copy-Item (Join-Path $root "dist\Soundesti Installer Wizard.exe") (Join-Path $out $asset)
$sha = (Get-FileHash (Join-Path $out $asset) -Algorithm SHA256).Hash.ToLower()
if (-not $Notes) { $Notes = "Soundesti $version" }
$feed = [ordered]@{
    version = $version
    url     = "https://github.com/$repo/releases/download/$tag/$asset"
    sha256  = $sha
    notes   = $Notes
}
$feed | ConvertTo-Json | Set-Content -Encoding ascii (Join-Path $out "update.json")

$body = @"
$Notes

**Install:** download ``$asset`` and run it. If Soundesti is already installed, the wizard offers Repair / Force update / Modify / Uninstall, and installed copies update themselves (Help > Check for updates).

Windows may show a SmartScreen warning because the installer is not code-signed: click **More info > Run anyway**.

SHA-256: ``$sha``
"@
$ghArgs = @("release", "create", $tag, (Join-Path $out $asset), (Join-Path $out "update.json"), "--repo", $repo, "--title", "Soundesti $version", "--notes", $body)
if ($Draft) { $ghArgs += "--draft" }
gh @ghArgs
if ($LASTEXITCODE -ne 0) { throw "gh release create failed" }
Write-Host "Published $tag (sha256 $sha)"
