[CmdletBinding()]
param(
    [switch]$ReleaseApk,
    [switch]$DebugApk,
    [switch]$MultiAbi
)

. (Join-Path $PSScriptRoot 'build-common.ps1')
$output = Reset-ReleaseDirectory 'Android'
Require-Command java

# Tauri's Android build is driven by Gradle, which requires a JDK. The default
# system JDK (and the Android Studio JBR that Tauri auto-selects) may be too new
# for the Gradle version Tauri pins; prefer a JDK 17 already exposed through
# PATH (repository workaround, no install assumed). We pin it explicitly in the
# project gradle.properties because Tauri overrides JAVA_HOME with its own
# preferred JDK when spawning Gradle.
if ($IsWindows) {
    $jdk17 = $env:PATH.Split([System.IO.Path]::PathSeparator) | ForEach-Object {
        $candidateHome = Split-Path -Parent $_
        $releaseFile = Join-Path $candidateHome 'release'
        if ((Test-Path -LiteralPath (Join-Path $_ 'java.exe')) -and
            (Test-Path -LiteralPath $releaseFile) -and
            (Get-Content -Raw $releaseFile) -match 'JAVA_VERSION="17(?:\.|\")') {
            $candidateHome
        }
    } | Select-Object -First 1
    if ($jdk17) { $env:JAVA_HOME = $jdk17 }
}
$javaExecutable = if ($env:JAVA_HOME) { Join-Path $env:JAVA_HOME 'bin/java' } else { 'java' }
$javaDetails = (& $javaExecutable '-XshowSettings:properties' -version 2>&1 | Out-String)

# The repository-provided pipe workaround targets JDK 17 only.
$agent = Join-Path $ProjectRoot 'target/build-support/disable-unix-pipe-agent.jar'
if ($IsWindows -and $javaDetails -match 'java\.specification\.version\s*=\s*17' -and (Test-Path -LiteralPath $agent)) {
    $env:JAVA_TOOL_OPTIONS = "--add-opens=java.base/sun.nio.ch=ALL-UNNAMED -javaagent:$agent"
}

# Default to the smallest possible release package: a single arm64-v8a APK
# whose native libraries are stripped. Debug builds are opt-in for on-device
# debugging and keep symbols; -MultiAbi produces the (much larger) universal
# APK with every ABI.
if ($ReleaseApk) { $DebugApk = $false }
if ($DebugApk) {
    Write-Host 'Building DEBUG Android APK (unstripped, for on-device debugging).'
    if (-not $MultiAbi) { Write-Host '  Single ABI: arm64-v8a' }
} else {
    Write-Host 'Building RELEASE Android APK (stripped, single ABI arm64-v8a).'
    if ($MultiAbi) { Write-Host '  -MultiAbi: also building universal APK (all ABIs).' }
}

$arguments = @('android', 'build', '--apk', '--target', 'aarch64')
if ($DebugApk) { $arguments += '--debug' }

# Pin the Gradle JVM to JDK 17 (if available) in the generated Android project.
# Tauri's CLI picks Android Studio's bundled JBR, which is too new for Gradle.
$gradleProps = Join-Path $AndroidGenDir 'gradle.properties'
$jdkLine = 'org.gradle.java.home='
$hadJdkPin = $false
if (Test-Path -LiteralPath $gradleProps) {
    $content = Get-Content -Raw -LiteralPath $gradleProps
    $hadJdkPin = $content -match '(?m)^org\.gradle\.java\.home\s*='
    if ($jdk17) {
        $escaped = $jdk17.Replace('\', '\\').Replace(':', '\:')
        if ($hadJdkPin) {
            $content = [regex]::Replace($content, '(?m)^org\.gradle\.java\.home\s*=.*$', "org.gradle.java.home=$escaped")
        } else {
            $content = $content.TrimEnd() + "`n`n# Pinned by build-android.ps1: Gradle 8.x needs JDK 17, not the JBR 25 Tauri auto-selects.`norg.gradle.java.home=$escaped`n"
        }
        Set-Content -LiteralPath $gradleProps -Value $content -Encoding utf8 -NoNewline
        Write-Host "Pinned Gradle JVM: org.gradle.java.home=$jdk17"
    } elseif ($hadJdkPin) {
        # JDK 17 not found; keep the existing pin (it may be user-managed).
        Write-Host 'JDK 17 not found on PATH; reusing existing org.gradle.java.home pin.'
    }
}

try {
    Invoke-Tauri $arguments
} finally {
    # Restore the original gradle.properties if we modified it.
    if ($jdk17 -and -not $hadJdkPin) {
        $current = Get-Content -Raw -LiteralPath $gradleProps
        $current = $current -replace '(?ms)\r?\n\r?\n# Pinned by build-android\.ps1:.*\r?\norg\.gradle\.java\.home=.*$', ''
        Set-Content -LiteralPath $gradleProps -Value $current -Encoding utf8 -NoNewline
        Write-Host 'Restored original gradle.properties (removed JDK pin).'
    }
}

$variant = if ($DebugApk) { 'debug' } else { 'release' }
$apkRoot = Join-Path $TauriRoot 'gen/android/app/build/outputs/apk'
$apks = Get-ChildItem -LiteralPath $apkRoot -Filter '*.apk' -File -Recurse -ErrorAction SilentlyContinue |
    Where-Object { $_.Directory.Name -eq $variant }
if (-not $apks) { throw "No $variant APK was generated below $apkRoot" }

foreach ($apk in $apks) {
    Copy-Item -LiteralPath $apk.FullName -Destination $output -Force
    Write-Host "APK: $($apk.Name) ($([math]::Round($apk.Length / 1MB, 2)) MB)"
}

$bundleRoot = Join-Path $TauriRoot 'gen/android/app/build/outputs/bundle'
Get-ChildItem -LiteralPath $bundleRoot -Filter '*.aab' -File -Recurse -ErrorAction SilentlyContinue |
    Where-Object { $_.Directory.Name -eq $variant } |
    ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $output -Force }

& (Join-Path $PSScriptRoot 'build-root-module.ps1') -Destination $output
if (-not $?) { throw 'Root module build failed.' }
Show-Artifacts $output
