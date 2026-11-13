# 1.8 work packages

*The build plan for the BUILD NOW items in [02-feature-list.md](02-feature-list.md). Six packages in
two waves of three. Each package is built by one Sonnet agent (the skeleton and every risky edit) and
finished by one Haiku agent (bodies, tests, verification), then reviewed by the orchestrator until it
is accepted and merged.*

| Wave | Package | Branch | Worktree | Items |
|---|---|---|---|---|
| 1 | P1 Alerts you can set and silence | `wip/p1-alerts` | `/home/user/mh-wt/a` | A1 to A9 |
| 1 | P2 A chart that reads | `wip/p2-chart` | `/home/user/mh-wt/b` | B1 to B5, F2 |
| 1 | P3 Everyday | `wip/p3-everyday` | `/home/user/mh-wt/c` | C1 to C7 |
| 2 | P4 Chart ranges | `wip/p4-ranges` | `/home/user/mh-wt/a` | B6, B7 |
| 2 | P5 Keyboard, cards and order | `wip/p5-keys` | `/home/user/mh-wt/b` | D1 to D4 |
| 2 | P6 Type it in | `wip/p6-prompt` | `/home/user/mh-wt/c` | F1, E1, E2, A10, A11 |

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
| Your constants in `ui.rs` | right after `ALERT_STEPS` | right after `CHART_MS` | right after `TRAY_DEBOUNCE` | assigned at wave 2 | | |
| Your `Ui` fields (struct and the initialiser in `run`) | after `alert_note` | after `open_hover` | after `tray_click_at` | | | |
| Your `Config` fields (struct and `Default`) | after `webhook` | after `panel` | after `topmost` | | | |
| A config test, if you need one | after `fallback_policy_is_off_by_default_and_persists_explicit_consent` | after `malformed_settings_are_not_accepted_as_defaults` | after `placement_falls_back_to_the_old_dock_flag` | | | |
| `mod` lines in `lib.rs` | none | after `pub(crate) mod alerts;` | after `pub(crate) mod webhook_host;` | | | |

Tests for a helper you add to `ui.rs` go in a small `#[cfg(test)] mod` directly under that helper, not
in the big `mod tests` at the bottom of the file.

### The hand-off

**Sonnet builds the skeleton and makes every decision.** That means: the types, the function
signatures with doc comments that state the contract, the state fields, **all** of the Win32 and
threading code (`wndproc`, `menu_at`, `run`, `unsafe` blocks), and the test names with their key
assertions. When Sonnet stops:

- `wcargo check --all-features --all-targets` is clean and the existing tests still pass;
- every unfinished piece is marked in the code as `// TODO(haiku): <what> (accept: <how to tell it is done>)`;
- `docs/1.8/notes/<package>-handoff.md` lists the TODOs in the order to do them, with anything Haiku
  must not change;
- the work is committed on the branch.

**Haiku finishes it.** Every `TODO(haiku)`: the bodies of the pure functions, the unit tests (the ones
named, plus edge cases: empty, zero, NaN, one element, the boundary itself), labels, clippy. Then the
five commands above, the notes file, and a commit. Haiku does not redesign: if a contract looks wrong,
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

## Wave 2 (outlines; the anchors are assigned after wave 1 merges)

**P4. Chart ranges (Ultra).** New `src/ranges.rs`: the range table (1D from the bars already held; 5D
from `5D.json`; 1M, 6M and 1Y from `1M.json`, which carries about a year of daily bars; 5Y from
`5Y.json`, weekly), each trimmed to its named span by time; and a pure state machine for "which
series is wanted, which is in flight, which are cached and how old" (select, arrive, tick, close) with
a generation so a late answer for a previous symbol or range is dropped. Fetched only for the locked
symbol, only while the pane is open, cached with a time to live, at most four series, all dropped when
the pane closes. Bar duration is inferred from the data, not assumed. Range tabs in the pane header;
dates on the axis; `--probe` prints every range. **Unverified live by construction**: the shapes come
from `FEATURES.md` Appendix A.

**P5. Keyboard, cards and order.** New `src/keys.rs` (pure: key and state in, action out). Arrows move
the lock across the grid, Enter opens CNBC, C the chart, S the picker, Ctrl+C copies, Delete removes
(never the last symbol), Ctrl+Left/Right moves the locked card. Card menu: Remove from strip, Move
left, Move right, Move first. Order is a permutation of the catalogue, so rows still follow catalogue
order. A click on a symbol in the strip opens the panel with that card locked.

**P6. Type it in.** New `src/prompt.rs`: a modal one-line prompt from an in-memory dialog template.
*Add symbol* (both builds): the symbol is checked against CNBC on a worker thread before it is added;
a second add while one is in flight is refused; hand-added symbols can be removed. *Alert at price*
per symbol. Alert levels drawn on the candle pane, and a right-click on the pane offers "Alert at
<price under the pointer>".
