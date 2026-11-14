#!/usr/bin/env bash
set -euo pipefail
repo=$(cd "$(dirname "$0")/.." && pwd)
d=$(mktemp -d /tmp/mh-ticker-logic.XXXXXX)
trap 'rm -rf "$d"' EXIT
# Build the actual pure modules with the project's locked dependencies. The
# Windows UI is deliberately excluded; model gets only its config Row shape.
cp "$repo/Cargo.toml" "$repo/Cargo.lock" "$d/"
python3 - "$repo" "$d" <<'PY'
import pathlib,sys
repo,d=map(pathlib.Path,sys.argv[1:])
p=d/'Cargo.toml';p.write_text(p.read_text().replace('path = "src/lib.rs"','path = "harness.rs"'))
modules='\n'.join(f'#[path = "{repo}/src/{name}.rs"] mod {name};' for name in ['menu_commands','workspace','model','webhook_host','chartview','hotkey','update','autostart','ranges','keys','meter','probe','report','governor','watch','retry','pins','undo','find'])
(d/'harness.rs').write_text('''#![allow(dead_code)]
mod config {
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct Row { pub symbol: String, pub label: String, pub name: String, pub digits: u32, pub strip: bool }
    pub fn default_rows() -> Vec<Row> { vec![Row { symbol: "@GC.1".into(), label: "Gold".into(), digits: 2, ..Row::default() }] }
}
'''+modules)
PY
cargo test --locked --manifest-path "$d/Cargo.toml" --lib
