# Journal

One section per milestone of `docs/ROADMAP.md`, N0 onwards: what went well, what failed, the
idioms that took more than one attempt, the rules that had to be corrected, and reproducible
numbers. Only what the history shows; `Effort` and `Time` are left for the user.

## Origin

This repository is EmiBSD.LZ's history up to `lz-origin` (`f5985f1d055a`, `docs: M16b met`,
2026-10-08), the faithful port of OpenBSD: milestones M0..M16b, 542 commits, 1860 `Upstream:`
trailers, 1042 ported and 140 wip entries of its `ports.toml`. Their journal is LZ's
`docs/JOURNAL.md` at that commit (`git show lz-origin:docs/JOURNAL.md`). From here on the port
continues in LZ and this journal records the native system.

## N0 Bootstrap

Boundary: _(the `docs: N0 met` commit)_. Range `lz-origin..<hash>`.

- Went well: _(filled at the close)_
- Failed: _(filled at the close)_
- Idioms: _(filled at the close)_
- Rules: the governance rewrite (`CLAUDE.md`, `.claude/rules/`), decisions 16 to 20
  (`docs/PHASE2.md`); the authorship rule of 2026-10-09 (the author's ISC block first in every
  `.rs`), first planned as a milestone N0b, then absorbed from LZ 44edb2c and pushed at once at
  the user's request; the milestone was dropped once it was in.
- LZ sync: `f5985f1d055a..44edb2c8323e`, 2 commits (the logo, the author's block): cherry-pick
  1, cherry-pick-conflicts 1, reimplemented 0.
- Numbers: _(from the tools at the close: `cargo xtask unsafe-report`, `lz status`, the smoke
  count of `just ci`, the `diff-openbsd` summary per arch, `just bench` when it exists)_

Effort: _(user)_

Time: _(user)_
