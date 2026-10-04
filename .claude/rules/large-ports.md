# Large ports and agent context

Decided by the user on 2026-10-03, after the first pf agent ran out of context with hours of
uncommitted work. Applies to every agent (main session or subagent) that ports or integrates
code, and to every prompt that launches one.

- **One big file, one subagent.** Every `.c` file of more than about 3,000 lines (`pf.c`,
  `tcp_input.c`, `if_pfsync.c`, ...) is ported by a subagent of its own. The coordinating
  agent only splits the work, integrates the results, runs `just ci` and commits.
- **Commit early.** Each piece that compiles is committed at once on the worktree branch.
  Never hold hours of uncommitted work. Intermediate commits must build; the last one of a
  series must be `just ci` green.
- **Handoff note.** The coordinator keeps an up-to-date note in the scratchpad (one file per
  task, e.g. `<scratchpad>/<task>/HANDOFF.md`): what is done (with commit hashes), what is
  left, the pending diffs and where they are, and the notes and `upstream_blob`s for
  `ports.toml`. A fresh agent must be able to resume from it without rereading everything.
- **Read cheaply.** `grep`, `sed -n '<a>,<b>p'` by ranges, and long command output redirected
  to scratchpad files and grepped. Never load a file of thousands of lines in one read. A file
  being ported is still read completely (`porting-workflow.md`), but in ranges, and by the
  subagent that ports it.
- **Stop cleanly.** An agent that feels its context running low stops in a committed state,
  writes the handoff note, and only then hands back, saying where the note is.
