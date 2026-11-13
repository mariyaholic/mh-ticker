# P1 hand-off: Alerts you can set and silence

Branch `wip/p1-alerts`. Builder part done: `wcargo check --all-features --all-targets` is clean, all
tests pass except the known Wine failure. Every `TODO(finish)` is in `src/alerts.rs`.

## TODO list, in order

1. `alerts::quiet(local_minute, window)`: `[from, to)`, `from == to` is off, `from > to` wraps midnight.
2. Un-ignore `quiet_window_inside_outside_wrapping_and_empty`, write its assertions (listed in the
   comment), and replace the placeholder lines in `silence_order_and_the_clock_ending_a_timed_mute`
   with a real `Silence::Quiet` assertion.
3. `alerts::describe(alert, digits)` and test `describe_lists_what_is_set`.
4. `alerts::entry_line(entry, local_minute_now, now_utc)` and test `entry_line_names_time_title_and_outcome`.
5. `alerts::accept_webhook(text)` and test `accept_webhook_takes_good_urls_and_refuses_bad_ones_silently`.
6. Edge cases: NaN/zero in `check` (day move NaN, cooldown 0), `preset_level` with a negative reference,
   `local_hhmm` with NaN, `Log` with `set_outcome` on an unknown seq.
7. Clippy, `scripts/check-state-linux.sh`, `stills-check`, the final notes.

Until 1, 3, 4, 5 are done the menu shows empty "recent" lines, no quiet window and a refusing Paste.
Nothing crashes: the stubs return safe values, not `todo!()`.

## Do not change

- Signatures and doc contracts in `alerts.rs`; `Fired { label, title, body, at }`, `Entry { seq, at, title, outcome }`.
- `PRESETS` order (the menu's `Action::Alert(label, k)` indexes it).
- `check` classifies nothing about silence; `ui::on_feed` does, once, inside its first `with_ui`.
- Menu IDs 5200 to 5214, and `alert_command` / `alerts_view` / `clear_pending` / `sync_icon` in `ui.rs`.
- `spawn_post` now returns `bool` (false = thread not started).

## Decisions the brief did not make

- `WM_APP + 6`, `+ 7` and timer 4 are unused: a timed mute ends by the clock and the webhook result
  reuses `WM_ALERT` (`wparam` 1 = failed, `lparam` = log seq).
- Superseded by the reviewer: "past today's high" is `Above(high + tick)` and "past today's low" is
  `Below(low - tick)` (tick = 10^-digits), so a price already at the extreme does not fire. Previous close is
  `Above` when the price is under it, else `Below`. The original note read: "A price already at the high fires
  at once (documented on `preset_level`). Unsure if
  that is what users want; the alternative is a strict `>`.
- A day-move preset is enabled whenever a fresh CNBC reference exists (it needs no other field).
- One `Fired` per alert per pass; a price level wins over the day move.
- A `once` alert that is silenced still loses its level (the brief says silenced alerts are evaluated
  like any other). It stays in Recent as muted/quiet.
- *Fire once* is a global toggle: new alerts copy `cfg.alert_once`, and toggling rewrites `once` on
  every existing alert.
- Test alert: logged as "Test alert" (outcome Shown, then Sent/SendFailed), ignores mute and quiet,
  does not set `alert_pending`, refused for 15 s after the previous one (time-based so it cannot stick).
- The tray marker is a white disc with a dark rim, bottom right, drawn in `make_icon`.
  `icon_link` is `Option<(Link, bool)>`. `tooltip(view, pending)` puts `Alert: <title>` first.
- `clear_pending` runs at the top of `expand` and `menu_at`.
- "Muted until HH:MM" uses the offset implied by the current local minute, so a DST change inside the
  mute window is off by an hour.
- Saves from new code go through `save_cfg()` outside `with_ui`.
- `Fired.label` carries `#[allow(dead_code)]` (nothing reads it yet).

## Unverified

Nothing was clicked: the menu, the clipboard read, the message box and the marker are reviewed by code only.
