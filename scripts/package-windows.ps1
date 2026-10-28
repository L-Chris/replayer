param(
    [string]$Binary = 'target/release/replayer.exe',
    [string]$OutputDirectory = 'dist',
    [string]$Iscc = '',
    [string]$RuntimeDirectory = $env:REMDIA_RUNTIME_DIR,
    [switch]$SkipInstaller
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $metadata = cargo metadata --no-deps --format-version 1 --locked | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed.' }
    $version = ($metadata.packages | Where-Object name -eq 'replayer').version
    if ($version -notmatch '^\d+\.\d+\.\d+$') { throw 'Windows packages require a stable major.minor.patch version.' }
    if (-not (Test-Path -LiteralPath $Binary)) { throw "Missing binary: $Binary" }
    $output = [IO.Path]::GetFullPath((Join-Path $root $OutputDirectory))
    $stage = Join-Path $output ('stage-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $stage -Force | Out-Null
    # Whitelist package contents: never copy the working tree, .env, settings or media.
    Copy-Item -LiteralPath $Binary -Destination (Join-Path $stage 'replayer.exe')
    Copy-Item -LiteralPath 'assets/branding/replayer.ico' -Destination $stage
    # Explorer caches icons by path. A content-addressed filename changes only
    # when the artwork does, including same-version installer rebuilds.
    $iconDigest = (Get-FileHash -LiteralPath 'assets/branding/replayer.ico' -Algorithm SHA256).Hash.ToLowerInvariant()
    $iconName = 'replayer-' + $iconDigest.Substring(0, 16) + '.ico'
    $iconDirectory = Join-Path $stage 'icons'
    New-Item -ItemType Directory -Path $iconDirectory -Force | Out-Null
    Copy-Item -LiteralPath 'assets/branding/replayer.ico' -Destination (Join-Path $iconDirectory $iconName)
    if (-not $RuntimeDirectory) { $RuntimeDirectory = Join-Path $root '.deps/remdia-source/dist/remdia' }
    if (-not (Test-Path (Join-Path $RuntimeDirectory 'plugins/native.json'))) { throw 'Run setup-windows.ps1 to build remdia components first.' }
    $cargo = Get-Content (Join-Path $root 'Cargo.toml') -Raw
    if ($cargo -notmatch '(?m)^remdia\s*=\s*\{[^}]*rev\s*=\s*"([a-f0-9]{40})"') { throw 'Missing pinned remdia revision.' }
    if ((Get-Content (Join-Path $RuntimeDirectory 'REVISION') -Raw).Trim() -cne $Matches[1]) { throw 'Runtime revision does not match Cargo.toml.' }
    $plugins = Join-Path $stage 'plugins'
    New-Item -ItemType Directory -Force $plugins | Out-Null
    foreach ($name in @('native', 'windows', 'image', 'system_audio', 'system_demux', 'fallback_demux')) {
        foreach ($filename in @("$name.json", "remdia_backend_$name.dll")) {
            Copy-Item -LiteralPath (Join-Path $RuntimeDirectory "plugins/$filename") -Destination $plugins
        }
    }
    if (Test-Path (Join-Path $RuntimeDirectory 'licenses')) {
        Copy-Item -LiteralPath (Join-Path $RuntimeDirectory 'licenses') -Destination (Join-Path $stage 'remdia-licenses') -Recurse
    }
    Copy-Item -LiteralPath (Join-Path $RuntimeDirectory 'THIRD_PARTY.md') -Destination (Join-Path $stage 'remdia-THIRD-PARTY.md')
    Copy-Item -LiteralPath (Join-Path $RuntimeDirectory 'REVISION') -Destination (Join-Path $stage 'remdia-REVISION')
    python scripts/audit-native-windows.py --directory $stage
    if ($LASTEXITCODE -ne 0) { throw 'Windows dependency audit failed.' }
    Copy-Item -LiteralPath 'README.md' -Destination $stage
    $userDocs = Join-Path $stage 'docs'
    New-Item -ItemType Directory -Path $userDocs -Force | Out-Null
    foreach ($name in @('guide.md', 'third-party.md')) {
        Copy-Item -LiteralPath (Join-Path 'docs' $name) -Destination $userDocs
    }
    Copy-Item -LiteralPath 'licenses' -Destination $stage -Recurse
    $zip = Join-Path $output "replayer-$version-windows-x86_64.zip"
    Compress-Archive -LiteralPath @(Get-ChildItem -LiteralPath $stage -Force | ForEach-Object FullName) -DestinationPath $zip -Force
    if (-not $SkipInstaller) {
        if (-not $Iscc) {
            $Iscc = @('C:/Program Files (x86)/Inno Setup 6/ISCC.exe','C:/Program Files/Inno Setup 6/ISCC.exe','C:/Program Files (x86)/Inno Setup 7/ISCC.exe') | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
        }
        if (-not $Iscc) { throw 'Inno Setup compiler not found; install Inno Setup or pass -Iscc.' }
        & $Iscc "/DAppVersion=$version" "/DSourceDir=$stage" "/DOutputDir=$output" "/DAppIconFile=$iconName" 'installer/replayer.iss'
        if ($LASTEXITCODE -ne 0) { throw 'Installer build failed.' }
    }
    Get-ChildItem -LiteralPath $output -File | Where-Object { $_.Name -like "replayer-$version-windows-x86_64*" -and $_.Extension -in @('.zip','.exe') } | ForEach-Object {
        $digest = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        "$digest  $($_.Name)"
    } | Set-Content -LiteralPath (Join-Path $output 'SHA256SUMS.txt') -Encoding ascii
    Write-Host "Packaged replayer $version in $output"
} finally { Pop-Location }
