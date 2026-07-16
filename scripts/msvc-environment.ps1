Set-StrictMode -Version Latest

function Initialize-MimicMsvcEnvironment {
    $compiler = Get-Command cl.exe -ErrorAction SilentlyContinue
    if ($compiler -and $env:INCLUDE -and $env:VCToolsInstallDir -and $env:WindowsSdkDir) {
        return
    }

    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    $installation = $null
    if (Test-Path -LiteralPath $vswhere -PathType Leaf) {
        $installation = (& $vswhere -latest -products * `
            -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
            -property installationPath | Select-Object -First 1)
    }

    $devCmd = if ($installation) {
        Join-Path $installation 'Common7\Tools\VsDevCmd.bat'
    }
    else {
        @(
            (Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat'),
            (Join-Path $env:ProgramFiles 'Microsoft Visual Studio\2022\Community\Common7\Tools\VsDevCmd.bat')
        ) | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
    }

    if (-not $devCmd -or -not (Test-Path -LiteralPath $devCmd -PathType Leaf)) {
        throw 'MSVC x64 build tools were not found. Install Visual Studio Build Tools with Desktop development with C++.'
    }

    $environmentLines = & $env:ComSpec /d /s /c `
        "call `"$devCmd`" -no_logo -arch=x64 -host_arch=x64 >nul && set"
    if ($LASTEXITCODE -ne 0) {
        throw "Visual Studio developer-environment initialization failed with exit code $LASTEXITCODE."
    }

    foreach ($line in $environmentLines) {
        $separator = $line.IndexOf('=')
        if ($separator -le 0) {
            continue
        }
        $name = $line.Substring(0, $separator)
        $value = $line.Substring($separator + 1)
        Set-Item -LiteralPath "Env:$name" -Value $value
    }

    $compiler = Get-Command cl.exe -ErrorAction SilentlyContinue
    if (-not $compiler -or -not $env:INCLUDE -or -not $env:VCToolsInstallDir -or -not $env:WindowsSdkDir) {
        throw 'Visual Studio initialized without a complete compiler, INCLUDE, and Windows SDK environment.'
    }

    Write-Host "MSVC environment: $($compiler.Source)"
}
