<!-- SPDX-License-Identifier: MIT -->
# Third-party software and data

made with love by maria. The MH Ticker application, the session engine and the Pine/MQL5 indicators in this
repository are MIT; see [LICENSE](LICENSE).

The table below lists every package in offline Cargo metadata (including platform-specific packages
not selected by the two audited target graphs). Versions and declared expressions come from
`cargo metadata --offline`; `cargo deny --offline check licenses` checked the Linux and Windows GNU
graphs against [deny.toml](deny.toml). An `OR` expression permits an allowed alternative;
LGPL, LLVM-exception and Unlicense alternatives are not relied on. This is a metadata and tool
audit, not a manual review of all shipped source notices.

| Crate | Version | Declared licence | Verification |
| --- | --- | --- | --- |
| `android_system_properties` | 0.1.6 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `autocfg` | 1.5.1 | Apache-2.0 OR MIT | Cargo metadata; cargo-deny for selected targets |
| `base64` | 0.22.1 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `bitflags` | 2.13.2 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `bumpalo` | 3.20.3 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `cc` | 1.8.0 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `cfg-if` | 1.0.5 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `chrono` | 0.4.45 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `chrono-tz` | 0.10.4 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `core-foundation` | 0.10.1 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `core-foundation-sys` | 0.8.7 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `displaydoc` | 0.2.7 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `errno` | 0.3.14 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `fastrand` | 2.5.0 | Apache-2.0 OR MIT | Cargo metadata; cargo-deny for selected targets |
| `find-msvc-tools` | 0.1.14 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `foreign-types` | 0.3.2 | MIT/Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `foreign-types-shared` | 0.1.1 | MIT/Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `form_urlencoded` | 1.2.2 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `futures-core` | 0.3.34 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `futures-task` | 0.3.34 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `futures-util` | 0.3.34 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `getrandom` | 0.4.3 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `iana-time-zone` | 0.1.65 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `iana-time-zone-haiku` | 0.1.2 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `icu_collections` | 2.3.0 | Unicode-3.0 | Cargo metadata; cargo-deny for selected targets |
| `icu_locale_core` | 2.3.0 | Unicode-3.0 | Cargo metadata; cargo-deny for selected targets |
| `icu_normalizer` | 2.3.0 | Unicode-3.0 | Cargo metadata; cargo-deny for selected targets |
| `icu_normalizer_data` | 2.3.0 | Unicode-3.0 | Cargo metadata; cargo-deny for selected targets |
| `icu_properties` | 2.3.0 | Unicode-3.0 | Cargo metadata; cargo-deny for selected targets |
| `icu_properties_data` | 2.3.0 | Unicode-3.0 | Cargo metadata; cargo-deny for selected targets |
| `icu_provider` | 2.3.1 | Unicode-3.0 | Cargo metadata; cargo-deny for selected targets |
| `idna` | 1.1.0 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `idna_adapter` | 1.2.2 | Apache-2.0 OR MIT | Cargo metadata; cargo-deny for selected targets |
| `itoa` | 1.0.18 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `js-sys` | 0.3.106 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `libc` | 0.2.190 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `linux-raw-sys` | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Cargo metadata; cargo-deny for selected targets |
| `litemap` | 0.8.3 | Unicode-3.0 | Cargo metadata; cargo-deny for selected targets |
| `log` | 0.4.34 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `memchr` | 2.8.3 | Unlicense OR MIT | Cargo metadata; cargo-deny for selected targets |
| `mh-session` | 1.0.0 | MIT | Cargo metadata; cargo-deny for selected targets |
| `mh-specgen` | 1.0.0 | MIT | Cargo metadata; cargo-deny for selected targets |
| `mh_ticker` | 1.3.0 | MIT | Cargo metadata; cargo-deny for selected targets |
| `native-tls` | 0.2.18 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `num-traits` | 0.2.19 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `once_cell` | 1.21.4 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `openssl` | 0.10.81 | Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `openssl-macros` | 0.1.1 | MIT/Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `openssl-probe` | 0.2.1 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `openssl-sys` | 0.9.117 | MIT | Cargo metadata; cargo-deny for selected targets |
| `percent-encoding` | 2.3.2 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `phf` | 0.12.1 | MIT | Cargo metadata; cargo-deny for selected targets |
| `phf_shared` | 0.12.1 | MIT | Cargo metadata; cargo-deny for selected targets |
| `pin-project-lite` | 0.2.17 | Apache-2.0 OR MIT | Cargo metadata; cargo-deny for selected targets |
| `pkg-config` | 0.3.34 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `potential_utf` | 0.1.6 | Unicode-3.0 | Cargo metadata; cargo-deny for selected targets |
| `proc-macro2` | 1.0.107 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `quote` | 1.0.47 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `r-efi` | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | Cargo metadata; cargo-deny for selected targets |
| `rustix` | 1.1.5 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Cargo metadata; cargo-deny for selected targets |
| `rustversion` | 1.0.23 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `schannel` | 0.1.29 | MIT | Cargo metadata; cargo-deny for selected targets |
| `security-framework` | 3.7.0 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `security-framework-sys` | 2.17.0 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `serde` | 1.0.229 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `serde_core` | 1.0.229 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `serde_derive` | 1.0.229 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `serde_json` | 1.0.154 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `shlex` | 2.0.1 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `siphasher` | 1.0.4 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `slab` | 0.4.12 | MIT | Cargo metadata; cargo-deny for selected targets |
| `smallvec` | 1.16.3 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `stable_deref_trait` | 1.2.1 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `syn` | 2.0.119 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `syn` | 3.0.9 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `synstructure` | 0.14.0 | MIT | Cargo metadata; cargo-deny for selected targets |
| `tempfile` | 3.27.0 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `tinystr` | 0.8.4 | Unicode-3.0 | Cargo metadata; cargo-deny for selected targets |
| `unicode-ident` | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 | Cargo metadata; cargo-deny for selected targets |
| `ureq` | 2.12.1 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `url` | 2.5.8 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `utf8_iter` | 1.0.4 | Apache-2.0 OR MIT | Cargo metadata; cargo-deny for selected targets |
| `vcpkg` | 0.2.15 | MIT/Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `wasm-bindgen` | 0.2.129 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `wasm-bindgen-macro` | 0.2.129 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `wasm-bindgen-macro-support` | 0.2.129 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `wasm-bindgen-shared` | 0.2.129 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `windows` | 0.58.0 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `windows-core` | 0.58.0 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `windows-implement` | 0.58.0 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `windows-interface` | 0.58.0 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `windows-link` | 0.2.1 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `windows-result` | 0.2.0 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `windows-strings` | 0.1.0 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `windows-sys` | 0.59.0 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `windows-sys` | 0.61.2 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `windows-targets` | 0.52.6 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `windows_aarch64_gnullvm` | 0.52.6 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `windows_aarch64_msvc` | 0.52.6 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `windows_i686_gnu` | 0.52.6 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `windows_i686_gnullvm` | 0.52.6 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `windows_i686_msvc` | 0.52.6 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `windows_x86_64_gnu` | 0.52.6 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `windows_x86_64_gnullvm` | 0.52.6 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `windows_x86_64_msvc` | 0.52.6 | MIT OR Apache-2.0 | Cargo metadata; cargo-deny for selected targets |
| `writeable` | 0.6.4 | Unicode-3.0 | Cargo metadata; cargo-deny for selected targets |
| `yoke` | 0.8.3 | Unicode-3.0 | Cargo metadata; cargo-deny for selected targets |
| `yoke-derive` | 0.8.4 | Unicode-3.0 | Cargo metadata; cargo-deny for selected targets |
| `zerofrom` | 0.1.8 | Unicode-3.0 | Cargo metadata; cargo-deny for selected targets |
| `zerofrom-derive` | 0.1.8 | Unicode-3.0 | Cargo metadata; cargo-deny for selected targets |
| `zerotrie` | 0.2.5 | Unicode-3.0 | Cargo metadata; cargo-deny for selected targets |
| `zerovec` | 0.11.8 | Unicode-3.0 | Cargo metadata; cargo-deny for selected targets |
| `zerovec-derive` | 0.11.6 | Unicode-3.0 | Cargo metadata; cargo-deny for selected targets |
| `zmij` | 1.0.23 | MIT | Cargo metadata; cargo-deny for selected targets |

## Data

- **IANA Time Zone Database (tzdb) 2025b** is embedded by `chrono-tz`. The tz database is public domain
  (the `tz` distribution's `LICENSE` permits use and redistribution).
- **Windows-to-IANA mapping**: `crates/session-engine/data/windows-zones.tsv` is a pinned, reviewable subset of
  CLDR `windowsZones` version 48 (001 defaults plus selected territory overrides). It is hand-maintained in
  this repository, not copied wholesale. CLDR is published by the Unicode Consortium under the
  Unicode License v3, approved for this project by the Manager on 2026-10-28. The full licence text is in
  [`crates/session-engine/data/LICENSE-UNICODE-CLDR.txt`](crates/session-engine/data/LICENSE-UNICODE-CLDR.txt).
  Credit: "Contains data from the Unicode Common Locale Data Repository (CLDR), Copyright © 1991-2026
  Unicode, Inc." Unknown mappings fail visibly rather than guessing.
- **Pine Script and MQL5 files** contain only project code and generated project data. No third-party
  indicator code is reused. TradingView and MetaQuotes are not affiliated with this project; MT5 is a
  MetaQuotes trademark.

## Existing application dependencies

The quote strip's existing dependencies (serde, serde_json, ureq, native-tls, windows-sys, windows and their
transitive crates) are unchanged by this feature and remain recorded in `Cargo.lock`. Windows itself is not a
third-party component.

## Bundled visual assets

`assets/icon.ico` and `assets/icon_256.png` are generated by `assets/make_icon.py` from project
shapes; `docs/strip_next_to_tray.png` is a project screenshot. Pillow is a build-time tool for
regenerating the icons and is not bundled. No separate third-party image attribution was found.

Unicode-3.0 applies to CLDR and to ICU data crates listed above. Retain their copyright and
permission notices with redistributed copies; the CLDR notice and full licence are linked above.
The IANA tzdb public-domain dedication is documented by the embedded tzdb distribution.
