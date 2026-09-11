# Builds the Windows release archive, dist\lipi-<version>-windows-x64.zip:
# lipi.exe, the installer, README, LICENSE, docs, examples and the VS Code
# extension, plus a .sha256 checksum file. Run from anywhere:
#   powershell -File scripts\package-windows.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
cargo build --release -p lipi
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$name = "lipi-$version-windows-x64"
$out = Join-Path $root "dist\$name"
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Force $out | Out-Null
Copy-Item target\release\lipi.exe, README.md, LICENSE, scripts\install.ps1, scripts\install.cmd $out
Copy-Item -Recurse docs, examples $out

Push-Location editors\vscode
npx --yes @vscode/vsce package --out "$out" | Out-Null
$vsceOk = $LASTEXITCODE -eq 0
Pop-Location
if (-not $vsceOk) { Write-Host "(the VS Code extension couldn't be packaged; the archive is built without it)" }

$zip = "$out.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path $out -DestinationPath $zip
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
"$hash  $name.zip" | Set-Content -Encoding ascii "$zip.sha256"
Write-Host "Built $zip"
Write-Host "SHA-256 $hash"
