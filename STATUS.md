<!-- SPDX-License-Identifier: Apache-2.0 -->
# Status — session timers (v1)

Made by Maria. Branch `feature/session-timers`, based on `main` at `f8cc026`. No remotes, pushes, uploads or
publishing were used.

## Built

- **Shared spec:** `spec/sessions.json` (spec 1.0.0, tzdb 2025b, coverage 2024–2036) with four ordered
  sessions, the London–New York overlap pair, no-holiday policy and the unverified broker preset.
  `spec/golden/vectors.json` holds shared UTC vectors.
- **Portable engine:** `crates/session-engine` (`mh-session`). Half-open intervals, earliest ambiguous-local
  handling, forward-shift for nonexistent local times, DST/weekend/overlap logic, broker offset/reopen
  arithmetic, generated bounded transition tables, pinned Windows-to-IANA mapping and the presentation-neutral
  panel rows.
- **Generator:** `crates/specgen` (`gen-spec`) writes the Rust rules/tables, `pine/MH_Sessions.pine` and
  `mql5/MH_Sessions.mq5`; `--check` fails on drift. The generated files are checked in.
- **Ticker integration:** fixed-order panel block (Sydney, Tokyo, London, New York, broker, one device row),
  countdowns, highlighted overlap, broker server clock/reopen state, unavailable device fallback, Session
  timers toggle, About dialog with "Made by Maria", config migration, and refresh on UI timer, clock, zone and
  resume events. The block is independent of quote fetching.
- **Indicators:** Pine v6 table-only boilerplate with a manual IANA extra clock (never auto-detected, default
  `syminfo.timezone`); MQL5 display-only, strictly read-only boilerplate with generated bounded IANA tables,
  measured/overridden broker offset, device clock and visible unknown/coverage states.
- **Docs/licensing:** `docs/SESSIONS.md`, `TESTING.md`, `THIRD_PARTY.md`, README/CHANGELOG updates; LICENSE
  and NOTICE unchanged; Apache-2.0 SPDX headers on new files.

Commits:

```
0460b91 feat(sessions): shared spec, portable engine and generated indicators
e3019c9 feat(ticker): show session timers and map the Windows device zone
fa21eba docs: session rules, generated artifacts, testing and licences
3a2f2e3 docs: add STATUS for the session timers v1 work
abc2944 fix(pine): convert the broker reopen day to Pine's Sunday-first numbering
c107986 fix(pine): call time() with the generated const session strings directly
d9577a2 fix: keep the unavailable-device test portable and shorten the broker detail
```

## Commands and results

| Command | Result |
| --- | --- |
| `cargo test` | **34 passed** (26 engine, 4 golden vectors, 4 generator) |
| `cargo run -p mh-specgen -- --check` | generated artifacts up to date |
| `cargo check -p mh_ticker --tests --ignore-rust-version` | app source type-checks on Linux with a temporary ICU-compatible lock and local pkg-config shim; no Win32 link/run |
| `cargo clippy` / `cargo fmt` | **not run**: components not installed on this box; per the brief, no toolchains were installed |
| Windows `cargo build --release -p mh_ticker` | **not run**: no Windows target/rustup on this box |

The normal `cargo test` at the workspace root tests the portable default members. `cargo test --workspace`
would include the Win32-only app and cannot run here.

## LOC added

From `git diff --numstat main...HEAD` before this status file:

- **4,821 added / 46 deleted** across 37 files.
- Rust: **2,300 added / 16 deleted** (engine, generator, app and generated Rust tables).
- Other: **2,521 added / 30 deleted** (spec/vectors, Pine 178, MQL5 437, docs, README).
- Excludes this `STATUS.md`.

## Tests needing real Windows

See `TESTING.md` for the full list. Summary:

- release build, tray/strip/panel at multiple DPI and monitor/taskbar layouts;
- actual `GetDynamicTimeZoneInformation` + territory mapping on US/CA/DE/AU machines, failed-map fallback and
  exactly one device row;
- `WM_TIMECHANGE`/`WM_SETTINGCHANGE`/`WM_POWERBROADCAST`, sleep/resume, Explorer restart, secondary taskbar;
- panel width/clipping for 1–12 cards, in-place toggle, About dialog;
- config migration of a real pre-1.3 config and broker/device overrides;
- clean Windows without Go/external zoneinfo; installer/uninstaller and unsigned-build warning.

## Tests needing real MT5 and TradingView

- MQL5 compile/run, timer cadence, disconnects, strategy tester, server/GMT offset and DST boundaries,
  generated-table coverage edges, southern-hemisphere sessions, exactly one device row.
- Pine v6 compile, table layout, manual zone input/default, `time(..., IANA)` membership on several
  timeframes, historical/realtime limitations, overlap and broker estimate.
- Cross-platform evaluation of the shared golden vectors is still to be done; the Rust tests consume them
  now.

## Open issues

1. **Session hours/weekdays are not owner-approved.** They are common FX conventions; the spec records that
   approval is required before a public release. No holiday/exception calendar exists in v1.
2. **Broker rule is unverified.** The default Exness-style preset shows the server clock but deliberately
   reports the next verified reopen as unknown. Daily reopens are not modelled; seasonal mapping, exceptions
   and broker changes need independent confirmation. `broker_rule_verified` is a user assertion, not proof.
3. **CLDR licence review.** The Windows-to-IANA table is a pinned subset of CLDR `windowsZones` 48. CLDR's
   Unicode License v3 is not on the approved dependency licence list; the mapping is hand-maintained and
   unknown keys fail visibly, but the Manager should approve or replace the data source.
4. **Windows mapping is a subset**, not the full CLDR table. Unlisted keys show unavailable rather than a
   guessed zone.
5. **No Windows build or Windows tests** on this box; no clippy/rustfmt. The Linux type-check is not a
   substitute.
6. **Pine/MQL5 not compiled** here. Their runtime behaviour, table rendering, timer semantics and golden-vector
   parity remain unverified. Pine deliberately has no session countdowns and cannot read chart/device time
   zones; its extra clock is manual.
7. **MQL5 coverage is bounded** to 2024–2036. Outside it the indicator says so; regeneration is required for
   later years or a tzdb update.
8. **Packaging is out of scope for this branch** (installer/MSIX/winget/signing review were not attempted).
9. **Rust 1.85 vs the app lockfile.** The checked-in `Cargo.lock` includes ICU crates requiring rustc 1.88;
   this box has 1.85.1. The portable engine/generator build fine, but a Windows build should use a toolchain
   matching the lock or a reviewed lock update.
