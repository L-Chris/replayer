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
    $ffmpeg = $env:FFMPEG_DIR
    if (-not $ffmpeg) { $ffmpeg = Join-Path $root '.deps/ffmpeg-n9.0.1-84-g946fcce07b-win64-lgpl-shared-9.0' }
    $dlls = @(Get-ChildItem -LiteralPath (Join-Path $ffmpeg 'bin') -Filter '*.dll')
    if ($dlls.Count -lt 6) { throw 'FFmpeg runtime DLLs are missing.' }
    $dlls | Copy-Item -Destination $stage
    Copy-Item -LiteralPath 'README.md' -Destination $stage
    Copy-Item -LiteralPath 'SUBTITLES.md' -Destination $stage
    Copy-Item -LiteralPath '.env.example' -Destination $stage
    Copy-Item -LiteralPath 'THIRD_PARTY.md' -Destination $stage
    Copy-Item -LiteralPath (Join-Path $ffmpeg 'LICENSE.txt') -Destination (Join-Path $stage 'FFmpeg-LICENSE.txt')
    $zip = Join-Path $output "replayer-$version-windows-x86_64.zip"
    Compress-Archive -LiteralPath @(Get-ChildItem -LiteralPath $stage -Force | ForEach-Object FullName) -DestinationPath $zip -Force
    if (-not $SkipInstaller) {
        if (-not $Iscc) {
            $Iscc = @('C:/Program Files (x86)/Inno Setup 6/ISCC.exe','C:/Program Files/Inno Setup 6/ISCC.exe','C:/Program Files (x86)/Inno Setup 7/ISCC.exe') | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
        }
        if (-not $Iscc) { throw 'Inno Setup compiler not found; install Inno Setup or pass -Iscc.' }
        & $Iscc "/DAppVersion=$version" "/DSourceDir=$stage" "/DOutputDir=$output" 'installer/replayer.iss'
        if ($LASTEXITCODE -ne 0) { throw 'Installer build failed.' }
    }
    Get-ChildItem -LiteralPath $output -File | Where-Object { $_.Name -like "replayer-$version-windows-x86_64*" -and $_.Extension -in @('.zip','.exe') } | ForEach-Object {
        $digest = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        "$digest  $($_.Name)"
    } | Set-Content -LiteralPath (Join-Path $output 'SHA256SUMS.txt') -Encoding ascii
    Write-Host "Packaged replayer $version in $output"
} finally { Pop-Location }
