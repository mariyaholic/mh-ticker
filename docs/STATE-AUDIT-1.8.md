# State-machine audit, 1.8

Scope: the integration head of 1.8 (alerts, chart readout, everyday settings, chart ranges and volume,
keyboard and card order, typed input) on `wip/audit`, both Classic and Ultra. Source-traced, with pure
unit tests for each fix. Nothing was clicked in a live window: Wine has no display here and there is no
network path to CNBC. `STATE-MACHINES.md` holds the inventory (27 machines); this file holds the findings.

Roles: the auditor traced and fixed. No fixer agent could be started in this environment, so the
auditor made the fixes directly, as the brief allows.

## Method

Every `Ui` field and every persisted `Config` field added in 1.8 was followed to its writers and its
resets. Every modal loop (`TrackPopupMenu` in `menu_at` and `pane_menu`, every `MessageBoxW` /
`alert_box`, the `prompt::ask` dialog) was checked for being reachable from inside a `with_ui` closure
(none is: no function that takes `&mut Ui` calls `with_ui`, and no closure calls a modal or a
message-sending window function), then for what its own message loop can dispatch: `WM_FEED`,
`WM_TIMER`, `WM_HOTKEY`, `WM_TRAY` (a nested tray menu), `WM_LOOKUP`, `WM_UPDATE`, `WM_ALERT`,
`WM_RANGE`. The blocking morph pumps no posted messages, only sent ones (paint, size, DPI).

## Findings

| # | Severity | Title | Status | Pinned by |
|---|---|---|---|---|
| 1 | Medium | *Refresh now* does not reach the range book | fixed | `ranges::tests::refresh_now_asks_again_for_a_failed_key_and_keeps_the_rest` |
| 2 | Medium | Fallback toggle writes a settings copy from before its confirmation box | fixed | `ui::tests::the_fallback_toggle_writes_what_happened_during_its_confirmation` |
| 3 | Low | A webhook result loses "test" once its log line is gone | fixed | `ui::tests::a_webhook_result_knows_a_test_without_its_log_line` |
| 4 | Low | "Alerts (n)" counts alerts on hidden symbols the submenu does not list | fixed | `ui::tests::the_alerts_count_is_of_listed_symbols_only` |
| 5 | Low | Card menu Copy/Open act on an index captured when the menu opened | open | none |
| 6 | Low | The tray alert marker stays lit while the panel is open | open | none |
| 7 | Low | A second *Add symbol* prompt from the tray silently drops the first one's symbol | fixed in review (a box says so) | none |
| 8 | Low | Menu snapshots (`open`, `picked`) go stale when the hotkey or a tray click changes the mode during the menu | open (carried from 1.7.1) | none |
| 9 | Low | A morph across monitors of different DPI lays the strip out at the old DPI until the next feed tick | open | none |

### 1. Medium: *Refresh now* does not reach the range book (fixed)

- Where: `src/ui.rs:2400` (`CMD_REFRESH`), `src/ranges.rs:256` (`Book::want`, the 30 s `RETRY_AFTER_S` gate).
- Trigger: Ultra, chart open on 5D; the 5D fetch fails (status `Failed`). Menu > *Refresh now*. The board
  `kick` wakes the quote loop and the day-chart thread, but `Book::want` still refuses the key for the
  rest of its 30 s back-off, so the pane keeps saying the range failed after the user asked for a retry.
  Two machines (the refresh kick and the book) disagree about what "now" means.
- Expected: a refresh asks again at once for a failed long range; cached series and requests in flight
  are untouched (no double fetch).
- Fix: `Book::retry_now()` forgets recorded failures; `CMD_REFRESH` calls it and `want_range` in the
  same `with_ui` call that bumps the kick.

### 2. Medium: fallback toggle writes a stale settings copy (fixed)

- Where: `src/ui.rs:2294` (`CMD_FALLBACK`), helper `fallback_settings` at `src/ui.rs:2468`.
- Trigger: the old code cloned `u.cfg` when the command arrived, then showed the confirmation
  `MessageBoxW`, then wrote that clone with `save_checked`. The box's modal loop dispatches `WM_FEED`:
  a once alert fires, `alerts::check` clears its level (or removes it) and `on_feed` saves. The user
  answers Yes and the pre-box copy is written over it: the file holds the spent alert again, with its
  old cooldown, and anything changed from a nested tray menu meanwhile (poll, webhook, symbols) is
  undone on disk. Memory stays right, so the next save repairs it, but a session that ends without
  another save (log-off sends no `WM_DESTROY`) re-fires the spent once alert on the next start.
- Expected: the settings written are the ones in force when the user answers, with only the fallback
  policy changed.
- Fix: the copy is taken after the box (`fallback_settings(&u.cfg, enabling)`).

### 3. Low: webhook result loses "test" (fixed)

- Where: `WM_ALERT` handler (`src/ui.rs`, search `webhook_note(wp)`), `post_alert` at `src/ui.rs:1283`.
- Trigger: *Send a test alert*, then *Recent > Clear list* (or ten alerts fire) before the POST
  returns. The handler looked the seq up in the log to decide whether it was the test; the line is gone,
  so a failed test read "alert kept here: webhook failed" (no alert was kept) and a sent one read
  "alert: webhook sent".
- Fix: the test bit travels in `wparam` bit 1 (`alert_wparam`); bit 0 is still the failure. The note is
  built by `webhook_note`. The URL is never part of any note.

### 4. Low: "Alerts (n)" counted unlisted alerts (fixed)

- Where: `src/ui.rs:1861` (`menu_at`), helper `armed_labels` at `src/ui.rs:2457`.
- Trigger: set an alert on XAU, untick XAU. The Alerts submenu lists only shown symbols, but its title
  still said "Alerts (1)" with no entry to see or clear it.
- Expected: the count matches the listed rows. The hidden symbol's alert is kept (it fires again when the
  symbol is shown again, and is listed then); this is the existing policy and is unchanged.

### 5. Low: card menu Copy/Open by index (open)

- Where: `src/ui.rs:1909`, `1910`, dispatched by `CMD_COPY + i` / `CMD_OPEN + i`.
- Trigger: right-click a card; while the menu is open a lookup finishes (`WM_LOOKUP` -> `add_custom` ->
  `apply_rows`) and the new row lands before that card (only possible when hand-added rows sit outside
  the catalogue, which `menu_list` lists after it). *Open* then opens the card now at that index.
- Expected: act on the card that was right-clicked, as Move/Remove do (they carry the symbol through
  `menu_commands`). Not fixed: it needs a new `menu_commands::Action` variant in a module the Linux harness
  pins, and the trigger is narrow.

### 6. Low: alert marker while the panel is open (open)

- Where: `src/ui.rs:1377` sets `alert_pending`; only `expand` and a menu clear it.
- Trigger: panel open, an alert fires. The tray marker and the tooltip line stay until a menu opens or the
  panel is collapsed and reopened, although the user is looking at the panel. Documented decision of P1;
  left for a product call (clear on fire while `Mode::Panel` is visible, or on the next panel click).

### 7. Low: second *Add symbol* prompt (fixed in review)

- Where: `src/ui.rs:2820`.
- Trigger: *Add symbol...* prompt open; from the tray menu (its messages still reach the disabled owner)
  choose *Add symbol...* again, type MSFT, OK (lookup starts, `adding` = MSFT). Close the first prompt
  with AAPL: `add_symbol` finds `adding` set and returns `Busy` with no message; AAPL is dropped silently.
- Expected: a box saying a lookup is already running. `adding` itself is always cleared (`on_lookup`), so
  nothing sticks.
- Fixed in review: the busy branch now shows that box and names the symbol that was not added. A
  message box path, so it has no unit test; the decision itself (`AddStep::Busy`) is unchanged.

### 8. Low: stale menu snapshot (open, from 1.7.1)

- Where: `src/ui.rs:1961` (`CMD_PANEL` text from `open`), `src/ui.rs:2330` (`CMD_POS` guarded by `!open`).
- Trigger: tray menu open with the strip showing; the global hotkey expands the panel during the menu; the
  user picks a Position entry. `apply_placement` runs with the panel open and calls `set_topmost` with
  the strip's setting, which can take topmost away from the open panel. "Expand view" picked after the
  hotkey does nothing (harmless). Expected: re-read the mode after the menu closes, as `Action::Monitor`
  already does.

### 9. Low: morph across DPI (open)

- Where: `WM_DPICHANGED` (`src/ui.rs:3848`) runs synchronously inside `run_morph` while the mode is
  `Morph`, so `relayout_and_place` does nothing, and `collapse` computed `strip_rect` and the frame before
  the morph at the old DPI.
- Trigger: panel last dragged to a 144 dpi monitor, strip docked on a 96 dpi one; collapse. The strip is
  laid out with the old font until the next `WM_FEED`/`WM_DOCK` relayout (one poll). Self-heals.

## Checked and found consistent

- `Ui::adding`: set in the same `with_ui` call that starts the lookup, cleared first thing in
  `on_lookup` for every outcome, a failed spawn posts `WM_LOOKUP` itself. No path leaves it set.
- `update_running`: cleared after the outcome box in every branch, a failed spawn included.
- Lock vs rows: every lock move goes through `lock_card`; `apply_rows` runs `settle_lock`, `Book::forget`
  and `Slots::clear_request` (with `cancel`), so no lock or range request outlives its symbol.
- Range book vs worker: per-key in-flight, replaced requests cancelled, late answers for forgotten keys
  dropped, lost requests released after 120 s. Results arrive through `WM_RANGE` and are filed in one
  borrow.
- Classic: `selected`, `chart`, `range`, `pick_open`, alerts, pane menu, keys other than Ctrl+C and the
  strip-slot click are all gated on `profile.ultra`; Classic has no range worker.
- Webhook secrecy: only `webhook_host::redact` output reaches menus, boxes and notes; worker errors are
  reduced to fixed strings and the UI keeps only a failure bit.
- Hotkey, autostart, save-failure flag: each re-read or reset at the point of use; no unreachable state.

## Verification

- `wcargo test --all-features --no-fail-fast`: 240 pass, 1 fails (the known Wine font case,
  `ui::tests::a_theme_switch_takes_the_new_themes_face`). Baseline was 236 + 1.
- `wcargo clippy --all-features --all-targets`: only the expected `windres not found` line.
- `bash scripts/check-state-linux.sh`: 107 pass (the new `ranges` test runs there too).
- `stills-check`: 36/36 identical; Classic does not drift.
- The new tests exercise functions introduced by the fixes (`Book::retry_now`, `fallback_settings`,
  `alert_wparam`/`webhook_note`, `armed_labels`), so they could not pass on the code before the fixes;
  each test's comment names the failing sequence it stands for.

## Not verified

Real Windows: the modal-loop interleavings (feed, hotkey and tray during a box, menu or prompt), the
cross-DPI morph, the prompt's keyboard behaviour, a real webhook endpoint and live CNBC range files.
