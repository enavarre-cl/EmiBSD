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
- Naming: C function names verbatim (`uvm_fault`, `tsleep_nsec`), types CamelCase (`Proc`,
  `VmMapEntry`), constants unchanged (`PAGE_SIZE`, `MAXCOMLEN`). Deviating from an OpenBSD name
  needs the user's OK.
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
- Idiom decisions live in `docs/C_TO_RUST.md`. Follow them; propose a new row rather than improvising.
- Dependencies allowed in `sys/`: `libkern`, `bitflags`; dev-only `proptest`. Lists and trees
  are our own (`sys/sys/queue.rs`, `sys/sys/tree.rs`), not `intrusive-collections`. Not allowed: `limine` (0.6+ is nightly-only, 0.5 is frozen at base revision 3; the
  protocol structs are written in `sys/stand/limine.rs` from the spec), `x86_64`, `aarch64-cpu`,
  `spin`, `uart_16550`, `fdt`, `linked_list_allocator`, `buddy_system_allocator`, or any crate that
  replaces code OpenBSD has. Porting that code is the project.
- Every `pub` item has a doc comment (`missing_docs` is warn; `just clippy` uses `-D warnings`).
- File layout, top to bottom, one blank line between sections, empty sections omitted:
  1. the `/* $OpenBSD ... */` line(s), then the license block(s) wrapped in `/* <LICENSES> */`
     and `/* </LICENSES> */` on lines of their own (ported files; the markers only mark, the
     licence text stays verbatim). Read a ported file from the closing marker on:
     `sed -n '/<\/LICENSES>/,$p' file`;
  2. `//!` docs: summary, `Upstream:`, prose, `## Deviations`;
  3. `mod` declarations (crate roots and `mod.rs` only), then `use` lines as rustfmt orders them;
  4. constants: `const`, constant-only `pub mod` blocks (`memmap_type`), and `macro_rules!` that
     define constants or types, placed just before their first use;
  5. types: `struct`, `enum`, `type`, each followed by its inherent `impl` blocks and `unsafe impl`
     marker traits;
  6. `static`s;
  7. traits;
  8. free functions and trait `impl`s, in the order of the C file;
  9. compile-time checks (`const _: () = { assert!(..) };`);
  10. `#[cfg(test)] mod tests`: inline when it is 50 lines or shorter, otherwise `mod tests;` with
      the body in `<name>/tests.rs` (`use super::*;` sees the parent's private items either way).
  Within a section keep the C file's order; the OpenBSD header/implementation split is the
  interface/implementation split (types in `sys/sys/<header>.rs`, functions in the `.c`'s module,
  traits in `sys/machine/<header>.rs`).
