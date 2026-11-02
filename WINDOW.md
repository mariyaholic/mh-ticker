# MH Ticker — the full window

*Design, 2026-11-02. Nothing here is built yet. Every number marked "measured" comes from the tree at
`48cb417`; every number marked "to verify" is an estimate that a phase has to prove on Windows.*

Maria asked for two compositions — a working window around 1180×760 and a wall around 1600×1000 —
and for both to rescale to whatever the machine actually offers. This is how a window that holds
more than one panel should be built, given what `ui.rs` already is.

## 1. Why the window is 760×394

It is not a taste. `panel::geometry` computes:

```
w = 2·16 + 3·236 + 2·10 = 760
h = 48 + 2·150 + 10 + 36 = 394
```

236×150 is the card size that fits the price, the change, the previous close, the hour line with
session markers, the day-range bar, the 52-week range and the volume, with no text clipped — 150 and
236 were both raised from rounder numbers for exactly that reason (see the comment at
`panel.rs:212`). The window is the card grid plus padding. Twelve cards give 760×654.

So "a bigger window" is not a resize problem. Nothing in the current layout knows what to do with
space it did not ask for: `geometry(n, catalog, dpi, picker)` takes a **count** and returns one
`Geo`, and `draw` fills it. Give it 1600×1000 and you get six cards in a 760×394 corner with dead
pixels around them.

## 2. What a window is, once it holds panes

One new module, `workspace.rs`, owning the composition and nothing else — no GDI, no HWND, no
config. It answers one question: *given a composition and a rectangle, where does everything go?*

```rust
pub enum Pane { Watchlist, Chart, Rail, Timeline, Wall }

pub struct Node {
    pub id: PaneId,
    pub pane: Pane,
    pub weight: f32,        // share of leftover space, not of total
    pub fold: Option<Fold>, // what this becomes when it cannot fit
}

pub enum Split { Row, Column, Grid { cols: usize } }

pub struct Compo { pub root: Split, pub kids: Vec<Node> }

/// Solved in device-independent pixels, then scaled by dpi/96 once.
pub fn solve(c: &Compo, area: Rect, dpi: i32) -> Vec<(PaneId, Rect)>
```

The solver is a two-pass splitter: give every pane its minimum, distribute the remainder by weight,
then clamp anything still under its minimum and redistribute once. Deterministic, pure, and
unit-testable with no window — which is the whole point, because `ui.rs` is already 1,979 lines and
must not grow another 400.

**Constraints live on the pane, not in the layout.** Each pane declares `min: (w, h)`; the solver
never invents a size a pane cannot draw at.

## 3. Four compositions, chosen by fit

Breakpoints are named compositions, not magic numbers, and they are chosen by what *fits*, not by
what looks nice on a design canvas.

| Composition | Needs (DIP) | Watchlist | Centre | Right | Bottom |
|---|---|---|---|---|---|
| `Compact` | 760×394 | 3 cols × 2 | — | — | — |
| `Working` | 900×600 | 3 cols, scrolls | Chart 1× | Rail folded | — |
| `Wide` | 1160×700 | 3–4 cols | Chart 1× | Rail 340 | Timeline 96 |
| `Wall` | 1500×860 | 1 col ticker | Wall grid 2×3 | Rail 340 | Timeline 96 |

`Compact` is today's strip and panel, byte for byte. That is an acceptance test, not an aspiration:
the existing `shots/*.png` must come out identical, because they are the only pixels anyone has
agreed on.

**What "most Windows PCs" actually means.** Steam's primary-display survey is the best public
sample, and it says 1920×1080 at ~50 %, 2560×1440 at ~22–27 %, 4K ~5 %. Converted to DIPs after the
taskbar and Windows scaling:

| Screen | Scaling | Work area (DIP) | Compact | Working | Wide | Wall |
|---|---|---|---|---|---|---|
| 1920×1080 | 100 % | 1920×1032 | ✓ | ✓ | ✓ | ✓ |
| 1920×1080 | 125 % | 1536×816 | ✓ | ✓ | ✓ | ✗ |
| 1920×1080 | 150 % | 1280×672 | ✓ | ✓ | ✗ | ✗ |
| 2560×1440 | 100 % | 2560×1392 | ✓ | ✓ | ✓ | ✓ |
| 2560×1440 | 125 % | 2048×1104 | ✓ | ✓ | ✓ | ✓ |
| 2560×1440 | 150 % | 1707×912 | ✓ | ✓ | ✓ | ✓ |
| 3840×2160 | 200 % | 1920×1032 | ✓ | ✓ | ✓ | ✓ |

Work area = screen ÷ scaling, minus a 48 DIP taskbar. That table is the requirement, in full:
**every composition except Wall must fit a 1080p laptop at 125 %**, Wall is for 1080p at 100 % or anything 1440p and up, and nothing may fall through the floor
below Compact. If a composition does not fit, it folds — it does not scale the type down.

## 4. The scaling law: layout scales, type does not

A 236×150 card at 150 % is 354×225 physical pixels holding the same 9 pt text. Shrinking the card to
fit more of them would clip the 52-week line and the day-range bar, which is what the "150, not 132"
comment exists to prevent.

Therefore:

- **Fonts never scale with the window.** Point sizes are fixed; `dpi/96` is the only multiplier, as
  today.
- **Counts scale.** Watchlist columns = `floor((w − 2·pad + gap) / (card_w + gap))`, clamped 1…6 —
  which means `panel::COLS`, today a `const 3`, becomes a parameter of the same name. At 1600 wide
  that is 6 columns; at 900 it is 3.
- **Panes fold.** `Rail` collapses to a 44 px icon strip below 1160 wide, and to nothing below 900.
  `Timeline` hides below 700 tall. `Wall` becomes a single `Chart` below 1500 wide.
- **Splitters are draggable** where they exist, and the dragged ratio persists per composition.

The one place type does change is the **wall ticker column**, which is deliberately one card wide and
draws the price larger — a different renderer, not a scaled one.

## 5. Paint: the buffer is per-dirty-region, not per-window

`Back` is a window-sized compatible bitmap kept between frames and rebuilt on resize
(`ui.rs:220`). That is correct for 760×394 — 1.14 MiB. It is not correct for a wall:

| Client | Buffer, 4 B/px | vs the 5 MB estate rule |
|---|---|---|
| 760×394 | 1.14 MiB | fine |
| 1180×760 | 3.42 MiB | fine |
| 1600×1000 | 6.10 MiB | **over, before the app allocates anything** |
| 1920×1032 | 7.56 MiB | over |
| 2560×1392 (1440p@100 %) | 13.6 MiB | far over |

So the buffer must be bounded by what changed, not by what exists:

```rust
struct Band { dc: HDC, bmp: HBITMAP, rect: Rect, size: (i32, i32) }

/// One frame may need several disjoint regions (a card here, the clock there).
/// Cap the total at 2 MiB; if a frame's union exceeds the cap, paint it as sequential bands
/// and BitBlt each before starting the next.
const BAND_CAP: usize = 2 << 20;
```

Rules that follow:

- **`WM_ERASEBKGND` returns 1.** Every band is painted opaque, so Windows never erases and there is
  nothing to flicker. This is already how the app behaves in practice; make it explicit.
- **The animated background never gets a buffer.** A breath is one `FillRect`, a ramp is 96, retro is
  16 — all solid fills, all cheap, and they repaint only where the background is actually visible
  (gutters, header, footer). Cards are opaque and painted over it.
- **Content repaints on data change, not on the clock.** Quotes land every 3 s; the background ticks
  at the palette's own rate (10 fps today). Those are two different invalidation sources and must
  not be conflated — an animated background must not force a text repaint 10 times a second.
- **Hover repaints one rect.** The crosshair already quantises to the nearest sample; it invalidates
  the old and new crosshair rect only.
- **Pane-local caches only for the expensive ones.** A candle chart's grid, axes and labels are
  static between bars: one 480×300 buffer (550 KiB) is worth it. Cards are not cached — they are
  cheap to draw and expensive to store (24 cards × 135 KiB = 3.2 MiB).

Rejected: `WS_EX_COMPOSITED`, which would let DWM buffer the whole window but adds a frame of
latency, doubles memory in DWM, and interacts badly with the morph animation. It stays as a fallback
if banded painting flickers in practice — to verify, not to assume.

## 6. Window management

Today the window is created `WS_POPUP` at 10×10 (`ui.rs:1882`) and positioned by `dock.rs`. A popup
with no frame is exactly right for a docked strip and exactly wrong for a resizable workspace: it has
no resize edges, no maximise, and no Snap Layouts.

Recommended: **`WS_THICKFRAME | WS_MAXIMIZEBOX | WS_MINIMIZEBOX`, custom-drawn chrome via
`WM_NCCALCSIZE`** (return 0 to keep the whole rect as client), so the window looks identical but
behaves like a real one:

- `WM_NCHITTEST` returns `HTLEFT/HTRIGHT/HTTOP/BOTTOM/TOPLEFT/…` on a 6 DIP border, `HTCAPTION` on
  the header, `HTCLIENT` everywhere else. Wins: drag-to-edge Aero Snap, Win+arrow, double-click to
  maximise, Snap Layouts on Win 11.
- `WM_GETMINMAXINFO`: `ptMinTrackSize` = 760×394 (Compact), `ptMaxTrackSize` = the work area of the
  monitor, so maximise never covers the taskbar even with custom NC.
- `WM_DPICHANGED`: re-`solve`, rebuild `Fonts`, take the suggested rect. The handler exists; it has
  never had a layout that could change size, so it needs its own test pass.
- `WM_SETCURSOR`: `IDC_SIZEWE` / `IDC_SIZENS` over a splitter, hand over a draggable card.

**The risk, stated plainly:** the strip↔panel morph animates window geometry, and a framed window
may fight it. Phase 1 therefore ships the frame behind a config switch (`frame: "system" | "popup"`)
so the old behaviour is one restart away, and the morph is re-shot at both.

**Persistence.** `config.json` gains:

```json
"window": { "monitor": "\\\\.\\DISPLAY1", "x": 120, "y": 80, "w": 1600, "h": 1000,
            "maximized": false, "compo": "wall", "splits": { "watchlist": 0.22, "rail": 0.20 } }
```

On startup the rect is clamped into that monitor's work area; a monitor that no longer exists falls
back to the primary. `dock.rs` already enumerates monitors and computes work areas — reuse it.

## 7. Panes

Each pane owns its rectangle, its hit test and its paint call. The window owns placement only.

- **Watchlist** — today's card grid. `min 3×2 cards`. Grows by columns, then scrolls. Click, hover,
  crosshair, picker chips unchanged.
- **Chart** — candles or a line from the bars the app already downloads. `source.rs:125` keeps only
  `close` and the timestamp; if `priceBars` carries `open/high/low` — **to verify against a live
  payload**, the fixture does not — keeping them as `f32` is +12 B per bar, ~4 KB per symbol per
  hour. A line chart needs nothing new. `min 380×260`. Crosshair reads `History::sample`, which exists. Timeframe (1 h / 1 d)
  switches which series it reads, not which request it makes.
- **Rail** — alerts and portfolio on tabs, `min 300×240`, folds to icons at 44 px. Alerts already
  exist (`alerts.rs`); portfolio is config plus arithmetic over existing quotes.
- **Timeline** — one row per venue across 24 h, `min 900×72`. Pure `Time` maths, no new traffic.
  The session-timers branch cannot be merged (it shares no history with `main`) but its logic is the
  reference.
- **Wall** — a grid of Chart panes, `min 2×2 at 380×260 each`. Same renderer as Chart, one scene per
  symbol; this is what makes 1600×1000 honest rather than empty.

## 8. Input

`panel::hit` becomes `workspace::pane_at(&frames, x, y) -> Option<PaneId>` followed by the pane's own
hit test, so one entry point routes clicks, hover and wheel. The context menu stays window-wide.
Splitter drags are a three-message protocol (`WM_LBUTTONDOWN` on `HTCLIENT` over the splitter,
`WM_MOUSEMOVE` while captured, `WM_LBUTTONUP`), and the ratio is written on release, not per move.

## 9. Phases, each shippable

| Phase | Work | Acceptance |
|---|---|---|
| P0 | `workspace.rs` + `solve()`, `geometry()` reimplemented on top | **existing `shots/*.png` byte-identical**; solver tests for every composition |
| P1 | frame, resize, min/max, DPI, persistence, splitter drag | window resizes from 760×394 to work area; Compact identical; morph re-shot |
| P2 | banded paint, no full-window buffer | peak private RAM < 5 MB at 1600×1000; no flicker on resize or hover |
| P3 | Chart pane | candles from existing bars; crosshair; timeframe switch |
| P4 | Rail + Timeline | folds at the documented widths |
| P5 | Wall composition | 2×3 charts live at 1600×1000 with 6 symbols |

P0 and P1 are worth doing even if nothing else lands: they make the window behave like a window.

## 10. Budget

| | Today | Compact | Working | Wall |
|---|---|---|---|---|
| Private RAM | 4.45–4.69 MB | unchanged | +~0.2 MB (chart series) | +~0.4 MB |
| Paint scratch | 1.14 MiB (one buffer) | 1.14 MiB | ≤ 2 MiB (band cap) | ≤ 2 MiB |
| Binary | 933 KB | +10 KB | +40 KB | +60 KB |
| Network | 3.35 MB/h (gzip) | — | unchanged | unchanged |

The estate's 5 MB rule survives all of this **only** because the buffer stops being window-sized.
That is the load-bearing decision in this document.

## 11. Open questions for Maria

1. **Wall on 1080p**: at 125 % scaling it does not fit (1536×816 vs 1500×860). Do we ship Wall only
   where it fits and fold to `Wide` elsewhere — my recommendation — or add a compressed Wall that
   drops the rail and timeline to reach 860 tall?
2. **Chart default**: candles or a line? Candles need `o/h/l/c` kept per bar (+16 B/bar); the line
   needs nothing new.
3. **Rail order**: alerts first or portfolio first? It decides the tab that opens.
4. **Frame switch**: acceptable to make the system frame the default in P1, with `frame: "popup"` as
   the escape hatch, or must the strip stay a popup and only the workspace be framed?
