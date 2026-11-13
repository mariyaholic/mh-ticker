# P3 Everyday: builder hand-off

Branch `wip/p3-everyday`. Built: all wiring (menu, `wndproc`, `run`, registry, threads, hotkey registration,
update worker), the config flag and field, `Board.kick`, `Feed::wait`/`kick_bars`, `source::get_release`.
Unfinished: the pure bodies and test stubs below. Until they are done the new menu entries are harmless:
the hotkey presets do not register (`parse` returns `None`, so the Hotkey submenu shows nothing selectable that
works), *Check for updates* reports "unreadable answer", *Copy all prices* copies nothing, and the Strip
radios mark the default step.

Dead-code warnings (`MOD_*`, `latest`, `newer`, `Outcome::UpToDate/Newer`) disappear when the bodies exist.
Each stub test has `#[allow(unused_imports)] use super::*;` with a TODO: remove the allow when the test body uses it.

## TODO(finish), in order

1. `src/hotkey.rs` `parse` (accept: grammar in the module doc; `" ctrl + ALT + m "` parses; `Ctrl+Alt`, `Alt+Q+W`,
   `M`, `Shift+M`, `Ctrl+F25`, `""` are `None`). Do not change `Combo`, the constants or `PRESETS`.
2. `hotkey::label` (canonical order Ctrl, Alt, Shift, Win, then key; round trip with `parse`). Delete the placeholder hex format.
3. `hotkey` tests (5 stubs, names and assertions are in the comments).
4. `src/update.rs` `latest`, `newer` (version rules are in the doc comment: numeric compare, missing parts 0,
   `-suffix` is older than none, `+build` ignored), `decide`.
5. `update` tests (6 stubs).
6. `model::board_text` and its test (`model.rs`).
7. `ui.rs` `nearest_rows`, `nearest_text`, `nearest_opacity` and `strip_step_tests` (3 stubs).
8. `autostart` tests `autostart_command_quotes_the_path`, `autostart_same_command` (bodies of `command` and `same_command` are
   done, and the Wine registry round-trip test is written and passes).
9. `config` tests `save_failed_flag_sets_and_clears_with_the_last_save`, `hotkey_defaults_to_none_and_old_configs_parse`.
10. clippy, `bash scripts/check-state-linux.sh`, `stills-check` (36/36 identical at hand-off), notes file for the user (`docs/1.8/notes/p3.md`).

TODO(finish) markers in code: see `grep -rn "TODO(finish)" src`.

## Must not change

- Signatures and doc contracts of the stubs; `config::save`'s signature; `record_save` is called from `save_to` and
  `save_checked` only, NOT from `save_checked_to` (so the existing failing-writer test cannot flip the flag in parallel).
- `Feed::wait` captures `kick` on entry; the chart thread tracks its own `seen_kick`; no new `Feed` field (the many test
  constructors would break).
- `apply_strip` sets the layered alpha only in `Mode::Strip` (see its doc: expand fades `cfg.opacity` -> 255, collapse fades back using
  `cfg.opacity`, so a change made with the panel open is picked up on collapse, never doubled).
- `on_update` clears `update_running` after the message box closes. The download page is the constant `update::RELEASES_URL`.
- Menu anchors and IDs (5400..5445), `WM_UPDATE = WM_APP + 9`, `HOTKEY_ID = 1`.

## Decisions the brief left open

- `hotkey::PRESETS`, `preset_index`, `Combo{mods,vk}` (no `MOD_NOREPEAT` inside; `register_hotkey` ORs it in), constants `MOD_*`, `WM_HOTKEY`.
- Hotkey menu: "Off", the four presets, and a "custom" entry (id 5415) when the configured text is not a preset. Clicking the
  selected custom entry retries registration. Failure text is on the parent entry and the selected item: "(in use by another
  app)" when the text parses, "(not a valid hotkey)" when it does not. `Ui.hotkey_failed` is a single bool.
- Hotkey toggle uses `Ui.mode` only: strip -> `expand`, panel -> `collapse`, morph ignored. It also works when the strip is hidden (expand shows the window).
- `update::Outcome {UpToDate, Newer(tag), Failed(String)}` and `update::decide(current, body)`; `source::get_release(url)` does the
  GET (native-tls, UA, 10 s, `redirects(0)`, `Accept: application/vnd.github+json`, 256 KiB cap, fixed error strings).
  A failed thread spawn stores `Failed("could not start")` and posts `WM_UPDATE` itself so the flag is always cleared.
- Newer-tag text in the message box drops control characters and is cut to 40 chars; nothing else from the response is shown or opened.
- Save failure shows as its own greyed line "Settings could not be saved (changes last until you quit)" under the status line (the link
  status is kept, not replaced).
- Version line text is `<profile title> <CARGO_PKG_VERSION>` (currently 1.7.1; the 1.8.0 bump is not this package's).
- Strip submenu: "Rows in the taskbar" is greyed unless the placement is on the taskbar (`dock_rows` only affects the taskbar grid). Text steps
  are named Small, Default, Large, Larger and set both `dock_font_pt` and `font_pt`. Ties in the nearest-step helpers go to the lower index.
- *Refresh now* only bumps `Board.kick`; the failure backoff counter is not reset (one attempt, then backoff continues).
- Timer 6 and `WM_APP + 10` are reserved and unused.
- `Start with Windows` shows an error message box if the registry write fails; there is no config copy of the setting.
- Unverified here: real `RegisterHotKey` behaviour, the live GitHub request, and the menu look (no display in the sandbox).
