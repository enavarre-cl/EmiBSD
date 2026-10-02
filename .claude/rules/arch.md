---
paths:
  - "sys/arch/**"
  - "sys/machine/**"
---

# Architecture code and the machine contract

- `sys/machine/<header>.rs`, one module per OpenBSD header, is the contract: `param.rs` for
  `<machine/param.h>` and `<machine/_types.h>`; `cpu.rs` for `<machine/cpu.h>`/`cpufunc.h`, the
  QEMU exit, `boot(9)` and `delay(9)`; `cons.rs` for `consinit()` (the console framework itself is
  generic, `dev/cons.rs`); `bus.rs` for `bus_space(9)` (tag and handle types per arch, C-named free
  functions); `db_machdep.rs` for `ddb`'s needs (`db_stack_trace_print`, `frame_address`,
  `db_enter`); more as milestones add them (`pmap.rs`, `intr.rs`). Generic code only sees
  `crate::machine` (every module is re-exported there). Adding a trait method means implementing
  it for amd64, arm64 AND host in the same commit; `sys/machine/mod.rs` asserts the selected arch
  implements every trait.
- Nothing generic lives in `sys/arch/`. If two archs would write the same code, it belongs in
  `kern/` or `uvm/`. If one arch needs a hook the other does not, it is still a trait method with a
  no-op implementation, never a `#[cfg(target_arch)]` in generic code.
- Each arch directory mirrors OpenBSD: `arch/<arch>/<arch>/` for `.c`/`.S` ports (`locore`,
  `machdep`, `pmap`, `trap`), `arch/<arch>/include/` for header ports, `arch/<arch>/conf/kernel.ld`,
  `arch/<arch>/dev/` for arch-only drivers. Module names in `include/` are identical across archs:
  `param`, `vmparam`, `cpu`, `intr`, `frame`, `pte`.
- `include/` holds constants, `#[repr(C)]` hardware structs and `#[inline]` register accessors.
  Never state. State lives in `machdep`/`cpu` modules.
- `asm!`/`global_asm!` only under `sys/arch/`. Every `asm!` has justified `options(...)` (`nomem`,
  `nostack`, `preserves_flags` only when true) and a `// SAFETY:`. amd64 uses Intel syntax (the
  Rust default). Whole-file assembly stays a real `.S` file next to the Rust and is pulled in with
  `global_asm!(include_str!("locore.S"))`, so it can be diffed against OpenBSD's.
- Interrupt and exception entry stubs are assembly (like OpenBSD's `vector.S`/`exception.S`), not
  `extern "x86-interrupt"` (unstable).
- `arch/host/` is a test double: it prints to stdout, has no-op SPL, a `HashMap`-backed fake pmap.
  It must not grow logic. If a test needs behaviour, the behaviour belongs in generic code.
- Targets are `x86_64-unknown-none` and `aarch64-unknown-none-softfloat`. No FP/SIMD in kernel code:
  the softfloat target enforces it on arm64, the `none` target on amd64.
- Hardware addresses and IRQ numbers come from firmware (ACPI, DTB). A hardcoded address is a
  bootstrap shortcut and carries a `// TODO(Mn):` naming the milestone that removes it.
