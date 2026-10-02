# Status

Milestone: **M3 done; M4 part a (traps) done; M4 part b (interrupts) next**. Updated: 2026-10-02.

Done:
- M4-a: real traps on both archs. amd64: GDT/TSS/IDT built in `init_x86_64`, `vector.S` stubs
  (`Xtrap00..1f`, `alltraps`), `kerntrap`/`trap_print`, `cpu_info`/`curcpu()` via `%gs`.
  arm64: `exception.S` vectors (EL1h), `do_el1h_sync`/`kdata_abort`, `cpu_info` via
  `tpidr_el1`, `SP_EL1` switch in `initarm`. ddb-lite: `db_enter` is `int3`/`brk`, `db_ktrap`
  saves `ddb_regs`, `db_trap` prints `Stopped at` + trace and continues. `smoke` runs three
  boots per arch: plain (33), `-d` (33), `selftest=trap` (fatal trap message + panic, 35).
- M3: page allocator, kernel page tables over the bootloader's tables, `km_alloc`, pools,
  `malloc(9)`, `GlobalAlloc`; `dev/rnd.rs` is a placeholder PRNG (NOT random).
- M2: `printf`/`panic`/`log`, message buffer, console framework (`com`, `pluart`), `main()`
  skeleton, ddb-lite, `boot(9)`/`delay(9)`. M1: `queue.h`/`tree.h`, base headers, libkern.
  M0: Limine boot on both archs.

Next:
- M4-b: `spl(9)` and the interrupt controllers (amd64 `intr.c`, i8259/LAPIC/IOAPIC; arm64
  `intr.c` `arm_intr_func`, `ampintc`/`agintc` GICv2, `agtimer`, `dev/ofw/fdt.c`), the
  `Xintr_*` stubs, `sti` in `alltraps_kern`, `intr_enable` in `do_el1h_sync`, UART RX by
  interrupt echoing on both archs (the M4 exit criterion). Then stop (user: "para en M4").
- Carried: `constab`/`cninit`, `pluart_fdt.c`, `db_access.c`/`db_sym.c`, `db_run.c`/
  `db_command.c` (the real debugger loop), user-mode trap paths (`usertrap`, `do_el0_sync`,
  `calltrap_specstk`, trampolines) with M6.

Blockers:
- `kern_tc.c` is beerware: `status = "todo"`, `getnsecuptime` reported; needs the user's
  decision. `crc32` is zlib-licensed: `skipped: license: zlib`.
