<!-- SPDX-License-Identifier: Apache-2.0 -->
# Third-party software and data

Made by Maria. The MH Ticker application and the session engine/indicators are Apache-2.0; see
[LICENSE](LICENSE) and [NOTICE](NOTICE).

Only MIT, BSD, ISC, Apache-2.0 and Zlib dependencies are intentionally used. The table records the direct
dependencies added by this feature and the existing application; transitive crates are pinned by
[Cargo.lock](Cargo.lock) and retain their own notices.

| Component | Version | Licence | Use |
| --- | --- | --- | --- |
| `chrono` | 0.4.45 (`=0.4.45`) | MIT OR Apache-2.0 | UTC instants, dates, durations |
| `chrono-tz` | 0.10.4 (`=0.10.4`) | MIT OR Apache-2.0 | Compiled IANA zone database |
| `serde` | 1 | MIT OR Apache-2.0 | Config and spec serialization |
| `serde_json` | 1 | MIT OR Apache-2.0 | Config, spec and golden vectors |
| `ureq` | 2.12 | MIT OR Apache-2.0 | Existing CNBC quote fetch (unchanged) |
| `native-tls` | 0.2 | MIT OR Apache-2.0 | Existing Windows SChannel TLS (unchanged) |
| `windows-sys` | 0.59 | MIT OR Apache-2.0 | Existing Win32 bindings; session adapter uses `Win32_System_Time` and `Win32_Globalization` |
| `windows` | 0.58 | MIT OR Apache-2.0 | Existing UI Automation taskbar dock (unchanged) |

## Data

- **IANA Time Zone Database (tzdb) 2025b** is embedded by `chrono-tz`. The tz database is public domain
  (the `tz` distribution's `LICENSE` permits use and redistribution).
- **Windows-to-IANA mapping**: `crates/session-engine/data/windows-zones.tsv` is a pinned, reviewable subset of
  CLDR `windowsZones` version 48 (001 defaults plus selected territory overrides). It is hand-maintained in
  this repository, not copied wholesale. CLDR is published by the Unicode Consortium; its data licence is the
  Unicode License v3, which is **not on the approved dependency licence list** for this project. This is an
  open item for the Manager (see [STATUS.md](STATUS.md) and [TESTING.md](TESTING.md)); unknown mappings fail
  visibly rather than guessing.
- **Pine Script and MQL5 files** contain only project code and generated project data. No third-party
  indicator code is reused. TradingView and MetaQuotes are not affiliated with this project; MT5 is a
  MetaQuotes trademark.

## Existing application dependencies

The quote strip's existing dependencies (serde, serde_json, ureq, native-tls, windows-sys, windows and their
transitive crates) are unchanged by this feature and remain recorded in `Cargo.lock`. Windows itself is not a
third-party component.
