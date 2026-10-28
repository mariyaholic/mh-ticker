<!-- SPDX-License-Identifier: MIT -->
# Status

Session timers, the portable engine and indicator boilerplates remain in place. This review adds
strict feed timestamp validation, balanced COM initialization, idempotent foreground-hook cleanup,
and an offline dependency inventory. See [testing](docs/TESTING.md) for platform checks.

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
