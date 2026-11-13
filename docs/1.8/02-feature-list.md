# What MH Ticker could gain: the full list

*2026-11-13, against 1.7.1. Produced by running [01-discovery-prompt.md](01-discovery-prompt.md) over
the documents and the code. Every item has a verdict. BUILD NOW items are grouped into six work
packages in [03-work-packages.md](03-work-packages.md); LATER and NO keep their reasons so the next
pass does not have to rediscover them.*

Two facts shaped the list:

- **The sandbox this was planned in cannot reach CNBC** (the proxy refuses `quote.cnbc.com` and
  `ts-api.cnbc.com`). Anything that depends on a response shape nobody has measured is either built
  defensively and marked unverified, or left for LATER. `FEATURES.md` §3 and Appendix A are the
  measured record this list leans on.
- **Nothing here can be clicked on a Windows desktop before it ships.** Tests run under Wine and on
  the native CI runner; the window itself is reviewed from rendered PNGs and from the code. Items
  whose value is all in the feel of a live window rank lower for that reason alone.

- **The RAM ceiling doubled while wave 1 was being built.** The owner's instruction on 2026-11-13:
  the budget may go up to twice what the app uses today (about 5.8 MB, so about 11.5 MB). Most of
  this list was never limited by RAM, so most of it is unchanged. Three things moved: the range cache
  (B6) no longer has to be thrown away when the pane closes, volume bars (B8) went from LATER to NOW,
  and the 12-symbol cap, which was a byte budget and not a design limit, is raised (E5). Classic gets
  the same ceiling and no reason to use it.

## A. Alerts (Ultra)

The most under-built feature for its value: four presets, no way to silence, no record of what fired.

| # | Item | Evidence | Cost / state | Verdict |
|---|---|---|---|---|
| A1 | More presets: ±0.5 / 1 / 2 / 5 % of the current price | `ALERT_STEPS` has four; README: "arbitrary levels are a one-line config edit for now" | 0 bytes; menu only | **NOW (P1)** |
| A2 | Levels from data already parsed: today's high, today's low, previous close | `Quote.high/low/prev_close` are parsed and drawn, never offered | 0 bytes | **NOW (P1)** |
| A3 | Day-move alerts (`±N %` on the day) | `FEATURES.md` §5.3 "free, derived from data already parsed", not shipped | one `Option<f64>` per alert | **NOW (P1)** |
| A4 | Fire once, then clear | an alert past its level repeats every cooldown for as long as the app runs | one `bool` per alert | **NOW (P1)** |
| A5 | Mute: 1 h, 8 h, until turned back on | `FEATURES.md` §5.3 "the thing you want in a meeting", not shipped | two config fields; time-based, no timer | **NOW (P1)** |
| A6 | Quiet hours (local time, wraps midnight) | `FEATURES.md` §5.3 "a ticker that pings at 3 a.m. gets uninstalled", not shipped | one config field | **NOW (P1)** |
| A7 | A visible fallback when Focus Assist eats the balloon: tray icon marker and tooltip line until looked at | `FEATURES.md` §5.3; today only the panel footer shows it | one `bool`; cleared on expand or menu | **NOW (P1)** |
| A8 | Recent alerts (last 10, with outcome: shown, muted, webhook failed) | a swallowed balloon leaves no trace once the footer is overwritten | 10 entries in memory, not on disk | **NOW (P1)** |
| A9 | Webhook without a config edit: paste from clipboard, send a test, remove | README: webhook is config-only; no way to learn it works until a real alert | reads the clipboard on an explicit click; host only is ever shown | **NOW (P1)** |
| A10 | An exact level, typed | README, same quote as A1 | needs a text prompt (see F1) | **NOW (P6)** |
| A11 | Alert levels drawn on the candle pane; right-click the pane to set one at that price | the chart is where a level is chosen by eye | painter + one small menu | **NOW (P6)** |
| A12 | Alert sound | balloons already carry the system sound | n/a | NO: Windows plays one already |
| A13 | Webhook delivery that survives quitting mid-POST | `STATE-MACHINES.md` "High: possible misdelivery on process exit" | a policy decision, not a feature | LATER: belongs to the state-machine pass |

## B. The chart (Ultra)

| # | Item | Evidence | Cost / state | Verdict |
|---|---|---|---|---|
| B1 | Crosshair and O/H/L/C readout on the candle pane | the sparkline has a hover readout, the big chart has none | pointer state; painter and hit test share one layout | **NOW (P2)** |
| B2 | Local time on the time axis (UTC one click away) | `panel::clock` prints UTC; a user in UTC+8 reads the wrong hour | one Win32 call; buckets cut on local boundaries | **NOW (P2)** |
| B3 | Previous close, day high and day low as reference lines | the card shows all three as numbers; the chart shows none | 0 bytes | **NOW (P2)** |
| B4 | A visible break where the market was closed | `STATE-MACHINES.md`: "a market closure is not drawn as proportional empty time" | one pure helper | **NOW (P2)** |
| B5 | Day change beside the last price in the pane header | the pane names the price and not the direction | 0 | **NOW (P2)** |
| B6 | Ranges: 1D, 5D, 1M, 6M, 1Y, 5Y | `FEATURES.md` §3 measured the endpoints; §7 "daily candles: yes" | on-demand only, for the locked symbol while the pane is open; about 6 to 40 KB gzipped per fetch; a cache of at most twelve series (about 0.3 MB at worst) that outlives the pane, so flipping ranges and reopening are instant | **NOW (P4)**, unverified live |
| B7 | `--probe` reports every range | `--probe` is the project's seam for "does it still fetch and parse" | 0 | **NOW (P4)** |
| B8 | Volume bars under the candles, where the series has volume | the payload has per-bar volume and the app drops it | `Bar` grows from 20 to 24 B (about 0.15 MB at 24 symbols); absent for FX and spot metals, where the strip is simply not drawn | **NOW (P4)**: was LATER on bytes held; the new ceiling pays for it |
| B9 | Moving averages, VWAP, indicators | n/a | scope | NO: a ticker, not a charting package |
| B10 | Chart in the classic build | n/a | n/a | NO: classic stays one window, one look |
| B11 | Remember chart and lock across restarts | `STATE-MACHINES.md` designs around the chart not being saved | interacts with `compact_frame` | LATER: real value, real state risk |

## C. Everyday use (both builds)

| # | Item | Evidence | Cost / state | Verdict |
|---|---|---|---|---|
| C1 | Start with Windows | a taskbar tool that must be started by hand after every boot | one registry value under HKCU; the registry is the source of truth, not the config | **NOW (P3)** |
| C2 | Global hotkey to open and close the panel | the panel opens only by mouse | off by default; a failed registration is shown, not hidden | **NOW (P3)** |
| C3 | Refresh now | backoff reaches 60 s after failures and nothing lets the user retry | one counter on the board | **NOW (P3)** |
| C4 | Strip rows, text size and opacity from the menu | `dock_rows`, `dock_font_pt`, `font_pt`, `opacity` are config-only | 0; defaults unchanged | **NOW (P3)** |
| C5 | Version line and "Check for updates" | there is no way to learn a release exists | one request to `api.github.com`, only on click; never opens a URL taken from the response | **NOW (P3)** |
| C6 | Copy all prices | Copy exists per card only | 0 | **NOW (P3)** |
| C7 | Tell the user when settings could not be saved | `STATE-MACHINES.md`: "saves ignore write/rename errors" | one flag | **NOW (P3)** |
| C8 | Proxy setting | ureq does not read the system proxy | cannot be verified here | LATER |
| C9 | Reload config without a restart | README: "if u edit it by hand restart the app after" | every `apply_*` path at once | LATER: P1, P3 and P6 remove most reasons to hand-edit |
| C10 | Pause polling while the session is locked | efficiency ethos | must not pause when alerts are armed | LATER |
| C11 | First-run balloon | onboarding | n/a | NO: the strip is already on screen |
| C12 | Automatic update / self-replace | n/a | security surface | NO: C5 tells, the user decides |

## D. Keyboard, cards and order

| # | Item | Evidence | Cost / state | Verdict |
|---|---|---|---|---|
| D1 | Keyboard in the open panel: arrows move the lock, Enter opens CNBC, C chart, S picker, Ctrl+C copy, Delete removes | `wndproc` handles Esc and paging only | pure key map; Ultra where it needs the lock | **NOW (P5)** |
| D2 | Card menu: Remove from strip, Move left, Move right, Move first | removing a symbol means Symbols menu, find, untick; order is fixed by the catalogue | pure functions on `Config` | **NOW (P5)** |
| D3 | Reorder that sticks | `config::toggle` keeps catalogue order, so order cannot be chosen | permutes the catalogue, so the "rows keep catalogue order" rule and its test stay true | **NOW (P5)** |
| D4 | Click a symbol on the strip: the panel opens with that card locked (Ultra) | a click anywhere on the strip opens the panel on nothing in particular | per-cell extents in the strip frame | **NOW (P5)** |
| D5 | Sort cards by day move | watchlist staple | a view permutation through painter, hit tests, history and bars | LATER: wide blast radius |
| D6 | Drag cards to reorder | n/a | a drag state machine | LATER: D2 and D3 cover the need |

## E. Symbols

| # | Item | Evidence | Cost / state | Verdict |
|---|---|---|---|---|
| E1 | Add any CNBC symbol from the menu, checked against CNBC before it is added | README: "add it here [the config] and then tick it in the menu" | a text prompt; one quote request on click; in-flight state with a way out | **NOW (P6)** |
| E2 | Remove a hand-added symbol from the list | follows from E1 | 0 | **NOW (P6)** |
| E3 | A bigger default catalogue, grouped | 23 symbols, no stocks | every symbol must be checked live ("checked live on 2026-10-15" is a promise) | LATER: needs network to CNBC |
| E4 | Computed rows (gold/silver ratio) | n/a | no chart, no day change | LATER |
| E5 | Up to 24 symbols at once (was 12) | `config::MAX_ROWS`; `FEATURES.md` §7 refused this on bytes, and once any symbol can be added (E1) twelve fills quickly | about 90 KB of RAM a symbol (an hour of samples and a day of bars), so +1.1 MB at 24; traffic is the user's choice and the README says what it costs (about 12 MB/h at 24 symbols and a 3 s poll) | **NOW (P6)** |

## F. Building blocks

| # | Item | Why | Verdict |
|---|---|---|---|
| F1 | A one-line text prompt (native dialog from an in-memory template) | A10 and E1 both need the user to type; nothing in the app can take text today | **NOW (P6)** |
| F2 | One layout struct for the candle pane, shared by painter and hit test | B1, B6 and A11 all need "which candle, which price" to agree with the pixels | **NOW (P2)** |

## G. Looked at and left

| Item | Verdict |
|---|---|
| Sparkline day span (1 h / day), the 24 h ribbon (`FEATURES.md` §7.3) | LATER: the card is full; needs a design, not just code |
| Tick flash on the strip, heat tint on cards | LATER: pure feel, cannot be judged without a live window |
| Panel zoom | LATER: every `u.dpi` call site must agree |
| Copy the panel as an image | LATER |
| Typed chart decode (`FEATURES.md` §4.2) | LATER: still right, still invisible |
| Portfolio, holdings, P&L | NO: a different product, and it would hold private data |
| News headlines | NO: steady traffic, scope |
| Telemetry | NO |
| Web view settings page, local server, history on disk | NO: `FEATURES.md` §8 |

## Count

35 items BUILD NOW in 6 packages, 14 LATER, 9 NO.
