# Status

Milestone: **M2 done; M3 next**. Updated: 2026-10-02.

Done:
- M2 closed: `printf(9)`/`panic(9)`/`log(9)` on `core::fmt` (`kern/subr_prf.rs`), the message
  buffer, the console framework with `com(4)` (amd64, `bus_space` port I/O) and `pluart(4)`
  (arm64, MMIO), `main()`'s ordered skeleton with every missing step reported by `unported!`,
  ddb-lite (`db_output.rs`, per-arch `db_trace`/`db_enter`), `boot(9)`/`delay(9)`, the
  `machine::{bus,cons,db_machdep}` contracts, `xtask symbolize`, a four-boot `just smoke`
  (plain boot → status 33; `boot -d` → `panic: db_enter` + stack trace → status 35).
- M1 closed: `queue.h`, `tree.h` and `kern/subr_tree.c` as in-house intrusive structures; the
  `sys/sys` base headers; libkern; `just test-ref` cross-checks constants against the C headers.
- M0: both archs boot under EDK2 and Limine 12.9.1, protocol in `sys/stand/limine.rs`, no crate.

Next:
- M3: Limine memmap → `uvm_page.c`, `uvm_km.c` subset, per-arch `pmap.c` subset over the HHDM,
  `subr_pool.c`, `kern_malloc.c`, `#[global_allocator]`; it also replaces the static message buffer
  area, arm64's bootstrap device map and amd64's unported memory-space `bus_space_map`.
- Carried from M2 into M4: `constab`/`cninit` (amd64 `conf.c`), `pluart_fdt.c` and `agtimer.c`
  (arm64), `db_access.c`/`db_sym.c` (in-kernel symbols), `db_ktrap` so `db_enter` is a real trap.
- `arc4random(9)` for `XSIMPLEQ_INIT` and `label_t`/`cpu_info` for the `wip` headers come with M5.

Blockers:
- OpenBSD's `crc32` is zlib-licensed (`lib/libz/crc32.c`): `skipped: license: zlib`.
