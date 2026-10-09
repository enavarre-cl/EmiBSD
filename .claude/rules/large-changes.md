# Large changes and agent context

Decided by the user on 2026-10-03 (as `large-ports.md`, after the first pf agent ran out of
context with hours of uncommitted work) and kept for the redesign. Applies to every agent (main
session or subagent) that redesigns or syncs code, and to every prompt that launches one.

- **One big module, one subagent.** A module (or the set of LZ files it derives from) of more
  than about 3,000 lines is redesigned by a subagent of its own. The coordinating agent only
  splits the work, integrates the results, runs `just ci` and commits.
- **Commit early.** Each piece that compiles is committed at once on the worktree branch.
  Never hold hours of uncommitted work. Intermediate commits must build; the last one of a
  series must be `just ci` green, with `lineage.toml` and the trailers right.
- **Handoff note.** The coordinator keeps an up-to-date note in the scratchpad (one file per
  task, e.g. `<scratchpad>/<task>/HANDOFF.md`): what is done (with commit hashes), what is
  left, the pending diffs and where they are, the `lineage.toml` entries and the numbers for
  the trailers. A fresh agent must be able to resume from it without rereading everything.
- **Read cheaply.** `grep`, `sed -n '<a>,<b>p'` by ranges, the `<CODE>` zone only
  (`sed -n '/<CODE>/,/<\/CODE>/p'`), and long command output redirected to scratchpad files and
  grepped. A module being redesigned is still read completely, but in ranges, and by the
  subagent that redesigns it.
- **Stop cleanly.** An agent that feels its context running low stops in a committed state,
  writes the handoff note, and only then hands back, saying where the note is.
- **Review by another.** A security-sensitive change (`security-review.md`) is reviewed by an
  agent that did not write it, when one is available; the review goes in the trailer.
