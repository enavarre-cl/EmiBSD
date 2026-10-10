# lineage.toml

The record of what each native module derives from in EmiBSD.LZ. `cargo xtask lz check`
validates it; `just ci` runs that. It replaces LZ's `ports.toml`: native never tracks C files,
it tracks LZ files.

```toml
[meta]
lz = "<full LZ commit>"            # must equal the Commit: line of lz/PINNED.md

[[module]]
rust   = "sys/kern/sched/runqueue.rs"          # the native file
lz     = ["sys/kern/kern_sched.rs"]            # LZ files it derives from, at the pinned commit
status = "inherited"                           # inherited | adapted | redesigned
adapted_by = ["sys/kern/sched/runqueue.rs"]    # adapted only: the redesigned modules whose API forced the change
notes = "..."                                  # optional: LZ's wip notes carried over at N0 (what is partial)
license = "Mach"                               # optional: a non-ISC/BSD/MIT licence, or "none"
[[module.fn]]                                  # function level: only the exceptions (below)
lz     = "sys/kern/kern_sched.rs::sched_choosecpu"
native = ["RunQueue::choose_cpu"]              # one or more; empty for dropped
kind   = "renamed"                             # split | renamed | moved | merged | dropped
reason = "..."                                 # required for dropped
lines  = "L120-L188"                           # optional, LZ lines at the pin, split only

[[extra]]                                      # a native file with no LZ source
rust   = "sys/kern/unported.rs"
reason = "project helper: visible stubs for unported subsystems"

[[dropped]]                                    # an LZ file nothing derives from any more
lz     = "sys/kern/kern_foo.rs"
reason = "absorbed by sys/kern/sched/runqueue.rs"
```

- `inherited` means untouched since the pin: the file's blob id equals the LZ file's at the
  pinned commit, both taken after the RCS ident lines (`/* $OpenBSD: ... $ */`, `/* $NetBSD:
  ... $ */`) are stripped (`git hash-object --stdin` of the stripped text on each side). Those
  lines are removed from every native file at N0 (decision 20); the original `<LICENSES>`
  blocks are never touched. The author's ISC block (`scope-and-stubs.md`, Authorship) is removed from
  the native side before the comparison, so a module that differs from LZ only by it stays
  `inherited`. A module an `applied` record of `lz-sync.toml` names in `modules` is compared
  with that LZ commit (the newest such record) instead of the pin, so a sync is green before
  the pin bump (`lz-sync.md`).
  The first commit that changes the file sets `status = "adapted"` or `"redesigned"` in the
  same commit.
- `adapted` means the module changed only at its call sites, because a type or API it uses was
  redesigned elsewhere (`Proc`, the pool API, ...): nothing of its own changed. `adapted_by`
  names the redesigned module(s) that forced it; each must be `redesigned`. An adapted module
  needs no `//! LZ:` lines, no `## Redesign` section and no `[[module.fn]]` rows; an LZ change
  to it is applied by cherry-pick with conflicts expected at the call sites (`lz-sync.md`). The
  transition is recorded in the redesign commit that caused it, with that commit's `Unsafe:`
  trailer. `lz status` shows the three columns.
- A `redesigned` module keeps the whole licence blocks and the
  `//! Upstream:` lines of every file in its `lz` list, and has one `//! LZ: <path>@<12-hex>`
  line per entry (`rust-kernel.md`, file layout). Outside the core of `unsafe-core.toml` it is
  also `forbid`: its `mod` declaration carries `#[forbid(unsafe_code)]` (`docs/ZERO_UNSAFE.md`;
  `lz check` enforces it once the sweep has marked N1's modules). The attribute lives in the
  parent `mod.rs`, which `lineage.toml` does not track, so an `inherited` module can be `forbid`
  and stay byte-identical to LZ. A module that needs no redesign and is `forbid` as it stands
  may carry a per-file verdict (`reviewed`, with its reason) once the xtask supports it.
- Every `.rs` under `sys/` except `mod.rs`, `lib.rs`, `main.rs`, `build.rs`, `sys/machine/`,
  `sys/arch/host/` and `sys/stand/` is referenced by exactly one `[[module]]` or `[[extra]]`.
- The reverse direction: every `.rs` at the LZ pin (same exclusions) appears in some `lz` list,
  is an `[[extra]]` of the same path (a project helper N0 carried over), or is a `[[dropped]]`
  with a reason. Dropping an LZ file is the user's decision, recorded in
  the commit body.
- `license` carries what LZ's `ports.toml` recorded (`notes = "license: ..."` or
  `license = "none"`): the licence family when it is not ISC, BSD or MIT, or `"none"` when the
  C source has no licence text: it describes the C, not the Rust file, whose `<LICENSES>` zone
  then holds only the author's block. `LICENSE` lists the families; a new
  one is added there in the same commit.
- A subsystem that moves as a whole is one `lineage:` commit that rewrites the `rust` paths and
  nothing else.
- Function level, only the exceptions: an item the LZ file defines at the pin (a function, a
  method of an `impl` block as `Type::method`, a `struct`, `enum`, `trait`, `type`, `const`,
  `static` or `macro_rules!`) needs no row while an item of the same qualified name exists in a
  module whose `lz` list names that file. Otherwise it is one `[[module.fn]]` row under the
  module: `lz = "<LZ path>::<item>"`; `native` lists the targets (`name`, `Type::method`, or
  `sys/<path>.rs::name` for a native file that does not list the LZ file; empty for `dropped`);
  `kind` is `split`, `renamed`, `moved`, `merged` or `dropped`; `reason` is required for
  `dropped` (where the behaviour went, or why none is needed; userland never sees a
  difference); `lines` (`"L120-L188"`, LZ lines at the pin) only on `split`. No C path is
  written: it follows from the LZ file's `//! Upstream:` line.
- A rename has one record, the row; no doc-comment convention repeats it. `cargo xtask lz check`
  parses the LZ files' items and enforces coverage and targets; `cargo xtask lz trace` answers
  LZ -> native, native -> LZ and C -> native; `cargo xtask lz drift --functions` lands an LZ
  change on the native items that own the code now (`lz-sync.md`).
- Zone markers: `lz check` validates `/* <LICENSES> */`, `/* <CODE> */` and `/* <TESTS> */` as
  LZ's `ports check` does since M15. `<LICENSES>` is required in every `.rs` under `sys/` and
  `tools/` and starts with the author's block (`scope-and-stubs.md`): in a module with an `lz` list and a
  licensed C source the author's comes first, then that source's blocks, whole; with
  `license = "none"`, in
  an `[[extra]]` and in every file outside `lineage.toml` it holds the author's block alone.
- `[meta].lz` equals `lz/PINNED.md`; the `Commit:` of `reference/PINNED.md` equals the one LZ
  records at the pinned commit (`git -C reference/emibsd-lz show <PIN>:reference/PINNED.md`).
- `cargo xtask lz status --write` regenerates the table between the markers in `README.md` and
  the summary line between the markers in `docs/STATUS.md`. Never edit them by hand.
