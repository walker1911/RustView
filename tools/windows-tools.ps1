# Shared by the two packaging scripts. This changes only the current process environment.
function Find-Cargo([string]$ProjectRoot) {
    $command = Get-Command cargo -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }
    if ($env:CARGO_HOME -and (Test-Path -LiteralPath (Join-Path $env:CARGO_HOME 'bin/cargo.exe'))) { return (Join-Path $env:CARGO_HOME 'bin/cargo.exe') }
    $local = Join-Path $ProjectRoot '.tools/cargo/bin/cargo.exe'
    if (Test-Path -LiteralPath $local) {
        $env:CARGO_HOME = Join-Path $ProjectRoot '.tools/cargo'
        if (Test-Path -LiteralPath (Join-Path $ProjectRoot '.tools/rustup')) { $env:RUSTUP_HOME = Join-Path $ProjectRoot '.tools/rustup' }
        return $local
    }
    throw 'cargo was not found. Install Rust stable for x86_64-pc-windows-msvc, including Visual Studio C++ Build Tools.'
}
function Find-ReleaseExe([string]$ProjectRoot) {
    $targetRoot = Join-Path $ProjectRoot 'target'
    if ($env:CARGO_TARGET_DIR) { $targetRoot = $env:CARGO_TARGET_DIR; if (-not [IO.Path]::IsPathRooted($targetRoot)) { $targetRoot = Join-Path $ProjectRoot $targetRoot } }
    $candidates = @('x86_64-pc-windows-msvc/release/RustView.exe','release/RustView.exe') | ForEach-Object { Join-Path $targetRoot $_ }
    $file = $candidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Get-Item | Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1
    if (-not $file) { throw 'No release RustView.exe exists. Run without -SkipBuild first.' }
    $stream = [IO.File]::OpenRead($file.FullName)
    try {
        $reader = New-Object IO.BinaryReader($stream)
        $stream.Position = 0x3c; $header = $reader.ReadInt32()
        $stream.Position = $header + 4
        if ($reader.ReadUInt16() -ne 0x8664) { throw 'The release executable is not Windows x64.' }
    } finally { $stream.Dispose() }
    return $file.FullName
}
function Copy-VcRuntime([string]$Package, [string]$RuntimeDirectory) {
    $candidates = @()
    if ($RuntimeDirectory) { $candidates += $RuntimeDirectory }
    if ($env:VCToolsRedistDir) { $candidates += Join-Path $env:VCToolsRedistDir 'x64/Microsoft.VC143.CRT' }
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    if (Test-Path -LiteralPath $vswhere) {
        $installations = & $vswhere -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        foreach ($installation in $installations) {
            $root = Join-Path $installation 'VC/Redist/MSVC'
            if (Test-Path -LiteralPath $root) {
                $candidates += Get-ChildItem -LiteralPath $root -Directory | Sort-Object Name -Descending | ForEach-Object { Join-Path $_.FullName 'x64/Microsoft.VC143.CRT' }
            }
        }
    }
    $names = @('msvcp140.dll','msvcp140_1.dll','msvcp140_2.dll','vcruntime140.dll','vcruntime140_1.dll','concrt140.dll')
    $directory = $candidates | Where-Object { $candidate = $_; @($names | Where-Object { -not (Test-Path -LiteralPath (Join-Path $candidate $_) -PathType Leaf) }).Count -eq 0 } | Select-Object -First 1
    if (-not $directory) { throw 'MSVC x64 app-local redistributables not found. Install the Visual Studio C++ Build Tools workload or pass -RuntimeDirectory pointing to Microsoft.VC143.CRT.' }
    foreach ($name in $names) { Copy-Item -LiteralPath (Join-Path $directory $name) -Destination (Join-Path $Package $name) -Force }
    $licenses = Join-Path $Package 'licenses'
    [void][IO.Directory]::CreateDirectory($licenses)
    foreach ($scope in @($directory, (Split-Path -Parent $directory), (Split-Path -Parent (Split-Path -Parent $directory)))) {
        Get-ChildItem -LiteralPath $scope -File | Where-Object { $_.Name -match '^(license|eula|redist).*[.](txt|rtf)$' } | ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $licenses $_.Name) -Force }
    }
    @('Microsoft Visual C++ x64 app-local redistributable runtime DLLs.', 'These Microsoft components are not covered by RustView''s MIT license.', 'Original source: the Visual Studio C++ Build Tools redistributable directory.', 'Redistribution information: https://learn.microsoft.com/cpp/windows/redistributing-visual-cpp-files') | Set-Content -LiteralPath (Join-Path $licenses 'MICROSOFT-RUNTIME.txt') -Encoding utf8
}
function Write-PackageArchive([string]$Package, [string]$ZipPath, [string[]]$Folders) {
    $shipping = @(Get-ChildItem -LiteralPath $Package -File | Where-Object { $_.Name -ne 'SHA256SUMS.txt' })
    $items = @((Get-ChildItem -LiteralPath $Package -File).FullName)
    foreach ($name in $Folders) {
        $folder = Join-Path $Package $name
        if (Test-Path -LiteralPath $folder -PathType Container) { $shipping += @(Get-ChildItem -LiteralPath $folder -File -Recurse); $items += $folder }
    }
    $prefix = $Package.TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar
    $shipping | ForEach-Object { (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash + '  ' + $_.FullName.Substring($prefix.Length) } | Set-Content -LiteralPath (Join-Path $Package 'SHA256SUMS.txt') -Encoding utf8
    $items = @((Get-ChildItem -LiteralPath $Package -File).FullName) + @($Folders | ForEach-Object { Join-Path $Package $_ } | Where-Object { Test-Path -LiteralPath $_ -PathType Container })
    Compress-Archive -LiteralPath $items -DestinationPath $ZipPath -CompressionLevel Optimal -Force
    ((Get-FileHash -LiteralPath $ZipPath -Algorithm SHA256).Hash + '  ' + (Split-Path -Leaf $ZipPath)) | Set-Content -LiteralPath ($ZipPath + '.sha256') -Encoding ascii
}

function Copy-WindowsIntegration([string]$Package) {
    [void][IO.Directory]::CreateDirectory((Join-Path $Package 'tools'))
    foreach ($file in @('windows-associations.ps1','configure-windows.ps1')) {
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot $file) -Destination (Join-Path $Package ('tools/' + $file)) -Force
    }
    foreach ($action in @('Setup','Remove')) {
        $argument = if ($action -eq 'Remove') { ' -Uninstall' } else { '' }
        $launcher = '@echo off' + "`r`n" + 'powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0tools\configure-windows.ps1"' + $argument + "`r`n" + 'if errorlevel 1 (pause & exit /b 1)' + "`r`n" + 'pause' + "`r`n"
        [IO.File]::WriteAllText((Join-Path $Package ($action + '-File-Associations.cmd')), $launcher, [Text.Encoding]::ASCII)
    }
}
