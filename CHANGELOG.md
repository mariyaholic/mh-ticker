# Changelog — MH Ticker

## 2026-10-22 — 1.4.0: a theme catalogue, and the nine card features in both builds   [freebuff.impl]

**Files:** `src/theme.rs` (rewritten: 26 palettes, 4 tiers, 13 faces), `src/panel.rs` (card layout,
range bar, 52-week badge, markers, pixel styling), `src/model.rs` + `src/source.rs` (eight more quote
fields), `src/ui.rs` (tiered menus, Font menu, animation timer, cached back buffer, click-to-open,
copy), `src/config.rs` (`theme_font`), `src/shot.rs` + `examples/shots.rs` (new), `Cargo.toml`
(1.4.0, optional `png`), `shots/` (generated), `THEMES.md` (new), `README.md`, `ARCHITECTURE.md`,
`FEATURES.md`.

**What.** Two asks from Maria: the classic build should inherit the nine byte-free card features
rather than stay minimalist, and the theme work should become a real catalogue — basic colours, then
gradients, then animated ones, then retro pixelated ones, and the same for fonts. Plus screenshots.

- **The nine, in both builds.** Day range (a track with the price as a marker), the day-open marker
on the line, session high/low dashes, the 52-week badge with the date of its high when the line has
room, absolute change beside the %, volume where CNBC sends one, currency, click a card to open it
on CNBC, and `Copy <label> <price>` in the card's context menu. Eight new fields per quote
(`open`, `high`, `low`, `volume`, `yrhiprice`, `yrloprice`, `yrhidate`, `currencyCode`) parsed from
the response the app was already downloading: **0 extra bytes of traffic**.
- **`0.00` is not a price.** `positive()` drops it: CNBC sends `high`/`low` = 0.00 for spot silver and
platinum and `yrloprice` = 0.00 for six of the 23 catalogue symbols, and a zero drawn on the card
reads as a real level. A test pins the JSON that does this.
- **26 palettes in four tiers, and the tier is a rendering rule.** `Kind::{Flat, Gradient, Animated,
Pixel}` drives band count, corner radius, antialiasing, scanlines and the animation — not just a
menu heading, so a palette cannot claim to be animated and be painted flat.
- **Animated backgrounds that cost nothing until you open one.** A 96-band ring cycling
`bg → bg_to → bg_mid → bg` is pre-built with the theme; a frame picks a different starting index.
The 9-second loop is repainted at 10 fps **only while the panel is open**, and `sync_anim` kills the
`WM_TIMER` on collapse and on any static theme.
- **The back buffer is cached.** `paint` used to `CreateCompatibleBitmap` on every repaint; at 10 fps
that is a ~1.2 MB allocation ten times a second. One buffer now lives on `Ui` until the window
resizes. `MH_Ticker_Ultra` at 60 s: **6.38 → 5.83 MB** private, CPU 0.52 → **0.23 %**.
- **Menus that scale.** 26 themes in one popup is a wall, so Theme nests four tier submenus (each
with a greyed line saying what the tier does) and Font nests System/Mono/Serif/Bitmap. Both are built
from `theme::ALL` and `theme::FACES`, so adding a palette is one array entry.
- **Screenshots are generated, not drawn.** `cargo run --example shots --features shots` renders
every theme through the real `panel::draw` into a 760 × 394 bitmap and writes a PNG; animated themes
get three frames. The PNG encoder sits behind the `shots` feature, so **neither shipped exe links
it**.
- **Cards grew** 224 × 132 → 236 × 150 for the range bar and the lo/hi + 52-week line. The panel is
760 × 394 (1.14 MiB of bitmap, up from 0.99). `prev` is right-aligned on the change row and **dropped
when it would collide**, because a clipped "rev 29,638" is worse than no previous close.

**Verify.** `cargo test --release`: **40 passed, 0 failed** (was 31). `cargo clippy --release
--all-targets`: clean. Both binaries build and `--probe` returns six quotes and 61 bars each with
their own config. 36 screenshots rendered; card layout checked mechanically (no text run within 8 px
of a card edge, no two runs touching, on six themes × six cards) and reviewed by eye.

**Numbers.** Private RAM at 60 s: classic **5.71 MB** (was 5.77), Ultra **5.83 MB** (was 6.38). CPU:
classic 0.44 %, Ultra 0.23 %. Binary 1,068,544 → **1,091,072 B** (+22.5 KB) for both.

**Next.** The `up`/`down` semantic rule holds, but a monochrome retro theme cannot express direction
by hue alone — `gameboy` and `phosphor` bend it with a red-leaning fall. If that ever looks wrong in
use, the honest fix is a direction glyph, not another colour.

## 2026-10-22 — 1.3.0: two builds, gzip, themes, hover, alerts   [freebuff.impl]

**Files:** `Cargo.toml` (lib + two bins, ureq `gzip`, 1.3.0), `src/lib.rs` (new), `src/bin/{classic,ultra}.rs` (new),
`src/theme.rs` (new), `src/alerts.rs` (new), `src/{config,panel,ui,model,source}.rs`, `src/main.rs` (deleted),
`README.md`, `ARCHITECTURE.md`, `FEATURES.md`.

**What.** Maria's brief: ship a heavy "Ultra" build where the design work goes, keep the original exe and its one
theme exactly as they are, and let both builds receive the fixes. Plus the QoL list from the review, research on
webhooks/Discord, other data sources, and a symbol picker prototype.

- **One engine, two products.** `src/` became a library; `src/bin/classic.rs` and `src/bin/ultra.rs` are ~10 lines
each. The profile (`config::Profile`) fixes the config file, the single-instance mutex and the window title, so
`MH_Ticker.exe` and `MH_Ticker_Ultra.exe` run side by side on separate settings. No feature branch in the engine:
the classic build has fewer *menu entries*, not a different code path.
- **gzip.** `ureq` was declared `default-features = false`, which silently turned off its `gzip` feature, so CNBC's
chart JSON came down uncompressed. With `gzip` on, the same six-symbol refresh is **2,535,894 → 316,824 B**
(12.5 %) and the quote batch **5,346 → 1,345 B** (25 %): **20.63 → 3.35 MB/h, 6.2x**. That is why the full hour of
history now costs nothing and was *not* gated on the panel being open.
- **Day-based line colour (bug).** `panel::spark` coloured on `last >= first sample of the rolling hour` while the
card's % is the *day's* change, so a symbol up 1.2 % could draw a red line. It now colours on `day_baseline` -
the previous close, else the open implied by the change % - and the guide line moved there too. One function, the
same one the card's number uses.
- **Hover crosshair.** `WM_MOUSEMOVE` is quantised to 5 s buckets and only invalidates when the target actually
changes; the crosshair names the sample (`-12m  4,155.24`) instead of sliding under the cursor every frame.
`panel::hit` and `panel::card_rect` are shared with the painter, so a click can't disagree with the pixels.
- **Themes (Ultra).** `theme.rs` owns 5 palettes (midnight/graphite/aurora/terminal/paper), 64-band vertical
gradients with the brushes built **once per theme** (never per frame, or the panel would allocate ~1 MB per
repaint), and per-theme panel fonts. `up`/`down` stay semantic and separate from `accent`; a test enforces it.
- **Symbol picker (Ultra).** A chip strip inside the panel, toggled by the button beside the minimize cross. The
panel grows in place and stays centred.
- **Alerts + webhook (Ultra).** Threshold crossings only on fresh quotes (a market closed at a level would retrigger
forever), floored 30 s cooldown, tray balloon plus an optional POST to a Discord/Slack/ntfy webhook. The post runs
on a detached thread and reports back by `WM_ALERT`, because `Ui` is thread-local. Errors are reduced to
"HTTP <code>"/"network" and URLs are redacted to the host: for a webhook the URL *is* the credential.
- **Poll interval menu.** `poll_s` existed in the config with no way to reach it. The feed now reads the cadence
from the shared `Board`, so a change applies on the next tick instead of needing a restart.

**Verify.** `cargo test --release`: **31 passed, 0 failed** (was 20). New tests cover the day baseline, the
crosshair's sample lookup, picker geometry/hit-testing, the alert firing rules (cooldown, stale quotes, the
cooldown floor) and that no message can leak a webhook URL. `--probe` on both exes: 6 quotes and 60-61 one-minute
bars each, exit 0, and separate `config.json` / `config_ultra.json` written. RAM and CPU with the project's own
`.probe_tmp/measure.ps1`, 60 s window, one build at a time: **1.2.3 4.64→5.73 MB / 0.42 %**, **1.3.0 classic
4.70→5.77 / 0.44 %** (parity within 0.06 MB), **Ultra 4.79→6.38 / 0.52 %** (+0.6 MB, the price of being heavy).
Binaries: 954,880 → 1,068,544 B each. Clippy `--all-targets` clean (it surfaced a new class:
`pub fn` taking `HDC`/`HWND` became real public API when the crate became a library, so the library now
exports only `launch` and `config::Profile` and everything else is `pub(crate)`).

**Unverified.** The panel was not driven by hand in this pass, so the theme gradients, the crosshair and the picker
are correct at the geometry/unit level but have not been seen rendered. Bytes were measured on the wire with
`curl`, not instrumented inside the app. Webhook delivery was exercised against `payload()`/`redact()` only, not
against a live Discord channel. The webhook URL is stored in plaintext in the config: DPAPI (`CryptProtectData`)
is the recommended follow-up, not done here.

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
