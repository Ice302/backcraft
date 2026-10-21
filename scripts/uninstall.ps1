<#
.SYNOPSIS
    Removes the BackCraft Voice Isolation extension from a FilmCraft checkout (Windows).

.DESCRIPTION
    Reverses patches\voice-isolation.patch. If the extension's files were edited since, nothing is
    changed. Projects saved while the extension was installed still open afterwards: FilmCraft
    keeps the unknown "voice_isolation" effect in the project file but no longer processes it,
    and Essential Sound ignores the "isolate" setting.
#>
param(
    [string]$FilmCraftDir = (Join-Path $HOME "Developer\filmcraft")
)

# exit codes are checked one by one (see install.ps1)
$ErrorActionPreference = "Continue"
$Patch = Join-Path (Split-Path $PSScriptRoot -Parent) "patches\voice-isolation.patch"
function Invoke-Git {
    & git @args 2>&1 | ForEach-Object { "$_" }
}

if (-not (Test-Path (Join-Path $FilmCraftDir "crates\audio-dsp\src\effects\voice_isolation.rs"))) {
    Write-Host "Voice Isolation is not installed in $FilmCraftDir." -ForegroundColor Yellow
    exit 0
}
$PatchFull = (Resolve-Path $Patch).Path
$null = Invoke-Git -C "$FilmCraftDir" apply -R --check --whitespace=nowarn "$PatchFull"
if ($LASTEXITCODE -eq 0) {
    Invoke-Git -C "$FilmCraftDir" apply -R --whitespace=nowarn "$PatchFull" | ForEach-Object { Write-Host "    $_" }
} else {
    $null = Invoke-Git -C "$FilmCraftDir" apply -R --check --3way --whitespace=nowarn "$PatchFull"
    if ($LASTEXITCODE -eq 0) {
        Invoke-Git -C "$FilmCraftDir" apply -R --3way --whitespace=nowarn "$PatchFull" | ForEach-Object { Write-Host "    $_" }
    }
}
if ($LASTEXITCODE -ne 0) {
    Write-Host "ERROR: nothing was changed: the extension's files were edited since it was installed, so they can't be reversed automatically." -ForegroundColor Red
    Write-Host "Use 'git -C `"$FilmCraftDir`" status' to see them; see docs\TROUBLESHOOTING.md (Uninstalling)." -ForegroundColor Red
    exit 1
}
Write-Host "Voice Isolation removed from $FilmCraftDir." -ForegroundColor Green
exit 0
