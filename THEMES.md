# MH Ticker — the theme catalogue

Twenty-six palettes in four tiers, thirteen faces in four groups. Every one of them is a row in
`src/theme.rs`; the Theme and Font menus in `MH_Ticker_Ultra` are built from that list, so nothing
below can drift out of the menu.

The classic build ignores all of it: `theme::pick(false, …)` always returns `MIDNIGHT`, and the
Theme and Font menus are not built at all.

## The tiers are rendering rules, not labels

| Tier | `theme::Kind` | What the painter does with it |
|---|---|---|
| **Basic** | `Flat` | one solid fill |
| **Gradients** | `Gradient` | 64 vertical bands between `bg` and `bg_to`, brushes built once per theme |
| **Animated** | `Animated` | a 96-band ring cycling `bg → bg_to → bg_mid → bg`, slid by a clock |
| **Retro** | `Pixel` | 16 chunky bands, a scanline every third row, square corners, a bitmap face with antialiasing off |

Because the tier lives in `Kind`, a palette cannot claim to be animated and be painted flat: the
menu groups by `kind` and `Gradient::new` reads the same field.

## Basic — one solid fill

| Name | Idea | Font |
|---|---|---|
| `midnight` | the original dark, byte for byte | Segoe UI |
| `ink` | near-black navy, cyan accent | Segoe UI |
| `slate` | cool grey, ice blue | Segoe UI |
| `moss` | deep green, soft | Segoe UI |
| `oxblood` | wine and warm cream | Georgia |
| `taupe` | muted stone, sage | Tahoma |
| `paper` | light, low glare | Segoe UI |
| `sand` | warm paper, clay | Verdana |
| `mono` | black, white, one red | Consolas |

## Gradients — a two-colour ramp

| Name | Idea | Font |
|---|---|---|
| `graphite` | neutral grey, blue accent | Segoe UI |
| `aurora` | violet to rose, the icon's ramp | Segoe UI |
| `dusk` | indigo to plum | Constantia |
| `ocean` | teal to deep blue | Segoe UI |
| `ember` | charcoal to rust | Tahoma |
| `glass` | light, cool and airy | Segoe UI |

## Animated — the ramp drifts

One loop is **9 seconds**, repainted at **10 fps**, and only while the panel is open. The ring closes
on the colour it opened with, so there is no seam at the top or bottom edge, and the phase is read
from `Instant`, so a repaint at 3 s or at 10 fps shows the same position.

| Name | Idea | Font |
|---|---|---|
| `drift` | blue, indigo, violet | Segoe UI |
| `pulse` | charcoal, violet breath | Segoe UI |
| `vaporwave` | magenta and cyan | Consolas |
| `aurora-live` | green and violet | Segoe UI |
| `sunset` | amber through purple | Tahoma |

Cost: the motion is a different starting index into brushes that already exist, so a frame allocates
nothing. The only price is the repaint itself — and the panel now reuses one back buffer instead of
creating a ~1.2 MB bitmap per frame, which is why `MH_Ticker_Ultra` went from 6.38 MB to **5.83 MB**
private at 60 s in the same harness.

## Retro — chunky, dithered, bitmap faces

| Name | Idea | Font |
|---|---|---|
| `ega` | the 16-colour EGA palette, cyan on blue | Terminal |
| `amber-crt` | amber phosphor | Terminal |
| `phosphor` | green phosphor | Terminal |
| `gameboy` | four shades of green | Fixedsys |
| `c64` | breadbin blue | Consolas |
| `pc98` | PC-98's 16 colours with the estate's gold accent | MS Gothic |

Retro themes ask for `NONANTIALIASED_QUALITY` and a fixed pitch: ClearType smears a raster glyph
into a grey fringe and the "pixel" stops looking like one. `up`/`down` keep a red channel even in the
monochrome themes — a green CRT never had red, but a rise and a fall that look the same are worse
than a small lie about hardware.

## Fonts — thirteen faces, four groups

`theme::FACES`, overridable per panel from the Font menu (`theme_font` in the config). An unknown
name in a hand-edited config is refused and the theme's own face is kept, rather than silently
falling back to a system font at paint time.

| Group | Faces |
|---|---|
| System | Segoe UI, Tahoma, Verdana, Calibri |
| Mono | Consolas, Cascadia Mono, Courier New |
| Serif | Constantia, Georgia, Palatino Linotype |
| Bitmap | Terminal, Fixedsys, MS Gothic |

The strip keeps `font` / `font_pt` / `dock_font_pt`: a taskbar gap and a centred panel want
different sizes for the same face.

## Adding one

Append a `theme!(...)` entry to `src/theme.rs` and put it in `ALL`, in its tier's block. The menu
picks it up from `theme::tier(kind)`, and the tests fail if the name is a duplicate, if a tier drops
below five entries, if the tiers stop being contiguous, or if any palette makes a rise and a fall
the same colour.

Two rules worth keeping:

1. **`up` and `down` are semantic.** They are set separately from `accent` and a test enforces
   `up != down`, `price != bg`, `card != bg`, `accent != bg` for every palette.
2. **No cached bitmaps.** Animated and gradient paint both reuse pre-built brushes. A cached
   gradient surface is 1.2 MB and would break the footprint the app is built around.

## Regenerating the screenshots

`shots/` is generated, not drawn by hand:

```text
cargo run --release --example shots --features shots -- shots/
open shots/index.html
```

It renders every theme through `panel::draw` — the same painter the window uses — into a 760 × 394
bitmap at 96 dpi and writes a PNG. Animated themes get three frames a third of a cycle apart. The
PNG encoder lives behind the `shots` feature, so neither shipped binary links it.
