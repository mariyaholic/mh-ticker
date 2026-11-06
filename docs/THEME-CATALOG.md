# Shared theme catalog adoption

The authoritative application catalog is
`mariyaholic/Uika:design/themes/catalog.json`. This checkout has generated,
offline-consumed adapters and a `theme-catalog.lock.json` content fingerprint.
Edit the catalog and regenerate rather than changing generated literals.

The Uika brand canon remains unchanged. The catalog has 37 explicit identities
from MH Ticker and Yozora; adapters retain renderer-specific behavior.

Ultra exposes all 37 identities. The original 26 preserve every source color,
font, cycle and background stop. Classic stays Midnight, and unknown IDs still
fall back to Midnight. Imported Yozora palettes are still native surfaces, using
local font fallbacks. Theme and font menu command ranges are tested for overlap.

Refresh with `node Uika/tool/src/theme-catalog.mjs --mh-ticker PATH`.

## Validation

Passed: six catalog test groups, all 37 web-role contrast checks, exact field parity
for the 26 original Ticker native palettes, deterministic generation/check mode,
and Linux compilation/tests for the generated Rust data and six fixed adapters.
Yozora config and server tests cross-compile for Windows; gofmt is clean.

Not passed: browser runtime (cloud local-socket restriction, then environment setup
failure on the supported escalation), Wine (fresh-prefix initialization abort),
and full Windows Rust builds (missing cached native-tls and Windows target).
The full Uika suite was attempted on the partial audit checkout: 173 passed,
19 failed, including missing documentation fixtures and environment-dependent
outbox tests. This is not a full-suite pass. The new catalog suite passes.

Run complete repository and UI checks before merging. No release, deployment,
merge, or final PR was created. The later integration pass owns final PRs.
