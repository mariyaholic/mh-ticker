# 1.8 work packages

*The build plan for the BUILD NOW items in [02-feature-list.md](02-feature-list.md). Six packages in
two waves of three. Each package is built by one **builder** agent (the skeleton and every risky edit) and
finished by one **finisher** agent (bodies, tests, verification), then reviewed by the orchestrator
until it is accepted and merged.*

| Wave | Package | Branch | Worktree | Items |
|---|---|---|---|---|
| 1 | P1 Alerts you can set and silence | `wip/p1-alerts` | `/home/user/mh-wt/a` | A1 to A9 |
| 1 | P2 A chart that reads | `wip/p2-chart` | `/home/user/mh-wt/b` | B1 to B5, F2 |
| 1 | P3 Everyday | `wip/p3-everyday` | `/home/user/mh-wt/c` | C1 to C7 |
| 2 | P4 Chart ranges and volume | `wip/p4-ranges` | `/home/user/mh-wt/a` | B6, B7, B8 |
| 2 | P5 Keyboard, cards and order | `wip/p5-keys` | `/home/user/mh-wt/b` | D1 to D4 |
| 2 | P6 Type it in | `wip/p6-prompt` | `/home/user/mh-wt/c` | F1, E1, E2, E5, A10, A11 |

## Rules for every package

### Where you work

- Your worktree and branch are in the table. **Edit only inside your worktree.** Other agents are
  working in the other two at the same time.
- Build and test with the wrapper (it targets `x86_64-pc-windows-gnu` and runs tests under Wine):

  ```bash
  /home/user/mh-wt/wcargo check --all-features --all-targets
  /home/user/mh-wt/wcargo test --all-features
  /home/user/mh-wt/wcargo clippy --all-features --all-targets
  bash scripts/check-state-linux.sh
  /home/user/mh-wt/stills-check
  ```

- **One test fails under Wine and is not yours to fix:**
  `ui::tests::a_theme_switch_takes_the_new_themes_face` (Wine has no Consolas). Every other test must
  pass. Clippy must print no warnings.
- `stills-check` renders the theme catalogue and compares it with the baseline byte for byte. It must
  report every still identical. If a still changes, you changed what Classic or an existing Ultra
  panel draws: find it and undo it. New `panel::Scene` or `panel::Chart` fields get a value in
  `src/shot.rs` that reproduces today's pixels.
- **There is no network path to CNBC from here.** Do not try live requests. Parsers are tested with
  JSON written by hand in the shape the existing tests use.

### What you must not touch

`README.md`, `CHANGELOG.md`, `STATE-MACHINES.md`, `FEATURES.md`, `ARCHITECTURE.md`, `THEMES.md`,
`SOURCES.md`, `shots/`, `scripts/`, `.github/`, `Cargo.toml`, `Cargo.lock`. No new dependencies. Write
what a user and a reviewer need to know in `docs/1.8/notes/<package>.md` instead (what was built, how
to use it, every decision you made that the brief did not make for you, what is unverified).

### Code rules

1. **Classic must not drift.** Anything not marked "both builds" is behind `u.profile.ultra`. A
   "both builds" feature defaults to exactly what the app does today.
2. **Logic goes in your own module, pure.** No `HWND`, no `HDC`, no clock: `now` is a parameter. Unit
   tests live in the same file. `ui.rs` gets wiring only.
3. **`with_ui` does not nest.** It uses `try_borrow_mut`: calling anything that calls `with_ui` from
   inside a `with_ui` closure silently does nothing. Never open a menu, message box or dialog, and
   never do I/O, inside a `with_ui` closure.
4. **Nothing blocks the UI thread.** Network work runs on its own thread and reports back with
   `PostMessageW` (the `Ui` is thread-local). A second request while one is in flight is refused or
   replaces it by an explicit rule, never by accident.
5. **Every new `Ui` field** is initialised in `run`, and the brief lists the transitions that reset
   it. A field with no reset path is a bug.
6. **Config**: new fields default to today's behaviour, and `{}` must still parse. A new field that
   holds a secret is never displayed.
7. **Painter and hit test read the same geometry**, from one function.
8. Do **not** extend the big tuple destructured at the top of `menu_at`; read extra state with a
   separate `with_ui` call, the way `fallback` is read.
9. Per-symbol menu entries go through `menu_commands::Action`. Fixed entries use your reserved ID
   range. Use only your reserved window messages and timers.
10. No `unwrap`/`expect` on anything that comes from the network, the registry, the clipboard or the
    config.

### Reserved numbers and anchors

Insert at your anchor and nowhere near another package's. This is what lets three branches merge.

| | P1 | P2 | P3 | P4 | P5 | P6 |
|---|---|---|---|---|---|---|
| Menu IDs | 5200 to 5299 | 5300 to 5349 | 5400 to 5499 | 5500 to 5549 | 5900 to 5999 | 6000 to 6099 |
| Window messages | `WM_APP + 6`, `+ 7` | `+ 8` | `+ 9`, `+ 10` | `+ 11`, `+ 12` | `+ 13` | `+ 14`, `+ 15` |
| Timer IDs | 4 | 5 | 6 | 7 | 8 | 9 |
| Your constants in `ui.rs` | right after `ALERT_STEPS` | right after `CHART_MS` | right after `TRAY_DEBOUNCE` | right after `CHART_Y_BAND` | right after `ANIM_MS` | right after `OPACITY_STEPS` |
| Your `Ui` fields (struct and the initialiser in `run`) | after `alert_note` | after `open_hover` | after `tray_click_at` | after `chart_layout` | after `wheel_remainder` | after `update_result` |
| Your `Config` fields (struct and `Default`) | after `webhook` | after `panel` | after `topmost` | none | none | none |
| A config test, if you need one | after `fallback_policy_is_off_by_default_and_persists_explicit_consent` | after `malformed_settings_are_not_accepted_as_defaults` | after `placement_falls_back_to_the_old_dock_flag` | none | after `hand_added_rows_stay_listed_and_in_place` | after `toggle_keeps_catalog_order_and_limits` |
| `mod` lines in `lib.rs` | none | after `pub(crate) mod alerts;` | after `pub(crate) mod webhook_host;` | after `pub(crate) mod panel;` | after `pub(crate) mod dock;` | after `pub(crate) mod update;` |
| `panel::Chart` fields (struct, both literals in `ui::draw_panel`, the fixture in `shot.rs`) | | | | after `unavailable` | none | after `change_pct` |
| `menu_commands::Action` variants, and their arms in `menu_at` | | | | none | after `Symbol(String)` | after `ClearAlert(String)` |
| New `wndproc` arms | | | | right after the `WM_ALERT` arm | right before `WM_KEYDOWN if wp == VK_ESCAPE` | right before `WM_UPDATE =>` |
| `config.rs` functions | | | | none | after `toggle` | after `menu_list` |
| `source.rs` functions | | | | after `Client::chart` | none | right after `get_release` |

Tests for a helper you add to `ui.rs` go in a small `#[cfg(test)] mod` directly under that helper, not
in the big `mod tests` at the bottom of the file.

### The hand-off

**The builder writes the skeleton and makes every decision.** That means: the types, the function
signatures with doc comments that state the contract, the state fields, **all** of the Win32 and
threading code (`wndproc`, `menu_at`, `run`, `unsafe` blocks), and the test names with their key
assertions. When the builder stops:

- `wcargo check --all-features --all-targets` is clean and the existing tests still pass;
- every unfinished piece is marked in the code as `// TODO(finish): <what> (accept: <how to tell it is done>)`;
- `docs/1.8/notes/<package>-handoff.md` lists the TODOs in the order to do them, with anything the
  finisher must not change;
- the work is committed on the branch.

**The finisher completes it.** Every `TODO(finish)`: the bodies of the pure functions, the unit tests (the ones
named, plus edge cases: empty, zero, NaN, one element, the boundary itself), labels, clippy. Then the
five commands above, the notes file, and a commit. The finisher does not redesign: if a contract looks wrong,
it says so in the notes and in its final message instead of changing it.

**Final message, both agents:** under 200 words. Branch, commit hash, what is done, what is not, the
test count, anything you are unsure about.

---

## P1. Alerts you can set and silence (Ultra)

Owns `src/alerts.rs`. Shared: `src/config.rs`, `src/menu_commands.rs`, `src/ui.rs`.

**Config** (after `webhook`): `alert_once: bool` (false), `alerts_muted: bool` (false),
`alerts_mute_until: f64` (0.0, UTC seconds), `quiet_hours: Option<[u16; 2]>` (None; minutes of the
local day, `[from, to)`, wrapping midnight when `from > to`). `Alert` gains `day_move: Option<f64>`
(absolute percent) and `once: bool`, both defaulted so old configs parse and behave as before.

**Pure, in `alerts.rs`:**

- `PRESETS`: replaces `ui::ALERT_STEPS`. Percent of the current price (±0.5, 1, 2, 5), today's high,
  today's low, previous close, and day move ±1, 2, 3, 5 %. `preset_level(preset, cell, reference)`
  returns the side and level, or `None` when the field is missing, zero or not finite (CNBC sends
  `0.00` for symbols with no high or low). The reference price still comes from `reference_price`.
- `check` learns day-move alerts (from `Quote.change_pct`, fresh quotes only, same cooldown) and
  `once` (the level that fired is cleared; an alert left with nothing is removed). `Fired` gains the
  label and the time.
- `quiet(local_minute, window) -> bool` and `silence(cfg, now_utc, local_minute) -> Option<Silence>`
  (`Muted`, `MutedUntil(t)`, `Quiet`). A timed mute ends by the clock, with no timer.
- A silenced alert is still evaluated: it burns its cooldown and is logged as silenced, so lifting the
  mute does not release a burst.
- `Log`: the last 10 `Entry { at, title, outcome }`, in memory only. `Outcome`: `Shown`, `Muted`,
  `Quiet`, `Sent`, `SendFailed`.
- `describe(alert, digits) -> String` for the menu ("above 4,200.00 · day ±2 %").
- `accept_webhook(clipboard_text) -> Result<String, &'static str>`: trimmed, `http`/`https` only (use
  `webhook_host::redact`), no whitespace or control characters, at most 2,048 bytes. Its error never
  contains the input.

**Wiring, in `ui.rs`:**

- The Alerts submenu is rebuilt (the block that starts at the comment "Alerts at a move from the
  current price" is yours): per symbol the presets, a greyed line describing what is set, Clear; then
  *Fire once, then clear*, *Mute* (1 hour, 8 hours, until I turn it back on, Unmute; the label says
  "Muted until 14:30" when it is), *Quiet hours* (Off, 22:00 to 07:00, 23:00 to 08:00, 00:00 to
  09:00), *Webhook* (the host greyed, Paste URL from clipboard, Send a test alert, Remove), *Recent*
  (the log, newest first, and Clear list).
- `on_feed`: after `check`, ask `silence`. Silenced: log it, no balloon, no POST. Otherwise balloon,
  POST, log. Either way set `alert_pending`.
- `alert_pending`: the tray icon carries a marker and the tooltip's first line names the alert, until
  the panel is expanded or any menu is opened. The icon cache key (`icon_link`) must include it.
- The webhook result message updates the log entry's outcome as well as the footer note.
- Local minute of day: `GetLocalTime` (`Win32::System::SystemInformation`, already enabled).
- Paste: read `CF_UNICODETEXT` on the click, validate, confirm with a message box that shows the
  **host only**, then save.

**Accept:** tests for the quiet window (inside, outside, wrapping, `from == to` is off), timed and
indefinite mute, a silenced alert burning its cooldown and logging, `once` clearing and pruning,
day-move firing only on a fresh quote and on both signs, presets refusing missing and zero fields, the
log capping at 10 and keeping order, `accept_webhook` refusing bad input without echoing it, and old
configs (`{}` and an alert without the new fields) parsing to today's behaviour.

## P2. A chart that reads (Ultra)

Owns new `src/chartview.rs`. Shared: `src/panel.rs` (`candles`, `Chart`, `Scene`), `src/ui.rs`,
`src/config.rs`, `src/shot.rs`, `examples/chart.rs`.

**`chartview` (pure):** one `Layout` for the candle pane, computed once per paint and used by the
painter for everything it draws in the plot, and by the pointer code. It holds the plot rectangle, the
candle step, body and wick, the padded price window, the bar count, and the header hit rectangles. It
answers `x_of(i)`, `index_at(x)`, `y_of(price)`, `price_at(y)`. `candle_metrics`, `nice_ticks`,
`tick_interval`, `fitted`, `fold` and `clock` move here from `panel.rs` with their tests. Also
`gaps(bars, bucket_s) -> Vec<usize>` (indices where the session broke: the next bar is more than 1.5
buckets later).

The painter measures text, so the layout cannot be recomputed without a device context. The painter
therefore **stores the layout it drew with** (`Scene` carries a `&Cell<Option<Layout>>`, owned by the
`Ui`), and `WM_MOUSEMOVE` reads that. The pointer can never disagree with the pixels, because it is
answered from the last frame painted. The cell is cleared when the pane is not drawn.

**What the pane gains:**

- **Crosshair and readout.** `Ui` keeps `chart_at: Option<Pointer { bar, y }>` (y quantised to 2 px
  so a mouse move does not repaint per pixel). The painter draws a vertical line through that candle,
  a horizontal line at `y` with a price tag on the axis, and a legend line in the header: time, O, H,
  L, C and the candle's change. Cleared on mouse leave, when the chart closes, when the lock moves and
  when the rows change.
- **Local time.** `Chart` gains `tz_offset_s: i32` and the header names the zone ("local" or "UTC").
  Buckets are cut on local boundaries (`fold` and `fitted` take the offset; 0 reproduces today's
  output and the existing tests keep passing untouched). The zone name in the header is a button:
  clicking it flips `cfg.chart_utc` (default false, so local) and saves. The offset comes from
  `GetTimeZoneInformation` (`Win32::System::Time`, already enabled) in one small `ui.rs` function.
- **Reference lines.** `Chart` gains `prev`, `day_hi`, `day_lo` (from the quote; `None` when absent or
  zero). Previous close is a dashed line with a small "prev" label; day high and low are short ticks
  on the axis. Drawn only when inside the price window.
- **Session breaks.** A faint dotted vertical line at each `gaps` index.
- **Header.** The day change beside the last price, in the up or down colour.

`examples/chart.rs` and `shot::write_chart_named_at` take an optional hovered bar so the readout can be
reviewed from a PNG.

**Accept:** tests for the layout round trips (`index_at(x_of(i)) == i`; `price_at(y_of(p))` within one
pixel of price; outside the plot is `None`), `gaps`, folding with a +05:30 and a -03:30 offset landing
on local hours, the clock with an offset, and that offset 0 leaves every existing candle test
unchanged. `stills-check` identical. A rendered chart PNG with a hovered bar, for the reviewer.

## P3. Everyday (both builds)

Owns new `src/autostart.rs`, `src/hotkey.rs`, `src/update.rs`. Shared: `src/ui.rs`, `src/config.rs`,
`src/model.rs`, `src/source.rs`, `src/lib.rs`.

- **Start with Windows.** `autostart::command(exe)` (quoted), `enabled(id)`, `set(id, on)`, on
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value name = the profile id. The registry is
  the source of truth: the menu reads it each time it opens. A stored command that is not this exe
  counts as off. (`Win32::System::Registry` is already enabled.) The registry code is
  `#[cfg(windows)]`; `command` is pure.
- **Hotkey.** `hotkey::parse("Ctrl+Alt+M") -> Option<Combo>`, `label(&Combo)`. `cfg.hotkey:
  Option<String>` (None). A *Hotkey* submenu: Off, four presets, plus the configured one if it is
  none of them. `RegisterHotKey` with `MOD_NOREPEAT`; a failed registration is remembered and the
  menu says "(in use by another app)". `WM_HOTKEY` toggles expand and collapse. Unregistered on
  change and on destroy.
- **Refresh now.** `Board.kick: u64`. `Feed::wait` returns early when it changes; the chart thread
  makes rows with no bars due at once. One menu entry.
- **Strip submenu.** Rows in the taskbar (1, 2, 3), text size (four steps, setting `dock_font_pt` and
  `font_pt` together), opacity (five steps). Applied live. Check how `opacity` is used by the morph
  before changing it at run time.
- **Version and update check.** A greyed "MH Ticker 1.8.0" line (`CARGO_PKG_VERSION`) and *Check for
  updates*. On click only: one GET to `https://api.github.com/repos/mariyaholic/mh-ticker/releases/latest`
  on a worker thread, body capped, no redirects. `update::latest(json)` and
  `update::newer(current, tag)` are pure. The result is a message box; "open the download page" opens
  the **constant** releases URL, never a URL from the response. A second click while one is running
  is refused (the entry reads "Checking...").
- **Copy all prices.** `model::board_text(cells)`: one line per symbol, label, price, change, tab
  separated.
- **Save failures.** `config::save` records whether the last save failed; the menu's status line says
  so while it is true.

**Accept:** tests for hotkey parse and label (round trip, case, unknown key, no modifier refused),
`autostart::command` quoting and a registry round trip under a throwaway value name, version
comparison (`v` prefix, unequal lengths, a pre-release suffix, garbage is `None`), `latest` on a
hand-written release JSON and on junk, `board_text`, the save-failed flag setting and clearing, and
`wait` returning early on a kick.

---

## Wave 2

Starts from the merged wave 1 (171 tests pass under Wine plus the one known failure; the Linux harness
runs 65; the stills baseline is unchanged). The anchors are in the table above. Three things are new
since wave 1 and every wave 2 package builds on them:

- `src/chartview.rs` owns the candle pane's arithmetic: `Layout` (plot, step, body, wick, price window,
  `x_of`, `index_at`, `y_of`, `price_at`, `pointer_at`), `fitted`, `fold`, `gaps`, `clock`, `readout`.
  It is pure and portable (`crate::model::Bar` and `std` only) and must stay that way.
- `panel::candles` draws from one `Layout` and stores it in `Scene::layout`; `Ui::chart_layout` is that
  cell, and the pointer is answered from it.
- `src/alerts.rs` has `PRESETS`, `preset_level`, `apply_level`, `Level`, `reference_price`, `silence`,
  `Log`. `ui::alerts_view` and `ui::alert_command` are the menu's read and write sides.

Review pictures: P4 and P6 render their own with `examples/chart.rs` and commit them as
`docs/1.8/notes/p4-*.png` and `p6-*.png`. The default fixture (no extra arguments) must keep rendering
exactly what it renders now.

## P4. Chart ranges and volume (Ultra)

Owns new `src/ranges.rs`. Shared: `src/model.rs` (`Bar`), `src/source.rs`, `src/chartview.rs`,
`src/panel.rs`, `src/ui.rs`, `src/shot.rs`, `examples/chart.rs`, `src/lib.rs`.

**Unverified live by construction.** Nothing here can be tried against CNBC from this sandbox. The
shapes come from `FEATURES.md` §3 and Appendix A, which measured one symbol on one day. So the code
assumes as little as it can: it trims by time instead of trusting a file's name, infers the bar
duration from the data, accepts either spelling of the time field, and shows "no data for this
range" instead of drawing something wrong.

**`ranges` (pure, portable: `crate::model::Bar` and `std` only):**

- `Range`: `D1`, `D5`, `M1`, `M6`, `Y1`, `Y5`, with `label()` ("1D" ... "5Y"), `file()` (`None` for
  `D1`, which uses the day of bars the board already holds; `"5D.json"`; `"1M.json"` for both `M1`
  and `M6`, because that file carries about a year of daily bars; `"1Y.json"`; `"5Y.json"`),
  `span_s()` (5, 31, 183, 366 and 5 x 366 days) and `ttl_s()` (10 minutes for `D5`, an hour for the
  daily files, six hours for `5Y`).
- `Key { symbol, file }`: the cache is keyed by the **file**, so `M1` and `M6` share one fetch.
- `Series { t0, bars: Arc<[Bar]>, bar_s, fetched }`. `bar_s` is the median spacing of consecutive
  bars, inferred, never assumed. `Series::window(now, span_s)` is the slice inside the range's span.
- `Book`: the state machine. It holds the cache (at most 12 series; the least recently used goes
  first), the one request in flight with its generation, and the last failure per key with its time.
  - `want(key, now) -> Option<Request>`: a fetch is due when the key is not cached or its time to
    live has passed, nothing is in flight for it, and it did not fail in the last 30 s.
  - `arrived(generation, key, result, now) -> bool`: accepted only when the generation is the one in
    flight. A late answer for a key the user has left is still cached (it is good data); an answer
    with a stale generation is dropped.
  - `status(key, now)`: `Ready`, `Loading`, `Failed`, `Empty`.
  - `forget(symbols_still_shown)`: called when the rows change.
  The cache outlives the pane (the RAM ceiling was doubled on 2026-11-13).

**Fetching (`source.rs`):** `Client::chart_file(symbol, file)` and `parse_series(json, now)`: the bar's
time is `tradeTimeinMills` or `tradeTimeInMills`, as given (the start of the bar), close required,
open/high/low falling back to the close, volume optional, newest 4,000 bars kept. `parse_chart` (the
1D path) is unchanged apart from reading volume. One long-lived `ranges` worker thread, started in
`spawn`, polls a shared request slot (the way the other workers poll, every `ROWS_CHECK`), fetches,
stores the result in a shared slot and notifies the window with `WM_APP + 11`. No thread per click. A
new request replaces one that has not been picked up yet; one that is already being fetched finishes
and is cached.

**`Bar` gains `v: f32`** (0 when the feed sends none; `Bar` derives `Default`). Every literal in the
tests gets the field. 24 bytes a bar.

**The pane:**

- Range tabs on the header's second line, right-aligned: `1D 5D 1M 6M 1Y 5Y`, the current one lit.
  Their rectangles are in the `Layout` (it stays `Copy`), and a click is answered from the stored
  layout like the zone button. The hover legend shares that line and falls back to its compact form
  when the two would touch.
- `Ui::range` (session only, like the chart flag; starts at `D1`). Changing the range or the locked
  card calls `Book::want`. `on_feed` calls it too, which is the retry and refresh tick: no new timer.
- While a series is loading or failed the pane keeps its header and tabs and says so in the plot:
  "loading 5D", "could not load 5D, retrying", "no data for this range". The way out of every one of
  those is a tab that still works.
- Buckets for long ranges: `fitted` starts at the series' own `bar_s` (never finer than the data) and
  climbs 1, 5, 15, 30, 60 minutes, 4 hours, a day, a week, a calendar month. Days are cut on local
  midnight, weeks on Monday, months on the calendar (a small pure civil-date function in
  `chartview`). The header names the bucket ("1 d", "1 w", "1 mo").
- Axis: intraday ranges tick on clock times as today and label a day boundary with the date
  ("Nov 12"); daily and longer ranges tick on month starts ("Oct") or year starts ("2025"), whichever
  leaves room for the labels. The hover legend shows the date, and the time only for intraday bars.
- Session-break lines are drawn only for buckets shorter than a day (every weekend is not a break).
- Volume: when any displayed candle has volume, the bottom fifth of the plot is a volume strip
  (bars in the candle's colour) and the price plot is that much shorter. No volume, no strip, and the
  pane is exactly as it is today.
- The reference lines (previous close, day high and low) are drawn only on `D1`.

`--probe` prints, for the first row, each range file's bar count, inferred bar duration, span in
days and whether it carried volume. That is the one-command live check for the owner.

`examples/chart.rs` takes a range argument with a synthetic series so each range can be rendered.

**Accept:** tests for `Book` (a full select, arrive, expire, refresh cycle; a stale generation
dropped; a late answer for a left key cached; failure back-off; the 13th series evicting the least
recently used; `forget`), `Series::window` and `bar_s` on minute, daily and weekly spacing,
`parse_series` on both time spellings, missing fields, an empty array and junk, the long-range bucket
ladder (never finer than the data; a day, a week on Monday, a month on the calendar, with a non-zero
zone offset), the date labels, the volume strip geometry (`y_of` and `price_at` still round-trip with
a strip), and that a series with no volume produces the same `Layout` as today. Pictures of 5D, 1M
and 5Y, and of 1D with volume. `stills-check` identical.

## P5. Keyboard, cards and order

Owns new `src/keys.rs`. Shared: `src/ui.rs`, `src/config.rs`, `src/menu_commands.rs`, `src/panel.rs`
(the footer hint only), `src/lib.rs`.

**`keys` (pure, portable: `std` only):** `map(vk, ctrl, state) -> Option<Action>` with
`state = { ultra, has_lock, picker_open, chart_open }`, and `step(index, n, cols, dir) -> usize` for
moving across the card grid (left and right stop at the ends, up and down stay put when there is no
card there).

| Key | Does | Build |
|---|---|---|
| Arrows | move the lock; with nothing locked, lock the first card | Ultra |
| Enter | open the locked card on CNBC | Ultra |
| C | open or close the chart | Ultra |
| S | open or close the symbol picker | Ultra |
| Ctrl+C | copy the locked card's line; with nothing locked, copy all prices | both |
| Delete | remove the locked card from the strip (never the last one) | Ultra |
| Ctrl+Left, Ctrl+Right | move the locked card one place | Ultra |
| Esc, PgUp, PgDn, Home, End | unchanged | both |

A moved lock is scrolled into view when the body scrolls. Moving the lock clears the chart pointer
exactly as a click on a card does.

**Order (`config.rs`, pure):** `move_row(cfg, symbol, delta) -> bool` and `move_first(cfg, symbol)`.
Order is a permutation of the **catalogue**: a row that is shown but not in the catalogue is appended
to it first (which changes nothing visible), then the symbol swaps places with its shown neighbour,
then `rows` is rebuilt the way `toggle` builds it. "Rows keep catalogue order" stays true, so adding a
symbol later does not undo the order.

**Card menu** (the block in `menu_at` that starts `if let Some((i, label, price)) = &card` is yours):
*Remove from strip* (greyed for the last one), *Move left*, *Move right*, *Move first*, as
`menu_commands::Action` entries that carry the symbol, so they act on the card that was clicked even
if the rows change while the menu is open.

**Strip click (Ultra):** the strip's layout records each symbol's rectangle (`Frame`); a click on a
symbol in a docked or cornered strip opens the panel with that card locked. A click between symbols,
or on the classic strip, does what it does today. The floating strip is a drag handle and is
unchanged.

**Footer hint** for Ultra names the keys. The classic hint and the stills do not change.

**Accept:** tests for `map` (every row of the table, for both builds, with and without a lock; an
unmapped key is `None`), `step` on full and ragged grids of 1, 2, 3 and 6 columns, `move_row` and
`move_first` (ends, a hand-added row, the order surviving a later `toggle`, the existing config tests
untouched), and the strip hit test. `stills-check` identical.

## P6. Type it in

Owns new `src/prompt.rs`. Shared: `src/ui.rs`, `src/config.rs`, `src/source.rs`, `src/alerts.rs`,
`src/menu_commands.rs`, `src/panel.rs`, `src/shot.rs`, `examples/chart.rs`, `src/lib.rs`.

**`prompt`:** `ask(owner, title, label, initial, max_len) -> Option<String>`, a modal one-line prompt
built from an in-memory dialog template (`DialogBoxIndirectParamW`): a label, an edit box, OK and
Cancel; Enter accepts, Esc cancels; the text is selected when it opens. `template(...)` builds the
bytes and is pure. Never called from inside `with_ui`. A test builds the real dialog, has it accept
itself from `WM_INITDIALOG` (a test-only switch), and checks the text comes back, so native CI proves
the template is one Windows will take.

**Add a symbol (both builds):** *Symbols > Add symbol...* asks for a CNBC symbol.
`config::clean_symbol` trims and upper-cases it and allows `A-Z 0-9 . @ = - ^ / : _`, 1 to 20
characters. If it is already listed it is simply ticked. Otherwise it is looked up on a worker thread
(`source::lookup`, one quote request) and the result comes back as `WM_APP + 14`. `Ui::adding` holds
the symbol in flight; the menu entry reads "Adding AAPL..." and is greyed; the handler clears it in
every outcome, a failed thread start included. Found: `config::add_custom` appends it to the catalogue
(label from the symbol, made unique; digits from the price's size; name from CNBC, cleaned and
shortened) and ticks it when there is room. Not found, or no network: a message box that says which.
*Symbols > Remove a symbol I added* lists the catalogue entries that are not in
`default_catalog()`; removing the last shown symbol is refused.

**`MAX_ROWS` goes from 12 to 24.** Check everything sized by it, and rewrite the config test that
assumes the catalogue is longer than the cap so it builds its own long catalogue.

**Alert at a price (Ultra):** each symbol's Alerts submenu gains *at a price...*. The prompt opens on
the current price. `alerts::parse_level` accepts `4200`, `4,200.50` and `4200.5` and refuses zero,
negatives, NaN, infinity and anything else. Above the fresh reference price it is an *above* level,
below it a *below* level, equal is refused with a reason. Greyed when there is no fresh price, like
the presets.

**Alert levels on the pane:** `panel::Chart` gains the locked symbol's levels. Inside the price
window each is a dashed line in the amber colour with its price at the right end of the plot; outside
it, a small marker on the top or bottom edge with the price, so a level off the screen is still
known. A right-click inside the plot (Ultra, pane open) opens a small menu of its own, before the
main menu: *Alert at <price under the pointer>* (from the stored layout's `price_at`, rounded to the
symbol's digits) and *Clear <symbol> alerts* when there are any. It is a separate function, not part
of `menu_at`.

`examples/chart.rs` takes an argument that adds alert levels to the fixture.

**Accept:** tests for the template (every item starts on a 4-byte boundary; the item count and ids),
the self-accepting dialog round trip, `clean_symbol`, the label and digits rules, `add_custom` and
removal (a duplicate label, a full strip, the last shown symbol), `parse_level`, the side rule, and
the in-window and off-window placement of alert levels (pure, in `chartview`). A picture with one
level inside the window and one above it. `stills-check` identical.
