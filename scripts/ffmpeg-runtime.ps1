param(
    [Parameter(Mandatory)][string]$Binary,
    [Parameter(Mandatory)][string]$RuntimeDirectory
)
$ErrorActionPreference = 'Stop'

# Read the normal import table directly so packaging needs no Visual Studio tools.
# Check the executable and every bundled FFmpeg DLL, including indirect imports.
function Get-PeImports([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    $reader = [IO.BinaryReader]::new($stream)
    try {
        if ($reader.ReadUInt16() -ne 0x5A4D) { throw "Not a PE file: $Path" }
        $stream.Position = 0x3C
        $peOffset = $reader.ReadUInt32()
        $stream.Position = $peOffset
        if ($reader.ReadUInt32() -ne 0x4550) { throw "Invalid PE signature: $Path" }
        $stream.Position = $peOffset + 6
        $sectionCount = $reader.ReadUInt16()
        $stream.Position = $peOffset + 20
        $optionalSize = $reader.ReadUInt16()
        $optionalOffset = $peOffset + 24
        $stream.Position = $optionalOffset
        if ($reader.ReadUInt16() -ne 0x20B) { throw "Expected a 64-bit PE file: $Path" }
        $stream.Position = $optionalOffset + 120
        $importRva = $reader.ReadUInt32()
        if ($importRva -eq 0) { return }
        $sections = @(for ($i = 0; $i -lt $sectionCount; $i++) {
            $stream.Position = $optionalOffset + $optionalSize + 40 * $i + 8
            $virtualSize = $reader.ReadUInt32()
            $virtualAddress = $reader.ReadUInt32()
            $rawSize = $reader.ReadUInt32()
            $rawOffset = $reader.ReadUInt32()
            @{ Address = $virtualAddress; Size = [Math]::Max($virtualSize, $rawSize); Offset = $rawOffset }
        })
        function Get-FileOffset([uint32]$Rva) {
            foreach ($section in $sections) {
                if ($Rva -ge $section.Address -and $Rva -lt $section.Address + $section.Size) {
                    return [long]($section.Offset + $Rva - $section.Address)
                }
            }
            throw "Invalid import RVA in $Path"
        }
        $descriptor = Get-FileOffset $importRva
        while ($true) {
            $stream.Position = $descriptor + 12
            $nameRva = $reader.ReadUInt32()
            if ($nameRva -eq 0) { break }
            $stream.Position = Get-FileOffset $nameRva
            $name = [Text.StringBuilder]::new()
            while (($character = $reader.ReadByte()) -ne 0) {
                [void]$name.Append([char]$character)
            }
            $name.ToString()
            $descriptor += 20
        }
    } finally { $reader.Dispose() }
}

# ABI versions match the checksum-pinned FFmpeg archive in setup-windows.ps1.
$names = @('avcodec-63.dll', 'avformat-63.dll', 'avutil-61.dll', 'swresample-7.dll', 'swscale-10.dll')
$dlls = @(foreach ($name in $names) {
    $path = Join-Path $RuntimeDirectory $name
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing FFmpeg runtime DLL: $path" }
    Get-Item -LiteralPath $path
})
foreach ($file in @((Get-Item -LiteralPath $Binary)) + $dlls) {
    foreach ($dependency in @(Get-PeImports $file.FullName)) {
        if ($dependency -match '^(avcodec|avdevice|avfilter|avformat|avutil|swresample|swscale|postproc|avresample)-\d+\.dll$' -and $dependency -notin $names) {
            throw "$($file.Name) imports excluded FFmpeg DLL $dependency; rebuild with the playback-only features."
        }
    }
}
$dlls
