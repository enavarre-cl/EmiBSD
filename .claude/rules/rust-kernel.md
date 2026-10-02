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
- Pointers: `&T`/`&mut T` where aliasing is clear; `NonNull<T>` for intrusive links and manually
  managed lifetimes; raw pointers only at hardware/ABI edges. `UnsafeCell` for fields the C mutates
  behind a shared pointer, with a doc line saying which lock protects them.
- MMIO through `read_volatile`/`write_volatile` behind the `bus_space`-shaped API, never plain derefs.
- Idiom decisions live in `docs/C_TO_RUST.md`. Follow them; propose a new row rather than improvising.
- Dependencies allowed in `sys/`: `libkern`, `limine` (only `sys/stand/`), `bitflags`,
  `intrusive-collections`; dev-only `proptest`. Not allowed: `x86_64`, `aarch64-cpu`, `spin`,
  `uart_16550`, `fdt`, `linked_list_allocator`, `buddy_system_allocator`, or any crate that replaces
  code OpenBSD has. Porting that code is the project.
- Every `pub` item has a doc comment (`missing_docs` is warn; `just clippy` uses `-D warnings`).
