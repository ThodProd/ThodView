[CmdletBinding()]
param(
    [string]$Version = '',
    [ValidateSet('Release', 'Debug')][string]$Configuration = 'Release',
    [string]$OutputDirectory = '',
    [string]$FfmpegZip = '',
    [string]$ExifToolZip = '',
    [switch]$SkipPackage
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $root 'artifacts' }
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
$cache = Join-Path $root '.cache'
if (-not $Version) {
    $manifest = Get-Content -LiteralPath (Join-Path $root 'Cargo.toml') -Raw
    $Version = [regex]::Match($manifest, '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
}
if ($Version -notmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$') {
    throw "Invalid version '$Version'."
}

$cargo = Get-Command cargo -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
if ($cargo) {
    $cargoPath = $cargo.Source
} else {
    $cargoPath = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
    if (-not (Test-Path -LiteralPath $cargoPath)) { throw 'Rust toolchain (cargo) was not found.' }
    $env:Path = "$(Split-Path -Parent $cargoPath);$env:Path"
}
$toolchain = & $cargoPath -vV | Select-String '^host: x86_64-pc-windows-(msvc|gnu)$'
if (-not $toolchain) { throw 'Use a Windows x64 Rust toolchain (MSVC or GNU).' }
$target = ($toolchain.Line -split ' ')[1]
$arguments = @('build', '--locked', '--bin', 'ThodView', '--target', $target,
    '--target-dir', (Join-Path $root 'target'), '--manifest-path', (Join-Path $root 'Cargo.toml'))
if ($Configuration -eq 'Release') { $arguments += '--release' }
& $cargoPath @arguments
if ($LASTEXITCODE -ne 0) { throw "Cargo build failed ($LASTEXITCODE)." }

function Get-ToolArchive {
    param([string]$Path, [string]$Uri, [string]$Sha256, [string]$Name)
    if (-not $Path) { $Path = Join-Path $cache $Name }
    if (-not (Test-Path -LiteralPath $Path)) {
        New-Item -ItemType Directory -Path (Split-Path -Parent $Path) -Force | Out-Null
        $download = "$Path.part"
        try {
            Invoke-WebRequest -Uri $Uri -OutFile $download -UseBasicParsing
            if ((Get-FileHash -LiteralPath $download).Hash -ne $Sha256) { throw "Invalid SHA-256 for $Name." }
            Move-Item -LiteralPath $download -Destination $Path -Force
        } finally {
            if (Test-Path -LiteralPath $download) { Remove-Item -LiteralPath $download -Force }
        }
    }
    if ((Get-FileHash -LiteralPath $Path).Hash -ne $Sha256) { throw "Invalid SHA-256 for $Name." }
    return (Resolve-Path -LiteralPath $Path).Path
}

$ffmpegArchive = Get-ToolArchive $FfmpegZip 'https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-9.0.2-essentials_build.zip' '60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba' 'ffmpeg-9.0.2-essentials_build.zip'
$exifArchive = Get-ToolArchive $ExifToolZip 'https://oliverbetz.de/cms/files/Artikel/ExifTool-for-Windows/exiftool-13.59_64.zip' '577257dd22baebe77157d905792d6ed2c5916cd03aa627f40b2175db12110ac6' 'exiftool-13.59_64.zip'

$name = "thodview-$Version-win-x64"
$stage = Join-Path $OutputDirectory $name
$licensesOut = Join-Path $stage 'licenses'
$ffmpegOut = Join-Path $stage 'runtime\ffmpeg'
$exifOut = Join-Path $stage 'runtime\exiftool'
New-Item -ItemType Directory -Path $stage, $licensesOut, $ffmpegOut, $exifOut -Force | Out-Null
$exe = Join-Path $root "target\$target\$($Configuration.ToLowerInvariant())\ThodView.exe"
Copy-Item -LiteralPath $exe -Destination $stage -Force
foreach ($file in 'README.md', 'THIRD-PARTY-NOTICES.md', 'LICENSE') {
    Copy-Item -LiteralPath (Join-Path $root $file) -Destination $stage -Force
}
Copy-Item -LiteralPath (Join-Path $root 'licenses\FFMPEG-SOURCE-OFFER.txt') -Destination $licensesOut -Force

Add-Type -AssemblyName System.IO.Compression.FileSystem
function Copy-ZipFiles {
    param([string]$Archive, [hashtable]$ExactNames, [string]$ExifDestination)
    $zip = [IO.Compression.ZipFile]::OpenRead($Archive)
    try {
        $remaining = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
        foreach ($key in $ExactNames.Keys) { [void]$remaining.Add($key) }
        foreach ($entry in $zip.Entries) {
            $name = $entry.FullName.Replace('\', '/')
            if ($ExactNames.ContainsKey($name)) {
                [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $ExactNames[$name], $true)
                [void]$remaining.Remove($name)
            } elseif ($ExifDestination -and $entry.Name -and $name -match '(?:^|/)exiftool_files/(.+)$') {
                $relative = $Matches[1]
                if ($relative -match '(^|/)\.\.(/|$)|:|^/' -or $relative -match '^unins') { continue }
                $destination = Join-Path $ExifDestination "exiftool_files/$relative"
                New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
                [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $destination, $true)
            }
        }
        if ($remaining.Count -ne 0) { throw "Missing archive entries: $($remaining -join ', ')" }
    } finally { $zip.Dispose() }
}

$ffmpegPrefix = 'ffmpeg-9.0.2-essentials_build'
Copy-ZipFiles $ffmpegArchive @{
    "$ffmpegPrefix/bin/ffmpeg.exe" = (Join-Path $ffmpegOut 'ffmpeg.exe')
    "$ffmpegPrefix/LICENSE" = (Join-Path $licensesOut 'ffmpeg-COPYING.txt')
}
$exifZipFile = [IO.Compression.ZipFile]::OpenRead($exifArchive)
try {
    $exifEntry = $exifZipFile.Entries | Where-Object { $_.Name -ieq 'ExifTool.exe' } | Select-Object -First 1
    if (-not $exifEntry) { throw 'ExifTool.exe was not found in the archive.' }
    $exifName = $exifEntry.FullName.Replace('\', '/')
} finally { $exifZipFile.Dispose() }
Copy-ZipFiles $exifArchive @{ $exifName = (Join-Path $exifOut 'ExifTool.exe') } $exifOut
Copy-Item -LiteralPath (Join-Path $exifOut 'exiftool_files\LICENSE') -Destination (Join-Path $licensesOut 'exiftool-COPYING.txt') -Force

if (-not $SkipPackage) {
    $file = Join-Path $OutputDirectory "$name.zip"
    $pending = Join-Path $OutputDirectory "$name.$([guid]::NewGuid().ToString('N')).tmp"
    try {
        [IO.Compression.ZipFile]::CreateFromDirectory($stage, $pending, [IO.Compression.CompressionLevel]::Optimal, $false)
        Move-Item -LiteralPath $pending -Destination $file -Force
    } finally {
        if (Test-Path -LiteralPath $pending) { Remove-Item -LiteralPath $pending -Force }
    }
    Get-FileHash -LiteralPath $file
}
Write-Host "ThodView: $(Join-Path $stage 'ThodView.exe')"
