# SPDX-License-Identifier: MIT
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
if (-not $env:SOURCE_DATE_EPOCH) { $env:SOURCE_DATE_EPOCH = git log -1 --format=%ct }
$env:RUSTFLAGS = "$env:RUSTFLAGS --remap-path-prefix=$((Get-Location).Path)=."
cargo build --locked --release -p mh_ticker
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
New-Item -ItemType Directory -Force dist | Out-Null
Copy-Item (Join-Path $target 'release/MH_Ticker.exe') dist/MH_Ticker.exe
$hash = (Get-FileHash dist/MH_Ticker.exe -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText((Join-Path (Get-Location) 'dist/SHA256SUMS'), "$hash  MH_Ticker.exe`n")
