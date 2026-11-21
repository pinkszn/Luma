param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$CargoArgs
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$vcvars = 'C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat'
$cargoHome = Join-Path $root '..\.tools\cargo'
$rustupHome = Join-Path $root '..\.tools\rustup'
$envFile = Join-Path $root '.build-env.txt'

if (-not (Test-Path $vcvars)) { throw "vcvars64.bat not found at $vcvars" }
if (-not (Test-Path (Join-Path $cargoHome 'bin\cargo.exe'))) { throw "cargo not found in $cargoHome" }

cmd /d /s /c "call `"$vcvars`" >nul 2>&1 && set > `"$envFile`""
if ($LASTEXITCODE -ne 0) { throw "failed to initialise MSVC environment" }

Get-Content $envFile | ForEach-Object {
    if ($_ -match '^([^=]+)=(.*)$') {
        $name = $matches[1]
        $value = $matches[2]
        if ($name -notmatch '^(PROMPT|PSModulePath)$') {
            Set-Item -Path "env:$name" -Value $value -ErrorAction SilentlyContinue
        }
    }
}
Remove-Item $envFile -ErrorAction SilentlyContinue

$env:RUSTUP_HOME = $rustupHome
$env:CARGO_HOME = $cargoHome
$cargo = Join-Path $cargoHome 'bin\cargo.exe'
& $cargo @CargoArgs
exit $LASTEXITCODE
