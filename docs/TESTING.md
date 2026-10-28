<!-- SPDX-License-Identifier: MIT -->
# Testing

Run `cargo test --locked` for portable engine and generator tests, including shared golden vectors,
DST, overlap, and weekend reopen. Run `cargo run --locked -p mh-specgen -- --check` to check generated
artifacts. On Windows run `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets
--locked`, and `cargo fmt --all --check`. Linux cross-build: `scripts/build-windows.sh` with the
Windows GNU target and MinGW installed. The Windows test binaries can be cross-compiled with
`cargo test --locked --workspace --target x86_64-pc-windows-gnu --no-run`.

## Real Windows checks

Check tray, strip and panel at multiple DPI settings and monitor/taskbar layouts; Explorer restart,
sleep/resume, manual clock and zone changes, and offline quotes. Confirm device-zone mapping and
unavailable fallback on different Windows zones and territories. Check panel clipping and toggle,
About credit, pre-1.3 config migration and broker/device overrides. Check a clean Windows machine
without external zoneinfo. Installer and uninstaller are not supplied by this repository.

## MT5 and TradingView checks

Compile and exercise both indicators on their platforms. Verify MT5 timer cadence, disconnects,
strategy tester clocks, server offset, DST and table coverage edges. Check Pine v6 table layout,
manual zone input, bar-event updates and historical/realtime behavior. Compare shared vectors across
both platforms before claiming parity. Pine cannot detect the chart or device zone. MT5 is a
MetaQuotes trademark; the project is not affiliated with MetaQuotes or TradingView.
