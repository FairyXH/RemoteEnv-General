[CmdletBinding()]
param()

. (Join-Path $PSScriptRoot 'build-common.ps1')
if (-not $IsLinux) { throw 'Linux packages must be built on Linux.' }
$output = Reset-ReleaseDirectory 'Linux'
Invoke-UiBuild
Invoke-Tauri @('build', '--bundles', 'deb,appimage')
Copy-Matches (Join-Path $ProjectRoot 'target/release/bundle/deb/*.deb') $output -Required | Out-Null
Copy-Matches (Join-Path $ProjectRoot 'target/release/bundle/appimage/*.AppImage') $output -Required | Out-Null
$binary = Join-Path $ProjectRoot 'target/release/remote-env-desktop'
Require-File $binary 'The Linux release binary was not generated.'
Copy-Item -LiteralPath $binary -Destination (Join-Path $output 'RemoteEnvCollector') -Force
Show-Artifacts $output
