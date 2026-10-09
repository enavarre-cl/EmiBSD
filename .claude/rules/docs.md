---
paths:
  - "docs/**"
  - "*.md"
  - "reference/*.md"
---

# Documentation

- English. Short sentences. Decisions come with a one-line reason.
- Each document has one job; never duplicate content between them:
  behaviour Claude must follow → `.claude/rules/`; knowledge and rationale → `docs/`;
  the always-in-context summary → `CLAUDE.md` (about 120 lines, links instead of copies).
  `docs/PHASE2.md` is the process (how a change is made, measured, reviewed and synced, the
  decisions log, the origin); `docs/ARCHITECTURE.md` the design; `docs/IDIOMS.md` the native
  idiom table; `docs/C_TO_RUST.md` LZ's idiom table, frozen; `docs/ROADMAP.md` the native
  milestones N0..; `docs/SYNC.md` the sync runbook; `docs/JOURNAL.md` one `N` section per
  milestone.
- `docs/STATUS.md` stays under 30 lines: current milestone, last three things done, next three,
  blockers. It is imported into every session; keep it current and small.
- `README.md`'s `Status:` line names the last milestone met and the one under way. It is
  updated in the same commit that marks a milestone (or sub-milestone) met in `docs/ROADMAP.md`
  and `docs/STATUS.md` (the user's rule of 2026-10-04, after it lagged at M5 while M9 closed).
- `README.md`'s sections "Status" (the milestone table), "What works today" (the smoke list and
  the serial excerpt) and "Lineage and safety" (the `cargo xtask lz status --write` tables and
  the unsafe totals against the baseline) are updated in
  that same commit, whenever a milestone or sub-milestone closes (the user's rule of
  2026-10-04). Real data only: numbers from the tool, serial lines from a smoke log.
- `docs/JOURNAL.md` gains the milestone's section (went well, failed, idioms that took several
  attempts, rules corrected, numbers from git with the commands; `Effort`/`Time` left for the
  user) in the same commit that marks a milestone or sub-milestone met (the user's rule of
  2026-10-05, M12+).
- `docs/ROADMAP.md`: every milestone has a mechanical exit criterion (a command that passes or a
  serial line that appears). Editing a milestone keeps that property.
- In a Markdown table, write a `|` inside a code span as `\|` (`\|d\| ...`, `\|=`): GitHub splits
  cells at every bare pipe, even between backticks, and cuts the row.
- `docs/IDIOMS.md`: one row per native idiom, columns LZ shape | native shape | why. Add a row
  when an idiom is settled, in the commit that first uses it. `docs/C_TO_RUST.md` is not edited.
- `docs/ARCHITECTURE.md` records the design and the reason for each dependency; per-module
  changes against LZ are in `lineage.toml` and the module's `## Redesign`; a design decision
  that spans modules is a section here.
- `reference/PINNED.md` and `lz/PINNED.md` are tiny and machine-read (`Commit:` line); do not
  add prose there.
- Refer to OpenBSD manuals as `name(section)`: `tsleep(9)`, `pledge(2)`, `com(4)`.
- Cite C as `reference/openbsd-src/sys/<path>:<line>`; LZ code as
  `reference/emibsd-lz/sys/<path>:<line>`; native Rust as `sys/<path>:<line>`.
