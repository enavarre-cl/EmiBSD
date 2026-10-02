# Architecture

Why the tree looks the way it does. Read when changing structure, not every session.

## Goal

Re-implement the OpenBSD kernel in Rust, file by file, preserving OpenBSD's structure, names and
semantics, as a standalone `#![no_std]` kernel for amd64 and arm64, booted by Limine, run in QEMU.
The C tree under `reference/openbsd-src/sys` is the specification.

## One kernel crate, not one crate per subsystem

`sys/` is a single Cargo package, `bsd` (OpenBSD's kernel image is `/bsd`). Subsystems are modules:
`kern`, `uvm`, `dev`, `ddb`, `sys` (headers), `machine`, `arch`.

Reason: kern, uvm and arch are mutually recursive (`trap → uvm_fault → pmap → tsleep → sched`).
OpenBSD resolves that at link time. Cargo forbids crate cycles, so a crate per subsystem would force
trait inversions everywhere. One crate resolves the cycles the way C does.

Exceptions are true leaves only: `sys/lib/libkern` (OpenBSD builds it as a library too). A module
may be promoted to a crate only if it uses nothing from `crate::{kern, uvm, arch, machine}`.

No `src/` directory (`[lib] path = "lib.rs"`), so C and Rust paths differ only by extension.

## Path and name mapping

| OpenBSD | Here |
|---|---|
| `sys/kern/subr_prf.c` | `sys/kern/subr_prf.rs` |
| `sys/sys/proc.h` (`struct proc`) | `sys/sys/proc.rs` (`pub struct Proc`) |
| `sys/kern/kern_fork.c` (`fork1()`) | `sys/kern/kern_fork.rs` (`impl Proc { pub fn fork1 }` or `pub fn fork1`) |
| `sys/arch/amd64/amd64/pmap.c` | `sys/arch/amd64/amd64/pmap.rs` |
| `sys/arch/amd64/include/pte.h` | `sys/arch/amd64/include/pte.rs` |
| `sys/lib/libkern/strlcpy.c` | `sys/lib/libkern/strlcpy.rs` |
| `sys/arch/*/stand/`, `boot(8)`, `efiboot` | `sys/stand/` (Limine glue); skipped: `replaced-by-limine` |
| `sys/conf/`, `config(8)`, Makefiles, `newvers.sh` | Cargo features and `tools/xtask`; not ported |

Types live where the **header** is; functions live where the **`.c`** is. Rust allows inherent
`impl` blocks in any module of the defining crate, which is exactly the header/implementation split.

## The `machine` contract

`sys/machine/<header>.rs` holds the traits standing in for `<machine/*.h>` and `cpufunc.h`, one
module per OpenBSD header (`param.rs`, `cpu.rs`, `cons.rs`, later `pmap.rs`, `intr.rs`, ...), all
re-exported from `sys/machine/mod.rs`, which also re-exports `crate::arch::current::Machine` and
asserts at compile time that it implements every trait. Generic code names only `crate::machine`.

Constants travel the same way as functions: `MachineParam` (M1) carries `<machine/param.h>` and
the alignment rules of `<machine/_types.h>` as associated consts, each arch defines them in
`arch/<arch>/include/{param,_types}.rs`, and `sys/sys/param.rs` re-exports them, so generic code
imports `PAGE_SIZE` from `sys::param` exactly as C includes `<sys/param.h>`.

Three implementors:

- `sys/arch/amd64`: `cfg(all(target_os = "none", target_arch = "x86_64"))`
- `sys/arch/arm64`: `cfg(all(target_os = "none", target_arch = "aarch64"))`
- `sys/arch/host`: `cfg(not(target_os = "none"))`, a std-backed double. It makes `cargo test` work
  on macOS and proves mechanically that the contract is complete (a missing method fails to compile
  for the host too). It must not grow logic.

## Boot flow

Limine (UEFI, both archs) → `_start` in `sys/stand/mod.rs` (protocol structs in
`sys/stand/limine.rs`) → `machine::BootInfo` (bootloader-neutral: memory map, HHDM offset, kernel
load addresses, DTB/RSDP pointers; it lives in `sys/machine/bootinfo.rs` so the machine traits can
name it) → `machine::Machine::early_init(&BootInfo)` (the polled console; on arm64 also a temporary
device mapping) → `kern::init_main::main` (OpenBSD's `main()`, milestone M2; until then
`stand::boot_main` prints the banner and the memory map and leaves).

Leaving the machine goes through `machine::Exit`: under feature `qemu`, amd64 uses the
`isa-debug-exit` device and arm64 the semihosting `SYS_EXIT` call, both making QEMU exit with
status 33 for success and 35 for failure, which `xtask smoke` checks. Without the feature the CPU
halts.

At entry Limine (protocol base revision 6) guarantees: 64-bit mode, MMU on, kernel mapped at
`0xffffffff80000000`, a higher-half direct map of physical memory (HHDM), a memory map, a stack of
at least 64 KiB, interrupts masked, secondary CPUs parked. Limine's page tables live in
bootloader-reclaimable memory, so the kernel owns its own `pmap` and trap vectors before reclaiming
(M3/M4).

Why Limine: it is the only option with an identical boot contract on amd64 and aarch64, which keeps
the arch split focused on real kernel work (traps, pmap, interrupts). OpenBSD also keeps the
bootloader separate from the kernel, so this is faithful in spirit. `bootloader` (crate) is
x86_64-only; QEMU `-kernel` raw loading on `virt` would mean two unrelated early-boot paths.

The protocol is specified in https://github.com/limine-bootloader/limine-protocol (`PROTOCOL.md`,
`include/limine.h`); `sys/stand/limine.rs` implements the subset the kernel asks for, at base
revision 6, with no crate in between (see "Dependencies").

## Toolchain and targets

Stable Rust, pinned in `rust-toolchain.toml`.

- `x86_64-unknown-none`: kernel code model, no SSE/AVX, no red zone.
- `aarch64-unknown-none-softfloat`: no NEON/FP. The hardfloat variant lets the compiler use `q`
  registers in `memcpy`; a kernel that does not save FP state on traps must not touch them. This is
  the Rust equivalent of OpenBSD's `-mgeneral-regs-only`.

No nightly: `asm!`/`global_asm!`, `#[panic_handler]`, `#[global_allocator]`, `#[unsafe(no_mangle)]`
and `#[unsafe(link_section)]` are stable; `core` and `alloc` ship precompiled for both targets.
`extern "x86-interrupt"` is unstable, so interrupt stubs are assembly, like OpenBSD's `vector.S`.
`custom_test_frameworks` is unstable, so in-QEMU tests are serial smoke tests driven by `xtask`.

## Linking

`sys/arch/<arch>/conf/kernel.ld` (identical except `OUTPUT_FORMAT`): base `0xffffffff80000000`,
`PHDRS` text/rodata/data, Limine request sections kept, `.eh_frame`/`.note` discarded.
`sys/build.rs` passes it with `cargo:rustc-link-arg-bins` only when `target_os = "none"`.

Per-target rustflags in `.cargo/config.toml`: `relocation-model=static` (non-PIE higher-half kernel)
and `force-frame-pointers=yes` (backtraces in `panic`). No `[build] target`, so host builds stay
the default and `cargo test` just works.

## Cargo features ↔ `option(4)`

| Feature | OpenBSD | Effect |
|---|---|---|
| `alloc` | — | `extern crate alloc`; default from M3 |
| `diagnostic` | `option DIAGNOSTIC` | `kassert!` active |
| `debug` | `option DEBUG` | `kdassert!` active |
| `qemu` | — | QEMU-only exits (`isa-debug-exit`, semihosting) and shortcuts |

More appear as they are needed (`multiprocessor`, `small_kernel`, ...), one per `option(4)`.

## Dependencies

| Crate | Where | Why it is not OpenBSD code |
|---|---|---|
| (none for Limine) | `sys/stand/limine.rs` | the `limine` crate was dropped: 0.6+ needs nightly (`ptr_metadata`), 0.5 is stable but frozen at base revision 3, which Limine has already tried to drop once. The protocol is about twenty `#[repr(C)]` structs; they are written from `PROTOCOL.md` |
| `bitflags` | `sys/` | typed flag sets for `#define` groups; a macro, no runtime |
| (none for lists and trees) | `sys/sys/queue.rs`, `sys/sys/tree.rs` | `intrusive-collections` was dropped at M1: the OpenBSD macros are short, their semantics are the project's to keep, and a crate's policy changes would bind us as the `limine` crate's did |
| `proptest` | dev-only | property tests for libkern |
| `serde`, `toml` | `tools/xtask` | tracker parsing |
| `fatfs` | `tools/xtask` | writes the FAT boot image; a host tool, not kernel code |

Not allowed: crates that replace OpenBSD code (`x86_64`, `aarch64-cpu`, `spin`, `uart_16550`,
`fdt`, `linked_list_allocator`, `buddy_system_allocator`). Porting that code is the project.

## Deviations from OpenBSD (deliberate)

- Limine instead of `boot(8)`/`efiboot`.
- Cargo features and `xtask` instead of `config(8)`, Makefiles and `newvers.sh`.
- `aarch64-unknown-none-softfloat` target; Intel syntax for amd64 inline assembly.
- `Result<T, Errno>` instead of `int` returns; RAII guards for `spl`/mutex.
- A host test double (`arch/host`), which OpenBSD does not have.
- Bootstrap consoles before their drivers exist (M0 to M2): amd64 polls COM1 through `pio.h`;
  arm64 reaches the PL011 through a one-block identity map of the first GiB installed in
  `TTBR0_EL1` with Device-nGnRnE attributes (`arch/arm64/arm64/earlycons.rs`), because the Limine
  protocol maps RAM but not devices. `pmap` (M3) and the FDT (M4) replace it.

Every file-level deviation is in that file's `//! ## Deviations` list and in `ports.toml` `notes`.

## Testing architecture

See `.claude/rules/testing.md`. Host tests exist because of `arch/host`. QEMU smoke tests exist
because `custom_test_frameworks` is unstable. Reference-backed tests exist because constants copied
by hand drift.
