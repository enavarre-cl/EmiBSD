# Phase 2: improving the port

**Status: draft (2026-10-05). Phase 2 has not started; nothing here applies to current work.**
Until the user starts Phase 2, every rule of Phase 1 (`CLAUDE.md`, `.claude/rules/`) stays in
force unchanged.

## Phase 1 and Phase 2

- **Phase 1** (current) is the faithful, as-is port. The C sources under
  `reference/openbsd-src` are the specification: names, structure and semantics stay OpenBSD's,
  and every deviation is written down (`docs/ARCHITECTURE.md`, each file's `## Deviations`).
- **Phase 2** improves the port, using Phase 1 as a measured baseline. It follows the criteria of
  DARPA's TRACTOR program (Translating All C to Rust), which judges a translation on functional
  correctness, safety guarantees, competitive performance and long-term maintainability.
  Phase 1 covers correctness; Phase 2 covers the rest.

## Entry criteria

- The Phase 1 milestones the user chooses as the cut-off are met, with `just ci` green.
- The baseline is tagged `phase1-baseline`, and its metrics are recorded (below): the `unsafe`
  counts, the smoke results and the benchmarks.

## Goals, in priority order

1. **Safety.** Fewer and smaller `unsafe` blocks, safe abstractions over the ones that remain,
   and a soundness argument wherever `unsafe` stays.
2. **Maintainability.** More idiomatic Rust where the Phase 1 shape exists only to mirror C:
   `Cell`-everywhere structs, raw-pointer links, C-shaped out parameters.
3. **Performance.** Measured improvements only, and never at the cost of 1 or 2.

## Rules

- The system call ABI and the behaviour OpenBSD's userland sees never change. Every smoke test
  stays green.
- Every change is checked against the Phase 1 baseline, and against real OpenBSD once the
  differential tests exist (`cargo xtask diff-openbsd`, proposed in the roadmap).
- One improvement per commit, with before and after numbers in the message: the `unsafe` count,
  a benchmark, or both.
- Deviations from OpenBSD's structure are allowed, and each one is recorded in this file, as
  Phase 1 records its deviations in `docs/ARCHITECTURE.md`.
- Crypto, IPsec, WireGuard and softraid CRYPTO change last, and only with extra tests.

## Metrics

- `unsafe` counts per subsystem (`kern`, `uvm`, `net`, `dev`, ...): `unsafe` blocks, `unsafe fn`,
  `unsafe impl`, from `cargo xtask unsafe-report` (planned). The totals are recorded in
  `docs/STATUS.md`.
- Benchmarks: boot time, `tcpbench` between two VMs (`smoke-tcpbench` exists), and file-system
  throughput on vioblk.
- Clippy at a stricter level, enabled subsystem by subsystem.

## Open questions (the user's; not decided here)

- Does Phase 2 keep the file-by-file mapping (`kern/tty.c` → `kern/tty.rs`), or may modules be
  reorganised?
- How are newer OpenBSD commits absorbed once the code diverges from the C?
- Which subsystem goes first? The candidate is `libkern` and the other leaves, which are well
  tested and low-risk.

## Rules that Phase 2 conflicts with (to settle before it starts)

These Phase 1 rules contradict the Phase 2 rules above. They are listed, not resolved. When
Phase 2 starts, each one needs the user's decision and an edit to the file named.

| Phase 1 rule | Where | Conflict |
|---|---|---|
| "This is NOT ... a new kernel design. OpenBSD's design is the design." | `CLAUDE.md` (intro) | Phase 2 allows deviations from OpenBSD's structure |
| One `.c` → one `.rs` with the same name in the same directory; types where the header is | `CLAUDE.md` (mapping rules), `porting-workflow.md` | the open question on reorganising modules |
| Keep OpenBSD function names verbatim; ask before renaming away from an OpenBSD name | `porting-workflow.md`, `CLAUDE.md` ("Working with the user") | idiomatic APIs may rename or reshape functions |
| Re-express the C's semantics; never mix a port with a refactor | `porting-workflow.md` | Phase 2 commits are refactors by definition |
| Commit subject scopes, one port per commit, an `Upstream:` trailer per ported file | `git-commits.md` | Phase 2 commits port nothing; they need a scope and a trailer convention, plus the before/after numbers |
| Every deviation lives in the file's `## Deviations` and in `ports.toml` `notes` | `scope-and-stubs.md`, `porting-workflow.md` | Phase 2 records deviations in this file; whether the per-file lists are kept as well needs deciding |
| Each document has one job; never duplicate content | `docs.md` | this file and `docs/ARCHITECTURE.md` would both hold deviations; their split needs defining |
| `ports.toml` maps each C path to its `.rs` and `ports check` validates paths and blobs | `ports-tracker.md`, `tools/xtask` | reorganised modules, or code diverged from the C, break that mapping and the drift checks |
| Never change the reference pin without the user's explicit OK | `CLAUDE.md`, `reference-readonly.md` | not a conflict; it bounds the open question on absorbing newer OpenBSD commits |
| A new crate needs the user's OK and an allowlist entry | `CLAUDE.md`, `rust-kernel.md` | not a conflict; safe abstractions that want a crate still go through it |
| M14b (code and test layout) | `docs/ROADMAP.md` | it reshapes every file; it should land before `phase1-baseline` so the baseline is not moved under Phase 2 |
