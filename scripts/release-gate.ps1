[CmdletBinding()]
param(
    [switch]$SkipAudit,
    [switch]$SkipPackage
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'msvc-environment.ps1')

function Assert-Success([string]$Step) {
    if ($LASTEXITCODE -ne 0) {
        throw "$Step failed with exit code $LASTEXITCODE."
    }
}

$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Initialize-MimicMsvcEnvironment
Push-Location $repoRoot
try {
    & cargo fmt --all -- --check
    Assert-Success 'format check'
    & cargo test --locked --all-targets
    Assert-Success 'test suite'
    & cargo clippy --locked --all-targets -- -D warnings
    Assert-Success 'clippy'
    if (-not $SkipAudit) {
        & (Join-Path $PSScriptRoot 'verify-audit.ps1')
    }
    & cargo build --locked --release --bins
    Assert-Success 'release build'
    if (-not $SkipPackage) {
        & (Join-Path $PSScriptRoot 'package.ps1') -SkipBuild
        $archive = Get-ChildItem -LiteralPath (Join-Path $repoRoot 'dist') -Filter 'mimic-v*-windows-x64.zip' -File |
            Sort-Object LastWriteTime -Descending |
            Select-Object -First 1
        if (-not $archive) {
            throw 'Packaging completed without a release archive.'
        }
        & (Join-Path $PSScriptRoot 'verify-package.ps1') -Archive $archive.FullName
    }
    Write-Host 'Mimic release gate passed.'
}
finally {
    Pop-Location
}
