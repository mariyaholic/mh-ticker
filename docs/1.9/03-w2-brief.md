# W2 brief: Alerts that say when they cannot see (Ultra, the governor both builds)

*The writer's brief for the second 1.9 package, prepared by the orchestrator from
[../1.8/05-next-features.md](../1.8/05-next-features.md) items H6, C10, H7, H8, A13, K1, K2 and J5
(rows in sections 1, H, J and K) and package W2. The workflow is [01-workflow.md](01-workflow.md).*

## Why

P1 made alerts settable and silenceable. What is left is the cases where an alert is set and the user
hears nothing for a reason the app knows: the feed was stale, the PC slept, a mute swallowed it, the
POST failed, or the same level fired forty times. W2 also holds the poll governor (slow when quiet,
rest when locked), because its hard rule is about alerts: never slower than 15 s while one is armed.

## What to build

| Item | What | Pure part | Wiring |
|---|---|---|---|
| H6 + C10 | The poll governor. Quiet: no shown quote changed for 10 min, poll every 15 s; for 30 min, every 60 s. Locked session and nothing armed: one poll per 5 min. Never slower than 15 s while an alert is armed. Any change, expand, hotkey, unlock or Refresh now returns to base at once (P3's `kick`). Settings `slow_when_quiet` and `rest_when_locked`: on for Ultra, off for Classic (Classic polls exactly as today) | `governor::interval(base, quiet_for, locked, armed)`; the stale threshold follows the governed interval | the feed's existing wait; one session-notification registration for lock and unlock; two menu ticks |
| H7 | Told when alerts could not be checked: a Recent line `02:10 to 02:31 not checked (no fresh price)`, a tooltip line while it lasts, and an optional webhook notice once per gap (default off, never a price in it). The threshold is the larger of 5 min and ten polls; sleep shows as a clock jump | `watch::step(now, fresh, armed)` giving `Blind` and `Back` | the feed tick; no timer |
| H8 | When a mute or quiet hours end: one balloon `3 alerts while muted. See Alerts > Recent.` | `digest(log, since)` | the feed tick; no timer |
| A13 | The webhook tries once more after a network error or a 5xx (never a 4xx), outcome `sent on retry`; quitting waits up to 2 s for a POST in flight | `retry::next(outcome, attempt)` | the existing alert thread; the quit path |
| K1 | Repeat mode "each crossing": fires on a crossing, re-arms only after the price goes back past the level by a small band. Beside "every cooldown" (default, as today) and "once". The first fresh quote after start fires once if already beyond | in `alerts.rs` | the Alerts menu radio |
| K2 | Fast-move alert: `1 % in 5 min`, `2 % in 15 min`, `3 % in 60 min`, from the hour of samples already held. Needs the window covered by fresh samples with no gap longer than three polls | in `alerts.rs` | the Alerts menu |
| J5 | Alerts at a glance: one list of everything set with its distance (`XAU above 4,200.00, 1.2 % away`), a warning on levels whose symbol is not on the strip, Clear all (asks first), a dot on cards with a level, Recent kept to 50 with Copy | `overview(config, quotes)` | the Alerts menu; the card painter (Ultra only) |

## Acceptance (the checker runs each), word for word from 05, W2

- `governor::interval` is tested over every mix of locked, armed and quiet time. It never exceeds 15 s
  while armed, returns to base on the first change, and returns base for every input when both
  settings are off (the Classic default).
- The stale test follows the governed interval: a quote one interval old is not stale, three is.
- In every repeat mode (every cooldown, once, each crossing, fast move) a stale or fallback quote fires
  nothing. One test per mode.
- `watch::step` over a scripted clock: no event under the threshold; a clock jump gives one line with
  both ends; the notice text contains no price.
- Crossing: once per crossing, re-armed only past the band, a first fresh quote already beyond the
  level fires once, NaN never, exactly on the level fires.
- Fast move: an uncovered window or a gap longer than three polls fires nothing; the exact threshold
  fires.
- Digest: none for zero, one request for one or more. Review confirms no timer was added.
- Retry: never on 4xx, once on 5xx or a network error, outcome `sent on retry`.
- Overview: distance text, the cannot-fire warning for a hidden symbol, the log at 50 and 51 entries.

Plus the standing checks in [01-workflow.md](01-workflow.md): Classic's 36 stills identical and its
polling unchanged by default, both exes under 2 MB, `{}` still parses with every new field at its 1.8
behaviour.

## Decisions taken (orchestrator, with the owner's autonomy for 1.9)

- The governor is on for Ultra and off for Classic by default. Classic's behaviour must be exactly
  1.8's unless the user ticks the setting.
- The gap notice to the webhook is off by default and never carries a price.
- J5's card dot is Ultra only and must not move any of the 36 stills (they show no alerts).

## Lessons carried from W1

- The data meter (`source::meter_line`) is how the governor's saving is shown; the notes should quote
  a before and after from a scripted feed if one can be made, and say so if not.
- Session lock and unlock arrive as window messages: they go through `with_ui` for state only, and the
  kick happens outside it.
- A mode added to an alert is new saved state: `{}` and every 1.8 settings file must still load with
  the 1.8 behaviour.
