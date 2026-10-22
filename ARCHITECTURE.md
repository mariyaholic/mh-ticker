# MH Ticker — architecture

*Written 2026-10-21 after measuring the running build; revised 2026-10-22 when 1.3.0 split the crate into
a library and two binaries, and again for 1.4.0's theme catalogue. This is the structural record: what
owns what, which direction data flows, and why the boundaries sit where they do. Line counts are real.*

## Current shape (1.4.0)

| File | Lines | Owns |
|---|---|---|
| `src/ui.rs` | **1,907** | windows, message pump, strip layout + paint, **the cached back buffer**, morph animation, tray icon + tooltip, positioning, docking heal, mode transitions, the tiered Theme/Font menus, clipboard and card clicks, config application, hit-testing, hover tracking |
| `src/panel.rs` | 930 | panel geometry (`Geo`, shared by paint *and* hit tests), **`History` (price series state)**, `Scene`, GDI paint, `Fonts`, crosshair, symbol picker, the day-range bar and the 52-week badge |
| `src/theme.rs` | 640 | **26 palettes in four tiers**, gradient / animated-ring / scanline paint, the face catalogue |
| `src/dock.rs` | 522 | taskbar geometry, UI Automation lookup of the Start button, monitor enumeration |
| `src/source.rs` | 453 | ureq client, `restQuote` + chart parsers, the `Feed` thread, one-shot `post` for alerts, `--probe` |
| `src/config.rs` | 384 | `Config`, `Row`, `Alert`, `Profile`, load/save, catalogue, menu list, placements |
| `src/shot.rs` | 205 | **theme screenshots** — off-screen GDI render into a DIB, then a PNG (`shots` feature only) |
| `src/model.rs` | 189 | `Quote` (incl. day range, volume, 52-week), `Link`, `Cell`, `Board` (incl. `poll_s`), `resolve`, staleness, formatting |
| `src/alerts.rs` | 179 | threshold rules, cooldowns, tray balloon, webhook payload/redaction |
| `src/lib.rs` | 53 | the crate's public surface and `launch(profile)` |
| `src/bin/classic.rs` | 14 | `launch(Profile::classic())` |
| `src/bin/ultra.rs` | 16 | `launch(Profile::ultra())` |

5,492 lines of Rust (plus 17 in `examples/shots.rs`), of which 30 are the two entry points. `ui.rs` is
**35 %** of the crate and holds 45 functions — the extraction in the second half of this file has now
been deferred twice, and it is the next real piece of work.

### What 1.4.0 changed structurally

- **`theme.rs` became a catalogue, and the tier became a rendering rule.** `Kind::{Flat, Gradient,
Animated, Pixel}` is read by the painter *and* by the menu builder, so the menu cannot describe a
theme differently from the way it is drawn. Adding a palette is one array entry.
- **Animated backgrounds allocate nothing per frame.** `Gradient` pre-builds a 96-band ring whose last
colour equals its first, so the wrap is seamless; a frame changes only the starting index.
`sync_anim` owns the `WM_TIMER` and exists only while the panel is open on an animated theme — the
strip and every static theme pay nothing.
- **`paint` reuses one back buffer.** When a repaint was a poll every 3 s and a hover move, a fresh
1.2 MB `CreateCompatibleBitmap` per frame was survivable. At 10 fps it is not. `Back` owns the DC and
the bitmap, restores the DC's original object before freeing, and is rebuilt only on resize. Only the
morph captures still allocate per frame, and there are ~14 of them per transition.
- **The card became a layout.** 236 × 150 now holds the price and currency, the change and the absolute
move, the previous close (only when it fits), the hour of line with session markers, a day-range bar,
the 52-week range with its date, and volume in the header — all from the response the app was already
downloading. `prev` is dropped rather than clipped when the change text would collide with it.
- **The screenshot tool is a seam, not a feature.** `src/shot.rs` draws through the *real* `panel::draw`
into a DIB and encodes a PNG. That is what makes "the themes look right" checkable, and it is the only
thing in the repo that needs a PNG encoder — hence `shots`, the crate's one feature.

## One engine, two products

The most important structural fact in the crate is now this: **the two builds differ by data, not by
code paths.** Both binaries link the identical library and call the identical `launch`. What differs is
`config::Profile` — a plain struct chosen at the top of `main`. The crate does have exactly one feature,
`shots`, and it exists to keep the PNG encoder out of both binaries; nothing that ships reads it:

```rust
Profile { id: "MH_Ticker",       config_file: "config.json",       ultra: false }
Profile { id: "MH_Ticker_Ultra", config_file: "config_ultra.json", ultra: true  }
```

`config::init` stores it once; `config::path()`, `ui::single_instance()` and `ui::run()` read it. That
buys three properties worth the indirection:

- **The classic build cannot drift.** Its palette is `theme::classic()`, a single `const`; its menu has
fewer entries. There is no second code path to keep in sync.
- **Fixes land in both by construction.** The gzip feature, the day-based colour, the full hour and the
  poll cadence are engine changes; they cannot be "forgotten" in one build.
- **The two run side by side** — different mutex, different config file, different window class and
  title.

The `ultra` flag gates *presence*, not behaviour: the picker is not drawn, the theme menu is not
appended, the alert checker is not called. Everything else is the same bytes.

The good news first: the two hardest things are already right.

- **`model.rs` is pure and tested.** No I/O, no Win32, `now` passed in as a parameter. This is why
  staleness and formatting have unit tests and everything else mostly does not.
- **`--probe` is a real seam.** A headless path through the same parsers, so network behaviour can be
  exercised without a window. Most native UI apps of this size do not have one.

## The three boundaries that are wrong, and get worse with every feature

### 1. The painter owns the only live state the user can interact with (halved, not closed)

**1.3.0 added the query it needed**: `History::sample(i, t)` returns the newest point at or before a
time, which is what the crosshair reads, and `History` now has a test for it. But the series is still
*inside* the paint module, and `day_baseline` is still a free function in `panel.rs`. Moving it to
`series.rs` is still the right next step — hover made the lack of a time-axis API visible, it did not
remove it.

`panel::History` is a 3,600-point-per-row series that lives *inside the paint module*. It is created
in `ui.rs`, recorded from `ui.rs`, seeded from `Board::backfill`, and read back only through
`History::row(i)` — which is private to `panel.rs`.

That was defensible while the only consumer was a polyline. It stops being defensible the moment you
add what was asked for this round:

| Feature | What it needs from the series |
|---|---|
| Hover readout | the nearest sample to an arbitrary time, and the time of each sample |
| Day-based colour | the day's open value, resolved to the series' own time base |
| Notifications | the last value, and the value one poll earlier |
| 24 h ribbon | a second, decimated series with its own retention rule |

All four want the same thing: **a queryable series per row with a time axis**. Today that object is
spelled "`VecDeque<(f64, f64)>` behind a private accessor in the module that draws it", and there is
no way to ask it a question (`at(t)`, `day_open()`, `range(t0, t1)`).

### 2. ~~The palette lives in the window module~~ — **fixed in 1.3.0**

The palette is now `theme::Theme` (14 fields, 5 built-ins, one resolution point) and every painter
takes `&Theme`. `panel.rs` no longer imports anything from `ui.rs`. The gradient lives in
`theme::Gradient`, which caches 64 `HBRUSH`es **per theme, not per frame** — the obvious
implementation (fill 64 bands, creating a brush per band) would have allocated and freed 64 GDI
objects on every repaint, and caching them in a bitmap instead would have added ~1 MB of private
memory for a purely decorative effect.

What the old shape looked like, for the record:

`src/ui.rs:80-86`:

```rust
pub(crate) const BG: COLORREF = rgb(18, 20, 24);
pub(crate) const LABEL: COLORREF = rgb(140, 148, 160);
pub(crate) const PRICE: COLORREF = rgb(235, 238, 242);
pub(crate) const DIM: COLORREF = rgb(105, 110, 118);
pub(crate) const UP: COLORREF = rgb(80, 200, 120);
pub(crate) const DOWN: COLORREF = rgb(235, 90, 90);
pub(crate) const AMBER: COLORREF = rgb(240, 180, 60);
```

and `src/panel.rs` opens with `use crate::ui::{link_color, link_name, BG, DIM, DOWN, LABEL, PRICE,
UP, AMBER};` while keeping three more of its own (`CARD`, `BTN_HOVER`, `GUIDE`) at `panel.rs:25-27`.

**The painter depends on the window for its colours.** Ten colours, two modules, all `const`, no
indirection, and the dependency points the wrong way (presentation → window management). Theme
settings are simply not expressible here, and every colour a theme needs — gradient stops, crosshair,
badge fills, hairline borders — would deepen the knot.

### 3. `ui.rs` is the god module, and every new feature lands in it

38 functions spanning: window class registration, the message pump, `layout_row`/`layout_grid`/
`layout`/`Frame`, `draw_strip`, `draw_panel`, `snapshot` (double-buffer), `draw_morph` + `run_morph` +
two easing curves, `tray`/`tray_delete`/`make_icon`/`tooltip`, `start_pos`/`corner`/`strip_place`/
`relayout_and_place`/`panel_rect`, `dock_check`/`heal`/`own_by_taskbar`, `expand`/`collapse`,
`menu`, `apply_placement`/`apply_rows`/`apply_visibility`/`set_topmost`, `on_feed`, `paint`.

**1.3.0 made this worse, honestly counted:** 1,357 → **1,688 lines** (+331) for hover tracking, the
picker toggle, `apply_theme`, `apply_picker`, three new submenus and the alert wiring. The prediction
("it will pass 1,800") is still on track. `menu.rs` + `tray.rs` remain the highest-value extraction,
and moving the theme submenu next time is cheaper than moving four const-range `match` arms later.

### 4. (Cheap to fix now, expensive later) the board is deep-cloned every 3 seconds

`src/model.rs:51`:

```rust
pub backfill: Vec<Vec<(f64, f64)>>,
```

and `ui.rs:611` / `ui.rs:1033`:

```rust
if let Ok(b) = u.board.lock() { u.view = b.clone(); }
```

Every feed publish — **every 3 seconds** — clones every `Cell`, every `Quote` (which owns two
`String`s), and the entire bar backfill. Today that is 6 rows × ~61 points. With the full hour kept
for 12 rows, plus per-bar OHLC for candles, plus a 24 h ribbon, it becomes the app's single largest
steady allocation, purely to hand the painter a snapshot.

## Target structure

```
src/
  main.rs        entry: config -> board -> app::run                  30 lines, unchanged
  config.rs      Config/Row + load/save/catalogue                    + alert & theme fields
  model.rs       Quote, Cell, Board, resolve, staleness, fmt         + open/high/low/vol/yrhi
  source.rs      ureq client, parsers, Feed thread                   + gzip
  series.rs   NEW  History + time axis + sampling/retention policy
  theme.rs    NEW  Palette + built-in themes + resolution
  alert.rs    NEW  threshold rules, cooldowns, quiet hours (pure)
  panel.rs       panel geometry + GDI paint only                     (reads &Palette, &History, HoverAt)
  strip.rs    NEW  strip window, layout, paint
  menu.rs     NEW  popup menu + command enum
  tray.rs     NEW  icon, tooltip, balloons
  app.rs      NEW  Ui state, message pump, mode/morph                (what ui.rs becomes)
  dock.rs        unchanged
```

Data flow — one direction, one owner per fact:

```
source::Feed ──writes──> Board  (one Mutex; payloads behind Arc)
                           │
                  ┌────────┴─────────┐
          app::Ui::view        series::History      (derived: live samples + seeded bars)
                  │                  │
          theme::Palette      alert::Rules ──> tray::notify
                  │                  │
          strip::paint        panel::paint(hover: Option<HoverAt>)
```

Rules that keep it honest:

- **`source` owns bytes → data.** Parsers stay unit-tested and take a `&Value` (or later a typed
  struct) with `now` passed in. No Win32.
- **`series` owns time-series maths.** Sampling cadence, retention, seeding, queried by time. No GDI,
  no config, no window handles. This is where `at(t)`, `day_open()` and `range()` live, and it is
  unit-testable the same way `model.rs` is.
- **`alert` owns policy.** Thresholds, cooldowns, quiet hours, "should this fire". Pure functions over
  `&Board` + `&Config` + `now`. It must be testable without a tray icon, and firing must be a
  separate one-line call from `app`.
- **`theme` owns colour.** A `Palette` struct with the ~14 fields the UI actually needs, a `Palette::default()`
  equal to today's values, named built-ins, and one resolution point from config. Painters take
  `&Palette` and never define a colour.
- **Painters own pixels only.** `strip` and `panel` take state + palette + hover, and draw. They must
  not read config, spawn threads, or call `Shell_NotifyIcon`.

## Migration order (each step behaviour-preserving)

| # | Step | Risk | Why in this order |
|---|---|---|---|
| 1 | **`theme.rs`** — lift the 10 colours into `Palette`; painters take `&Palette`; default equals today | very low | Pure mechanical move, same `COLORREF` values, and it unblocks the theme feature. Do it before adding any colour. |
| 2 | **`series.rs`** — move `History`, `seed`, `WINDOW_S`/`HIST_CAP`/`SAMPLE_S`/`BAR_DEDUP_S` out of `panel.rs`; add `at(t)`, `day_open()`, `range(t0,t1)` | low | The existing `panel::tests` for seeding and rolling move with it. Unblocks hover, day colour, and the 24 h ribbon. |
| 3 | **`Arc` the payloads** — `backfill: Arc<[Arc<[(f64,f64)]>]>`, and `Option<Arc<[Bar]>>` for candles | low | Kills the 3-second deep clone before the series get bigger. |
| 4 | **`menu.rs` + `tray.rs`** out of `ui.rs`; replace the `CMD_*` const arithmetic with one `enum Cmd` | medium | Pure moves, but the command range arithmetic (`CMD_SYM + index`, `CMD_POS + index`) is the fiddly part. Do it *before* adding an alert and a theme submenu, so the new entries go into the enum, not into more const arithmetic. |
| 5 | **`app.rs`** — rename what is left of `ui.rs`; `ui.rs` keeps only `run` and `single_instance` | low | Cosmetic, do it last so the moves above have already settled. |

After each step: `cargo test` (20 tests today), `MH_Ticker.exe --probe`, and one real launch with the
panel opened. Step 1 and step 2 must produce **pixel-identical output** — they are moves, not edits.

## Non-goals

- **Don't restructure `model.rs`.** It is pure, tested, and 170 lines. Nothing about it is in the way.
- **Don't split `dock.rs`.** It is dense Win32 with its own tests and a narrow public surface
  (`rect`, `monitors`, `autohide`, `set_paused`). Splitting it further would be ceremony, not clarity.
- **Don't introduce a trait or dependency injection for the HTTP client** until something genuinely
  needs a fake. `--probe` is the seam today and it is enough.
- **Don't move the animation machinery out of the app module** until there is a second thing that
  animates. One consumer is not a shared concern.
