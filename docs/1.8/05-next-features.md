# What MH Ticker could gain next: the list after 1.8

*2026-11-13, against 1.8. Written from the 1.8 survey ([02-feature-list.md](02-feature-list.md)) and
the six package notes ([notes/p1.md](notes/p1.md) to [notes/p6.md](notes/p6.md)) and nothing else. The
owner's reading limit kept the source, the README and the design documents closed for this pass, so
every statement about what the app does today is cited to one of those seven files, and everything
that could not be checked is in "Unsure" near the end. Every item has a verdict. BUILD NEXT items are
ranked and grouped into four work packages at the bottom; LATER and NO keep their reasons.*

## Budget

- **Ultra:** private RAM up to about 23 MB (twice the 1.8 ceiling of about 11.5 MB, which was twice
  the 5.8 MB of 1.7.1).
- **Classic:** stays at about 11.5 MB and must not drift: same pixels, same click behaviour. An engine
  feature that reaches Classic defaults to exactly what Classic does today.
- **Both:** one exe under 2 MB. Idle CPU near zero. No new dependency. No server, web view, installer,
  login, telemetry or history files on disk. No new steady traffic beyond what the user's own choices
  cost; any other request only on an explicit click, to a host the app already names. Never alert on
  stale or fallback data. A webhook URL is a secret.

## What shaped the list

- **RAM is no longer what limits Ultra.** The whole BUILD NEXT list spends about 1 MB of the 11.5 MB
  of new headroom (tally below). Most things more RAM could hold would have to be fetched first, and
  fetching every range for every symbol is new steady traffic. Inside the rule, RAM buys only "keep
  more of what a click already fetched" (L3) and, later, more symbols (J8).
- **The limits that bind are traffic and proof.** P6: "The size of that cost at 24 was not measured."
  P4: "Every CNBC range shape is unverified live." Each of the six notes ends in an Unverified section
  and none of the six was clicked on Windows. So the first package is about knowing, not adding.
- **1.8 built the blocks that make old LATER items cheap:** a text prompt (P6), order as a permutation
  of the catalogue (P5), a `kick` that ends a wait at once (P3), one chart layout for painter and hit
  test (P2), a click rectangle per strip symbol (P5), an alert log with outcomes (P1).
- **Classic.** Every item marked "both" is a menu entry or a config field whose default is the 1.8
  behaviour. Two accessibility items (M3, M6) are where that rule costs the Classic user something.
  They are built off by default on Classic and flagged for the owner.

Columns: *Cost* is RAM, bytes on the wire and when, exe size (an estimate, see Unsure), threads and
timers. *Off Windows* is what cannot be verified without a Windows desktop.

## 1. The 1.8 LATER and NO lists, weighed again

| # | Item and build | Evidence | Cost | State, way back | Pure part | Off Windows | Verdict |
|---|---|---|---|---|---|---|---|
| A13 | Webhook tried once more after a network or 5xx failure, and a quit that waits up to 2 s for a POST in flight (Ultra) | 02 A13 was LATER as "a policy decision, not a feature". P1 now logs `send failed` in Recent, so the policy has somewhere to report | RAM 0; one extra POST, only after a failure; about +2 KB; no thread (the alert thread sleeps 5 s), no timer | One attempt count per POST, in memory. A 4xx is never retried. No queue on disk (budget) | `retry::next(outcome, attempt)`; the outcome text `sent on retry` | The quit path with a real POST in flight; a real 5xx | **BUILD NEXT (W2)**: small once outcomes are logged |
| B11a | The chart reopens on the range last used (Ultra) | P4: "Closing the chart resets the range to `1D`." The range alone has none of the state risk 02 B11 named | RAM 0; one range file (6 to 40 KB, 02 B6) on the click that opens the chart, only if not cached; under 1 KB | `chart_range`, default `1D`. Way back: click `1D` | Round trip; an unknown value reads as `1D` | Nothing beyond P4's own list | **BUILD NEXT (W3)** |
| B11b | Chart open and lock restored at start (Ultra) | 02 B11: "interacts with `compact_frame`"; that document was not read | A range fetch at start with no click, unless it waits for the first expand | Two saved fields; the exits are unclear | None found | The start-up morph | LATER: the same state risk, and now a traffic question |
| C8 | Proxy setting (both) | 02 C8: "cannot be verified here". The prompt (P6) removes the config edit and a loopback listener in a test could check the CONNECT line | RAM 0; wire 0 new | A proxy URL can carry a password, so it is a secret like the webhook: host only is shown | Validation and redaction (P1's `redact`) | System proxy discovery; signed-in company proxies, which are the ones that block users | LATER: the cheap half is not the half users need |
| C9 | Reload config without a restart (both) | P1, P3 and P6 gave menus to alerts, strip, hotkey, symbols and levels; M2 types the rest | Every apply path at once | n/a | n/a | All of it | LATER, close to NO: after M2 little is left to hand-edit |
| C10 | Polling rests while the session is locked and no alert is armed (both; off by default on Classic) | 02 C10 LATER: "must not pause when alerts are armed". 02 E5: "about 12 MB/h at 24 symbols"; a locked PC overnight is about 100 MB nobody reads. P3's `kick` gives the instant refresh on unlock | RAM 0; wire goes down (one poll per 5 min while locked and unarmed); about +2 KB; no thread, no timer, one session-notification registration | `rest_when_locked`: Ultra on, Classic off. Unlock, expand, hotkey or Refresh now kick at once. Way back: untick | `governor::interval(base, quiet_for, locked, armed)`, shared with H6 | The lock and unlock messages arriving | **BUILD NEXT (W2)**: the armed rule is one argument of a pure function |
| D5 | Order the cards once: by day move, by name, or back to the default order (both) | 02 D5 LATER for "a view permutation through painter, hit tests, history and bars". P5 made order a permutation of the catalogue, so a one-time reorder touches none of those | RAM 0; wire 0; about +1 KB | No new state: it rewrites the order P5 already saves. Way back: J2 undo, or Default order | `order_by(config, quotes, key)`; rows with no fresh quote keep their relative place at the end | The menu | **BUILD NEXT (W3)** as a one-shot. A live sort stays LATER for the 1.8 reason |
| D6 | Drag cards to reorder | P5 added Ctrl+Left, Ctrl+Right and three menu entries; D5 adds a fourth way | A drag state machine | Drag state with cancel paths | Little | All of it | NO: the need is met four ways, and none of it can be exercised off Windows |
| E3 | A bigger default catalogue, grouped (both) | 02 E3: "every symbol must be checked live". Still no route to CNBC from the build sandbox | 0 | Catalogue only | The list itself | n/a | LATER: waits on H1, which lets anyone with a network run the check |
| E4 | Computed rows such as gold/silver (both) | The prompt could take `XAU/XAG`, but P4, P5 and P6 all assume a row is one CNBC symbol (range files, Enter opens CNBC, lookup, presets) | Small RAM; no wire | A second kind of row everywhere | Ratio and its day change from two quotes | The card | LATER, with L4: the same two-series machinery |
| G1 | Sparkline day span, 24 h ribbon | 02 G: "the card is full; needs a design, not just code" | RAM is no longer the obstacle (N1) | n/a | n/a | The look | LATER: still a design question |
| G2 | Tick flash on the strip, heat tint on cards | 02 G: "pure feel" | A repaint timer | Would have to obey M6 | n/a | All of it | LATER |
| G3 | Panel zoom: 75, 90, 100, 125, 150 % (Ultra) | 02 G: "every `u.dpi` call site must agree". P2 already tests `index_at(x_of(i)) == i` at 96, 120, 144 and 192 DPI, and stills exist at 96 and 144. P6: "The panel's grid for 24 cards is taller than a 1080p work area", so zoom below 100 has a use too | RAM: a larger back buffer above 100 %; wire 0; about +1 KB | `panel_zoom`, default 100. Way back: 100 % | Effective DPI is DPI times zoom; 150 % at 96 DPI must render byte for byte like 100 % at 144 DPI | Real fonts at odd sizes; window size on a small screen | **BUILD NEXT (W4)**: the still-equality test is the proof 1.8 lacked |
| G4 | Copy the panel as an image | 02 G | A bitmap on the clipboard | None | None | All of it | LATER |
| G5 | Typed chart decode | 02 G: "still right, still invisible" | 0 | None | All of it | Nothing | LATER: H4 ships the visible part (a series that says when it is suspect) |
| A12 | Alert sound | 02 A12 | n/a | n/a | n/a | n/a | NO: Windows plays one already |
| B9 | Moving averages, VWAP, indicators | 02 B9. Volume (P4) does not change it; L1 is a number, not an indicator | n/a | n/a | n/a | n/a | NO: a ticker, not a charting package |
| B10 | Chart in Classic | 02 B10; the budget says Classic must not drift | n/a | n/a | n/a | n/a | NO |
| C11 | First-run balloon | 02 C11 | n/a | n/a | n/a | n/a | NO: the strip is already on screen |
| C12 | Automatic update | 02 C12; P3 keeps the check on a click and opens only a fixed address | n/a | n/a | n/a | n/a | NO |
| G6 | Portfolio, holdings, P&L | 02 G | n/a | Private data | n/a | n/a | NO |
| G7 | News headlines | 02 G | Steady traffic | n/a | n/a | n/a | NO |
| G8 | Telemetry | 02 G. H2 is a report the user copies and sends by hand; nothing leaves on its own | n/a | n/a | n/a | n/a | NO |
| G9 | Web view settings page, local server, history on disk | 02 G; the budget repeats it. Recent alerts stay in memory (K8) | n/a | n/a | n/a | n/a | NO |

## H. Trust: knowing the numbers are true (new)

| # | Item and build | Evidence | Cost | State, way back | Pure part | Off Windows | Verdict |
|---|---|---|---|---|---|---|---|
| H1 | `--probe` that judges: every shown symbol, every range, an unknown symbol and the release answer, each named assumption printed as PASS or WARN, plus the field names the parser does not read (both) | P4: "for the first row it fetches each range file" and a person reads the stamp. P4: "If the stamps of the daily files are not midnight on the exchange's clock, daily candles can be dated a day off." P6: "What CNBC returns for an unknown symbol is assumed to be no quote." P4: "All tests use hand-written JSON." | RAM 0 (command line only); about 50 to 250 KB per run, when the user runs it, to hosts the app already names; about +4 KB; no thread, no timer | None. Exit code is non-zero on any WARN | `probe::judge(bodies)` gives the findings; fixtures drive it | Only the live run itself, which needs a network and not a desktop | **BUILD NEXT (W1)**: it turns six Unverified sections into a checklist |
| H2 | Diagnostics > Copy report: the same checks from the menu, on a click, as text on the clipboard (both) | Every note says "Nothing was clicked on a real Windows desktop." The first users are the first testers; give them one thing to paste | Same wire cost as H1, on the click; RAM a few KB while it runs; about +3 KB; reuses the existing workers | A "Checking..." flag as in P3's update check; a second click is ignored. The report holds version, DPI, placement, counts, hotkey and autostart state, H5's totals and H1's findings | `report::text(facts)`; a test that the webhook shows as host only and the settings path carries no user name | The clipboard write; the facts read from Windows | **BUILD NEXT (W1)** |
| H3 | `--probe --dump <dir>`: the raw answers saved as files, to become test fixtures (both, command line only) | P4: "All tests use hand-written JSON." Real bodies are the largest single gain in proof | RAM 0; wire as H1; about +1 KB | Files only in a folder the user names on the command line. Never written by the running app, never the config or the webhook | Refuses a missing folder; file naming | Nothing | **BUILD NEXT (W1)**, with a flag for the owner: this reads "no history files on disk" as being about the running app. Strike it if the rule is stricter |
| H4 | A chart that says when it is suspect: `as of 14:05` when the series on screen is past its time to live, and `check dates` when a daily file's stamps do not sit on a day boundary (Ultra) | P4: "Old data stays on screen during a refresh." P4's midnight assumption, as H1. P4: bars with no finite close "are skipped" silently | RAM a few bytes per series; wire 0; about +2 KB | Flags computed from the series. Nothing saved | `Series::check()`; `age_label(now, fetched, ttl)` | The header with real fonts | **BUILD NEXT (W1)** |
| H5 | A data meter in the menu: `Data: 3.1 MB this hour, 41 MB since start` (both) | P6: "The size of that cost at 24 was not measured." 02 E5 promises "the README says what it costs"; the app can say what it did cost | RAM about 0.5 KB (two counters and a 60-slot ring); wire 0; about +2 KB; no thread, no timer | In memory, gone at quit. One greyed menu line | Ring arithmetic, roll-over and the line's text | Whether the HTTP library reports bytes as sent or as unpacked | **BUILD NEXT (W1)**: makes "traffic is the user's choice" a fact, and W2 needs its numbers |
| H6 | Slow down when nothing moves: no shown quote changed for 10 min, poll every 15 s; for 30 min, every 60 s (15 s at most while an alert is armed); the first change returns to full speed (both; off by default on Classic) | 02 E5: "about 12 MB/h at 24 symbols and a 3 s poll". A weekend of closed markets is about 49 h, near 590 MB at full rate and near 30 MB at 60 s | RAM 16 B; wire goes down by up to 95 % in quiet hours; about +2 KB; no thread, no timer (it changes the wait the feed already makes) | `slow_when_quiet`: Ultra on, Classic off. Any change, expand, hotkey or Refresh now returns to base. Way back: untick | `governor::interval`, as C10. The stale threshold must follow the governed interval | Nothing; the saving is read from H5 | **BUILD NEXT (W2)**. Limit: one symbol that trades all weekend keeps the whole board at full rate |
| H7 | Told when alerts could not be checked: a Recent line `02:10 to 02:31 not checked (no fresh price)`, a tooltip line while it lasts, and an optional webhook notice once per gap (Ultra) | Budget: "Never alert on stale or fallback data." P1 fires "only on a fresh quote", so an armed alert is silent through a dead feed or a night asleep and nothing says so | RAM about 100 B; wire 0 by default, at most two small POSTs per gap if the user ticks the notice; about +3 KB; no timer (checked on the feed tick; sleep is seen as a jump of the clock) | In memory: blind since when. `alerts_gap_notice`, default off. The notice never carries a price | `watch::step(now, fresh, armed)` gives `Blind` and `Back` events; the threshold is the larger of 5 min and ten polls | Sleep and resume on real hardware | **BUILD NEXT (W2)**: the missing half of the stale-data rule |
| H8 | One balloon when a mute or quiet hours end: `3 alerts while muted. See Alerts > Recent.` (Ultra) | P1: "A silenced `once` alert consumes its only level without anyone hearing it." P1: "lifting a mute does not release a burst" | RAM 8 B; wire 0; about +1 KB; no timer (P1: "A timed mute ends by the clock") | The log number of the last digest, in memory. `once` keeps the P1 rule; the digest is how it gets heard | `digest(log, since)` gives the count and the line | The balloon | **BUILD NEXT (W2)** |
| H9 | Recent says `not shown (Windows busy)` instead of `shown` when Windows would hold the balloon (Ultra) | P1: "Balloons (Focus Assist can suppress them)" yet the outcome is `shown` | One shell query per alert | None | The outcome mapping | Whether the query reports do-not-disturb at all on current Windows; it may see only full-screen and presentation states | LATER: an outcome that is right only sometimes is worse than P1's tray marker |
| H10 | Typed levels that cannot be misread: `4200,5` is refused with a reason, the parsed level is echoed in the symbol's own digits, and a level more than 20 % from the price asks first (Ultra) | P6: "A typed level is read with commas as thousands separators, with at most one decimal point." A user whose decimal mark is a comma types `4200,5`; read loosely that is 42,005, an alert that never fires and never complains | RAM 0; wire 0; about +1 KB | None. Way back: Clear this symbol's alerts (P1) | Grouping rule (a comma must be followed by exactly three digits), the echo and the 20 % test | The message boxes | **BUILD NEXT (W1)**. If grouping is already strict, the echo and the far-level question remain |
| H11 | Local time read per instant, not per paint: the mute label and 5D candles are right across a clock change (Ultra) | P1: "a DST change inside a mute window is an hour off." P2: "the offset is read with `GetTimeZoneInformation` on each paint", and P4's 5D range now spans days, so one offset is applied to bars on both sides of a change | RAM 0; wire 0; about +1 KB | None | Folding with an offset function instead of one offset; tests with a made-up rule that changes mid-series | The Windows call that gives an offset for a past instant | **BUILD NEXT (W1)**: two known bugs, one fix |
| H12 | Hardening sweep from the notes: a non-finite price prints `--`; `y_of(NaN)` stays in the plot; no candle past the plot's right edge; the update check sends an app name, not a browser string and not a version; cooldowns run on a clock that cannot jump (both) | P3 concern 2: "A non-finite price prints as `NaN` or `inf`." P2: "`Layout::y_of(NaN)` returns the bottom of the plot." P2 said the overflow "needs a span of several days at one-hour candles", which "the single-day feed does not produce"; P4's 5D now does. P3 concern 5 on the User-Agent | RAM 0; wire: the same one request on the click, other header text; under +1 KB | None | All of it | Nothing | **BUILD NEXT (W1)** |
| H13 | A config that will not parse is kept beside the new one, not overwritten, and the menu says so (both) | P3 shows a line when a save fails; no note says what a bad file does at start. A user with 24 symbols, levels and a webhook has something to lose | RAM 0; wire 0; about +1 KB; one extra file only after a failed parse | One flag and one greyed menu line, as P3's save line. Way back: fix or delete the kept file | `load` returns loaded, or defaulted with the kept path; the original bytes are intact | Nothing (a temp folder under Wine is enough) | **BUILD NEXT (W1)**, unless today's code already does this |
| H14 | A wider set of pictures and fit tests: 24 cards, 24 symbols on 1, 2 and 3 strip rows, the narrow pane, 192 DPI, each range; and for fixed-pitch faces a test that each header, legend and footer line fits by character count (test work, both) | P2: the compact legend "is covered by a unit test only"; "PNGs are rendered under Wine with substitute fonts, so text widths and line heights differ". P6: "24 symbols on a real taskbar" is unverified | No cost in the exe | None. New stills are added; the 36 old ones must stay identical | The fit test: characters times advance against the box, at each DPI | Real fonts, which is what the fit test stands in for | **BUILD NEXT (W1)** |

## J. A strip of twenty-four (new)

| # | Item and build | Evidence | Cost | State, way back | Pure part | Off Windows | Verdict |
|---|---|---|---|---|---|---|---|
| J1 | Pin to strip: the taskbar strip shows the symbols you pick, the panel shows all of them (both) | P6 raised the cap to 24 and lists "24 symbols on a real taskbar" as unverified. A taskbar has room for a few; today the strip and the panel show the same set | RAM 1 B per row; wire 0 (the same symbols are polled); about +2 KB | `strip` per row, absent means shown, so Classic and old configs are unchanged. The last pinned symbol cannot be unpinned. Way back: Show all on strip | `strip_rows(config)`; P5's click rectangles map through it to the right card | The strip at each placement | **BUILD NEXT (W3)**: what a 24-symbol user hits first |
| J2 | Undo the last removal, move, sort, pin change or alert clear: Ctrl+Z in the panel and one menu line (both) | P5: Delete removes the locked card at a key press. P6: "Removing one also removes its alerts." D5 rewrites the whole order | RAM one config copy (about 10 KB); wire 0 (a restored symbol is fetched on the next poll); about +2 KB | One snapshot in memory, replaced by the next such action, gone at quit. A removed webhook is never restored | `undo::take` and `restore`; config equality after each kind of action | The key press | **BUILD NEXT (W3)** |
| J3 | Rename a card and set its decimals (both) | P6: labels are "cut to 8 characters" and may end as `SYM`; "Decimal places come from the first price", which is wrong for a symbol first seen at an odd price | RAM about 16 B per row; wire 0; about +2 KB | A label and a digits override per symbol. Way back: Reset name, Decimals: auto. Levels already set keep their value | P6's label rules reused; collision suffix; digits clamp | The prompt and the menu | **BUILD NEXT (W3)** |
| J4 | Add several symbols in one prompt (`AAPL, NVDA, EUR=`) (both) | P6 adds one at a time, "at most 20 characters" | One lookup per symbol, on the click | An in-flight list with a way out | Split and clean | The prompt | LATER: filling a list is done once; J1 and J2 are felt every day |
| J5 | Alerts at a glance: one list of everything set with its distance (`XAU above 4,200.00, 1.2 % away`), a warning on levels whose symbol is not on the strip, Clear all, a dot on cards with a level, and Recent kept to 50 with Copy (Ultra) | P1 shows what is set one symbol at a time, across up to 24 submenus now. P1: "The last 10 alerts". P6 removes alerts with a removed symbol; no note says what unticking a symbol does to its levels | RAM about 5 KB (50 entries); wire 0; about +3 KB | No new saved state. Clear all asks first and J2 can undo it. The dot is Ultra only | `overview(config, quotes)`; distance; the cannot-fire test; log at 50 and 51 entries | The menu and the dot | **BUILD NEXT (W2)** |
| J6 | Find a card: `/` opens a prompt and locks the first match, Home and End lock the first and last card, and the body scrolls so the locked card is in view (Ultra) | P6: "The body scrolls." P5 moves the lock with arrows and does not say the view follows it | RAM 0; wire 0; about +2 KB | None. Esc in the prompt changes nothing | `find(rows, text)`: prefix before substring, case ignored; `scroll_to_show(card, view, scroll)` | Focus after the prompt closes | **BUILD NEXT (W3)** |
| J7 | Named sets of symbols to switch between | 24 symbols and any symbol addable (P6) | A list of lists in the config | Which set is live; per-symbol history dropped on a switch | Set operations | The menu | LATER: J1 covers it at a cap of 24; sets matter when the cap rises |
| J8 | 48 symbols in Ultra | 02 E5: "about 90 KB of RAM a symbol", so +2.2 MB, which the budget now covers. P6 sized two menu ranges for 24 | Traffic doubles at the user's choice | Menu ranges, panel height | The cap | A real taskbar | LATER: 24 has not been seen on a taskbar or measured yet (H5, H14 first) |
| J9 | Strip that rotates through pages | Follows from 24 | A repaint timer | Page state | Paging | All of it | LATER: pure feel, and a timer against idle CPU |
| J10 | Search for a symbol by company name | P6 needs the exact CNBC symbol | A lookup endpoint nobody has measured, likely on a host the app does not name | n/a | n/a | n/a | LATER: unmeasured shape, new host |

## K. Alerts, next (new, Ultra)

| # | Item | Evidence | Cost | State, way back | Pure part | Off Windows | Verdict |
|---|---|---|---|---|---|---|---|
| K1 | Repeat on each crossing: an alert fires when the price crosses its level, then waits until the price has gone back past it by a small band | P1: an alert past its level fires "between firings" every `cooldown_s` for as long as the price stays there, unless Fire once drops it. Day-move alerts repeat the same way | RAM 1 B per level; wire 0; about +2 KB | A third choice beside every cooldown (the default, as today) and once. The side last seen is in memory; the first fresh quote after a start fires if it is already beyond, so a crossing made while the app was off is reported once | All of it, in `alerts.rs`: band, re-arm, NaN never fires, a price exactly on the level fires | The menu radio | **BUILD NEXT (W2)** |
| K2 | Fast-move alert: `1 % in 5 min`, `2 % in 15 min`, `3 % in 60 min` | 02 E5: each symbol already holds "an hour of samples". Free, from data already in memory | RAM 16 B per alert; wire 0; about +2 KB | One optional field per alert. It needs the window covered by fresh samples with no gap longer than three polls, so it cannot fire off a stale stretch. Way back: clear | All of it: window cover, gap rule, threshold | The menu | **BUILD NEXT (W2)** |
| K3 | Several levels on one side (a ladder) | P1's example holds one `above`, one `below`, one day move. P6's typed levels and the chart's *Alert at* invite a second level | A config shape change | A list where there was a field; old files must still read | The list logic | The menu | LATER: a schema change for a need J5 first makes visible (it says when a new level replaces an old one) |
| K4 | Click the balloon to open the panel on that card | P5 already has "lock that card and open the panel" | A few lines | None | The action exists | All of it; balloon clicks differ by Windows version | LATER |
| K5 | A note on a level, shown in the alert | The prompt exists | A short string per level | Sanitising | The cleaner | The prompt | LATER, with K3 |
| K6 | Levels that expire at the end of the day | A "past today's high" preset is a fixed number tomorrow (P1) | A time per level | Which day, whose midnight | The clock rule | n/a | LATER: P4 shows how hard "whose midnight" is |
| K7 | Heartbeat to the webhook | Would prove the app is alive | Steady traffic | n/a | n/a | n/a | NO: steady traffic. H7 says when watching stopped, which is the honest version |
| K8 | Recent alerts kept across restarts | P1: "gone after a restart" | A history file | n/a | n/a | n/a | NO: history on disk |
| K9 | A warning at Exit that levels are set | Alerts are missed while the app is closed | n/a | n/a | n/a | n/a | NO: a nag. Start with Windows (P3) is the answer |
| K10 | Volume-spike alert | P4 gave bars a volume | n/a | n/a | n/a | n/a | NO: indicator ground (B9) |

## L. The chart, next (new, Ultra)

| # | Item | Evidence | Cost | State, way back | Pure part | Off Windows | Verdict |
|---|---|---|---|---|---|---|---|
| L1 | The header shows the change over the range on screen (`5Y +182 %`), and the range's high and low are marked | P2 puts "the day's change" beside the last price; on a 5Y chart (P4) that is the wrong number to lead with | RAM 0; wire 0; about +1 KB | None. On 1D the header is as P2 left it | `range_change(bars)` with P2's rules for a zero or non-finite open | Fit with real fonts | **BUILD NEXT (W3)** |
| L2 | Copy the chart's candles as tab-separated text | P3's Copy all prices is the pattern | 0 | None | `series_text` | The clipboard | LATER: cheap, but nobody has asked |
| L3 | The series cache holds 48 series, not 12, so coming back to a symbol and range already seen is instant | P4: "`Book`, at most 12 series, least recently used goes first"; twelve is two ranges each for six symbols, and there are 24 symbols now. 02 B6: "about 0.3 MB at worst" for twelve | RAM about +0.9 MB at worst; wire 0, provided a series is refreshed only while it is on screen; exe 0 | One constant, Ultra only. Way back: the constant | Eviction order; a test that a series off screen is never due | Nothing | **BUILD NEXT (W3)**: the one place the new RAM buys something at no traffic |
| L4 | Compare: a second symbol on the same chart, both as percent | The cache could hold both series | One more range file per refresh while the pane is open, on a click | Which symbol is compared; an exit | Rebase and join by time | The picture | LATER, with E4 |
| L5 | Wheel to zoom and drag to pan | P4's windows are fixed | 0 | A window with a reset | `Series::window` | The feel | LATER |
| L6 | Log scale and a line style for long ranges | 5Y exists now | 0 | Two toggles | `y_of` | The look | LATER: drifts toward B9 |
| L7 | Fetch the other ranges ahead of the click | Would make every tab instant | Up to five files per symbol with no click | n/a | n/a | n/a | NO: traffic the user did not ask for. L3 gets the same effect for ranges already opened |
| L8 | Extended-hours price and a market-closed mark | Quote fields nobody has measured | Unknown | n/a | n/a | n/a | LATER: H1 prints the unread field names, which is the measurement this needs |

## M. Reach: keyboard, contrast, size (new)

| # | Item and build | Evidence | Cost | State, way back | Pure part | Off Windows | Verdict |
|---|---|---|---|---|---|---|---|
| M1 | The chart by keyboard: Shift+Left and Shift+Right move the crosshair, R changes the range, Z flips the zone, A sets an alert at the crosshair price through the prompt, the menu key opens the card menu, F10 the main menu, F1 lists the keys (Ultra) | P5's key table stops at the cards. P2: "Move the pointer over a candle". P4: "A click on a tab". P6: "Right-click on the chart". P5: the footer names the keys "only when the line fits" | RAM 0; wire: a range file when R lands on a range not cached, as a click would; about +3 KB | The hovered candle can now be set by a key; a pointer move takes it back. Plain arrows keep P5's meaning | `keys::map` grows; `step_candle`; the help text comes from the same table, with a test that every action has a line | Real key presses and focus (already open in P5) | **BUILD NEXT (W4)**: a keyboard user cannot read a candle today |
| M2 | Type what is preset-only today: any hotkey, any quiet hours (`22:30-06:45`), a mute until a time, a cooldown (both for the hotkey; Ultra for the rest) | P3: a custom hotkey must be "set in the config file". P1: quiet hours are three fixed windows | RAM 0; wire 0; about +2 KB | No new fields; it writes the ones P1 and P3 made. A bad entry is refused with a fixed sentence and changes nothing | Parsers for a time span and a time; P3's hotkey grammar unchanged | The prompts | **BUILD NEXT (W4)** |
| M3 | High contrast: when Windows is in a contrast theme the app draws in the system's colours, and direction is carried by sign and shape as well as colour (Ultra follows Windows; Classic has one tick, off by default) | No note mentions contrast themes or any colour test. Themes are an Ultra feature already, so this is one more theme whose colours come from Windows | RAM about 1 KB; wire 0; about +3 KB; no timer (it reacts to the settings-changed message) | `contrast`: `auto` on Ultra, `off` on Classic. Way back: the tick | `Theme::from_system(colours)`; stills from two fixed palettes | Reading the real system colours; the change message | **BUILD NEXT (W4)**. Owner's call: Classic off by default keeps its pixels and fails the user who needs this most |
| M4 | Choose the up and down colours: green and red, red and green (red is up in several markets), or blue and orange (safe for the common colour-vision differences); an option for hollow up-candles (Ultra) | 02 B2 already had to fix the clock for "a user in UTC+8"; P4's review example is "Tokyo seen from Manila". Colour meaning differs there too | RAM 0; wire 0; about +1 KB | `updown`, default as today. Way back: the default | The palette swap touches only the two colours; stills for each | Nothing | **BUILD NEXT (W4)** |
| M5 | A contrast test over every theme: text against background at 4.5 to 1 or better, up and down against background at 3 to 1 (test work) | P1 to P6 test geometry and text, never colour | No cost in the exe | Ultra themes that fail are tuned and their stills replaced, each named. Classic is measured and reported, not changed | All of it | Nothing | **BUILD NEXT (W4)** |
| M6 | Reduced motion: when Windows has animations off, the panel opens and closes in one step (Ultra follows Windows; Classic has one tick, off by default) | P2 and P5 mention "collapse's animation" and "a morph" | RAM 0; wire 0; under +1 KB | `animate`: `auto` on Ultra, `on` on Classic. Way back: the tick | `frames(animate, system)` | Reading the system setting | **BUILD NEXT (W4)**. Same owner's call as M3 |
| M7 | A first step for screen readers: the window's name carries the locked card as a sentence (`Gold 4,012.30, up 0.4 percent`, `stale` when it is), or a one-line summary with no lock (Ultra) | The window is painted by the app, so a screen reader finds no text in it. P5 gives a lock that arrows move | RAM 0; wire 0; about +1 KB | None | `spoken_line(row, quote)` | All of the effect: whether a screen reader speaks a name change, or only on request | **BUILD NEXT (W4)**, ranked last: a small honest step |
| M8 | A full automation provider (every card and candle as an element) | The real answer to M7 | A COM provider; exe size unknown | A tree kept in step with the painter | Little | All of it | LATER: cannot be tested here at all |
| M9 | Moving between monitors of different DPI | P2 tests four DPIs one at a time | 0 | Rebuild on the change message | Layout at each DPI | All of it; whether it is already handled could not be checked | LATER |
| M10 | Tooltip on a strip symbol: full name, day high and low, age of the quote | P5's click rectangles | A tooltip window | Hover state | The text | All of it | LATER |

## N. What else the RAM could hold (new, Ultra)

| # | Item | Evidence | Cost | State, way back | Pure part | Off Windows | Verdict |
|---|---|---|---|---|---|---|---|
| N1 | A session tape: eight hours of the app's own samples at 15 s (the hour at full rate stays) | 02 E5: an hour is held now | About 23 KB a symbol, 0.55 MB at 24; wire 0 | A second ring per symbol | Downsampling | Nothing | LATER: it only matters once G1 has a design to draw it |
| N2 | Cached card bitmaps to save repaint time | 24 cards repaint | 3.5 MB at 96 DPI, four times that at 192 DPI | Invalidation | n/a | The saving | NO: spends most of the headroom on a cost nobody has measured |
| N3 | Repaint only the cards whose quote changed | Idle CPU "near zero" with 24 cards on a 3 s poll | 0 | Dirty rectangles | `changed(old, new)` | The saving; whether the painter already does this could not be checked | LATER: measure first |
| N4 | Slow down on a metered connection | H6's cousin | A COM query | One flag | The rule (H6's function) | All of it | LATER |

## Rank: value over (cost + risk)

Value is 1 to 10, cost and risk 1 to 5. Ties go to the higher value.

| Rank | # | Item | V | C | R | Score | Package |
|---|---|---|---|---|---|---|---|
| 1 | H1 | Probe that judges | 10 | 2 | 1 | 3.33 | W1 |
| 2 | H5 | Data meter | 8 | 1 | 2 | 2.67 | W1 |
| 3 | H10 | Typed levels that cannot be misread | 5 | 1 | 1 | 2.50 | W1 |
| 4 | H2 | Diagnostics report | 7 | 2 | 1 | 2.33 | W1 |
| 5 | H7 | Told when alerts could not be checked | 9 | 2 | 2 | 2.25 | W2 |
| 6 | J1 | Pin to strip | 9 | 2 | 2 | 2.25 | W3 |
| 7 | H6 | Slow down when nothing moves | 8 | 2 | 2 | 2.00 | W2 |
| 8 | H12 | Hardening sweep | 4 | 1 | 1 | 2.00 | W1 |
| 9 | M5 | Contrast test | 4 | 1 | 1 | 2.00 | W4 |
| 10 | L1 | Change over the range | 4 | 1 | 1 | 2.00 | W3 |
| 11 | K1 | Repeat on each crossing | 7 | 2 | 2 | 1.75 | W2 |
| 12 | M1 | The chart by keyboard | 7 | 2 | 2 | 1.75 | W4 |
| 13 | H13 | Bad config kept | 5 | 1 | 2 | 1.67 | W1 |
| 14 | H14 | Wider stills and fit tests | 5 | 2 | 1 | 1.67 | W1 |
| 15 | H4 | A chart that says when it is suspect | 5 | 2 | 1 | 1.67 | W1 |
| 16 | J2 | Undo | 5 | 2 | 1 | 1.67 | W3 |
| 17 | J5 | Alerts at a glance | 5 | 2 | 1 | 1.67 | W2 |
| 18 | M4 | Up and down colours | 5 | 2 | 1 | 1.67 | W4 |
| 19 | L3 | Cache of 48 series | 3 | 1 | 1 | 1.50 | W3 |
| 20 | B11a | Range remembered | 3 | 1 | 1 | 1.50 | W3 |
| 21 | D5 | Order once | 3 | 1 | 1 | 1.50 | W3 |
| 22 | M3 | High contrast | 7 | 3 | 2 | 1.40 | W4 |
| 23 | H3 | Probe dump | 4 | 1 | 2 | 1.33 | W1 |
| 24 | M2 | Typed settings | 4 | 2 | 1 | 1.33 | W4 |
| 25 | H8 | Mute digest | 5 | 2 | 2 | 1.25 | W2 |
| 26 | C10 | Rest while locked | 5 | 2 | 2 | 1.25 | W2 |
| 27 | J6 | Find a card | 5 | 2 | 2 | 1.25 | W3 |
| 28 | K2 | Fast-move alert | 5 | 2 | 2 | 1.25 | W2 |
| 29 | G3 | Panel zoom | 6 | 3 | 3 | 1.00 | W4 |
| 30 | J3 | Rename and decimals | 3 | 2 | 1 | 1.00 | W3 |
| 31 | M6 | Reduced motion | 3 | 1 | 2 | 1.00 | W4 |
| 32 | A13 | Webhook retry and quit wait | 4 | 2 | 3 | 0.80 | W2 |
| 33 | H11 | Local time per instant | 3 | 2 | 2 | 0.75 | W1 |
| 34 | M7 | Screen reader first step | 4 | 2 | 4 | 0.67 | W4 |

## Work packages

Built in this order. W1 goes first because W2 reads its meter and W3 and W4 are judged from its
pictures. Each package must also pass the standing checks: `check`, `test` and `clippy` with
`--all-features --all-targets` under Wine (the one known failure is the Consolas theme test),
`scripts/check-state-linux.sh`, every existing still identical, both exes under 2 MB, `{}` still
parsing with every new field at its 1.8 behaviour, and a test that no report, log or error text
contains a webhook URL.

### W1: Know it is true (both builds; H1, H2, H3, H4, H5, H10, H11, H12, H13, H14)

Nothing in 1.8 was seen on Windows or run against CNBC. This package does not add a feature a user
would list; it makes the ones they have checkable. `--probe` learns to judge its own output against
every assumption the notes name, the same checks become a clipboard report anyone can paste back,
real answers can be saved as fixtures, the chart says when its series is old or oddly stamped, the
menu says how much data the app has used, and the small known faults (the comma in a typed level,
the hour lost across a clock change, NaN in a price, a config that will not parse) are closed.

Acceptance, without a Windows desktop:

- `probe::judge` has one passing and one warning fixture for each named assumption (quote fields,
  unknown symbol, time spelling, bar start or end, daily stamp boundary, volume present, release tag).
  A WARN gives a non-zero exit.
- The report built from a fixture config shows the webhook as its host only; a test searches the
  text for the path and query and finds neither. The settings path carries no user name.
- `--dump` refuses to run without a folder and writes neither the config nor the webhook (temp folder
  test).
- Meter: tests for the hour roll-over, an idle hour and the menu line. Review confirms no new call
  to the HTTP client.
- Typed levels: `4200,5`, `4,20`, `1,2345` and `,500` are refused with a reason; `4,200.50` is read
  as 4200.50; the far-level question triggers at exactly 20 %.
- A 5D fixture folded across an offset change puts the bars on each side at their own local hour.
  The mute label after a change shows the right hour.
- `fmt_price` and `board_text` print `--` for NaN and infinity. `y_of(NaN)` lies inside the plot.
  5D at one-hour candles in the narrowest pane has no candle past the plot's right edge. The update
  request's header test finds no version number and no browser string.
- A config that will not parse: the original bytes survive under another name, defaults load, the
  flag is set.
- New stills exist for 24 cards, 24 symbols on 1, 2 and 3 rows, the narrow pane, 192 DPI and each
  range. The fit test passes for every fixed-pitch face at 96, 120, 144 and 192 DPI.

### W2: Alerts that say when they cannot see (H6, C10, H7, H8, A13, K1, K2, J5)

P1 made alerts settable and silenceable. What is left is the cases where an alert is set and the
user hears nothing for a reason the app knows: the feed was stale, the PC slept, a mute swallowed
it, the POST failed, or the same level fired forty times and was muted for good. The package adds
the gap notice, the mute digest, one retry, a crossing rule and a fast-move rule, and one list of
everything set. It also holds the poll governor (slow when quiet, rest when locked), because the
governor's hard rule is about alerts: never slower than 15 s while one is armed.

Acceptance, without a Windows desktop:

- `governor::interval` is tested over every mix of locked, armed and quiet time. It never exceeds
  15 s while armed, returns to base on the first change, and returns base for every input when both
  settings are off (the Classic default).
- The stale test follows the governed interval: a quote one interval old is not stale, three is.
- In every repeat mode (every cooldown, once, each crossing, fast move) a stale or fallback quote
  fires nothing. One test per mode.
- `watch::step` over a scripted clock: no event under the threshold; a clock jump gives one line
  with both ends; the notice text contains no price.
- Crossing: once per crossing, re-armed only past the band, a first fresh quote already beyond the
  level fires once, NaN never, exactly on the level fires.
- Fast move: an uncovered window or a gap longer than three polls fires nothing; the exact
  threshold fires.
- Digest: none for zero, one request for one or more. Review confirms no timer was added.
- Retry: never on 4xx, once on 5xx or a network error, outcome `sent on retry`.
- Overview: distance text, the cannot-fire warning for a hidden symbol, the log at 50 and 51 entries.

### W3: Twenty-four symbols (J1, J2, J3, J6, D5, L1, L3, B11a)

P6 raised the cap to 24 and let any symbol in. This package is what that user meets next: a strip
too crowded to read, cards hard to find in a panel taller than the screen, a Delete key with no way
back, labels cut at 8 characters, and a chart that forgets its range and leads a five-year view with
today's change. Pin to strip is the core; the rest are small and mostly reuse P5's order functions.

Acceptance, without a Windows desktop:

- With no `strip` field every row is on the strip and the existing strip stills are identical. One
  row is always shown. A click rectangle maps to the right card when some rows are unpinned. A new
  still shows 24 symbols with 6 pinned.
- Undo restores the exact config after a remove, a move, a sort, a pin change and an alert clear.
  It never restores a removed webhook. A second undo does nothing.
- Rename follows P6's label rules; a taken label gets a number; Reset returns the derived label.
- `find` prefers a prefix, ignores case and leaves the lock alone on no match. `scroll_to_show` is
  tested at the top, the bottom and for a card taller than the view.
- `order_by` is stable, puts rows with no fresh quote last, leaves hidden symbols in place, and P5's
  "rows keep catalogue order" test still passes.
- `range_change` returns nothing for a zero or non-finite first open. The 1D header still matches
  its P2 still.
- The cache holds 48 on Ultra, evicts least recently used first, never marks a series off screen as
  due, and 48 full series come to under 1.3 MB by size arithmetic in a test.
- `chart_range` round-trips; an unknown value reads as `1D`.

### W4: Reach (Ultra unless marked; M1, M2, M3, M4, M5, M6, G3, M7)

The panel can be driven by keys since P5, but the chart cannot, three settings still need a config
edit, and nothing in the app has ever been checked for contrast, colour meaning, size or motion.
This package finishes the keyboard, lets Ultra follow a Windows contrast theme and the animation
setting, offers the up and down colours other markets and other eyes need, adds panel zoom, and
gives screen readers one sentence to read. On Classic the two system-following items exist but are
off, so its pixels do not move.

Acceptance, without a Windows desktop:

- `keys::map` is tested for each new key with the chart open and closed, with and without a lock.
  P5's tests pass unchanged. Every action has a help line. A crosshair set by key and one set by
  pointer give the same layout index.
- Quiet hours `22:30-06:45` parses, wraps midnight, and equal ends mean off. Bad entries are refused
  with fixed sentences and change nothing. The hotkey prompt uses P3's grammar as it is.
- The contrast theme is rendered from two fixed system palettes. Every text and background pair in
  it is a pair the system defines. Up and down rows differ by a character, not only by colour.
- The contrast test passes for every Ultra theme, with each tuned still named in the note. Classic's
  ratios are printed in the note and its stills are identical.
- Each up and down palette has a still. `red_green` changes only the two direction colours.
- `frames` returns one step for off, and Classic's default is on.
- Zoom 150 % at 96 DPI is byte for byte the still at 144 DPI and 100 %. Zoom 100 % matches the
  baseline. P2's `index_at(x_of(i)) == i` holds at every zoom step.
- `spoken_line` is tested for up, down, zero, stale and no quote. The window-name call is reviewed
  by code only, and the note says so.

## RAM tally (Ultra, worst case)

| Item | RAM |
|---|---|
| L3: 48 series in the cache | about +0.9 MB |
| J2: one config copy for undo | about +10 KB |
| J5: Recent kept to 50 | about +5 KB |
| G3: back buffer at 150 % | grows with the window, as a 144 DPI screen already does |
| Everything else | under 5 KB together |
| **Total** | **about +1 MB**, so near 12.5 MB of the 23 MB allowed |

The remaining 10 MB or so is left unspent on purpose. J8 (48 symbols, +2.2 MB) and N1 (+0.55 MB) are
the next claims on it, and both wait on proof, not on bytes. Classic gains under 20 KB (H5, J2 and
the governor's state) and no new thread.

## Unsure, because of the reading limit

These could not be checked against the source, and each changes a verdict or a cost if it turns out
the other way.

- Whether the poll interval can already be changed from the menu. If not, it belongs in M2.
- Whether a typed level's comma grouping is already strict (H10).
- Whether an alert can hold more than one level on a side today (K3 assumes not, from P1's example).
- What a config that will not parse does at start (H13 assumes it is replaced by defaults).
- Whether the HTTP library reports bytes as sent or as unpacked (H5 may be an estimate).
- How "stale" is decided, and so how much of it must follow the governed interval (H6, C10).
- Whether quotes come in one request for all symbols (the reason H6 slows the whole board together).
- Whether the session-lock notice needs an import the exe does not link yet (C10).
- Whether the panel body already follows the lock when arrows move it off screen (J6).
- Whether `u.dpi` is set in one place (G3's cost), and whether a DPI change is already handled (M9).
- Whether Classic has a card menu (J1 and J3 fall back to the Symbols menu there).
- Whether every theme's face is fixed-pitch (H14's fit test covers only those that are).
- Whether the CI runner can reach CNBC (if it can, H1 could run at release time).
- The exe size and RAM of 1.8 as built. All exe figures above are estimates, about 60 KB in total,
  and each package must measure against the 2 MB ceiling.
- What unticking a symbol does to its alert levels (J5's warning assumes they are kept and silent).

## Count

34 items BUILD NEXT in 4 packages (5 from the old LATER list, 29 new), 30 LATER, 16 NO.
