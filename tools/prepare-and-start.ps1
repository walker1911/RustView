#Requires -Version 5.1
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
try {
    $manifest = Get-Content -LiteralPath (Join-Path $projectRoot 'native/manifest.json') -Raw | ConvertFrom-Json
    $missing = @($manifest.runtime_sha256.PSObject.Properties | Where-Object { -not (Test-Path -LiteralPath (Join-Path $projectRoot ('native/' + $_.Name)) -PathType Leaf) })
    if ($missing.Count -gt 0 -or -not (Test-Path -LiteralPath (Join-Path $projectRoot 'native/ExifTool/exiftool_files/perl532.dll'))) {
        Write-Host 'RustView: first launch needs an Internet connection to prepare the official image engines.'
        Write-Host 'Downloads are pinned and checked with SHA-256. Later launches work offline.'
        & (Join-Path $PSScriptRoot 'fetch-native.ps1')
    }
    foreach ($item in $manifest.runtime_sha256.PSObject.Properties) {
        $path = Join-Path $projectRoot ('native/' + $item.Name)
        if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $item.Value) { throw "Native runtime checksum mismatch: $($item.Name)" }
    }
    Start-Process -FilePath (Join-Path $projectRoot 'RustView.exe') -WorkingDirectory $projectRoot
} catch {
    Write-Host ('RustView setup failed: ' + $_.Exception.Message) -ForegroundColor Red
    Write-Host 'You can retry Start-RustView.cmd, or run tools/fetch-native.ps1 from PowerShell.'
    [void](Read-Host 'Press Enter to close')
    exit 1
}
