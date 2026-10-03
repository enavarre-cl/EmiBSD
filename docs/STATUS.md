# Status

Milestone: **M7a done; M7b (the network) in progress**. Updated: 2026-10-03.

Done:
- M7a (exit met: `init: demand-zero bss ok` on both archs, user pages served by
  `uvm_fault`): `uvm_map.c`/`uvm_addr.c` (`a4f74ba`), `uvm_fault.c` (`e527558`), the traps
  calling it (`5d9550b`), pv lists, `pmap_protect`, arm64 R/M emulation (`492c991`), exec
  through `uvm_map` with the wired stand-ins retired (`5474591`), `uvm_mmap.c`, `uvm_unix.c`
  and the `plimit` layer of `kern_resource.c` (`65fbb24`).
- M7b so far: `kern_task.c` (task queues, `selftest=taskq`).

Next:
- M7b: mbufs, autoconf + mainbus, credentials (`kern_prot.c`), then PCI and virtio, `if.c`,
  ARP, ip/icmp, `if_vio.c`; exit: the kernel's ICMP echo to `10.0.2.2` is answered. Then
  the rest of M7+ (vfs, ffs, virtio-blk, signals, tty), then M8 (`docs/ROADMAP.md`).
- Widen the reference clone to `lib/ bin/ sbin/ usr.bin/ libexec/` (same pin, sparse) in its
  own `reference:` + `rules:` commit, before M8.

Blockers:
- None. `crc32` stays `skipped: license: zlib`; amd64's TSC timecounter (`tsc.c`) is deferred.

Decisions pending (the user's): the scope section (open until M13); the PC's CPU (Intel VMX
or AMD SVM) for vmm and M15; the exact Raspberry Pi 4 model; networking in M15.
