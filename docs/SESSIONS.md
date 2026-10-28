<!-- SPDX-License-Identifier: MIT -->
# Session timers — rules, generated files and limits

made with love by maria. MIT. Not financial advice. Not affiliated with TradingView. Not affiliated with
MetaQuotes; MT5 is a MetaQuotes trademark.

## One source

[`spec/sessions.json`](../spec/sessions.json) is the single source for the four ordered sessions, the
London–New York overlap, the broker preset and the policies. It is versioned (`spec_version` 1.0.0) and pinned
to tzdb 2025b. The generated files are build inputs — never edit them by hand:

| Output | What it contains |
| --- | --- |
| `crates/session-engine/src/generated_rules.rs` | Rust session/broker constants and coverage |
| `crates/session-engine/src/generated_tables.rs` | Rust bounded IANA transition tables |
| `pine/MH_Sessions.pine` | Pine v6 indicator with generated session strings |
| `mql5/MH_Sessions.mq5` | MQL5 indicator with generated constants and transition tables |

Regenerate and check drift:

```sh
cargo run -p mh-specgen
cargo run -p mh-specgen -- --check
```

`--check` exits non-zero if a generated file differs from the spec. The generator also rejects a changed
policy it does not implement.

## Session rules (v1)

The fixed display order is **Sydney, Tokyo, London, New York, broker, device**. The four sessions use these
common FX-session conventions:

| ID | Zone | Local hours | Days |
| --- | --- | --- | --- |
| `sydney` | `Australia/Sydney` | 07:00–16:00 | Mon–Fri |
| `tokyo` | `Asia/Tokyo` | 09:00–18:00 | Mon–Fri |
| `london` | `Europe/London` | 08:00–17:00 | Mon–Fri |
| `new_york` | `America/New_York` | 08:00–17:00 | Mon–Fri |

**Approval status: approved by the owner on 2026-10-28.** These are conventions, not guaranteed exchange
hours. No holiday or exceptional
closure calendar is applied in v1; a holiday can invalidate a countdown. Tokyo has no DST.

Interval and DST policies:

- intervals are half-open `[open, close)`: a close instant is already closed;
- an ambiguous local time (fall-back) uses the **earliest** occurrence;
- a nonexistent local time (spring-forward) moves **forward one hour at a time** until it exists;
- the overlap is the UTC intersection of the London and New York intervals.

The engine is `crates/session-engine` (crate `mh-session`). It is platform independent, uses `chrono` and
`chrono-tz` with the compiled IANA database, and needs no external zoneinfo on Windows.

## Broker server time

A broker rule has a winter/summer UTC offset, an optional US-DST seasonal switch, a local reopen time,
eligible reopen days and provenance. The default preset is **Exness-style, approved by the owner on 2026-10-28 as the default, and still not
verified against a live server**: UTC+0 year-round,
weekly reopen estimated at Sunday 22:00 server. The reported observation includes 22:00/23:00 depending on US
DST; that has not been independently confirmed and daily maintenance reopens are not modelled.

Because the rule is unverified, the engine **does not present a verified next reopen**. The panel shows the
server clock and "reopen unknown · est …". Config overrides:

```json
{
  "sessions": {
    "enabled": true,
    "broker_offset_min": 120,
    "broker_reopen_min": 0,
    "broker_rule_verified": true,
    "device_zone": null
  }
}
```

- `broker_offset_min` forces a fixed offset; it does not claim a seasonal rule.
- `broker_reopen_min` is minutes after server midnight.
- `broker_rule_verified` must only be set after checking the broker's own server. With it, the next reopen is
  calculated as a UTC instant and shown as a countdown.
- `device_zone` is a manual IANA fallback. The device row is then labelled **configured**, never
  auto-detected.

The broker clock is an estimate, not a live feed. Changing `broker_offset_min` or `broker_reopen_min` while
leaving `broker_rule_verified` false keeps the unknown state.

## Windows device clock

`src/device.rs` reads `GetDynamicTimeZoneInformation` (the dynamic Windows key) and the user's territory
(`GetUserDefaultGeoName`, then the locale as a fallback). `mh_session::zones` maps the key through a pinned,
reviewable subset of CLDR `windowsZones` (version 48, 001 defaults plus selected territory overrides). Unknown
keys or failed mappings show the single device row as **unavailable**; the app never guesses.

The panel refreshes from wall-clock instants on every draw (a 500 ms UI timer), and re-reads the device zone
and recomputes on `WM_SETTINGCHANGE`, `WM_TIMECHANGE` and `WM_POWERBROADCAST` (resume/clock/zone changes).
Timers are independent of quote fetching.

Exactly one device row is always present, even when its zone equals a session zone. The rows are never
reordered or replaced.

## Coverage and versions

- Generated coverage: **2024-01-01T00:00:00Z to 2036-01-01T00:00:00Z**.
- tzdb: **2025b**, via `chrono-tz = "=0.10.4"`.
- Spec: **1.0.0**.
- The MQL5 tables fail visibly outside coverage; the Rust engine marks status as outside coverage too.
- Regenerating after a tzdb or CLDR update requires re-running the golden vectors and reviewing the diff.

## Golden vectors

[`spec/golden/vectors.json`](../spec/golden/vectors.json) holds shared UTC instants and expected session,
overlap and broker results. Rust tests run every vector. The cases cover:

- winter opens/closes and the overlap;
- the March US-before-UK mismatch and the late-October UK-before-US mismatch;
- Sydney's April and October DST transitions and Tokyo's lack of DST;
- Friday close, weekend reopen, leap day and year rollover;
- exact open/close instants (half-open intervals);
- fixed-offset and US-DST broker reopen examples.

The Pine and MQL5 files are generated from the same rules and numbers, but this machine has no TradingView or
MT5 terminal, so their runtime behaviour is **not verified here** (see [TESTING.md](../TESTING.md)).

## Pine v6 boilerplate

`pine/MH_Sessions.pine` is a table-only, display-only indicator. Important limits:

- Pine scripts **cannot read the chart time zone or the device time zone**. `syminfo.timezone` is the
  symbol's exchange zone, not the chart or device zone. The one extra clock is therefore a manual
  `input.string` IANA dropdown, defaulting to `syminfo.timezone`, explicitly labelled "set to your
  chart/device zone" and "not auto-detected".
- Session membership uses `time(timeframe.period, generated_session_string, IANA_zone)`. The chart timeframe
  affects the granularity, and the table updates on bar events, not on a wall clock.
- The table shows the last bar time. Historical bars are not re-rendered into a live clock.
- This boilerplate does **not** show session countdowns; the native MH Ticker panel is the accurate countdown
  surface. It does show the London–New York overlap state from the generated rules and a broker reopen
  estimate from the generated weekly rule.
- The broker offset input is an estimate, not a live server feed.

## MQL5 boilerplate

`mql5/MH_Sessions.mq5` is strictly read-only: no order, position, trade or account functions. It uses
`TimeTradeServer()` and `TimeGMT()` to estimate the server-minus-GMT offset, with an explicit override input
and an unverified-rule default. It never applies today's offset retrospectively to historical bars.

- MQL5 has no assumed general IANA database. The generated transition tables are bounded to 2024–2036; outside
  them the indicator shows `outside tzdb table` and does not invent a time.
- Session DST comes from the generated IANA tables evaluated at UTC instants, not from broker server time.
- The device clock is `TimeLocal()` with its offset from `TimeGMT()`, visibly labelled and shown unavailable
  if the difference is implausible. Strategy-tester semantics may differ; the indicator says so.
- Timer and local-clock behaviour, disconnects and tester semantics must be verified on the target terminal
  before claiming automatic detection.

## Config and migration

The new `sessions` object is optional in `config.json`. Old configs load unchanged and get the session
defaults written in; quote settings, symbols, placement and positions are untouched. `sessions.enabled`
defaults to true.
