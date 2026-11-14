# P2 "A chart that reads": builder hand-off

Branch `wip/p2-chart`. The skeleton, all painter and Win32 wiring are done and verified
(`check` clean; `test`: 112 pass, 9 ignored, only the known Wine failure; `stills-check` 36/36 identical;
chart fixture renders, with and without a hovered bar). 12 `TODO(finish)` markers remain, all in
`src/chartview.rs`.

## Finisher: do in this order

1. `chartview::gaps` body (line ~140). Then its test `gaps_mark_the_candle_after_a_session_break`.
   The painter already calls it and draws a dotted line half a slot left of each index.
2. `chartview::bar_change_pct` body + test `a_candles_change_is_close_over_open`.
3. `chartview::readout` body to the full contract in its doc comment (replace the placeholder, which
   prints `HH:MM  C price  +x.xx%`), then tests `the_readout_names_time_ohlc_and_change` and
   `the_compact_readout_keeps_time_close_and_change`. The painter passes `compact = true` only when the
   full line is wider than the pane.
4. Replace each remaining `#[ignore = "TODO(finish)"]` test stub with its real body (names and key
   assertions are in the comments): `a_reference_outside_the_window_is_not_drawn` (`in_window`),
   `positive_drops_zero_and_non_finite`, `folding_lands_candles_on_local_boundaries` (+05:30 and -03:30),
   `the_clock_reads_the_zone_and_wraps_midnight`, `fitted_with_an_offset_chooses_the_same_bucket`.
   `in_window`, `positive`, `clock`, `fold`, `fitted` are already implemented; these are tests only.
5. Edge cases the brief asks for: empty, zero, NaN, one element, the boundary itself, on the functions
   above and on `Layout` (zero bars, `step` 0 is already guarded in `index_at`).
6. Render `... --example chart ... -- target/chart-out midnight 96 1488 4 40` and look at the legend
   (needs step 3); also at dpi 144 and width 1130 (narrow pane: day change should drop, legend should
   go compact, nothing overlaps).
7. `clippy`, `scripts/check-state-linux.sh`, `stills-check`, commit.

## Must not change

- `Layout`, `Pointer`, `Rect` shapes and the `Layout` method semantics (painter, pointer and the
  fixture all use them). `Layout`/`Pointer` stay `Copy`; `chartview` stays `Bar` + `std` only.
- `fold`/`fitted` with `tz_offset_s == 0` must stay byte-identical to the old output.
- Signatures of `gaps`, `readout`, `bar_change_pct`, `in_window`, `positive`.
- The `wndproc` / `Act::Zone` / reset wiring in `ui.rs`; `Scene::layout` / `Scene::chart_at`; `shot.rs`
  fixtures (they keep `stills-check` identical).

## Decisions the brief did not make

- **Legend placement:** the pane header is now two lines (`plot.top` is `r.top + 50 px`, was 34). Line 1:
  label, bucket, zone button on the left; day change (small) then last price on the right. Line 2 is the
  hover legend, reserved even when idle so the plot does not move when the pointer arrives. If the full
  legend is wider than the pane the compact one is used.
- **Day change** is dropped (not the zone) when it would touch the zone button on a narrow pane.
- **Zone button** is the underlined accent-coloured word; its rectangle is `Layout::zone_btn`. Click
  arm is `Act::Zone`, placed right after the chart-button arm (before the picker/Open/card arms). It
  needs `u.chart` and a stored layout. The flip saves outside the `with_ui` closure.
- **Zone words:** "local" / "UTC" (`chartview::zone_label`). Config field `chart_utc` (default false).
- **Reference lines** drawn under the candles: previous close dashed (`t.dim`) with "prev" label inside
  the plot at its left edge; day high/low are 8 px ticks at the plot's right edge, over the candles, no
  label. All via `in_window(positive(..))`, so a value outside the bars' padded window draws nothing.
- **Crosshair** is dotted `t.accent`; the hover price tag is `t.accent` and is drawn after the last-price
  tag (it may cover it while the pointer is on that row).
- **y quantisation:** `CHART_Y_BAND` = 2 px at 96 dpi, scaled by DPI, counted from `plot.top`.
  `pointer_at` returns `None` outside the plot or past the last candle, which clears the crosshair.
- **Stale pointer:** the painter clamps `Pointer::bar` to the bar count and ignores a `y` outside the
  plot (a resize or zone flip can refold candles between frames). The zone flip also clears `chart_at`.
- **Layout cell resets:** cleared by `draw` at the start of every frame (so a frame without the pane
  leaves none), by `candles` on early return, and with each `chart_at` reset (set_chart, Select, apply_rows,
  expand) plus after collapse's morph (the morph snapshots paint at other sizes and also overwrite the cell).
- **Fixture:** `write_chart_named_at(dir, name, dpi, w, hover_card, hover_bar)`; the example's 6th argument
  is the hovered candle (index among the candles as drawn). The fixture runs at a fixed UTC-4 labelled
  "local" so the picture is identical on every machine. The pointer is aimed from a first paint's stored
  layout, as the app does.
- `local_offset_s` / `offset_from_bias` in `ui.rs` (with a small test module under them). Id 0 and 1 are
  standard time, 2 daylight, anything else gives 0.
- Menu IDs 5300-5349, `WM_APP + 8` and timer 5 are unused.

## Unverified

- Real-mouse behaviour under Windows (only the painter and pure code are exercised here).
- DST edge: `GetTimeZoneInformation` is read each paint, so a change at the DST boundary applies on the
  next repaint, but already-fetched bars are refolded then, not cached.
