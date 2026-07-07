# Local development build script (Windows)
# Builds the plugin. Deploys to the test project only when -Deploy is given.
#
# Usage:
#   .\scripts\build.ps1                            # debug build only
#   .\scripts\build.ps1 -Deploy                    # debug build + deploy
#   .\scripts\build.ps1 -Deploy -TestProject C:\path\to\project
#   .\scripts\build.ps1 -Release                   # release build only
#   .\scripts\build.ps1 -SkipChecks                # skip cargo fmt/clippy

param(
    [switch]$Deploy,
    [string]$TestProject = (Join-Path $HOME 'dev\godot-camerafeed-demo'),
    [switch]$Release,
    [switch]$SkipChecks
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

function Invoke-Cargo {
    param([string[]]$CargoArgs)
    Write-Host ">> cargo $($CargoArgs -join ' ')" -ForegroundColor Cyan
    cargo @CargoArgs
    if ($LASTEXITCODE -ne 0) {
        Write-Error "cargo $($CargoArgs -join ' ') failed (exit $LASTEXITCODE)"
    }
}

if (-not $SkipChecks) {
    Invoke-Cargo @('fmt')
    Invoke-Cargo @('clippy')
}

$profile_ = if ($Release) { 'release' } else { 'debug' }
$buildArgs = @('build')
if ($Release) { $buildArgs += '--release' }
Invoke-Cargo $buildArgs

$dll = Join-Path $repoRoot "target\$profile_\godot_neovim.dll"
if (-not (Test-Path $dll)) {
    Write-Error "Build artifact not found: $dll"
}

if (-not $Deploy) {
    Write-Host "Done: $profile_ build ($dll)" -ForegroundColor Green
    Write-Host "(use -Deploy to copy to the test project)"
    return
}

if (-not (Test-Path $TestProject)) {
    Write-Error "Test project not found: $TestProject (use -TestProject to specify)"
}

$addonDest = Join-Path $TestProject 'addons\godot-neovim'
$binDest   = Join-Path $addonDest 'bin\windows'
$luaDest   = Join-Path $addonDest 'lua\godot_neovim'
$inputDest = Join-Path $addonDest 'input'

foreach ($dir in @($binDest, $luaDest, $inputDest)) {
    New-Item -ItemType Directory -Force $dir | Out-Null
}

Write-Host ">> Deploying to $addonDest" -ForegroundColor Cyan
Copy-Item $dll $binDest -Force
Copy-Item (Join-Path $repoRoot 'addons\godot-neovim\lua\godot_neovim\*.lua') $luaDest -Force
Copy-Item (Join-Path $repoRoot 'addons\godot-neovim\input\*.gd') $inputDest -Force

Write-Host "Done: $profile_ build deployed to $TestProject" -ForegroundColor Green
