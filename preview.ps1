param(
    [string]$File,
    [switch]$Release,
    [switch]$Stats
)

Set-Location $PSScriptRoot

Get-Process pordo -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

$profileArg = if ($Release) { @('--release') } else { @() }
$subdir = if ($Release) { 'release' } else { 'debug' }

Write-Host "building ($subdir)..."
cargo build @profileArg
if ($LASTEXITCODE -ne 0) { Write-Host "build failed" -ForegroundColor Red; exit 1 }

$exe = Join-Path $PSScriptRoot "target\$subdir\pordo.exe"

$target = $File
if (-not $target) {
    $sample = Join-Path $env:TEMP 'pordo_av.mp4'
    if (Test-Path $sample) { $target = $sample }
}

if ($Stats) { $env:PORDO_STATS = '1' } else { Remove-Item Env:\PORDO_STATS -ErrorAction SilentlyContinue }

if ($target) { Start-Process -FilePath $exe -ArgumentList "`"$target`"" }
else { Start-Process -FilePath $exe }

Write-Host "launched: $exe $(if ($target) { $target })" -ForegroundColor Green
