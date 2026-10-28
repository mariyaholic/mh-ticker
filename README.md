# MH Ticker

_made with love by [maria](https://github.com/mariyaholic)_

A little, extremely resource efficient (~5 MB RAM, and any potato CPU won't feel it) price ticker that sits in the empty part of ur windows taskbar. Click it and it glides out into a
bigger window in the middle of the screen, hit minimize and it glides right back into the taskbar. Prices
come from CNBC's public quotes. You don't need any login, code, payment, whatever. Just click the .exe file!
```
XAU 4,153.73   BTC 81,219   S&P 7,743.75
XAG    56.590  DOW 52,048   NAS 29,801.50  ●
```

![MH Ticker in the gap next to the tray](docs/strip_next_to_tray.png)

## Download

Grab `MH_Ticker.exe` from the [latest release](https://github.com/mariyaholic/mh-ticker/releases/latest) and
Double click it. If prompted, simply select Run anyway. 

## How to use it

- **Run it:** Double click `MH_Ticker.exe` and it docks itself into whatever free space ur taskbar has - left of
  the start button if ur icons are centred, or between ur app icons and the tray if they're on the left. It moves
  along with the icons as u open and close apps. Only one copy runs at a time so u can't accidentally stack them.
- **Expand it:** Click the strip (or left click the tray icon) and u get a card for each symbol with:
  - the price
  - the day change in % and points, plus the previous close
  - where the price is from and how old it is
  - a little line of the last hour. it already has the past hour the moment u launch it (it pulls cnbc's
    1-minute bars) and then just keeps moving forward live
- **Put it back:** The minimize button top right, **Esc**, or the tray icon again
- **Pick ur symbols:** right click → *Symbols* and tick whatever u want from 23 of them (metals, crypto, us
  futures, oil, gas, copper, fx, the dollar index, us yields, vix, nikkei, dax, ftse, hang seng). the strip and
  the panel change right away, no restart. max 12 at once, and *Reset to default* gets u back the original six
- **Session timers:** the expanded panel also shows Sydney, Tokyo, London and New York in that fixed order,
  with local time, open/closed and a countdown to the next open or close. Below them are the London–New York
  overlap, the broker server clock, and exactly one device-zone clock detected from Windows. Right click →
  *Session timers* hides or shows the block. The times are common FX-session conventions, not guaranteed
  exchange hours; there is no holiday calendar in v1. See [docs/SESSIONS.md](docs/SESSIONS.md).
- **Broker clock:** the default preset is a reported, *unverified* Exness-style rule, so the panel shows the
  server time but marks the next reopen unknown until u confirm the rule in config. Set
  `sessions.broker_offset_min`, `sessions.broker_reopen_min` and `sessions.broker_rule_verified` in
  `config.json` if u have checked them against ur own server. A configured offset is an estimate, not a live
  feed.
- **Move it:** right click → *Position*, since not everyone has free space on the left of their taskbar:
  - **Auto** (default) - Stays in the taskbar and finds the space itself. it uses the left of start while theres
    room there, otherwise next to the tray, and if ur taskbar has no room at all (auto-hide or a vertical one) it
    goes to the bottom right corner just above the taskbar
  - **Taskbar, left of Start** / **Taskbar, next to the tray** - If u want it locked to one side
  - **Floating** - Drag it wherever u want and it remembers the spot
  - **the four corners** - One row tucked into that corner of the screen
  - **Display 1 / 2 / ...** - Only shows up if u have more than one screen, picks which one (and so which taskbar)
- **Quit or whatever else:** Right click the strip or the tray icon. u get *Expand view*, *Symbols*, *Position*,
  *Show strip*, *Always on top*, what its currently doing, *Open config folder* and *Quit* - so no more going to
  Task Manager every time u need it killed
- **Tray only:** Untick *Show strip* if u just want the tray icon. hovering it still shows every price and clicking
  it still opens the panel
- **Games/fullscreen stuff:** it hides itself while something fullscreen is in front so it wont sit on top of ur game

**The tray icon** - Green means live, amber means some prices aren' t fresh, red means offline.

## Settings

`%LOCALAPPDATA%\MH_Ticker\config.json` gets made with the defaults the first time u run it (*Open config folder*
in the menu takes u there). if u edit it by hand restart the app after (the menus dont need a restart tho).
whats in there:
- `rows`: the symbols that show, each with a `label`, the cnbc `symbol`, `digits` and an optional `name`
- `catalog`: the list the Symbols menu shows, same fields. any symbol cnbc's quote pages have works, like
  `AUD=` for aud/usd or `@SI.1` for silver futures - add it here and then tick it in the menu
- `poll_s`: seconds between price requests
- `stale_after_s` and `delayed_grace_s`: when a price gets dimmed
- `placement` (`auto`, `taskbar_left`, `taskbar_right`, `floating`, `top_right`, `bottom_right`, `top_left`,
  `bottom_left`) and `monitor` (like `\\.\DISPLAY2`, empty = main screen)
- `dock_rows`, `dock_font_pt`: the grid when its in the taskbar
- `font`, `font_pt`, `opacity`, `topmost`
- `x`/`y`: where it was left in Floating
- `sessions`: `enabled`, optional `broker_offset_min` / `broker_reopen_min`, `broker_rule_verified`, and an
  optional `device_zone` IANA name (e.g. `Europe/Berlin`) if Windows detection fails or picks the wrong zone.
  A configured device zone is labelled *configured*, not *auto-detected*.

`MH_Ticker --probe` does one pass over the prices and the last hour of bars, prints it and exits - handy for
checking a new symbol actually works before u add it

## Session engine and indicators

The session rules live in one versioned spec, [`spec/sessions.json`](spec/sessions.json), pinned to tzdb
2025b. `cargo run -p mh-specgen` generates the Rust constants, the bounded MQL5 transition tables and both
indicator files from it; `--check` fails if anything drifted. The shared UTC golden vectors are in
[`spec/golden/vectors.json`](spec/golden/vectors.json).

- **Rust engine:** `crates/session-engine` is platform independent and tests on Linux/macOS/Windows with
  `cargo test`. It handles half-open `[open, close)` intervals, DST transitions, the London–New York overlap
  and weekend reopen.
- **Pine v6:** [`pine/MH_Sessions.pine`](pine/MH_Sessions.pine) is a display-only boilerplate. Pine cannot
  read the chart or device time zone, so its one extra clock is a manual `input.string` IANA dropdown
  (default `syminfo.timezone`, the exchange zone) and is never auto-detected. It updates on bar events, so it
  shows the last update and has no session countdowns.
- **MQL5:** [`mql5/MH_Sessions.mq5`](mql5/MH_Sessions.mq5) is display-only, strictly read-only, with no
  trading functions. It embeds bounded IANA transition tables and fails visibly outside their coverage. The
  broker offset is an estimate from `TimeTradeServer()`/`TimeGMT()` or a configured override. Not affiliated
  with MetaQuotes; MT5 is a MetaQuotes trademark.

See [docs/SESSIONS.md](docs/SESSIONS.md) for the rules, limits and verification status, and
[docs/TESTING.md](docs/TESTING.md) for what still needs a real Windows/MT5/TradingView check.

## Building it Yourself

You only need Windows 10 onwards and Rust (most recent stable release). The app is Win32-only, but the session
engine and generator are portable:

```sh
cargo test                 # session engine + generator + golden vectors, any OS
cargo run -p mh-specgen -- --check
cargo build --release -p mh_ticker
```

The exe ends up at `target/release/MH_Ticker.exe`, copy it next to this readme if u want to double click it from
here. If u want the build somewhere else (another drive or whatever) set `CARGO_TARGET_DIR` or keep an untracked
`.cargo/config.toml` with `[build] target-dir = "..."` - mine goes to `D:/mariyaholic-build/mh-ticker/target`

It's plain win32/gdi - no GUI toolkit, no webview, no GPU stuff - so it only uses about 5 MB of memory and like
~0.1% of one cpu core.

## License

[MIT](LICENSE) © 2026 mariyaholic - Use it, change it, share it, just keep the copyright notice and the license
text with it. And if u build something on top of it please credit this repo as the original source!

## Privacy and offline behaviour

Session timers use bundled time-zone data and work offline. The quote feed sends configured symbols to
CNBC over HTTPS; prices and charts require a network connection. Settings are stored locally in
`%LOCALAPPDATA%\MH_Ticker`. No account or local server is required.

Windows binaries are not code-signed yet and may trigger Microsoft SmartScreen. Review the source and
verify the checksum before running a binary. MH Ticker is free and open source (MIT).

For release builds, run `scripts/build-windows.ps1` on Windows or `scripts/build-windows.sh` on Linux
with the Windows GNU target and MinGW installed. Both write the executable and SHA256SUMS to `dist/`;
the Linux script supports `DIST_DIR` and `CARGO_TARGET_DIR`. The scripts use the last commit time as
`SOURCE_DATE_EPOCH` unless set. Rust 1.99.1 and the lockfile are used for the reviewed build; byte-for-byte
reproducibility across toolchain versions and machines has not been verified.
