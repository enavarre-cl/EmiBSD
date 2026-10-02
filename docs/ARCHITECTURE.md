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
module per OpenBSD header (`param.rs`, `cpu.rs` with `boot(9)`, `delay(9)` and, since M5, `curcpu()` with the
`cpu_info` accessors the clock code needs, `cons.rs` for
`consinit()`, `bus.rs` for `bus_space(9)`, `db_machdep.rs` for what `ddb` needs; later `pmap.rs`,
`intr.rs`, ...), all re-exported from `sys/machine/mod.rs`, which also re-exports
`crate::arch::current::Machine` and asserts at compile time that it implements every trait. Generic
code names only `crate::machine`. `bus.rs` also carries the C names as free functions
(`bus_space_read_1(t, h, o)`), so a driver reads like its original; the tag and handle types are
the architecture's (`X86BusSpace`/`BusSpaceHandle` on amd64, `&'static BusSpace` on arm64).

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
load addresses, DTB/RSDP pointers, command line; it lives in `sys/machine/bootinfo.rs` so the
machine traits can name it) → `boothowto` from the command line (`BootInfo::boothowto`, the
`boot(8)` letters `-a -c -d -s` as arm64's `initarm` parses them) →
`machine::Machine::early_init(&BootInfo)` (OpenBSD's `init_x86_64` / `initarm` as far as they are
ported: the message buffer, `consinit()`, and `db_enter()` for `boot -d`) → the `bsd: booted on`
banner → `kern::init_main::main` (OpenBSD's `main()` in the C's order; every step whose subsystem
is not here yet reports itself with `unported!`). Under feature `qemu`, `main` ends with the success
exit where proc0 would go to sleep.

A panic anywhere (`panic!` is `kern::subr_prf::panic` through the crate's panic handler) prints
`panic: <message>` through `db_printf`, a frame-pointer stack trace (`db_stack_dump` →
`machine::DbMachdep::db_stack_trace_print`, addresses only; `cargo xtask symbolize --arch A` names
them offline from the ELF symbol table) and reaches `reboot` → `machine::Cpu::boot`, which, cold,
halts; under feature `qemu` the "press any key" wait is the failure exit (status 35).

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
| `alloc` | — | `extern crate alloc` and the `GlobalAlloc` over `malloc(9)`; default since M3 |
| `diagnostic` | `option DIAGNOSTIC` | `kassert!` active |
| `debug` | `option DEBUG` | `kdassert!` active |
| `kmemstats` | `option KMEMSTATS` | `malloc(9)` statistics and per-type limits |
| `pool_debug` | `option POOL_DEBUG` | `pool_debug = 1` (poisoning, once `subr_poison.c` is here) |
| `qemu` | — | QEMU-only exits (`isa-debug-exit`, semihosting), the boot self-tests |

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
- Console attach before autoconfiguration exists (M2 to M4): `consinit()` attaches `com(4)` at
  `CONADDR` (amd64, `consinit.rs`) directly instead of `cninit()`'s `constab[]` walk; arm64
  finds its PL011 in the device tree since M4 (`pluart_init_cons`). On arm64, `initarm` installs a one-block identity map of the first GiB in
  `TTBR0_EL1` with Device-nGnRnE attributes, because the Limine protocol maps RAM but not devices;
  `bus_space_map` is the identity inside it until `pmap` maps devices (M3, page tables).
- `delay(9)` before the clocks: amd64 polls the i8254 (`isa/clock.rs`, as OpenBSD does before the
  TSC is calibrated); arm64 uses `intr.c`'s `arm_dflt_delay` until `agtimer` attaches (M4).
- ddb-lite: `db_enter()` is a breakpoint trap (`int3`, `brk #0xf000`) that lands in `db_ktrap`
  and `db_trap`, which print `Stopped at <pc>` and the stack trace from `ddb_regs` and then
  return, as the `c` command would, because there is no command loop (`db_command.c`,
  `db_run.c`). A panic prints its trace through `db_stack_dump`. `db_panic` therefore defaults
  to 0: a fatal trap is printed by `kerntrap`/`do_el1h_sync` and panics instead of entering a
  debugger that could not be left. No symbols in the kernel yet (`db_sym.c`): traces are
  addresses, symbolised by `xtask symbolize`.
- Traps (M4, part a): the entry stubs are OpenBSD's `vector.S`/`locore.S` and `exception.S`,
  kept as `.S` files and included by `global_asm!` with the `assym.h` symbols (frame offsets,
  selectors, trap numbers) passed as `const` placeholders. amd64 builds its GDT, TSS and IDT in
  `init_x86_64` (the IDT is a static page; `cpu_init_msrs` runs first thing because there is no
  `locore0.S`), NMI and double fault take `alltraps` on their IST stacks (the `calltrap_specstk`
  path exists for user-mode GS/CR3, M6), and `alltraps_kern` does not re-enable interrupts until
  the interrupt stubs exist. arm64's `initarm` switches to `SP_EL1` (Limine enters with
  `SPSel = 0`, whose vectors are empty, as in C), sets `tpidr_el1` and `VBAR_EL1` itself; `x18`
  is a general register here, so the EL1 paths of `exception.S` save and restore it instead of
  keeping `curcpu()` in it, and `do_el1h_sync` keeps interrupts masked. Without processes every
  kernel page fault or data abort is fatal (`kpageflttrap` returns 0 when `curproc` is NULL, as
  in C; `kdata_abort` has no `pcb_onfault` and `uvm_fault` is reported), which is what the
  `selftest=trap` boot of `smoke` asserts on both archs.
- Interrupts (M4, part b1, amd64): `spl(9)` is OpenBSD's: `splraise`/`spllower` in `intr.c`,
  `Xspllower`/`Xdoreti` in `spl.S`, the per-source masks in `cpu_info`, the `INTRSTUB` stubs of
  `vector.S` for the sixteen legacy IRQs and the MI soft interrupts (`kern_softintr.c`, the
  `Xsoft*` stubs). What autoconfiguration would do is done by `cpu_configure` directly until
  `config_rootfound` exists (M5): `lapic_boot_init` at the architectural LAPIC base (the MADT
  and MP tables are M5), `cpu_intr_init`, `intr_enable`. `lapic_set_lvt` programs LINT0 as
  ExtINT and LINT1 as NMI, the MP default configuration, because the firmware leaves LINT0
  masked and there are no tables to read it from; the IOAPIC stays off, so the 8259 is the
  PIC. The mutex is the uniprocessor one (`kern_lock.c`), `evcount` has no per-CPU counters
  yet. The console's receive interrupt is armed by the machine (`Console::cn_rx_intr_establish`,
  what `com_isa`'s attach does) for the `selftest=uart` boot, which types a line on the serial
  console and expects it echoed through the hard handler, `softintr_schedule` and the soft
  handler.
- Interrupts (M4, part b2, arm64): the device tree is the one Limine hands over (`fdt.c`
  parses it in place); QEMU `virt` boots with `acpi=off`, because EDK2 installs the device
  tree only when it does not publish ACPI tables, and OpenBSD arm64 needs the tree. The
  console is found through `/chosen` (`pluart_init_cons`), which retires the fixed PL011
  address. `cpu_configure` pre-registers the interrupt controllers (`arm_intr_init_fdt`) and
  attaches the GICv2 (`ampintc`) from fdt attach arguments it builds as `simplebus` would;
  `ampintc` then owns `spl` through `arm_set_intr_handler`. `do_el1h_sync` enables interrupts
  as the C does. The console's receive interrupt goes through `arm_intr_establish_fdt`, so the
  `selftest=uart` boot exercises the same path on arm64 as on amd64.
- Clocks (M5-a, part 1): the time code is OpenBSD's (`kern_tc.c` over the timehands ring,
  `kern_clockintr.c`'s per-CPU queue, `kern_timeout.c`'s timing wheel, `kern_clock.c`), reached
  from the machine through the `Cpu` trait's `CpuInfo`/`ClockFrame` associated types and
  accessors. `main` brings up the wheel, the clock queue and the four per-CPU clock interrupts
  (`sched_init_cpu`'s binds); amd64 starts the i8254, calibrates the LAPIC timer against it
  (`lapic_calibrate_timer`, as the boot CPU's `cpu_attach` does) and has the timer stub
  installed, but `initclocks` stays reported until the arm64 generic timer (`agtimer`) lands,
  so no clock interrupt runs yet on either arch. The host double owns a `cpu_info` of its own
  so the clock queue and the wheel are unit-tested over the dummy timecounter.
- `unported!("name")` (`sys/kern/unported.rs`) marks every call into a subsystem that is not here
  yet: it prints once per site and yields `ENOSYS`. The serial transcript of a boot is therefore an
  honest list of what the kernel skipped.
- Physical memory and the direct map (M3): the memory handed to `uvm_page_physload` is the boot
  protocol's usable regions (already without the kernel, the firmware and the bootloader's data),
  so the BIOS/EFI map walks, `avail_end`, the ISA hole and arm64's `memreg_*` bookkeeping have
  nothing to do. Both pmaps use the bootloader's higher-half direct map (`BootInfo::hhdm_offset`)
  as `__HAVE_PMAP_DIRECT` until the kernel owns its page tables: `pmap_direct_base` is that
  offset, `pmap_bootstrap` does not build the direct map's tables, and `virtual_avail` on amd64
  starts above the direct map when Limine places it at `VM_MIN_KERNEL_ADDRESS`. OpenBSD arm64 has
  no direct map and no `PMAP_STEAL_MEMORY` (it uses `pmap_steal_avail` and maps page by page);
  here it has both, so `uvm_pageboot_alloc` works before any `pmap_kenter_pa` exists. The
  `vm_physmem[]` half of amd64's `pmap_steal_memory` is `uvm_page_physsteal` (`uvm/uvm_page.rs`),
  shared by amd64, arm64 and the host double instead of being written three times.
- Kernel page tables (M3, part 2): both kernels keep running on the bootloader's tables and
  extend them. amd64 adopts the PML4 in `CR3` as `pmap_kernel()->pm_pdir`, installs the recursive
  mapping in slot 255 itself (the C's `locore0.S` does) and counts the kernel's PTPs from
  `virtual_avail` (above the direct map, which shares PML4 slot 256); `pmap_alloc_level` keeps the
  page-table pages the bootloader already installed. arm64 copies the bootloader's level-0 table
  and the level-1 table of the kernel's slot into `pmapvp0`/`pmapvp1` so the vp shadow exists,
  switches `TTBR1_EL1` to the copy and fills `MAIR_EL1` indices 2 to 4 around the bootloader's
  0 (write-back) and 1 (device); the kernel pmap is four-level where OpenBSD's is three-level,
  and `pmap_growkernel` populates the first GiB that the C's `pmap_bootstrap` pre-allocates.
  The level-2/3 tables of the kernel image and of the direct map stay the bootloader's on both
  archs. `pg_nx` comes from `EFER.NXE` as the bootloader left it. `uvm_km_init` only records the
  kernel map's bounds until `uvm_map.c`; `kern/selftest.rs` (feature `qemu`) maps a page there
  at boot and `smoke` asserts `selftest: pmap kernel mapping ok` on both archs.
- Kernel allocators (M3, part 3): `km_alloc` has no `kernel_map`/`kmem_map` yet (`uvm_map.c`
  is M6), so every request is served physically contiguous through the direct map, which the C
  does only for single pages and single segments; `kmem_map` is therefore the direct map and
  `kmemusage` has one entry per loaded page frame. `pool(9)` and `malloc(9)` are ported on top
  with their locks reduced to assertion flags (M5), no sleeping (`PR_WAITOK`/`M_WAITOK` fail
  where the C would wait), no idle-page timestamps (`getnsecuptime` is in `kern_tc.c`, whose
  beerware licence needs the user's decision) and the freelist poison (`subr_poison.c`) reported.
  `dev/rnd.rs` is a placeholder stream (SplitMix64, constant seed, NOT random) behind
  `arc4random`, which pools and `XSIMPLEQ` need for their cookies, until the entropy pool and
  ChaCha20 land (M5). `kern/rust_alloc.rs` is the Rust `GlobalAlloc` over `malloc(9)`
  (`M_TEMP`, `M_NOWAIT`); feature `alloc` is on by default. `physmem` lives in `sys/systm.rs`
  (the C defines it per arch) and `<machine/intr.h>`'s `IPL_*` are the `machine::Intr` contract.
- `uvmexp` is a static of atomics (exported under its C name so the amd64 interrupt stubs can
  count `V_INTR`) and the page-queue locks (`uvm_lock_pageq`, `uvm_lock_fpageq`) are no-ops
  until the pools and uvm take the mutex (M5): the boot CPU is alone. `wakeup` and `uvm_wait` report
  themselves unported, so a `UVM_PLA_WAITOK` allocation that cannot be met fails with `ENOMEM`
  instead of sleeping.
- Licences: `ddb/` and the `db_*` arch files carry the Mach licence (Carnegie Mellon);
  `dev/ic/comvar.h` and amd64 `include/bus.h` have a BSD block with the 4-clause advertising
  clause. Both were accepted by the user at M2 (`.claude/rules/scope-and-stubs.md`). A translation
  is still a derivative work, so each ported file keeps its original licence block whatever the
  language; a licence outside the list is routed around, never rewritten.

Every file-level deviation is in that file's `//! ## Deviations` list and in `ports.toml` `notes`.

## Testing architecture

See `.claude/rules/testing.md`. Host tests exist because of `arch/host`. QEMU smoke tests exist
because `custom_test_frameworks` is unstable. Reference-backed tests exist because constants copied
by hand drift.
