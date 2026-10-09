# Re-engineering workflow

Applies to every change of a module under `sys/` that moves it away from EmiBSD.LZ's shape.
The faithful port is LZ's job; here OpenBSD's behaviour is the specification, LZ's shape is the
starting point, and what userland sees never changes.

- The unit of work is one module of one subsystem (`kern`, `uvm`, `net`, `dev/<bus>`, ...).
  The subsystem directories are fixed; inside one, files may be split, merged, renamed or moved
  into subdirectories. A whole subsystem moves only when it is redesigned, in a `lineage:`
  commit of its own.
- Timing, not order: a redesign of an item used by more than 50 modules (`grep -rl` over
  `sys/`, or `cargo xtask lz status` for the subsystem) does not start while an LZ milestone
  that touches those modules is open; it waits for the close. Every LZ driver commit that
  arrives after a core type is redesigned becomes a re-implementation instead of a cherry-pick,
  and that cost is paid every month (`lz-sync.md`, the `method` totals).
- Before writing a line: read the LZ module(s) completely (the `<CODE>` zone), their
  `## Deviations`, and the C they ported (the `//! Upstream:` line names it under
  `reference/openbsd-src`). Big modules are read in ranges and redesigned by a subagent of
  their own (`large-changes.md`).
- Measure first: `cargo xtask unsafe-report` for the subsystem, the smokes that exercise the
  module (`tools/xtask`, the justfile's `smokes`), the `diff-openbsd` scenarios that touch it,
  and a benchmark (`just bench`) when performance is the point. The numbers go in the commit.
- Redesign with ownership first: data inside the lock that guards it, `Result<T, Errno>` and
  slices at the API, the `machine` contract untouched (`arch.md`), the ABI edge byte-identical
  (`sys/sys/syscall.rs`, the sysctl MIBs, ioctls, device majors, every `#[repr(C)]` shared with
  userland). An idiom used for the first time is a new row of `docs/IDIOMS.md` in the same
  commit.
- Prove: every `unsafe` that stays carries a `// SAFETY:` with a soundness argument (the
  invariant and who upholds it); every behaviour whose shape changed gets a test in the
  `<TESTS>` zone; the affected smokes and `just diff-openbsd` pass; a security-sensitive area
  follows `security-review.md`.
- Keep the gaps honest: an inherited `unported!` path stays listed in `## Deviations` and is
  closed by the redesign or kept visible, never widened; no new behavioural difference from
  OpenBSD (`tools/xtask/diff-openbsd/expected.toml` records only inherited ones).
- Record: `lineage.toml` (`status = "redesigned"`, the `lz` list, one `[[module.fn]]` row for
  every item split, renamed, moved, merged or dropped, `status = "adapted"` with `adapted_by`
  on every module whose call sites followed, `[[dropped]]` when an LZ file has no derivation
  left), the `//! LZ:` line per source and the `//! ## Redesign` section in the
  file (`lineage.md`, `rust-kernel.md`). `cargo xtask lz trace` must answer where every item
  went before the commit.
- Commit one step at a time (`git-commits.md`): the `LZ:` trailer per source file, the
  `Unsafe:` trailer when the subsystem's count changed, `Bench:` when measured. Never mix a
  redesign with an LZ sync (`lz-sync.md`).
- Before ending a session, update `docs/STATUS.md` (milestone, done, next, blockers) and run
  `cargo xtask lz status --write` if modules changed status.
