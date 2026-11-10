# Registry operations accept an explicit base key so tests use an isolated HKCU subtree.
# Production passes CurrentUser; no machine-wide keys or default assignments are changed.
$RustViewImageExtensions = @(
    'jpg','jpeg','jpe','jfif','png','webp','gif','bmp','dib','tif','tiff','ico',
    'pnm','pbm','pgm','ppm','pam','tga','qoi','heic','heif','hif','avif','jxl','psd','psb',
    '3fr','arw','cr2','cr3','crw','dcr','dcs','dng','erf','fff','gpr','iiq','kdc','mef',
    'mos','mrw','nef','nrw','orf','pef','raf','raw','rw2','rwl','sr2','srf','srw','x3f'
)
$RustViewArchiveExtensions = @('zip','rar','7z')

function Set-RustViewRegistryValue($BaseKey, [string]$Path, [string]$Name, [string]$Value) {
    $key = $BaseKey.CreateSubKey($Path)
    try { $key.SetValue($Name, $Value, [Microsoft.Win32.RegistryValueKind]::String) }
    finally { $key.Dispose() }
}

function Install-RustViewAssociations($BaseKey, [string]$Executable) {
    $command = '"' + $Executable + '" "%1"'
    $icon = '"' + $Executable + '",0'
    $classes = 'Software\Classes'
    foreach ($kind in @('Image','Archive')) {
        $progId = 'RustView.' + $kind
        Set-RustViewRegistryValue $BaseKey "$classes\$progId" '' "RustView $kind"
        Set-RustViewRegistryValue $BaseKey "$classes\$progId\DefaultIcon" '' $icon
        Set-RustViewRegistryValue $BaseKey "$classes\$progId\shell\open\command" '' $command
        Set-RustViewRegistryValue $BaseKey "$classes\$progId" 'MultiSelectModel' 'Single'
        $extensions = if ($kind -eq 'Image') { $RustViewImageExtensions } else { $RustViewArchiveExtensions }
        foreach ($extension in $extensions) {
            Set-RustViewRegistryValue $BaseKey "$classes\.$extension\OpenWithProgids" $progId ''
            Set-RustViewRegistryValue $BaseKey "$classes\Applications\RustView.exe\SupportedTypes" ".$extension" ''
            if ($kind -eq 'Image') {
                Set-RustViewRegistryValue $BaseKey 'Software\RustView\Capabilities\FileAssociations' ".$extension" $progId
            }
        }
    }
    Set-RustViewRegistryValue $BaseKey "$classes\Applications\RustView.exe" 'FriendlyAppName' 'RustView'
    Set-RustViewRegistryValue $BaseKey "$classes\Applications\RustView.exe" 'MultiSelectModel' 'Single'
    Set-RustViewRegistryValue $BaseKey "$classes\Applications\RustView.exe\shell\open\command" '' $command
    Set-RustViewRegistryValue $BaseKey 'Software\RustView\Capabilities' 'ApplicationName' 'RustView'
    Set-RustViewRegistryValue $BaseKey 'Software\RustView\Capabilities' 'ApplicationDescription' 'Image, camera RAW, PSD and ZIP/RAR/7z image viewer'
    Set-RustViewRegistryValue $BaseKey 'Software\RustView\Capabilities' 'ApplicationIcon' $icon
    Set-RustViewRegistryValue $BaseKey 'Software\RustView\Capabilities' 'RustViewExecutable' $Executable
    Set-RustViewRegistryValue $BaseKey 'Software\RegisteredApplications' 'RustView' 'Software\RustView\Capabilities'
    foreach ($extension in $RustViewArchiveExtensions) {
        $verb = "$classes\SystemFileAssociations\.$extension\shell\RustView.Open"
        Set-RustViewRegistryValue $BaseKey $verb '' 'Open with RustView'
        Set-RustViewRegistryValue $BaseKey $verb 'Icon' $icon
        Set-RustViewRegistryValue $BaseKey $verb 'MultiSelectModel' 'Single'
        Set-RustViewRegistryValue $BaseKey "$verb\command" '' $command
    }
}

function Remove-RustViewAssociations($BaseKey, [string]$Executable) {
    $owner = $BaseKey.OpenSubKey('Software\RustView\Capabilities')
    if (-not $owner) { return }
    try { $registered = $owner.GetValue('RustViewExecutable') } finally { $owner.Dispose() }
    if ($registered -ne $Executable) { throw 'Another RustView folder owns the registration. Run Remove-File-Associations.cmd in that folder.' }
    foreach ($extension in $RustViewImageExtensions + $RustViewArchiveExtensions) {
        $key = $BaseKey.OpenSubKey("Software\Classes\.$extension\OpenWithProgids", $true)
        if ($key) {
            try { $key.DeleteValue('RustView.Image', $false); $key.DeleteValue('RustView.Archive', $false) }
            finally { $key.Dispose() }
        }
    }
    $key = $BaseKey.OpenSubKey('Software\RegisteredApplications', $true)
    if ($key) { try { $key.DeleteValue('RustView', $false) } finally { $key.Dispose() } }
    foreach ($path in @('Software\Classes\RustView.Image','Software\Classes\RustView.Archive',
            'Software\Classes\Applications\RustView.exe','Software\RustView\Capabilities') +
            @($RustViewArchiveExtensions | ForEach-Object { "Software\Classes\SystemFileAssociations\.$_\shell\RustView.Open" })) {
        $BaseKey.DeleteSubKeyTree($path, $false)
    }
}
