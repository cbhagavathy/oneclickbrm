# Build release heartbeat-client on Windows and package to dist\.
# Run in PowerShell from the repo root: .\scripts\release\build-client-windows.ps1
# Requires: Rust (cargo) on PATH.
#
# Output: dist\heartbeat-client-<version>-windows-x86_64.zip

$ErrorActionPreference = "Stop"

function Get-ClientVersion {
    $toml = Join-Path $PSScriptRoot "..\..\heartbeat-client\Cargo.toml"
    $line = Get-Content $toml | Where-Object { $_ -match '^\s*version\s*=' } | Select-Object -First 1
    if (-not $line) { throw "Could not read version from $toml" }
    return ($line -replace '.*=\s*"([^"]+)".*', '$1').Trim()
}

$Root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
Set-Location $Root

$ver = Get-ClientVersion
$dist = Join-Path $Root "dist"
New-Item -ItemType Directory -Force -Path $dist | Out-Null

Write-Host "Building heartbeat-client $ver for Windows x86_64..."
& cargo build --release -p heartbeat-client
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$bin = Join-Path $Root "target\release\heartbeat-client.exe"
if (-not (Test-Path $bin)) {
    throw "Expected binary missing: $bin"
}

$stage = Join-Path $dist "stage-windows-x86_64"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force -Path $stage | Out-Null

Copy-Item $bin (Join-Path $stage "heartbeat-client.exe")

$installTxt = Join-Path $stage "INSTALL.txt"
@"
Heartbeat client $ver (Windows x86_64)

Run heartbeat-client.exe from this folder (double-click or Command Prompt).

Environment (optional):
  HEARTBEAT_URL     Server heartbeat URL
  HEARTBEAT_INTERVAL_SECS  Seconds between heartbeats
  CLIENT_ID         Pre-registered client id
  OPEN_BROWSER=0    Do not open the browser on start

Example:
  heartbeat-client.exe --heartbeat-url https://your-server.example/heartbeat

Support: see your product documentation.
"@ | Set-Content -Encoding UTF8 $installTxt

$bundle = "heartbeat-client-$ver-windows-x86_64"
$zip = Join-Path $dist "$bundle.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip -Force
Remove-Item -Recurse -Force $stage

Write-Host "Built: $zip"

# SHA256 (PowerShell 4+)
$hash = Get-FileHash -Algorithm SHA256 $zip
$sums = Join-Path $dist "SHA256SUMS-windows.txt"
Add-Content -Path $sums -Value "$($hash.Hash.ToLower())  $(Split-Path $zip -Leaf)"
