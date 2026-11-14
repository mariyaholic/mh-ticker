# The 1.8 cycle: how it was made

*Record written 2026-11-14 by the scribe role, from the notes, the commit messages and the audit. It says what
happened, what was sent back, and where the work stands. Nothing here was clicked on a Windows desktop.*

## Starting point

1.7.1 (`6f6ee3c`). The owner asked what MH Ticker could gain without becoming a different product.

Facts that shaped the plan:

- The build sandbox could not reach CNBC (the proxy refuses `quote.cnbc.com` and `ts-api.cnbc.com`). Anything that
  depended on a response shape nobody had measured was built defensively and marked unverified, or left for later.
- Nothing could be clicked on a Windows desktop before shipping. Tests ran under Wine and on the native Windows CI
  runner. Items whose value was all in the feel of a live window ranked lower for that reason.
- The RAM budget doubled during wave 1, on the owner's instruction of 2026-11-13. Ultra may use up to about 23 MB
  of private memory (1.7.1 used about 5.8 MB; the 1.8 ceiling was about 11.5 MB). Classic stays at about 11.5 MB.

## Discovery

The discovery prompt is in [01-discovery-prompt.md](01-discovery-prompt.md). It was written by the orchestrating
agent for itself and kept, because it is reusable against a later version. It asked for every addition that makes
the ticker more valuable without changing what it is, and it required a verdict for each item.

The list is [02-feature-list.md](02-feature-list.md): 35 items BUILD NOW in six packages, 14 LATER, 9 NO.

## The six packages, in two waves

The plan is [03-work-packages.md](03-work-packages.md). Each package was built in one worktree, finished in the same
worktree, then reviewed by the orchestrator until accepted and merged.

| Wave | Package | Items | Commit |
|---|---|---|---|
| 1 | P1 Alerts you can set and silence (Ultra) | A1 to A9 | `396e28d` |
| 1 | P2 A chart that reads (Ultra) | B1 to B5, F2 | `2c03516` |
| 1 | P3 Everyday (both builds) | C1 to C7 | `bb92cb3`, fixed in `0e574ee` |
| 2 | P4 Chart ranges and volume (Ultra) | B6 to B8 | `7b79ec8` |
| 2 | P5 Keyboard, cards and order | D1 to D4 | `ef82ea6` |
| 2 | P6 Type it in | F1, E1, E2, E5, A10, A11 | `62fa9ef` |

Each wave ran three packages at once, one per worktree. Wave 2 followed wave 1.

## Roles

- **Orchestrator.** Wrote the discovery prompt, the list and the packages, wrote the briefs, ran the worktrees, and
  reviewed and merged each package.
- **Builder.** Built the skeleton and every risky edit in a package, including the Win32 wiring.
- **Finisher.** Wrote the bodies, the tests and the verification notes, and recorded its decisions in the package
  notes.
- **Auditor.** Traced the state machines after the packages were merged, and fixed what it found (see below).
- **Scribe.** Wrote this record, the changelog entry, the README and SOURCES updates, and the site page.

Each package's notes name its builder and finisher decisions. The notes also list what was not verified.

## What review sent back

One line per package, from the notes and the commit messages. Where the notes record no review change, the line says so.

- **P1.** "Past today's high or low" now needs a price one tick beyond the extreme, so a price already there does
  not fire at once. The first rule fired at the extreme itself.
- **P2.** A doc example was corrected (the change for the sample bar is +0.01 %, not +0.02 %), and a clippy lint in
  the painter was rewritten with the same pixels. Stills stayed 36 of 36.
- **P3.** `Feed::wait` takes the kick value read before its request, so a kick during a request ends the next wait at
  once. Stale cells in *Copy all prices* get a fourth field. The update message shows only the digits and dots of the
  tag. The shell-reserved preset `Win+Shift+M` was replaced by `Ctrl+Alt+Space`. A later commit (`0e574ee`) fixed
  *Start with Windows* so it creates the `Run` key when the profile has none.
- **P4.** Daily and longer series are dated 12 hours east of UTC, not on the viewer's clock. On the viewer's clock
  every daily candle was a day early for anyone west of the exchange, and Monday's bar fell into the week before.
  `--probe` prints each file's last bar stamp in UTC so the assumption can be checked. When the symbol list changes, a
  request taken back from the worker is also cancelled in the book.
- **P5.** The notes record no review change. The finisher's decisions are in `docs/1.8/notes/p5.md`: removing the
  locked card passes the lock on, and a hand-added row is adopted only when its move succeeds.
- **P6.** The notes record no review change. The finisher's decisions are in `docs/1.8/notes/p6.md`: label rules,
  decimal places, the 24-symbol limit and the menu ranges.

## Usage-limit interruptions and the checkpoint protocol

Wave 2 lost work to usage limits: agents were cut off partway through a package. The fix was procedural, and it is
now part of the work-package rules (`08aee66`):

- Each worker runs `checkpoint "<done>" "<next>"` after each coherent step and before any long command. The script
  commits everything as a `checkpoint:` commit and appends a line to `.progress.md`, which is never committed.
- A cut-off worker is resumed from its last checkpoint and progress line, not re-read from scratch. The orchestrator
  uses `where` to list each worktree's commits, uncommitted files and open TODOs.
- Squash-merging hides the checkpoint commits.

This scribe pass also followed the protocol: one checkpoint per file.

## The audit

The state-machine audit is in [../STATE-AUDIT-1.8.md](../STATE-AUDIT-1.8.md). It covered the 27 machines in
`STATE-MACHINES.md`, on both builds, by reading the source and writing a pure unit test for each fix. Nothing was
clicked in a live window.

No fixer agent could be started in this environment, so the auditor made the fixes itself, as its brief allowed. The audit recorded nine findings:

- Fixed: two Medium (*Refresh now* did not reach the range book; the fallback toggle wrote a settings copy from
  before its confirmation box) and three Low (a webhook result lost its "test" flag once its log line was gone;
  "Alerts (n)" counted hidden symbols; a second *Add symbol* prompt dropped its symbol silently, fixed in review).
- Open, recorded for a later pass: a card menu's *Copy* and *Open* act on the index the card had when the menu
  opened; the tray alert marker stays lit while the panel is open; a menu opened before a hotkey or tray click
  changes the mode can show a stale state; a morph across monitors of different DPI lays out at the old DPI until
  the next refresh.

## The next-features pass

The next pass is [05-next-features.md](05-next-features.md). It weighed the LATER and NO items again and added new
ones: 34 items BUILD NEXT in four packages (five from the old LATER list, 29 new), 30 LATER, 16 NO.

The four packages are W1 Know it is true, W2 Alerts that say when they cannot see, W3 Twenty-four symbols, and W4
Reach. The items are ranked by value over cost plus risk. The top of the ranking is H1 (a probe that judges), H5
(a data meter) and H10 (typed levels that cannot be misread).

Its budget, which the next cycle must keep: Ultra up to about 23 MB of private memory, Classic at about 11.5 MB and
no drift, one exe under 2 MB, no new dependency, no server, no installer, no telemetry, no new steady traffic beyond
what the user's own choices cost, and any other request only on an explicit click to a host the app already names.
A webhook URL is a secret.

## Final state

- Tests under Wine: 240 pass. The one failure is the known Wine-only case
  `ui::tests::a_theme_switch_takes_the_new_themes_face`, which needs Consolas.
- Linux harness (`scripts/check-state-linux.sh`): 107 pass.
- Stills: 36 of 36 identical to the baseline, so Classic has not drifted.
- Native Windows CI: green.

Not done in this cycle:

- Nothing was clicked on a Windows desktop. The menus, dialogs, hotkey registration, balloons, the chart hover with
  a real mouse, and keyboard focus were all reviewed from code and from pictures rendered under Wine.
- CNBC range shapes are not verified live. `--probe` is the check.
- The `Cargo.toml` version is still 1.7.1 in this tree. The 1.8.0 bump has not been made.
- The next-features packages are not started.
