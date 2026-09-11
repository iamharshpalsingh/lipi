# Installs LiPi for the current user (no administrator rights needed):
# copies lipi.exe to %LOCALAPPDATA%\Programs\LiPi\bin and adds that folder to
# your PATH. Also installs the VS Code extension if VS Code is installed.
#
#   .\install.ps1              install (or update)
#   .\install.ps1 -Uninstall   remove LiPi again
#   -Dest <folder>             install somewhere else
#   -NoPath                    don't change PATH
param([switch]$Uninstall, [string]$Dest = "", [switch]$NoPath)
$ErrorActionPreference = "Stop"

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
if ($Dest -eq "") { $Dest = Join-Path $env:LOCALAPPDATA "Programs\LiPi\bin" }
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
$parts = @($userPath -split ";" | Where-Object { $_ -ne "" })

if ($Uninstall) {
    if (Test-Path (Join-Path $Dest "lipi.exe")) { Remove-Item -Force (Join-Path $Dest "lipi.exe") }
    if (-not $NoPath) {
        [Environment]::SetEnvironmentVariable("Path", (($parts | Where-Object { $_ -ne $Dest }) -join ";"), "User")
    }
    Write-Host "LiPi was removed."
    exit 0
}

$exe = Join-Path $here "lipi.exe"
if (-not (Test-Path $exe)) {
    Write-Host "lipi.exe wasn't found next to this script. Run install.ps1 from the unzipped LiPi folder."
    exit 1
}
New-Item -ItemType Directory -Force $Dest | Out-Null
Copy-Item $exe $Dest -Force
Unblock-File (Join-Path $Dest "lipi.exe") -ErrorAction SilentlyContinue

$added = $false
if (-not $NoPath -and $parts -notcontains $Dest) {
    [Environment]::SetEnvironmentVariable("Path", (($parts + $Dest) -join ";"), "User")
    $added = $true
}

$vsix = Get-ChildItem $here -Filter "lipi-*.vsix" -ErrorAction SilentlyContinue | Select-Object -First 1
if ($vsix -and (Get-Command code -ErrorAction SilentlyContinue)) {
    code --install-extension $vsix.FullName | Out-Null
    Write-Host "Installed the LiPi extension for VS Code."
}

& (Join-Path $Dest "lipi.exe") --version
Write-Host "LiPi is installed in $Dest"
if ($added) {
    Write-Host "Open a new terminal, then try:  lipi new my-app"
} else {
    Write-Host "Try:  lipi new my-app"
}
