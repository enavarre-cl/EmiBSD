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
| `sys/net/if.h`, `sys/netinet/in.h` (Rust keywords) | `sys/net/if_.rs`, `sys/netinet/in_.rs` (`docs/C_TO_RUST.md`) |
| `sys/arch/*/stand/`, `boot(8)`, `efiboot` | `sys/stand/` (Limine glue) until M14, when the user decided (2026-10-03) to port `boot(8)`/`efiboot` and `sys/lib/libsa/`; Limine is scaffolding until `boot(8)` boots the same kernel in QEMU |
| `sys/conf/`, `config(8)`, Makefiles, `newvers.sh` | Cargo features and `tools/xtask`; `ioconf.c` is `sys/arch/<arch>/conf/ioconf.rs`, by hand (M7b); not ported |

Types live where the **header** is; functions live where the **`.c`** is. Rust allows inherent
`impl` blocks in any module of the defining crate, which is exactly the header/implementation split.

## The `machine` contract

`sys/machine/<header>.rs` holds the traits standing in for `<machine/*.h>` and `cpufunc.h`, one
module per OpenBSD header (`param.rs`, `cpu.rs` with `boot(9)`, `delay(9)` and, since M5, `curcpu()` with the
`cpu_info` accessors the clock code needs, `cons.rs` for
`consinit()`, `bus.rs` for `bus_space(9)`, `db_machdep.rs` for what `ddb` needs; later `pmap.rs`,
`intr.rs`, ...; `autoconf.rs` is what `ioconf.c` and the machine's `autoconf.c` give
`subr_autoconf.c`; `signal.rs` is `<machine/signal.h>` plus `sendsig`, `sys_sigreturn` and the
signal trampoline), all re-exported from `sys/machine/mod.rs`, which also re-exports
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
- Cargo features and `xtask` instead of `config(8)`, Makefiles and `newvers.sh`; the
  autoconfiguration tables `config(8)` generates are written by hand ("Autoconfiguration",
  below).
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
  `Xsoft*` stubs). What autoconfiguration would do was done by `cpu_configure` directly until
  `config_rootfound` existed (M7b, "Autoconfiguration" below): `lapic_boot_init` at the
  architectural LAPIC base (the MADT and MP tables are not ported), `cpu_intr_init`,
  `intr_enable`. `lapic_set_lvt` programs LINT0 as
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
  address. `mainbus_attach` pre-registers the interrupt controllers (`arm_intr_init_fdt`)
  and attaches the GICv2 (`ampintc`) from the device tree (built by hand in `cpu_configure`
  until M7b); `ampintc` then owns `spl` through `arm_set_intr_handler`. `do_el1h_sync` enables interrupts
  as the C does. The console's receive interrupt goes through `arm_intr_establish_fdt`, so the
  `selftest=uart` boot exercises the same path on arm64 as on amd64.
- Clocks (M5-a): the time code is OpenBSD's (`kern_tc.c` over the timehands ring,
  `kern_clockintr.c`'s per-CPU queue, `kern_timeout.c`'s timing wheel, `kern_clock.c`), reached
  from the machine through the `Cpu` trait's `CpuInfo`/`ClockFrame` associated types and
  accessors. `main` brings up the wheel, the clock queue, the four per-CPU clock interrupts
  (`sched_init_cpu`'s binds) and `initclocks`. amd64 starts the i8254, calibrates the LAPIC
  timer against it (`lapic_calibrate_timer`, as the boot CPU's `cpu_attach` does) and drives
  `clockintr_dispatch` from `Xintr_lapic_ltimer`; the i8254 is the timecounter (the TSC one,
  `tsc.c`, is not ported). arm64 attaches `agtimer` from the device tree (through mainbus
  since M7b) and takes the virtual timer's PPI through `ampintc`. The `selftest=clock` boot waits for
  `hz` hardclocks and a `timeout(9)`. The host double owns a `cpu_info` of its own so the
  clock queue and the wheel are unit-tested over the dummy timecounter.
- Processes (M5-b, part 1): `struct proc`/`struct process` are OpenBSD's with the members
  the scheduler and the kernel threads use; the machine-dependent parts (`mdproc`, `pcb`)
  come through `machine::proc` (associated types with associated-constant initialisers, so
  `proc0` is a `static`). `proc0paddr` is a static u-area per arch (`Uarea`, `USPACE` bytes,
  page aligned, as `locore` reserves it in C): proc0's kernel stack stays the boot stack
  Limine gave us, its pcb and the trap frame `cpu_fork` copies live in the static. `main`
  sets `curproc` first and builds process 0 as `init_main.c` does.
- Processes (M5-b, part 2, the scheduler): the sleep queues, `mi_switch`, the run queues,
  `fork1` and the kernel threads are OpenBSD's, single-CPU (`MULTIPROCESSOR` paths such as
  stealing, `SPCF_SHOULDHALT` and the barrier task are not configured, `sched_choosecpu` is
  `curcpu()`). The machine contract gained `cpu_switchto`, `cpu_fork`, `clear_resched`,
  `cpu_unidle`, the idle hooks, `cpu_info_foreach` and the mutex nesting counter. The
  context switches are the kernel-thread subsets of `locore.S`/`cpuswitch.S`: stack
  pointers, `curproc`/`curpcb`/`p_cpu`/`p_stat` and, on amd64, `%cr3`; the FPU/xstate and
  user segment handling, the Meltdown CR3s, retguard and the RSB refill come with user
  mode. `proc_trampoline` hands the thread function and its argument to a Rust
  `proc_trampoline_run` instead of calling the function itself (Rust `fn` pointers have no C
  calling convention); the syscall return path after it is M6. Every thread runs on the
  kernel pmap until vmspaces exist: amd64 `pmap_activate` loads it, arm64 `pmap_setttb`
  records `ci_curpm` and leaves `TTBR0_EL1` (still the bootloader's) alone. `uvm_uarea_alloc`
  hands out `USPACE` blocks from the direct map without the guard page (`km_alloc` cannot
  punch a hole in the direct map; the guard returns with `kernel_map`). `cold` and `safepri`
  are `sys/systm.rs` statics like `physmem`. The `selftest=kthread` boot runs two kernel
  threads passing a turn with `msleep`/`wakeup` through the run queues and the idle thread.
- System calls (M6-a): the tables are generated, as in C, but by `cargo xtask gen-syscalls`
  from `syscalls.master` instead of `makesyscalls.sh`; every syscall the tree does not define
  is `sys_nosys` in `init_sysent.rs`, and the generator's `--check` keeps the four files
  current in `just ci`. The entry paths are OpenBSD's: amd64 `Xsyscall` (`syscall`
  instruction, `MSR_LSTAR`) building the trap frame on `ci_kern_rsp`, `syscall()`,
  `mi_syscall` and the AST loop before `sysretq`; arm64 `handle_el0_sync` → `do_el0_sync` →
  `svc_handler`, `do_ast` and `eret`. Not here: the Meltdown U-K page and `Xsyscall_meltdown`,
  the xstate/FS.base restores and the Spectre code patches on amd64; the trampoline vectors
  (`trampoline.S`) on arm64, so `VBAR_EL1` keeps the kernel vectors; `pin_check` accepts
  every call site until `exec` reads `PT_OPENBSD_SYSCALLS`. `copyin(9)` is each arch's
  `copy.S` behind the `machine::copy` contract, with `pcb_onfault` recovery in both page fault
  handlers (amd64 validates it against the `.nofault` table the linker script collects);
  amd64 runs without SMAP's `stac`/`clac` (no `codepatch`, `CR4.SMAP` not set).
- The first user program (M6-b): there is no filesystem, so `init` is a Limine module
  (`module_path: boot():/init` in `limine.conf`, which `cargo xtask image` adds when the
  `init` binary exists) that the boot glue hands over as `BootInfo::modules` and `start_init`
  will exec from memory. `init/` is a freestanding Rust crate (`#![no_std]`, static ELF at
  `0x400000`, raw `syscall`/`svc` with OpenBSD's carry-flag convention) built for the two
  bare targets by `just build-init-*`; it is not OpenBSD code and lives outside `sys/`.
- Process exit (M6-b): `kern_exit.c`'s `exit1`/`exit2`/`reaper`/`process_zap` are OpenBSD's
  with the pieces that need signals, file descriptors, limits, credentials or a vmspace
  reported; `initprocess` is null until `init` exists and process 0 adopts orphans meanwhile.
  The `selftest=kthread` threads now `kthread_exit` and proc0 checks the reaper freed them.
- User address spaces (M6-b, then M7a): M6 built `exec`'s segments from wired pages outside
  the entry tree; M7a-3b retired those stand-ins. `exec` now maps each segment with
  `uvm_map` (anonymous, copy-on-write) and copies the image bytes in with `copyout`, so the
  pages are faulted in by `uvm_fault`; with no vnode, `vmcmd_map_pagedvn` maps anonymous
  memory as `vmcmd_map_readvn` does (`sys/kern/exec_subr.rs`).
- Exec of a memory image (M6-b2): `kern_exec.c`'s `sys_execve` is ported from the point where
  the executable is in hand as `exec_image(p, name, image)`; `check_exec` runs the exec switch
  (`exec_elf_makecmds`, which requires the OpenBSD ELF note as the C does) without `namei`,
  the vmcmds (`exec_subr.c`) act on the image instead of a vnode, `copyargs` lays out an empty
  `argv`/`envp` (the boot flags come with a real `sys_execve`, M6-c), and `setregs` builds the
  user trap frame. `start_init` execs the `init` module and returns through
  `proc_trampoline` to the syscall exit path, exactly where a forked user thread would go.
- Signals (M7, `kern_sig.c`): the whole file is OpenBSD's, and the traps of both archs call
  its `trapsignal`. The machine half (`sendsig`, `sys_sigreturn`, the `sigcode` trampoline of
  each `locore.S`) is the `machine::MachineSignal` contract; `sys_sigreturn` is entered from
  the table through a forwarding `sys_sigreturn` in `kern_sig.rs`, because the syscall
  generator only scans `sys/kern` and `sys/uvm`. `exec_image` maps the trampoline with the
  C's `exec_sigcode_map` (one shared aobj, `PROT_EXEC`, immutable) and draws a new
  `ps_sigcookie`. amd64 has no FPU code yet (`fpu.c`): `sendsig` copies out the pcb's
  `fxsave`-sized area as it is and `sigreturn` copies it back without `xrstor`, so a handler
  shares the interrupted code's FPU/SSE registers. arm64's trampoline saves the `q`
  registers itself, which needs `fpu_load` (the first FP use of a thread traps): `fpu_save`
  and `fpu_load` are ported, SVE is reported. The kqueue notes, ptrace stops (the code is
  there; nothing sets `PS_TRACED`), core dumps (`vn_open` is reported, so no core is ever
  written) and `pledge_kill` are reported. The stand-in `init` checks `sigaction`, `kill`,
  delivery on the way back from a system call, `sigreturn`, `sigprocmask` and `sigpending`
  (`init: signals ok` in `smoke`).
- User pmaps (M6-b2): amd64 walks a user pmap's tables through the direct map
  (`pmap_get_ptp`, `pmap_enter`, `pmap_do_remove`) instead of borrowing its `%cr3` for the
  recursive mapping (`pmap_map_ptes`), has no pv entries yet and no `pmaps` list, and
  `pmap_pdp_ctor` copies the kernel's whole upper half of the PML4; `cpu_init_msrs` sets
  `EFER.SCE` (the C's `locore0.S` does). On arm64 the bootstrap device map lived in `TTBR0`
  (the lower half), which user pmaps now own: `pmap_init` initialises the pools, remaps the
  console into the kernel half (`pluartcn_remap`), switches `bus_space_map` to kernel-half
  mappings from the `vmmap` range (a 4 MiB window below `virtual_avail`, so device mappings
  never overlap what `kernel_map` hands out; the C takes them from `kernel_map` with
  `km_alloc(kv_any)`), sets `TCR_EL1.T0SZ` for `USER_SPACE_BITS` and points
  `TTBR0_EL1` at the empty table, as the C's `locore` and `pmap_init` do between them; user
  pmaps are three-level, their tables come from the same two-page allocator as the kernel's
  (no `pmap_vp_pool`), and ASIDs are an 8-bit bitmap without rollover. `init` is linked with
  `-z nobtcfi` so `setregs` leaves `pm_guarded` clear (no BTI landing pads yet).
- Autoconfiguration (M7b): `subr_autoconf.c` and `<sys/device.h>` are OpenBSD's and
  `cpu_configure` starts them with `config_rootfound("mainbus")` on both archs. `config(8)`
  is not ported: what it would generate into `ioconf.c` (`cfdata[]` with its locators and
  parent vectors, `cfroots[]`) is written by hand per architecture in
  `sys/arch/<arch>/conf/ioconf.rs`, following `config(8)`'s layout, for the GENERIC lines
  whose drivers exist: `mainbus0 at root` and `cpu0 at mainbus?` on amd64; `mainbus0 at
  root`, `ampintc* at fdt? early 1` and `agtimer* at fdt?` on arm64. The tables, `mainbus_cd`
  and `device_register` reach `subr_autoconf.rs` through `machine::autoconf`, so generic code
  never names an arch; the host double serves whatever table a test installs. A device that
  GENERIC configures but whose driver is not ported is reported with `unported!` where its bus
  would probe or attach it (amd64's `bios0`, `pci0`, `isa0`, ...); on arm64 every device-tree
  node without a driver prints OpenBSD's `"name" at mainbus0 not configured`. The counts
  `config(8)` writes into `<dev>.h` follow the tables: `NMPATH` is 0, the `hotplug(4)` calls
  are reported. Without ACPI or MP tables, amd64's mainbus attaches the boot CPU as
  `CPU_ROLE_SP`, as the C does on such a machine; `cpu_configure` keeps doing around
  `config_rootfound` what `acpimadt` and the boot processor's attach would add (the LAPIC
  base, `lapic_enable`, `lapic_set_lvt`, `lapic_calibrate_timer`). Adding a driver means its
  `cfattach`/`cfdriver` and one `Cfdata` row in each `ioconf.rs` that has it in GENERIC.
- File descriptors (M7b): `kern_descrip.c`, `<sys/file.h>`, `<sys/filedesc.h>` and the
  read/write/ioctl paths of `sys_generic.c` are OpenBSD's: process 0 gets `fdinit()`,
  `fork1` copies or shares the table, `exec` runs `fdprepforexec`, `exit1` runs `fdfree`,
  and every `read`/`write`/`ioctl` goes through `fd_getfile_mode` and the file's
  `fileops`. What needs the vfs, kqueues or pledge is reported (`VOP_ADVLOCK`,
  `VOP_PATHCONF`, `knote_fdclose`, `pledge_*`). Pipes (`sys_pipe.c`, `<sys/pipe.h>`) are
  not ported: their licence (John S. Dyson's) is outside the accepted list and waits for
  the user's decision.
- The console as a file (M7b, stand-in): in OpenBSD `init(8)` opens `/dev/console`, a
  vnode of the console's character device whose tty does the I/O. Without the vfs and
  the tty layer (M10), `start_init` installs `sys/dev/consfile.rs` instead: one `struct
  file` of type `DTYPE_CONSFILE` (127, outside OpenBSD's range) whose `fileops` write
  through `cnputc` and read a line through polled `cngetc` with echo, put at descriptors
  0, 1 and 2 of process 1 by `falloc`/`fdinsert`/`fdalloc`. It is not a tty (`F_ISATTY`
  and the `termios` ioctls answer `ENOTTY`), and it goes away when `init` can open
  `/dev/console`.
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
