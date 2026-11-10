#Requires -Version 5.1
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-associations.ps1')
function Assert($Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Read-Value([string]$Path, [string]$Name = '') {
    $key = $sandbox.OpenSubKey($Path)
    if (-not $key) { return $null }
    try { return $key.GetValue($Name) } finally { $key.Dispose() }
}
$testPath = 'Software\RustView.AssociationTests\' + [Guid]::NewGuid().ToString('N')
$sandbox = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($testPath)
try {
    $executable = 'C:\' + [char]0x7167 + [char]0x7247 + ' & RAW 100%\RustView.exe'
    $command = '"' + $executable + '" "%1"'
    Set-RustViewRegistryValue $sandbox 'Software\Classes\.zip' '' 'Existing.Archive'
    Set-RustViewRegistryValue $sandbox 'Software\Classes\.jpg\OpenWithProgids' 'Other.Viewer' ''
    Set-RustViewRegistryValue $sandbox 'Software\Classes\SystemFileAssociations\.zip\shell\Other' '' 'Keep me'
    Set-RustViewRegistryValue $sandbox 'Software\RegisteredApplications' 'OtherApp' 'Software\Other\Capabilities'
    Set-RustViewRegistryValue $sandbox 'Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.jpg\UserChoice' 'ProgId' 'Existing.Viewer'
    Install-RustViewAssociations $sandbox $executable
    Install-RustViewAssociations $sandbox $executable
    foreach ($extension in $RustViewImageExtensions + $RustViewArchiveExtensions) {
        Assert ((Read-Value 'Software\Classes\Applications\RustView.exe\SupportedTypes' ".$extension") -eq '') "Missing type $extension"
        if ($RustViewImageExtensions -contains $extension) {
            Assert ((Read-Value 'Software\RustView\Capabilities\FileAssociations' ".$extension") -eq 'RustView.Image') "Missing capability $extension"
        } else {
            Assert ($null -eq (Read-Value 'Software\RustView\Capabilities\FileAssociations' ".$extension")) 'Archive must not be included in image default assignments'
        }
    }
    foreach ($extension in $RustViewArchiveExtensions) {
        Assert ((Read-Value "Software\Classes\SystemFileAssociations\.$extension\shell\RustView.Open\command") -eq $command) "Archive command quoting: $extension"
    }
    Assert ((Read-Value 'Software\Classes\RustView.Image\shell\open\command') -eq $command) 'Image command quoting'
    Assert ((Read-Value 'Software\Classes\.zip') -eq 'Existing.Archive') 'Archive default was changed'
    Assert ((Read-Value 'Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.jpg\UserChoice' 'ProgId') -eq 'Existing.Viewer') 'UserChoice was changed'
    $newExecutable = 'C:\New Folder\RustView.exe'
    Install-RustViewAssociations $sandbox $newExecutable
    $refused = $false
    try { Remove-RustViewAssociations $sandbox $executable } catch { $refused = $true }
    Assert $refused 'An old portable folder must not remove a newer registration'
    Assert ((Read-Value 'Software\RegisteredApplications' 'RustView') -eq 'Software\RustView\Capabilities') 'Owner check changed registration'
    Assert ((Read-Value 'Software\Classes\RustView.Image\shell\open\command') -eq ('"' + $newExecutable + '" "%1"')) 'New portable path was not registered'
    Remove-RustViewAssociations $sandbox $newExecutable
    Remove-RustViewAssociations $sandbox $newExecutable
    Assert ($null -eq (Read-Value 'Software\RegisteredApplications' 'RustView')) 'Uninstall left app registration'
    Assert ($null -eq (Read-Value 'Software\Classes\RustView.Image\shell\open\command')) 'Uninstall left image command'
    foreach ($extension in $RustViewArchiveExtensions) {
        Assert ($null -eq (Read-Value "Software\Classes\SystemFileAssociations\.$extension\shell\RustView.Open\command")) 'Uninstall left archive verb'
    }
    Assert ((Read-Value 'Software\Classes\.jpg\OpenWithProgids' 'Other.Viewer') -eq '') 'Uninstall removed another viewer'
    Assert ((Read-Value 'Software\Classes\SystemFileAssociations\.zip\shell\Other') -eq 'Keep me') 'Uninstall removed another context menu'
    Assert ((Read-Value 'Software\RegisteredApplications' 'OtherApp') -eq 'Software\Other\Capabilities') 'Uninstall removed another app'
    Assert ((Read-Value 'Software\Classes\.zip') -eq 'Existing.Archive') 'Uninstall changed the archive default'
    Assert ((Read-Value 'Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.jpg\UserChoice' 'ProgId') -eq 'Existing.Viewer') 'Uninstall changed UserChoice'
    $imaging = Get-Content -LiteralPath (Join-Path (Split-Path -Parent $PSScriptRoot) 'src/imaging.rs') -Raw
    $supported = [regex]::Match($imaging, '(?s)pub fn is_raw\(.*?(?=\nfn output_size)').Value
    $extensions = @([regex]::Matches($supported, '"([a-z0-9]+)"') | ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique)
    Assert (-not (Compare-Object $extensions ($RustViewImageExtensions | Sort-Object -Unique))) 'Windows registration differs from decoder format gate'
    Write-Output ('PASS: ' + $extensions.Count + ' image formats, 3 archive verbs; repeat install/remove, ownership, quoting, defaults and other apps preserved. Isolated registry only.')
} finally {
    $sandbox.Dispose()
    if ($testPath -notmatch '^Software\\RustView[.]AssociationTests\\[a-f0-9]{32}$') { throw 'Unexpected test cleanup path' }
    [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($testPath, $false)
}
