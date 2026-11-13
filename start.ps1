# Starts Lampdesigner. Builds the release binary first if it is missing or
# older than the sources. Extra arguments are passed through to the app,
# e.g.  .\start.ps1 simulate   or   .\start.ps1 export default out.3mf

$ErrorActionPreference = 'Stop'
Set-Location -Path $PSScriptRoot

$exe = Join-Path $PSScriptRoot 'target\release\lampdesigner.exe'

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    if (-not (Test-Path $exe)) {
        Write-Error 'Rust (cargo) is not installed and no built app was found. Install Rust from https://rustup.rs'
    }
} else {
    $needsBuild = -not (Test-Path $exe)
    if (-not $needsBuild) {
        $exeTime = (Get-Item $exe).LastWriteTime
        $newer = Get-ChildItem -Path 'src', 'Cargo.toml' -Recurse -File |
            Where-Object { $_.LastWriteTime -gt $exeTime } | Select-Object -First 1
        $needsBuild = $null -ne $newer
    }
    if ($needsBuild) {
        Write-Host 'Building Lampdesigner (release)...' -ForegroundColor Cyan
        cargo build --release
        if ($LASTEXITCODE -ne 0) { Write-Error 'Build failed.' }
    }
}

if ($args.Count -gt 0) {
    # CLI mode: run in this console and wait
    & $exe @args
    exit $LASTEXITCODE
} else {
    # GUI mode: start detached so the console can be closed
    Start-Process -FilePath $exe -WorkingDirectory $PSScriptRoot
}
