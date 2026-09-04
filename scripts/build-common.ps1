$ErrorActionPreference = 'Stop'

$ProjectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$UiRoot = Join-Path $ProjectRoot 'app/ui'
$TauriRoot = Join-Path $ProjectRoot 'app/desktop/src-tauri'
$ReleaseRoot = Join-Path $ProjectRoot 'Release'
$TauriCli = Join-Path $UiRoot 'node_modules/@tauri-apps/cli/tauri.js'
$AndroidGenDir = Join-Path $TauriRoot 'gen/android'

function Require-Command([string]$Name) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "Missing build dependency: $Name"
    }
}

function Require-File([string]$Path, [string]$Hint) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Missing file: $Path. $Hint"
    }
}

function Reset-ReleaseDirectory([string]$Platform) {
    $path = Join-Path $ReleaseRoot $Platform
    $resolvedRelease = [System.IO.Path]::GetFullPath($ReleaseRoot)
    $resolvedPath = [System.IO.Path]::GetFullPath($path)
    if (-not $resolvedPath.StartsWith($resolvedRelease + [System.IO.Path]::DirectorySeparatorChar)) {
        throw "Refusing to clean a path outside Release: $resolvedPath"
    }
    if (Test-Path -LiteralPath $resolvedPath) {
        Remove-Item -LiteralPath $resolvedPath -Recurse -Force
    }
    New-Item -ItemType Directory -Path $resolvedPath -Force | Out-Null
    return $resolvedPath
}

function Invoke-UiBuild {
    Require-Command npm
    & npm --prefix $UiRoot run build
    if ($LASTEXITCODE -ne 0) { throw 'UI build failed.' }
}

function Invoke-Tauri([string[]]$Arguments) {
    Require-Command node
    Require-Command cargo
    Require-File $TauriCli 'Run npm ci in app/ui first.'
    Push-Location $TauriRoot
    try {
        & node $TauriCli @Arguments
        if ($LASTEXITCODE -ne 0) { throw "Tauri build failed: $($Arguments -join ' ')" }
    } finally {
        Pop-Location
    }
}

function Copy-Matches([string]$Pattern, [string]$Destination, [switch]$Required) {
    $items = Get-ChildItem -Path $Pattern -ErrorAction SilentlyContinue
    if ($Required -and -not $items) { throw "No build artifact matched: $Pattern" }
    foreach ($item in $items) {
        Copy-Item -LiteralPath $item.FullName -Destination $Destination -Recurse -Force
    }
    return @($items).Count
}

function Show-Artifacts([string]$Directory) {
    Write-Host "Artifacts in $Directory"
    Get-ChildItem -LiteralPath $Directory -Recurse -File | ForEach-Object {
        Write-Host "  $($_.FullName.Substring($Directory.Length + 1))"
    }
}
