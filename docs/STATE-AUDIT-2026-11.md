# State-machine review, November 2026

`STATE-MACHINES.md` contains the broader inventory: startup/profile/instance
ownership; strip/morph/panel/chart return; monitor placement, fullscreen and
visibility; picker/card selection; geometry/DPI/persistence; timers and GDI;
row identity; quote freshness/backoff/fallback consent; chart generations;
sparklines/candles; alerts/webhooks; menu commands; workers and shutdown.
This pass does not prove every native transition or concurrency interleaving.

## Integrated changes

- Popup commands use a per-menu registry carrying symbol, alert-label and monitor
  identity instead of overlapping arithmetic ID ranges. Catalog entry 50 could
  previously collide with the first clear-alert command. IDs stay within
  10000–65535; exhaustion disables later entries instead of wrapping or dispatching
  a different action. A monitor selection rechecks current strip mode after the
  modal menu closes. Other existing modal-menu snapshot behavior is unchanged.
- `WM_SETTINGCHANGE` re-reads Windows' client-area-animation preference and
  synchronizes the background timer and repaint. This is not a redesign of the
  separate strip/panel morph animation.
- Webhook error display uses parsed HTTP(S) host/port only, excluding userinfo,
  path, query and fragment. Malformed URLs produce a generic marker rather than
  echoing input. The direct `url` dependency reuses the version already in the
  lockfile; no dependency versions are upgraded.
- This branch is based on main and excludes the shelved shared-theme pass. Its
  separate review branch is retained. A read-only PR workflow tests/builds
  without publishing anything.

## Verification

On Windows:

```sh
cargo test --locked --all-features
cargo build --locked --release --bins
```

These build both classic and Ultra. A build without `windres` warns and omits the
embedded icon; this PR does not package or publish a release.

On Linux:

```sh
bash scripts/check-state-linux.sh
```

This test harness includes the actual menu registry, layout, model and webhook
redaction modules. Only the model's small `config::Row` type/default fixture is
stubbed. It uses the project's locked dependencies but does not include the
Win32 UI or claim to build a Linux application. Tests cover 30,000 distinct menu
actions, full ID-space exhaustion, existing layout/model transitions including
10,000 fallback cycles, and 300,000 secret-bearing URL redaction cases.

The local Windows Rust standard-library target is absent, so local cross-target
compilation reports E0463 before application validation. The prior Wine 10.0
attempt aborted during prefix/test startup (`free(): invalid pointer`, exit 134).
No Wine runtime pass is claimed. Native PR CI results must be read for the exact
head commit, not inferred from these component checks.

## Outstanding checks and risks

Exercise classic and Ultra on real Windows: large custom catalogs, alert/clear
selection, monitor radio state and topology changes while a menu is open, and
animation-preference changes with the panel open and collapsed. Wine, if available,
is supplementary and cannot establish native Windows behavior.

Previously identified items remain separate follow-up work: detached webhook
completion versus persisted cooldown on exit; worker cancellation/join; serialized
chart requests; ignored settings-save errors and migration before instance check;
other captured modal-menu state; display/DPI/focus/z-order; history corrections,
cached outage-alert policy and market-closure chart spacing. No merge, release or
deployment is part of this review.
