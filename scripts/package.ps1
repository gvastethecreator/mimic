[CmdletBinding()]
param(
    [string]$OutputDirectory = 'dist',
    [switch]$SkipBuild,
    [switch]$Sign,
    [string]$CertificateThumbprint,
    [string]$TimestampUrl = 'http://timestamp.digicert.com'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'msvc-environment.ps1')

function Assert-Success([string]$Step) {
    if ($LASTEXITCODE -ne 0) {
        throw "$Step failed with exit code $LASTEXITCODE."
    }
}

function Get-SignTool {
    $command = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    if (Test-Path -LiteralPath $kits) {
        $candidate = Get-ChildItem -LiteralPath $kits -Filter signtool.exe -File -Recurse |
            Where-Object { $_.FullName -match '\\x64\\signtool\.exe$' } |
            Sort-Object FullName -Descending |
            Select-Object -First 1
        if ($candidate) {
            return $candidate.FullName
        }
    }
    throw 'Signing was requested, but SignTool was not found in PATH or the Windows SDK.'
}

function Assert-RepoChild([string]$Path, [string]$RepoRoot) {
    $full = [System.IO.Path]::GetFullPath($Path)
    $prefix = $RepoRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    if (-not $full.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Release output must stay inside the repository: $full"
    }
    return $full
}

function Write-Utf8NoBom([string]$Path, [string]$Content) {
    [System.IO.File]::WriteAllText($Path, $Content, [System.Text.UTF8Encoding]::new($false))
}

function New-DeterministicZip(
    [string]$SourceDirectory,
    [string]$ArchivePath,
    [string]$RootName,
    [long]$SourceDateEpoch
) {
    Add-Type -AssemblyName System.IO.Compression
    $stream = [System.IO.File]::Open($ArchivePath, [System.IO.FileMode]::CreateNew)
    try {
        $archive = [System.IO.Compression.ZipArchive]::new(
            $stream,
            [System.IO.Compression.ZipArchiveMode]::Create,
            $false
        )
        try {
            $timestamp = [System.DateTimeOffset]::FromUnixTimeSeconds($SourceDateEpoch)
            foreach ($file in Get-ChildItem -LiteralPath $SourceDirectory -File -Recurse | Sort-Object FullName) {
                $relative = [System.IO.Path]::GetRelativePath($SourceDirectory, $file.FullName).Replace('\', '/')
                $entry = $archive.CreateEntry(
                    "$RootName/$relative",
                    [System.IO.Compression.CompressionLevel]::Optimal
                )
                $entry.LastWriteTime = $timestamp
                $input = [System.IO.File]::OpenRead($file.FullName)
                $output = $entry.Open()
                try {
                    $input.CopyTo($output)
                }
                finally {
                    $output.Dispose()
                    $input.Dispose()
                }
            }
        }
        finally {
            $archive.Dispose()
        }
    }
    finally {
        $stream.Dispose()
    }
}

$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$outputRoot = if ([System.IO.Path]::IsPathRooted($OutputDirectory)) {
    Assert-RepoChild $OutputDirectory $repoRoot
}
else {
    Assert-RepoChild (Join-Path $repoRoot $OutputDirectory) $repoRoot
}

Push-Location $repoRoot
try {
    if (-not $SkipBuild) {
        Initialize-MimicMsvcEnvironment
    }
    $metadata = (& cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json)
    Assert-Success 'cargo metadata'
    $package = $metadata.packages | Where-Object name -eq 'mimic' | Select-Object -First 1
    if (-not $package) {
        throw 'Cargo metadata did not contain the Mimic package.'
    }
    $version = $package.version
    $rootName = "mimic-v$version-windows-x64"
    $stage = Assert-RepoChild (Join-Path $outputRoot $rootName) $repoRoot
    $archivePath = Assert-RepoChild (Join-Path $outputRoot "$rootName.zip") $repoRoot
    $archiveChecksumPath = "$archivePath.sha256"

    if (-not $SkipBuild) {
        & cargo build --locked --release --bins
        Assert-Success 'release build'
    }
    foreach ($binary in @('mimic.exe', 'mimic-doctor.exe')) {
        $path = Join-Path $repoRoot "target\release\$binary"
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Release binary is missing: $path"
        }
    }

    New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
    if (Test-Path -LiteralPath $stage) {
        Remove-Item -LiteralPath $stage -Recurse -Force
    }
    if (Test-Path -LiteralPath $archivePath) {
        Remove-Item -LiteralPath $archivePath -Force
    }
    if (Test-Path -LiteralPath $archiveChecksumPath) {
        Remove-Item -LiteralPath $archiveChecksumPath -Force
    }
    New-Item -ItemType Directory -Path $stage | Out-Null

    Copy-Item -LiteralPath 'target\release\mimic.exe' -Destination $stage
    Copy-Item -LiteralPath 'target\release\mimic-doctor.exe' -Destination $stage
    Copy-Item -LiteralPath 'README.md', 'CHANGELOG.md', 'LICENSE' -Destination $stage
    Copy-Item -LiteralPath 'docs\release\runbook.md' -Destination (Join-Path $stage 'RELEASE-RUNBOOK.md')

    if ($Sign) {
        if ([string]::IsNullOrWhiteSpace($CertificateThumbprint)) {
            throw 'Signing was requested, but -CertificateThumbprint is empty.'
        }
        $signTool = Get-SignTool
        foreach ($binary in @('mimic.exe', 'mimic-doctor.exe')) {
            $path = Join-Path $stage $binary
            & $signTool sign /sha1 $CertificateThumbprint /fd SHA256 /tr $TimestampUrl /td SHA256 $path
            Assert-Success "signing $binary"
            & $signTool verify /pa /v $path
            Assert-Success "signature verification for $binary"
        }
    }

    $fullMetadata = (& cargo metadata --locked --format-version 1 | ConvertFrom-Json)
    Assert-Success 'dependency metadata'
    $noticeLines = @(
        'Mimic third-party dependency inventory'
        'Generated from Cargo.lock; review license obligations before public distribution.'
        ''
    )
    $noticeLines += $fullMetadata.packages |
        Where-Object name -ne 'mimic' |
        Sort-Object name, version -Unique |
        ForEach-Object {
            $license = if ($_.license) { $_.license } elseif ($_.license_file) { "license-file: $($_.license_file)" } else { 'UNKNOWN' }
            $source = if ($_.repository) { $_.repository } else { $_.source }
            "$($_.name) $($_.version) | $license | $source"
        }
    Write-Utf8NoBom (Join-Path $stage 'THIRD-PARTY-NOTICES.txt') (($noticeLines -join "`n") + "`n")

    $commit = (& git rev-parse HEAD).Trim()
    Assert-Success 'git commit discovery'
    $dirty = [bool](& git status --porcelain)
    $sourceDateEpoch = [long]((& git show -s --format=%ct HEAD).Trim())
    Assert-Success 'source date discovery'
    $lockHash = (Get-FileHash -LiteralPath 'Cargo.lock' -Algorithm SHA256).Hash.ToLowerInvariant()
    $provenance = [ordered]@{
        schema_version = 1
        package = 'mimic'
        version = $version
        target = 'x86_64-pc-windows-msvc'
        commit = $commit
        worktree_dirty = $dirty
        source_date_epoch = $sourceDateEpoch
        cargo_lock_sha256 = $lockHash
        rustc = ((& rustc -Vv) -join "`n")
        cargo = (& cargo -V)
        authenticode_signed = [bool]$Sign
    } | ConvertTo-Json -Depth 4
    Write-Utf8NoBom (Join-Path $stage 'provenance.json') ($provenance + "`n")

    $manifestLines = Get-ChildItem -LiteralPath $stage -File -Recurse |
        Where-Object Name -ne 'manifest.sha256' |
        Sort-Object FullName |
        ForEach-Object {
            $relative = [System.IO.Path]::GetRelativePath($stage, $_.FullName).Replace('\', '/')
            $hash = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
            "$hash *$relative"
        }
    Write-Utf8NoBom (Join-Path $stage 'manifest.sha256') (($manifestLines -join "`n") + "`n")

    New-DeterministicZip $stage $archivePath $rootName $sourceDateEpoch
    $archiveHash = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    Write-Utf8NoBom $archiveChecksumPath "$archiveHash *$([System.IO.Path]::GetFileName($archivePath))`n"

    Write-Host "Package: $archivePath"
    Write-Host "SHA-256: $archiveHash"
    Write-Host "Signed: $([bool]$Sign)"
}
finally {
    Pop-Location
}
