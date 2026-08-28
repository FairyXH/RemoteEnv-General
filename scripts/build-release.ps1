[CmdletBinding()]
param(
    [switch]$SkipBundle
)

$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot
$releaseRoot = Join-Path $projectRoot "Release\Windows"
$uiRoot = Join-Path $projectRoot "app\ui"
$tauriRoot = Join-Path $projectRoot "app\desktop\src-tauri"
$configPath = Join-Path $tauriRoot "tauri.conf.json"

function Require-Command([string]$Name) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "缺少构建依赖: $Name。请安装后重新运行此脚本。"
    }
}

function Copy-Artifact([string]$Source, [string]$DestinationDirectory) {
    if (-not (Test-Path $Source)) {
        return $false
    }
    Copy-Item -Path $Source -Destination $DestinationDirectory -Force
    return $true
}

Set-Location $projectRoot
Require-Command cargo
Require-Command npm

$config = Get-Content -Raw $configPath | ConvertFrom-Json
$version = $config.version
if ([string]::IsNullOrWhiteSpace($version) -or $version -notmatch '^\d+\.\d+\.\d+$') {
    throw "Tauri version 必须使用 MAJOR.MINOR.PATCH 格式。"
}

if (Test-Path $releaseRoot) {
    Remove-Item -Path $releaseRoot -Recurse -Force
}
New-Item -ItemType Directory -Path $releaseRoot -Force | Out-Null

Write-Host "构建 React 生产资源..."
npm --prefix $uiRoot run build
if ($LASTEXITCODE -ne 0) { throw "React production build 失败。" }

Write-Host "构建 Tauri Release executable..."
cargo build -p remote-env-desktop --release
if ($LASTEXITCODE -ne 0) { throw "Tauri Rust Release build 失败。" }

$portableRoot = Join-Path $releaseRoot "RemoteEnvCollector"
New-Item -ItemType Directory -Path $portableRoot -Force | Out-Null
$exe = Join-Path $projectRoot "target\release\remote-env-desktop.exe"
if (-not (Copy-Artifact $exe $portableRoot)) {
    throw "未找到 Release executable: $exe"
}
Rename-Item -Path (Join-Path $portableRoot "remote-env-desktop.exe") -NewName "RemoteEnvCollector.exe"

$bundleCreated = $false
if (-not $SkipBundle) {
    $cargoTauri = Get-Command cargo-tauri -ErrorAction SilentlyContinue
    if ($cargoTauri) {
        Write-Host "构建 NSIS installer..."
        cargo tauri build --bundles nsis
        if ($LASTEXITCODE -ne 0) { throw "Tauri NSIS bundle 失败。" }
        $bundles = Get-ChildItem -Path (Join-Path $projectRoot "target\release\bundle\nsis") -Filter "*.exe" -File -ErrorAction SilentlyContinue
        foreach ($bundle in $bundles) {
            Copy-Item $bundle.FullName (Join-Path $releaseRoot "RemoteEnvCollector-Setup.exe") -Force
            $bundleCreated = $true
            break
        }
    } else {
        Write-Warning "未检测到 cargo-tauri；已生成 Portable 包，未生成 NSIS installer。"
    }
}

$portableExe = Join-Path $portableRoot "RemoteEnvCollector.exe"
if (-not (Test-Path $portableExe)) { throw "Portable artifact 不完整。" }
$metadata = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($portableExe)

Write-Host ""
Write-Host "RemoteEnvCollector Release"
Write-Host "平台: Windows"
Write-Host "版本: $version"
Write-Host "构建: release"
Write-Host ""
Write-Host "Artifacts:"
Get-ChildItem -Path $releaseRoot -Recurse -File | ForEach-Object { Write-Host "  $($_.FullName.Substring($releaseRoot.Length + 1))" }
Write-Host ""
Write-Host "输出: $releaseRoot"
Write-Host ""
Write-Host "Checks:"
Write-Host "  UI build             PASS"
Write-Host "  Tauri Rust build     PASS"
Write-Host "  executable exists    PASS"
Write-Host "  installer bundle     $(if ($bundleCreated) { 'PASS' } else { 'NOT GENERATED' })"
Write-Host "  file description     $($metadata.FileDescription)"