[CmdletBinding()]
param()

. (Join-Path $PSScriptRoot 'build-common.ps1')
if (-not $IsWindows -and $PSVersionTable.PSEdition -eq 'Core') {
    throw 'Windows packages must be built on Windows.'
}

$output = Reset-ReleaseDirectory 'Windows'
Invoke-UiBuild
Invoke-Tauri @('build', '--bundles', 'nsis')

$portable = Join-Path $output 'RemoteEnvCollector'
New-Item -ItemType Directory -Path $portable -Force | Out-Null
$binary = Join-Path $ProjectRoot 'target/release/remote-env-desktop.exe'
Require-File $binary 'The Windows release binary was not generated.'
Copy-Item -LiteralPath $binary -Destination (Join-Path $portable 'RemoteEnvCollector.exe') -Force
$version = (Get-Content -Raw (Join-Path $TauriRoot 'tauri.conf.json') | ConvertFrom-Json).version
$installer = Get-ChildItem (Join-Path $ProjectRoot "target/release/bundle/nsis/*_${version}_*-setup.exe") -File
if (@($installer).Count -ne 1) { throw "Expected exactly one NSIS installer for version $version." }
Copy-Item -LiteralPath $installer.FullName -Destination (Join-Path $output 'RemoteEnvCollector-Setup.exe') -Force
Show-Artifacts $output
