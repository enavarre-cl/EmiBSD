# The native process

EmiBSD is an operating system in Rust, derived from EmiBSD.LZ, the faithful file-by-file port of
the OpenBSD kernel. LZ ports; EmiBSD redesigns. This document is the process: where the code
comes from, what never changes, how a change is made, measured and reviewed, how LZ's later
work is absorbed, and the decisions that shaped all of it. The design itself is in
`docs/ARCHITECTURE.md`; the rules Claude follows are in `.claude/rules/`.

## Origin

EmiBSD started as the port itself. On 2026-10-09 the port's repository became
[EmiBSD.LZ](https://github.com/enavarre-cl/EmiBSD.LZ), with its whole history (tag `lz-origin`,
commit `f5985f1d055a`, `docs: M16b met`), and this repository, which keeps that history up to that
commit, became the native system from there on (milestone N0). `lz/PINNED.md` records the LZ commit EmiBSD is synced to;
`lineage.toml` records what every module derives from; `docs/JOURNAL.md` keeps the port's
milestones M0..M14 as the first part of this system's history. LZ never refers to this
repository: the dependency points one way.

## What never changes

- The system-call ABI and the behaviour OpenBSD's userland sees. The ABI equals LZ's at the
  pin; additions arrive only through `lz-sync:` commits.
- The same userland runs on LZ and on EmiBSD, and `just diff-openbsd` is the oracle: EmiBSD
  against LZ against OpenBSD.
- Every smoke stays green (`just ci`, `just ci-full`).
- Licences: every file keeps the whole licence blocks of every LZ file it derives from (the RCS
  ident lines are not licence text and were dropped at N0), with the author's ISC block before
  them (the authorship rule of 2026-10-09); new code is ISC; code from outside OpenBSD's tree is the user's decision.
- The `machine` contract and the two architectures.

## Goals, in priority order

1. **Safety.** Fewer and smaller `unsafe` blocks, safe abstractions over the ones that remain,
   and a soundness argument wherever `unsafe` stays (`.claude/rules/unsafe-budget.md`).
2. **Maintainability.** Idiomatic Rust where the LZ shape exists only to mirror C:
   `Cell`-everywhere structs, raw-pointer links, C-shaped out parameters (`docs/IDIOMS.md`).
3. **Performance.** Measured improvements only (`just bench`), never at the cost of 1 or 2.

These are the criteria of DARPA's TRACTOR program (functional correctness, safety guarantees,
competitive performance, long-term maintainability); LZ covers correctness, this system the rest.

## How a change is made

`.claude/rules/re-engineering.md`, in one line: pick one module of one subsystem, measure it,
read the LZ module and the C it ported, redesign with ownership first, prove it (soundness
arguments, tests, smokes, `diff-openbsd`), record it (`lineage.toml`, `//! LZ:`, `## Redesign`,
`docs/IDIOMS.md`), commit one step with the numbers (`.claude/rules/git-commits.md`).

Layout (decided 2026-10-07): the subsystem directories under `sys/` are fixed, as the unit for
metrics and lineage; inside a subsystem the module tree is free; a whole subsystem moves only
when it is redesigned, in a `lineage:` commit. A module is `inherited` (byte for byte LZ's, modulo
the RCS ident lines), `adapted` (only its call sites changed, because a module it uses was
redesigned) or `redesigned`; a redesign of an item used by more than 50 modules waits for the
LZ milestone that touches them to close (`.claude/rules/re-engineering.md`).

## Who redesigns: the agents

Since 2026-10-10 the redesign is done by Claude Code subagents with fixed roles, defined in
`.claude/agents/` and sharing one contract, `.claude/rules/subagents.md` (setup in a worktree,
the machine lock, the long-run watcher, when to stop, what to report). The set-up is EmiBSD.LZ's
(its commit `b0901dd3`, where 196 hand-written launch prompts had carried the same rules),
adapted: the porter became the redesigner, `ports.toml` became `lineage.toml`, the trailers are
this repository's, and there is no bug-log role, since LZ is never told this repository exists.

| Agent | Model | Does |
|---|---|---|
| `milestone-coordinator` | opus | an `N` milestone: measure the baseline, split, launch, integrate, close CI and docs |
| `redesigner` | opus | a delicate redesign: locking, `unsafe`, data structures, the areas of `security-review.md`, any module over 3,000 lines |
| `mechanical` | sonnet | tests added to a module, `lineage.toml` bookkeeping, the same call-site change over many files, docs once decided |
| `integrator` | opus | merge agent branches, resolve the shared files (`lineage.toml`, the budget, the justfile), `just ci` |
| `debugger` | opus | the root cause of a failing or flaky smoke, test or `diff-openbsd` scenario, fixed without weakening an expectation |
| `reviewer` | opus | a read-only review of a branch: behaviour kept, `unsafe`, rules, security; it writes the `Security-Review:` text |
| `openbsd-probe` | sonnet | boot the real OpenBSD 8.0 on a QEMU setup and report what it does |
| `image-worker` | sonnet | view, crop, resize images, so the coordinator's context stays small |

Five of them keep a memory, `.claude/agent-memory/<agent>/MEMORY.md`: one line per lesson that
outlives a run (an idiom that took two attempts, a known flake, a conflict pattern), committed
with the tree so the whole team of agents learns. An agent in a worktree writes it on its branch;
the `integrator` keeps both sides on merge. Task state lives in a `HANDOFF.md`, never in memory.
The `reviewer` has no memory on purpose: every change gets fresh eyes.

`/redesign <modules | N row>` (`.claude/workflows/redesign.js`) runs the loop above as one
workflow: plan the clusters (modules that share types together, a big module alone), one
redesigner per cluster in its own worktree (at most four at once), a reviewer per branch with one
fix round, then an integrator that merges the approved branches and runs `just ci`. Its result
is a branch; `main` moves only by hand, after the user's OK. Running it is the user's decision,
never the model's. The machine lock, `/tmp/emibsd/ci.lock`, is shared with LZ: one Mac, one
`ci` at a time.

## How it is measured

- `cargo xtask unsafe-report`: per subsystem, blocks, `unsafe fn`, `unsafe impl`, `unsafe
  trait`, other, total; the baseline at `lz-origin` and the current totals in `docs/STATUS.md`;
  the budget in `unsafe-budget.toml`, checked in `just ci`.
- `cargo xtask lz status`: inherited and redesigned modules per subsystem.
- `just bench`: boot time per arch, `tcpbench` between two VMs, vioblk throughput; before and
  after in the commit (`Bench:` trailer).
- `just diff-openbsd`: steps compared, equal, expected; the expected list never grows.

Baseline at `lz-origin` (`f5985f1d055a`), from the tools: the `Unsafe` line and the lineage table of
`docs/STATUS.md`, `unsafe-budget.toml` (the per-subsystem totals, written by
`cargo xtask unsafe-report --write`), the smoke count and the `diff-openbsd` result of the
first `just ci` and `just diff-openbsd` in this tree (recorded in the N0 section of
`docs/JOURNAL.md`), and the benchmarks of `just bench` once it exists (boot to `login:` per
arch, `smoke-tcpbench`, a vioblk number; amd64 under TCG is indicative only until M17 brings
real hardware). The blockers LZ had at `lz-origin` are inherited and listed in `docs/STATUS.md`:
an N-milestone report is not a regression for them.

## Measuring progress

`/progress [milestone]` (`.claude/skills/progress/`) prints one row per ROADMAP milestone,
computed when asked from `lineage.toml`, `docs/ROADMAP.md`, the LZ clone at the pin and git:
modules redesigned, adapted and inherited, LZ lines done and left (the lines of each module's LZ
sources at the pin), Rust lines, and the time the rest would take at the project's own average
of LZ lines redesigned per active day. Its conventions:

- a module belongs to the `N` row whose scope cell names its path in backticks (a directory, a
  file or a glob), the most specific pattern winning; a row whose scope is prose resolves
  nothing; modules no row names are listed apart;
- `adapted` modules are shown apart from `redesigned` ones: their call sites followed a
  redesign, nothing of their own changed; extras count nowhere;
- the time column is a linear extrapolation of the past; it promises nothing.

The numbers in README, STATUS and JOURNAL do not come from it: they come from
`cargo xtask lz status`, `cargo xtask unsafe-report` and the git commands `docs/JOURNAL.md` names.

## How LZ's later work is absorbed

`.claude/rules/lz-sync.md` and `docs/SYNC.md`: `cargo xtask lz drift` lists the LZ commits
after the pin, the native modules they touch and (`--functions`) the items; every one gets a
record in `lz-sync.toml` (`applied` with the EmiBSD commit and its `method`: `cherry-pick`,
`cherry-pick-conflicts` or `reimplemented`; `not-applicable` with a reason;
`covered-by-redesign` with the proof); security fixes are never skipped silently; the pin
moves in a `lz: bump pin` commit. `just ci` fails on an untriaged LZ commit. The three method
totals in each sync commit say whether the roadmap's order is costing re-implementations.

## How security changes are reviewed

`.claude/rules/security-review.md`: the listed areas are redesigned last in their subsystem,
carry a `Security-Review:` trailer, and pass `just diff-openbsd` and their smokes before the
commit; crypto, IPsec, WireGuard and softraid CRYPTO change last of all, with extra tests.

## Decisions

| Date | Decision |
|---|---|
| 2026-10-07 | Three stages: OpenBSD -> EmiBSD.LZ (the port, continuous) -> EmiBSD (native). Governance files live inside each repository. |
| 2026-10-07 | The migration happens when M14 is complete, on the user's signal, never with agents running. |
| 2026-10-07 | `enavarre-cl/EmiBSD` stays the public home of the native system; `enavarre-cl/EmiBSD.LZ` is public and locked down (Issues only). |
| 2026-10-07 | LZ never names EmiBSD; EmiBSD declares LZ in `lz/PINNED.md`, `lineage.toml`, the `LZ:` trailer, this section and the README. |
| 2026-10-07 | Layout: subsystem directories fixed, free inside. |
| 2026-10-08 | Function-level traceability lives in `lineage.toml` (`[[module.fn]]`, exceptions only), not in the files; the RCS ident lines are dropped (decision 20). |
| 2026-10-09 | The port's repository became EmiBSD.LZ (public, locked down); this one is native from `lz-origin` = `f5985f1d055a`; `reference/openbsd-src` kept at LZ's pin; the milestone order N1..N8 recommended, the user's call at each start. |
| 2026-10-09 | Authorship, the user's decision: every `.rs` under `sys/` and `tools/` carries the author's ISC block ("Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>") in its `<LICENSES>` zone, first (the latest change), before the original blocks, which never change, or alone where there are none; `license = "none"` now describes the C source, not the Rust file; `lz check` requires the block and compares inherited modules without it. |
| 2026-10-09 | From the external review: `adapted` as a third module status; the `method` of every applied sync; a timing rule for widely used items; `docs/SYNC.md`; the blockers inherited at `lz-origin` recorded in the baseline. |
| 2026-10-10 | EmiBSD.LZ's subagent set-up (`b0901dd3`) adopted: roles in `.claude/agents/` (`redesigner` and `mechanical` for LZ's porters), one contract in `.claude/rules/subagents.md`, `.claude/agent-memory/` committed, the machine lock `/tmp/emibsd/ci.lock` shared with LZ, `/redesign` and `/progress` (the ROADMAP scope resolved against `lineage.toml`; no milestone key in its rows); no `external-bugs` role: slips go to the user. |

## Later

Graphics (drm, nouveau, Mesa, GSP firmware) is a proposal for LZ after M17; it would reach this
system through `lz-sync` like any other LZ work.
