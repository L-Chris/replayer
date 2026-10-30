param([string]$RuntimeDirectory = '', [switch]$RebuildRuntime)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$cargo = Get-Content (Join-Path $root 'Cargo.toml') -Raw
if ($cargo -notmatch '(?m)^remdia\s*=\s*\{[^}]*rev\s*=\s*"([a-f0-9]{40})"') { throw 'Cargo.toml must pin a complete remdia revision.' }
$revision = $Matches[1]
if (-not $RuntimeDirectory) {
    if ($env:REMDIA_RUNTIME_DIR) { $RuntimeDirectory = $env:REMDIA_RUNTIME_DIR }
    else { $RuntimeDirectory = Join-Path $root '.deps/remdia-source/dist/remdia' }
}
$marker = Join-Path $RuntimeDirectory 'REVISION'
$ready = (Test-Path $marker) -and (Get-Content $marker -Raw).Trim() -ceq $revision
if ($RebuildRuntime -or -not $ready) {
    $source = Join-Path $root '.deps/remdia-source'
    if (-not (Test-Path (Join-Path $source '.git'))) {
        New-Item -ItemType Directory -Force (Split-Path -Parent $source) | Out-Null
        git clone --no-checkout https://github.com/L-Chris/remdia.git $source
        if ($LASTEXITCODE -ne 0) { throw 'Cannot fetch remdia source.' }
    }
    git -C $source fetch --depth 1 origin $revision
    if ($LASTEXITCODE -ne 0) { throw 'Cannot fetch pinned remdia revision.' }
    git -C $source checkout --detach $revision
    if ($LASTEXITCODE -ne 0) { throw 'Cannot select pinned remdia revision.' }
    & (Join-Path $source 'scripts/build-windows.ps1') -Release
    $RuntimeDirectory = Join-Path $source 'dist/remdia'
}
$layout = Get-Content (Join-Path $RuntimeDirectory 'BUNDLED.json') -Raw | ConvertFrom-Json
if ($layout.layout -ne 'unified' -or $layout.abi_major -ne 2 -or $layout.os -ne 'windows' -or $layout.arch -ne 'x86_64') { throw 'Incompatible bundled remdia runtime.' }
$env:REMDIA_RUNTIME_DIR = (Resolve-Path $RuntimeDirectory).Path
$env:REMDIA_PLUGIN_DIR = Join-Path $env:REMDIA_RUNTIME_DIR 'plugins'
if ($env:GITHUB_ENV) {
    "REMDIA_RUNTIME_DIR=$env:REMDIA_RUNTIME_DIR" >> $env:GITHUB_ENV
    "REMDIA_PLUGIN_DIR=$env:REMDIA_PLUGIN_DIR" >> $env:GITHUB_ENV
}
Write-Host "Using FFmpeg-free remdia components at $env:REMDIA_RUNTIME_DIR ($revision)."
