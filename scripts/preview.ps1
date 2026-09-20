param(
    [string]$Media,
    [switch]$NoLaunch
)
$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Push-Location $repoRoot
try {
    $metadata = cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed.' }
    $previewExe = [IO.Path]::GetFullPath((Join-Path $metadata.target_directory 'debug/replayer.exe'))
    $mediaPath = $null
    if ($Media) { $mediaPath = (Resolve-Path -LiteralPath $Media).Path }

    # Only replace this checkout's debug preview, never an installed/release app.
    foreach ($previewProcess in @(Get-Process -Name replayer -ErrorAction SilentlyContinue)) {
        if ($previewProcess.Path -and [string]::Equals($previewProcess.Path, $previewExe, [StringComparison]::OrdinalIgnoreCase)) {
            $null = $previewProcess.CloseMainWindow()
            if (-not $previewProcess.WaitForExit(5000)) {
                $previewProcess.Kill()
                $previewProcess.WaitForExit()
            }
        }
    }
    cargo build --locked --bin replayer
    if ($LASTEXITCODE -ne 0) { throw 'Preview build failed; no executable was launched.' }
    if (-not $NoLaunch) {
        $start = New-Object System.Diagnostics.ProcessStartInfo
        $start.FileName = $previewExe
        $start.WorkingDirectory = $repoRoot
        $start.UseShellExecute = $false
        $start.CreateNoWindow = $true
        if ($mediaPath) { $start.Arguments = '"' + $mediaPath + '"' }
        $previewProcess = [Diagnostics.Process]::Start($start)
        Write-Host "Preview: $previewExe (PID $($previewProcess.Id))"
    } else {
        Write-Host "Preview built: $previewExe"
    }
} finally { Pop-Location }
