# Installs LiPi for the current user (no administrator rights needed):
# - copies lipi.exe to %LOCALAPPDATA%\Programs\LiPi\bin and adds that folder to your PATH
# - registers .lipi files with Windows, so they show the LiPi logo and open
#   for editing (in VS Code if it's installed, otherwise Notepad)
# - installs the VS Code extension if VS Code is installed
#
#   .\install.ps1              install (or update)
#   .\install.ps1 -Uninstall   remove LiPi again
#   -Dest <folder>             install somewhere else
#   -NoPath                    don't change PATH
#   -NoFileTypes               don't register .lipi files
param([switch]$Uninstall, [string]$Dest = "", [switch]$NoPath, [switch]$NoFileTypes)
$ErrorActionPreference = "Stop"

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
if ($Dest -eq "") { $Dest = Join-Path $env:LOCALAPPDATA "Programs\LiPi\bin" }
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
$parts = @($userPath -split ";" | Where-Object { $_ -ne "" })
$classes = "HKCU:\Software\Classes"
$progId = "LiPi.SourceFile"

function Ensure-Key([string]$path) {
    if (-not (Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
}

# Tell Explorer that file types changed, so icons update right away.
function Update-Explorer {
    Add-Type -Namespace LiPiInstall -Name Shell -MemberDefinition '[DllImport("shell32.dll")] public static extern void SHChangeNotify(int eventId, int flags, IntPtr item1, IntPtr item2);' -ErrorAction SilentlyContinue
    [LiPiInstall.Shell]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
}

function Register-FileTypes([string]$exe) {
    Ensure-Key "$classes\.lipi"
    Set-ItemProperty "$classes\.lipi" -Name "(default)" -Value $progId
    Set-ItemProperty "$classes\.lipi" -Name "PerceivedType" -Value "text"
    Set-ItemProperty "$classes\.lipi" -Name "Content Type" -Value "text/plain"
    Ensure-Key "$classes\$progId\DefaultIcon"
    Ensure-Key "$classes\$progId\shell\open\command"
    Set-ItemProperty "$classes\$progId" -Name "(default)" -Value "LiPi source file"
    Set-ItemProperty "$classes\$progId\DefaultIcon" -Name "(default)" -Value ('"' + $exe + '",0')
    # Double-clicking opens the file for editing; it never runs the program.
    $editor = "notepad.exe"
    $code = Get-Command code -ErrorAction SilentlyContinue
    if ($code) {
        $vscode = Join-Path (Split-Path -Parent (Split-Path -Parent $code.Source)) "Code.exe"
        if (Test-Path $vscode) { $editor = $vscode }
    }
    Set-ItemProperty "$classes\$progId\shell\open\command" -Name "(default)" -Value ('"' + $editor + '" "%1"')
    Update-Explorer
}

function Unregister-FileTypes {
    $current = (Get-ItemProperty "$classes\.lipi" -ErrorAction SilentlyContinue)."(default)"
    if ($current -eq $progId) { Remove-Item "$classes\.lipi" -Recurse -Force }
    if (Test-Path "$classes\$progId") { Remove-Item "$classes\$progId" -Recurse -Force }
    Update-Explorer
}

if ($Uninstall) {
    if (Test-Path (Join-Path $Dest "lipi.exe")) { Remove-Item -Force (Join-Path $Dest "lipi.exe") }
    Get-ChildItem $Dest -Filter "lipi.exe.old-*" -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
    if (-not $NoPath) {
        [Environment]::SetEnvironmentVariable("Path", (($parts | Where-Object { $_ -ne $Dest }) -join ";"), "User")
    }
    if (-not $NoFileTypes) { Unregister-FileTypes }
    Write-Host "LiPi was removed."
    exit 0
}

$exe = Join-Path $here "lipi.exe"
if (-not (Test-Path $exe)) {
    Write-Host "lipi.exe wasn't found next to this script. Unzip the LiPi release first, then run install.cmd from the unzipped folder."
    exit 1
}
New-Item -ItemType Directory -Force $Dest | Out-Null
$installed = Join-Path $Dest "lipi.exe"
# Copies moved aside by earlier updates (see below) can go once nothing uses them.
Get-ChildItem $Dest -Filter "lipi.exe.old-*" -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
try {
    Copy-Item $exe $Dest -Force
} catch {
    # lipi.exe is in use (VS Code's language server or `lipi dev`). Windows
    # lets a running program be renamed, so move it aside and put the new one in place.
    Rename-Item -Path $installed -NewName ("lipi.exe.old-" + [DateTime]::Now.ToString("yyyyMMddHHmmss"))
    Copy-Item $exe $Dest -Force
    Write-Host "LiPi was running; restart VS Code and any lipi dev to use the new version."
}
Unblock-File $installed -ErrorAction SilentlyContinue

$added = $false
if (-not $NoPath -and $parts -notcontains $Dest) {
    [Environment]::SetEnvironmentVariable("Path", (($parts + $Dest) -join ";"), "User")
    $added = $true
}

if (-not $NoFileTypes) {
    Register-FileTypes $installed
    Write-Host ".lipi files now show the LiPi logo."
}

$vsix = Get-ChildItem $here -Filter "lipi-*.vsix" -ErrorAction SilentlyContinue | Select-Object -First 1
if ($vsix -and (Get-Command code -ErrorAction SilentlyContinue)) {
    code --install-extension $vsix.FullName | Out-Null
    Write-Host "Installed the LiPi extension for VS Code."
}

& $installed --version
Write-Host "LiPi is installed in $Dest"
if ($added) {
    Write-Host "Open a new terminal, then try:  lipi new my-app"
} else {
    Write-Host "Try:  lipi new my-app"
}
