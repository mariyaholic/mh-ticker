# Changelog — MH Ticker

## unreleased - 1.9: know it is true   [claude.impl]

**Files:** mostly `src/`: new `probe.rs`, `report.rs`, `meter.rs`, `governor.rs`, `watch.rs` and `retry.rs`; changed
`alerts.rs`, `chartview.rs`, `config.rs`, `ranges.rs`, `source.rs`, `update.rs`, `panel.rs`, `ui.rs`, `shot.rs`
and `Cargo.toml` (the `Win32_System_RemoteDesktop` feature). Also `examples/wide.rs`
(new), `shots/wide-*.png` (new), `scripts/check-state-linux.sh`, `README.md`, `SOURCES.md`, `CHANGELOG.md`
and `docs/1.9/`.

**What.** The 1.9 packages, grouped by what you see. The first group adds no new feature; it makes the features
already here easier to check. The alerts group adds behaviour. Each group says which build gets it: Classic is `MH_Ticker.exe`, Ultra is `MH_Ticker_Ultra.exe`.

- **Checking the feed (both builds).**
  - `--probe` judges each assumption and prints PASS, WARN or NOTE. It checks the quote fields, an unknown
    symbol, the time spelling, whether a bar's time is its start or its end, the stamps of daily bars, volume,
    and the release tag. It also lists response field names the parser ignores. A WARN makes the run exit with 1.
    A NOTE never fails a run. It marks a check the answers cannot show, such as bar start while the market is shut.
  - `--probe` reads the settings and never writes them.
  - `--probe --dump <folder>` (command line only) saves the raw answers into the folder for test fixtures. It
    creates the folder if it is missing. It never writes the settings or the webhook. Without a folder, or without
    `--probe`, it exits with 2 and says why.
  - *Copy diagnostics report* (in the right-click menu): runs the same checks and puts the text on the clipboard.
    It gives the version, window mode, DPI, placement, counts, the settings path with the user name removed, the
    kept settings file if there is one, the hotkey, autostart, the webhook as its host only, and the data meter.
    While it runs the menu shows a greyed "checking" line. A second click is ignored until it finishes.
- **The data meter (both builds).** A greyed menu line above the version line, for example
  `Data: 3.1 MB this hour, 41 MB since start`. It counts response bytes as they are read, after gzip is undone, so
  it is higher than the compressed bytes on the wire. Request headers are not counted. It makes no request of its own.
- **The chart says when it is suspect (Ultra).** The chart's second header line can show `as of 14:05`, when the
  series is past its refresh time and the refresh has not come back. Older data also shows its date. Daily and
  longer ranges show `check dates` when their stamps do not fall on a day boundary. Neither note shows while a
  candle is under the pointer, and 1D has no note.
- **Local time per instant (Ultra).** Each candle and each alert time is read on the clock in force at its own
  instant. Candles on either side of a clock change each sit on their own local midnight. The mute label shows the
  right hour after a change. Daily ranges keep the exchange's dates.
- **Typed levels (Ultra).** Commas in a level must group thousands. `4,200.50` is read as 4200.50. `4200,5`,
  `4,20`, `1,2345` and `,500` are refused, and the menu gives the reason. A level more than 20 % from the price
  asks before it is set, and the question echoes the level and the price in the symbol's digits. A level within
  20 % is set at once, as before.
- **A settings file that will not load is kept (both builds).** A file that is not UTF-8 or does not parse is
  renamed beside the new one, to `config.bad.json` (or `config_ultra.bad.json`), then `.bad-2.json` and so on.
  Its bytes are moved, not rewritten. The defaults load and a greyed menu line names the kept file. If the rename
  fails, the file stays and the error is reported as before.
- **Hardening (both builds, the chart items Ultra only).**
  - A price that is NaN or infinite prints as `--`, in the card text and in *Copy all prices*.
  - A NaN price is drawn in the middle of the chart's plot, and an infinite one is held inside it.
  - At one-hour candles in the narrowest pane, the oldest candles that cannot fit are dropped, so no candle is
    drawn past the right edge.
  - The update check identifies itself as `MH-Ticker`: no version number and no browser string. CNBC requests
    keep their browser string.
  - A wall clock set back no longer silences an alert until the clock catches up. The alert counts as cooled and
    fires once more, with a fresh stamp.
- **Pictures (in the repository, not in the exes).** Fourteen new pictures, `shots/wide-*`: 24 cards, 24 symbols
  on one, two and three strip rows, the narrow pane, 192 DPI, and each range. The 36 existing pictures are unchanged.
- **Alerts that say when they cannot see (the governor: both builds; the alert parts: Ultra).**
  - **Poll governor.** When nothing moves, quotes are polled less often: every 15 s once no shown quote has changed
    for 10 minutes, and every 60 s after 30 minutes. With the PC locked and no alert armed, one poll every 5 minutes.
    While an alert is armed, the wait is never longer than 15 s. Any change, expanding a card, the hotkey, an unlock
    or *Refresh now* goes back to the normal poll at once. Two ticks under *Poll every* turn it on or off: *Slow down
    when nothing moves* and *Rest while the PC is locked*. Ultra has both on unless a choice is saved. Classic has both
    off, so it polls as it did in 1.8 unless the user ticks them. The governor never polls more often than *Poll every*.
    A source that keeps failing still backs off as before, so it can wait longer than 15 s while an alert is armed.
  - **Scripted saving.** In a scripted two-hour run, a market that moved for five minutes and was then quiet made
    2,400 polls at a 3 s base. The governor made 465, or 720 with an alert armed. Bytes were not measured.
  - **Not checked.** When no alert on a shown symbol has a fresh price for longer than the larger of 5 minutes and
    ten polls, the tooltip gets a line, and Recent gets one line when checking is back, for example
    `02:10 to 02:31 not checked (no fresh price)`. After a sleep the line says `no checks ran, the PC slept`. A gap
    needs every armed alert on a shown symbol to be blind. An optional webhook notice (*Say when alerts could not be
    checked*, off by default) sends one message when checking is back, `Alerts could not be checked from 02:10 to 02:31.`
    It never carries a price, and it is not sent while alerts are muted or in quiet hours.
  - **Mute digest.** When a mute or quiet hours end, one balloon says how many alerts came in meanwhile, for example
    `3 alerts while muted. See Alerts > Recent.` Nothing is shown when there were none. No timer was added.
  - **Webhook retry.** A POST that fails with a network error or a 5xx is tried once more after 3 s. A 4xx is never
    retried, and a second failure is final. A retry that gets through is logged as `sent on retry`. Quitting waits up
    to 2 s for a POST still in flight, and a waiting retry is sent at once.
  - **Each crossing.** The *Repeat* submenu has *Every cooldown* (still the default), *Fire once, then clear* and
    *Each crossing*. A crossing alert fires when the price crosses its level, and fires again only after the price
    has gone back past the level by 0.1 %. If the first fresh quote is already beyond the level, it fires once.
  - **Fast move.** An alert can watch for a move of 1 % in 5 minutes, 2 % in 15 minutes or 3 % in 60 minutes. It is
    judged from the price history already held, and needs the whole window covered by fresh samples with no gap
    longer than three polls. In every repeat mode, a stale or fallback quote fires nothing.
  - **Alerts at a glance.** *Alerts* → *At a glance* lists every level and move set, with its distance
    (`XAU above 4,200.00, 1.2 % away`) or `reached, 2.3 % past`. A level whose symbol is not on the strip is marked
    as unable to fire. *Clear all alerts...* asks first. A card with a level has a small amber dot. *Recent* keeps the
    last 50 entries, and *Copy list* copies them.

**Checked.** Under Wine: 334 tests pass, plus the one known Wine-only failure
(`ui::tests::a_theme_switch_takes_the_new_themes_face`, because Wine has no Consolas). The Linux harness
(`scripts/check-state-linux.sh`) passes 159. The 36 old stills are identical to the baseline (36 of 36), and 14 new
stills were drawn. Both release exes are about 1.42 MB.

**Not verified.** The live CNBC and GitHub answers: `--probe`, the report and the release tag have not run
against the services. On a real Windows desktop: the clipboard, the menu lines, a real clock change, the real
fonts, and gzip byte counts against a capture. Also not verified: the real lock and unlock, and
`WTSRegisterSessionNotification` on a real desktop; sleep and resume; a real 5xx from a webhook host; quitting with
a POST in flight; and the poll governor's saving and bytes against the live feed.

## 2026-11-14 — 1.8.0: alerts you can set and silence, a chart that reads, typed symbols   [claude.impl]

**Files:** most of `src/` (`alerts.rs`, `chartview.rs`, `ranges.rs`, `keys.rs`, `hotkey.rs`, `update.rs`,
`autostart.rs`, `prompt.rs`, `source.rs`, `config.rs`, `menu_commands.rs`, `panel.rs`, `ui.rs`, `model.rs`,
`shot.rs`), `examples/chart.rs`, `scripts/check-state-linux.sh`, `README.md`, `SOURCES.md`,
`docs/STATE-AUDIT-1.8.md`, `docs/1.8/`, `Cargo.toml` (version 1.8.0).

**What.** The 1.8 cycle, grouped by what you see. Each group says which build gets it: Classic is
`MH_Ticker.exe`, Ultra is `MH_Ticker_Ultra.exe`.

- **Alerts (Ultra).**
  - New presets: above and below at 0.5, 1, 2 and 5 % of the current price; past today's high or low (one
    tick beyond the extreme, so a price already there does not fire at once); the previous close; and a day
    move of plus or minus 1, 2, 3 or 5 %.
  - *Alerts > at a price...* takes a typed level such as `4,200.50`. A level above the price is an above
    alert, a level below is a below alert.
  - *Fire once* clears an alert's level after it fires. The checkbox changes every alert at once.
  - *Mute* for 1 hour, 8 hours or until turned back on. *Quiet hours* (off, 22:00 to 07:00, 23:00 to 08:00,
    or 00:00 to 09:00, local time). A silenced alert is still checked and listed in *Recent*, but it shows no
    balloon and sends nothing to the webhook.
  - The webhook is set from the menu: *Paste URL from clipboard* checks the text and asks before saving, and
    shows only the host. *Send a test alert* and *Remove* are there too. The URL is never shown, logged or put
    in an error message.
  - *Recent* lists the last 10 alerts with their outcome (shown, muted, quiet hours, sent, send failed). It is
    kept in memory only.
  - When an alert fires, the tray icon gets a marker and the tooltip's first line reads `Alert: <title>`. Both
    clear when the panel opens or a menu is used.
  - Alerts still fire only on a fresh CNBC price, as before.
- **The chart (Ultra).**
  - A hover readout on the candle pane: time, open, high, low, close and the candle's change, with a crosshair
    and a price line. The readout line is always reserved, so the plot does not jump.
  - Local time on the candles and the time axis. The `local` / `UTC` word in the header switches it, and the
    choice is saved.
  - Reference lines: the previous close (dashed, labelled `prev`), and the day's high and low as ticks on the
    right edge. A faint dotted line marks each session break, where the market was shut.
  - The day's change sits beside the last price, in the colour of its direction.
  - Ranges `1D`, `5D`, `1M`, `6M`, `1Y` and `5Y`. Ranges other than `1D` are read from CNBC range files for the
    locked symbol, when the tab is first clicked. A file is refreshed when it is old (10 minutes for `5D`, an
    hour for the daily files, six hours for `5Y`). A failed range retries after 30 seconds.
  - Volume bars under the candles where the feed sends volume. FX, yields and spot metals have none, so no
    strip is drawn.
  - Alert levels on the chart: a dashed line inside the price window, and a marker on the nearest edge outside
    it. Right-click the chart for *Alert at* and *More...*.
- **Everyday (both builds).**
  - *Start with Windows* adds or removes a `Run` key for the profile. The tick is read each time the menu opens.
  - *Hotkey* opens and closes the panel from anywhere. Four presets, or a custom combination from the config.
  - *Refresh now* fetches every price at once and asks again for a chart range that failed.
  - *Strip* submenu: rows in the taskbar (1, 2 or 3, only when docked to the taskbar), text size, and opacity.
    Changes apply at once.
  - A version line, and *Check for updates*. The check makes one request to `api.github.com`, and only when you
    click it. The download link is a fixed address, never a link taken from the answer.
  - *Copy all prices* copies the board as tab-separated text. A stale price gets a `stale` field.
  - A greyed line under the status says when settings could not be saved, and clears after a later save works.
- **Keyboard, card menu and order (Ctrl+C both builds; the rest Ultra).**
  - In the open panel: arrows move the lock, Enter opens the locked card on CNBC, `C` opens the chart, `S` opens
    the symbol picker, Delete removes the locked card from the strip, and Ctrl+Left and Ctrl+Right move it one
    place. Ctrl+C copies the locked card's line, or every price when nothing is locked.
  - The card menu gets *Move left*, *Move right*, *Move first* and *Remove from strip* (Ultra).
  - A click on a symbol on a docked or cornered Ultra strip locks that card and opens the panel on it.
  - Moving a card changes the Symbols catalogue, so the order survives ticking and unticking symbols.
- **Add a symbol, up to 24 (both builds).**
  - *Symbols > Add symbol...* checks the typed symbol against CNBC with one quote request, then adds it to the
    menu and the strip. *Remove a symbol I added* removes it again, and its alerts with it.
  - The strip shows up to 24 symbols, up from 12. Each shown symbol adds quote requests on every refresh, so
    traffic grows with the number shown. The cost at 24 was not measured.
- **Responsiveness (both builds).**
  - A hover repaints only the part that changed: a button, the candle pane, or the cards the pointer left and
    entered. Under Wine the candle pane's hover repaint went from 3.32 ms to 2.37 ms a frame on 1D, and from
    3.94 ms to 2.79 ms on 5Y. Full repaints are unchanged. These Wine numbers are ratios between runs, not Windows timings.
  - The catalogue is no longer copied on every frame. The picker's list is built only while the picker is open,
    which took 3.9 us a frame with 23 rows, and now takes nothing with the picker closed.
  - Every settings save now happens outside the UI state borrow. This keeps a rule true. It does not make anything faster.
  - Measured and ruled out: re-folding the candle series on each repaint (165 us for 4,000 one-minute bars, under a
    tenth of a frame) and text measurement on each feed tick (0.33 us a call). Neither was changed.
- **Fixes from the state audit (both builds unless noted).**
  - *Refresh now* asks again at once for a failed longer range. It used to wait out a 30 second back-off.
  - The fallback toggle writes the settings as they are after its confirmation box. The old copy could bring a
    spent once alert back on the next start.
  - A test alert's result reads correctly after the log list is cleared.
  - `Alerts (n)` in the menu counts only the symbols the submenu lists.
  - A second *Add symbol* prompt from the tray now says a lookup is already running, instead of dropping its
    symbol silently.
  - The card menu's *Copy* and *Open* act on the card you clicked, even if the strip has moved since the menu opened (Ultra).
  - The tray marker no longer lights for an alert that fires while the panel is open in front. It still lights when the panel is behind another window (Ultra).
  - A menu opened before a hotkey or tray click changed the panel's mode acts on the mode you choose, so *Expand view* does nothing once the panel is already open.
  - Moving the panel to a monitor with a different DPI lays the strip out at the new scale straight away, not at the next refresh.

**Checked.** Under Wine: 246 tests pass. The one failure is the known
`ui::tests::a_theme_switch_takes_the_new_themes_face` (Wine has no Consolas). The Linux harness
(`scripts/check-state-linux.sh`) passes 107. Clippy is clean apart from the expected `windres not found`
line. The theme stills are 36 of 36 identical to the baseline, so Classic has not drifted. The native Windows CI
run is green.

**Not verified.** Nothing was clicked on a Windows desktop. Tests ran under Wine and on the native Windows
CI runner, and the window was reviewed from rendered pictures and from the code. The pictures were drawn under
Wine with substitute fonts, so text widths differ from a real Windows machine. Not checked by hand: the menus,
the dialogs, hotkey registration, balloons with Focus Assist on, the chart hover with a real mouse, and
keyboard focus after a click or the hotkey. CNBC range shapes are not verified live: the time spelling, whether
a bar's stamp is its start or its end, the time of day of daily bars, and whether each file carries volume come
from a single measurement. What CNBC returns for an unknown symbol is assumed. `--probe` is the check: run it on
a machine with network access before trusting the ranges or *Add symbol*.

## 2026-11-04 — 1.7.1: closing the chart glides back to the middle   [claude.impl]

**Files:** `src/ui.rs` (`grown_rect`, `rect_after_chart`, `grow_target`, `shrink_target`, `set_chart`),
`README.md`, `STATE-MACHINES.md`, `Cargo.toml`/`Cargo.lock`.

**What.** 1.7.0 gave the chart's width back on close, but kept the window's left edge. Opening the chart
had anchored the left edge and slid the window left only when it would overflow the screen (a 760 px
panel at x = 580 on a 1,920 px screen became 1,490 px at x = 430), so the compact panel came back
stuck left of where it opened, and both moves snapped.

- **Opening centres the wider frame on the old one**, clamped to the work area, so the panel opens out
  in both directions.
- **Closing returns to the exact frame it grew from**, carried by however far the user moved the
  window in between (a panel dragged to another corner shrinks where it is). A window the user has
  resized is still left alone.
- **Both directions glide** (210 ms, the strip <-> panel morph: a picture of the old state and one of the
  new, stretched between the frames). It is a picture, not the live window, because the panel lays out
  from its width and animating the window itself would reflow the card grid every frame. Opening was
  animated too, which was not asked for: a glide on close and a snap on open would feel like a bug.
- A chart toggled from the tray menu while the strip is showing has no window to move, but it now
  rewrites the frame the panel will reopen at (see the state-machine pass below).

**State-machine pass (same release).** Re-audited the chart/lock/mode transitions (`STATE-MACHINES.md`,
"Re-audit 1.7.1") and fixed five that were wrong in 1.7.0: a wide frame stranded in `last_panel` after
the chart was closed from the tray while collapsed; the chart-wide frame saved to the config when the
window was merely moved (so the next launch opened wide with no chart); a lock on a removed symbol that
re-lit when it came back; a morph that could skip moving the window and leak a bitmap if a picture
failed; and a give-back record that outlived a DPI change. The first two are one rule, now one function:
a frame that outlives the chart is the compact one (`compact_frame`).

**Checked.** Under Wine (Haiku subagents): 100 tests pass; the one failure is the known
`a_theme_switch_takes_the_new_themes_face` (Wine has no Consolas). Clippy is clean and both binaries
link in release. 2 tests replace the old width-only one: the centred growth (including the edge clamp and a screen narrower than the want) and
the give-back (untouched, dragged, resized). **Not checked: the animation itself.** It needs a live
window on Windows; what is covered is the geometry it animates between.

## 2026-11-04 — 1.7.0: lock a card, settings in the panel, a chart that reads   [claude.impl]

**Files:** `src/panel.rs` (selection border, Open button, cog, the candle pane rewritten, clock-aligned
`fold`), `src/ui.rs` (the lock, `set_chart` / width give-back, settings menu anchor, hover state),
`src/shot.rs` + `examples/chart.rs` (the fixture shows the locked state), `README.md`,
`STATE-MACHINES.md`, `Cargo.toml`/`Cargo.lock` (1.7.0).

**What.** Four complaints about the Ultra panel, one release.

- **A click locks a card; it no longer opens CNBC.** The card gets an accent outline and an
  *Open on CNBC* button in the corner where its source tag sits; clicking the same card again unlocks
  it. The candle pane follows the lock and **only** the lock: it used to re-target on hover, so it
  changed on the way to it. Opening the chart with nothing locked locks the first card, so the pane
  never opens on an instruction. Right-click *Open ... on CNBC* is unchanged. **The classic build is
  untouched** and still opens CNBC on click: it has no chart to lock.
- **Closing the chart gives the width back.** Opening it widens the window to fit both panes; closing
  it left the window wide with the cards stretched across the gap, and the only way back was dragging
  an edge. The widening is remembered and undone exactly, and only while the window is still the
  width the chart made it: a frame the user dragged is theirs. The picker closes the chart through the
  same path, so it hands the width back too.
- **Settings in the panel (Ultra).** A cog at the left end of the header buttons opens the same menu the
  tray icon does, dropped from under it. It sits on the far side of the chart toggle so the buttons that
  were already there keep their pixels. The chart and cog buttons also light on hover now.
- **The candle pane.** Bodies alternated 5 and 6 px in one row and a 1 px wick cannot centre on an even
  body, so candles looked uneven. The step is now a whole odd pixel count, every body is the same odd
  width, and the wick is the middle pixel. Gridlines sit on round prices (1/2/2.5/5 × 10ⁿ) instead of
  quarters of the day's range (`6,709.47 / 6,686.35 / 6,663.23` is now `6,725 / 6,700 / 6,675 / 6,650`),
  the time axis ticks on round UTC times instead of two labels at odd minutes, and a tag on the price
  axis marks where it last traded, with a dotted line under the candles. The axis is as wide as its
  widest label. `MIN_BAR` is 5 (odd) so a fitting candle can be centred.
- **Buckets are cut on the UTC clock.** `fold` bucketed from the first bar, so a day opening at 13:31
  had candles at 13:31, 13:36... and the new axis labels would have lied by up to a bucket. It now uses
  `t0 + dt`. This also narrows the "inaccurate time axis" note in `STATE-MACHINES.md`; the by-index
  spacing across a market closure remains.

**Why.** A ticker is read by pointing and travelling; a chart that moves while the pointer is on its way
to it, an action that throws a browser window at a stray click, a window that cannot shrink back, and a
settings menu that lives only in the tray are four ways the panel got in the way of that. The chart
fixes are arithmetic on whole pixels and round numbers, which is most of what makes a candle chart look
right.

**Checked** (under Wine against `x86_64-pc-windows-gnu`, by Haiku subagents; the author reviewed the
renders). 99 tests pass; the one failure, `a_theme_switch_takes_the_new_themes_face`, fails on the
unmodified HEAD too, because Wine has no Consolas. 8 tests are new (button row, Open-button targeting,
candle pixel parity over 4 dpis × 6 widths × 7 counts, round ticks, time ticks, clock-aligned buckets, the
lock toggle, the width give-back). Clippy is clean (HEAD was too). 32 chart renders (midnight, paper,
pc98, aurora, ega × 96/192 dpi × three locked cards, plus 1,130 and 1,920 px frames) exited 0 and were
inspected. **`shots/*.png` regenerate byte-identically: 36 of 36 SAME against HEAD.**
**Not checked:** a real Windows session. The window-level behaviour (click lock, width restore on a live
window, the menu dropping from the cog) is covered by pure-function tests and by reading the message
handler, not by driving a live window, and the fonts under Wine are not Windows' fonts.

## 2026-11-03 — 1.6.0: a window that resizes, and a candle pane that reads   [hermes.impl]

**Files:** `src/workspace.rs` (width-aware grid), `src/panel.rs` (`geometry_in`, `minimum`, the
candle pane and its bucket ladder), `src/ui.rs` (resize edges, minimum/maximum track sizes, panel
persistence, the chart toggle and its pointer tracking), `src/config.rs` (`panel`), `src/model.rs`
(`Bar`, the board's bars), `src/source.rs` (the day's OHLC kept, a longer timeout for it),
`src/shot.rs` + `examples/chart.rs` (the pane rendered from a fixture), `README.md`, `Cargo.toml`.

**What.** Two PRs, one release: the panel stops being one fixed size, and the space it gains is used
by a chart. `#6` made the window resizable; this one puts candles in it.

- **The window resizes like a window.** `WS_THICKFRAME | WS_MAXIMIZEBOX` with `WM_NCCALCSIZE`
  returning 0, so the frame costs no border and no client-area offset, and `WM_NCHITTEST` returns
  the eight edge codes on the panel only — the strip stays a drag handle. The floor is the content
  size, the ceiling the monitor's work area.
- **Width is columns, never zoom.** A dragged edge reflows the card grid (1–6 columns) and the
  painter lays out from the width it is asked to draw in. Cards stay `236×150` and type never
  shrinks; `shots/*.png` are still byte-identical, which is the proof.
- **The panel remembers its frame.** `panel: [x, y, w, h]` in the config, fitted to the work area of
  the monitor it lands on, so an unplugged display or a resolution drop cannot strand it.
- **Candles from data that was already arriving.** `1D.json` has always sent `open/high/low/close`
  per minute; `parse_chart` used to keep the last hour and the close only. It now keeps the day
  (~1,450 bars) and the sparkline is a slice of the same series, so the two can never disagree.
  No extra request, no extra byte.
- **A candle has to be readable to be a candle.** Below four pixels a body is a scratch between two
  wicks, so the pane folds the day into 5-, 15-, 30- or 60-minute buckets until each candle has its
  pixels: 5 m at 96 dpi on a 720 px pane, 1 m when there is room. Buckets are cut on the clock, so a
  break in the session stays a gap. The header names the bucket and the zone (`S&P · 5 m UTC`).
- **The pane follows the pointer**, and the toggle raises the window's minimum to two whole panes, so
  opening the chart never folds the card grid. The picker and the chart take turns: both want the
  space under the cards.
- **The chart request gets its own patience.** A day of bars is ~220 KB; the 10 s that serves a
  quote can truncate it on a slow link, and a truncated body parses as a JSON error at 128 KB.

**Why.** A ticker that only shows six cards has nothing to put in a bigger window. The pane model
from `#5` gave the geometry a shape; this is the first pane that is worth the width, and it is built
on the feed that already ran — the only new machinery is a bucket ladder and a fold.

**Checked.** 59 tests pass under Wine against `x86_64-pc-windows-gnu` (grid sizing, the pane split,
the window floor, the bucket ladder, clock-cut folding, the parser's OHLC). `shots/*.png` regenerate
byte-identically (`diff -rq` empty). The pane was inspected at 96 and 192 dpi in midnight, paper and
pc98, and driven live: toggle opens it, the window widens to 1,490, hovering another card recharts,
and the empty state names its symbol instead of drawing a bare rectangle. Live bars are slow under
Wine's TLS (two of six symbols answered in the probe run); the fixture path is what the images use.

## 2026-11-02 — 1.5.0: every palette moves, and a face the machine lacks is greyed   [hermes.impl]

**Files:** `src/theme.rs` (motion as a field, all 26 palettes rewritten, 22 faces, the availability
query), `src/ui.rs` (per-palette cycle, `allow_motion`, face fallback, greyed Font entries),
`src/shot.rs` (three frames for the animated tier only), `Cargo.toml` (1.5.0), `THEMES.md`,
`README.md`, `FEATURES.md`, `ARCHITECTURE.md`, `shots/` (generated).

**What.** Maria asked for animated versions of all 26 palettes plus font alternatives, approved the
proposal, and asked for it as a branch and a PR.

- **Motion is a field, not a tier.** `Theme` carries `cycle: Option<f64>`; `animated()` is now
  `cycle.is_some()`. `Kind` stays the rendering rule, so a palette keeps its surface and still
  moves. Three modes, none of them new machinery: a flat palette **breathes** (48-step ring of the
  three stops, drawn as one flat fill), a ramp becomes a **96-band ring**, retro keeps its **16
  chunky bands** and rolls its scanlines a cell per cycle.
- **Every palette got a real three-stop cycle and its own pace** (6–22 s), and `bg_mid` is no longer
  a copy of `bg_to` — that duplicate is why a ring on a gradient used to have a dead segment.
  Values are in `THEMES.md`.
- **Faces: 13 → 22** — Segoe UI Variable, Bahnschrift, Candara, Corbel, Franklin Gothic Medium,
  Sitka Text, Cambria, Cascadia Code, Lucida Console — one proposed face per palette.
- **A missing face is refused instead of substituted.** `face_available` enumerates the machine's
  families once (`EnumFontFamiliesExW`) and the Font menu greys what it cannot draw, because GDI's
  silent substitution looks like a theme bug. An empty enumeration is read as "unknown" and greys
  nothing.
- **Reduced motion is honoured.** `SPI_GETCLIENTAREAANIMATION` off holds every cycle at phase 0.
- **The classic build is untouched**: it never runs a clock, so midnight still draws `rgb(18,20,24)`
  as one flat fill, byte for byte.

**Why.** A palette that can only move if it is filed under "Animated" is a catalogue arranged around
an implementation detail. Motion belongs to the palette; the surface decides what moving looks like.
The 22nd face only makes the substitution problem worse, so the availability query ships with it.

**Checked.** 40 tests pass under Wine against `x86_64-pc-windows-gnu` (palette stops, cycle range,
brush counts per mode, tier contiguity, pixel faces fixed-pitch, font resolution). `shots/`
regenerated from `panel::draw` at this commit. Motion itself is not unit-testable — the frames were
inspected as GIFs before the patch landed.

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
