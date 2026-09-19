param(
    [string]$File,
    [switch]$Release,
    [switch]$Stats
)

Set-Location $PSScriptRoot

Get-Process replayer -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

$profileArg = if ($Release) { @('--release') } else { @() }
$subdir = if ($Release) { 'release' } else { 'debug' }

Write-Host "building ($subdir)..."
cargo build @profileArg
if ($LASTEXITCODE -ne 0) { Write-Host "build failed" -ForegroundColor Red; exit 1 }

$exe = Join-Path $PSScriptRoot "target\$subdir\replayer.exe"

$target = $File
if (-not $target) {
    $sample = Join-Path $env:TEMP 'replayer_av.mp4'
    if (Test-Path $sample) { $target = $sample }
}

if ($Stats) { $env:REPLAYER_STATS = '1' } else { Remove-Item Env:\REPLAYER_STATS -ErrorAction SilentlyContinue }

if ($target) { Start-Process -FilePath $exe -ArgumentList "`"$target`"" }
else { Start-Process -FilePath $exe }

Write-Host "launched: $exe $(if ($target) { $target })" -ForegroundColor Green
