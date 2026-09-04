[CmdletBinding()]
param()

. (Join-Path $PSScriptRoot 'build-common.ps1')
if (-not $IsMacOS) { throw 'macOS packages must be built on macOS.' }
$output = Reset-ReleaseDirectory 'macOS'
Invoke-UiBuild
Invoke-Tauri @('build', '--bundles', 'app,dmg')
Copy-Matches (Join-Path $ProjectRoot 'target/release/bundle/dmg/*.dmg') $output -Required | Out-Null
Copy-Matches (Join-Path $ProjectRoot 'target/release/bundle/macos/*.app') $output -Required | Out-Null
Show-Artifacts $output
