# Status

Milestone: **M7a done; M7b (the network) in progress**. Updated: 2026-10-03.

Done:
- M7a (exit met: `init: demand-zero bss ok` on both archs, user pages served by
  `uvm_fault`): `uvm_map.c`/`uvm_addr.c` (`a4f74ba`), `uvm_fault.c` (`e527558`), the traps
  calling it (`5d9550b`), pv lists, `pmap_protect`, arm64 R/M emulation (`492c991`), exec
  through `uvm_map` with the wired stand-ins retired (`5474591`), `uvm_mmap.c`, `uvm_unix.c`
  and the `plimit` layer of `kern_resource.c` (`7253ef3`).
- M7b so far: task queues, mbufs, autoconf + `mainbus` (`ioconf.rs` per arch), the socket/
  net/netinet headers, bus_dma (both archs) + PCI (amd64), `if.c`/`ifq.c`/`if_ethersubr.c` +
  `lo0`, `km_alloc` over `kernel_map`/`kmem_map`. For M8: credentials, limits, signals
  (handler + `sigreturn`), the file table, `sysctl` (the system reports EmiBSD 7.8).

Next:
- M7b: virtio (pci/mmio) + `if_vio`, ARP/IPv4/ICMP + routing (two agents running); exit: the
  kernel's ICMP echo to `10.0.2.2` is answered. For M8 in parallel: the vfs core (running),
  then the buffer cache, ffs, a ramdisk or virtio-blk, tty/pty (`docs/ROADMAP.md`).
- `reference/PINNED.md` (`Subtree:`) and `reference/README.md` (the clone command) still say
  `sys` only: the user edits them (Edit/Write are denied under `reference/`).

Blockers:
- Licence answers pending: Dyson (`sys_pipe.c`, skipped until then); NRL (BSD-4 style) taken
  as accepted. `crc32` stays `skipped: license: zlib`; amd64's TSC timecounter (`tsc.c`) is deferred.

Decisions pending (the user's): the scope section (open until M13); the PC's CPU (Intel VMX
or AMD SVM) for vmm and M15; the exact Raspberry Pi 4 model; networking in M15.
