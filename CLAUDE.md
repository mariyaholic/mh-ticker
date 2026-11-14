# CLAUDE.md

## Checkpoints for long or delegated work

- Every worker (a subagent, or a session doing multi-step work) runs `scripts/agent/checkpoint "<what is done>" "<what is next>"` after each coherent step, at least every few edits, and before any long-running command. It runs the fast check (`cargo check --locked --workspace --all-targets`), appends an entry to `.progress.md` (ignored by git), and commits everything as `checkpoint: <what is done>`. It never pushes, and a failing check is recorded without blocking the commit.
- Hand-offs between roles use `// TODO(finish): <what> (accept: <how to tell it is done>)` markers, written in the comment syntax of the file's language, plus a `<package>-handoff.md` note.
- When a worker stops early, the orchestrator runs `scripts/agent/where`, then resumes the same worker if it can still be messaged, or starts a fresh one with the last "next" line and the commits `where` shows. The orchestrator does not rebuild the worker's state from the diff.
- Run delegated workers one at a time unless told otherwise. When a usage limit hits, parallel workers all stop at once.
- Squash-merge work branches so checkpoint commits stay out of the main history.
- Files and commit messages name roles as builder, finisher, orchestrator or reviewer. No committed file or commit message names an AI model or vendor.
