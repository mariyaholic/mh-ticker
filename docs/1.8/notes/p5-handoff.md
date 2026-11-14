# P5 hand-off (builder -> finisher)

Branch `wip/p5-keys`. The builder was stopped by a usage limit after the wiring was complete; the
orchestrator checked the tree and wrote this note. State: `wcargo check --all-features --all-targets`
clean. All Win32 and wiring code is done; only pure bodies and tests are left.

## TODO(finish), in the order to do them

1. `keys::map`: the table in its doc comment. Classic answers only Ctrl+C (`CopyAll`).
2. `keys::step`: the contract in its doc comment.
3. Delete the `#![allow(dead_code)]` at the top of `keys.rs` (and its TODO) once nothing warns.
4. `config::move_row` and `config::move_first`: the contract in their doc comments. The order is a
   permutation of `catalog`; `rows` is rebuilt the way `toggle` rebuilds it (reuse that code, do not
   copy it: factor a small private helper if needed, without changing `toggle`'s behaviour). A false
   return leaves `cfg` exactly as it was.
5. The nine `keys` tests and six `config` tests named in `TODO(finish)` comments, each with the
   assertions written beside it, plus edge cases (empty, one row, the boundary itself).
6. The five commands, clippy, `docs/1.8/notes/p5.md`.

## Do not change

- `ui::lock_card` is the one place the lock moves (click, keys, Remove, strip click). `ui::on_key`
  decides inside one `with_ui` call and acts after it. `remove_card`, `reorder`, `reveal_lock`.
- `config::remove` (never adds, refuses the last row) and the `menu_commands::Action` variants
  `Remove`, `Move`, `MoveFirst`, which carry the symbol.
- `Scene::keys_hint` is false in `shot.rs`, so the stills do not change.
- `keys::is_bound` lists exactly the keys `map` answers; Esc and paging stay in their own arms.

## Decisions the brief did not make

- Removing the locked card passes the lock to the card that takes its place (the next one, or the
  previous one for the last card).
- The strip records one `keys::Slot` per symbol while it lays out the text (`Frame::slots`); a click
  on a slot of a docked or cornered Ultra strip locks that card and expands. A click between symbols,
  the floating strip and Classic do what they did.
- The Ultra footer names the keys only when the line fits beside the left-hand text; otherwise it
  keeps the old hint.
- An unmapped key (or any key on a strip or during a morph) goes to `DefWindowProcW`.

## Unverified off Windows

Real key presses and focus (does the panel have the keyboard after a click on the strip or the
hotkey?), a real strip click on each placement, the card menu.
