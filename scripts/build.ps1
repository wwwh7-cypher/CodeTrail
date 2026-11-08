param([switch]$Gnu)
$ErrorActionPreference = 'Stop'
$skillRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $skillRoot
try {
    $useGnu = $Gnu -or ($IsWindows -and ((& rustup toolchain list) -match 'stable-x86_64-pc-windows-gnu'))
    if ($useGnu) {
        & cargo +stable-x86_64-pc-windows-gnu build --release --locked
    } else {
        & cargo build --release --locked
    }
    if ($LASTEXITCODE -ne 0) { throw 'Rust build failed.' }
    $exeName = if ($IsWindows) { 'code-trail.exe' } else { 'code-trail' }
    New-Item -ItemType Directory -Path (Join-Path $skillRoot 'bin') -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $skillRoot "target/release/$exeName") -Destination (Join-Path $skillRoot "bin/$exeName") -Force
    Write-Output (Join-Path $skillRoot "bin/$exeName")
} finally { Pop-Location }
