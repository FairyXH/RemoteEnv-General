[CmdletBinding()]
param([string]$Destination)

. (Join-Path $PSScriptRoot 'build-common.ps1')
$source = Join-Path $ProjectRoot 'android/root-module'
if (-not $Destination) {
    $Destination = Join-Path $ReleaseRoot 'Android'
}
New-Item -ItemType Directory -Path $Destination -Force | Out-Null
$archive = Join-Path $Destination 'RemoteEnvCollector-Magisk-KernelSU.zip'
if (Test-Path -LiteralPath $archive) { Remove-Item -LiteralPath $archive -Force }
$entries = @('module.prop', 'customize.sh', 'service.sh', 'uninstall.sh', 'README.md') |
    ForEach-Object { Join-Path $source $_ }
foreach ($entry in $entries) { Require-File $entry 'Root module source is incomplete.' }
Compress-Archive -Path $entries -DestinationPath $archive -CompressionLevel Optimal
Write-Host "Root module: $archive"
