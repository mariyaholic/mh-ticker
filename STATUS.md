<!-- SPDX-License-Identifier: MIT -->
# Status

Session timers, the portable engine and indicator boilerplates remain in place. This review adds
strict feed timestamp validation, balanced COM initialization, idempotent foreground-hook cleanup,
and an offline dependency inventory. See [testing](docs/TESTING.md) for platform checks.

## Opus-review fix run (branch `fix/opus-review-02`, 2026-11-02)

All 13 findings from the Opus review of this branch (at `295b2d8`) are implemented, one commit per
finding, author Maria, highs first:

| Finding | Commit | Summary |
|---|---|---|
| 02-H1 | `edd25a7` | MQL5 "midnight" zeroed (was "now"); Rust port replayed against the vectors |
| 02-H2 | `d0a489f` | Pine `input.*` defaults const (`UTC`) + exchange-zone toggle; TradingView compile gate documented |
| 02-M1 | `c48f198` | malformed config archived as `config.json.bad-<ms>`, never overwritten; migration only after a successful parse |
| 02-M2 | `433e192` | MQL5 reopen uses the effective broker offset (anchor and conversion), 2 fixed-offset vectors asserted |
| 02-M3 | `36c5abd` | generated-artifact tests assert whole `MH_AT_n`/`MH_OFF_n` arrays, const-default Pine shape, real device mapping; vectors now cross platforms |
| 02-M4 | `374770d` | sessions panel minimum width `s(6*112) + 2*pad + 24` at any symbol count (150% DPI covered); sessions-off panels stay compact |
| 02-L1 | `e8eb90d` | pen/brush restored before delete in `make_icon`; GDI-count regression guard added (review's leak is ref-count-time only on this box — measured, honestly noted) |
| 02-L2 | `09dcd91` | poll wait clamped 1..=3600 s via `wait_seconds`; `f64::MAX`/Inf/NaN/1e300 can no longer abort the app |
| 02-L3 | `76b3738` | device row unavailable when Windows DST auto-adjust is off (Manager ruling); configured zones keep priority |
| 02-L4 | `ba967c4` | seasonal broker reopen modelled (22:00 winter / 23:00 summer) in spec/engine/MQL5/Pine; `summer_reopen_min: Option<u16>` declared, two new golden vectors |
| 02-L5 | `3cb6fb2` | About text versions from `CARGO_PKG_VERSION`; changelog melded into draft-Unreleased |
| 02-L6 | `b30da19` | pre-public scrub: `/workspace` links de-linked, broker identity anonymised (`broker_default`), MIT line corrected, AGENTS.md/opencode.json untracked per XR-1 |
| 02-L7 | `d3ed7c7` | CI pinned by SHA (checkout v4.4.0, rust-toolchain 1.99.1), `-D warnings`, cargo-deny step, plus a repo-side pin gate |

Counts: 77 tests pass locally (26 engine lib, 4 golden, 4 mql5-port, 5 specgen, 38 app, 0 filtered);
the tree was 63 green tests before the run. Rust sources count and LOC are unchanged in substance
(about +700/-120 lines of code and tests across the findings; exact per-commit totals in each
commit).

Gates run locally on this Windows box (toolchain `x86_64-pc-windows-gnu`, rustc 1.98.1; CI uses
1.99.1):

```text
cargo fmt --all --check                                   # exit 0
cargo clippy --locked --all-targets                       # zero warnings
cargo build --locked --workspace                          # ok (app compiles natively)
cargo test --locked --workspace                           # 77 passed, 0 failed
cargo run -p mh-specgen -- --check                        # artifacts current
```

Fails-before evidence per finding is recorded in each commit message; highlights: the structural
MQL5 gate was red before the H1 zeroing (`dt.hour = 0` count 0 vs 2); the effective-offset gate was
red before the M2 signature change; both malformed-config tests were red against the reinstated
overwrite semantics; the DST-off test was red while mapping always ran; the two seasonal-reopen
vectors were red before the L4 engine change; the pre-fix Pine text carried the unpinned
`defval = syminfo.timezone` blocker (the new const-default gate would be red there).

### Deviations

- 02-H1: verification is the Rust vector port `mql5_port.rs` + the structural artifact gate — MT5
  itself cannot execute from this box (declared in TESTING.md/PR).
- 02-H2: the branch brief said "default `syminfo.timezone`"; the Pine const rule forces `UTC` +
  toggle. The brief's doc wording was amended instead of kept (it prescribed a non-compiling
  script).
- 02-L1: measured on this box (200 builds), the documented leak does not materialise here — GDI
  frees DC-selected objects with the memory DC. The fix keeps documented-correct semantics and the
  count test grades real accumulation.
- 02-L4: `summer_reopen_min` is `Option<u16>` so periodic inheritors cannot silently go seasonal;
  the review's "add a field or state the gap" was resolved to "model it" by the Manager rulering on
  2026-11-02, and Pine models the DST window in UTC (no offline TZ arithmetic in Pine).
- 02-L6: broker identity anonymised everywhere (Manager part of the ruling); agent files untracked
  rather than deleted (working copies remain for the harness).

### Tests that need real Windows / platforms

Real Windows: Panel readability at 100% and 150% DPI (M4), multi-monitor tray/strip behavior and
Explorer restarts, the hook/COM lifecycle matrix, DST-off device comparison with the taskbar clock
(L3), a `--probe` run with a new symbol, `scripts/build-windows.ps1` and a clean-build comparison,
and the session-panel visual with the new minimum width (M4). MQL5 (MT5 tester or a real terminal):
compile and compare countdowns against the vectors (H1), the override/measured reopen clock (M2),
the seasonal reopen (L4). TradingView: paste `MH_Sessions.pine`, confirm compile, toggle the
exchange-zone input (H2), compare plate rendering. The branch CI (`fix/opus-review-02`) runs the
Windows job; this is the first CI run of `.github/workflows/ci.yml` ever, and the runner GDI calls
(see L1 test) are the CI-only unknown.

Open issues: a tag/release job is not in scope of L7 (the 07A-13-style release model can be ported
from the sister repos, the Manager's call); the broker identity must stay verified offline in the
private workspace (the provenance notes the anonymisation).

Offline Linux: `cargo test --offline` passed 34 tests; `cargo build --offline`, `cargo fmt --all
--check`, and `cargo clippy --offline --all-targets` passed. Windows GNU cross-build and
`cargo test --offline --workspace --target x86_64-pc-windows-gnu --no-run` passed; its test binaries
were not executed. Workspace Windows Clippy passed without warnings. `cargo deny --offline check
licenses` passed for both configured targets; generated artifacts passed the spec generator check.

Real Windows is needed for COM and hook lifecycle, UI behavior, and release-script/icon and clean-build
checks; MT5 and TradingView need platform execution and vector comparison. The full manual audit of
shipped third-party notices, visual asset provenance, and all source paths remains open. Quote-service
compatibility beyond the checked-in example is unverified offline. No release or resource usage is verified.

Review delta: 314 lines added across tracked and new files, including this status line; this is
an added-line count, not net growth.
