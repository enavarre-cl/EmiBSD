# Roadmap

OpenBSD `sys/` is thousands of C files. Even the core (`kern` + `uvm` + `sys` + `libkern` + two
arch directories + a handful of drivers) is several hundred thousand lines. M0 to M6 is realistically a
12 to 24 month hobby arc; the full kernel is multi-year with no fixed end. This roadmap exists so
that progress is **measurable** (`cargo xtask ports status`, `just smoke`), not to promise dates.

Every milestone has a mechanical exit criterion: a command that passes or a serial line that appears.
Editing a milestone must keep that property.

| Milestone | Scope (OpenBSD files) | Exit criterion |
|---|---|---|
| **M0 Toolchain & boot** | No ports. `stand/limine.rs` (protocol structs from `PROTOCOL.md`, base revision 6, no crate), `machine/{cpu,cons}.rs` (`Cpu`, `Exit`, `Console`), polled early consoles (amd64 COM1 at `0x3f8` via `out`; arm64 PL011 at `0x0900_0000` MMIO, hardcoded with `TODO(M4)`), linker scripts, `xtask image/qemu/smoke`, docs and rules | `just ci` green; both archs print `bsd: booted on <arch>` and exit QEMU with the success code |
| **M1 libkern + sys/sys** | `sys/sys/{types,_types,errno,syslimits,param}.h`; `machine/{param,_types}.h` for both archs behind `machine::MachineParam`; `queue.h`/`tree.h` as adapters in `sys/sys/queue.rs`, `tree.rs`; libkern `strlcpy strlcat strnlen crc32c timingsafe_bcmp explicit_bzero` (OpenBSD's `crc32` is zlib code in `lib/libz`, skipped) | `just test` has ≥ 1 test per function; the reference-backed errno test passes (`just test-ref`) |
| **M2 console, printf, panic, ddb-lite** | `kern/subr_prf.c` (`kprintf!`, `panic`, log levels), `dev/ic/com.c` and `dev/ic/pluart.c` polled paths, `kern/init_main.c` skeleton with ordered init, frame-pointer backtrace printed as addresses (symbolised offline by `xtask symbolize`) | `panic!("test")` prints `panic: test` plus a backtrace on both archs; smoke asserts it and a non-zero exit |
| **M3 Physical memory + uvm basics** | Limine memmap → `uvm/uvm_page.c` (page array, free lists), `uvm_km.c` subset, per-arch `pmap.c` subset (`pmap_bootstrap`, `pmap_kenter_pa`, `pmap_kremove`, `pmap_extract`) via HHDM, `kern/subr_pool.c`, `kern/kern_malloc.c`, `#[global_allocator]`; `alloc` becomes a default feature | alloc/free stress in QEMU on both archs; host tests for the page allocator with a synthetic memmap |
| **M4 Traps & interrupts** | amd64: `machdep.c`, `gdt.c`, `vector.S`, `trap.c`, LAPIC (+IOAPIC); arm64: `exception.S`, `trap.c`, GIC (`arch/arm64/dev/agintc.c` or `ampintc.c`), `dev/ofw/fdt.c` to locate UART/GIC/timer; real `spl(9)` per arch | a deliberate bad access prints the OpenBSD-format trap message then panics; UART RX by interrupt echoes on both archs |
| **M5 Timers, scheduler, proc** | arch clocks (LAPIC timer / ARM generic timer), `kern_clock.c`, `kern_tc.c`, `kern_timeout.c`, `sys/sys/proc.h` → `Proc`/`Process`, `kern_proc.c`, `kern_synch.c` (tsleep/wakeup), `sched_bsd.c`, `kern_sched.c`, `cpu_switchto` per arch, `kern_kthread.c`, idle | two kthreads ping-pong via tsleep/wakeup; uptime ticks at `hz` on both archs |
| **M6 Syscalls + minimal init** | `syscalls.master` → `xtask gen-syscalls` → `sys/sys/syscall.rs` + `init_sysent`, per-arch syscall entry, `exec_elf.c`, user `uvm_map`/`uvm_fault`, `sys_generic.c` (`write`), `kern_exit.c`; a freestanding Rust `init` loaded as a Limine module | `init` prints via `sys_write` and exits; the kernel logs `init exited with status 0` on both archs |
| **M7+ (open-ended)** | `vfs_*` + ramdisk, virtio-blk, ufs/ffs, signals, tty/pty, SMP (Limine MP request + `kern_lock.c`), virtio-net + `net/if.c` + `netinet` (ping), pledge/unveil, real `ddb` | each gets its own exit criterion when scheduled |

## Adding or changing a milestone

State the OpenBSD files it claims (they become `todo` entries in `ports.toml`) and one mechanical
exit criterion. Update `docs/STATUS.md` when a milestone starts and when it closes.
