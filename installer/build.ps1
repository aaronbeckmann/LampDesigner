# Builds the Windows installer:
#   dist\Lampdesigner-<version>.msi        (per-user MSI)
#   dist\Lampdesigner-<version>-setup.exe  (setup program wrapping the MSI)
# Needs the WiX toolset 5 (dotnet tool install --global wix --version 5.0.2).

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
Write-Host "Lampdesigner $version" -ForegroundColor Cyan

cargo build --release
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

# the bundle needs the Burn bootstrapper extension. The NuGet package ships the
# DLL under its newer name, so WiX reports it as "damaged": pass the DLL path.
$wixVersion = ((wix --version) -split '\+')[0]
$balDir = Join-Path $env:USERPROFILE ".wix\extensions\WixToolset.Bal.wixext\$wixVersion"
if (-not (Test-Path $balDir)) {
    wix extension add -g "WixToolset.Bal.wixext/$wixVersion"
    if ($LASTEXITCODE -ne 0) { throw 'could not add WixToolset.Bal.wixext' }
}
$balExt = Get-ChildItem $balDir -Recurse -Filter '*.wixext.dll' | Select-Object -First 1 -ExpandProperty FullName
if (-not $balExt) { throw "no bootstrapper extension DLL found in $balDir" }

$bin = Join-Path $root 'target\release'
$dist = Join-Path $root 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null
$msi = Join-Path $dist "Lampdesigner-$version.msi"
$exe = Join-Path $dist "Lampdesigner-$version-setup.exe"

wix build installer\Lampdesigner.wxs -arch x64 -d "Version=$version" -d "BinDir=$bin" -d "RootDir=$root" -o $msi
if ($LASTEXITCODE -ne 0) { throw 'MSI build failed' }

wix build installer\Bundle.wxs -arch x64 -ext $balExt -d "Version=$version" -d "BinDir=$bin" -d "MsiPath=$msi" -o $exe
if ($LASTEXITCODE -ne 0) { throw 'setup.exe build failed' }

Get-ChildItem $dist -Filter "Lampdesigner-$version*" | Where-Object { $_.Extension -in '.msi', '.exe' } |
    Select-Object Name, @{ n = 'MB'; e = { [math]::Round($_.Length / 1MB, 1) } }
