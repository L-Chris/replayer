$ErrorActionPreference = 'Stop'
$logPath = Join-Path (Split-Path -Parent $env:REPLAYER_UPDATE_INSTALLER) 'install.log'
try {
    $parent = Get-Process -Id ([int]$env:REPLAYER_UPDATE_PID) -ErrorAction SilentlyContinue
    if ($null -ne $parent -and -not $parent.WaitForExit(60000)) { throw 'Player did not exit; update cancelled.' }
    $actual = (Get-FileHash -LiteralPath $env:REPLAYER_UPDATE_INSTALLER -Algorithm SHA256).Hash
    if ($actual -ne $env:REPLAYER_UPDATE_SHA256) { throw 'Installer SHA-256 mismatch.' }
    # Paths arrive through environment variables, never interpolated PowerShell code.
    foreach ($value in @($env:REPLAYER_UPDATE_DIR, $logPath)) {
        if ($value.Contains('"')) { throw 'Invalid path.' }
    }
    $arguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/NOCLOSEAPPLICATIONS',
        ('/DIR="' + $env:REPLAYER_UPDATE_DIR + '"'), ('/LOG="' + $logPath + '"'))
    $installer = Start-Process -FilePath $env:REPLAYER_UPDATE_INSTALLER -ArgumentList $arguments -WindowStyle Hidden -Wait -PassThru
    if ($installer.ExitCode -ne 0) { throw "Installer failed with exit code $($installer.ExitCode). See install.log." }
    Start-Process -FilePath $env:REPLAYER_UPDATE_RELAUNCH -WorkingDirectory $env:REPLAYER_UPDATE_DIR -WindowStyle Hidden
} catch {
    $_ | Out-String | Set-Content -LiteralPath ($logPath + '.error.txt') -Encoding utf8
    # The old installation remains usable if installation did not start.
    if (Test-Path -LiteralPath $env:REPLAYER_UPDATE_RELAUNCH) {
        Start-Process -FilePath $env:REPLAYER_UPDATE_RELAUNCH -WorkingDirectory $env:REPLAYER_UPDATE_DIR -WindowStyle Hidden
    }
    exit 1
}
