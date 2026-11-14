# The 1.9 workflow

*Prepared 2026-11-14, after 1.8.0. The work is the four packages ranked in
[../1.8/05-next-features.md](../1.8/05-next-features.md) (W1 to W4, in that order). This file is how
they get built; the packages themselves, with their acceptance lists, stay where they are.*

## What changed from 1.8, and why

1.8 ran a builder, then a finisher, then a review, per package, three packages at once. Usage limits
cut the parallel agents off twice, and an agent that stopped without committing left the next one to
rediscover its work. So 1.9 runs:

- **One agent at a time.** Never two packages in flight.
- **Writer, then checker, in a loop.** One writer builds the whole package (no separate finisher).
  The orchestrator checks it and sends back a numbered list of findings; the same writer fixes them
  and reports again. The loop ends when the checker has nothing left that blocks merging.
- **A scribe at the end** of each package documents it (CHANGELOG, README, SOURCES, the package note)
  in its own short pass, so the writer spends nothing on prose for users.
- **Checkpoints everywhere.** Every agent runs `checkpoint "<done>" "<next>"` after each step and
  midway through any long file. If an agent stops, the orchestrator runs `where` and resumes it, or a
  fresh one, from its last "next" line.

## The loop, per package

1. **Prepare.** The orchestrator makes a worktree on `wip/<package>` at the integration head and
   writes the writer's brief from the package's section in `05-next-features.md`: the acceptance list
   word for word, the files it owns, the budget, the standing rules below, and anything learned from
   the previous package.
2. **Write.** The writer builds code, tests and `docs/1.9/notes/<package>.md` (what was built,
   decisions, numbers measured, what is unverified). It finishes with check, test, clippy, the Linux
   harness and the stills clean, and a report of under 250 words.
3. **Check.** The orchestrator reads the diff, not the whole files: every acceptance line has a test
   or a stated reason; every new state has a reset path and a row in `STATE-MACHINES.md`; nothing
   blocks the UI thread; nothing nests `with_ui`; Classic's stills are identical; nothing secret is
   printed. It runs the five commands itself. Findings go back as a numbered list with file:line and
   the expected behaviour.
4. **Loop** steps 2 and 3 until nothing blocking remains. Non-blocking findings are written into the
   package note as open, not silently dropped.
5. **Document.** The scribe updates CHANGELOG (an unreleased 1.9 section), README, SOURCES and the
   package note from the writer's note and the diff. The orchestrator checks it for claims nobody
   verified.
6. **Merge.** Squash-merge to the integration branch, push, read CI. A red CI is fixed at its root
   cause before the next package starts.

## Standing rules for every agent

- The budget: Ultra up to about 23 MB private RAM, Classic about 11.5 MB and unchanged pixels, one exe
  under 2 MB, idle CPU near zero, no new dependency, no new steady traffic beyond the user's choices,
  any other request only on an explicit click to a host the app already names.
- `with_ui` does not nest. No menu, box, dialog or I/O inside a `with_ui` closure.
- Every new state has an explicit reset path and goes into `STATE-MACHINES.md`.
- Every bug fix comes with a test that was seen failing first.
- Never skip, weaken or delete a test to get green. One Wine-only failure is known
  (`ui::tests::a_theme_switch_takes_the_new_themes_face`); everything else passes.
- A webhook URL is never displayed, logged or echoed.
- No AI model or vendor names in files or commit messages; roles are writer, checker (orchestrator)
  and scribe.
- Forbidden to the writer: `README.md`, `CHANGELOG.md`, `SOURCES.md` (the scribe's), `.github/`,
  `Cargo.toml`, `Cargo.lock`, `shots/` unless the package's acceptance asks for new stills.

## Commands

```bash
/home/user/mh-wt/wcargo check --all-features --all-targets
/home/user/mh-wt/wcargo test --all-features --no-fail-fast
/home/user/mh-wt/wcargo clippy --all-features --all-targets
bash scripts/check-state-linux.sh
/home/user/mh-wt/stills-check
```

`/home/user/mh-wt/` holds the cross-compile wrapper, the stills baseline, `checkpoint` and `where`.
They live outside the repository; a fresh environment has to recreate them (the wrapper targets
`x86_64-pc-windows-gnu` and runs tests with `xvfb-run -a wine`).

## Order and what each package hands the next

| Order | Package | Hands on |
|---|---|---|
| 1 | W1 Know it is true | the judging probe, the report, the data meter, wider stills and fit tests, which W2 to W4 are checked against |
| 2 | W2 Alerts that say when they cannot see | the poll governor and the alert overview |
| 3 | W3 Twenty-four symbols | pin to strip, undo, find, the 48-series cache |
| 4 | W4 Reach | chart keys, contrast, up and down colours, zoom, reduced motion, a spoken line |

W1 goes first because its pictures and its probe are what the later packages are judged by.
