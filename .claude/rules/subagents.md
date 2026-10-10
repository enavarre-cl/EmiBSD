# Subagents: the common contract

The project's subagents are defined in `.claude/agents/*.md` (the set-up EmiBSD.LZ adopted on
2026-10-09, adapted here on 2026-10-10; it replaces the ad-hoc `general-purpose` prompts of
N1). Every definition there points here; this file is the part they share. The rules it
summarises stay authoritative in their own files (`re-engineering.md`, `large-changes.md`,
`lineage.md`, `testing.md`, `git-commits.md`, `security-review.md`, `scope-and-stubs.md`,
`unsafe-budget.md`, `lz-sync.md`); when they change, this file follows.

## Roles

| Agent | Model | Worktree | Memory | Writes | Use it for |
|---|---|---|---|---|---|
| `milestone-coordinator` | opus | yes | yes | branch | an `N` milestone: measure, split, launch, integrate, close CI and docs |
| `redesigner` | opus | yes | yes | branch | a delicate redesign: locking, `unsafe`, data structures, anything in `security-review.md`'s list, any module over 3,000 lines (one each) |
| `mechanical` | sonnet | yes | yes | branch | tests added to an existing module, `lineage.toml` bookkeeping, the same call-site change over many files, doc updates once decided |
| `integrator` | opus | yes | yes | branch | merge agent branches, resolve the shared files, finish a stopped agent's work, `just ci` |
| `debugger` | opus | yes | yes | branch | root cause of a failing or flaky smoke, test or `diff-openbsd` scenario, fixed without weakening an expectation |
| `reviewer` | opus | no | no | nothing | read-only review of a branch: behaviour kept, `unsafe`, rules, security; returns the `Security-Review:` text |
| `openbsd-probe` | sonnet | no | no | nothing | boot real OpenBSD 8.0 on a QEMU setup and report what it does |
| `image-worker` | sonnet | no | no | scratchpad | view, crop, resize images so the coordinator's context stays small |

Mechanical vs delicate (the user's split, 2026-10-03): when unsure, it is delicate (opus).
At most 4 agents run at once; a milestone coordinator runs at most 2 of its own. There is no
`external-bugs` role here: a slip found in OpenBSD's C, in QEMU, in EDK2 or in LZ goes to the
user in the final report, with the evidence; no agent reports anything upstream or to LZ (LZ
is never told this repository exists, `reference-readonly.md`).

## Memory (`memory: project`)

An agent with memory keeps `.claude/agent-memory/<agent>/MEMORY.md`: lessons that outlive one
run (an idiom that took two attempts, a known flake, a conflict pattern), one line each, in
English, appended at the end. It is committed with the tree, so a worktree agent's lines travel
on its branch and the `integrator` merges them (both sides kept, duplicates dropped). Task
state (branches, hashes, what is left) goes in `HANDOFF.md`, never in memory. The `reviewer`
has none on purpose: it reads every change with fresh eyes. `agent-memory-local/` is not used.

## The machine lock

`/tmp/emibsd/ci.lock` is shared with EmiBSD.LZ on purpose: both repositories run QEMU on the
same Mac, and the Mac is the bottleneck (the rule of 2026-10-07: no competing CI). Two
repositories sharing one lock never run two `ci`s at once. A launching session may name a lock
of its own in its scratchpad instead. There is no batch workflow: the main session (or a
`milestone-coordinator` it launches) drives the redesign, launches the roles one by one with
the Agent tool and sees every plan and every commit (the user's decision of 2026-10-10).

## What the launching prompt must give

The definitions are generic; the prompt of each launch supplies the specifics:

1. the task and its scope (`lineage.toml` modules, the ROADMAP row, the LZ files);
2. the branch to merge first (the coordinator's), or the base commit;
3. the scratchpad directory for `HANDOFF.md` and logs;
4. the machine lock path (`/tmp/emibsd/ci.lock`, or `ci.lock` in the launching session's
   scratchpad), or "no lock";
5. what else runs on the machine at the same time (in this repository and in LZ);
6. any authorisation the user gave for this launch, quoted.

A prompt that misses one of these gets a question back, not a guess.

## Setup in a worktree

- The main checkout is the first line of `git worktree list --porcelain`:
  `MAIN=$(git worktree list --porcelain | sed -n '1s/^worktree //p')`. The gitignored reference
  trees live only there: `ln -s "$MAIN/reference/openbsd-src" reference/openbsd-src` and
  `ln -s "$MAIN/reference/emibsd-lz" reference/emibsd-lz` if missing (`lz/PINNED.md` is tracked
  and needs nothing). Both are read-only (`reference-readonly.md`). Never commit them, never
  `git add -A` / `git add .`; add paths by name. Unlink both before the worktree is removed.
- `PATH=/opt/homebrew/opt/rustup/bin:$PATH` before `cargo`/`just`. Install nothing.
- `diff-openbsd` needs the snapshot: `cp -Rc "$MAIN/target/openbsd" target/openbsd`. Never
  re-download it, never delete the main checkout's copy.
- The smokes need the userland (`target/userland/<arch>/ramdisk.ffs`): `cp -Rc
  "$MAIN/target/userland" target/userland` (an APFS clone, seconds), then `just userland`,
  which rebuilds only what its stamps say changed. Once, before any smoke.
- Read CLAUDE.md and every file of `.claude/rules/` before writing anything.

## While working

- Author block: every `.rs` under `sys/` and `tools/` opens `/* <LICENSES> */` with the
  author's ISC block (`AUTHOR_BLOCK` in `tools/xtask/src/layout.rs`), then one blank line and
  the original notice(s) of every LZ source, whole (`scope-and-stubs.md`, Authorship).
- Commit early: each piece that builds and passes clippy/fmt is committed at once. Trailers
  (`git-commits.md`): `LZ: <lz path>@<12-hex>` per LZ source file, `Unsafe: <subsystem>
  <before> -> <after>` per subsystem whose count moved, `Security-Review:` in the areas of
  `security-review.md`, `Bench:` when measured, then the session's `Co-Authored-By:` line.
  Never push, never amend a published commit, never force anything, never touch `main`. Never
  mix a redesign with an `lz-sync:` or a `lineage:` commit.
- Machine lock: while the lock directory exists and is not yours, run no smokes, no
  `just test`, no full `just build`/`just clippy`, no `ci`; reading, editing and a single-target
  check are fine. To take it: `mkdir -p` its parent once, then `mkdir <lock>` (it fails while someone
  holds it), your name in `<lock>/owner`, `rmdir`-remove it when done. Only coordinators and integrators run `just ci` / `just smoke` (all); redesigners run
  single smokes, one QEMU at a time.
- Long runs (`large-changes.md`): every background run of minutes has a watcher that warns when
  its log has not changed for ten minutes (`stat -f %m <log>`); a progress report gives the
  log's last change time; before the final report every background task started is waited for
  or stopped, and `ps` shows none left (the report names any that was killed).
- Read cheaply: `grep`, `sed -n 'a,bp'`, long output redirected to scratchpad files. An LZ
  module being redesigned, and the C behind it, are still read completely, in ranges.
- Behaviour: OpenBSD's behaviour is the specification and what userland sees never changes
  (the syscall ABI, sysctl MIBs, ioctls, device majors, every `#[repr(C)]` shared with
  userland, every line a smoke or `diff-openbsd` compares). No behavioural deviation to make
  something pass; an inherited `unported!` path is closed or kept visible, never widened.

## Stopping

- A "STOP ... wait for the user" refusal or a permission denial: stop at once, commit nothing
  more, report it. Never retry it or route around it.
- Anything that is the user's decision: a dependency, a download, an install, anything outside
  the repository, a raise of a line in `unsafe-budget.toml`, a bump of `lz/PINNED.md` or of the
  toolchain, dropping an LZ file, moving a whole subsystem, a scope change: stop and hand back
  with the question.
- This repository's own stop rules: a change userland could see (the ABI edge); an LZ security
  fix skipped without a `covered-by-redesign` reason that names the test or the type; a new
  algorithm or dependency in `sys/crypto/`; a check the C makes relaxed instead of made a type;
  more `unsafe` in a module than LZ had, without a reason: stop and ask.
- Low on context: stop in a committed state, update `HANDOFF.md` (done with hashes, left,
  pending diffs, the `lineage.toml` rows and the numbers for the trailers), then hand back
  naming its path.

## Final report

Branch and tip, commits in order, the evidence (`cargo xtask unsafe-report` before and after
per subsystem, test counts before and after, serial lines, `rc=`, wall times, the
`diff-openbsd` result), the `lineage.toml` rows changed, deviations closed or kept, what is
left, the `HANDOFF.md` path, and the background tasks stopped. Then `rm -rf target/smoke` in
the worktree.
