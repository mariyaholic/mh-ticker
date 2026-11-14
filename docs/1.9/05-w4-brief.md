# W4 brief: Reach (Ultra unless marked)

*The writer's brief for the last 1.9 package, prepared by the orchestrator from
[../1.8/05-next-features.md](../1.8/05-next-features.md) items M1, M2, M3, M4, M5, M6, G3 and M7
and package W4. The workflow is [01-workflow.md](01-workflow.md).*

## Why

The panel can be driven by keys since 1.8, but the chart cannot, some settings are still preset-only,
and nothing in the app has ever been checked for contrast, colour meaning, size or motion. This
package finishes the keyboard, lets Ultra follow a Windows contrast theme and the animation setting,
offers the up and down colours other markets and other eyes need, adds panel zoom, and gives screen
readers one sentence to read.

## What to build

| Item | What | Build | Pure part |
|---|---|---|---|
| M1 | The chart by keyboard: Shift+Left and Shift+Right move the crosshair, R changes the range, Z flips the zone, A sets an alert at the crosshair price through the prompt, the menu key opens the card menu, F10 the main menu, F1 lists the keys. Plain arrows keep their 1.8 meaning; a pointer move takes the crosshair back | Ultra | `keys::map` grows; `step_candle`; the help text from the same table |
| M2 | Type what is preset-only today: any hotkey (both), any quiet hours `22:30-06:45`, a mute until a time, a cooldown (Ultra). No new fields; a bad entry is refused with a fixed sentence and changes nothing | both / Ultra | parsers for a time span and a time; P3's hotkey grammar unchanged |
| M3 | High contrast: in a Windows contrast theme the app draws in the system's colours, and direction is carried by sign and shape as well as colour. `contrast`: `auto` on Ultra, `off` on Classic | Ultra follows Windows; Classic one tick, off | `Theme::from_system(colours)` |
| M4 | Choose the up and down colours: green/red, red/green, blue/orange; an option for hollow up-candles. `updown`, default as today | Ultra | the palette swap touches only the two colours |
| M5 | A contrast test over every theme: text against background at 4.5 to 1 or better, up and down against background at 3 to 1 | test work | all of it |
| M6 | Reduced motion: with Windows animations off, the panel opens and closes in one step. `animate`: `auto` on Ultra, `on` on Classic | Ultra follows Windows; Classic one tick, on | `frames(animate, system)` |
| G3 | Panel zoom: 75, 90, 100, 125, 150 %. `panel_zoom`, default 100. Effective DPI is DPI times zoom | Ultra | the still-equality test |
| M7 | A first step for screen readers: the window's name carries the locked card as a sentence (`Gold 4,012.30, up 0.4 percent`, `stale` when it is), or a one-line summary with no lock | Ultra | `spoken_line(row, quote)` |

## Acceptance (the checker runs each), word for word from 05, W4

- `keys::map` is tested for each new key with the chart open and closed, with and without a lock. P5's
  tests pass unchanged. Every action has a help line. A crosshair set by key and one set by pointer
  give the same layout index.
- Quiet hours `22:30-06:45` parses, wraps midnight, and equal ends mean off. Bad entries are refused
  with fixed sentences and change nothing. The hotkey prompt uses P3's grammar as it is.
- The contrast theme is rendered from two fixed system palettes. Every text and background pair in it
  is a pair the system defines. Up and down rows differ by a character, not only by colour.
- The contrast test passes for every Ultra theme, with each tuned still named in the note. Classic's
  ratios are printed in the note and its stills are identical.
- Each up and down palette has a still. `red_green` changes only the two direction colours.
- `frames` returns one step for off, and Classic's default is on.
- Zoom 150 % at 96 DPI is byte for byte the still at 144 DPI and 100 %. Zoom 100 % matches the
  baseline. P2's `index_at(x_of(i)) == i` holds at every zoom step.
- `spoken_line` is tested for up, down, zero, stale and no quote. The window-name call is reviewed by
  code only, and the note says so.

## Decisions taken (orchestrator, with the owner's autonomy for 1.9)

- **Classic stays as it is by default.** Contrast is `off` and animation `on` for Classic unless the
  user ticks them, so Classic's pixels and behaviour do not move. This fails some Classic users who need
  contrast most; the README must say the tick exists and what it does.
- **M5 tuning is limited.** Measure every theme. Tune only where text against its background fails
  4.5 to 1, or up and down against background fail 3 to 1, and change as little as the ratio needs.
  Each tuned theme is named with its before and after ratios. Classic is measured and reported, never
  changed. **Ultra theme stills that change because of tuning are the one exception to "the 36 stills
  stay identical"**: list every one in the report; the checker looks at each before and after and then
  updates the baseline. No other still may change.
- **M7 changes the window title**, which Windows also shows in the taskbar's thumbnail and Alt+Tab.
  Keep it short, update it at most once per feed tick, and never put a price in the title of a Classic
  window.

## Lessons carried from W1 to W3

- Run the checkpoint after every item. The W3 writer lost its first hour to a usage limit before its
  first checkpoint; the checker had to commit for it.
- A new example needs its `[[example]]` entry with `required-features = ["shots"]` in `Cargo.toml`.
  Ask the checker for it, do not leave it out.
- A header change regenerates the stills that show that header; say which.
- Any repaint must stay inside what `panel::hot_dirty` invalidates or take the full-window path; zoom
  changes every rectangle, so it takes the full-window path.
