#Requires -Version 5.1
[CmdletBinding()]
param([switch]$SkipBuild, [string]$OutputRoot, [string]$RuntimeDirectory, [string]$SourceArchive, [switch]$Offline)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'windows-tools.ps1')
if (-not $OutputRoot) { $OutputRoot = Join-Path $projectRoot 'dist' }
if (-not $SourceArchive) { $SourceArchive = Join-Path $projectRoot '.tools/archives/7z2606-src.7z' }
$OutputRoot = [IO.Path]::GetFullPath($OutputRoot)
$version = ([regex]::Match((Get-Content -LiteralPath (Join-Path $projectRoot 'Cargo.toml') -Raw),'(?m)^version\s*=\s*"([^"]+)"')).Groups[1].Value
if (-not $version) { throw 'Missing package version.' }
$manifest = Get-Content -LiteralPath (Join-Path $projectRoot 'native/manifest.json') -Raw | ConvertFrom-Json
foreach ($entry in $manifest.runtime_sha256.PSObject.Properties) {
    $path = Join-Path $projectRoot ('native/' + $entry.Name)
    if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $entry.Value) { throw "Missing or incorrect native runtime: $($entry.Name). Run tools/fetch-native.ps1." }
}
if (-not (Test-Path -LiteralPath $SourceArchive -PathType Leaf) -or (Get-FileHash -LiteralPath $SourceArchive -Algorithm SHA256).Hash -ne $manifest.archives.sevenZipSource.sha256) { throw 'The matching 7-Zip source archive is required for a full local package. Run tools/fetch-native.ps1.' }
$libRawSource = Join-Path $projectRoot 'native/LibRaw/LibRaw-0.22.3-Win64.zip'
if (-not (Test-Path -LiteralPath $libRawSource -PathType Leaf) -or (Get-FileHash -LiteralPath $libRawSource -Algorithm SHA256).Hash -ne $manifest.archives.libRaw.sha256) { throw 'The official LibRaw source/binary ZIP is required. Run tools/fetch-native.ps1.' }
Push-Location $projectRoot
try {
    if (-not $SkipBuild) {
        $cargo = Find-Cargo $projectRoot
        & $cargo build --release --locked --target x86_64-pc-windows-msvc
        if ($LASTEXITCODE -ne 0) { throw 'RustView release build failed.' }
    }
    $package = Join-Path $OutputRoot ('RustView-' + $version)
    [void][IO.Directory]::CreateDirectory($package)
    Copy-Item -LiteralPath (Find-ReleaseExe $projectRoot) -Destination (Join-Path $package 'RustView.exe') -Force
    foreach ($engine in @('7zip','LibRaw','ImageMagick','ExifTool')) {
        $engineRoot = Join-Path $projectRoot ('native/' + $engine)
        Get-ChildItem -LiteralPath $engineRoot -File -Recurse | ForEach-Object {
            $relative = $_.FullName.Substring($engineRoot.TrimEnd('\','/').Length).TrimStart('\','/')
            $target = Join-Path $package ('native/' + $engine + '/' + $relative)
            [void][IO.Directory]::CreateDirectory((Split-Path -Parent $target))
            Copy-Item -LiteralPath $_.FullName -Destination $target -Force
        }
    }
    foreach ($file in @('README.md','README.zh-CN.md','THIRD-PARTY-NOTICES.md','LICENSE','BUILDING.md','OFFLINE.md')) {
        $source = Join-Path $projectRoot $file
        if (Test-Path -LiteralPath $source -PathType Leaf) { Copy-Item -LiteralPath $source -Destination (Join-Path $package $file) -Force }
    }
    foreach ($folder in @('docs','assets','licenses')) {
        if (Test-Path -LiteralPath (Join-Path $projectRoot $folder)) { Copy-Item -LiteralPath (Join-Path $projectRoot $folder) -Destination $package -Recurse -Force }
    }
    [void][IO.Directory]::CreateDirectory((Join-Path $package 'sources'))
    Copy-Item -LiteralPath $SourceArchive -Destination (Join-Path $package 'sources/7z2606-src.7z') -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot 'native/manifest.json') -Destination (Join-Path $package 'native/manifest.json') -Force
    if ($Offline) {
        Copy-Item -LiteralPath (Join-Path $projectRoot 'docs/NATIVE-SOURCES.md') -Destination (Join-Path $package 'sources/README.md') -Force
    }
    Copy-VcRuntime $package $RuntimeDirectory
    $launcher = '@echo off' + "`r`n" + 'start "" "%~dp0RustView.exe" %*' + "`r`n"
    [IO.File]::WriteAllText((Join-Path $package 'Start-RustView.cmd'), $launcher, [Text.Encoding]::ASCII)
    $suffix = if ($Offline) { '-win-x64-offline.zip' } else { '-win-x64.zip' }
    Write-PackageArchive $package (Join-Path $OutputRoot ('RustView-' + $version + $suffix)) @('native','docs','sources','assets','licenses')
    Write-Output $package
} finally { Pop-Location }
