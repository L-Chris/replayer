param(
    [Parameter(Mandatory)][string]$Installer,
    [Parameter(Mandatory)][string]$InstallDirectory
)
# Run only in the isolated Windows CI account: installs twice to check upgrades.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$sourceIcon = Join-Path $root 'assets/branding/replayer.ico'
$digest = (Get-FileHash -LiteralPath $sourceIcon -Algorithm SHA256).Hash.ToLowerInvariant()
$expectedIcon = Join-Path $InstallDirectory ('icons/replayer-' + $digest.Substring(0, 16) + '.ico')
$executable = Join-Path $InstallDirectory 'replayer.exe'
$shell = New-Object -ComObject WScript.Shell
$pinnedDirectory = Join-Path $env:APPDATA 'Microsoft/Internet Explorer/Quick Launch/User Pinned/TaskBar'
New-Item -ItemType Directory -Path $pinnedDirectory -Force | Out-Null
$fixtureID = [Guid]::NewGuid().ToString('N')
$pinnedLink = Join-Path $pinnedDirectory "replayer-icon-test-$fixtureID.lnk"
$unrelatedLink = Join-Path $pinnedDirectory "unrelated-icon-test-$fixtureID.lnk"
$desktopLink = Join-Path ([Environment]::GetFolderPath('DesktopDirectory')) 'replayer.lnk'
$startLink = Join-Path ([Environment]::GetFolderPath('Programs')) 'replayer/replayer.lnk'
$retiredCodecFiles = @('plugins/image_webp.json', 'plugins/remdia_backend_image_webp.dll',
    'remdia-licenses/image-webp-MIT.txt', 'remdia-licenses/image-webp-Apache-2.0.txt',
    'plugins/fallback_vorbis.json', 'plugins/remdia_backend_fallback_vorbis.dll',
    'plugins/fallback_mp3_lame.json', 'plugins/remdia_backend_fallback_mp3_lame.dll',
    'remdia-licenses/LAME-COPYING.txt', 'remdia-licenses/mp3lame-encoder-LGPL-3.0.txt')
$unrelatedPluginFile = Join-Path $InstallDirectory 'plugins/upgrade-test.keep'

function Assert-LinkIcon([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { throw "Missing shortcut: $Path" }
    $link = $shell.CreateShortcut($Path)
    if ($link.TargetPath -ine $executable) { throw "Shortcut target changed: $Path" }
    $location = $link.IconLocation
    $separator = $location.LastIndexOf(',')
    if ($separator -lt 0 -or $location.Substring($separator + 1).Trim() -ne '0' -or
        $location.Substring(0, $separator).Trim('"') -ine $expectedIcon) {
        throw "Shortcut still has the wrong icon: $Path -> $location"
    }
}
function Install-Fixture([string]$Tasks) {
    $arguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART',
        ('/DIR="' + $InstallDirectory + '"'), ('/TASKS=' + $Tasks))
    $process = Start-Process -FilePath $Installer -ArgumentList $arguments -WindowStyle Hidden -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Installer failed: $($process.ExitCode)" }
}
try {
    $link = $shell.CreateShortcut($pinnedLink)
    $link.TargetPath = $executable
    $link.Arguments = '--icon-smoke-fixture'
    $link.IconLocation = "$env:SystemRoot/System32/shell32.dll,0"
    $link.Save()
    $other = $shell.CreateShortcut($unrelatedLink)
    $other.TargetPath = "$env:SystemRoot/System32/cmd.exe"
    $other.IconLocation = "$env:SystemRoot/System32/shell32.dll,0"
    $other.Save()
    $unrelatedHash = (Get-FileHash -LiteralPath $unrelatedLink).Hash

    Install-Fixture 'desktopicon'
    if ((Get-FileHash -LiteralPath $expectedIcon -Algorithm SHA256).Hash.ToLowerInvariant() -ne $digest) {
        throw 'Installed ICO does not match approved branding.'
    }
    foreach ($path in @($desktopLink, $startLink, $pinnedLink)) { Assert-LinkIcon $path }
    if ($shell.CreateShortcut($pinnedLink).Arguments -cne '--icon-smoke-fixture') {
        throw 'Pinned shortcut arguments changed.'
    }
    # Reinstall over existing links whose icons came from an older executable.
    foreach ($path in @($desktopLink, $pinnedLink)) {
        $link = $shell.CreateShortcut($path)
        $link.IconLocation = "$executable,0"
        $link.Save()
    }
    # An existing desktop shortcut must be refreshed even if the task is unchecked.
    foreach ($relative in $retiredCodecFiles) {
        Set-Content -LiteralPath (Join-Path $InstallDirectory $relative) -Value 'retired codec fixture' -Encoding ascii
    }
    Set-Content -LiteralPath $unrelatedPluginFile -Value 'preserve unrelated plugin files' -Encoding ascii
    Install-Fixture '!desktopicon'
    foreach ($relative in $retiredCodecFiles) {
        if (Test-Path -LiteralPath (Join-Path $InstallDirectory $relative)) { throw "Upgrade retained removed codec file: $relative" }
    }
    if (-not (Test-Path -LiteralPath $unrelatedPluginFile)) { throw 'Upgrade removed an unrelated plugin file.' }
    foreach ($path in @($desktopLink, $startLink, $pinnedLink)) { Assert-LinkIcon $path }
    if ($shell.CreateShortcut($pinnedLink).Arguments -cne '--icon-smoke-fixture') {
        throw 'Pinned shortcut arguments changed after upgrading.'
    }
    if ((Get-FileHash -LiteralPath $unrelatedLink).Hash -cne $unrelatedHash) {
        throw 'An unrelated pinned shortcut was changed.'
    }
    Write-Host 'Installed desktop, Start Menu and existing pinned icons match current branding.'
} finally {
    if (Test-Path -LiteralPath $unrelatedPluginFile) { Remove-Item -LiteralPath $unrelatedPluginFile }
    foreach ($path in @($pinnedLink, $unrelatedLink)) {
        if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path }
    }
}
