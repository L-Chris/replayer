$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$dependencies = Join-Path $root '.deps'
$archive = Join-Path $dependencies 'ffmpeg-ci.zip'
$destination = Join-Path $dependencies 'ffmpeg-ci'
$expected = '74e35908a759dbebca48c1f382ea0366f3d9d270e4bf4660503803052896b807'
$url = 'https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-09-19-13-11/ffmpeg-n9.0.2-win64-lgpl-shared-9.0.zip'
New-Item -ItemType Directory -Force -Path $dependencies | Out-Null
if (-not (Test-Path -LiteralPath $archive) -or (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $expected) {
    Invoke-WebRequest -Uri $url -OutFile $archive
}
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $expected) { throw 'FFmpeg checksum mismatch.' }
if (-not (Test-Path -LiteralPath $destination)) { Expand-Archive -LiteralPath $archive -DestinationPath $destination }
$ffmpeg = Get-ChildItem -LiteralPath $destination -Directory | Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'include/libavcodec/avcodec.h') } | Select-Object -First 1
if (-not $ffmpeg) { throw 'FFmpeg development files are missing.' }
$clang = @('C:/Program Files/LLVM/bin', (Join-Path $dependencies 'libclang')) | Where-Object { Test-Path -LiteralPath (Join-Path $_ 'libclang.dll') } | Select-Object -First 1
if (-not $clang) { throw 'Install LLVM with libclang.dll before building.' }
$env:FFMPEG_DIR = $ffmpeg.FullName
$env:LIBCLANG_PATH = $clang
$env:PATH = (Join-Path $ffmpeg.FullName 'bin') + ';' + $env:PATH
if ($env:GITHUB_ENV) {
    "FFMPEG_DIR=$env:FFMPEG_DIR" >> $env:GITHUB_ENV
    "LIBCLANG_PATH=$env:LIBCLANG_PATH" >> $env:GITHUB_ENV
    (Join-Path $ffmpeg.FullName 'bin') >> $env:GITHUB_PATH
}
Write-Host 'Verified FFmpeg 9.0.2 and configured Windows build dependencies.'
