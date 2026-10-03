# EmiBSD

A re-implementation of the OpenBSD kernel in Rust: a standalone `#![no_std]` kernel, booted by
Limine, running on amd64 and arm64 in QEMU. The OpenBSD C sources under `reference/openbsd-src/sys`
are the specification. We port them file by file, keeping OpenBSD's structure, names and semantics.

This is NOT a Rust-for-Linux style hybrid, a wrapper around C, or a new kernel design.
OpenBSD's design is the design. Where we deviate, the deviation is written down.

Everything in this repository (code, comments, docs, commits) is in English.

## Non-negotiables

- Both architectures build on every commit (`just build`). Never land amd64-only or arm64-only work.
- Generic code (`kern/`, `uvm/`, `dev/`, `sys/`) reaches arch code ONLY through `crate::machine`.
  `crate::arch::amd64` / `crate::arch::arm64` are never named outside `sys/arch/` and `sys/machine/`.
- `reference/` is read-only. Never edit it, never copy C verbatim (`.claude/rules/reference-readonly.md`).
- Every ported file keeps the original `$OpenBSD$` line and the full copyright/license block.
- `ports.toml` is updated in the same commit as the port it describes.
- No `std` outside `sys/arch/host/`, `#[cfg(test)]` code and `tools/xtask/`.
- Stable toolchain, pinned in `rust-toolchain.toml`. No nightly features, ever.
- A new crate dependency needs the user's OK, a row in `docs/ARCHITECTURE.md` ("Dependencies") and
  the allowlist in `.claude/rules/rust-kernel.md`.

## Directory map

```
reference/openbsd-src/sys/   OpenBSD C source: sparse clone, gitignored, READ-ONLY (pin: reference/PINNED.md);
                             lib/ bin/ sbin/ usr.bin/ libexec/ include/ and gnu/{lib/libcompiler_rt,
                             llvm/compiler-rt} beside it are M8's userland
sys/                         package `bsd`, the kernel. Mirrors reference/openbsd-src/sys/ 1:1
  sys/ kern/ uvm/ dev/ ddb/  same meaning as in OpenBSD; sys/sys = headers -> types
  machine/                   the <machine/*.h> contract: traits every arch implements
  arch/{amd64,arm64,host}/   per-arch code; `host` is a std-backed test double for `cargo test`
  stand/                     Limine boot glue (replaces OpenBSD's boot(8)/efiboot)
  lib/libkern/               package `libkern`, a leaf crate
tools/xtask/                 host tooling: `cargo xtask {image,qemu,smoke,ports}`
docs/                        ARCHITECTURE, PORTING, C_TO_RUST, ROADMAP, SETUP, STATUS
ports.toml                   porting tracker, the source of truth
```

Two mapping rules:

1. `reference/openbsd-src/sys/<dir>/<name>.c` ↔ `sys/<dir>/<name>.rs`. Same name, same directory, no `src/`.
2. Types live where the C **header** is; functions live where the C **file** is.
   `struct proc` → `sys/sys/proc.rs` as `pub struct Proc`; `fork1()` → `sys/kern/kern_fork.rs`.

## Commands

| Command | What |
|---|---|
| `just build` | kernel for amd64 + arm64 |
| `just run-amd64` / `just run-arm64` | boot in QEMU, serial on stdio |
| `just smoke` | boot both archs headless, assert serial output and exit code |
| `just test` | host unit tests (libkern + bsd through arch/host) |
| `just test-ref` | tests that cross-check constants against the C reference |
| `just clippy` / `just fmt` | clippy for amd64, arm64 and host with `-D warnings` / format check |
| `just check-ports` | validate `ports.toml` |
| `just ci` | all of the above; must be green before a commit |

Never call `qemu-system-*`, `cargo build --target ...` or `rustup` by hand; use `just`.
Tool installation lives in `docs/SETUP.md` and needs the user's explicit go-ahead.

## Porting one file (the loop)

1. Pick: `cargo xtask ports next` lists `todo` entries whose dependencies are done.
2. Read the `.c` AND its `.h` AND the `(9)` man page comments completely. No skimming.
3. List every function, struct and macro it uses; mark each ported / unported in `ports.toml`.
4. Port leaf dependencies first. Set the entry to `wip`.
5. Write `sys/<same path>.rs`: license header, `//!` doc with an `Upstream:` line, idiomatic Rust
   that preserves semantics (`docs/C_TO_RUST.md` is the idiom table).
6. Host-testable logic gets `#[cfg(test)] mod tests` in the same file, or in `<name>/tests.rs`
   once the tests pass 50 lines. Section order inside a file: `.claude/rules/rust-kernel.md`.
7. `just ci` green → `status = "ported"`, fill `upstream_commit` and `upstream_blob`.
8. One commit per file or coherent cluster; trailer `Upstream: <c path>@<12-hex>`.

Details: `.claude/rules/porting-workflow.md` and `docs/PORTING.md`.

## Definition of done

`just ci` green on both archs; `ports.toml` row updated; `Upstream:` trailer in the commit;
`docs/C_TO_RUST.md` or `docs/ARCHITECTURE.md` updated if a new idiom or structural decision was made;
`docs/STATUS.md` updated at the end of the session.

## Never

- Edit, format or create anything under `reference/`.
- Copy C verbatim or translate line by line without re-expressing the semantics in Rust.
- `todo!()`, `unimplemented!()`, or silently dropping a code path (`.claude/rules/scope-and-stubs.md`).
- `unsafe` without a `// SAFETY:` comment; `unsafe fn` without a `# Safety` section.
- `static mut`. Use atomics, `Mutex<T>` or the documented `StaticCell<T>`.
- Nightly features, `std` in kernel code, `[build] target` in `.cargo/config.toml`.
- Run `brew`, `rustup`, or change anything outside the repo without asking.
- Force-push, amend published commits, commit `reference/openbsd-src/`, `*.img` or `target/`.

## Working with the user

- Reply in the language the user writes in (usually Spanish). All artifacts stay in English.
- The user is new to Rust but knows other languages. When a choice rests on a non-obvious Rust
  concept (ownership, `UnsafeCell`, `cfg`, traits vs generics, `Pin`, `MaybeUninit`), add a one- or
  two-sentence **Rust note**. One concept at a time. Short.
- When explaining a port, show the C excerpt (cite `path:line`) and the Rust side by side.
- Call out every `unsafe` block you write and why it is sound.
- Ask before: adding a dependency, renaming something away from its OpenBSD name, skipping a file,
  bumping the toolchain or the reference pin, running anything outside the repo.
- Be honest about scale: the full kernel is a multi-year effort. Progress is measured by
  `cargo xtask ports status` and `just smoke`, not by promises.

## Where things are documented

- `docs/ARCHITECTURE.md`: why the tree looks like this, the `machine` contract, boot flow, deviations.
- `docs/PORTING.md`: the human porting process and the generated status table.
- `docs/C_TO_RUST.md`: C idiom → Rust idiom decisions.
- `docs/ROADMAP.md`: milestones M0..M7 with mechanical exit criteria.
- `docs/SETUP.md`: toolchain, QEMU and Limine on macOS.
- `docs/STATUS.md` and `reference/PINNED.md` are imported below, so they are always in context.

@docs/STATUS.md
@reference/PINNED.md
