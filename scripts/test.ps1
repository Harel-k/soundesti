# Builds all tests once, then runs each test binary separately, retrying only
# when Windows refuses to launch it (antivirus, KI-001). Extra args go to the test binaries.
# Usage: scripts\test.ps1            scripts\test.ps1 --ignored
$root = Split-Path $PSScriptRoot -Parent
$testArgs = @($args) + @("--quiet")
$build = & powershell -NoProfile -ExecutionPolicy Bypass -File "$PSScriptRoot\cargo.ps1" test --workspace --no-run 2>&1 | ForEach-Object { "$_" }
if ($LASTEXITCODE -ne 0) { $build; exit 1 }
$exes = $build | Select-String -Pattern "Executable .*\((.+\.exe)\)" | ForEach-Object { $_.Matches[0].Groups[1].Value } | Sort-Object -Unique
$passed = 0; $failed = @()
foreach ($rel in $exes) {
    $exe = Join-Path $root $rel
    for ($attempt = 1; $attempt -le 15; $attempt++) {
        try {
            $out = & $exe @testArgs 2>&1 | ForEach-Object { "$_" }
            $code = $LASTEXITCODE
            # Seen on this machine: an intercepted launch occasionally runs with a stray
            # filter ("0 passed; ... N filtered out"). Treat that as a launch failure too.
            if (-not ($out -match " 0 passed; .* [1-9]\d* filtered out") -or $args.Count) { break }
            Start-Sleep -Milliseconds 500
        } catch {
            if ($attempt -eq 15) { $out = @("launch failed: $_"); $code = 1 }
            Start-Sleep -Milliseconds 800
        }
    }
    $summary = $out | Select-String "test result" | Select-Object -Last 1
    $name = [IO.Path]::GetFileNameWithoutExtension($rel) -replace "-[0-9a-f]{16}$", ""
    "{0,-28} {1}" -f $name, $(if ($summary) { $summary.Line.Trim() } else { ($out | Select-Object -Last 1) })
    if ($code -ne 0) { $failed += $name; $out | Select-String -Pattern "panicked|FAILED|assert" | ForEach-Object { "    $($_.Line)" } }
    $m = [regex]::Match("$summary", "(\d+) passed"); if ($m.Success) { $passed += [int]$m.Groups[1].Value }
}
"TOTAL: $passed passed, $($failed.Count) binaries failed $($failed -join ', ')"
if ($failed.Count) { exit 1 }
