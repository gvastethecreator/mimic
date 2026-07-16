[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Push-Location $repoRoot
try {
    if (-not (Get-Command cargo-audit -ErrorAction SilentlyContinue)) {
        throw 'cargo-audit 0.22.2 is required. Install it with: cargo install cargo-audit --version 0.22.2 --locked'
    }
    $auditVersion = (& cargo audit --version | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or $auditVersion -notmatch '\b0\.22\.2\b') {
        throw "cargo-audit 0.22.2 is required; found: $auditVersion"
    }

    $target = 'x86_64-pc-windows-msvc'
    foreach ($crate in @('quick-xml', 'anyhow', 'memmap2')) {
        $tree = (& cargo tree --locked --target $target --invert $crate 2>&1 | Out-String)
        if ($LASTEXITCODE -ne 0) {
            throw "Could not inspect the Windows dependency graph for $crate.`n$tree"
        }
        if ($tree -match "(?m)^$([regex]::Escape($crate)) v") {
            throw "$crate unexpectedly entered the $target dependency graph.`n$tree"
        }
    }

    & cargo audit --ignore RUSTSEC-2026-0194 --ignore RUSTSEC-2026-0195
    if ($LASTEXITCODE -ne 0) {
        throw 'cargo audit reported an unaccounted vulnerability.'
    }
    Write-Host 'Audit policy passed: ignored Wayland advisories are absent from the Windows graph.'
}
finally {
    Pop-Location
}
