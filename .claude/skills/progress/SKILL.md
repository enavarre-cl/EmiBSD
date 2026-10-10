---
name: progress
description: Redesign progress of EmiBSD per ROADMAP milestone (N0..), measured live - modules redesigned, adapted and inherited, LZ lines done and left, Rust lines, and a time estimate from the project's own rate. Use when the user asks how far the redesign is, how much is left, how long a milestone will take, or wants status numbers.
argument-hint: [milestone]
allowed-tools: Bash(python3 -I ${CLAUDE_SKILL_DIR}/scripts/progress.py*)
---

The table below was computed just now from `lineage.toml`, `docs/ROADMAP.md`, the LZ clone at
the pin and git by `${CLAUDE_SKILL_DIR}/scripts/progress.py` (its docstring defines every
column).

```!
python3 -I ${CLAUDE_SKILL_DIR}/scripts/progress.py $ARGUMENTS
```

How to answer:

- Show the table as it is; never retype or round its numbers. Reply in the user's language,
  keeping the table's English headers (artifacts stay in English).
- Under it, three to six lines: the last milestone met and the one under way, what is left and
  where it concentrates, and the caveats that matter for the question asked: an `adapted`
  module only followed a redesign and is shown apart; a row whose scope is prose (N8) resolves
  no modules; a met row may keep inherited modules that a later row's prose claims (N1's
  crypto framework is N8's); the estimate is a linear extrapolation of the project's own
  average and promises nothing.
- With a milestone argument (`/progress N2`) the module list follows the table: say what is
  left in it and which modules are the biggest.
- If the table is missing or starts with `progress: error`, run the script with Bash and
  report the error. Never estimate by hand.
- These numbers are for the conversation. The docs (README, STATUS, JOURNAL) take theirs from
  `cargo xtask lz status`, `cargo xtask unsafe-report` and the git commands `docs/JOURNAL.md`
  names.
