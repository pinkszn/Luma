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
$profileName = if ($CargoArgs -contains '--release' -or $CargoArgs -contains '-r') { 'release' } else { 'debug' }
$exe = Join-Path $root "target\$profileName\luma.exe"

# Sign the binary with the local code-signing certificate when one exists.
# Without a signature Windows SmartScreen treats every build as an unknown
# publisher and blocks a double-click launch behind "Windows protected your PC".
if ($code -eq 0) {
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

# Package release builds under release\<version>\: the signed exe, a README with
# the version line taken from Cargo.toml, and the distributable ZIP. The
# autostart entry is repointed when it still targets an older packaged build, so
# a stale installed copy cannot keep running after the source has moved on.
if ($code -eq 0 -and $profileName -eq 'release' -and (Test-Path $exe)) {
    $versionLine = Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
    if (-not $versionLine) { throw "could not read the version from Cargo.toml" }
    $version = $versionLine.Matches[0].Groups[1].Value
    $releaseRoot = Join-Path $root 'release'
    $outDir = Join-Path $releaseRoot $version
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    $outExe = Join-Path $outDir 'Luma.exe'
    Copy-Item $exe $outExe -Force

    $template = Join-Path $releaseRoot 'README.md'
    $contents = @($outExe)
    if (Test-Path $template) {
        $readme = [regex]::Replace(
            (Get-Content $template -Raw),
            '(?m)^Version [0-9]+\.[0-9]+\.[0-9]+\.$',
            "Version $version.")
        $outReadme = Join-Path $outDir 'README.md'
        Set-Content -Path $outReadme -Value $readme -NoNewline -Encoding utf8
        $contents += $outReadme
    }

    $zip = Join-Path $outDir ("Luma-{0}-win64.zip" -f $version)
    if (Test-Path $zip) { Remove-Item $zip -Force }
    Compress-Archive -Path $contents -DestinationPath $zip -CompressionLevel Optimal

    # dist\ mirrors the current build for quick manual testing.
    $dist = Join-Path $root 'dist'
    if (Test-Path $dist) {
        Copy-Item $outExe (Join-Path $dist 'Luma.exe') -Force
        if (Test-Path $outReadme) { Copy-Item $outReadme (Join-Path $dist 'README.md') -Force }
    }

    $runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
    $current = (Get-ItemProperty -Path $runKey -Name 'Luma' -ErrorAction SilentlyContinue).Luma
    if ($current -and $current.Trim('"') -like "$releaseRoot\*") {
        Set-ItemProperty -Path $runKey -Name 'Luma' -Value ('"{0}"' -f $outExe)
        Write-Host ("autostart -> {0}" -f $outExe)
    }

    Write-Host ("packaged {0}" -f $outDir)
}

exit $code
