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
- `docs/STATUS.md` stays under 30 lines: current milestone, last three things done, next three,
  blockers. It is imported into every session; keep it current and small.
- `docs/ROADMAP.md`: every milestone has a mechanical exit criterion (a command that passes or a
  serial line that appears). Editing a milestone keeps that property.
- `docs/C_TO_RUST.md`: one row per idiom, columns C | Rust | Why. Add a row when an idiom is
  settled, not before.
- `docs/ARCHITECTURE.md` records deviations from OpenBSD and the reason for each dependency.
- `reference/PINNED.md` is tiny and machine-read (`Commit:` line); do not add prose there.
- Refer to OpenBSD manuals as `name(section)`: `tsleep(9)`, `pledge(2)`, `com(4)`.
- Cite C as `reference/openbsd-src/sys/<path>:<line>`; cite Rust as `sys/<path>:<line>`.
