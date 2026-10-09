---
paths:
  - "sys/**/*.rs"
---

# Kernel Rust

- `#![no_std]` everywhere under `sys/`. `extern crate std` only in `sys/lib.rs` for the host
  build, in `sys/arch/host/`, and under `#[cfg(test)]`. `alloc` only behind `feature = "alloc"`
  until M3.
- Workspace lints are law: `unsafe_op_in_unsafe_fn`, `clippy::undocumented_unsafe_blocks`,
  `missing_safety_doc`, `todo`, `unimplemented`, `unwrap_used`, `expect_used` are denied. Never
  `#[allow]` them; fix the code. `clippy::panic` is warn: a `panic!` is allowed only in ported
  `panic()` paths and in `kassert!`, each with `#[allow(clippy::panic)]` and a one-line reason.
- Every `unsafe {}` block has a `// SAFETY:` comment stating the invariant that makes it sound.
  Every `unsafe fn` has a `# Safety` doc section stating what the caller must guarantee.
- Errors: `Result<T, Errno>` with `#[repr(i32)] pub enum Errno` in `sys/sys/errno.rs`
  (includes `ERESTART = -1`, `EJUSTRETURN = -2`). No `Option` for "failed", no `-1` returns.
- Naming: names at the ABI edge (syscalls, sysctl MIBs, ioctls, device majors, structs shared
  with userland) are OpenBSD's and never change. Internal items keep the LZ name (which is the
  C name) until their shape changes; a rename is a `[[module.fn]]` row of `lineage.toml`
  (`lineage.md`), the one record of it, so the old name stays greppable. Types CamelCase
  (`Proc`, `VmMapEntry`), constants unchanged (`PAGE_SIZE`, `MAXCOMLEN`).
- Arch access only via `crate::machine::*`. Naming `crate::arch::amd64` or `crate::arch::arm64`
  outside `sys/arch/` and `sys/machine/` is a bug.
- `static mut` is forbidden. Use atomics, the ported `Mutex<T>`, or `StaticCell<T>` with a
  documented invariant.
- `#[repr(C)]` only where layout matters (hardware, ABI, bootloader). Otherwise let Rust lay it out.
- Pointers: `&T`/`&mut T` where aliasing is clear; `Cell<*const T>` inside the `queue.h`/`tree.h`
  entries (the list's lock guards them, the API takes and returns `&T`); `NonNull<T>` for other
  manually managed lifetimes; raw pointers only at hardware/ABI edges. `UnsafeCell` for fields the C mutates
  behind a shared pointer, with a doc line saying which lock protects them.
- MMIO through `read_volatile`/`write_volatile` behind the `bus_space`-shaped API, never plain derefs.
- LZ's idioms are in `docs/C_TO_RUST.md` (frozen, the key to reading LZ code); native idioms,
  LZ shape -> native shape, are rows of `docs/IDIOMS.md`. Follow them; propose a new row rather
  than improvising.
- Every `unsafe` in a redesigned module carries a soundness argument, not a reference to the C
  (`unsafe-budget.md`).
- Dependencies allowed in `sys/`: `libkern`, `libz`, `bitflags`; dev-only `proptest`. The boot
  loaders' crates (M14) may also use `libsa`, `boot` and `efi` (ours, OpenBSD code); the kernel
  does not depend on them. Lists and trees
  are our own (`sys/sys/queue.rs`, `sys/sys/tree.rs`), not `intrusive-collections`. Not allowed: `limine` (0.6+ is nightly-only, 0.5 is frozen at base revision 3; the
  protocol structs are written in `sys/stand/limine.rs` from the spec), `x86_64`, `aarch64-cpu`,
  `spin`, `uart_16550`, `fdt`, `linked_list_allocator`, `buddy_system_allocator`, or any crate that
  replaces code OpenBSD has. Porting that code is the project.
- Every `pub` item has a doc comment (`missing_docs` is warn; `just clippy` uses `-D warnings`).
- File layout (M15): every `.rs` under `sys/` and `tools/` is split into zones, each opened and
  closed by a comment line of its own, in this order, and nothing else sits outside them but
  blank lines (or a generated-file banner); the RCS ident lines are gone since N0 (decision 20):
  - `/* <LICENSES> */` ... `/* </LICENSES> */`: every file. The licence block(s) of every LZ
    source come first, verbatim (the markers only mark), then the author's ISC block (N0b,
    `scope-and-stubs.md`) after one blank line. A module whose C source has no licence text
    (`license = "none"`), an `[[extra]]` and a file outside `lineage.toml` hold the author's
    block alone;
  - `/* <CODE> */` ... `/* </CODE> */`: everything that is not a licence or a test, in the
    section order below, one blank line between sections, empty sections omitted;
  - `/* <TESTS> */` ... `/* </TESTS> */`: only in files that have tests: the inline
    `#[cfg(test)] mod tests { .. }`, at the end, however long it is. `use super::*;` sees the
    parent's private items. No `<name>/tests.rs` exists; `cargo xtask lz check` validates
    the markers, their order and the licence/test rules (`lineage.md`).
  Read the code of a file with `sed -n '/<CODE>/,/<\/CODE>/p' file.rs`, its tests with
  `sed -n '/<TESTS>/,/<\/TESTS>/p'`. Section order inside CODE:
  1. `//!` docs: summary, the `Upstream:` line(s) of every LZ source, one `LZ: <path>@<12-hex>`
     line per source (redesigned modules only), prose, `## Deviations` (inherited gaps),
     `## Redesign` (what changed against LZ and why; redesigned modules only); inner
     attributes (`#![..]`);
  2. `mod` declarations (crate roots and `mod.rs` only), then `use` lines as rustfmt orders them;
  3. constants: `const`, constant-only `pub mod` blocks (`memmap_type`), and `macro_rules!` that
     define constants or types, placed just before their first use;
  4. types: `struct`, `enum`, `type`, each followed by its inherent `impl` blocks and `unsafe impl`
     marker traits;
  5. `static`s;
  6. traits;
  7. free functions and trait `impl`s, in the order of the C file;
  8. compile-time checks (`const _: () = { assert!(..) };`);
  9. `#[cfg(test)]` helpers other tests share (`pub(crate) fn foo_reset()`, `mod testutil;`):
     in CODE, not in TESTS.
  Within a section keep the C file's order; the OpenBSD header/implementation split is the
  interface/implementation split (types in `sys/sys/<header>.rs`, functions in the `.c`'s module,
  traits in `sys/machine/<header>.rs`).
