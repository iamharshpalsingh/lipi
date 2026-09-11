# Builds the LiPi playground: one self-contained page, dist\lipi-playground.html,
# with the compiler (WebAssembly) inside it. Open it directly or host it anywhere.
#   powershell -File scripts\build-playground.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

cargo build -p lipi_web --target wasm32-unknown-unknown --release
if ($LASTEXITCODE -ne 0) { throw "building the WebAssembly compiler failed" }
$wasm = Join-Path $root "target\wasm32-unknown-unknown\release\lipi_web.wasm"
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value

$utf8 = New-Object System.Text.UTF8Encoding($false)
$template = [IO.File]::ReadAllText((Join-Path $root "playground\template.html"), $utf8)
$b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($wasm))
$html = $template.Replace("/*__WASM__*/", $b64).Replace("/*__VERSION__*/", $version)
New-Item -ItemType Directory -Force (Join-Path $root "dist") | Out-Null
$out = Join-Path $root "dist\lipi-playground.html"
[IO.File]::WriteAllText($out, $html, $utf8)
Write-Host "Built $out ($([math]::Round((Get-Item $out).Length / 1KB)) KB; compiler $([math]::Round((Get-Item $wasm).Length / 1KB)) KB)"
