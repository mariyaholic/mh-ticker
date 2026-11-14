# W1 brief: Know it is true (both builds)

*The writer's brief for the first 1.9 package, prepared by the orchestrator from
[../1.8/05-next-features.md](../1.8/05-next-features.md) section H and package W1. The workflow is
[01-workflow.md](01-workflow.md). Items: H1, H2, H3, H4, H5, H10, H11, H12, H13, H14.*

## Why first

Nothing in 1.8 was seen on Windows or run against CNBC. This package adds nothing a user would list
as a feature; it makes the features they have checkable, and its pictures and probe are what W2 to W4
are judged by.

## What to build

| Item | What | New pure code (Linux harness) | Wiring |
|---|---|---|---|
| H1 | `--probe` judges: every shown symbol, every range file, an unknown symbol and the release answer; each named assumption printed PASS or WARN, plus response field names the parser ignores. Non-zero exit on any WARN | `probe.rs`: `judge(bodies) -> Vec<Finding>` | `source::probe` collects bodies and prints |
| H2 | Settings or tray: *Copy diagnostics report*: the same checks on a click, as text on the clipboard. A "checking" flag like the update check; a second click is ignored | `report.rs`: `text(facts) -> String` | a one-shot worker posting one reserved message |
| H3 | `--probe --dump <dir>`: raw answers saved as files for fixtures. Refuses without a folder; never writes the config or the webhook | file naming in `probe.rs` | command line only |
| H4 | The chart says when it is suspect: `as of 14:05` past the series' time to live; `check dates` when daily stamps are off a day boundary | `ranges::Series::check`, `age_label` | the pane header |
| H5 | Data meter: `Data: 3.1 MB this hour, 41 MB since start` as a greyed menu line | `meter.rs`: counters and a 60-slot ring | count bytes where every response is read; no new HTTP call |
| H10 | Typed levels: `4200,5` refused with a reason; the parsed level echoed in the symbol's digits; a level more than 20 % from the price asks first | `alerts::parse_level` grouping rule | the existing prompt path |
| H11 | Local time per instant: the mute label and 5D candles right across a clock change | fold with an offset function, not one offset | the Windows call for a past instant's offset |
| H12 | Hardening: non-finite prices print `--`; `y_of(NaN)` inside the plot; no candle past the plot's right edge; the update check sends an app name, not a browser string or version; cooldowns on a clock that cannot jump | all pure | |
| H13 | A config that will not parse is kept beside the new one, defaults load, the menu says so | `config::load` returns loaded or defaulted-with-kept-path | one flag, one greyed line |
| H14 | Wider stills and fit tests: 24 cards; 24 symbols on 1, 2 and 3 strip rows; the narrow pane; 192 DPI; each range; for fixed-pitch faces a fit-by-characters test at 96, 120, 144, 192 DPI | the fit test | new stills; the 36 old ones stay identical |

## Acceptance (the checker runs each)

Word for word from 05, W1:

- `probe::judge` has one passing and one warning fixture for each named assumption (quote fields,
  unknown symbol, time spelling, bar start or end, daily stamp boundary, volume present, release tag).
  A WARN gives a non-zero exit.
- The report built from a fixture config shows the webhook as its host only; a test searches the
  text for the path and query and finds neither. The settings path carries no user name.
- `--dump` refuses to run without a folder and writes neither the config nor the webhook (temp folder
  test).
- Meter: tests for the hour roll-over, an idle hour and the menu line. Review confirms no new call to
  the HTTP client.
- Typed levels: `4200,5`, `4,20`, `1,2345` and `,500` are refused with a reason; `4,200.50` is read as
  4200.50; the far-level question triggers at exactly 20 %.
- A 5D fixture folded across an offset change puts the bars on each side at their own local hour. The
  mute label after a change shows the right hour.
- `fmt_price` and `board_text` print `--` for NaN and infinity. `y_of(NaN)` lies inside the plot. 5D
  at one-hour candles in the narrowest pane has no candle past the plot's right edge. The update
  request's header test finds no version number and no browser string.
- A config that will not parse: the original bytes survive under another name, defaults load, the
  flag is set.
- New stills exist for 24 cards, 24 symbols on 1, 2 and 3 rows, the narrow pane, 192 DPI and each
  range. The fit test passes for every fixed-pitch face at 96, 120, 144 and 192 DPI.

Plus the standing checks in [01-workflow.md](01-workflow.md), and both exes still under 2 MB.

## Decisions for the owner before the writer starts

1. **H3 and "no history files on disk".** `--dump` writes files, but only from the command line,
   only into a folder the user names, and never from the running app. 05 reads the rule as being
   about the running app. If the rule is stricter, H3 is struck and the rest of W1 is unchanged.
2. **H14 adds stills to `shots/`.** The 36 existing stills stay byte-identical; the new ones are added
   beside them. This is the one package allowed to add to `shots/`.

## Lessons carried from 1.8 and the responsiveness pass

- Measure under Wine before optimising, and state Wine numbers as ratios (`examples/perf.rs` exists).
- Every repaint goes through `panel::hot_dirty` or the full-window path; a new header line in the pane
  (H4) must be inside the rectangle the hover invalidates, or it will not update.
- Saves go through `save_cfg()` outside `with_ui`.
- A long single file is checkpointed midway, not only at the end.
