[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$Archive
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$archivePath = (Resolve-Path -LiteralPath $Archive).Path
$verificationRoot = Join-Path $repoRoot ".scratch\package-verification\$([guid]::NewGuid())"
$verificationRoot = [System.IO.Path]::GetFullPath($verificationRoot)
$scratchPrefix = [System.IO.Path]::GetFullPath((Join-Path $repoRoot '.scratch\package-verification')).TrimEnd('\') + '\'
if (-not $verificationRoot.StartsWith($scratchPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Unsafe package verification path: $verificationRoot"
}

try {
    $sidecar = "$archivePath.sha256"
    if (Test-Path -LiteralPath $sidecar) {
        $expectedArchiveHash = ((Get-Content -LiteralPath $sidecar -Raw).Trim() -split '\s+')[0]
        $actualArchiveHash = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actualArchiveHash -ne $expectedArchiveHash.ToLowerInvariant()) {
            throw "Archive checksum mismatch: expected $expectedArchiveHash, got $actualArchiveHash"
        }
    }

    New-Item -ItemType Directory -Path $verificationRoot -Force | Out-Null
    Expand-Archive -LiteralPath $archivePath -DestinationPath $verificationRoot
    $roots = @(Get-ChildItem -LiteralPath $verificationRoot -Directory)
    if ($roots.Count -ne 1) {
        throw 'Package must contain exactly one root directory.'
    }
    $packageRoot = $roots[0].FullName
    $manifest = Join-Path $packageRoot 'manifest.sha256'
    if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) {
        throw 'Package manifest.sha256 is missing.'
    }

    foreach ($line in Get-Content -LiteralPath $manifest) {
        if ([string]::IsNullOrWhiteSpace($line)) { continue }
        if ($line -notmatch '^([a-f0-9]{64}) \*(.+)$') {
            throw "Malformed manifest entry: $line"
        }
        $expected = $Matches[1]
        $relative = $Matches[2].Replace('/', [System.IO.Path]::DirectorySeparatorChar)
        $candidate = [System.IO.Path]::GetFullPath((Join-Path $packageRoot $relative))
        $prefix = $packageRoot.TrimEnd('\') + '\'
        if (-not $candidate.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "Manifest path escapes package root: $relative"
        }
        if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) {
            throw "Manifest file is missing: $relative"
        }
        $actual = (Get-FileHash -LiteralPath $candidate -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actual -ne $expected) {
            throw "Manifest checksum mismatch for $relative"
        }
    }

    $doctor = Join-Path $packageRoot 'mimic-doctor.exe'
    & $doctor --version | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw 'Packaged mimic-doctor --version failed.'
    }
    $checkJson = (& $doctor check --json | Out-String)
    $checkExit = $LASTEXITCODE
    $check = $checkJson | ConvertFrom-Json
    if ($check.schema_version -ne 1 -or $check.command -ne 'check') {
        throw 'Packaged doctor returned an unexpected JSON contract.'
    }
    if ($checkExit -notin @(0, 3)) {
        throw "Packaged doctor check failed with exit code $checkExit.`n$checkJson"
    }
    Write-Host "Package verification passed: $archivePath"
}
finally {
    if (Test-Path -LiteralPath $verificationRoot) {
        Remove-Item -LiteralPath $verificationRoot -Recurse -Force
    }
}
