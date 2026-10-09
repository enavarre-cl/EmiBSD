# EmiBSD

An operating system in Rust, derived from EmiBSD.LZ (`lz/PINNED.md`), the faithful file-by-file
port of the OpenBSD kernel: a standalone `#![no_std]` kernel for amd64 and arm64, booted by its
own boot(8)/efiboot or by Limine, running in QEMU, with OpenBSD's own userland unmodified.
OpenBSD's behaviour is the specification; LZ's shape is the starting point. What userland sees
never changes; the inside may: this repository redesigns, LZ ports. `docs/PHASE2.md` is the
process.

Everything in this repository (code, comments, docs, commits) is in English.

## Non-negotiables

- Both architectures build on every commit (`just build`). Never land amd64-only or arm64-only work.
- Generic code (`kern/`, `uvm/`, `dev/`, `sys/`) reaches arch code ONLY through `crate::machine`.
  `crate::arch::amd64` / `crate::arch::arm64` are never named outside `sys/arch/` and `sys/machine/`.
- The system-call ABI and the behaviour OpenBSD's userland sees never change: the ABI equals
  LZ's at the pin, and additions arrive only through `lz-sync:` commits (`.claude/rules/lz-sync.md`).
- `reference/openbsd-src/`, `reference/emibsd-lz/` and `lz/` are read-only. LZ is never written
  to and never told this repository exists. Never copy C verbatim (`.claude/rules/reference-readonly.md`).
- Every file keeps the full copyright/licence block(s) of every LZ file it derives from, whole,
  and every `.rs` under `sys/` and `tools/` ends its `<LICENSES>` zone with the author's ISC block
  (N0b, `.claude/rules/scope-and-stubs.md`).
- `lineage.toml` is updated in the same commit as the change it describes (`.claude/rules/lineage.md`).
- Every commit carries its numbers: `LZ:` per source file, `Unsafe:` when a count moved
  (`.claude/rules/git-commits.md`). `cargo xtask unsafe-report --check` stays green.
- No `std` outside `sys/arch/host/`, `#[cfg(test)]` code and `tools/xtask/`.
- Stable toolchain, pinned in `rust-toolchain.toml`. No nightly features, ever.
- A new crate dependency needs the user's OK, a row in `docs/ARCHITECTURE.md` ("Dependencies") and
  the allowlist in `.claude/rules/rust-kernel.md`.

## Directory map

```
reference/openbsd-src/       OpenBSD C source at LZ's pin: sparse clone, gitignored, READ-ONLY (the behavioural spec)
reference/emibsd-lz/         EmiBSD.LZ, full history, gitignored, READ-ONLY (what every module started from)
lz/PINNED.md                 the LZ commit this tree is synced to (machine-read by `cargo xtask lz check`)
lineage.toml                 native module -> LZ files and items, the source of truth for provenance
lz-sync.toml                 one triage record per LZ commit after the pin
unsafe-budget.toml           per-subsystem unsafe totals that `just ci` enforces
sys/                         package `bsd`, the kernel; the subsystem directories are LZ's, the inside is ours
  kern/ uvm/ dev/ net/ ...   fixed subsystems; files inside may be split, merged, renamed or moved
  machine/ arch/ stand/      unchanged from LZ: the machine contract, the two archs, the boot glue
  lib/libkern/ lib/libz/     leaf crates
tools/xtask/                 host tooling: `cargo xtask {image,qemu,smoke,lz,unsafe-report,diff-openbsd,...}`
docs/                        PHASE2 (the process), ARCHITECTURE, IDIOMS, ROADMAP (N0..), SYNC, JOURNAL, STATUS, SETUP, C_TO_RUST (frozen)
```

Two provenance rules:

1. Every `.rs` under `sys/` is a `[[module]]` of `lineage.toml` naming the LZ files it derives
   from, or an `[[extra]]` with a reason. An `inherited` module is byte-identical to LZ's (modulo
   the RCS ident lines); an `adapted` module changed only because a module it uses was
   redesigned; a `redesigned` module is ours.
2. A redesigned module keeps the licence blocks and the `//! Upstream:` lines of every LZ source,
   adds one `//! LZ: <path>@<12-hex>` per source and a `## Redesign` section, and lists in
   `lineage.toml` (`[[module.fn]]`) every item it splits, renames, moves, merges or drops.

## Commands

| Command | What |
|---|---|
| `just build` | kernel for amd64 + arm64 |
| `just run-amd64` / `just run-arm64` | boot in QEMU, serial on stdio |
| `just smoke` | boot both archs headless, assert serial output and exit code; recipes run `JOBS` (4) at a time |
| `just test` | host unit tests (libkern + bsd through arch/host) |
| `just test-ref` | tests that cross-check constants against the C reference |
| `just clippy` / `just fmt` | clippy for amd64, arm64 and host with `-D warnings` / format check |
| `just check-lineage` | `cargo xtask lz check`: every module's provenance, blobs, rows and zones |
| `just check-drift` | `cargo xtask lz drift --strict`: no LZ commit after the pin without a record |
| `just check-unsafe` | `cargo xtask unsafe-report --check`: no subsystem over its budget |
| `just ci` | all of the above; must be green before a commit |
| `just ci-full` | `ci` with every smoke on `-smp 4` (not 2), then the installer end to end on both archs (`smoke-install-*`, needs `just comp`); must be green before a milestone is met |
| `just lz-sync` | fetch LZ, list the commits to triage with the items they touch (`docs/SYNC.md`) |
| `just diff-openbsd` | the oracle: the same scenarios on EmiBSD and on a real OpenBSD, compared step by step |

Never call `qemu-system-*`, `cargo build --target ...` or `rustup` by hand; use `just`.
Tool installation lives in `docs/SETUP.md` and needs the user's explicit go-ahead.

## Re-engineering one module (the loop)

1. Pick: one module of the subsystem the roadmap names; `cargo xtask lz status` shows what is
   still inherited. A redesign of an item used by many modules waits for the LZ milestone that
   touches them to close (`.claude/rules/re-engineering.md`).
2. Measure: `cargo xtask unsafe-report` for the subsystem, the smokes that cover the module, the
   `diff-openbsd` scenarios, a benchmark when performance is the point.
3. Read the LZ module(s) completely (the `<CODE>` zone) and the C they ported. No skimming.
4. Redesign with ownership first; the ABI edge and `crate::machine` untouched. A new idiom is a
   row of `docs/IDIOMS.md` in the same commit.
5. Prove: `// SAFETY:` soundness arguments for what stays, tests in `<TESTS>` for what changed,
   the smokes, `just diff-openbsd`; `.claude/rules/security-review.md` for its areas.
6. Record: `lineage.toml` (`redesigned`, the `lz` list, a `[[module.fn]]` row per item split,
   renamed, moved, merged or dropped, `adapted` on the modules whose call sites followed,
   `[[dropped]]`), the `//! LZ:` lines and `## Redesign`. `cargo xtask lz trace` must answer
   where every item went.
7. `just ci` green -> commit one step, trailers `LZ:`, `Unsafe:`, `Bench:`.

Details: `.claude/rules/re-engineering.md`; LZ's later work: `.claude/rules/lz-sync.md` and
`docs/SYNC.md`.

## Definition of done

`just ci` green on both archs (lineage, drift and budget checks included); `lineage.toml`
updated; `LZ:` and `Unsafe:` trailers in the commit; `docs/IDIOMS.md` or `docs/ARCHITECTURE.md`
updated if a new idiom or structural decision was made; `docs/STATUS.md` updated at the end of
the session.

## Never

- Edit, format or create anything under `reference/` or `lz/`; write to, push to, or mention
  this repository in, `reference/emibsd-lz/`.
- Change anything userland can see: a syscall, a sysctl MIB, an ioctl, a device major, a
  `#[repr(C)]` shared with userland, a line a smoke or `diff-openbsd` compares.
- Copy C verbatim or translate line by line without re-expressing the semantics in Rust.
- `todo!()`, `unimplemented!()`, or silently dropping a code path (`.claude/rules/scope-and-stubs.md`).
- `unsafe` without a `// SAFETY:` comment; `unsafe fn` without a `# Safety` section.
- `static mut`. Use atomics, `Mutex<T>` or the documented `StaticCell<T>`.
- Raise a subsystem's unsafe budget, bump `lz/PINNED.md` or drop an LZ file without the user.
- Mix a redesign with an `lz-sync:` or a `lineage:` commit.
- Nightly features, `std` in kernel code, `[build] target` in `.cargo/config.toml`.
- Run `brew`, `rustup`, or change anything outside the repo without asking.
- Force-push, amend published commits, commit `reference/`, `*.img` or `target/`.

## Working with the user

- Reply in the language the user writes in (usually Spanish). All artifacts stay in English.
- The user is new to Rust but knows other languages. When a choice rests on a non-obvious Rust
  concept (ownership, `UnsafeCell`, `cfg`, traits vs generics, `Pin`, `MaybeUninit`), add a one- or
  two-sentence **Rust note**. One concept at a time. Short.
- When explaining a redesign, show the LZ excerpt (cite `reference/emibsd-lz/sys/<path>:<line>`)
  and the native side by side, and the C where the semantics come from.
- Call out every `unsafe` block you write and why it is sound.
- Ask before: adding a dependency, raising an unsafe budget, bumping the toolchain or the LZ
  pin, dropping an LZ file, moving a whole subsystem, running anything outside the repo.
- Be honest about scale: the full redesign is a multi-year effort. Progress is measured by
  `cargo xtask lz status`, `cargo xtask unsafe-report` against the baseline, and `just smoke`,
  not by promises.

## Where things are documented

- `docs/PHASE2.md`: the process, the origin, the decisions.
- `docs/ARCHITECTURE.md`: the design, inherited from LZ at `lz-origin` and rewritten as
  subsystems are redesigned.
- `docs/IDIOMS.md`: LZ shape -> native shape decisions. `docs/C_TO_RUST.md` (frozen): C -> LZ.
- `docs/ROADMAP.md`: milestones N0.. with mechanical exit criteria. `docs/SYNC.md`: the monthly
  sync with LZ, step by step.
- `docs/SETUP.md`: toolchain, QEMU and the boot loaders on macOS.
- `docs/STATUS.md` and `lz/PINNED.md` are imported below, so they are always in context.

@docs/STATUS.md
@lz/PINNED.md
