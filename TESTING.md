<!-- SPDX-License-Identifier: Apache-2.0 -->
# Testing and verification status

Made by Maria. This file records what was run on the build box and what still needs real Windows, MT5 or
TradingView. It is not a claim that the unchecked items pass.

## Run here (Linux, rustc 1.85.1)

```sh
cargo test
# 26 engine unit tests + 4 shared golden-vector tests + 4 generator tests = 34 passed

cargo run -p mh-specgen -- --check
# all generated artifacts are up to date

cargo check -p mh_ticker --tests --ignore-rust-version
# source type-checks on Linux; see the note below
```

`cargo check -p mh_ticker` is not a normal Linux command for this crate: the root app is Win32-only and the
checked-in `Cargo.lock` contains ICU crates that require rustc 1.88 while this box has 1.85.1. For a
source-level type-check only, the lock was temporarily pinned to ICU-compatible transitive versions and a
local `pkg-config` shim pointed at the system OpenSSL. Neither is part of this repository. The check compiled
the app code and its tests on Linux (with `windows` UI Automation gated behind `cfg(windows)`), but **did not
link or run any Win32 code**. It is not a substitute for a Windows build.

`cargo clippy` and `cargo fmt` are **not installed** on this box (`cargo-clippy`/`cargo-fmt` components are
absent). They were not installed, per the brief's "do not install toolchains". Formatting was kept consistent
by hand.

The engine tests cover:

- all four zones and their DST transitions (US, UK, Sydney, Tokyo no-DST);
- both London/New York mismatch weeks (March US-first, late October UK-first);
- half-open exact open and exact close instants;
- weekend reopen (Sydney Sunday UTC open, New York Monday open);
- ambiguous (`01:30` fall-back) and nonexistent (`02:30` spring-forward) local times;
- leap day and year rollover without holidays;
- fixed-offset and US-DST broker reopen arithmetic, Friday-to-Monday, and the unverified default;
- generated MQL5 transition tables against `chrono-tz` at every transition ±1 s;
- the generated Pine session strings and manual-zone input;
- the pinned Windows-to-IANA mapping, territory overrides, unknown keys and every IANA name parsing.

## Needs real Windows

- `cargo build --release -p mh_ticker` (no Windows target or rustup on this box) and the tray/strip/panel
  visuals at 100/125/150/200% DPI.
- **Device-zone adapter:** call `GetDynamicTimeZoneInformation` on real US/CA/DE/AU machines and confirm the
  territory (`GetUserDefaultGeoName`, locale fallback) selects the expected IANA zone. Test an unknown
  Windows key / failed mapping: exactly one device row must say unavailable, with no guessed clock.
- **Change notifications:** manual clock change, time-zone change, sleep/resume, Explorer restart and a
  secondary taskbar. Confirm `WM_TIMECHANGE`, `WM_SETTINGCHANGE` and `WM_POWERBROADCAST` re-read the zone and
  the panel recomputes from the wall clock. Confirm the block does not depend on quote updates or go stale
  when quotes are offline.
- **Panel layout:** six session columns plus the overlap/caveat line with 1, 2, 6 and 12 cards; check the
  footer and the session block do not clip at the narrowest width. Toggle *Session timers* while the panel is
  open; confirm it resizes in place. Check *About MH Ticker* shows "Made by Maria".
- **Config migration:** open a real pre-1.3 `%LOCALAPPDATA%\MH_Ticker\config.json` (with symbols, placement,
  dock and x/y) and confirm quotes/settings are unchanged while `sessions` defaults are added. Change
  `broker_rule_verified`, `broker_offset_min`, `broker_reopen_min` and `device_zone` by hand, restart, and
  confirm the panel states match.
- **Fresh Windows without Go or external zoneinfo:** confirm the compiled `chrono-tz` data works offline and
  the London/New York overlap matches known UTC instants on a clean machine.
- Packaging/installer checks (not part of this branch's v1 scope, but listed by the architecture): installer,
  uninstaller, SHA256SUMS, unsigned-build warning.

## Needs real MT5

`mql5/MH_Sessions.mq5` was not compiled here (no MetaEditor/terminal). Verify:

- compile warnings and the display-only guarantee (no trade/order/account functions);
- `EventSetTimer`/`OnTimer` cadence and chart-object updates during ticks and quiet periods;
- measured `TimeTradeServer()` − `TimeGMT()` across a server DST change, the 9999 override path and the
  "disconnected / unreliable" state;
- generated table boundaries at the start/end of each DST transition, including southern-hemisphere Sydney
  April/October and the 2024/2036 coverage edges;
- strategy-tester semantics for `TimeLocal`, `TimeGMT` and `TimeTradeServer`, and the visible tester caveat;
- exactly one device row, fixed session order, London–New York overlap and the unverified broker reopen text;
- indicator timer behaviour on a weekend/disconnected terminal.

## Needs real TradingView

`pine/MH_Sessions.pine` was not compiled here. Verify:

- v6 compilation, the 4-column/10-row table, and `time(timeframe.period, session, IANA)` membership on
  intraday, hourly and daily charts;
- the extra clock input defaults to `syminfo.timezone`, is labelled manual/not auto-detected, and a user
  change updates the clock;
- no claim of chart/device auto-detection anywhere in the UI or docs;
- historical vs realtime behaviour: the table updates on bar events, shows the last bar time, and does not
  pretend to be a live countdown;
- the overlap row and the generated broker reopen estimate, including the unverified warning;
- missing ticks / market-closed charts and a symbol whose exchange zone is not in the dropdown list.

## Shared-vector parity still to run in the platforms

`spec/golden/vectors.json` is consumed by the Rust tests now. The Pine/MQL5 boilerplates are generated from
the same rules, but there is no automated TradingView/MT5 harness here. Before claiming parity, evaluate the
same UTC instants in both platforms and compare open/closed, overlap and broker output, especially the March
and late-October mismatch weeks and the Sydney April/October transitions.
