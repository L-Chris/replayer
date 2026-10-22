param(
    [switch]$DryRun,
    [switch]$IncludeLegacyTempCache
)
$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$targetRoot = Join-Path $repoRoot 'target'
$separator = [IO.Path]::DirectorySeparatorChar
foreach ($path in @($targetRoot, (Join-Path $targetRoot 'debug'), (Join-Path $targetRoot 'debug/deps'))) {
    if ((Test-Path -LiteralPath $path) -and
        ((Get-Item -LiteralPath $path).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "Refusing cleanup under a redirected build directory: $path"
    }
}

function Assert-InDirectory([string]$Path, [string]$Root) {
    $absolute = [IO.Path]::GetFullPath($Path)
    $prefix = [IO.Path]::GetFullPath($Root).TrimEnd($separator) + $separator
    if (-not $absolute.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Cleanup target outside its allowed directory: $absolute"
    }
    $absolute
}

$directories = @(
    'debug/incremental', 'comic-review', 'package-review', 'release-review', 'ui-review'
) | ForEach-Object { Assert-InDirectory (Join-Path $targetRoot $_) $targetRoot }
if ($IncludeLegacyTempCache) {
    # One-time cleanup of the previous cache migration. Never create this path.
    $tempRoot = [IO.Path]::GetTempPath()
    $directories += Assert-InDirectory (Join-Path $tempRoot 'replayer-build-cache-recovery') $tempRoot
}
$directories = @($directories | Where-Object { Test-Path -LiteralPath $_ -PathType Container })

$depsRoot = Join-Path $targetRoot 'debug/deps'
$files = @()
if (Test-Path -LiteralPath $depsRoot) {
    $artifacts = @(Get-ChildItem -LiteralPath $depsRoot -File -Filter 'replayer*')
    $latestTest = $artifacts | Where-Object Name -Match '^replayer-[0-9a-f]+\.exe$' |
        Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1
    $keepTest = if ($latestTest) { $latestTest.BaseName } else { '' }
    $files += $artifacts | Where-Object {
        $_.Name -like 'replayer.*.rcgu.o' -or
        ($_.Name -match '^replayer-[0-9a-f]+\.(exe|pdb)$' -and $_.BaseName -ne $keepTest)
    }
}
foreach ($name in @('avdevice-63.dll', 'avfilter-12.dll')) {
    $path = Assert-InDirectory (Join-Path $targetRoot "debug/$name") $targetRoot
    if (Test-Path -LiteralPath $path -PathType Leaf) { $files += Get-Item -LiteralPath $path }
}

# Validate every directory and file before the first deletion. Reject junctions
# and symlinks so recursive cleanup cannot leave the explicitly listed roots.
$bytes = 0L
foreach ($directory in $directories) {
    $items = @((Get-Item -LiteralPath $directory)) + @(Get-ChildItem -LiteralPath $directory -Recurse -Force)
    foreach ($item in $items) {
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw "Refusing cleanup through a reparse point: $($item.FullName)"
        }
        if (-not $item.PSIsContainer) { $bytes += $item.Length }
    }
}
foreach ($file in $files) {
    $null = Assert-InDirectory $file.FullName $targetRoot
    if ($file.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw "Refusing cleanup through a reparse point: $($file.FullName)"
    }
    $bytes += $file.Length
}
Write-Output "Disposable directories: $($directories.Count); files: $($files.Count); logical bytes: $bytes"
foreach ($directory in $directories) { Write-Output $directory }
if ($latestTest) { Write-Output "Keeping latest test executable and symbols: $($latestTest.BaseName)" }
Write-Output 'Keeping target/debug/replayer.exe, its symbols, playback DLLs and dependency libraries.'
if ($DryRun) { return }
foreach ($directory in $directories) { Remove-Item -LiteralPath $directory -Recurse -Force }
foreach ($file in $files) { Remove-Item -LiteralPath $file.FullName -Force }
Write-Output 'Cleanup completed. No artifacts were moved or copied to another drive.'
