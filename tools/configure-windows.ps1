#Requires -Version 5.1
[CmdletBinding(SupportsShouldProcess)]
param([switch]$Uninstall, [switch]$NoSettings)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-associations.ps1')
$executable = [IO.Path]::GetFullPath((Join-Path (Split-Path -Parent $PSScriptRoot) 'RustView.exe'))
if (-not $Uninstall -and -not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    throw 'RustView.exe is missing. Run this script from a complete, extracted RustView package.'
}
$action = if ($Uninstall) { 'Remove RustView file associations and archive context menus' } else { 'Register RustView file types and archive context menus' }
if (-not $PSCmdlet.ShouldProcess($executable, $action)) { return }
if ($Uninstall) { Remove-RustViewAssociations ([Microsoft.Win32.Registry]::CurrentUser) $executable }
else { Install-RustViewAssociations ([Microsoft.Win32.Registry]::CurrentUser) $executable }
# Notify Explorer without restarting it. This does not assign a default application.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class RustViewShell {
    [DllImport("shell32.dll")] public static extern void SHChangeNotify(uint e, uint f, IntPtr a, IntPtr b);
}
'@
[RustViewShell]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
if ($Uninstall) { Write-Output 'Removed RustView registrations. Choose another default in Windows Settings if needed.' }
else {
    Write-Output 'Registered supported image types and ZIP/RAR/7z context menus for this user.'
    Write-Output 'Select RustView in Windows Default Apps to confirm your preferred defaults.'
    if (-not $NoSettings) { Start-Process 'ms-settings:defaultapps?registeredAppUser=RustView' }
}
