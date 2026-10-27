param(
    [string]$Binary = 'target/release/replayer.exe',
    [string]$OutputDirectory = 'dist',
    [string]$Iscc = '',
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
    $ffmpeg = $env:FFMPEG_DIR
    if (-not $ffmpeg) { $ffmpeg = Join-Path $root '.deps/ffmpeg-playback' }
    $listing = @(& (Join-Path $ffmpeg 'bin/ffmpeg.exe') -hide_banner -encoders)
    if ($LASTEXITCODE -ne 0) { throw 'Could not inspect the playback runtime encoders.' }
    $encoders = @($listing | ForEach-Object {
        if ($_ -cmatch '^\s*[VAS][.A-Z]{5}\s+([a-z0-9_-]+)\s') { $Matches[1] }
    })
    if ($encoders.Count -ne 1 -or $encoders[0] -cne 'libmp3lame') {
        throw 'Packages require the playback-only runtime with only libmp3lame encoding. Run setup-windows.ps1 -RuntimeArchive first.'
    }
    $dlls = @(& "$PSScriptRoot/ffmpeg-runtime.ps1" -Binary $Binary -RuntimeDirectory (Join-Path $ffmpeg 'bin'))
    $dlls | Copy-Item -Destination $stage
    Copy-Item -LiteralPath 'README.md' -Destination $stage
    $userDocs = Join-Path $stage 'docs'
    New-Item -ItemType Directory -Path $userDocs -Force | Out-Null
    foreach ($name in @('guide.md', 'third-party.md')) {
        Copy-Item -LiteralPath (Join-Path 'docs' $name) -Destination $userDocs
    }
    Copy-Item -LiteralPath 'licenses' -Destination $stage -Recurse
    Copy-Item -LiteralPath (Join-Path $ffmpeg 'LICENSE.txt') -Destination (Join-Path $stage 'FFmpeg-LICENSE.txt')
    if (Test-Path -LiteralPath (Join-Path $ffmpeg 'BUILD.txt')) {
        Copy-Item -LiteralPath (Join-Path $ffmpeg 'BUILD.txt') -Destination (Join-Path $stage 'FFmpeg-BUILD.txt')
        $recipes = Join-Path $stage 'ffmpeg-build'
        New-Item -ItemType Directory -Path $recipes -Force | Out-Null
        foreach ($name in @('build-windows-runtime.sh', 'ffmpeg-playback-options.sh', 'verify-ffmpeg-config.py')) {
            Copy-Item -LiteralPath (Join-Path $ffmpeg $name) -Destination $recipes
        }
    }
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
