<#
.SYNOPSIS
    Installs the BackCraft Voice Isolation extension into a FilmCraft source checkout (Windows).

.DESCRIPTION
    Applies patches\voice-isolation.patch to a FilmCraft git checkout. Nothing outside that
    checkout is changed, and a checkout the patch does not fit is left untouched. Run
    scripts\uninstall.ps1 to take it out again.

.PARAMETER FilmCraftDir
    The FilmCraft checkout. Default: $HOME\Developer\filmcraft (where FilmCraft's own setup
    instructions clone it).

.PARAMETER Clone
    Clone FilmCraft into FilmCraftDir first (at the tested commit, on a new branch
    "voice-isolation") if the folder does not exist yet.

.PARAMETER Test
    Run scripts\verify.ps1 after installing (builds FilmCraft and runs the extension's tests).

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Clone -Test
#>
param(
    [string]$FilmCraftDir = (Join-Path $HOME "Developer\filmcraft"),
    [switch]$Clone,
    [switch]$Test
)

# Native commands (git) report through exit codes, which are checked one by one. ("Stop" would
# turn git's progress and diagnostics on stderr into exceptions in Windows PowerShell 5.1.)
$ErrorActionPreference = "Continue"
$PinnedCommit = "70f0fc7a75f2681bc18302e67eaf0c9d41c68873"
$MinRust = [version]"1.95.0"
$Root = Split-Path $PSScriptRoot -Parent
$Patch = Join-Path $Root "patches\voice-isolation.patch"

function Fail([string]$msg) {
    Write-Host ""
    Write-Host "ERROR: $msg" -ForegroundColor Red
    Write-Host "See docs\TROUBLESHOOTING.md for help." -ForegroundColor Red
    exit 1
}
function Step([string]$msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
function Warn([string]$msg) { Write-Host "WARNING: $msg" -ForegroundColor Yellow }
# Run git; its output (stdout and stderr) comes back as plain strings, $LASTEXITCODE is git's.
function Invoke-Git {
    & git @args 2>&1 | ForEach-Object { "$_" }
}

# ---- tools
Step "Checking tools"
if (-not (Get-Command git -ErrorAction SilentlyContinue)) { Fail "git was not found. Install Git for Windows (https://git-scm.com/download/win) and open a new terminal." }
if (-not (Test-Path $Patch)) { Fail "The patch file is missing: $Patch" }
if (Get-Command rustc -ErrorAction SilentlyContinue) {
    $rv = "$(rustc --version)" -replace '^rustc ([0-9.]+).*$', '$1'
    $rustOk = $false
    try { $rustOk = ([version]$rv -ge $MinRust) } catch { $rustOk = $false }
    if ($rustOk) { Write-Host "    rustc $rv (FilmCraft needs $MinRust or newer)" }
    else { Warn "rustc $rv is older than $MinRust, which FilmCraft needs. Run 'rustup update stable' before building (see docs\INSTALL.md)." }
} else {
    Warn "Rust was not found. Install it from https://rustup.rs before building FilmCraft (see docs\INSTALL.md)."
}

# ---- FilmCraft checkout
if (-not (Test-Path $FilmCraftDir)) {
    if (-not $Clone) { Fail "No FilmCraft checkout at '$FilmCraftDir'. Pass -FilmCraftDir <path>, or -Clone to download it there." }
    Step "Cloning FilmCraft into $FilmCraftDir (a few minutes)"
    $parent = Split-Path $FilmCraftDir -Parent
    if ($parent -and -not (Test-Path $parent)) { New-Item -ItemType Directory -Force $parent | Out-Null }
    Invoke-Git clone https://github.com/storytold/filmcraft.git "$FilmCraftDir" | ForEach-Object { Write-Host "    $_" }
    if ($LASTEXITCODE -ne 0) { Fail "git clone failed (network?). Retry, or clone manually as described in docs\INSTALL.md." }
    Invoke-Git -C "$FilmCraftDir" switch -c voice-isolation $PinnedCommit | ForEach-Object { Write-Host "    $_" }
    if ($LASTEXITCODE -ne 0) { Fail "Could not check out the tested FilmCraft commit $PinnedCommit." }
}
if (-not (Test-Path (Join-Path $FilmCraftDir "crates\audio-dsp\src\effects\mod.rs"))) {
    Fail "'$FilmCraftDir' does not look like a FilmCraft checkout (crates\audio-dsp is missing)."
}
$null = Invoke-Git -C "$FilmCraftDir" rev-parse --git-dir
if ($LASTEXITCODE -ne 0) { Fail "'$FilmCraftDir' is not a git checkout. Clone FilmCraft with git (docs\INSTALL.md); a downloaded ZIP cannot be patched safely." }

if (Test-Path (Join-Path $FilmCraftDir "crates\audio-dsp\src\effects\voice_isolation.rs")) {
    Write-Host "Voice Isolation is already installed in $FilmCraftDir. (Run scripts\uninstall.ps1 first to reinstall.)" -ForegroundColor Green
    if ($Test) { & (Join-Path $PSScriptRoot "verify.ps1") -FilmCraftDir $FilmCraftDir; exit $LASTEXITCODE }
    exit 0
}

$head = "$(Invoke-Git -C "$FilmCraftDir" rev-parse HEAD)".Trim()
if ($head -ne $PinnedCommit) {
    Warn "FilmCraft is at $($head.Substring(0, [Math]::Min(10, $head.Length))); the extension was tested at $($PinnedCommit.Substring(0, 10)). It usually still applies; if not, see docs\INSTALL.md (Newer FilmCraft versions)."
}

# ---- apply: dry runs first, so a checkout the patch doesn't fit is never left half-changed
Step "Applying the extension"
$PatchFull = (Resolve-Path $Patch).Path
$null = Invoke-Git -C "$FilmCraftDir" apply --check --whitespace=nowarn "$PatchFull"
if ($LASTEXITCODE -eq 0) {
    Invoke-Git -C "$FilmCraftDir" apply --whitespace=nowarn "$PatchFull" | ForEach-Object { Write-Host "    $_" }
} else {
    # newer FilmCraft versions or committed local edits: merge through git's history
    Write-Host "    A plain apply does not fit this checkout; trying a three-way merge..."
    $why = Invoke-Git -C "$FilmCraftDir" apply --check --3way --whitespace=nowarn "$PatchFull"
    if ($LASTEXITCODE -ne 0) {
        $why | Where-Object { $_ -match "error|conflict" } | ForEach-Object { Write-Host "    $_" }
        Fail "Nothing was changed: the patch does not fit this checkout. If you edited FilmCraft files yourself, commit those edits first ('git commit -am ""my changes""') and run this again; for a much newer FilmCraft see docs\INSTALL.md (Newer FilmCraft versions)."
    }
    # --check can't see merge conflicts, so merge in a scratch worktree first
    $trial = Join-Path ([IO.Path]::GetTempPath()) ("fc-vi-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
    $null = Invoke-Git -C "$FilmCraftDir" worktree add --detach --quiet "$trial" HEAD
    if ($LASTEXITCODE -ne 0) { Fail "Could not create a scratch worktree to test the merge in ($trial)." }
    $null = Invoke-Git -C "$trial" apply --3way --whitespace=nowarn "$PatchFull"
    $merged = ($LASTEXITCODE -eq 0)
    $conflicts = @(Invoke-Git -C "$trial" diff --name-only --diff-filter=U)
    $null = Invoke-Git -C "$FilmCraftDir" worktree remove --force "$trial"
    if (-not $merged) {
        $conflicts | ForEach-Object { Write-Host "    conflict: $_" }
        Fail "Nothing was changed: the extension conflicts with changes in this checkout (listed above). Resolve it by hand as described in docs\INSTALL.md (Newer FilmCraft versions), or install on a fresh checkout of the tested commit."
    }
    Invoke-Git -C "$FilmCraftDir" apply --3way --whitespace=nowarn "$PatchFull" | ForEach-Object { Write-Host "    $_" }
}
if ($LASTEXITCODE -ne 0) { Fail "git could not apply the patch (see the messages above)." }
if (-not (Test-Path (Join-Path $FilmCraftDir "crates\audio-dsp\src\effects\voice_isolation.rs"))) { Fail "The patch reported success but the new files are missing." }

Write-Host ""
Write-Host "Voice Isolation installed into $FilmCraftDir" -ForegroundColor Green
Write-Host "Changed files:"
Invoke-Git -C "$FilmCraftDir" status --short | ForEach-Object { Write-Host "  $_" }
Write-Host ""
Write-Host "Next:"
Write-Host "  cd `"$FilmCraftDir`""
Write-Host "  cargo run -p filmcraft --release"
Write-Host "Then: Effects panel > Audio Effects > Noise Reduction/Restoration > Voice Isolation,"
Write-Host "or Essential Sound > Dialogue > Repair > Isolate Voice. See docs\USER-GUIDE.md."

if ($Test) {
    & (Join-Path $PSScriptRoot "verify.ps1") -FilmCraftDir $FilmCraftDir
    exit $LASTEXITCODE
}
exit 0
