param(
    [string]$Media,
    [switch]$NoLaunch,
    [switch]$Stats
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
    . "$PSScriptRoot/setup-windows.ps1"
    cargo build --locked --bin replayer
    if ($LASTEXITCODE -ne 0) { throw 'Preview build failed; no executable was launched.' }
    $plugins = Join-Path (Split-Path -Parent $previewExe) 'plugins'
    New-Item -ItemType Directory -Force $plugins | Out-Null
    Copy-Item "$env:REMDIA_PLUGIN_DIR/*" $plugins -Force
    if (-not $NoLaunch) {
        # UseShellExecute detaches the child from this shell's pipe handles;
        # otherwise the launched app keeps stdout open and callers block forever.
        $start = New-Object System.Diagnostics.ProcessStartInfo
        $start.FileName = $previewExe
        $start.WorkingDirectory = $repoRoot
        $start.UseShellExecute = $true
        if ($mediaPath) { $start.Arguments = '"' + $mediaPath + '"' }
        if ($Stats) {
            $previousStats = $env:REPLAYER_STATS
            try {
                $env:REPLAYER_STATS = '1'
                $previewArguments = @{FilePath=$previewExe; WorkingDirectory=$repoRoot; PassThru=$true; WindowStyle='Hidden';
                    RedirectStandardOutput=(Join-Path $metadata.target_directory 'preview-stdout.log');
                    RedirectStandardError=(Join-Path $metadata.target_directory 'preview-stderr.log')}
                if ($mediaPath) { $previewArguments.ArgumentList = '"' + $mediaPath + '"' }
                $previewProcess = Start-Process @previewArguments
            } finally { $env:REPLAYER_STATS = $previousStats }
            Write-Host "Playback diagnostics: $(Join-Path $metadata.target_directory 'preview-stderr.log')"
        } else {
            $previewProcess = [Diagnostics.Process]::Start($start)
        }
        Write-Host "Preview: $previewExe (PID $($previewProcess.Id))"
    } else {
        Write-Host "Preview built: $previewExe"
    }
} finally { Pop-Location }
