param([string]$RuntimeArchive = '', [switch]$WithTestTools)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$dependencies = Join-Path $root '.deps'
New-Item -ItemType Directory -Force -Path $dependencies | Out-Null
function Get-FullFFmpeg {
    $archive = Join-Path $dependencies 'ffmpeg-ci.zip'
    $destination = Join-Path $dependencies 'ffmpeg-ci'
    $expected = '74e35908a759dbebca48c1f382ea0366f3d9d270e4bf4660503803052896b807'
    $url = 'https://github.com/L-Chris/replayer/releases/download/deps-ffmpeg-9.0.2/ffmpeg-ci.zip'
    if (-not (Test-Path -LiteralPath $archive) -or (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $expected) {
        Invoke-WebRequest -Uri $url -OutFile $archive
    }
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $expected) { throw 'FFmpeg checksum mismatch.' }
    if (-not (Test-Path -LiteralPath $destination)) { Expand-Archive -LiteralPath $archive -DestinationPath $destination }
    $full = Get-ChildItem -LiteralPath $destination -Directory | Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'include/libavcodec/avcodec.h') } | Select-Object -First 1
    if (-not $full) { throw 'FFmpeg development files are missing.' }
    return $full
}
$localRuntime = Join-Path $root 'target/windows-runtime/ffmpeg-playback.zip'
if (-not $RuntimeArchive -and (Test-Path -LiteralPath $localRuntime)) { $RuntimeArchive = $localRuntime }
if ($RuntimeArchive) {
    $archive = (Resolve-Path -LiteralPath $RuntimeArchive).Path
    $manifest = Join-Path (Split-Path -Parent $archive) 'SHA256SUMS.txt'
    $line = (Get-Content -LiteralPath $manifest -Raw).Trim()
    if ($line -cnotmatch '^([a-f0-9]{64})  ffmpeg-playback\.zip$') { throw 'Invalid runtime checksum manifest.' }
    $expected = $Matches[1]
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -cne $expected) { throw 'Playback runtime checksum mismatch.' }
    $destination = Join-Path $dependencies 'ffmpeg-playback'
    $marker = Join-Path $destination 'archive-sha256.txt'
    if (-not (Test-Path -LiteralPath $marker) -or (Get-Content -LiteralPath $marker -Raw).Trim() -cne $expected) {
        if (Test-Path -LiteralPath $destination) { Remove-Item -LiteralPath $destination -Recurse -Force }
        Expand-Archive -LiteralPath $archive -DestinationPath $destination
        Set-Content -LiteralPath $marker -Value $expected -Encoding ascii
    }
    $ffmpeg = Get-Item -LiteralPath $destination
    if (-not (Test-Path -LiteralPath (Join-Path $ffmpeg.FullName 'include/libavcodec/avcodec.h'))) { throw 'Playback runtime development headers are missing.' }
} else {
    # Preserve the verified full runtime for local bootstrap without Docker.
    $ffmpeg = Get-FullFFmpeg
    $env:REPLAYER_TEST_FFMPEG = Join-Path $ffmpeg.FullName 'bin/ffmpeg.exe'
    Write-Host 'Using the full legacy runtime for development. Packaging requires -RuntimeArchive from the windows-ffmpeg-runtime CI artifact.'
}
if ($WithTestTools) {
    $full = Get-FullFFmpeg
    $env:REPLAYER_TEST_FFMPEG = Join-Path $full.FullName 'bin/ffmpeg.exe'
}

$clang = @('C:/Program Files/LLVM/bin', (Join-Path $dependencies 'libclang')) | Where-Object { Test-Path -LiteralPath (Join-Path $_ 'libclang.dll') } | Select-Object -First 1
if (-not $clang) { throw 'Install LLVM with libclang.dll before building.' }
$env:FFMPEG_DIR = $ffmpeg.FullName
$env:LIBCLANG_PATH = $clang
$env:PATH = (Join-Path $ffmpeg.FullName 'bin') + ';' + $env:PATH
if ($env:GITHUB_ENV) {
    "FFMPEG_DIR=$env:FFMPEG_DIR" >> $env:GITHUB_ENV
    "LIBCLANG_PATH=$env:LIBCLANG_PATH" >> $env:GITHUB_ENV
    if ($env:REPLAYER_TEST_FFMPEG) { "REPLAYER_TEST_FFMPEG=$env:REPLAYER_TEST_FFMPEG" >> $env:GITHUB_ENV }
    (Join-Path $ffmpeg.FullName 'bin') >> $env:GITHUB_PATH
}
Write-Host 'Verified FFmpeg 9.0.2 and configured Windows build dependencies.'
