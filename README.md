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

And the panel, in `aurora` (every theme is in [shots/](shots/) — rendered by the app itself):

![The panel in the aurora theme](shots/aurora.png)

## Two builds, one engine

Same source, same network code, same everything that makes it correct. What differs is where the
effort went:

| | `MH_Ticker.exe` | `MH_Ticker_Ultra.exe` |
|---|---|---|
| Look | one theme, exactly as it always was | **26 palettes in 4 tiers**, every one of them moving, plus 22 fonts — see [THEMES.md](THEMES.md) |
| Symbols | the *Symbols* menu | that **plus a picker strip inside the panel** |
| Alerts | — | price alerts, mute and quiet hours, tray balloon, optional **Discord/Slack/ntfy webhook** |
| Chart | — | a candle pane with ranges from 1D to 5Y, volume and alert levels |
| Memory after a minute | ~5.7 MB (measured on 1.7.1, not yet on 1.8) | ~5.8 MB (measured on 1.7.1, not yet on 1.8) |

They run side by side (separate settings files, separate tray icons). The classic exe keeps its name
and its look on purpose: if you just want the little ticker, nothing about it changed except the bugs.
Everything in the "whats new" list below that isn't a theme, a picker or an alert is in **both**.

## Download

Grab `MH_Ticker.exe` (or `MH_Ticker_Ultra.exe` if you want the themes and alerts) from the
[latest release](https://github.com/mariyaholic/mh-ticker/releases/latest) and
Double click it. If prompted, simply select Run anyway. 

## How to use it

- **Run it:** Double click `MH_Ticker.exe` and it docks itself into whatever free space ur taskbar has - left of
  the start button if ur icons are centred, or between ur app icons and the tray if they're on the left. It moves
  along with the icons as u open and close apps. Only one copy runs at a time so u can't accidentally stack them.
- **Expand it:** Click the strip (or left click the tray icon) and u get a card for each symbol with:
  - the price, its currency, the day change in % **and points**, and the previous close
  - a range bar under the line: where today's price sits between the session low and high, in one glance
  - the **52-week range** with the date its high was set, plus **volume** where CNBC sends one
  - where the price is from and how old it is
  - a little line of the last hour. it already has the past hour the moment u launch it (it pulls cnbc's
    1-minute bars) and then just keeps moving forward live. **the line's colour is the day's direction**, the
    same one the % next to it reports - so a symbol that's up 1% on the day stays green even while the last
    hour ticks down, and it can never disagree with its own number
  - **point at the line** and it tells u what the price was at that moment ("-12m  4,155.24") with a dotted
    crosshair, so u can actually read the hour instead of just eyeballing the shape. the session high and low
    sit as little dashes where the line turned, and the day's open is the faint guide line across the plot
  - **click a card** to open that symbol on CNBC, or right click it for *Copy XAU 4,155.24*. (In Ultra a click
    *locks* the card instead - see Candles below - and CNBC is an *Open on CNBC* button on the locked card.)
- **Put it back:** The minimize button top right, **Esc**, or the tray icon again
- **Pick ur symbols:** right click → *Symbols* and tick whatever u want from 23 of them (metals, crypto, us
  futures, oil, gas, copper, fx, the dollar index, us yields, vix, nikkei, dax, ftse, hang seng). the strip and
  the panel change right away, no restart. up to 24 at once, and *Reset to default* gets u back the original six
- **Pin to strip:** the taskbar strip can show only the symbols you pick, while the panel still shows all of them.
  Right click a card and choose *Pin to strip* or *Unpin from strip*. *Show all on strip* in the Symbols menu pins
  every card again. The last pinned card cannot be unpinned, and a settings file from before this version shows every card.
- **Rename a card:** right click a card and choose *Rename...*. Its alerts and its lock follow the new label. *Reset name*
  goes back to the symbol's own name. *Decimals* sets how many digits the price shows (0 to 6), or *Auto*, which uses the
  symbol's own count or one worked out from its price.
- **Order the cards once:** Symbols → *Order the cards (once)* sorts them by day move (biggest first), by name, or back to
  the default order. It changes the saved order one time and does not keep sorting.
- **Undo:** the main menu has an *Undo the last ...* line that says what it would undo: a removal, move, sort, pin change or
  alert clear. Ctrl+Z does the same on Ultra. It steps back one change, does not bring back a removed webhook, and is gone
  when you quit.
- **Add any symbol:** *Symbols* → *Add symbol...* and type a CNBC symbol like `AAPL`, `EUR=` or `@GC.1`. the app
  checks it with one request to CNBC before it adds it, and the name and first price come from CNBC. if the symbol
  isn't one CNBC knows, nothing is added. u can take it back off with *Symbols* → *Remove a symbol I added*, which
  also removes its alerts
- **Traffic:** every symbol on the strip is one more quote in each refresh, so the data grows with the number u
  show. the cost at 24 hasn't been measured yet. all of it goes to CNBC, except the optional fallback providers
  and one request to `api.github.com` for the update check below, which only happens when u click it. the
  diagnostics report (below) makes the same requests as `--probe` once, to the same hosts, only when u click it.
  the poll governor (below) can make fewer quote requests when nothing moves, never more than the *Poll every* setting
  asks for. a webhook POST that fails with a network error or a 5xx is tried once more, and never after a 4xx
- **Quiet markets and locks (both builds, on for Ultra):** right click → *Poll every* has two ticks, *Slow down when
  nothing moves* and *Rest while the PC is locked*. with them on, a quote is polled every 15 s once no shown price has
  changed for 10 minutes, every 60 s after 30 minutes, and once every 5 minutes while the PC is locked with no alert
  armed. while an alert is armed it's never slower than 15 s. any change, expanding the panel, the hotkey, an unlock or
  *Refresh now* goes back to the normal poll at once. Classic has both ticks off, so it polls as it did before unless u
  tick them. in a scripted two-hour run, a market that moved for 5 minutes and then sat still made 2,400 polls at a 3 s
  setting, and 465 with the governor (720 with an alert armed). the bytes weren't measured on the live feed. a source
  that keeps failing still backs off as before, so it can wait longer than 15 s while an alert is armed
- **Move it:** right click → *Position*, since not everyone has free space on the left of their taskbar:
  - **Auto** (default) - Stays in the taskbar and finds the space itself. it uses the left of start while theres
    room there, otherwise next to the tray, and if ur taskbar has no room at all (auto-hide or a vertical one) it
    goes to the bottom right corner just above the taskbar
  - **Taskbar, left of Start** / **Taskbar, next to the tray** - If u want it locked to one side
  - **Floating** - Drag it wherever u want and it remembers the spot
  - **the four corners** - One row tucked into that corner of the screen
  - **Display 1 / 2 / ...** - Only shows up if u have more than one screen, picks which one (and so which taskbar)
- **Poll faster or slower:** right click → *Poll every* (1 s to 60 s). it used to be a config edit only. at 1 s
  u get a genuinely live feel for about 3x the data; at 60 s it barely touches the network
- **Optional fallback prices:** right click → *Data sources* → *Allow fallback sources*. CNBC stays
  primary and fallback is **off by default**; enabling asks for confirmation every time. If CNBC fails or a
  quote is stale, Gold API may supply USD gold/silver spot prices, and Kraken may supply BTC/USD from a
  different exchange. Other symbols have no fallback. Your device connects directly to those providers.
  Each alternate quote names its provider and shows `age unknown` (or `?` in the strip); without a reliable
  observation timestamp it cannot trigger an alert. Alternate charts and day-change data are unavailable,
  rather than mixed with CNBC's. Turning fallback off clears its prices and rejects in-flight results;
  a request already queued at the toggle boundary may still connect or finish after disabling, but its
  result is discarded. Recovery depends on network timeouts and polling.
  Kraken requires prior permission for non-personal commercial use of its public API data; see
  [Kraken API notices](https://docs.kraken.com/api/docs/guides/global-intro).
- **Themes (Ultra):** right click → *Theme*, in four groups. **Basic** is one solid colour (midnight, ink, slate,
  moss, oxblood, taupe, paper, sand, mono). **Gradients** ramp top to bottom (graphite, aurora - the same violet-to-rose
  as the app icon, dusk, ocean, ember, glass). **Animated** ones drift on a 9-second loop (drift, pulse,
  vaporwave, aurora-live, sunset) and only animate while the panel is open, so a collapsed strip costs nothing.
  **Retro** ones are chunky, scanlined and drawn in a bitmap face (ega, amber-crt, phosphor, gameboy, c64, pc98 -
  the PC-98 palette with the gold accent). it changes immediately, no restart
- **Fonts (Ultra):** right click → *Font*. twenty-two faces in 4 groups (system, mono, serif, bitmap), applied to the
  the panel only - the strip keeps its own size so it still fits a taskbar gap. the retro themes pick a bitmap face
  themselves; an unknown name in the config is refused, and a face this machine does not have is greyed in the menu
  rather than silently substituted
- **Symbol picker (Ultra):** the button left of the minimize cross opens a strip of chips inside the panel - click
  one to add it, click again to drop it. same as the Symbols menu but u dont have to hunt through a submenu, and u
  see the whole list at once
- **Resize it (Ultra):** drag any edge or corner. the window is the layout: a wider frame adds card columns
  (up to six) instead of stretching the cards, and it remembers the size it was left at, so expanding it again
  comes back where u left it. no native border - the frame is still drawn by the app
- **Locking a card (Ultra):** click a card and it stays locked: an accent outline, and the candle pane (below)
  follows it. nothing changes when u just move the mouse, so u can travel to the chart without it changing under
  u. click the same card again to unlock it. CNBC is one explicit button on the locked card, *Open on CNBC*, and
  right click → *Open ... on CNBC* still works on any card
- **Candles (Ultra):** the candle button in the panel header opens a candle pane down the right side, charting the
  locked card (it locks the first card if nothing is). one-minute bars from the same CNBC request the sparklines
  already use, folded to 5/15/30/60 minutes on a narrow pane so a candle is never a one-pixel scratch. candles are
  whole-pixel and uniform, gridlines sit on round prices, and a tag on the price axis marks where it last traded.
  the time axis is on ur local clock, and the `local` / `UTC` word in the header switches it (it's saved). opening it on a narrow window widens the frame to fit both, **closing it
  glides the window back to the size and spot it came from**, centred again (unless u dragged the edge yourself), and the picker and the chart take turns (they
  want the same space)
- **Chart reading (Ultra):** move the pointer over a candle and a line in the header shows its time, open, high,
  low, close and change, with a crosshair and the price on the axis. the previous close is a dashed line marked
  `prev`, the day's high and low are small ticks on the right edge, and a faint dotted line marks where the
  market was shut. the day's change sits beside the last price
- **Suspect data (Ultra):** the chart's second header line can say `as of 14:05`. that means the data is past its
  refresh time and the refresh hasn't come back, so it's older than the chart looks (once it's nearly a day old it
  shows the date too). daily and longer ranges say `check dates` when their dates don't fall on the day boundary,
  which can mean the feed's dates are off. neither note shows while a candle is under the pointer, and 1D has none
- **Ranges (Ultra):** the tabs on the chart's second header line are `1D 5D 1M 6M 1Y 5Y`. `1D` is today. the others
  load the first time u click them, for the locked card, and refresh themselves when they get old. a range that
  fails says so and tries again after 30 seconds. `1M` and `6M` are daily bars trimmed to a month or half a year.
  daily and longer bars keep the exchange's own dates for everyone, so they have no local/UTC switch. The chart opens on the range you used last. The header shows the change over the range
  on screen (such as `5Y +182 %`) in place of the day's change, except on 1D, and the range's high and low are marked
- **Volume (Ultra):** where CNBC sends volume, a strip of volume bars sits under the candles. FX, yields and spot
  metals don't have it, so there's no strip for them
- **Alert levels on the chart (Ultra):** an alert level inside the chart's price range is a dashed line with its
  price at the right end. a level outside it is a marker on the nearest edge. right click the chart for *Alert
  at...* or *More...* (which opens the main menu)
- **Settings button (Ultra):** the cog at the left end of the panel header opens the same menu as the tray icon
  (Symbols, Poll, Theme, Font, Alerts, Position...) right under it, so u don't have to go hunting for the tray
- **Alerts (Ultra):** right click → *Alerts* → pick a symbol. the presets are above or below 0.5, 1, 2 or 5 % (of
  whatever it is right now), past today's high or low, the previous close, and a day move of ±1, 2, 3 or 5 %.
  *at a price...* takes an exact level, like `4,200.50`. u get a tray balloon and, if u set a webhook, a message in
  Discord/Slack/ntfy. it fires once per cooldown and **never on a stale price** (a market closed at a level would
  otherwise ping u forever). *Fire once* clears a level after it fires. Focus Assist eats balloons, so the tray icon
  gets a marker and its tooltip names the alert, and the *Recent* list keeps the last 50 with what happened to each,
  and *Copy list* copies them
- **Repeat (Ultra):** *Alerts* → *Repeat* is one choice: *Every cooldown* (the default, as before), *Fire once, then
  clear*, or *Each crossing*. a crossing alert fires when the price crosses its level, and only fires again after the
  price has gone back past the level by 0.1 %. if the first fresh price is already past the level, it fires once.
  a stale or fallback price fires nothing in any of them
- **Fast move (Ultra):** an alert can watch for a move of 1 % in 5 minutes, 2 % in 15 minutes or 3 % in 60 minutes. it's
  worked out from the prices the app already holds, and needs fresh prices covering the whole window with no gap longer
  than three polls. a window it can't cover fires nothing
- **Alerts at a glance (Ultra):** *Alerts* → *At a glance* lists everything set, with its distance, like
  `XAU above 4,200.00, 1.2 % away`, or `reached, 2.3 % past`. a level whose symbol isn't on the strip says it cannot
  fire. *Clear all alerts...* asks first. a card with a level has a small amber dot
- **Not checked (Ultra):** when no alert on a shown symbol has a fresh price for longer than 5 minutes (or ten polls,
  if that's longer), the tray tooltip says so, and *Recent* gets a line like `02:10 to 02:31 not checked (no fresh price)`.
  after a sleep it says `no checks ran, the PC slept`. *Alerts* → *Say when alerts could not be checked* sends one
  message to the webhook when checking is back, off by default. that message never has a price in it, and it isn't sent
  while alerts are muted or in quiet hours
- **Typed levels (Ultra):** in *at a price...* a comma only groups thousands, so `4,200.50` works. `4200,5`, `4,20`
  and `,500` are refused and the menu says why. a level more than 20 % away from the price asks first, and the question
  repeats the level and the price, so a slip like `42,005` for 4200.5 gets caught. a level within 20 % is set at once
- **Mute and quiet hours (Ultra):** *Alerts* → *Mute* for 1 hour, 8 hours or until u turn it back on. *Quiet hours*
  is off or a window of 22:00 to 07:00, 23:00 to 08:00 or 00:00 to 09:00, by ur PC's clock. a muted or quiet alert is still
  checked and shows up in *Recent*, but it makes no balloon and sends nothing to the webhook. when the mute or the quiet
  hours end, one balloon says how many alerts came in meanwhile, like `3 alerts while muted. See Alerts > Recent.`
  (nothing if there were none)
- **Webhook (Ultra):** *Alerts* → *Paste URL from clipboard* reads the URL u copied, checks it and asks before saving.
  the menu shows only its host. *Send a test alert* checks it works, and *Remove* deletes it. the URL is the password
  for that channel, so the app never shows or logs all of it. a POST that fails with a network error or a 5xx is tried
  once more after 3 seconds, and the log says `sent on retry` if that works. a 4xx is never retried. quitting waits up
  to 2 seconds for a POST still on its way
- **Start with Windows:** right click → tick *Start with Windows* and it starts when u sign in. untick it to stop.
  it's a `Run` entry for ur own account (no admin needed), and the menu reads the tick each time it opens
- **Hotkey:** right click → *Hotkey* → *Off*, one of four presets (`Ctrl+Alt+M`, `Ctrl+Alt+T`, `Ctrl+Shift+F12`,
  `Ctrl+Alt+Space`), or a custom one from the config. the key opens and closes the panel from anywhere. if another
  app already has the combination, the entry says so
- **Refresh now:** right click → *Refresh now* fetches every price and chart range at once, instead of waiting for
  the next poll. the backoff after a source that keeps failing still applies
- **Strip:** right click → *Strip* for the rows in the taskbar (1, 2 or 3; only when it's docked to the taskbar),
  the text size (small, default, large, larger) and the opacity (100 down to 60 %). changes apply at once
- **Check for updates:** right click → *Check for updates*. the app asks GitHub once for the latest release and
  tells u whether it's newer. if it is, it offers to open the download page. nothing else from GitHub's answer is
  used. it never checks by itself, only when u click. the request names the app `MH-Ticker` and nothing else about u
- **Copy all prices:** right click → *Copy all prices* puts the whole board on the clipboard, one symbol per line,
  tab-separated. in the open Ultra panel, Ctrl+C copies the locked card's line, or every price if nothing is locked
- **Copy diagnostics report:** right click → *Copy diagnostics report* runs the same checks as `--probe` and puts the
  result on the clipboard as text: the app version, window and display details, the settings path (with ur user name
  taken out), the webhook as its host only, and the data meter. it's for pasting into a message when something looks
  wrong. the menu says `checking...` while it runs, and a second click waits for the first one to finish
- **Data meter:** a greyed line above the version line in the menu shows how much the app has downloaded, like
  `Data: 3.1 MB this hour, 41 MB since start`. it counts what the app reads after compression is undone, so it's a
  little more than the network carried. request headers aren't counted
- **Keys in the open Ultra panel:** arrows move the lock between cards, Enter opens the locked card on CNBC, `C`
  opens the chart, `S` opens the symbol picker, Delete takes the locked card off the strip, and Ctrl+Left and
  Ctrl+Right move it one place. `/` opens a prompt that finds a card by its label or name, Home and End lock the first and
  last card, and Ctrl+Z undoes the last removal, move, sort, pin change or alert clear. the card menu has *Move left*, *Move right*, *Move first* and *Remove from strip*
  too, and a click on a symbol on a docked Ultra strip locks that card and opens the panel on it
- **Save failures:** if the settings can't be written (a read-only folder, say), the menu shows a greyed line saying
  so. it clears when a later save works
- **Quit or whatever else:** Right click the strip or the tray icon (or use the cog in the Ultra panel). u get *Expand view*, *Symbols*,
  *Poll every*, *Theme* and *Alerts* (Ultra), *Position*, *Show strip*, *Always on top*, what its currently doing,
  *Open config folder* and *Quit* - so no more going to Task Manager every time u need it killed
- **Tray only:** Untick *Show strip* if u just want the tray icon. hovering it still shows every price and clicking
  it still opens the panel
- **Games/fullscreen stuff:** it hides itself while something fullscreen is in front so it wont sit on top of ur game

**The tray icon** - Green means live, amber means some prices aren' t fresh, red means offline.

## Settings

`%LOCALAPPDATA%\MH_Ticker\config.json` gets made with the defaults the first time u run it (*Open config folder*
in the menu takes u there). **Ultra keeps its own `config_ultra.json`** in the same folder, so the two builds
never overwrite each other. if u edit it by hand, restart the app after. anything the menus change (symbols, poll, theme, font, alerts, strip,
hotkey, position, chart time zone) takes effect at once, with no restart.

If the settings file is there but won't load (it isn't UTF-8 text, or it isn't valid JSON), the app doesn't overwrite
it. it renames it beside the new one as `config.bad.json` (`config_ultra.bad.json` for Ultra), or `config.bad-2.json`
and so on after that, with the bytes unchanged, and loads the defaults. the menu has a greyed line naming the kept file.
the kept file may still hold ur webhook URL, so check it before u share it.

whats in there:
- `rows`: the symbols that show, each with a `label`, the cnbc `symbol`, `digits` and an optional `name`
- `catalog`: the list the Symbols menu shows, same fields. any symbol cnbc's quote pages have works, like
  `AUD=` for aud/usd or `@SI.1` for silver futures. *Symbols* → *Add symbol...* adds one from the menu, after a
  check against CNBC, and an entry can be taken out again with *Remove a symbol I added*
- `fallback_enabled`: optional fallback consent, `false` by default; use the menu to change it
- `poll_s`: seconds between price requests. the poll governor only ever waits longer than this, never shorter
- `slow_when_quiet` and `rest_when_locked` (Ultra): the two *Poll every* ticks for the poll governor. leave both out
  and Ultra has them on and Classic has them off, so Classic polls as before. a `true` or `false` written here wins
- `stale_after_s` and `delayed_grace_s`: when a price gets dimmed
- `placement` (`auto`, `taskbar_left`, `taskbar_right`, `floating`, `top_right`, `bottom_right`, `top_left`,
  `bottom_left`) and `monitor` (like `\\.\DISPLAY2`, empty = main screen)
- `dock_rows`, `dock_font_pt`: the grid when its in the taskbar
- `font`, `font_pt`, `opacity`, `topmost`
- `x`/`y`: where it was left in Floating
- `panel`: the panel's last frame as `[x, y, w, h]` (Ultra). leave it out and the panel sizes itself to its cards
  and centres - which is what every config written before the panel could be resized does
- `theme` (Ultra): any name from [THEMES.md](THEMES.md) - `midnight`, `aurora`, `vaporwave`, `pc98`, …
- `theme_font` (Ultra): a panel face from `theme::FACES`, e.g. `Georgia` or `Terminal`. empty = the theme's own
- `alerts` (Ultra): `{ label, above, below, day_move, once, cooldown_s, last, crossing, fast }` each. `label` is the row label,
  `above`/`below` are absolute prices (not %), `day_move` is a percent day change that fires it (or missing),
  `once` clears a level after it fires, `cooldown_s` is floored at 30 s and `last` is the last fire time so a
  restart doesn't re-ping u. `crossing` (missing means off) is the *Each crossing* repeat, and `fast` (missing means 0) is 1, 2 or 3 for
  the 1 % in 5 min, 2 % in 15 min and 3 % in 60 min fast moves
- `alert_crossing` and `gap_notice` (Ultra): `alert_crossing` true sets *Each crossing* for every alert (*Fire once*
  wins if both are set). `gap_notice` true sends the "could not be checked" webhook notice. both are off when missing, which is the default
- `alert_once`, `alerts_muted`, `alerts_mute_until` (seconds since 1970, UTC) and `quiet_hours` (`[from, to]` in
  minutes of the local day, or missing) (Ultra): what the *Alerts* menu sets for *Fire once*, *Mute* and *Quiet hours*
- `webhook` (Ultra): a URL to also POST alerts to. **this is a secret** - the URL is the password for that
  channel, so dont paste it anywhere public. set it from *Alerts* → *Paste URL from clipboard* or here. the app
  never prints it (menus and `--probe` show the host at most)
- `chart_utc` (Ultra): `true` puts the chart's time axis on UTC. the `local` / `UTC` word in the chart header sets it
- `hotkey`: the global key to open and close the panel, as text like `Ctrl+Alt+M`, or `null` for none. the keys are
  letters, digits, `F1` to `F24`, `Space`, `Home`, `End`, `Insert`, `Delete`, `PageUp` and `PageDown`, with at
  least one of `Ctrl`, `Alt` or `Win`. the menu's presets are the usual choice

`MH_Ticker --probe` does one pass over the prices, the day's bars and (for the first row) each chart range file,
judges each assumption as PASS, WARN or NOTE, prints it and exits - handy for checking a new symbol actually works
before u add it, and for checking the ranges on a machine with a connection. a WARN exits with 1, so a script can
use it. a NOTE never fails the run; it marks a check the answers can't show, like bar times while the market is shut.
`--probe` reads the settings but never writes them.

`MH_Ticker --probe --dump <folder>` does the same and also saves the raw answers into that folder as files (the
quotes, the unknown-symbol answer, the day files, the range files and the release answer), for making test fixtures.
the folder is created if it's missing. it never writes the settings or the webhook. it only works from the command
line, and it needs both the folder and `--probe`; without them it exits with 2 and says why

## Building it Yourself

You only need Windows 10 onwards and Rust (most recent stable release).

```sh
cargo test
cargo build --release
```

That builds **both** exes: `target/release/MH_Ticker.exe` and `target/release/MH_Ticker_Ultra.exe`. they're one
library (`src/`) with two tiny entry points (`src/bin/`), so u can't fix something in one and forget the other.
copy whichever u want next to this readme if u want to double click it from here. If u want the build somewhere else (another drive or whatever) set `CARGO_TARGET_DIR` or keep an untracked
`.cargo/config.toml` with `[build] target-dir = "..."` - mine goes to `D:/mariyaholic-build/mh-ticker/target`

It's plain win32/gdi - no GUI toolkit, no webview, no GPU stuff - so it only uses about 5.7 MB of memory and like
~0.4% of one cpu core.

Want to see every theme before picking one? They are rendered by the app's own painter:

```sh
cargo run --release --example shots --features shots -- shots/
```

that writes `shots/*.png` (three frames for each animated theme) and prints the path of each. the PNG encoder
sits behind the `shots` feature, so neither shipped exe links it - the binaries are byte-for-byte what u download.

## License

[MIT](LICENSE) © 2026 mariyaholic - Use it, change it, share it, just keep the copyright notice and the license
text with it. And if u build something on top of it please credit this repo as the original source!
