# Status

Milestone: **M4 done** (traps and interrupts on both archs). Updated: 2026-10-02.
The user asked to stop at M4: M5 has not been started.

Done:
- M4-b2: arm64 `dev/ofw/fdt.c` over the DTB Limine passes (QEMU `virt,acpi=off`), the
  `machine::fdt` contract, `pluart_init_cons` (console from `/chosen`, no fixed address),
  `arm_intr_*_fdt` registration, the GICv2 driver `ampintc`, the PL011 receive interrupt;
  `do_el1h_sync` enables interrupts. `smoke`'s `selftest=uart` echoes `hello` on both archs.
- M4-b1: amd64 `spl(9)` (`intr.c`, `spl.S`, `i8259.c`, legacy `INTRSTUB`s, `Xsoft*`, LAPIC
  subset), MI soft interrupts, UP mutex, `evcount`, `cpu_configure` on both archs.
- M4-a: real traps on both archs, `cpu_info`/`curcpu()`, ddb-lite `db_ktrap`/`ddb_regs`,
  `selftest=trap` (OpenBSD's fatal trap message + panic).
- M3: page allocator, kernel page tables, `km_alloc`/pools/`malloc`/`GlobalAlloc`.
  M2: printf/panic, console, `main()`, ddb-lite. M1: queues, trees, headers, libkern. M0: boot.

Next (M5, not started):
- Clocks (`agtimer`, LAPIC timer, clockintr, `kern_clock`/`kern_timeout`), autoconf
  (`config_rootfound`, `mainbus`/`simplebus`, `cpu_attach`), IOAPIC/MADT, proc0 and the
  scheduler, `tsleep`/`wakeup`, the real `arc4random`, `constab`/`cninit`.
- Carried: `db_run.c`/`db_command.c`, `comintr`/`pluart_intr` on a tty (M7), the user-mode
  trap paths and trampolines (M6), `agintc` (GICv3) for other machines.

Blockers:
- `kern_tc.c` is beerware: `status = "todo"`, `getnsecuptime` reported; needs the user's
  decision (M5 needs timecounters). `crc32` is zlib-licensed: `skipped: license: zlib`.
