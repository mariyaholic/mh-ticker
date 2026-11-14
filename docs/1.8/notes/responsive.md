# Responsiveness and state pass, 1.8

Branch `wip/responsive`, from the 1.8.0 head. Writer's notes. Nothing here was run on a Windows desktop:
the numbers are from the painter under Wine (`examples/perf.rs`), so read them as ratios between runs.
The `fold` and allocation lines are plain CPU and carry over; the GDI lines do not.

## Measurement

`cargo run --release --example perf --all-features` (under `wcargo`). It draws the real `panel::draw` into
an off-screen bitmap, then copies to the screen DC, as `paint` does, 200 frames per line. Fixture: the
chart fixture's S&P card locked, 1488 px at 96 dpi.

| Path | Before | After |
|---|---|---|
| Hover repaint, pane on 1D (ms/frame) | 3.32 | 2.37 |
| Hover repaint, pane on 5D | 3.26 | 2.16 |
| Hover repaint, pane on 1M | 3.61 | 2.26 |
| Hover repaint, pane on 5Y | 3.94 | 2.79 |
| Full repaint, pane on 5Y (unchanged path) | 4.08 | 4.08 |
| Full repaint, chart closed | 2.87 | 2.87 |
| `fitted_to`, 1,950 one-minute bars (us) | 72 | not changed |
| `fitted_to`, 4,000 one-minute bars (us) | 165 | not changed |
| `menu_list` + `shown` per frame, 23 rows (us) | 3.9 | 0 with the picker closed |
| One `GetTextExtentPoint32W` (us) | 0.33 | not changed |

"After" for the hover rows is the back buffer clipped to the pane and only the pane copied, which is what
a crosshair move now causes. A button hover or a card-sparkline hover is a smaller rectangle than the pane,
so it saves at least as much. Run to run the numbers move by about 0.2 ms.

## Candidates, one by one

- Candle pane re-folding on every repaint (`chartview::fitted_to`/`fold`): checked, ruled out. A fold of
  4,000 one-minute bars is 165 us; the longest real series (5D at one minute) is about 1,950 bars, 72 us;
  a 1D day is 390 bars, 9 us. A frame is 2 to 4 ms, so a cache would save 2 to 5 percent. It would also
  be a new state with a key (series identity, width, zone, bucket) that the feed tick invalidates every
  poll, which is the one moment the series changes. The cost does not justify the machine. Left as is.
- Text measurement and strip relayout on every feed tick: ruled out. A measurement is 0.33 us under Wine;
  a tick is every 3 s at the default poll; the strip only relayouts when visible.
- Full-panel repaint on hover (`InvalidateRect(hwnd, null(), ...)` in `WM_MOUSEMOVE`): real, fixed (below).
- `config::save` inside `with_ui`: the call was inside ten closures. It is user-initiated (a toggle, the end
  of a drag, a theme) or once per fired alert, never per frame, and moving it out of the closure does not
  shorten the wait (one thread). What it does is keep the rule "no I/O inside `with_ui`" true. Fixed for
  all ten sites with the existing `save_cfg()` (copy the config in `with_ui`, write outside). A background
  writer thread would remove the wait on a slow disk but adds a writer/exit-flush machine; not done, and
  not measurable here (Wine's disk is a tmpfs).
- Registry read when a menu opens: one `RegQueryValueExW` for "Start with Windows", per menu open, which
  is a user action followed by a modal loop. Ruled out.
- Tray tooltip: rebuilt at most every 5 s (`TIP_EVERY`) unless the link colour or a source changes;
  `sync_icon` only rebuilds the icon when its key changes. Ruled out.
- Per-frame allocation: `draw_panel` cloned the whole catalogue (`menu_list`) and built the shown flags on
  every frame, although only the open picker draws them. Fixed: built only while the picker is open
  (3.9 us, 23 rows; a user with a long catalogue pays more). The painter's other allocations (a UTF-16
  buffer per text call, a few `format!`) are small and left.
- The 500 ms dock timer repaints the whole open panel to keep the ages ticking: left. That is two
  frames a second, about 0.4 percent of a core at the Wine numbers, and the card age text and the sliding
  sparkline do change.
- `GetTimeZoneInformation` per frame: one cheap call, left.

## Fix: repaint only what a hover changed

- `panel::Hot` (the hover flags, the aimed candle, the hovered card) and `panel::hot_dirty` turn two hover
  states into the rectangle that differs: a button, the Open button, the candle pane, or the cards the
  pointer left and entered. `WM_MOUSEMOVE` and `WM_MOUSELEAVE` invalidate that rectangle, not the window.
- `paint` clips the back buffer to `rcPaint` and copies only that. A new back buffer is drawn whole, so a
  resize cannot show an empty buffer. The painter still runs end to end (layout and pointer data are
  written every frame), only the GDI output outside the clip is dropped.
- A moving background (`clock_runs`) is drawn from the clock at paint time, two partial paints of it
  would not match along their edge, so those themes keep the whole-window invalidate (they repaint every
  100 ms anyway).
- Test: `shot::tests::a_hover_change_repaints_inside_the_rectangle_it_names` renders the real painter for
  each hover part (and one pair), with the pane open and closed, diffs the pixels against the idle frame
  and asserts every changed pixel is inside the rectangle. A missed rectangle would leave a stale mark on
  a real screen.
- State: `Back` buffer validity (see `STATE-MACHINES.md`): it holds the last frame drawn, and everything
  outside the invalid rectangle is, by construction, still current because every other state change
  invalidates the whole window.

## State findings

Each was fixed test first. Red means I ran the test against a stub with the old behaviour and saw it fail.

- 5, card menu Copy/Open by index: `Action::Copy(label)` and `Action::Open(symbol)`, resolved by
  `card_target`. Red: Copy put "A 1.00" on the clipboard for the card "B". Test:
  `the_card_menu_copy_and_open_follow_the_card_not_its_old_index`. `CMD_COPY`/`CMD_OPEN` ranges removed.
- 6, alert marker while the panel is open: `marks_tray(mode, panel_in_front)`; a fired alert does not set
  `alert_pending` when the panel is open and the foreground window. This does not contradict P1 ("until
  the panel is expanded or a menu is opened"): that sentence covers a marker lit before; one lit while the
  panel is already open and in front would never meet an expand. The footer note still names the alert. A
  panel open behind another window still lights it, which is the Focus Assist case the marker exists
  for. Red: the stub lit it in every mode. Test: `an_alert_does_not_light_the_marker_while_the_panel_is_in_front`.
- 8, stale menu snapshot: `panel_step` and `position_applies` take the mode when the entry is chosen.
  Choosing "Expand view" after the hotkey opened the panel now does nothing; a Position entry is dropped
  if the panel is open. Red: both followed the opening snapshot. Test:
  `the_menu_acts_on_the_mode_it_is_chosen_in_not_the_one_it_opened_in`. The check-mark toggles (Show strip,
  Always on top, Symbol picker) still flip the state the user saw, which is what the tick promised.
- 9, morph across DPI: `collapse` and `expand` compare the scale before and after the morph and lay the
  end frame out again at the new one (`settled_frame`). Red: the stub kept the old plan. Test:
  `a_morph_across_a_scale_change_replans_its_end_frame`.

New implicit machines found while doing this: the back buffer's validity and the hover rectangle (both
now in `STATE-MACHINES.md`). Nothing else missed: the two reachable ways the buffer could be wrong
(a resize, a theme change) both end in a whole-window invalidate or a new buffer.

## Not verified on a real desktop

The real GDI cost of a full versus a partial repaint on DWM (Wine's blit goes through X11); the cross-DPI
morph (needs two monitors at different scales); the Focus Assist case for finding 6 (the foreground test
is `GetForegroundWindow() == hwnd`); slow-disk behaviour of `config::save`; that no other Windows-sent
partial invalidation leaves a stale area (the design relies on `rcPaint` being exactly the invalid part).
