# Feature discovery prompt

*Written 2026-11-13 against `main` at 1.7.1 (`6f6ee3c`). This is the prompt the orchestrating model
wrote for itself before the 1.8 cycle. It is kept because it is reusable: run it again against any
later version and it should produce the next list. The answer it produced is
[02-feature-list.md](02-feature-list.md).*

---

You are reviewing MH Ticker to find every addition that would make it more valuable or more pleasant
to use **without turning it into a different product**. It is a Windows taskbar price strip that opens
into a panel: one exe, public CNBC quotes, no login, no server. Two builds link one engine: Classic
(one look, lean) and Ultra (themes, picker, alerts, candles).

## Read first, in this order

1. `README.md`: what users are told. Every "config edit", "restart", "for now" and "only from the
   tray" in it is a finding.
2. `CHANGELOG.md`, newest three releases: what the owner asked for most recently is the best evidence
   of what the owner values (here: fewer surprises on click, a way back out of every state, fluid
   motion, settings where the user already is).
3. `FEATURES.md`: the budget, the data already paid for and discarded, and "what I would not do".
4. `STATE-MACHINES.md` and `docs/STATE-AUDIT-*.md`: open risks. A feature that lands on an open risk
   has to close it or stay away from it.
5. `ARCHITECTURE.md`: where code is supposed to go. `ui.rs` is the god module; do not feed it.
6. The code a user touches: `ui::menu_at`, `ui::wndproc`, `ui::on_feed`, `panel::draw`,
   `panel::candles`, `config::Config`, `alerts::check`, `source::Feed`.

## What to look for

1. **Friction.** Anything that needs a config edit, a restart, the tray, or a workaround.
2. **Dead ends.** A state the user can enter and not leave, an action with no visible result, a
   failure nobody is told about.
3. **Data paid for and thrown away.** Fields in responses already downloaded; bars already in memory.
4. **Promises not kept yet.** Everything documented as "not shipped", "for now" or "open".
5. **Table stakes for the category.** What every taskbar ticker or watchlist does and this one does
   not: start with the system, a hotkey, alerts you can set and silence, more than one chart range,
   a keyboard, a way to learn there is a new version.
6. **Power-user affordances.** Copy, reorder, add a symbol, exact levels.
7. **Trust.** Can the user tell when a number is old, when a save failed, when an alert was missed?

## For every candidate, write down

- **What the user gets**, in one sentence, and which build gets it (classic / ultra / both).
- **Evidence** that the need is real: a file and line, or a quote from a document.
- **Cost**: bytes on the wire, bytes held, binary size, a new dependency or Win32 feature, a new
  thread or timer.
- **State it adds**: which machine, which transitions, and the way back out of each new state.
- **What is pure**: the part that can be unit-tested with no window (and run on Linux by
  `scripts/check-state-linux.sh`).
- **What cannot be verified off Windows.** Say it; do not hide it.
- **Verdict**: BUILD NOW, LATER or NO, with the reason.

## Hard constraints (a candidate that breaks one is a NO)

- One exe. No server, no web view, no installer, no login, no history files on disk, no telemetry.
- Budget: private RAM up to **twice the 1.7.1 figure** (about 11.5 MB), under 2 MB binary, idle CPU
  near zero. **No new steady traffic** beyond what the user's own choices cost (more symbols, a
  faster poll); any other request happens only on an explicit click, to a host the README names.
  *(The RAM ceiling was about 6 MB when this prompt was first run. The owner doubled it on
  2026-11-13, mid-cycle; [02-feature-list.md](02-feature-list.md) records what that changed.)*
- **Classic must not drift.** Same pixels (`shots/*.png` byte-identical) and the same click
  behaviour. An engine feature that reaches Classic defaults to exactly what Classic does today.
- Never assume a response field exists. Never alert on stale or fallback data. A webhook URL is a
  secret and is never displayed, logged or echoed in an error.
- Every setting the menu can change survives a restart; every new state has a way back; painter and
  hit test read the same geometry.
- No dependency that is not already in `Cargo.lock`.

## Then

1. Rank BUILD NOW by value over (cost + risk).
2. Group it into work packages that can be built in parallel. A package owns its new module(s)
   outright and touches the shared files (`ui.rs`, `config.rs`, `panel.rs`, `lib.rs`) only at named,
   distinct anchors, so three packages merge without anyone resolving a conflict by guessing.
3. Order packages into waves by dependency.
4. Give each package acceptance criteria a reviewer can check without a Windows desktop, and list
   what stays unverified until someone runs it on Windows.
5. Keep the LATER and NO lists, with reasons. A list that only contains what was built is not a
   survey, it is a changelog.
