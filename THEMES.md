# MH Ticker — the theme catalogue

Thirty-seven palettes in four tiers, twenty-two faces in four groups. Palette values are generated
in `src/theme_catalog.rs` from the shared Uika application catalog; rendering stays in `src/theme.rs`.
The original twenty-six palettes below retain their colors, fonts and motion. Eleven Yozora
identities are also available as still native palettes: Yozora, Uika, Pine, Kurisu, Ava, Astra,
Mariyaholic-98, Sumimi, Ledger, Confluence and Astra light. See
[shared catalog adoption](docs/THEME-CATALOG.md) for refresh commands and validation limits.

The classic build ignores all of it: `theme::pick(false, …)` always returns `MIDNIGHT` and never
runs a clock, and the Theme and Font menus are not built at all.

## The tiers are rendering rules, not labels

| Tier | `theme::Kind` | What the painter does with it |
|---|---|---|
| **Basic** | `Flat` | one solid fill |
| **Gradients** | `Gradient` | 64 vertical bands between `bg` and `bg_to`, brushes built once per theme |
| **Animated** | `Animated` | a 96-band ring cycling `bg → bg_to → bg_mid → bg`, slid by a clock |
| **Retro** | `Pixel` | 16 chunky bands, a scanline every third row, square corners, a bitmap face with antialiasing off |

Because the tier lives in `Kind`, a palette cannot claim to be animated and be painted flat: the
menu groups by `kind` and `Gradient::new` reads the same field.

## Motion is a field, not a tier

Every palette names a `cycle` in seconds; `None` is a still background and no shipped palette uses
it. What the motion *looks like* is the surface's decision, so a palette does not have to be copied
into an "animated" block to move:

| Surface | Moving it produces |
|---|---|
| `Flat` | **breath** — one colour chosen from a 48-step ring of the three stops, drawn as a single flat fill. The whole panel lifts and settles; the surface stays flat. |
| `Gradient` | the 64-band ramp becomes a **ring** — three segments, 96 bands, slid by the phase |
| `Animated` | the ring, at the palette's own pace |
| `Pixel` | a ring at **16 chunky bands**, plus one scanline cell of roll per cycle |

Only the three background stops move. `card`, `label`, `price`, `dim`, `up`, `down`, `amber`,
`guide`, `btn` and `accent` are fixed, so every contrast pair in the catalogue is the same at every
point of every cycle — a test enforces that no stop collides with `card` or `price`.

`SPI_GETCLIENTAREAANIMATION` is honoured: with Windows' "Show animations" switched off, every
palette holds phase 0 (its `bg`), which is also what the classic build always draws.

## Basic — one solid fill

| Name | Idea | Cycle | Font |
|---|---|---|---|
| `midnight` | the original dark | 16 s | Bahnschrift |
| `ink` | near-black navy, cyan accent | 14 s | Cascadia Mono |
| `slate` | cool grey, ice blue | 18 s | Corbel |
| `moss` | deep green, soft | 16 s | Candara |
| `oxblood` | wine and warm cream | 18 s | Constantia |
| `taupe` | muted stone, sage | 20 s | Franklin Gothic Medium |
| `paper` | light, low glare | 22 s | Sitka Text |
| `sand` | warm paper, clay | 20 s | Palatino Linotype |
| `mono` | black, white, one red | 14 s | Cascadia Mono |

Midnight's `bg` is still exactly `rgb(18, 20, 24)`: the classic build holds phase 0, so the one
theme it ships is byte for byte what it always was.

## Gradients — a two-colour ramp

| Name | Idea | Cycle | Font |
|---|---|---|---|
| `graphite` | neutral grey, blue accent | 14 s | Bahnschrift |
| `aurora` | violet to rose, the icon's ramp | 12 s | Candara |
| `dusk` | indigo to plum | 14 s | Cambria |
| `ocean` | teal to deep blue | 16 s | Corbel |
| `ember` | charcoal to rust | 14 s | Franklin Gothic Medium |
| `glass` | light, cool and airy | 18 s | Segoe UI Variable |

Weight-named instances — Segoe UI Variable *Light*, Bahnschrift *Light* — are not families in GDI,
and the panel picks a face by family name, so those proposals land on the family: the catalogue
offers the face, not the weight.

A gradient's `bg_mid` used to repeat `bg_to`, which left a ring with two stops and nothing between
them. Every ramp now carries a real third stop, and the ramp is still what a still frame shows.

## Animated — the ramp drifts

One loop is however long the palette says (6–16 s), repainted at **10 fps**, and only while the
panel is open. The ring closes on the colour it opened with, so there is no seam at the top or
bottom edge, and the phase is read from `Instant`, so a repaint at 3 s or at 10 fps shows the same
position.

| Name | Idea | Cycle | Font |
|---|---|---|---|
| `drift` | blue, indigo, violet | 12 s | Bahnschrift |
| `pulse` | charcoal, violet breath | 16 s | Segoe UI |
| `vaporwave` | magenta and cyan | 6 s | Cascadia Code |
| `aurora-live` | green and violet | 10 s | Candara |
| `sunset` | amber through purple | 14 s | Georgia |

Cost: the motion is a different starting index into brushes that already exist, so a frame allocates
nothing. The only price is the repaint itself — and the panel reuses one back buffer instead of
creating a ~1.2 MB bitmap per frame, which is why `MH_Ticker_Ultra` went from 6.38 MB to **5.83 MB**
private at 60 s in the same harness. A breath costs 49 brushes and no allocation at all.

## Retro — chunky, dithered, bitmap faces

| Name | Idea | Cycle | Font |
|---|---|---|---|
| `ega` | the 16-colour EGA palette, cyan on blue | 8 s | Terminal |
| `amber-crt` | amber phosphor | 7 s | Terminal |
| `phosphor` | green phosphor | 7 s | Lucida Console |
| `gameboy` | four shades of green | 10 s | Fixedsys |
| `c64` | breadbin blue | 9 s | Fixedsys |
| `pc98` | PC-98's 16 colours with the estate's gold accent | 8 s | MS Gothic |

Retro themes ask for `NONANTIALIASED_QUALITY` and a fixed pitch: ClearType smears a raster glyph
into a grey fringe and the "pixel" stops looking like one. `up`/`down` keep a red channel even in
the monochrome themes — a green CRT never had red, but a rise and a fall that look the same are
worse than a small lie about hardware.

## Fonts — twenty-two faces, four groups

`theme::FACES`, overridable per panel from the Font menu (`theme_font` in the config). An unknown
name in a hand-edited config is refused and the theme's own face is kept; a face this machine does
not have is refused the same way and greyed in the menu, because GDI would otherwise substitute a
font silently at paint time — the one mistake the Font menu could not otherwise show.

| Group | Faces |
|---|---|
| System | Segoe UI, Segoe UI Variable, Bahnschrift, Tahoma, Verdana, Calibri, Candara, Corbel, Franklin Gothic Medium |
| Mono | Consolas, Cascadia Mono, Cascadia Code, Courier New, Lucida Console |
| Serif | Constantia, Georgia, Palatino Linotype, Cambria, Sitka Text |
| Bitmap | Terminal, Fixedsys, MS Gothic |

Not all of these ship with every Windows: Bahnschrift is 1709+, Segoe UI Variable is Windows 11,
Cascadia arrives with Windows Terminal, MS Gothic with the Japanese pack. The enumeration at startup
is what the menu greys with.

The strip keeps `font` / `font_pt` / `dock_font_pt`: a taskbar gap and a centred panel want
different sizes for the same face.

## Adding one

Append a `theme!(...)` entry to `src/theme.rs` and put it in `ALL`, in its tier's block. The menu
picks it up from `theme::tier(kind)`, and the tests fail if the name is a duplicate, if a tier drops
below five entries, if the tiers stop being contiguous, if any palette makes a rise and a fall the
same colour, if a cycle falls outside 5–24 s, or if a background stop collides with the cards it sits
behind.

Four rules worth keeping:

1. **`up` and `down` are semantic.** They are set separately from `accent` and a test enforces
   `up != down`, `price != bg`, `card != bg`, `accent != bg` for every palette.
2. **No cached bitmaps.** Moving and gradient paint both reuse pre-built brushes. A cached
   gradient surface is 1.2 MB and would break the footprint the app is built around.
3. **Motion is a field.** Do not add a second "animated" copy of a palette to make it move; give it
   a `cycle` and let its surface do the rest.
4. **A still theme is a valid theme.** `cycle: None` draws one solid fill and starts no clock, which
   is what the classic build relies on.

## Regenerating the screenshots

`shots/` is generated, not drawn by hand:

```text
cargo run --release --example shots --features shots -- shots/
open shots/index.html
```

It renders every theme through `panel::draw` — the same painter the window uses — into a 760 × 394
bitmap at 96 dpi and writes a PNG. The animated tier gets three frames a third of a cycle apart,
since one PNG cannot show a drift; the other tiers move too, but their screenshots are stills at
phase 0. The PNG encoder lives behind the `shots` feature, so neither shipped binary links it.
