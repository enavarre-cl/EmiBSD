# Status

Milestone: **M3 done (parts 1 to 3 landed); M4 next**. Updated: 2026-10-02.

Done:
- M3 part 3: `km_alloc`/`km_free` over the direct map, `pool(9)`, `malloc(9)`, the Rust
  `GlobalAlloc` over `malloc` (feature `alloc` on by default), a placeholder `arc4random`
  (`dev/rnd.rs`, not random yet), `machine::Intr` (`IPL_*`), host tests for pools and malloc over
  real memory, `selftest: malloc/pool stress ok` asserted by `smoke` on both archs.
- M3 part 2: kernel page tables on both archs over the bootloader's tables (amd64: recursive
  slot and `pmap_growkernel`; arm64: `pmapvp` shadow of the adopted tables), `pmap_kenter_pa`/
  `pmap_kremove`/`pmap_extract`, `uvm_km_init` bounds, `selftest: pmap kernel mapping ok`.
- M3 part 1: the physical page allocator (`uvm_page.c`, `uvm_pmemrange.c`, `uvm_init`) from the
  Limine memory map; `real mem`/`avail mem`; the `machine::{vmparam,pmap}` contracts.
- M2 closed: `printf(9)`/`panic(9)`, message buffer, `com(4)`/`pluart(4)`, `main()`'s skeleton,
  ddb-lite; M1: `queue.h`/`tree.h`, base headers, libkern; M0: both archs boot under Limine.

Next:
- M4: traps and interrupts (`trap.c`, `vector.S`/`exception.S`, `intr.c`, `spl(9)`, GIC and
  APIC/i8259, `db_ktrap`), `cpu_info`/`curcpu`, the device tree (`fdt`), `constab`/`cninit`;
  then arm64's bootstrap device map and the static message buffer retire.
- M5: `wakeup`/`tsleep`, the mutexes behind the pool/malloc/uvm locks, the real `arc4random`,
  `getnsecuptime`, the page daemon; M6: `uvm_map.c` (then `km_alloc` leaves the direct map).

Blockers:
- `kern_tc.c` is beerware-licensed (Poul-Henning Kamp): needs the user's decision
  (`getnsecuptime` is reported as unported meanwhile).
- OpenBSD's `crc32` is zlib-licensed (`lib/libz/crc32.c`): `skipped: license: zlib`.
