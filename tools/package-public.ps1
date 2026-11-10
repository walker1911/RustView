#Requires -Version 5.1
[CmdletBinding()]
param([switch]$SkipBuild, [string]$OutputRoot, [string]$RuntimeDirectory)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'windows-tools.ps1')
if (-not $OutputRoot) { $OutputRoot = Join-Path $projectRoot 'dist' }
$OutputRoot = [IO.Path]::GetFullPath($OutputRoot)
$version = ([regex]::Match((Get-Content -LiteralPath (Join-Path $projectRoot 'Cargo.toml') -Raw),'(?m)^version\s*=\s*"([^"]+)"')).Groups[1].Value
if (-not $version) { throw 'Missing package version.' }
$manifest = Get-Content -LiteralPath (Join-Path $projectRoot 'native/manifest.json') -Raw | ConvertFrom-Json
Push-Location $projectRoot
try {
    if (-not $SkipBuild) {
        $cargo = Find-Cargo $projectRoot
        & $cargo build --release --locked --target x86_64-pc-windows-msvc
        if ($LASTEXITCODE -ne 0) { throw 'RustView release build failed.' }
    }
    $package = Join-Path $OutputRoot ('RustView-' + $version + '-bootstrap')
    [void][IO.Directory]::CreateDirectory($package)
    # Never pick up downloaded engines from a previously prepared end-user folder.
    $native = Join-Path $package 'native'
    if (Test-Path -LiteralPath $native) {
        Get-ChildItem -LiteralPath $native -File -Recurse | ForEach-Object {
            $relative = $_.FullName.Substring($native.TrimEnd('\','/').Length).TrimStart('\','/').Replace('\','/')
            if ($relative -ne 'manifest.json' -and $manifest.templates -notcontains $relative) { throw 'The bootstrap output already contains generated native files. Choose a fresh -OutputRoot.' }
        }
    }
    Copy-Item -LiteralPath (Find-ReleaseExe $projectRoot) -Destination (Join-Path $package 'RustView.exe') -Force
    foreach ($file in @('README.md','README.zh-CN.md','THIRD-PARTY-NOTICES.md','LICENSE','BUILDING.md')) {
        $source = Join-Path $projectRoot $file
        if (Test-Path -LiteralPath $source -PathType Leaf) { Copy-Item -LiteralPath $source -Destination (Join-Path $package $file) -Force }
    }
    foreach ($folder in @('docs','assets','licenses')) {
        if (Test-Path -LiteralPath (Join-Path $projectRoot $folder)) { Copy-Item -LiteralPath (Join-Path $projectRoot $folder) -Destination $package -Recurse -Force }
    }
    foreach ($file in @('manifest.json') + @($manifest.templates)) {
        $destination = Join-Path $native $file
        [void][IO.Directory]::CreateDirectory((Split-Path -Parent $destination))
        Copy-Item -LiteralPath (Join-Path $projectRoot ('native/' + $file)) -Destination $destination -Force
    }
    [void][IO.Directory]::CreateDirectory((Join-Path $package 'tools'))
    foreach ($file in @('fetch-native.ps1','prepare-and-start.ps1')) { Copy-Item -LiteralPath (Join-Path $PSScriptRoot $file) -Destination (Join-Path $package ('tools/' + $file)) -Force }
    Copy-VcRuntime $package $RuntimeDirectory
    Copy-WindowsIntegration $package
    $launcher = '@echo off' + "`r`n" + 'powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0tools\prepare-and-start.ps1"' + "`r`n"
    [IO.File]::WriteAllText((Join-Path $package 'Start-RustView.cmd'), $launcher, [Text.Encoding]::ASCII)
    Write-PackageArchive $package (Join-Path $OutputRoot ('RustView-' + $version + '-win-x64-bootstrap.zip')) @('native','docs','assets','tools','licenses')
    Write-Output $package
} finally { Pop-Location }
