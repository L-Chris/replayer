param([switch]$Software, [switch]$Stress)
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
./scripts/preview.ps1 -NoLaunch
if ($LASTEXITCODE -ne 0) { throw 'Build failed.' }
$fixtures = $env:REPLAYER_NATIVE_FIXTURES
if (-not $fixtures) { $fixtures = Join-Path $PSScriptRoot '.deps/remdia-source/tests/fixtures' }
$metadata = cargo metadata --no-deps --locked --format-version 1 | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed.' }
$executable = Join-Path $metadata.target_directory 'debug/replayer.exe'
$previousSoftware = $env:REPLAYER_SOFTWARE
try {
    if ($Software) { $env:REPLAYER_SOFTWARE = '1' }
    $count = if ($Stress) { 25 } else { 1 }
    for ($index = 0; $index -lt $count; $index++) {
        foreach ($name in @('av.mp4','av.mkv')) {
            & $executable --selftest (Join-Path $fixtures $name)
            if ($LASTEXITCODE -ne 0) { throw "Playback regression failed: $name" }
        }
    }
} finally {
    if ($null -eq $previousSoftware) { Remove-Item Env:REPLAYER_SOFTWARE -ErrorAction SilentlyContinue }
    else { $env:REPLAYER_SOFTWARE = $previousSoftware }
}
