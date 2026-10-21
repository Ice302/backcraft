<#
.SYNOPSIS
    Builds FilmCraft with the Voice Isolation extension and runs its checks (Windows).

.DESCRIPTION
    Runs, in order: rustfmt check, clippy (warnings are errors), the DSP tests (the effect's own
    tests plus FilmCraft's registry-wide stress tests), the project/render mapping tests, the
    engine integration tests, the i18n completeness tests and the headless UI tests.
    The first run compiles FilmCraft and takes a while (10-30 minutes); later runs are quick.

.PARAMETER Quick
    Only the DSP and engine tests (no UI build).
#>
param(
    [string]$FilmCraftDir = (Join-Path $HOME "Developer\filmcraft"),
    [switch]$Quick
)

$ErrorActionPreference = "Continue"
if (-not (Test-Path (Join-Path $FilmCraftDir "crates\audio-dsp\src\effects\voice_isolation.rs"))) {
    Write-Host "ERROR: Voice Isolation is not installed in $FilmCraftDir (run scripts\install.ps1 first)." -ForegroundColor Red
    exit 1
}
# Deep folders hit Windows' 260-character path limit while linking build scripts (LNK1104).
if ((-not $env:CARGO_TARGET_DIR) -and ((Resolve-Path $FilmCraftDir).Path.Length -gt 60)) {
    $env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "fc-target"
    Write-Host "Using CARGO_TARGET_DIR=$env:CARGO_TARGET_DIR (the checkout path is long)." -ForegroundColor Yellow
}

# Big debug test binaries exceed the MSVC linker's PDB module limit (LNK1140); FilmCraft's own UI
# tests hit it too. Building the checks without debug info avoids it.
if (-not $env:CARGO_PROFILE_DEV_DEBUG) { $env:CARGO_PROFILE_DEV_DEBUG = "0" }

$crates = @("filmcraft-audio-dsp", "filmcraft-project", "filmcraft-render", "filmcraft-engine", "filmcraft-ui-egui")
$steps = @()
foreach ($c in $crates) { $steps += , @("rustfmt $c", @("fmt", "-p", $c, "--check")) }
$steps += , @("DSP tests (effect + registry stress tests)", @("test", "-p", "filmcraft-audio-dsp"))
$steps += , @("project/render mapping tests", @("test", "-p", "filmcraft-project", "-p", "filmcraft-render"))
$steps += , @("engine integration tests", @("test", "-p", "filmcraft-engine", "--lib", "voice_isolation"))
$steps += , @("engine Essential Sound tests", @("test", "-p", "filmcraft-engine", "--lib", "essential_sound"))
if (-not $Quick) {
    # Two lints are allowed because unmodified FilmCraft code (file_dialogs.rs, timeline.rs) trips them
    # under clippy 1.95; every file the extension adds or changes is clean without them.
    $steps += , @("clippy (warnings are errors)", @("clippy", "-p", "filmcraft-audio-dsp", "-p", "filmcraft-project", "-p", "filmcraft-render", "-p", "filmcraft-engine", "-p", "filmcraft-ui-egui", "--all-targets", "--", "-D", "warnings", "-A", "clippy::nonminimal_bool", "-A", "clippy::collapsible_match"))
    $steps += , @("i18n completeness tests", @("test", "-p", "filmcraft-ui-egui", "--lib", "i18n"))
    $steps += , @("headless UI tests", @("test", "-p", "filmcraft-ui-egui", "--test", "voice_isolation_ui", "--test", "audio_fx_editor_ui", "--test", "essential_sound_ui"))
}

Push-Location $FilmCraftDir
$failed = @()
try {
    foreach ($s in $steps) {
        Write-Host "==> $($s[0])" -ForegroundColor Cyan
        & cargo @($s[1])
        if ($LASTEXITCODE -ne 0) { $failed += $s[0] }
    }
} finally {
    Pop-Location
}
Write-Host ""
if ($failed.Count -eq 0) {
    Write-Host "All checks passed." -ForegroundColor Green
    exit 0
}
Write-Host "Failed: $($failed -join ', ')" -ForegroundColor Red
Write-Host "See docs\TROUBLESHOOTING.md." -ForegroundColor Red
exit 1
