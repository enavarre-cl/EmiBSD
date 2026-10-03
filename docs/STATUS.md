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

Next:
- M8's kernel side: buffer cache + vnode pager + syncer/lockf, the device switch + tty +
  `mem` (two agents running), then ffs on a ramdisk root, exec from vnodes with `argv`,
  pipes (licence answer pending); then the userland build (`docs/ROADMAP.md`).
- `reference/PINNED.md` (`Subtree:`) and `reference/README.md` (the clone command) still say
  `sys` only: the user edits them (Edit/Write are denied under `reference/`).

Blockers:
- Licence answers pending: Dyson (`sys_pipe.c`, skipped until then); NRL (BSD-4 style) taken
  as accepted. `crc32` stays `skipped: license: zlib`; amd64's TSC timecounter (`tsc.c`) is deferred.

Decisions pending (the user's): the scope section (open until M13); the PC's CPU (Intel VMX
or AMD SVM) for vmm and M15; the exact Raspberry Pi 4 model; networking in M15.
