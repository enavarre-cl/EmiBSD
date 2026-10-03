# Status

Milestone: **M7a and M7b done; M7+ toward M8 in progress**. Updated: 2026-10-03.

Done:
- M7a (exit met: `init: demand-zero bss ok` on both archs, user pages served by
  `uvm_fault`): `uvm_map.c`/`uvm_addr.c` (`a4f74ba`), `uvm_fault.c` (`e527558`), the traps
  calling it (`5d9550b`), pv lists, `pmap_protect`, arm64 R/M emulation (`492c991`), exec
  through `uvm_map` with the wired stand-ins retired (`5474591`), `uvm_mmap.c`, `uvm_unix.c`
  and the `plimit` layer of `kern_resource.c` (`7253ef3`).
- M7b (exit met: `selftest: ping 10.0.2.2: echo reply received` on both archs): task queues,
  mbufs, autoconf, bus_dma + PCI (amd64), `if.c`/`ifq.c`/ether, virtio (pci, mmio) + `vio`,
  routing (art, rtable, route), ARP, IPv4, ICMP. For M8: credentials, limits, signals, the
  file table, `sysctl` (EmiBSD 7.8), the vfs core.

- M8 so far: buffer cache, vnode pager, syncer/lockf, pipes (`362d1a9`), the device switch,
  tty layer and console tty (`befd60d`); the userland build links libc, libcompiler_rt,
  `init`, `ksh`, `echo` and `ls` on both archs (`9326395`, `6ee7cd2`).

Next:
- M8's kernel side: ufs/ffs and a real `execve` + process syscalls (two agents running),
  then an `rd` ramdisk root with an ffs image built by `xtask`, and OpenBSD's `init(8)`/`ksh`
  booting from it to a prompt that `just smoke` drives (`docs/ROADMAP.md`).

Blockers:
- `crc32` stays `skipped: license: zlib`; amd64's TSC timecounter (`tsc.c`) is deferred.

Decisions pending (the user's): the scope section (open until M13); the PC's CPU (Intel VMX
or AMD SVM) for vmm and M15; the exact Raspberry Pi 4 model; networking in M15.
