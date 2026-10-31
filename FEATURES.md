# MH Ticker — what we can add without making it fatter

*rev 2, 2026-10-21. Every number measured on this machine against `mh_ticker` 1.2.3
(`MH_Ticker.exe`, 954,880 B). Recipe in [Appendix A](#appendix-a--how-each-number-was-measured).*

## What changed since rev 1, and what I got wrong

Rev 1 recommended **gating the intraday backfill on panel visibility** to save 14.7 MB/h, and
refreshing it every 60 minutes instead of 10. That was the wrong trade: it bought bytes by quietly
degrading the product, and the "full hour of history, always" is worth more than the bandwidth.

**The trade was also unnecessary.** `Cargo.toml` sets `default-features = false` on ureq, and ureq's
`default = ["tls", "gzip"]` — so turning defaults off silently turned **gzip off**. The app has been
downloading CNBC's JSON **uncompressed**:

| | Plain | gzip | |
|---|---|---|---|
| Chart refresh, 6 symbols | 2,535,894 B | **316,824 B** | 12.5 % |
| Quote batch, 6 rows | 5,346 B | **1,345 B** | 25 % |
| **Total traffic** | **20.63 MB/h** | **3.35 MB/h** | **6.2× smaller** |

One word in `Cargo.toml` gives back 17.3 MB/h *and keeps every feature*. The backfill stays at 10
minutes, unconditional, so the chart always has a full hour. Everything else below is budgeted
against the compressed number.

---

## 1. The budget

Estate rule for native tools: **idle RAM < 5 MB, binary < 2 MB, low idle CPU**, and the README's own
promise is "~5 MB RAM, and any potato CPU won't feel it".

| Metric | Today | Headroom |
|---|---|---|
| Private RAM | 4.45 – 4.69 MB | **~0.3 – 0.55 MB** ← binding |
| Peak private (a chart parse in flight) | 5.61 MB | over the rule already |
| CPU | 0.31 % of one core (0.52 % hot) | ample |
| Threads / handles | 11 / 367 | ample |
| Binary | 933 KB | ~1.06 MB |
| **Network** | **20.63 MB/h** → **3.35 MB/h** with gzip | ← the real budget |

Two constraints follow, and they shape every recommendation:

1. **Bytes are cheap to fix; RAM is not.** gzip alone removes 84 % of the traffic. What remains
   (3.35 MB/h) is not worth trading a feature for. RAM, at ~0.4 MB of headroom, *is*.
2. **CPU is not a constraint** (0.31 % of a core). "Is it cheap in CPU" is the wrong question; ask
   "how many bytes move" and "how many bytes are held".

---

## 2. The byte ledger

Cadence as shipped: quotes every 3 s (`poll_s`), chart backfill every 10 min per row.

| Traffic | Plain | gzip | Ratio | Plain /h | **gzip /h** |
|---|---|---|---|---|---|
| Quotes, 6 rows | 5,346 B | 1,345 B | 25 % | 6.12 MB | **1.54 MB** |
| Quotes, 12 rows (`MAX_ROWS`) | 10,737 B | 2,176 B | 20 % | 12.29 MB | 2.49 MB |
| Quotes, 23 rows (catalogue) | 20,177 B | 3,904 B | 19 % | 23.09 MB | 4.47 MB |
| Charts, 6 symbols × 6 refreshes/h | 2,535,894 B | 316,824 B | 12.5 % | 14.51 MB | **1.81 MB** |
| **Total, 6 rows** | | | | **20.63 MB** | **3.35 MB** |

Per-symbol chart size varies far more than rev 1 assumed (I had used XAU's 408,800 B for all six):

| `XAU=` | `XAG=` | `BTC.CB=` | `@DJ.1` | `@SP.1` | `@ND.1` |
|---|---|---|---|---|---|
| 408,800 | 397,470 | 446,750 | 426,029 | 423,354 | 433,491 |

Chart decode cost, measured with a tracking allocator over a real 408,800 B payload:

| Path | CPU | Transient heap |
|---|---|---|
| `serde_json::Value` tree (**ships today**) | 9.1 – 12.8 ms | **2.07 MB** |
| Typed `#[derive(Deserialize)]`, 2 fields | 2.4 – 4.0 ms | **0.19 MB** |

The quote endpoint sends `Cache-Control: max-age=1` and **no `ETag` or `Last-Modified`**, so
conditional GET (304 Not Modified) is not available — gzip is the only transport win on offer.

---

## 3. Data we already pay for and throw away

The quote response carries **35 fields per symbol**; `src/source.rs` parses **7**. Free and unused:
`change` (absolute move), `changetype`, `open`, `high`, `low`, `volume`, `yrhiprice`, `yrloprice`,
`yrhidate`, `currencyCode`, `curmktstatus`, `timeZone`, `onAirName`, `shortName`, `type`, `subType`,
`countryCode`, `provider`, `altSymbol`, `feedSymbol`, `code`, `streamable`, `symbolType`, `source`,
`altName`, `portfolioindicator`, `issue_id`.

The chart payload carries **per-bar `open`/`high`/`low`/`close`/`volume`**; the app keeps `close` only.
Of **2,705 bars** fetched per symbol, **57 are used** (the last hour) — 97.9 % discarded.

And the app already computes what your first request needs: `panel::day_open(price, pct)` exists and
is used 40 lines above the sparkline, for the card. The line just doesn't use it.

Guards, measured not assumed: `high`/`low` are `0.00` for `XAG=` and `XPT=`; `yrloprice` is `0.00`
for 6 of 23 symbols; `volume` is absent for FX, yields and spot metals; `curmktstatus` is `REG_MKT`
for all 23, so it is not a market-open signal.

Chart ranges that exist (the endpoint ignores `range`/`duration`/`interval`, does not batch symbols,
and has no 1-hour range):

| Range | Bytes | Bars | Granularity | Span |
|---|---|---|---|---|
| `1D.json` | ~0.40 MB/sym | 2,705 | 1-minute | ~45 h |
| `5D.json` | 296,607 | 1,962 | 1-minute | 9.6 d |
| `1M.json` | **49,443** | 326 | **daily** | 363 d |
| `6M.json` | 129,322 | 855 | daily | 1,093 d |
| `1Y.json` | 90,364 | 597 | daily | 728 d |
| `5Y.json` | **79,190** | 523 | weekly | 3,654 d |

---

## 4. The fixes that pay for everything

### 4.1 Enable gzip — the whole bandwidth argument, in one word
`Cargo.toml`: `ureq = { version = "2.12", default-features = false, features = ["json", "native-tls", "gzip"] }`

20.63 → **3.35 MB/h**, keeping the full hour and the 10-minute refresh. Transparent decompression in
ureq; no code change beyond the feature flag.

**Built and verified, not just reasoned about:** a throwaway copy with only that word changed produced
**1,003,520 B** against the 954,880 B baseline — **+47.5 KB**, leaving the binary at half the 2 MB
budget — and `--probe` on it parsed all six quotes and all six one-hour backfills (60–61 points each,
exit 0), which proves ureq decompresses transparently and the parsers see normal JSON. The throwaway
copy was deleted; the real tree is untouched.

### 4.2 Decode charts into a typed struct, not a `Value` tree
`src/source.rs` — one `#[derive(Deserialize)]` with `priceBars: Vec<Bar>`, `Bar { t, o, h, l, c, v }`.

2.07 MB → 0.19 MB transient, 9–13 ms → 3–4 ms. This is what brings the *peak* private memory back
under 5 MB (5.61 MB measured today), and it is a prerequisite for keeping per-bar OHLC (¾ more data
per bar) without growing the spike.

### 4.3 Put the board payloads behind `Arc`
`model.rs:51` is `pub backfill: Vec<Vec<(f64, f64)>>`, and `ui.rs` does `u.view = b.clone()` on every
publish — **every 3 seconds**, deep-cloning every `Cell`, every `Quote` (two `String`s each) and the
whole bar set, for 6 rows × 61 points today. With the full hour at 12 rows, plus candles, plus the
24-hour ribbon proposed in §7, this becomes the app's largest steady allocation.

`backfill: Arc<[Arc<[(f64, f64)]>]>` makes the snapshot an atomic increment. Cost: 0 bytes, ~2 frees
per publish.

### 4.4 (Optional) a poll-interval menu item
`poll_s` already exists in config and is honoured; there is just no menu entry. 3 s → 30 s takes
quotes from 1.54 → 0.15 MB/h (gzipped) for people who don't need 3-second prices. Zero new bytes.

### 4.5 Do **not** gate the backfill or the history recording
Reversing rev 1 explicitly. Keep fetching every 10 minutes and keep sampling every second, panel open
or not, because that is what makes "full hour, no matter what" true. The 345 KB the history costs at
6 rows (691 KB at 12) is the price of the feature, and it is worth it. If RAM ever gets tight, halve
it by storing `f32` (172–345 KB) — not by not recording.

**Net effect with 4.1 – 4.3:**

| | Now | After |
|---|---|---|
| Network | 20.63 MB/h | **3.35 MB/h** |
| Transient heap per chart cycle | 12.4 MB | 1.1 MB |
| Peak private RAM | 5.61 MB | **~4.7 MB** (under the rule) |
| Steady private RAM | 4.45 – 4.69 MB | unchanged |
| User-visible change | — | **none** |

---

## 5. The four you asked for

### 5.1 Day-based sparkline colour — **free, and it fixes a contradiction**

Current code, `panel.rs:275`:

```rust
let first = pts.front().unwrap().1;      // first sample of the rolling hour
let last  = pts.back().unwrap().1;
let color = if last >= first { UP } else { DOWN };
```

and the guide line is drawn at `first`. So the line's colour is "did it rise over the last 60 minutes",
while the number above it is "is it up on the day". A row can be **+1.2 % green on the day and draw a
red line**, because the last hour happened to be down. That is the bug you're describing, and it is
worse than cosmetic: two things in the same card disagree about direction.

Fix: pass a `baseline: f64` into `spark()`, colour on `last >= baseline`, and draw the guide line at
`baseline`. The baseline is the day open, which the app **already resolves** for the card:

| Order | Source | Already in the app? |
|---|---|---|
| 1 | `previous_day_closing` from the quote | yes, parsed |
| 2 | `price − change` (the absolute-move field, currently unparsed) | needs one new field |
| 3 | `panel::day_open(price, change_pct)` | yes, existing fallback |

Cost: **0 bytes**, 8 B/row, one comparison. Tests: extend `panel::tests` with
`spark_colour_follows_the_day_not_the_hour`.

Suggestion: keep the guide line labelled — a faint "open 4,112.14" at its right end makes the crossing
readable, and it costs one more text run in the same paint.

### 5.2 Hover anywhere on the chart — **free, but it has a performance trap**

The infrastructure is already there: `WM_MOUSEMOVE` (`ui.rs:1152`), `WM_MOUSELEAVE` (1177),
`TrackMouseEvent`/`TME_LEAVE` (1169), and a whole-window `InvalidateRect` on hover change. What is
missing:

- `panel::hit_test(&Geo, x, y) -> Option<(row, col)>`. `Geo` already carries `cols`, `pad`, `gap`,
  `card_w`, `card_h`, `header`, but `draw()` recomputes each card rect inline, so lift that into a
  `card_rect(&Geo, i) -> RECT` helper and hit-test through it.
- `HoverAt { row: usize, t: f64 }` passed into `panel::draw` → `card()` → `spark()`: vertical crosshair
  at `t`, a dot on the nearest sample, and a readout — `14:32 · 4,138.26 (+0.34 %)`.
- Freeze the time axis while hovering, so the crosshair stops sliding under the cursor as `now` moves.

**The trap.** `paint()` calls `snapshot()`, which does `CreateCompatibleBitmap(w, h)` **on every
repaint** and deletes it at the end. The panel is 724×358 (6 cards) → **0.99 MB** and 724×642 (12 cards)
→ **1.77 MB** per bitmap at 96 DPI, and 2.25× that at 150 % scaling. Repainting on every mouse move
would allocate and destroy about a megabyte sixty times a second.

Two mitigations, both cheap:

1. **Quantise.** Invalidate only when the *nearest sample index* changes. 61 samples across ~300 px
   means a change every ~5 px, so a full sweep repaints ~12 times instead of ~150.
2. **Split the layers** (the better fix, and it also helps §5.4): while the panel is open, keep the
   rendered base as one cached bitmap, and on hover `BitBlt` it then draw only the crosshair layer —
   one line, one ellipse, one text run. That is a GDI surface, not private heap, and it is freed the
   moment the panel collapses.

Cost: **0 bytes**; 0 RAM with quantising alone, up to 0.99–1.77 MB of GDI surface while the panel is
open with the layer split. CPU: unmeasurable with quantising; **must be verified by measurement**, not
asserted (see §9).

### 5.3 Notifications — **free in bytes and CPU**

The tray is already there (`tray()`, `Shell_NotifyIcon`), so a balloon is `NIF_INFO` on the same icon,
routed through the existing `WM_TRAY`. No new dependency, no toast XML, no WinRT.

Settings worth having:

| Setting | Why |
|---|---|
| Per-row threshold, above and/or below | the obvious one |
| **Cooldown** (default ~5 min) | without it, a price oscillating on a level fires forever |
| Quiet hours | a ticker that pings at 3 a.m. gets uninstalled |
| Master switch, and a "mute until restart" | the thing you want in a meeting |
| Day-change alert (`±N %`) | free — derived from data already parsed |

Two honesty notes: **Windows Focus Assist suppresses balloons silently**, so a fired alert that the
user never sees will read as a bug — give alerts a visible fallback (flash the strip, or turn the tray
dot amber). And alerts must not fire on **stale** data: a closed market freezing at a level would
otherwise retrigger every cooldown. Reuse `is_stale` as the gate.

Cost: **0 bytes**, ~1 KB of config, one comparison per rule per poll. Pure logic → belongs in
`alert.rs` and is unit-testable without a tray.

### 5.4 Themes — gradients, hues, styles, fonts

Prerequisite is architecture step 1 (`theme.rs`), because the palette currently lives in `ui.rs:80-86`
as `pub(crate) const` and `panel.rs` imports it **from the window module**, plus three more consts of
its own. Ten colours, two modules, no indirection: themes are not expressible today.

Target: `Palette` with ~14 fields — `bg`, `card`, `label`, `price`, `dim`, `up`, `down`, `amber`,
`guide`, `crosshair`, `badge`, `hairline`, `grad_top`, `grad_bottom`, `radius` — plus `Palette::default()`
equal to today's exact values, named built-ins (**Charcoal** = today, Midnight, Nord, Amber CRT, Paper),
and one resolution point from config.

| Part | How | Cost |
|---|---|---|
| Gradients | ~64 stacked `FillRect` bands per paint — no new import. `GradientFill` (msimg32) needs a `#[link]`; not worth it for a 64-band gradient. | 0 RAM; drawn once per data change, not per frame |
| Hue / accent | one `accent` hue in config drives labels, guides, crosshair, badges | 0 |
| Styles | two axes: flat ↔ gradient, and `radius` (lift the literal `s(12)` out of 4 call sites) | 0 |
| Fonts | `font`, `font_pt`, `dock_font_pt` already exist; add a menu built with `EnumFontFamiliesEx` (~20 lines) so it isn't JSON-only | 0 after enumeration |

**The trap, again**: do **not** cache a gradient bitmap. A 724×642×32-bit ARGB surface is 1.77 MB —
that alone would push private memory from 4.5 MB to 6.3 MB and break the house rule. Bands into the
existing double buffer, or into the cached base layer from §5.2, cost nothing extra.

**One rule**: keep `up`/`down` **semantic** and separately overridable from `accent`. A theme must
never make rising and falling hard to tell apart — the rest is taste.

Cost: **0 bytes**, < 2 KB RAM, ~0 CPU.

---

## 6. Still free: the nine from rev 1

Day range on the card · day-open marker on the line · session high/low markers · 52-week badge with
its date · absolute change next to the % · volume where it exists · currency · click a card to open
the symbol on CNBC · copy the price.

All nine: **0 bytes, ~540 B of RAM, no measurable CPU** — from fields the 3-second response already
carries and the app parses away.

---

## 7. Charts, and two ideas worth more than the features above

| # | Idea | Cost | Worth it |
|---|---|---|---|
| 1 | **Daily candles** (`1M.json`, 326 daily OHLC bars) | 49,443 B/symbol before gzip → ~0.30 MB/h for six, hourly; 31–62 KB RAM | Yes. Different and more useful data than the intraday line, for a tenth of one chart refresh. |
| 2 | **Intraday candles** — the `1D` payload already has per-bar OHLC; keep `o/h/l` alongside `c` | 11.7 KB RAM (61 bars × 6 rows × 32 B) | Yes, once 4.2 lands so the bigger per-bar record doesn't inflate the spike. |
| 3 | **A 24-hour ribbon per card** — min/max per minute from the samples the app already takes | **0 bytes**; 1,440 buckets × 8 B = 11.5 KB/row → 138 KB at 12 rows | **The highest value-per-byte item on this page.** The app already samples every second and throws it away after an hour. Keeping a day of min/max turns each card into a market ribbon that survives the session, with the 1-hour line as the zoomed detail. |
| 4 | **Collapse the trust vocabulary** — `Link` (3 states) + `stale` + `realTime` + `age` + amber = five ways of saying "how much do I trust this number" | 0 | One per-row confidence 0–1 drawn as a thin bar, tray icon shows the minimum. A simplification, not a feature: it deletes a class of "why is that amber" questions. |
| 5 | **`--probe` as a budget gate** — print bytes fetched and parse ms, exit non-zero if either exceeds a threshold | 0 | The best defence against exactly what this conversation is about. Turns "don't make it fat" from a document into a test that fails. |

Raising traffic, priced honestly: **two-tier polling** so the Symbols menu shows live prices for all 23
catalogue symbols is 1.54 (shown) + 0.36 MB/h (the other 17 at 30 s, 3,137 B gzipped) = **1.90 MB/h,
+0.36**. In the plain-JSON world that was +1.70 MB/h and hard to justify; behind gzip it is almost
free. **Raising `MAX_ROWS` to 23** stays a no: same data, 4.47 vs 2.49 MB/h.

---

## 8. What I would not do

- **No local HTTP server, no web view, no WebView2.** "No login, no server, just the exe" is the
  product; a WebView2 host costs more than the entire RAM budget.
- **No per-symbol history files on disk.** Writing a year of candles turns a read-only tool into one
  that needs cleanup, locking and migration.
- **No higher poll rate.** 3 s is already aggressive, and gzip makes the remaining traffic small
  enough that there is nothing left to win.
- **No cached gradient or panel bitmaps sized to the window** unless they are freed on collapse — at
  12 rows and 150 % scaling a single back buffer is ~4 MB.

---

## 9. Order of work

| Phase | Work | Result |
|---|---|---|
| **0** | **Enable gzip** (4.1) | 20.63 → 3.35 MB/h, no feature lost, +47.5 KB binary. One word, already built and `--probe`-verified. Ship it alone. |
| 1 | Typed decode (4.2) · `Arc` payloads (4.3) | peak private 5.61 → ~4.7 MB; kills the 3-second clone |
| 2 | `theme.rs` + `series.rs` (architecture steps 1–2) | palette and series become addressable; unblocks §5 |
| 3 | Day-based colour (§5.1) | the contradiction is gone; 0 bytes |
| 4 | Hover (§5.2), with quantised invalidation | crosshair + readout; **measure the repaint cost** |
| 5 | Notifications (§5.3) | `alert.rs`, pure and tested |
| 6 | Themes (§5.4) | palette + built-ins + font menu |
| 7 | 24-hour ribbon (§7.3) · `--probe` gate (§7.5) | the biggest visual win for zero bytes; a regression test for the budget |

Steps 0–1 are each independently shippable and user-invisible. Step 4 is the only one with a
performance risk that **must be measured rather than argued**: run the panel with 12 rows, sweep the
mouse across it, and read CPU before and after. If quantising alone doesn't hold it under ~1 % of a
core, take the layer split.

---

## Appendix A — how each number was measured

| Number | Method |
|---|---|
| RAM / CPU / threads / handles | PowerShell: start `MH_Ticker.exe`, sample `PrivateMemorySize64`, `WorkingSet64`, `Threads.Count`, `HandleCount` at t=4 s and t=64 s; CPU delta ÷ window. Two runs: 0.31 % and 0.52 % of one core. |
| Plain vs gzip bytes | `curl` with `-H 'Accept-Encoding: identity'` and `-H 'Accept-Encoding: gzip'`, `-w '%{size_download}'`, against the exact URLs in `src/source.rs` with the app's User-Agent. Chart total = all six configured symbols, uncompressed 2,535,894 B / gzip 316,824 B. |
| Why gzip is off | `Cargo.toml` sets `default-features = false`; ureq 2.12.1's `Cargo.toml` declares `default = ["tls", "gzip"]` and sends `accept-encoding` only when that feature is on (`ureq/src/request.rs:93-109`). |
| Conditional GET | Response headers carry `Cache-Control: max-age=1` and `Expires`; **no** `ETag`, **no** `Last-Modified`, so `If-None-Match` / `If-Modified-Since` are not offered. |
| Chart decode CPU and heap | Throwaway Rust example with a tracking `GlobalAlloc` (live + peak bytes) and `Instant` timing over a saved 408,800 B payload: `Value` tree vs typed `Deserialize`. 3 iterations each; harness deleted afterwards. |
| Panel bitmap size | `panel::geometry`: `w = 2·16 + cols·236 + (cols−1)·10`, `h = 48 + rows·150 + (rows−1)·10 + 36`, ×4 bytes/px — 1 card 268×234 (0.24 MiB), 6 cards 760×394 (1.14 MiB), 12 cards 760×710 (2.06 MiB). One such bitmap now lives on `Ui` and is reused until the window resizes; only the morph captures allocate per frame. |
| Card layout | Rendered PNGs decoded and scanned column by column: for six themes × six cards, every text run stays ≥ 8 px inside the card and no two runs on the same line touch. `prev` is skipped when it would collide with the change text, which is why NAS shows no previous close at 236 px. |
| Animated cost | 9-second loop at 10 fps, `WM_TIMER` only while the panel is open (`sync_anim`), and three rendered frames a third of a cycle apart differ by ~4–5 mean luminance — the drift is real, not a palette that never moves. |
| gzip build works | Copied `src`, `Cargo.toml`, `Cargo.lock`, `build.rs`, `assets` to a throwaway dir, added `"gzip"` to the ureq features, `cargo build --release` → 1,003,520 B (+47.5 KB over 954,880 B), then `--probe` → six quotes and six backfills parsed, exit 0. Temp dir removed afterwards. |
| Chart ranges | `curl` each of `1D/5D/1M/6M/1Y/5Y.json`, count `barData.priceBars`, span from `tradeTimeInMills`. |
| Quote field inventory | Batched `restQuote` for all 23 catalogue symbols; dump every key of one row (35 keys). |
| Endpoint limits | Probed `1H.json`, `?range=1h`, `?duration=60`, `?interval=1m`, and a two-symbol `symbols=`: no 1-hour range, params ignored, no symbol batching. |
| History RAM | `HIST_CAP` 3,600 × 16 B / point × rows. |

Reproduce the RAM/CPU baseline with the two scripts in `.probe_tmp/` (`measure.ps1` steady state,
`peak.ps1` for the `--probe` peak). `measure.ps1` now takes `-Exe <path>`, so one harness measures both
builds instead of a variant script per binary.

---

## Status — 2026-10-22 (shipped in 1.3.0)

The plan above was written against one exe. Maria's answer was "ship an Ultra build for the
aesthetics, keep the original lean" — so the split is by *product*, and everything correctness-shaped
went to both.

| § | Item | State |
|---|---|---|
| — | **gzip** (the finding that made the plan cheap) | **shipped**, both builds. 20.63 → 3.35 MB/h. |
| 4.2 / 4.3 | panel-gated backfill, 60-min refresh | **dropped deliberately.** With gzip the full hour costs 0.53 MB/h; gating it would have opened the panel cold for no real saving. The "full hour, always" requirement won. |
| 4.5 | poll-interval menu | **shipped**, both builds. `poll_s` is now on the shared `Board` so it applies on the next tick. |
| 5.3 | notifications | **shipped (Ultra)**: thresholds, 30 s cooldown floor, stale-quote gate, balloon, webhook. *Not* shipped: quiet hours, mute-until-restart, the visible-fallback flash (the panel footer shows the last alert instead). |
| 5.4 | themes | **shipped (Ultra)**: **26 palettes in four tiers** (basic, gradient, animated, retro pixel) in `theme.rs`, plus 13 faces in a Font menu. 64-band gradients, a 96-band ring for the animated ones, scanlines and bitmap faces for the retro ones. `up`/`down` kept semantic and tested. Gradient bitmaps still not cached — the ring slides over pre-built brushes instead. See `THEMES.md`. |
| 6 | the nine free card features (day range, absolutes, 52-week, volume…) | **shipped, both builds** (1.4.0). All nine, from eight more fields of the response the app was already downloading: 0 extra bytes. The 52-week **date** appears only when the card has room for it (XAG does, XAU does not at 236 px). |
| 4.1 | typed decode instead of a `Value` tree | **not shipped.** At 3.35 MB/h it is no longer a bytes problem; it is still 3× the decode CPU and 10× the transient heap, so it stays the right move before any per-bar OHLC feature. |
| 5.1 / 5.2 | daily candles from `1M.json`; 24 h ribbon | **not shipped.** Both now fit the RAM budget more easily than they did, because Ultra is allowed to be heavier. |
| — | day-based line colour | **shipped**, both builds. This was a bug, not a feature: the line coloured on the rolling hour while the card reported the day. |
| — | hover crosshair | **shipped**, both builds. |
| — | symbol picker | **shipped (Ultra)**, as the prototype asked for — a chip strip inside the panel. |

Measured after the change, same harness, 60 s window, one build at a time:

| Build | private @4 s | private @64 s | CPU | binary |
|---|---|---|---|---|
| 1.2.3 (before) | 4.64 MB | 5.73 MB | 0.42 % | 954,880 B |
| 1.3.0 classic | 4.70 MB | 5.77 MB | 0.44 % | 1,068,544 B |
| 1.3.0 Ultra | 4.79 MB | 6.38 MB | 0.52 % | 1,068,544 B |
| 1.4.0 classic | 5.36 MB | 5.71 MB | 0.44 % | 1,091,072 B |
| 1.4.0 Ultra | 5.57 MB | **5.83 MB** | **0.23 %** | 1,091,072 B |

1.4.0 is cheaper at 60 s than 1.3.0 despite a bigger card, a wider window and 26 palettes in the
binary: `paint` now reuses one back buffer instead of `CreateCompatibleBitmap`-ing a fresh 1.2 MB
surface on every repaint, which is worth ~0.5 MB on Ultra and most of its CPU. The t=4 s figures are
higher than 1.3.0's, which is the honest part of this table: the first seconds are dominated by
freshly-committed pages and are the noisiest number in it.

Both grow ~0.3–0.4 MB in the first minute while the hour of history fills in. 1.2.3 did the same,
which is why the old "4.45–4.69 MB" figure was an early-window reading and not a steady state.
