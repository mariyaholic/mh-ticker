# P4 hand-off (builder -> finisher)

Branch `wip/p4-ranges`. State: `wcargo check --all-targets` clean, clippy clean, 188 tests pass (the one
known Wine failure aside), `stills-check` 36/36 identical, and the default `examples/chart.rs` render is
byte-identical to the one from before this branch. Pictures: `p4-5d.png`, `p4-1m.png` (with volume),
`p4-5y.png`, `p4-1d-volume.png` (rendered by the builder; re-render if the painter changes).

## TODO(finish), in the order to do them

1. `ranges::median_spacing` (stub returns 0.0). Contract in its doc comment. Test
   `bar_s_is_the_median_spacing` (named in the test module).
2. `ranges::Series::window` (stub returns every bar). Partition point on `Bar::t(t0)`. Test
   `window_takes_the_tail_inside_the_span`.
3. `source::parse_series` (stub returns an empty series). Contract in its doc comment; five test names
   with their assertions are at the bottom of `source::tests` (`parse_series_*`, `parse_chart_reads_volume`).
   Build the JSON by hand in the shape `parse_chart`'s tests use. Do not shift the time by 60 s.
4. Three tests in `chartview::tests` (comments at the bottom): legend stamps, "no volume lays out as
   before", `tab_at`.
5. Edge tests on top of mine: `Book` already has 9 tests; add NaN/zero spans for `window`, a `Slots`
   test (submit returns the replaced request; `take_done` drains in order), `clear_request`.
6. Final: the five commands, clippy, re-render the four pictures, update this folder's notes (`p4.md`).

Until 1-3 are done the app shows "no data for this range" on every non-1D tab (the stubs are safe).

## Do not change

- `Book`'s transitions and `Slots` (`ranges.rs`): the one in-flight entry per KEY (not one global),
  `cancel` on a replaced request, `arrived` accepting only the in-flight generation for that key,
  `forget`, the 120 s lost-request rule, 30 s failure back-off. The tests pin them.
- `Layout` stays `Copy` (`tabs: [Rect; 6]`, `vol: Option<Rect>`). `plot` is the PRICE plot; `base()` is
  where the time axis hangs. Painter and pointer read the same layout.
- `fold` / `fitted` / `fitted_to` / `bucket_key`: `fitted` keeps its old ladder (nothing above 1 h) for
  the 1D pane; `fitted_to` has the long ladder. `fold` with offset 0 and 60..3600 s buckets is
  byte-for-byte what it was (`fold_before_zones` test).
- `Chart::tabs == false` reproduces the pane from before ranges; `shot::write_chart_named_at` (no range)
  passes false. Do not set it true in the default fixture.
- `want_range` call sites in `ui.rs` and the reset list below.

## Decisions the brief did not make

- **Where the worker starts.** `source::spawn_ranges(slots, notify)` is a separate function called from
  `ui::run` next to `spawn`, with its own message `WM_RANGE` (`WM_APP + 11`). `spawn`'s signature is
  shared with other packages and its `notify` posts `WM_FEED`. `WM_APP + 12` and timer 7 are unused.
- **Shared slots**: `ranges::Slots` (pure, in `ranges.rs`) behind `Arc<Mutex<_>>` held in
  `Ui::range_slots`: one request slot and a vector of results. The lock is never held across I/O.
- **Replaced request**: `Slots::submit` returns the request it replaced (the worker had not taken it);
  the UI calls `Book::cancel(generation)`, so its key can be asked for again. A request already being
  fetched is never interrupted; it is delivered and cached if its generation is still in flight for its
  key. A request for a symbol that left the strip is dropped when it arrives (`forget` removed its
  in-flight entry). A request is presumed lost after 120 s (`LOST_AFTER_S`) so a dead worker cannot wedge
  a key; its late answer is then dropped.
- **In flight is per key**, not one global request (the brief's two sentences conflict otherwise: a late
  answer for a key the user left must still be cached).
- **Book API**: `want(key, now)` (the time to live comes from `ranges::ttl_of(key.file)`, not a
  parameter), `get(&self, key)` touches LRU through a `Cell`, so the painter can read through `&Ui`.
- **Status**: cached wins (even past its ttl and even after a later failure); then a recorded failure
  (`Failed`, still shown while the retry runs); else `Loading`. An empty cached series is `Empty` and is
  not re-requested before its ttl.
- **Reset paths** (all in `ui.rs`): range tab click: pointer, layout, `want_range`. Lock change
  (`Act::Select`): pointer and layout (as before) and `want_range`. Rows change (`apply_rows`):
  `Book::forget`, `Slots::clear_request`, `want_range`. Chart close (`set_chart(false)`): range back to 1D
  (the cache stays). Chart open: `want_range`. Panel collapse: layout cleared (as before), range kept.
  Panel expand: `want_range` once the mode is Panel. Fallback source in use: `want_range` asks for
  nothing and the pane says "chart unavailable for this source" with the tabs still drawn. Every feed
  tick: `want_range` (the retry and the refresh).
- **Pane states**: the tabs are drawn in every state that names a card, and the layout is stored with
  `bars == 0` (tabs only) until the plot is drawn. With tabs, the empty-state message sits at
  `top + 56 px`, clear of the tabs (without tabs it is where it always was).
- **Long ranges**: reference lines (prev, day high/low), the day change in the header and the
  day-direction colour are 1D only; a long range is coloured by its first open and last close.
- **Legend**: 1D `13:35` (as before), other intraday `Nov 12 13:35`, daily and longer `Nov 12 2026`.
  It shares the second line with the tabs: full form, else compact, else not drawn when even the compact
  form would touch the tabs (narrowest pane).
- **Axis**: intraday ranges tick on clock times and label the first tick of a local day with the date
  (date labels are placed before clock labels so they win a collision); not on 1D, which stays as it
  was. Daily and longer: `chartview::date_ticks` (months, else years, else dates).
- **Calendar buckets**: a day is cut on local midnight, a week on Monday, a month on the 1st
  (`MONTH_S` is only the nominal name of the month bucket). Candle starts are `f32`, 16 s coarse five
  years out, so day-or-longer bucketing and dates read the instant 30 s late (`BIAS_S`); unverified
  against real data.
- **Volume**: strip = bottom fifth of the plot with a 3 px gap, bars in the candle colour, drawn when any
  drawn candle has volume. The pointer answers only inside the price plot (the strip is not pointer
  territory); `price_at` is unchanged. `fold` sums volume.
- **1M and 6M** share one fetch (`1M.json`); `Series::window` trims it by the range's span.
- **Closing the chart resets the range to 1D** (the pane reopens on the day); collapsing the panel does not.
- `--probe` prints one line per range file for the first row (bars, seconds a bar, days, volume yes/no).
- `shot::write_chart_named_range(..., range: Option<&str>, volume)` is new; `write_chart_named_at` is
  unchanged and calls it. `examples/chart.rs`: 8th argument a range (`-` for none), 9th `vol`.

## For the orchestrator

- `scripts/check-state-linux.sh` lists the pure modules by name; `ranges` is new and pure (`Bar` and
  `std`), so it should be added there. I could not (scripts are forbidden). `chartview` still has no
  dependency on `ranges`.
- Unverified live: every CNBC shape (time spelling, bar start vs end, daily bar stamp time of day,
  volume presence). `--probe` is the check.
- No hand cursor over the tabs (the cursor logic is not touched).
