# Changelog — MH Ticker

## [Unreleased]

- Validate quote timestamps and balance COM initialization and foreground hook cleanup.
- Audit locked dependency licences and clarify unverified release and platform checks.

- Add session timers, a portable session engine, and generated Pine and MQL5 indicators.
- Review Windows cross-builds, resource compilation, documentation, and release preparation.

## 2026-10-28 — 1.3.0 licence follow-up   [agent]

- Licence stays **MIT** (Manager ruling 2026-10-28): restored the original `LICENSE`, removed the Apache-2.0
  `NOTICE`, restored the original README credit and License section, and switched SPDX headers, crate
  metadata and the About text to MIT / "made with love by maria". The Pine and MQL5 boilerplates live in
  this repository, so they are MIT too.
- Added the Unicode License v3 text for the bundled CLDR `windowsZones` subset (approved 2026-10-28).
- Session hours and the default broker preset are recorded as owner-approved (2026-10-28).

## 2026-10-28 — 1.3.0: session timers, plus Pine v6 and MQL5 boilerplates   [agent]

**Files:** `spec/sessions.json`, `spec/golden/vectors.json`, `crates/session-engine/**`, `crates/specgen/**`,
`pine/MH_Sessions.pine`, `mql5/MH_Sessions.mq5`, `src/{config,device,panel,ui,dock,main}.rs`, `Cargo.toml`,
`docs/SESSIONS.md`, `TESTING.md`, `THIRD_PARTY.md`.

**What.** Added the v1 session-timer feature from `docs/ARCHITECTURE.md`.
- **Panel:** in the expanded view, a fixed-order block for Sydney, Tokyo, London and New York (local clock,
  open/closed, next-transition countdown), the highlighted London–New York overlap, the broker server clock
  and next reopen, and **exactly one** Windows device-zone clock. Rows never reorder or disappear; a failed
  zone mapping shows the device row as unavailable. New *Session timers* menu toggle and an *About MH Ticker*
  item with "made with love by maria".
- **Engine:** a platform-independent `mh-session` workspace crate with half-open intervals, `earliest`
  ambiguous-local handling, forward-shift for nonexistent local times, DST/weekend/overlap logic, and a
  broker rule with an explicit verified/unverified state. It builds and tests on Linux with `cargo test`.
- **Shared rules:** `spec/sessions.json` generates the Rust constants, bounded MQL5 IANA transition tables,
  Pine session strings and both indicator files. `cargo run -p mh-specgen -- --check` guards drift. Shared UTC
  golden vectors cover DST transitions, both mismatch weeks, overlap, weekend reopen, exact open/close, leap
  day, year rollover and broker reopen.
- **Pine v6:** table-only boilerplate. Pine cannot read the chart/device zone, so the extra clock is a manual
  IANA `input.string` (default `syminfo.timezone`) and the docs never claim auto-detection.
- **MQL5:** display-only, strictly read-only, no trading functions. Generated bounded transition tables,
  measured or overridden server offset, visible unknown state, device clock from `TimeLocal()`/`TimeGMT()`.
- **Config:** a new optional `sessions` object; old configs keep every quote/placement setting and get the
  defaults. No new network calls.

**Verify.** `cargo test` (34 tests: 26 engine, 4 golden, 4 generator), `cargo run -p mh-specgen -- --check`,
and a Linux cross-platform `cargo check` of the app source. Rustfmt/clippy are not installed on this box. Real
Windows, MT5 and TradingView behaviour is not verified here; see `TESTING.md`.

## 2026-10-16 — 1.2.3: its own app icon   [claude.impl]

**Files:** `assets/{icon.ico,icon_256.png,make_icon.py,app.rc}`, `build.rs` (new), `src/ui.rs` (class icon), `Cargo.toml`
(1.2.3).

**What.** Maria asked for the icon setup AO Ticker got, in its own variant so the two don't look the same.
- **Design:** the tray icon's slate tile and its dip-then-climb price line, drawn as a violet-to-rose gradient with a
  soft fill under it and a small four-point sparkle at the tip (AO's is a green line ending in a live dot).
  `assets/make_icon.py` draws each of the nine sizes (16-256 px) separately at 8x; the sparkle is left out below 24 px.
- **Embedding:** `build.rs` compiles `assets/app.rc` with windres (shipped with the windows-gnu Rust toolchain) and links
  it as icon 1. Without windres (e.g. an MSVC-only setup) the build warns and keeps the generic icon. The window class
  uses icon 1 too.

**Verify.** `cargo build --release` (no windres warning), 20 tests, clippy clean. `Icon.ExtractAssociatedIcon` on the
exe returns the new tile; the size sheet was checked on light and dark. `--probe` runs, and the strip docks and
self-heals as before.

## 2026-10-15 — 1.2.2: docked strip owned by the taskbar, so it stays above it with Search/Start open   [claude.impl]

**Files:** `src/{dock,ui}.rs`, `Cargo.toml` (1.2.2). Same change as AO Ticker 0.6.2.

**What.** Maria sent a screenshot after 1.2.1: with the taskbar search open, the docked strip was still gone.
- **Real cause (logged, not the fullscreen check).** A 0.1 s log of foreground, `IsWindowVisible` and
  `GetWindowBand` while Maria opened Search and clicked app icons showed the strip **visible** the whole time. Search's
  window is 1000 px wide (`SearchHost` CoreWindow 525,77-1525,1080, band 13), not monitor-sized. While Start or Search is
  open, Windows moves the taskbar from z-band 1 to band 6. A plain topmost window stays in band 1, under it. App-icon
  clicks raised the taskbar over the strip for 0.2-0.4 s until the re-raise. So 1.2.1's shell-window rule was a guess
  that did not cover this; it stays as a guard.
- **Fix.** A docked strip is now an owned window of its taskbar (`GWLP_HWNDPARENT`, `own_by_taskbar` in `ui.rs`).
  Windows keeps an owned window above its owner and carries it into the owner's band. Other placements and the panel
  are unowned. The owner is re-checked on every dock check, so a new taskbar after an Explorer restart is picked up.

**Verify.** Experiment first: a test box owned by the taskbar moved to band 6 with it when Search opened, and a plain
topmost box stayed under it. Two-process test: when the owner's process exits, the owned window survives and its owner
field reads 0. After the change: 20 tests pass, clippy clean. Search opened via the shell's Search CLSID (no synthetic
input): strip band 6, nothing above it. Taskbar raised over the strip by script: the strip is on top again within
0-6 ms (1.2.1: 125-208 ms). Outside move and hide undone within 0.9 s. **Unverified:** a real Explorer restart, a
secondary-monitor taskbar.

## 2026-10-15 — 1.2.1: no hiding for the shell's own windows; strip self-heals   [claude.impl]

**Files:** `src/{dock,ui}.rs`, `Cargo.toml` (1.2.1). Same change as AO Ticker 0.6.1.

**What.** Maria saw the docked strip vanish on clicking the taskbar search box and, sometimes, an app icon.
- **Cause (fullscreen check).** The strip hides when the foreground window covers its whole monitor (a game). Some of
  Windows' own shell windows are monitor-sized: the touch-keyboard host's `Windows.UI.Core.CoreWindow` is
  0,0-1920,1080 here (Verified: window listing), and Search/Start use the same kind of host window. Clicking them made
  the strip read them as a fullscreen app. `dock::shell_window` now excludes shell classes (CoreWindow, the XAML
  thumbnail/overflow islands, ForegroundStaging, Task View, the desktop and taskbars) and shell processes (explorer,
  SearchHost, StartMenuExperienceHost, ShellExperienceHost, ShellHost, TextInputHost). Hidden or DWM-cloaked foreground
  windows never count either.
- **Self-heal.** Every check (each foreground change, then every 500 ms) now puts a taskbar or corner strip back where
  the app placed it, and re-shows it if something outside hid it. Floating is left alone. A screenshot/automation tool
  that hides and moves other windows left the strip stranded mid-screen before this change.
- **Faster re-raise.** A foreground change also schedules a re-check 150 ms later, because a taskbar click raises the
  taskbar just after the foreground event.

**Verify.** 20 tests pass (`cargo test`, new `shell_windows_are_not_fullscreen_apps`); clippy clean. Live, by script, without input to the shell: an outside `SetWindowPos` to mid-screen
was undone within 0.9 s, an outside `ShowWindow(SW_HIDE)` re-shown within 0.9 s, and the taskbar raised over the strip
gave way in 185-208 ms (three tries each). **Unverified:** the search box and app-icon clicks themselves (no synthetic
input to Explorer); that Search's flyout window is monitor-sized is inferred from the touch-keyboard host, not seen.

## 2026-10-15 — public repo, MIT license, v1.2.0 release   [claude.impl]

- Own git repository (`main`), published as https://github.com/mariyaholic/mh-ticker (public, per Maria).
- `LICENSE`: MIT, © 2026 mariyaholic (Maria asked that reusers credit the original source; MIT requires
  keeping the copyright notice). `Cargo.toml` gains `license`, `repository`, `authors`.
- `.gitattributes` pins LF; `.cargo/config.toml` (this machine's D: target dir) is untracked, and the README's
  Build section says how to set a target dir elsewhere.
- README: Download (release exe, SmartScreen note) and License sections.

## 2026-10-15 — 1.2.0: Position menu, for taskbars without room left of Start   [claude.impl]

**What.** Maria pointed out that not everyone has free space on the left of the taskbar. A left-aligned taskbar (the
Windows 10 default, a common Windows 11 setting) has none.
- **Right-click → Position** replaces *Dock in taskbar*. Options:
  - **Auto** (default): the gap left of Start while it is roomy, else the gap between the app icons and the tray,
    else the bottom-right corner above the taskbar (auto-hide or vertical taskbar).
  - **Taskbar, left of Start** / **Taskbar, next to the tray**: that gap only.
  - **Floating** (drag anywhere).
  - The four screen corners.
  - **Display 1 / 2 / ...** when there are several screens.
- **`dock.rs` tracks both gaps:**
  - The end of the app icons (UIA: the last child of `TaskbarFrame`) and the Widgets button on either side.
  - The tray's left edge (`TrayNotifyWnd`, or the first tray button on a secondary screen's taskbar).
  - Auto-hide (`ABM_GETSTATE`).
  - Auto keeps the left gap unless the tray side is 30% wider, so the strip does not hop as apps open.
- **Every placement hides under a fullscreen app.** Taskbar placements are always on top; corners and floating follow
  *Always on top*.
- **Config:** new `placement` and `monitor`. An older config has no `placement`; its `dock` flag decides (true =
  Auto, false = Floating).
- The same change went into AO Ticker 0.6.0 (Argonaut Oak); `dock.rs` is shared byte for byte.

**Verified:**
- `cargo test`: 19 passed (new: gaps for centred and left-aligned icons, Auto's choice, no-probe fallback, placement
  fallback). `cargo clippy --all-targets`: clean.
- `cargo build --release`: `MH_Ticker.exe` is 910,848 bytes, copied next to the README.
- Each placement, set in a temp config and read via `GetWindowRect`:

  | Placement | Rect |
  |---|---|
  | auto | left gap, x 10-371 |
  | taskbar_right | x 1426-1506, then 1457-1585 after apps changed |
  | bottom_right | x 1445-1910, y 972-1005 |
  | top_right | y 10-43 |
  | floating | its default spot |

- Seen on screen: the strip next to the tray, between the last app icon and the tray (`docs/strip_next_to_tray.png`).

**Unverified:**
- The Position menu was not clicked.
- Not tested: a left-aligned taskbar (this one is centred), several displays, auto-hide, and Widgets on.

## 2026-10-15 — 1.1.0: choose the symbols from the menu   [claude.impl]

**What.** Maria asked whether the ticker could change which symbols appear. Before this, only by editing
`config.json` and restarting. Same feature as AO Ticker 0.5.0.
- **Symbols submenu** (right-click the strip, the panel or the tray icon):
  - Every entry of the new `catalog`, ticked when shown. A tick or untick applies within a quarter second: the strip
    re-fits, an open panel resizes in place, no restart.
  - Shown rows keep catalog order. The last symbol cannot be hidden, and at most 12 show.
  - *Reset to default* restores the original six.
- **Catalog (23 CNBC symbols):**
  - Metals, crypto and US futures: XAU, XAG, XPT, BTC, ETH, DOW, S&P, NAS.
  - Energy and copper futures: WTI, BRENT, NATGAS, COPPER.
  - FX and the dollar: EURUSD, GBPUSD, USDJPY, DXY.
  - Rates and volatility: US10Y, US2Y, VIX.
  - World indices: N225, DAX, FTSE, HSI.
  - Any other CNBC symbol can be added to `catalog` by hand. An old config gets the catalog written in on load; its
    rows stay.
- **Live hand-off:**
  - The UI hands the new list to the feed thread through the board (`rows_gen`). The feed's 3 s wait checks for a
    change every 250 ms.
  - Quotes are now fetched before the hour of bars, so a new symbol's price is not stuck behind a dozen chart requests.
  - Sparkline history is kept by label, so a symbol that stays keeps its line.

**Verified:**
- `cargo test`: 15 passed (new: toggle order and limits, hand-added rows, catalog migration, history re-keying).
- `cargo clippy --all-targets`: clean.
- `cargo build --release`: `MH_Ticker.exe` is 893,952 bytes, copied next to the README.
- `MH_Ticker.exe --probe` with all 23 catalog symbols as rows (temp config):
  - Every symbol returned a price.
  - Real-time: metals, crypto, FX, DXY, yields and VIX. CNBC-delayed: futures and the world indices.
  - N225 and HSI were stale because those markets were closed.
  - 60-61 one-minute bars for every row except N225 and HSI (no bars in the last hour, closed).

**Unverified:** the menu was not clicked, and MH Ticker is not running (AO Ticker holds the taskbar slot on this laptop).

## 2026-10-15 — 1.0.0: standalone fork of AO Ticker with public CNBC prices   [claude.impl]

**What.** Maria asked for a fork of AO Ticker for the mariyaholic room with the Argonaut Oak sources and
the MT5 bridge removed. The goal was just the taskbar widget and the expanded centred view, as a
one-click app, plus a tray item to quit or expand the view.

- **Forked** from Argonaut Oak `apps/ao_ticker` at `aaa93149`. Kept: `dock.rs`, the panel and morph in
  `panel.rs`/`ui.rs`, the single instance and the fullscreen hide.
- **Removed:** the Market Terminal WebSocket, the investor bridge (and its token), broker/proxy
  resolution and `tungstenite`.
- **New `source.rs`:**
  - CNBC `restQuote`, one batched request every 3 s over a kept-alive HTTPS connection (`ureq` +
    `native-tls`, i.e. Windows SChannel). On failure it backs off up to 60 s.
  - CNBC 1-day chart: the last hour of 1-minute bars seeds the sparklines at start and every 10 min.
  - Own ISO-time parser (`parse_time`) with no date crate.
- **Default rows:** XAU spot, XAG spot, BTC (Coinbase), and Dow/S&P/Nasdaq front-month futures. CNBC
  marks the futures as delayed; they are labelled *CNBC delayed* and get `delayed_grace_s` (900 s)
  before going stale.
- **Tray:**
  - Left-click toggles the panel. A second click within 600 ms is ignored, so a double-click
    does not open and close it.
  - Right-click menu: *Expand view* (bold default) / *Back to taskbar*, *Show strip*,
    *Dock in taskbar*, *Always on top*, status, *Open config folder*, *Quit*.
  - The icon is a chart line on a dark tile, coloured by state (green live, amber partly stale,
    red offline).
- **Its own identity**, so it can run beside AO Ticker: mutex `MH_Ticker_single_instance`, window
  class `MH_Ticker_Strip`, config `%LOCALAPPDATA%\MH_Ticker\config.json`, exe `MH_Ticker.exe`.

**Verified:**
- `cargo test`: 11 passed (model staleness/resolve/format; CNBC time, quote, chart parsing and
  symbol encoding; panel geometry/history; dock span).
- `cargo clippy`: no warnings.
- `cargo build --release`: `MH_Ticker.exe` is 876,544 bytes.
- `MH_Ticker.exe --probe`, live:
  - XAU 4,153.73, XAG 56.590, BTC 81,219 real-time (ages 9–57 s).
  - DOW/S&P/NAS futures DELAYED (ages ~600 s).
  - 60 one-minute bars per row covering the last hour.
- Ran for 30 s:
  - The strip window was created at the taskbar slot (10,1024) and hidden while a fullscreen game
    was in front, as designed.
  - 31 ms CPU over 30 s, 4.9 MB private.
  - A second launch exited (one instance).
  - The config was written with `dock: true`, `poll_s: 3`.

**Unverified:**
- Not seen on screen: the strip, panel and tray icon. Maria was in a fullscreen game, so no
  screenshots or input were taken. The drawing code is AO Ticker's (verified there); the new
  tray icon glyph and menu were not seen.
- The tray left-click/double-click behaviour and the menu items were not clicked.
