# lhtPDF Windows x64 build environment.
# Generated STEP 2363. Keep this file out of public releases.

$vcvars = 'C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Auxiliary\Build\vcvars64.bat'

if (-not (Test-Path -LiteralPath $vcvars)) {
    throw "lhtPDF: MSVC environment script missing: $vcvars"
}

if ([string]::IsNullOrWhiteSpace($env:VCToolsInstallDir) -or
    [string]::IsNullOrWhiteSpace($env:LIB)) {

    $command = '"' + $vcvars + '" >nul && set'
    $lines = & $env:ComSpec /d /s /c $command

    if ($LASTEXITCODE -ne 0) {
        throw 'lhtPDF: MSVC environment initialization failed'
    }

    foreach ($line in $lines) {
        if ($line -match '^([^=][^=]*)=(.*)$') {
            [Environment]::SetEnvironmentVariable(
                $Matches[1], $Matches[2], 'Process'
            )
        }
    }
}

$env:CARGO_TARGET_DIR = 'C:\dev\pdfcraft\target\release-check-26.10.22'
$env:CARGO_NET_OFFLINE = 'true'

$crt = Join-Path $env:VCToolsInstallDir 'lib\x64\libcmt.lib'

if (-not (Test-Path -LiteralPath $crt)) {
    throw "lhtPDF: libcmt.lib missing: $crt"
}
