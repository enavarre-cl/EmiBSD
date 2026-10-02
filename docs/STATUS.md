# Status

Milestone: **M1 done; M2 next**. Updated: 2026-10-02.

Done:
- M1 closed: `tree.h` (SPLAY, RB, RBT) and `kern/subr_tree.c` ported on the `queue.rs` adapter
  pattern, one red-black algorithm for `RB_*` and `RBT_*`; 19 files ported, `just ci` green,
  `just test-ref` cross-checks errno, syslimits, param and the CRC table against the C headers.
- `queue.h` ported as our own intrusive lists; `intrusive-collections` dropped.
- M0: both archs boot under EDK2 and Limine 12.9.1 (`just smoke`, status 33), Limine protocol in
  `sys/stand/limine.rs` without a crate. Toolchain per `docs/SETUP.md`.
- Coding standard: section order per file, `<name>/tests.rs` past 50 lines, `machine/<header>.rs`.

Next:
- M2: `kern/subr_prf.c` (`kprintf!`, `panic`, `log(9)`), `dev/ic/comreg.h` + `com.c`,
  `dev/ic/pluart.c`, `kern/init_main.c`; the M0 early consoles and `stand::boot_main` retire;
  `panic!("test")` prints a backtrace on both archs and `smoke` asserts it.
- `arc4random(9)` for `XSIMPLEQ_INIT` and `label_t`/`cpu_info` for the `wip` headers come with M5.

Blockers:
- None. OpenBSD's `crc32` is zlib-licensed (`lib/libz/crc32.c`): `skipped: license: zlib`.
