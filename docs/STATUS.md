# Status

Milestone: **M0 done; M1 nearly done** (`queue.h`/`tree.h` pending). Updated: 2026-10-02.

Done:
- M0: `just ci` green. `just smoke` boots both archs under EDK2 and Limine 12.9.1, sees
  `bsd: booted on <arch>` (amd64 in 1.7 s, arm64 in 5.7 s), prints the memory map and exits QEMU
  with status 33. The Limine protocol (base revision 6) is `sys/stand/limine.rs`, no crate.
- Toolchain installed per `docs/SETUP.md` (rustup 1.98.1, just, QEMU 11.1, Limine 12.9.1).
- Coding standard: fixed section order per file (`.claude/rules/rust-kernel.md`), tests longer
  than 50 lines in `<name>/tests.rs`, one `machine/<header>.rs` per `<machine/*.h>` header.
- `queue.h` ported as our own intrusive lists (six families, adapters, 13 tests);
  `intrusive-collections` dropped.
- Reference pinned at `3ce1f3f79392`. Ported: `sys/sys/{types,_types,errno,syslimits,param}.h`,
  `machine/{param,_types}.h` for both archs, `amd64/pio.h`, libkern `strlcpy strlcat strnlen
  crc32c timingsafe_bcmp explicit_bzero`. Wip subsets: `amd64/cpufunc.h`, `arm64/cpu.h`.

Next:
- Port `tree.h` (`RB_*`, `SPLAY_*`) with the `queue.rs` adapter pattern; that closes M1.
- M2: `kern/subr_prf.c` (`kprintf!`, `panic`), `dev/ic/comreg.h` + `com.c`, `dev/ic/pluart.c`,
  `kern/init_main.c`; the M0 early consoles and `stand::boot_main` retire.

Blockers:
- None for tooling. OpenBSD's `crc32` is zlib-licensed (`lib/libz/crc32.c`): `skipped: license: zlib`.
