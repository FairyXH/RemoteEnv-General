[CmdletBinding()]
param(
    [switch]$ReleaseAndroid,
    [switch]$RequireEveryPlatform
)

$ErrorActionPreference = 'Stop'
$failures = [System.Collections.Generic.List[string]]::new()
$skipped = [System.Collections.Generic.List[string]]::new()

function Run-Build([string]$Name, [string]$Script, [hashtable]$Parameters = @{}) {
    Write-Host "`n=== Building $Name ===" -ForegroundColor Cyan
    try {
        & (Join-Path $PSScriptRoot $Script) @Parameters
        if (-not $?) { throw "$Name build returned a failure status." }
    } catch {
        $failures.Add("$Name`: $($_.Exception.Message)")
    }
}

if ($IsWindows -or $PSVersionTable.PSEdition -eq 'Desktop') {
    Run-Build 'Windows' 'build-windows.ps1'
    $skipped.Add('Linux packages require a Linux host.')
    $skipped.Add('macOS packages require a macOS host and Apple toolchain.')
} elseif ($IsLinux) {
    Run-Build 'Linux' 'build-linux.ps1'
    $skipped.Add('Windows packages require a Windows host.')
    $skipped.Add('macOS packages require a macOS host and Apple toolchain.')
} elseif ($IsMacOS) {
    Run-Build 'macOS' 'build-macos.ps1'
    $skipped.Add('Windows packages require a Windows host.')
    $skipped.Add('Linux packages require a Linux host.')
}

Run-Build 'Android + Magisk/KernelSU' 'build-android.ps1' @{ ReleaseApk = $ReleaseAndroid }

Write-Host "`n=== Build summary ===" -ForegroundColor Cyan
foreach ($item in $skipped) { Write-Host "SKIPPED: $item" -ForegroundColor Yellow }
foreach ($item in $failures) { Write-Host "FAILED:  $item" -ForegroundColor Red }

if ($failures.Count -gt 0 -or ($RequireEveryPlatform -and $skipped.Count -gt 0)) {
    exit 1
}
Write-Host 'All buildable targets for this host completed.' -ForegroundColor Green
