# Runs `cargo <args>` and retries when the OS refuses to launch a build tool or
# freshly built binary ("Access is denied. (os error 5)").
#
# Why: on the main dev machine a third-party antivirus intermittently blocks
# rustc.exe, link.exe and new test executables. That is a launch failure, not a
# code failure; genuine compile errors and test failures are never retried.
# See docs/KNOWN_ISSUES.md KI-001. Usage:  scripts\cargo.ps1 test --workspace
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
$cargoArgs = $args
for ($attempt = 1; $attempt -le 10; $attempt++) {
    $output = & cargo @cargoArgs 2>&1 | ForEach-Object { "$_" }
    $code = $LASTEXITCODE
    $blocked = $output | Select-String -SimpleMatch "Access is denied. (os error 5)"
    $realFailure = $output | Select-String -Pattern "test result: FAILED|^error\[|panicked at"
    if ($code -eq 0 -or $realFailure -or -not $blocked) {
        $output
        exit $code
    }
    Write-Host "[cargo.ps1] attempt ${attempt}: launch blocked by antivirus, retrying..."
    Start-Sleep -Milliseconds 1500
}
Write-Host "[cargo.ps1] gave up after repeated antivirus blocks"
exit 1
