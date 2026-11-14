# W3 brief: Twenty-four symbols (pin, undo, find, order; the cache and the range header Ultra)

*The writer's brief for the third 1.9 package, prepared by the orchestrator from
[../1.8/05-next-features.md](../1.8/05-next-features.md) items J1, J2, J3, J6, D5, L1, L3 and B11a
and package W3. The workflow is [01-workflow.md](01-workflow.md).*

## Why

1.8 raised the cap to 24 and let any symbol in. This package is what that user meets next: a strip
too crowded to read, cards hard to find in a panel taller than the screen, a Delete key with no way
back, labels cut at 8 characters, and a chart that forgets its range and leads a five-year view with
today's change. Pin to strip is the core; the rest are small and mostly reuse P5's order functions.

## What to build

| Item | What | Build | Pure part |
|---|---|---|---|
| J1 | Pin to strip: the taskbar strip shows the symbols you pick, the panel shows all of them. `strip` per row, absent means shown, so Classic and old configs are unchanged. The last pinned symbol cannot be unpinned. *Show all on strip* undoes it | both | `strip_rows(config)`; P5's click rectangles map through it to the right card |
| J2 | Undo the last removal, move, sort, pin change or alert clear: Ctrl+Z in the panel and one menu line. One snapshot in memory, replaced by the next such action, gone at quit. A removed webhook is never restored | both | `undo::take`, `restore` |
| J3 | Rename a card and set its decimals: a label and a digits override per symbol. *Reset name* and *Decimals: auto* undo them. Levels already set keep their value | both | P6's label rules reused; collision suffix; digits clamp |
| J6 | Find a card: `/` opens a prompt and locks the first match; Home and End lock the first and last card; the body scrolls so the locked card is in view. Esc in the prompt changes nothing | Ultra | `find(rows, text)` (prefix before substring, case ignored); `scroll_to_show` |
| D5 | Order the cards once: by day move, by name, or back to the default order. A one-shot rewrite of the order P5 saves; a live sort stays LATER | both | `order_by(config, quotes, key)`; rows with no fresh quote keep their relative place at the end |
| L1 | The pane header shows the change over the range on screen (`5Y +182 %`) and the range's high and low are marked. 1D unchanged | Ultra | `range_change(bars)` with P2's rules for a zero or non-finite open |
| L3 | The series cache holds 48 series, not 12 | Ultra | eviction order; a series off screen is never due |
| B11a | The chart reopens on the range last used: `chart_range`, default `1D` | Ultra | round trip; an unknown value reads as `1D` |

## Acceptance (the checker runs each), word for word from 05, W3

- With no `strip` field every row is on the strip and the existing strip stills are identical. One
  row is always shown. A click rectangle maps to the right card when some rows are unpinned. A new
  still shows 24 symbols with 6 pinned.
- Undo restores the exact config after a remove, a move, a sort, a pin change and an alert clear. It
  never restores a removed webhook. A second undo does nothing.
- Rename follows P6's label rules; a taken label gets a number; Reset returns the derived label.
- `find` prefers a prefix, ignores case and leaves the lock alone on no match. `scroll_to_show` is
  tested at the top, the bottom and for a card taller than the view.
- `order_by` is stable, puts rows with no fresh quote last, leaves hidden symbols in place, and P5's
  "rows keep catalogue order" test still passes.
- `range_change` returns nothing for a zero or non-finite first open. The 1D header still matches its
  P2 still.
- The cache holds 48 on Ultra, evicts least recently used first, never marks a series off screen as
  due, and 48 full series come to under 1.3 MB by size arithmetic in a test.
- `chart_range` round-trips; an unknown value reads as `1D`.

Plus the standing checks in [01-workflow.md](01-workflow.md).

## Decisions taken (orchestrator, with the owner's autonomy for 1.9)

- J2's snapshot holds the whole config minus the webhook; restoring never touches the webhook field,
  so an undo after a webhook removal leaves it removed.
- Renaming changes the label alerts are keyed by: alerts and the lock must follow the rename (one
  rename function moves them), and a test pins it.
- L1's high and low marks must not collide with W1's "as of" and "check dates" header text, nor with
  the alert level labels from 1.8; the checker will look at a 5Y picture with levels, flags and marks.
- Home and End already page the body in 1.8 (P5 left them "unchanged"). With W3 they lock the first
  and last card on Ultra when the panel has cards; on Classic they keep paging. Say this in the notes.

## Lessons carried from W1 and W2

- A key that opens a prompt (J6's `/`) does so outside `with_ui`; focus after the prompt closes is
  unverified and the notes say so.
- Any change to rows goes through `apply_rows`; any save through `save_cfg()` outside `with_ui`.
- When a choice needs a Cargo feature, say so in the report instead of working around it; the checker
  can enable a `windows-sys` feature.
