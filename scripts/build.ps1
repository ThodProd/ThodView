<#
  Builds ThodView and packs it for ThodHUB.

  Pinned tools:
    ffmpeg 9.0.2 essentials (Gyan build, GPLv3)
    ExifTool 13.59 portable (GPLv3)

  Examples:
    .\scripts\build.ps1
    .\scripts\build.ps1 -Version 1.0.1 -FfmpegZip D:\tools\ffmpeg.zip -ExifToolZip D:\tools\exiftool.zip
#>
[CmdletBinding()]
param(
    [string]$Version = '',
    [string]$Configuration = 'Release',
    [string]$OutputDirectory = '',
    [string]$FfmpegZip = '',
    [string]$ExifToolZip = '',
    [switch]$SkipPackage
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $root 'artifacts' }
$manifestPath = Join-Path $root 'hub\manifest.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json

if (-not $Version) { $Version = $manifest.version }
if ($Version -notmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$') {
    throw "Version must look like 1.2.3, got '$Version'."
}

$OutputDirectory = [System.IO.Path]::GetFullPath($OutputDirectory)
$cache = Join-Path $root '.cache'
$stage = Join-Path $OutputDirectory "stage-$Version"
$appOut = Join-Path $stage 'app'
$ffmpegSha = '60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba'
$exifToolSha = '577257dd22baebe77157d905792d6ed2c5916cd03aa627f40b2175db12110ac6'
$ffmpegUrl = 'https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-9.0.2-essentials_build.zip'
$exifToolUrl = 'https://oliverbetz.de/cms/files/Artikel/ExifTool-for-Windows/exiftool-13.59_64.zip'

function Get-ToolFile {
    param([string]$Path, [string]$Url, [string]$Sha256, [string]$Name)
    if (-not $Path) {
        New-Item -ItemType Directory -Path $cache -Force | Out-Null
        $Path = Join-Path $cache $Name
    }
    if (-not (Test-Path -LiteralPath $Path)) {
        Write-Host "Downloading $Name ..."
        Invoke-WebRequest -Uri $Url -OutFile $Path -UseBasicParsing
    }
    $actual = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $Sha256) {
        throw "SHA-256 mismatch for $Name. Expected $Sha256, got $actual."
    }
    return (Resolve-Path -LiteralPath $Path).Path
}

Write-Host "ThodView $Version"
Write-Host "Workspace: $root"

New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
New-Item -ItemType Directory -Path $stage -Force | Out-Null

# --- self-contained application ---------------------------------------------
$project = Join-Path $root 'src\ThodView\ThodView.csproj'
Write-Host 'Publishing ThodView (win-x64, self-contained) ...'
& dotnet publish $project `
    -c $Configuration `
    -r win-x64 `
    --self-contained true `
    -p:Version=$Version `
    -p:DebugType=None `
    -p:DebugSymbols=false `
    -p:PublishSingleFile=false `
    -o $appOut
if ($LASTEXITCODE -ne 0) { throw "dotnet publish failed with exit code $LASTEXITCODE." }

# --- manifest ---------------------------------------------------------------
$manifest.version = $Version
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText((Join-Path $stage 'manifest.json'), ($manifest | ConvertTo-Json -Depth 10), $utf8NoBom)

# --- docs / licenses --------------------------------------------------------
foreach ($file in 'README.md', 'THIRD-PARTY-NOTICES.md', 'LICENSE') {
    Copy-Item -LiteralPath (Join-Path $root $file) -Destination $stage -Force
}
$licensesOut = Join-Path $stage 'licenses'
New-Item -ItemType Directory -Path $licensesOut -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $root 'licenses\FFMPEG-SOURCE-OFFER.txt') -Destination $licensesOut -Force

# --- ffmpeg -----------------------------------------------------------------
$ffmpegArchive = Get-ToolFile -Path $FfmpegZip -Url $ffmpegUrl -Sha256 $ffmpegSha -Name 'ffmpeg-9.0.2-essentials_build.zip'
$ffmpegExtract = Join-Path $cache 'ffmpeg-extract-view'
if (Test-Path -LiteralPath $ffmpegExtract) { Remove-Item -LiteralPath $ffmpegExtract -Recurse -Force }
Write-Host 'Extracting ffmpeg ...'
Expand-Archive -LiteralPath $ffmpegArchive -DestinationPath $ffmpegExtract -Force
$ffmpegBin = Get-ChildItem -LiteralPath $ffmpegExtract -Recurse -Filter 'ffmpeg.exe' | Select-Object -First 1
if (-not $ffmpegBin) { throw 'ffmpeg.exe was not found in the archive.' }
$runtimeOut = Join-Path $appOut 'runtime\ffmpeg'
New-Item -ItemType Directory -Path $runtimeOut -Force | Out-Null
Copy-Item -LiteralPath $ffmpegBin.FullName -Destination $runtimeOut -Force
$ffprobe = Join-Path $ffmpegBin.Directory.FullName 'ffprobe.exe'
if (-not (Test-Path -LiteralPath $ffprobe)) { throw 'ffprobe.exe was not found in the archive.' }
Copy-Item -LiteralPath $ffprobe -Destination $runtimeOut -Force
$ffmpegLicense = Join-Path (Split-Path $ffmpegBin.Directory.FullName -Parent) 'LICENSE'
if (-not (Test-Path -LiteralPath $ffmpegLicense)) { throw 'ffmpeg LICENSE was not found in the archive.' }
Copy-Item -LiteralPath $ffmpegLicense -Destination (Join-Path $licensesOut 'ffmpeg-COPYING.txt') -Force

# --- exiftool ---------------------------------------------------------------
$exifArchive = Get-ToolFile -Path $ExifToolZip -Url $exifToolUrl -Sha256 $exifToolSha -Name 'exiftool-13.59_64.zip'
$exifExtract = Join-Path $cache 'exiftool-extract'
if (Test-Path -LiteralPath $exifExtract) { Remove-Item -LiteralPath $exifExtract -Recurse -Force }
Write-Host 'Extracting ExifTool ...'
Expand-Archive -LiteralPath $exifArchive -DestinationPath $exifExtract -Force
$exifExe = Get-ChildItem -LiteralPath $exifExtract -Recurse -Filter 'ExifTool.exe' | Select-Object -First 1
if (-not $exifExe) { throw 'ExifTool.exe was not found in the archive.' }
$exifOut = Join-Path $appOut 'runtime\exiftool'
New-Item -ItemType Directory -Path $exifOut -Force | Out-Null
Copy-Item -LiteralPath $exifExe.FullName -Destination (Join-Path $exifOut 'ExifTool.exe') -Force
$exifFiles = Join-Path $exifExe.Directory.FullName 'exiftool_files'
if (-not (Test-Path -LiteralPath $exifFiles)) { throw 'exiftool_files folder was not found in the archive.' }
Copy-Item -LiteralPath $exifFiles -Destination (Join-Path $exifOut 'exiftool_files') -Recurse -Force
Get-ChildItem -LiteralPath (Join-Path $exifOut 'exiftool_files') -Filter 'unins*' -Force | Remove-Item -Force -ErrorAction SilentlyContinue
$exifLicense = Join-Path $exifFiles 'LICENSE'
if (Test-Path -LiteralPath $exifLicense) {
    Copy-Item -LiteralPath $exifLicense -Destination (Join-Path $licensesOut 'exiftool-COPYING.txt') -Force
}
$exifVersion = & (Join-Path $exifOut 'ExifTool.exe') -ver
if ($LASTEXITCODE -ne 0) { throw 'Bundled ExifTool did not start.' }
Write-Host "ExifTool: $exifVersion"

# --- package ----------------------------------------------------------------
$zip = $null
if (-not $SkipPackage) {
    $packageScript = Join-Path $root 'scripts\package-module.ps1'
    $existing = Join-Path $OutputDirectory "thodview-$Version-win-x64.zip"
    if (Test-Path -LiteralPath $existing) { Remove-Item -LiteralPath $existing -Force }
    $zip = & $packageScript `
        -PackageDirectory $stage `
        -Repository 'ThodProd/ThodView' `
        -OutputDirectory $OutputDirectory `
        -Notes "ThodView $Version - fast standalone image viewer with free panning, folder navigation and metadata editing."
    if (-not $zip -or -not (Test-Path -LiteralPath $zip)) { throw 'Packaging failed: ZIP was not produced.' }

    $releasePath = Join-Path $OutputDirectory 'thodhub-release.json'
    $release = Get-Content -LiteralPath $releasePath -Raw | ConvertFrom-Json
    $actualHash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    $actualSize = (Get-Item -LiteralPath $zip).Length
    if ($release.sha256 -ne $actualHash -or [long]$release.size -ne $actualSize) {
        throw 'Packaging verification failed: SHA-256/size do not match thodhub-release.json.'
    }
    Write-Host ''
    Write-Host "Package : $zip"
    Write-Host "SHA-256 : $actualHash"
    Write-Host "Size    : $actualSize bytes"
    Write-Host "Release : $releasePath"
} else {
    Write-Host "Staged  : $stage"
}
