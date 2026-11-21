param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$CargoArgs
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$vcvars = 'C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat'

# Find the workspace-local Rust toolchain (.tools). The project may be checked
# out at any depth below the folder that holds it, so walk up to four levels.
$tools = $null
$dir = $root
for ($i = 0; $i -lt 4; $i++) {
    $candidate = Join-Path $dir '.tools'
    if (Test-Path (Join-Path $candidate 'cargo\bin\cargo.exe')) {
        $tools = $candidate
        break
    }
    $parent = Split-Path -Parent $dir
    if (-not $parent -or $parent -eq $dir) { break }
    $dir = $parent
}
if (-not $tools) { throw "Rust toolchain not found: no .tools\cargo\bin\cargo.exe in $root or its parents" }

$cargoHome = Join-Path $tools 'cargo'
$rustupHome = Join-Path $tools 'rustup'
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
$code = $LASTEXITCODE

# Sign the binary with the local code-signing certificate when one exists.
# Without a signature Windows SmartScreen treats every build as an unknown
# publisher and blocks a double-click launch behind "Windows protected your PC".
if ($code -eq 0) {
    $profileName = if ($CargoArgs -contains '--release' -or $CargoArgs -contains '-r') { 'release' } else { 'debug' }
    $exe = Join-Path $root "target\$profileName\luma.exe"
    $cert = Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert -ErrorAction SilentlyContinue |
        Where-Object { $_.Subject -like 'CN=Luma Dev*' -and $_.NotAfter -gt (Get-Date) } |
        Sort-Object NotAfter -Descending |
        Select-Object -First 1
    if ((Test-Path $exe) -and $cert) {
        $sig = Set-AuthenticodeSignature -FilePath $exe -Certificate $cert -HashAlgorithm SHA256 `
            -TimestampServer 'http://timestamp.digicert.com' -ErrorAction SilentlyContinue
        if (-not $sig) {
            $sig = Set-AuthenticodeSignature -FilePath $exe -Certificate $cert -HashAlgorithm SHA256
        }
        Write-Host ("signed {0} [{1}]" -f $exe, $sig.Status)
    }
}

exit $code
