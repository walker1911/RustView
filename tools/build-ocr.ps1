#Requires -Version 5.1
[CmdletBinding()]
param([string]$CacheDirectory, [string]$DestinationRoot)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$projectRoot = Split-Path -Parent $PSScriptRoot
if (-not $CacheDirectory) { $CacheDirectory = Join-Path $projectRoot '.tools/archives' }
if (-not $DestinationRoot) { $DestinationRoot = Join-Path $projectRoot 'native' }
$CacheDirectory = [IO.Path]::GetFullPath($CacheDirectory)
$runtime = Join-Path ([IO.Path]::GetFullPath($DestinationRoot)) 'Tesseract'
$spec = (Get-Content -LiteralPath (Join-Path $projectRoot 'native/manifest.json') -Raw | ConvertFrom-Json).ocr
$cmake = (Get-Command cmake.exe).Source
$ninja = (Get-Command ninja.exe).Source
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$visualStudio = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $visualStudio) { throw 'Visual Studio C++ Build Tools x64 were not found.' }
$toolset = (Get-Content -LiteralPath (Join-Path $visualStudio 'VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt') -Raw).Trim()
$msvc = (Join-Path $visualStudio "VC/Tools/MSVC/$toolset").Replace('\','/')
$sdk = (Get-ItemProperty -LiteralPath 'HKLM:/SOFTWARE/Microsoft/Windows Kits/Installed Roots' -Name KitsRoot10).KitsRoot10.Replace('\','/').TrimEnd('/')
$sdkVersion = Get-ChildItem -LiteralPath (Join-Path $sdk 'Lib') -Directory |
    Where-Object { Test-Path -LiteralPath (Join-Path $sdk "bin/$($_.Name)/x64/rc.exe") } |
    Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1 -ExpandProperty Name
if (-not $sdkVersion) { throw 'The Windows SDK x64 build tools were not found.' }

function Get-OcrAsset($asset) {
    [void][IO.Directory]::CreateDirectory($CacheDirectory)
    $path = Join-Path $CacheDirectory $asset.file
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        $temporary = $path + '.partial-' + [Guid]::NewGuid().ToString('N')
        try {
            [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
            Invoke-WebRequest -UseBasicParsing -Uri $asset.url -OutFile $temporary -TimeoutSec 180
            if ((Get-FileHash -LiteralPath $temporary).Hash -ne $asset.sha256) { throw "SHA-256 mismatch: $($asset.file)" }
            Move-Item -LiteralPath $temporary -Destination $path
        } finally {
            if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force }
        }
    }
    if ((Get-FileHash -LiteralPath $path).Hash -ne $asset.sha256) { throw "SHA-256 mismatch: $path" }
    $path
}

$sources = @{}
foreach ($source in $spec.sources.PSObject.Properties) { $sources[$source.Name] = Get-OcrAsset $source.Value }
$models = @($spec.models | ForEach-Object { Get-OcrAsset $_ })
$build = Join-Path $CacheDirectory ('ocr-build-' + [Guid]::NewGuid().ToString('N'))
$originalPath = $env:PATH
$originalInclude = $env:INCLUDE
$originalLib = $env:LIB
try {
    $env:PATH = "$msvc/bin/Hostx64/x64;$sdk/bin/$sdkVersion/x64;$originalPath"
    $env:INCLUDE = "$msvc/include;$sdk/Include/$sdkVersion/ucrt;$sdk/Include/$sdkVersion/shared;$sdk/Include/$sdkVersion/um"
    $env:LIB = "$msvc/lib/x64;$sdk/Lib/$sdkVersion/ucrt/x64;$sdk/Lib/$sdkVersion/um/x64"
    Expand-Archive -LiteralPath $sources.leptonica -DestinationPath $build
    Expand-Archive -LiteralPath $sources.tesseract -DestinationPath $build
    $leptonica = Join-Path $build ([IO.Path]::GetFileNameWithoutExtension($sources.leptonica))
    $tesseract = Join-Path $build ([IO.Path]::GetFileNameWithoutExtension($sources.tesseract))
    $leptonicaBuild = Join-Path $build 'lept-build'
    $leptonicaInstall = Join-Path $build 'lept-install'
    $tesseractBuild = Join-Path $build 'tess-build'
    $compiler = "$msvc/bin/Hostx64/x64/cl.exe"
    $common = @('-G','Ninja',"-DCMAKE_MAKE_PROGRAM=$ninja",'-DCMAKE_BUILD_TYPE=Release',
        "-DCMAKE_C_COMPILER=$compiler", "-DCMAKE_CXX_COMPILER=$compiler",
        "-DCMAKE_RC_COMPILER=$sdk/bin/$sdkVersion/x64/rc.exe", "-DCMAKE_MT=$sdk/bin/$sdkVersion/x64/mt.exe",
        '-DCMAKE_POLICY_DEFAULT_CMP0091=NEW','-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded','-DBUILD_SHARED_LIBS=OFF')
    & $cmake -S $leptonica -B $leptonicaBuild @common "-DCMAKE_INSTALL_PREFIX=$leptonicaInstall" `
        -DBUILD_PROG=OFF -DSW_BUILD=OFF -DENABLE_ZLIB=OFF -DENABLE_PNG=OFF -DENABLE_GIF=OFF `
        -DENABLE_JPEG=OFF -DENABLE_TIFF=OFF -DENABLE_WEBP=OFF -DENABLE_OPENJPEG=OFF
    if ($LASTEXITCODE -ne 0) { throw 'Leptonica configuration failed.' }
    & $cmake --build $leptonicaBuild --target install --parallel 4
    if ($LASTEXITCODE -ne 0) { throw 'Leptonica build failed.' }
    & $cmake -S $tesseract -B $tesseractBuild @common "-DCMAKE_PREFIX_PATH=$leptonicaInstall" `
        -DBUILD_TRAINING_TOOLS=OFF -DGRAPHICS_DISABLED=ON -DDISABLED_LEGACY_ENGINE=ON `
        -DDISABLE_ARCHIVE=ON -DDISABLE_CURL=ON -DDISABLE_TIFF=ON -DOPENMP_BUILD=OFF `
        -DINSTALL_CONFIGS=OFF -DENABLE_CCACHE=OFF
    if ($LASTEXITCODE -ne 0) { throw 'Tesseract configuration failed.' }
    & $cmake --build $tesseractBuild --target tesseract --parallel 4
    if ($LASTEXITCODE -ne 0) { throw 'Tesseract build failed.' }

    $built = Join-Path $tesseractBuild 'bin/tesseract.exe'
    & $built --version
    if ($LASTEXITCODE -ne 0) { throw 'The built Tesseract executable did not run.' }
    $tessdata = Join-Path $runtime 'tessdata'
    [void][IO.Directory]::CreateDirectory($tessdata)
    Copy-Item -LiteralPath $models -Destination $tessdata -Force
    $licenses = Join-Path $projectRoot 'native/Tesseract'
    foreach ($name in @('SOURCE.txt','Tesseract-LICENSE.txt','Leptonica-LICENSE.txt','Tessdata-LICENSE.txt')) {
        if ($licenses -ne $runtime) { Copy-Item -LiteralPath (Join-Path $licenses $name) -Destination $runtime -Force }
    }
    Copy-Item -LiteralPath $built -Destination $runtime -Force
} finally {
    $env:PATH = $originalPath
    $env:INCLUDE = $originalInclude
    $env:LIB = $originalLib
    $resolved = [IO.Path]::GetFullPath($build)
    $prefix = $CacheDirectory.TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar
    if (-not $resolved.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) -or -not (Split-Path -Leaf $resolved).StartsWith('ocr-build-')) {
        throw 'Refusing cleanup outside the OCR build directory.'
    }
    if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force }
}
