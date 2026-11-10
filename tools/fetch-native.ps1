#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$CacheDirectory,
    [string]$DestinationRoot,
    [switch]$VerifyOnly
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$projectRoot = Split-Path -Parent $PSScriptRoot
if (-not $CacheDirectory) { $CacheDirectory = Join-Path $projectRoot '.tools/archives' }
if (-not $DestinationRoot) { $DestinationRoot = Join-Path $projectRoot 'native' }
$CacheDirectory = [IO.Path]::GetFullPath($CacheDirectory)
$DestinationRoot = [IO.Path]::GetFullPath($DestinationRoot)
$manifest = Get-Content -LiteralPath (Join-Path $projectRoot 'native/manifest.json') -Raw | ConvertFrom-Json

function Assert-Hash([string]$Path, [string]$Expected) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Missing file: $Path. Run tools/fetch-native.ps1 first." }
    if ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ne $Expected) { throw "SHA-256 mismatch; refusing this file: $Path" }
}
function Get-Archive($Asset) {
    $path = Join-Path $CacheDirectory $Asset.file
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        if ($VerifyOnly) { throw "Missing cached archive: $path. Run tools/fetch-native.ps1 first." }
        [void][IO.Directory]::CreateDirectory($CacheDirectory)
        $temporary = $path + '.partial-' + [Guid]::NewGuid().ToString('N')
        Write-Host "Downloading $($Asset.file) from the official upstream..."
        try {
            [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
            # SourceForge serves its HTML landing page to the default PowerShell UA.
            # Standard command-line download negotiation returns the actual archive.
            Invoke-WebRequest -UseBasicParsing -UserAgent 'Wget/1.21.4' -Uri $Asset.url -OutFile $temporary -TimeoutSec 180
            Assert-Hash $temporary $Asset.sha256
            Move-Item -LiteralPath $temporary -Destination $path
        } finally {
            if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force }
        }
    }
    Assert-Hash $path $Asset.sha256
    return $path
}
function Copy-Checked([string]$From, [string]$To) {
    if (-not (Test-Path -LiteralPath $From -PathType Leaf)) { throw "Upstream archive is missing: $From" }
    if (Test-Path -LiteralPath $To -PathType Leaf) {
        if ((Get-FileHash -LiteralPath $From -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath $To -Algorithm SHA256).Hash) {
            throw "Existing file differs; it was not overwritten: $To. Use a fresh DestinationRoot, or review and remove that generated file yourself."
        }
        return
    }
    [void][IO.Directory]::CreateDirectory((Split-Path -Parent $To))
    Copy-Item -LiteralPath $From -Destination $To
}
function Copy-Tree([string]$From, [string]$To) {
    Get-ChildItem -LiteralPath $From -File -Recurse | ForEach-Object {
        $relative = $_.FullName.Substring($From.TrimEnd('\','/').Length).TrimStart('\','/')
        Copy-Checked $_.FullName (Join-Path $To $relative)
    }
}

$archives = @{}
foreach ($asset in $manifest.archives.PSObject.Properties) { $archives[$asset.Name] = Get-Archive $asset.Value }
if (-not $VerifyOnly) {
    $stage = Join-Path $CacheDirectory ('native-stage-' + [Guid]::NewGuid().ToString('N'))
    [void][IO.Directory]::CreateDirectory($stage)
    try {
        # The pinned, hash-verified 7zr is an extractor; no installer is executed.
        $seven = Join-Path $stage '7zip'
        & $archives.bootstrap x $archives.sevenZip ('-o' + $seven) -y -bd | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'Unable to extract official 7-Zip package.' }
        foreach ($name in @('7z.exe','7z.dll')) { Copy-Checked (Join-Path $seven $name) (Join-Path $DestinationRoot ('7zip/' + $name)) }
        $extractor = Join-Path $seven '7z.exe'
        foreach ($name in @('libRaw','imageMagick','exifTool')) {
            & $extractor x $archives[$name] ('-o' + (Join-Path $stage $name)) -y -bd | Out-Null
            if ($LASTEXITCODE -ne 0) { throw "Unable to extract official $name package." }
        }
        $libRaw = Join-Path $stage 'libRaw/LibRaw-0.22.3'
        Copy-Checked (Join-Path $libRaw 'bin/libraw.dll') (Join-Path $DestinationRoot 'LibRaw/libraw.dll')
        Copy-Checked $archives.libRaw (Join-Path $DestinationRoot 'LibRaw/LibRaw-0.22.3-Win64.zip')
        $magick = Join-Path $stage 'imageMagick'
        Get-ChildItem -LiteralPath $magick -File | Where-Object { $_.Name -notin @('policy.xml','LICENSE.txt','NOTICE.txt') -and ($_.Extension -ne '.exe' -or $_.Name -eq 'magick.exe') } | ForEach-Object { Copy-Checked $_.FullName (Join-Path $DestinationRoot ('ImageMagick/' + $_.Name)) }
        $exif = Join-Path $stage 'exifTool/exiftool-13.62_64'
        Copy-Checked (Join-Path $exif 'exiftool(-k).exe') (Join-Path $DestinationRoot 'ExifTool/exiftool.exe')
        Copy-Tree (Join-Path $exif 'exiftool_files') (Join-Path $DestinationRoot 'ExifTool/exiftool_files')
        foreach ($name in $manifest.templates) { Copy-Checked (Join-Path $projectRoot ('native/' + $name)) (Join-Path $DestinationRoot $name) }
        Copy-Checked (Join-Path $projectRoot 'native/manifest.json') (Join-Path $DestinationRoot 'manifest.json')
    } finally {
        $resolved = [IO.Path]::GetFullPath($stage)
        if (-not $resolved.StartsWith($CacheDirectory.TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or -not (Split-Path -Leaf $resolved).StartsWith('native-stage-')) { throw 'Refusing cleanup outside the native staging directory.' }
        if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force }
    }
}
foreach ($file in $manifest.runtime_sha256.PSObject.Properties) { Assert-Hash (Join-Path $DestinationRoot $file.Name) $file.Value }
foreach ($file in $manifest.templates) { if (-not (Test-Path -LiteralPath (Join-Path $DestinationRoot $file) -PathType Leaf)) { throw "Missing native policy or license: $file" } }
if (-not (Test-Path -LiteralPath (Join-Path $DestinationRoot 'ExifTool/exiftool_files/perl532.dll'))) { throw 'ExifTool runtime is incomplete.' }
Write-Host "Native engine archives and core runtimes verified: $DestinationRoot"
