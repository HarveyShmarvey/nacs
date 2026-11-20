# sidecar/setup.ps1: the sidecar's one-time setup on Windows. A Python 3.11
# venv under sidecar/.venv and the pinned stack from requirements.txt (about
# 1.5 GB, from wheels, no compiler). This is the only network use the sidecar
# ever makes, and it is yours, once.
# Keep this file ASCII: Windows PowerShell 5.1 reads it as ANSI, where the
# last byte of a UTF-8 dash is a closing quote and the script stops parsing.
# Run from the repo root:  powershell -ExecutionPolicy Bypass -File sidecar\setup.ps1
$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$venv = Join-Path $here ".venv"
$python = $null
foreach ($candidate in @("py -3.11", "python3.11", "python")) {
    try {
        $v = & cmd /c "$candidate --version" 2>$null
        if ($v -match "3\.11") { $python = $candidate; break }
    } catch {}
}
if (-not $python) {
    Write-Error "Python 3.11 is needed (the pinned stack installs from wheels there). Install it from python.org, then run this again."
}
if (-not (Test-Path (Join-Path $venv "Scripts\python.exe"))) {
    Write-Host "creating $venv with $python"
    & cmd /c "$python -m venv `"$venv`""
}
& (Join-Path $venv "Scripts\python.exe") -m pip install --upgrade pip
& (Join-Path $venv "Scripts\pip.exe") install -r (Join-Path $here "requirements.txt")
Write-Host ""
Write-Host "done. The window imports a dropped STEP file now, and so does:  nacs --import <model.step> <project.cam>"
