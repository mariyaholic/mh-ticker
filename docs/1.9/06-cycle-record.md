# The 1.9 cycle record

*How 1.9 was made. The workflow is in [01-workflow.md](01-workflow.md); the package briefs are 02 to 05,
and each package's note is in [notes/](notes/).*

## The workflow

- One writer per package, and one agent at a time. No two packages were in flight.
- The writer builds the code, the tests and the package note. The checker reads the diff, runs the five
  commands, and sends back a numbered list of findings. The same writer fixes them, and the loop runs until
  nothing blocks merging. The checker also fixed a few things itself, noted below.
- A scribe documents each package at the end: CHANGELOG, README, SOURCES and the note.
- Every step ends with a checkpoint commit. If an agent stopped, the checker resumed it from its last
  checkpoint, or from a fresh agent.

## The packages

- **W1, know it is true.** A judging probe (`--probe`, and `--dump` for test fixtures), a diagnostics report,
  a data meter, staleness notes on the chart, local time per instant, typed levels, kept settings files,
  hardening, and wider stills with fit tests. Check rounds: one; nothing was sent back. The checker
  removed a personal name that had got into a test fixture itself.
- **W2, alerts that say when they cannot see.** The poll governor, the not-checked gap, the mute digest, webhook
  retry, each-crossing and fast-move alerts, and the alerts overview. Check rounds: one review round, recorded
  in its note. Two fixes came out of it: the lock and unlock session call now goes through a `windows-sys`
  feature rather than a name looked up at run time, and the alert's reference price uses the same freshness
  limit as the governor and the fire path.
- **W3, twenty-four symbols.** Pin to strip, undo, rename, decimals, ordering once, find, the chart's
  range change and a bigger cache. Check rounds: one; nothing was sent back. The checker added the
  `[[example]]` entries for the two new examples and regenerated the six wide stills whose header changed.
- **W4, reach.** Chart keys, typed settings, the Windows contrast and animation ticks, up and down colours,
  a contrast test over every theme, panel size, and the window name. Check rounds: the history records one
  checker fix commit. The checker added the `[[example]]` entry and reviewed the four tuned Ultra stills,
  which it then took into its baseline. The committed copies were not regenerated (see the W4 note).

## Interruptions

- The W3 writer stopped on a usage limit before its first checkpoint. The checker committed its work and
  resumed the package from there.
- A restart during W2 made no difference to the work.

## Still unverified for 1.9 as a whole

- The live CNBC and GitHub answers: `--probe`, the diagnostics report, the range files and the release check
  have not run against the services.
- A real Windows desktop: the clipboard, the menus, a real clock change, the real fonts, the real lock and
  unlock, sleep and resume, the system colours and their change message, the Windows animation setting,
  key presses, focus, screen readers, and the window name in Alt+Tab. The package notes list their own
  open items.
- Every test and still was run under Wine, not on Windows.
