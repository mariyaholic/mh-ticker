# P6 "Type it in": builder hand-off

Branch `wip/p6-prompt`. The builder wrote the skeleton and every Win32, threading and wiring piece; the
finisher completes the pure rules and the tests marked `// TODO(finish)`.

## State at hand-off

- `wcargo check` and `clippy` clean (the `windres not found` line is expected).
- Tests: 228 pass under Wine, 1 fails and is not ours (`ui::tests::a_theme_switch_takes_the_new_themes_face`).
- Linux harness: 106 pass (one new `chartview` test).
- `stills-check`: 36/36 identical. The default chart fixture (`... 1 40 refs`) was rendered before and
  after this package and the two PNGs are byte-identical.
- `docs/1.8/notes/p6-levels.png`: the fixture with `levels` (one level inside the window, one above it),
  looked at: the dashed amber line with its price at the plot's right end, and the top-edge marker with
  its price.

## TODOs, in the order to do them

1. `config::clean_symbol` (code stub, returns `None`: until done, *Add symbol* refuses every symbol).
2. `config::label_for` (code stub, returns the symbol cut to 8 characters, not unique).
3. `config::digits_for` (code stub, returns 2).
4. `config::clean_name` (code stub, returns empty).
5. The config tests listed after `toggle_keeps_catalog_order_and_limits` (six tests, including
   `add_custom` and `forget_custom`, which are already implemented).
6. `alerts::parse_level` (code stub, returns `None`: until done, *at a price...* refuses every text).
7. `alerts::side_of` (code stub, always `Err`: until done, the pane's *Alert at* is always greyed and
   *at a price...* always refuses).
8. The two alerts tests after `round_level_keeps_the_symbols_digits`.
9. `source::tests::lookup_answers_found_or_not_found`: the remaining cases.
10. `chartview::tests::alert_levels_are_lines_inside_and_markers_outside`: the edge cases.
11. `prompt::tests`: the template edge cases and the `max_len` round trip.

Each TODO states its contract in the doc comment above it and its acceptance cases in the TODO itself.

## Do not change

- `prompt::ask`, `dialog_proc`, the test-only `ACCEPT_AT_ONCE` switch and the `template` layout (ids,
  item order, the 4-byte alignment, the copy into `u32` words before `DialogBoxIndirectParamW`).
- The reserved numbers: `WM_LOOKUP = WM_APP + 14`; menu ids 6000 (`CMD_ADD_SYMBOL`), 6010 to 6012 (the
  pane menu). `WM_APP + 15` and timer 9 are reserved and unused.
- `Ui::adding` and `Ui::lookup_result`, `add_symbol`, `start_lookup`, `on_lookup`: `adding` is set in
  the same `with_ui` call that decides to look up, and `on_lookup` clears it before anything else, for
  every outcome; a failed thread start posts `WM_LOOKUP` itself.
- `pane_menu`, `set_price_level`, `alert_at_price`, `put_level`, `fresh_price`: no prompt, menu or box
  runs inside a `with_ui` closure.
- The signatures and contracts of `clean_symbol`, `label_for`, `digits_for`, `clean_name`, `add_custom`,
  `custom_entries`, `forget_custom`, `parse_level`, `side_of`, `round_level`, `source::lookup`,
  `source::parse_lookup`, `chartview::level_marks`, `LevelMarks`. If one looks wrong, say so in the notes.
- `panel::Chart::levels` and the painter block in `panel::candles` (it draws exactly `level_marks`).

## Decisions the brief did not make

- **Prompt**: 220 x 66 dialog units, "Segoe UI" 9 pt (Wine substitutes), label up to two lines with
  `SS_NOPREFIX` (so `S&P` reads as typed). The test switch posts `WM_COMMAND IDOK` from `WM_INITDIALOG`,
  so the test runs the same OK path a click does. The template is returned as bytes plus item offsets
  (`Template { bytes, items }`) so the alignment test needs no parser.
- **Add symbol**: the prompt allows 40 typed characters; `clean_symbol` enforces 20 after trimming. A
  symbol already shown gets an information box ("already on the strip"); a listed but hidden one is
  ticked, or a box says the strip is full. The lookup is one quote request with its own short-lived
  agent (10 s, no redirects, at most 256 KB read); errors are reduced to `network`, `HTTP <code>`,
  `unreadable answer`, `could not start`. Found means the answer has a quote whose symbol equals the
  typed one ignoring case, with a positive price. A found symbol that does not fit (24 shown) is still
  added to the catalogue and a box says to untick one.
- **Labels and digits** (contracts in `config.rs`): label = symbol without a leading `@ . ^`, a trailing
  `=` or a trailing `.1`, at most 8 characters, `SYM` if empty, made unique by `2`, `3`... Digits: 0
  from 10,000, 2 from 10, 4 from 0.01, else three significant figures up to 8. Name: control characters
  dropped (a tab would split the menu's columns), spaces collapsed, at most 40 characters with `...`.
  A `&` in a name still shows as a menu mnemonic, as `S&P` always has.
- **Remove a symbol I added**: lists catalogue entries not in `default_catalog()` as `label<TAB>symbol`;
  the entry for the only shown symbol is greyed; the whole submenu is greyed when there are none.
  Removing also deletes that label's alerts (an alert for a symbol that is gone can never fire).
- **Alert at a price**: the prompt opens on the fresh price formatted with `fmt_price` (with thousands
  separators, which `parse_level` accepts). The side rule is applied against the fresh price at the
  moment of accepting, not of opening, and compares at the symbol's digits, so accepting the unchanged
  current price is refused with a reason. The new level goes through the same find-or-create as the
  presets (honouring *Fire once*).
- **Pane menu**: opens instead of the main menu when the right-click is inside the price plot of the
  stored layout (Ultra, panel open, chart open, candles drawn). Its last entry, *More...*, opens the main
  menu, so nothing is out of reach. *Alert at* is greyed when there is no fresh price or the side rule
  refuses. The pane repaints after a level is set or cleared.
- **Levels on the pane**: drawn on every range, over the candles and under the crosshair. Outside the
  window each edge shows only the nearest level, with `+n` for the others beyond that edge. The label of
  an in-window level goes above its line (below it at the very top), like the `prev` label.
- **MAX_ROWS 12 to 24**, what is sized by it: `toggle` (the cap), the Symbols menu's greying, and the
  card menu's `CMD_COPY + i` (5600..5623) and `CMD_OPEN + i` (5700..5723) ranges, which stay below the
  next range (5700, and `CMD_PICK` 5800). The panel's grid and minimum size come from the cell count,
  not the cap: 24 cards in three columns is about 1,354 px tall at 96 dpi, more than a 1080p work area,
  but `WM_GETMINMAXINFO` already caps the minimum track size to the work area and the body scrolls. The
  picker sizes from the catalogue length. The dock strip at 24 symbols was not looked at on a real
  taskbar. The config test now builds its own catalogue one longer than the cap.

## Unverified off Windows

- The dialog on real Windows (CI runs the same self-accepting test natively). Keyboard behaviour
  (Enter, Esc, the text selected on open) was not exercised by hand anywhere.
- `source::lookup` against CNBC (no network path here). What CNBC returns for an unknown symbol is
  assumed to be either no quote or a quote without a positive `last`; both read as not found.
- The pane menu and the message boxes were not clicked through; they follow the existing menu code.
- `examples/chart.rs` takes a 10th argument, `levels`, e.g.
  `... midnight 96 1488 1 - - - - levels`.
