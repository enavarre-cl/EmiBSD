# Status

Milestone: **M3 done; M4-a (traps) and M4-b1 (amd64 interrupts) done; M4-b2 (arm64) next**.
Updated: 2026-10-02.

Done:
- M4-b1: `spl(9)` on amd64 (`intr.c`, `spl.S`, `i8259.c`, the `INTRSTUB` legacy stubs and
  `Xsoft*` of `vector.S`, `isa_intr_establish`, the LAPIC subset: `lapic_map`/`enable`/
  `set_lvt`), the MI soft interrupts (`kern_softintr.c`), the UP mutex (`kern_lock.c`),
  `evcount`, `cpu_configure` on both archs, the `machine::intr` spl contract and
  `Console::cn_rx_intr_establish`. `smoke` boots amd64 with `selftest=uart`, types `hello` on
  the serial console and sees `selftest: uart echo: hello` through hard + soft interrupt.
  arm64 has the spl machinery and the default handlers; its GIC is next.
- M4-a: real traps on both archs, `cpu_info`/`curcpu()`, ddb-lite `db_ktrap`/`ddb_regs`,
  `selftest=trap` (fatal trap message + panic). M3: page allocator, kernel page tables,
  `km_alloc`/pools/`malloc`/`GlobalAlloc`. M2: printf/panic, console, `main()`, ddb-lite.
  M1: `queue.h`/`tree.h`, base headers, libkern. M0: Limine boot on both archs.

Next:
- M4-b2: arm64 `dev/ofw/fdt.c` (the device tree Limine hands over), `ampintc` (GICv2),
  `arm_intr_register_fdt`/`arm_intr_establish_fdt`, `pluart`'s receive interrupt, `intr_enable`
  in `do_el1h_sync`, the arm64 `selftest=uart` boot. That closes M4; stop there (user).
- Carried to M5: `agtimer`/LAPIC timer and clockintr, IOAPIC/MADT/MP tables, `comintr`/
  `comsoft` and `pluart_intr` on a tty (M7), `constab`/`cninit`, `db_run.c`/`db_command.c`,
  autoconf (`config_rootfound`, `cpu_attach`), the user-mode trap paths (M6).

Blockers:
- `kern_tc.c` is beerware: `status = "todo"`, `getnsecuptime` reported; needs the user's
  decision. `crc32` is zlib-licensed: `skipped: license: zlib`.
