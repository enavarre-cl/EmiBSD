# Status

Milestone: **M3 in progress (part 1 landed)**. Updated: 2026-10-02.

Done:
- M3 part 1: the physical page allocator (`uvm_page.c`, `uvm_pmemrange.c`, the `uvm/*.h` types,
  `uvm_init`) runs on both archs from the Limine memory map; `cpu_startup` prints `real mem` /
  `avail mem` and `just smoke` asserts both; the `machine::{vmparam,pmap}` contracts; per-arch
  `pmap.c` subsets over Limine's direct map (`pmap_bootstrap`, `pmap_steal_memory`, zero/copy
  page); host tests drive the allocator over synthetic segments (`uvm_pmemrange/tests.rs`).
- M2 closed: `printf(9)`/`panic(9)`/`log(9)`, message buffer, console framework with `com(4)`
  and `pluart(4)`, `main()`'s skeleton with `unported!` markers, ddb-lite, `xtask symbolize`.
- M1 closed: `queue.h`, `tree.h`, `subr_tree.c`, the `sys/sys` base headers, libkern, `test-ref`.
- M0: both archs boot under EDK2 and Limine 12.9.1, protocol in `sys/stand/limine.rs`, no crate.

Next:
- M3 part 2: kernel page tables (`pmap_kenter_pa`/`pmap_kremove`/`pmap_extract`/
  `pmap_growkernel` on both archs), `uvm_km.c` (`km_alloc`/`km_free`), `subr_pool.c`,
  `kern_malloc.c`, `#[global_allocator]`, the `alloc` feature on by default, an allocation
  stress line in `smoke`; then the static message buffer and arm64's bootstrap device map retire.
- Carried from M2 into M4: `constab`/`cninit`, `pluart_fdt.c`, `agtimer.c`, `db_access.c`/
  `db_sym.c`, `db_ktrap`. `wakeup`/`uvm_wait`/the page daemon and the uvm locks come with M5.

Blockers:
- OpenBSD's `crc32` is zlib-licensed (`lib/libz/crc32.c`): `skipped: license: zlib`.
