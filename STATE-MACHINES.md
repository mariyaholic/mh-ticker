# Runtime state machines, 1.8 (W1 and W2 rows are at the end of the inventory)

Scope: Classic and Ultra at the 1.8 integration head. Source inventory plus targeted tests; not a proof
of every native interleaving. Findings and fixes of this pass: `docs/STATE-AUDIT-1.8.md`. The 1.7.1
record (fallback policy, startup, placement, geometry, feed, backfill) is `docs/STATE-AUDIT-2026-11.md`
and the earlier version of this file; what it says still holds unless a row below changes it.

## Rules every machine follows

- All `Ui` state lives in one thread-local `RefCell`; `with_ui` uses `try_borrow_mut` and silently does
  nothing when nested. So: no modal loop, menu, box, dialog, morph, file write or message-sending window
  call inside a `with_ui` closure, and no function taking `&mut Ui` calls `with_ui`. Settings are written
  with `save_cfg()` (copy inside, write outside); the ten in-closure saves of 1.8.0 were moved out.
- Modal loops (menus, boxes, the prompt) dispatch posted messages: feed, timers, hotkey, tray, lookup,
  update, alert and range results. Code after a modal must re-read state, never act on a copy taken
  before it (finding 2 fixed one that did not; finding 8 fixed the menu's mode snapshots).
- The morph (`run_morph`) blocks the thread and pumps only sent messages; while it runs `mode == Morph`.
- Workers never touch `Ui`: they fill an `Arc<Mutex<_>>` slot and post a message; the handler takes the
  slot in one borrow.

## Inventory

| Machine | States | Transitions (trigger) | Encoded in | Way out / reset | Tested by |
|---|---|---|---|---|---|
| Window mode | Strip, Morph, Panel | expand (click, tray, hotkey, ExpandLock, Expand view); collapse (button, Esc, tray, hotkey, Back to taskbar); set_chart glide | `Ui::mode`, `morph`, `strip_rect`, `last_panel`, `prev_fg` | morph always ends in a mode; `TIMER_DOCK` repairs `Morph` with no picture; a scale change during the morph re-plans the end frame (`settled_frame`) | pure geometry tests; `a_morph_across_a_scale_change...`; no live morph test |
| Chart pane | closed, open | button, key C, `apply_picker(true)` closes; open only from Panel | `chart`, `chart_grew`, `last_panel` | close gives the width back (`rect_after_chart`), resize or DPI drops the record, `compact_frame` for saved frames | `rect_after_chart`, `compact_frame`, `grown_rect` tests |
| Lock | none, label | card click (toggle), arrows, Delete/Remove (to neighbour), strip slot click, chart open with none | `selected` (Ultra only) | `lock_card` is the one mover; `apply_rows` -> `settle_lock` | `toggle_lock`, `settle_lock`, `neighbour_of` tests |
| Pointer and layout | none, bar+row | mouse move over plot; reset on leave, lock move, range tab, zone flip, rows change, chart toggle, expand, collapse | `chart_at`, `chart_layout` (Cell, painter-written) | painter clears the layout every frame without a card; painter clamps a stale bar | chartview layout and pointer tests |
| Range choice | 1D, 5D, 1M, 6M, 1Y, 5Y | tab click | `range` | chart close -> 1D; kept across lock change and collapse | none in ui |
| Range book (per key) | absent, in flight, cached, failed | `want` (pane needs it, every feed tick), `arrived`, `cancel`, `forget`, lost after 120 s, retry after 30 s, `retry_now` (Refresh now) | `book` | every state leaves by time, by answer or by `forget` | 10+ `ranges` tests incl. `refresh_now_asks_again_for_a_failed_key_and_keeps_the_rest` |
| Range slots | empty, request waiting; done queue | `submit` replaces (caller cancels), worker `take_request`, `deliver`, UI `take_done`, `clear_request` on rows change | `range_slots` (Arc, shared with the worker) | lock never held across I/O | `Slots` tests |
| Feed and chart workers | polling, backoff, waiting; rows_gen, kick | rows change, `Board.kick` (Refresh now), timers | `Board` (`rows_gen`, `kick`, `poll_s`, fallback) | generation checks drop stale publications | source tests incl. kick tests |
| Alerts (per alert) | armed, cooling down, spent (once) | `check` on fresh CNBC quotes; preset, *at a price*, pane *Alert at* (`apply_level` re-arms); Clear; Forget symbol | `Config::alerts` (`above`, `below`, `day_move`, `last`, `once`) | cooldown by clock (a clock set back reads as cooled, `alerts::cooled`); once alert removed when empty; a typed level that is far from the price asks first (a box, nothing stored) | alerts tests |
| Silence | none, Muted, MutedUntil(t), Quiet | Mute menu, Unmute, clock, quiet window | `alerts_muted`, `alerts_mute_until`, `quiet_hours` | timed mute ends by the clock; Unmute | `silence_order...`, `quiet_window...` |
| Alert marker | none, pending(title) | any fired alert (silenced too), unless the panel is open and in front (`marks_tray`) | `alert_pending`, `icon_link` | `clear_pending` on expand and any menu | `a_pending_alert_leads_the_tooltip...`, `an_alert_does_not_light_the_marker_while_the_panel_is_in_front` |
| Alert log and webhook | entry Shown/Muted/Quiet -> Sent/SendFailed | fire, POST result `WM_ALERT` (bit 0 failed, bit 1 test), Clear list | `alert_log` (seq never reused), `alert_note` | late result for a gone line changes nothing; the note still names a test | log tests, `a_webhook_result_knows_a_test_without_its_log_line` |
| Test alert | idle, sent within 15 s | *Send a test alert* | `test_alert_at` | time-based, cannot stick | none |
| Webhook URL | none, set | Paste (validated, host confirmed), Remove | `Config::webhook` | only the host is ever shown | `accept_webhook...`, redaction tests |
| Update check | idle, running, box shown | menu; worker posts `WM_UPDATE` (failed spawn posts too) | `update_running`, `update_result` | cleared after the box in every outcome | update tests |
| Hotkey | off, registered, failed | menu, start-up | `Config::hotkey`, `hotkey_failed` | re-registered on every change; Off | hotkey tests |
| Autostart | off, on (this exe) | menu | registry only | read at every menu open | autostart tests |
| Save failure | ok, failed | every `save`/`save_checked` | `config::SAVE_FAILED` | next good save clears | `save_failed_flag...` |
| Symbol lookup | idle, adding(symbol) | *Add symbol* -> worker -> `WM_LOOKUP` | `adding`, `lookup_result` | `on_lookup` clears first, every outcome | config add tests, lookup parse tests |
| Prompt dialog | closed, open (owner disabled) | add symbol, alert at a price | `prompt::ask` locals | OK / Cancel; caller re-reads state after | template and self-accepting tests |
| Main menu | built, tracking, dispatched | right-click, tray, settings button, pane *More...* | `MenuCommands` registry (dynamic ids carry symbol/label, card Copy and Open included); fixed ids; the mode is re-read when an entry is chosen (`panel_step`, `position_applies`) | registry dropped with the menu | menu_commands tests; `the_card_menu_copy_and_open...`, `the_menu_acts_on_the_mode...` |
| Pane menu | built, tracking | right-click inside the plot (Ultra, chart, bars drawn) | captured label, digits, level | acts by label, re-checks the side rule against the fresh price | `side_of`, `round_level` tests |
| Keyboard | panel focused or not | `on_key` decides in one borrow, acts after | `keys::State` from `Ui` | Panel only; unmapped keys to the system | keys tests |
| Picker and scroll | closed, open; scroll offset | button, key S, tray entry; wheel, paging | `pick_open`, `picker_scroll`, `wheel_remainder` | toggle resets both; viewport clamps | wheel, reveal tests |
| Strip slots | per laid-out symbol | every strip layout | `frame.slots` | rebuilt by `strip_place` on each relayout and collapse | `slot_at` tests |
| Strip settings | rows, text, opacity | Strip menu | `dock_rows`, `dock_font_pt`, `font_pt`, `opacity` | alpha set only in Strip; collapse picks up a change | `strip_step_tests` |
| Zone | local, UTC | zone button | `Config::chart_utc` | toggles back | chartview clock tests |
| Back buffer | none, valid for (w, h) | `paint` allocates on the first paint and on any size change; every paint draws into it clipped to `rcPaint` and copies that rectangle | `Ui::back` | a new buffer is drawn whole; every state change other than a hover invalidates the whole window, so outside `rcPaint` the buffer is still current | `shot` pixel test below |
| Hover rectangle | none, rect | `WM_MOUSEMOVE`/`WM_MOUSELEAVE` compare `panel::Hot` before and after (`hot_dirty`) and invalidate the union of what differs; moving backgrounds and unnameable changes invalidate everything | `Ui::hover`, `pick_hover`, `chart_hover`, `gear_hover`, `open_hover`, `chart_at`, `hover_at` (read as one `Hot`) | the next full invalidate repaints everything | `a_hover_change_repaints_inside_the_rectangle_it_names` (pixel diff against the real painter) |
| Folded candles | none (not cached) | `fitted_to` runs on every pane repaint: 72 us for 1,950 bars, 165 us for 4,000 (`docs/1.8/notes/responsive.md`) | none | nothing to reset; a cache was measured and rejected | none |
| Diagnostics report (W1) | idle, running | menu *Copy diagnostics report*; worker posts `WM_REPORT` (a failed spawn posts too) | `report_running`, `report_result` | cleared after the box in every outcome; a second click while set does nothing | `report` tests (text); the flag follows the update check's pattern, no ui test |
| Data meter (W1) | counting | every response body read is counted when its reader drops; read when the menu is built | `source::METER` (`meter::Meter`, 60 minute slots and a total) | in memory only, gone at quit; an hour-old slot is ignored by `last_hour` and overwritten, no timer | `meter` tests (roll-over, idle hour, clock set back) |
| Kept settings (W1) | none, kept(name) | start-up finds a settings file that will not parse: it is renamed to `settings.bad.json` (or `.bad-2`...) and the defaults are written | `config::KEPT_AS` (file name only) | for the run; fix or delete the kept file, nothing in the app clears it before quit | `a_config_that_will_not_parse_is_kept_and_defaults_load` |
| Chart note (W1) | none, as of, check dates | computed at every paint from the cached series (`Series::note`) | none stored | disappears when the series is fresh or the pointer is over the plot (the legend takes the line) | `ranges` tests, `the_chart_note_shows_until_a_candle_is_hovered` |
| Probe and dump (W1) | command line only | `--probe`, `--probe --dump <folder>` | none (a process that exits) | exit 0, 1 on any WARN, 2 on a bad `--dump`; writes only into the named folder | `probe` tests |
| Clock zone (W1) | fixed, one change | built per paint from the bars' first and last instant (`chartview::Zone::spanning`) | none stored | rebuilt every paint | `a_fold_across_a_clock_change...`, `zone_spanning...` |
| Poll governor (W2) | base, slowed (quiet 10 min: 15 s; 30 min: 60 s), resting (locked, nothing armed: 300 s) | the feed asks `governor::interval` after every poll; any `kick` (Refresh now, expand, hotkey, unlock, a settings or alert change that could shorten the wait) ends the wait and restarts the quiet clock | `Board::{gov, locked, armed, governed_s, slowed}` (UI writes the first three, the feed the last two), `governor::Quiet` (a local of `Feed::run`, monotonic seconds) | both ticks off (the Classic default) is the plain cadence and `governed_s` 0; armed caps the wait at 15 s (or the user's slower base); a failing feed keeps its own back-off (up to 60 s) | `governor` tests (every mix of locked, armed, quiet), `the_feed_waits_the_governed_interval_only_when_a_tick_is_on`, `publish_uses_the_governed_stale_limit` |
| Stale limit (W2) | configured, governed | `governor::stale_after(stale_after_s, governed_s)`: twice the governed interval when that is larger | `Board::governed_s`, read by `Feed::publish`, `on_feed`, `alerts::check_with`, `alerts::overview` | 0 means the configured limit; recomputed each tick | `the_stale_limit_follows_the_governed_interval`, `the_stale_limit_in_check_follows_the_governed_interval` |
| Session lock (W2) | unlocked, locked | `WM_WTSSESSION_CHANGE` 7 and 8 (registered by name from `wtsapi32.dll` at start-up); unlock wakes the feed | `Board::locked` | unlock; a failed registration leaves it unlocked for good (the governor then never rests) | no ui test (needs a real session); unverified |
| Repeat mode and crossing latch (W2) | per alert: every cooldown, once, each crossing; latched bits above, below, day move, fast | the Repeat radio rewrites `once` and `crossing` on every alert and clears the latches; `check_with` latches on a fire and unlatches when the price is back past the band (0.1 % of the level, 0.1 point for a day move, half a fast threshold) | `Alert::{once, crossing, latched}`, `Config::{alert_once, alert_crossing}`; `latched` is not saved | a restart, `apply_level` and a mode change clear the latches; stale and fallback quotes neither fire nor re-arm | `each_crossing_*`, `in_every_repeat_mode_a_stale_quote_fires_nothing` |
| Fast move (W2) | none, 1 % in 5 min, 2 % in 15 min, 3 % in 60 min | the per-symbol menu (the current choice again turns it off) | `Alert::fast` (index plus one) | `once` clears it; Clear this symbol or Clear all removes the alert | `a_fast_move_*`, `fast_move_once_clears_and_crossing_latches_until_it_calms` |
| Gap tracker (W2) | idle, watching, blind (announced) | `watch::Watch::step` on every feed tick: armed and a fresh price on some armed shown symbol; a span longer than max(5 min, 10 polls) is announced once (tooltip line) and, when checking is back, becomes one Recent line (and one webhook POST if asked) | `Ui::watch` | nothing armed on a shown symbol forgets the span; a clock set back starts over; in memory only | `watch` tests, `checkable_*` |
| Hush and digest (W2) | quiet, silenced(since) | `watch::Hush::step` on every feed tick with `alerts::silence`; the tick that finds the silence over asks `alerts::digest` | `Ui::hush` | ends when the silence does (a timed mute ends by the clock, no timer); one balloon per ending, none for zero | `the_digest_*`, `a_hush_reports_its_start_when_it_ends` |
| Webhook post, retry and quit (W2) | idle, in flight, waiting to retry | spawn per alert; a network error or a 5xx waits 3 s and tries once more (`retry`); quitting waits up to 2 s for `IN_FLIGHT` to reach 0 and sends a waiting retry at once | `alerts::{IN_FLIGHT, QUITTING}`, `WM_ALERT` bit 2 (retried) and bit 3 (gap notice) | the thread always ends (10 s timeout per try); a gap notice has no log line | `retry` tests, `quitting_waits_for_a_post_in_flight_but_never_past_the_cap`; the quit path itself is unverified |
| Recent list (W2) | up to 50 entries | an alert, a gap line, a webhook result; Clear list; Copy list | `Ui::alert_log` | memory only; sequence numbers survive a clear | `the_log_holds_fifty_and_drops_the_oldest_at_fifty_one` |

## Ownership

`ui.rs` owns every transition and every window call. `ranges`, `alerts`, `keys`, `chartview`,
`menu_commands`, `config` row functions and `update`/`hotkey` parsing are pure and are what the tests pin;
the Linux harness runs the pure modules (see `scripts/check-state-linux.sh`).

## Open findings

See `docs/STATE-AUDIT-1.8.md`: findings 1 to 9 are all fixed (5, 6, 8, 9 in the responsiveness pass,
`docs/1.8/notes/responsive.md`). Still open: the 1.7.1 list (webhook
durability on exit, worker join, DPI and monitor changes, topmost and focus restoration, history
corrections). Nothing here was exercised on native Windows.
