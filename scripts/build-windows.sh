#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
set -euo pipefail
cd "$(dirname "$0")/.."
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct)}"
export CARGO_NET_OFFLINE="${CARGO_NET_OFFLINE:-true}"
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$(pwd)=."
target="${CARGO_TARGET_DIR:-$(pwd)/target}"
dest="${DIST_DIR:-$(pwd)/dist}"
cargo build --locked --offline --release --target x86_64-pc-windows-gnu -p mh_ticker
mkdir -p "$dest"
cp "$target/x86_64-pc-windows-gnu/release/MH_Ticker.exe" "$dest/MH_Ticker.exe"
(cd "$dest" && sha256sum MH_Ticker.exe > SHA256SUMS)
