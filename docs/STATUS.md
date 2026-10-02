# Status

Milestone: **M3 in progress (parts 1 and 2 landed)**. Updated: 2026-10-02.

Done:
- M3 part 2: kernel page tables on both archs over the bootloader's tables (amd64: recursive
  slot, `pmap_growkernel`/`pmap_alloc_level`; arm64: `pmapvp` shadow of the adopted tables,
  `pmap_growkernel`), `pmap_kenter_pa`/`pmap_kremove`/`pmap_extract`, `uvm_km_init` bounds, a
  boot self-test (`kern/selftest.rs`, feature `qemu`) that `smoke` asserts on both archs.
- M3 part 1: the physical page allocator (`uvm_page.c`, `uvm_pmemrange.c`, the `uvm/*.h` types,
  `uvm_init`) runs on both archs from the Limine memory map; `cpu_startup` prints `real mem` /
  `avail mem`; the `machine::{vmparam,pmap}` contracts; host tests over synthetic segments.
- M2 closed: `printf(9)`/`panic(9)`/`log(9)`, message buffer, console framework with `com(4)`
  and `pluart(4)`, `main()`'s skeleton with `unported!` markers, ddb-lite, `xtask symbolize`.
- M1 closed: `queue.h`, `tree.h`, `subr_tree.c`, the `sys/sys` base headers, libkern, `test-ref`.
- M0: both archs boot under EDK2 and Limine 12.9.1, protocol in `sys/stand/limine.rs`, no crate.

Next:
- M3 part 3: `uvm_km.c` (`km_alloc`/`km_free` over the direct map until `uvm_map`),
  `subr_pool.c`, `kern_malloc.c`, `#[global_allocator]`, the `alloc` feature on by default, an
  allocation stress line in `smoke`; then the static message buffer and arm64's bootstrap device
  map retire (`bus_space_map` through `pmap_kenter_pa(PMAP_DEVICE)`).
- Carried from M2 into M4: `constab`/`cninit`, `pluart_fdt.c`, `agtimer.c`, `db_access.c`/
  `db_sym.c`, `db_ktrap`. `wakeup`/`uvm_wait`/the page daemon and the uvm locks come with M5.

Blockers:
- OpenBSD's `crc32` is zlib-licensed (`lib/libz/crc32.c`): `skipped: license: zlib`.
