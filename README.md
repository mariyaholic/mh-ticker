# MH Ticker

_made with love by [maria](https://github.com/mariyaholic)_

a little price strip that sits in the empty part of ur windows taskbar. click it and it glides out into a
bigger window in the middle of the screen, hit minimize and it glides right back into the taskbar. prices
come from cnbc's public quotes so theres **no login, no account, no server** - its literally just one exe,
double click and ur done

```
XAU 4,153.73   BTC 81,219   S&P 7,743.75
XAG    56.590  DOW 52,048   NAS 29,801.50  ●
```

![MH Ticker in the gap next to the tray](docs/strip_next_to_tray.png)

## download

grab `MH_Ticker.exe` from the [latest release](https://github.com/mariyaholic/mh-ticker/releases/latest) and
double click it. its not code-signed so windows smartscreen might say it "protected your PC" the first time -
just hit *More info → Run anyway*, or build it urself from the source if u'd rather (see build below)

## how to use it

- **run it:** double click `MH_Ticker.exe` and it docks itself into whatever free space ur taskbar has - left of
  the start button if ur icons are centred, or between ur app icons and the tray if theyre on the left. it moves
  along with the icons as u open and close apps. only one copy runs at a time so u cant accidentally stack them
- **expand it:** click the strip (or left click the tray icon) and u get a card for each symbol with:
  - the price
  - the day change in % and points, plus the previous close
  - where the price is from and how old it is
  - a little line of the last hour. it already has the past hour the moment u launch it (it pulls cnbc's
    1-minute bars) and then just keeps moving forward live
- **put it back:** the minimize button top right, **Esc**, or the tray icon again
- **pick ur symbols:** right click → *Symbols* and tick whatever u want from 23 of them (metals, crypto, us
  futures, oil, gas, copper, fx, the dollar index, us yields, vix, nikkei, dax, ftse, hang seng). the strip and
  the panel change right away, no restart. max 12 at once, and *Reset to default* gets u back the original six
- **move it:** right click → *Position*, since not everyone has free space on the left of their taskbar:
  - **Auto** (default) - stays in the taskbar and finds the space itself. it uses the left of start while theres
    room there, otherwise next to the tray, and if ur taskbar has no room at all (auto-hide or a vertical one) it
    goes to the bottom right corner just above the taskbar
  - **Taskbar, left of Start** / **Taskbar, next to the tray** - if u want it locked to one side
  - **Floating** - drag it wherever u want and it remembers the spot
  - **the four corners** - one row tucked into that corner of the screen
  - **Display 1 / 2 / ...** - only shows up if u have more than one screen, picks which one (and so which taskbar)
- **quit or whatever else:** right click the strip or the tray icon. u get *Expand view*, *Symbols*, *Position*,
  *Show strip*, *Always on top*, what its currently doing, *Open config folder* and *Quit* - so no more going to
  task manager every time u need it killed
- **tray only:** untick *Show strip* if u just want the tray icon. hovering it still shows every price and clicking
  it still opens the panel
- **games/fullscreen stuff:** it hides itself while something fullscreen is in front so it wont sit on top of ur game

**the tray icon** is a tiny chart line on a dark tile - green means live, amber means some prices arent fresh (a
market's probably closed), red means offline. windows 11 likes to hide new tray icons in the **^** flyout, so if u
want it out on the taskbar just drag it out of there, or turn it on in *Settings → Personalization → Taskbar →
Other system tray icons → MH Ticker*

## where the prices come from

one request every 3 seconds grabs every row at once over a connection that stays open, so its super light. the
default six:

| Row | CNBC symbol | what it is | timing |
|---|---|---|---|
| XAU | `XAU=` | gold spot, USD | real-time |
| XAG | `XAG=` | silver spot, USD | real-time |
| BTC | `BTC.CB=` | bitcoin on coinbase | real-time |
| DOW | `@DJ.1` | dow futures, front month | **delayed ~10 min** |
| S&P | `@SP.1` | s&p 500 futures, front month | **delayed ~10 min** |
| NAS | `@ND.1` | nasdaq 100 futures, front month | **delayed ~10 min** |

- **theres more in the Symbols menu:** platinum and eth (real-time), wti, brent, natural gas and copper futures
  (delayed), eur/usd, gbp/usd, usd/jpy and the dollar index, the us 10 and 2 year yields, vix, and nikkei, dax,
  ftse and hang seng (delayed)
- **delayed ones** say *CNBC delayed* in the panel and get an extra 15 min before theyre dimmed as stale, since
  theyre always ~10 min behind anyway
- **why futures and not the actual indexes:** the cash indexes (`.DJI`, `.SPX`, `.NDX`) only move during us
  hours, the futures trade pretty much around the clock
- **heads up:** these are public but undocumented cnbc endpoints, totally fine for personal use but cnbc could
  change them whenever. if that happens the dot turns red and the tray menu tells u whats wrong

## settings

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

`MH_Ticker --probe` does one pass over the prices and the last hour of bars, prints it and exits - handy for
checking a new symbol actually works before u add it

## building it urself

u need windows and rust (stable)

```sh
cargo test
cargo build --release
```

the exe ends up at `target/release/MH_Ticker.exe`, copy it next to this readme if u want to double click it from
here. if u want the build somewhere else (another drive or whatever) set `CARGO_TARGET_DIR` or keep an untracked
`.cargo/config.toml` with `[build] target-dir = "..."` - mine goes to `D:/mariyaholic-build/mh-ticker/target`

its plain win32/gdi - no gui toolkit, no webview, no gpu stuff - so it only uses about 5 MB of memory and like
~0.1% of one cpu core

## where it came from

its a fork of AO Ticker, a ticker i had built for my own trading setup (Argonaut Oak `apps/ao_ticker`, commit
`aaa93149`). i essentially just wanted the widget and the expanded view as its own one click app, so the
argonaut oak bits (the local market terminal feed and the mt5 investor bridge, which needs an mt5 login) got
taken out and swapped for cnbc. the strip, the taskbar docking, the panel and its animation and the tray are
all the same code

## license

[MIT](LICENSE) © 2026 mariyaholic - use it, change it, share it, just keep the copyright notice and the license
text with it. and if u build something on top of it please credit this repo as the original source
