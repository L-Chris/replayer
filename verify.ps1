param([switch]$Software, [switch]$Stress)
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
$ffmpeg = Join-Path $PSScriptRoot '.deps/ffmpeg-n9.0.1-84-g946fcce07b-win64-lgpl-shared-9.0/bin/ffmpeg.exe'
if (-not (Test-Path -LiteralPath $ffmpeg)) { throw 'Project FFmpeg binary is missing.' }
$media = Join-Path $PSScriptRoot 'target/test-media'
New-Item -ItemType Directory -Path $media -Force | Out-Null
function Make-Media([string]$Name, [string[]]$Arguments) {
    & $ffmpeg -hide_banner -loglevel error -y @Arguments (Join-Path $media $Name)
    if ($LASTEXITCODE -ne 0) { throw "FFmpeg failed: $Name" }
}
Make-Media 'silent.mp4' @('-f','lavfi','-i','testsrc2=size=640x360:rate=30:duration=4','-c:v','mpeg4','-g','90','-bf','2','-q:v','4')
Make-Media 'audio-short.mp4' @('-f','lavfi','-i','testsrc2=size=640x360:rate=30:duration=4','-f','lavfi','-i','sine=frequency=440:sample_rate=44100:duration=2','-c:v','mpeg4','-g','90','-bf','2','-c:a','aac')
Make-Media 'video-short.mp4' @('-f','lavfi','-i','testsrc2=size=640x360:rate=30:duration=2','-f','lavfi','-i','sine=frequency=440:sample_rate=44100:duration=4','-c:v','mpeg4','-g','90','-bf','2','-c:a','aac')
Make-Media 'h264.mp4' @('-f','lavfi','-i','testsrc2=size=1920x1080:rate=30:duration=4','-f','lavfi','-i','sine=frequency=440:sample_rate=44100:duration=4','-c:v','libopenh264','-g','90','-c:a','aac')
Make-Media '4k.mp4' @('-f','lavfi','-i','testsrc2=size=3840x2160:rate=30:duration=3','-f','lavfi','-i','sine=frequency=440:sample_rate=44100:duration=3','-c:v','libopenh264','-g','90','-c:a','aac')
Make-Media 'audio-delayed.mp4' @('-f','lavfi','-i','testsrc2=size=640x360:rate=30:duration=4','-itsoffset','1','-f','lavfi','-i','sine=frequency=440:sample_rate=44100:duration=3','-c:v','mpeg4','-bf','2','-c:a','aac')
$cases = @('silent.mp4','audio-short.mp4','video-short.mp4','h264.mp4','4k.mp4','audio-delayed.mp4')
if ($Stress) {
    Make-Media 'large-short-audio.mp4' @('-f','lavfi','-i','testsrc2=size=3840x2160:rate=30:duration=12','-f','lavfi','-i','sine=frequency=440:sample_rate=44100:duration=0.5','-c:v','mpeg4','-q:v','1','-g','30','-c:a','aac')
    $cases += 'large-short-audio.mp4'
}
# A separate executable allows checking while the regular player is running.
cargo rustc --locked --bin replayer -- -o target/debug/replayer-verify.exe
if ($LASTEXITCODE -ne 0) { throw 'Build failed.' }
$previousSoftware = $env:REPLAYER_SOFTWARE
try {
    if ($Software) { $env:REPLAYER_SOFTWARE = '1' }
    foreach ($name in $cases) {
        & ./target/debug/replayer-verify.exe --selftest (Join-Path $media $name)
        if ($LASTEXITCODE -ne 0) { throw "Regression failed: $name" }
    }
} finally {
    if ($null -eq $previousSoftware) { Remove-Item Env:REPLAYER_SOFTWARE -ErrorAction SilentlyContinue }
    else { $env:REPLAYER_SOFTWARE = $previousSoftware }
}
