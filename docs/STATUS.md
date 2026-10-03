# Status

Milestone: **M7a done; M7b (the network) in progress**. Updated: 2026-10-03.

Done:
- M7a (exit met: `init: demand-zero bss ok` on both archs, user pages served by
  `uvm_fault`): `uvm_map.c`/`uvm_addr.c` (`a4f74ba`), `uvm_fault.c` (`e527558`), the traps
  calling it (`5d9550b`), pv lists, `pmap_protect`, arm64 R/M emulation (`492c991`), exec
  through `uvm_map` with the wired stand-ins retired (`5474591`), `uvm_mmap.c`, `uvm_unix.c`
  and the `plimit` layer of `kern_resource.c` (`7253ef3`).
- M7b so far: `kern_task.c` (task queues), `uipc_mbuf.c`/`uipc_mbuf2.c` (mbufs),
  `subr_autoconf.c` with `mainbus` and hand-written `ioconf.rs` per arch; for M8,
  `kern_prot.c` (credentials, ids, `__set_tcb`) and the priority syscalls.

Next:
- M7b: the net headers, bus_dma + PCI, then virtio, `if.c`, ARP, ip/icmp, `if_vio.c`; exit:
  the kernel's ICMP echo to `10.0.2.2` is answered. In parallel for M8: signals, the file
  table, then vfs, ffs, virtio-blk, tty (`docs/ROADMAP.md`).
- `reference/PINNED.md` (`Subtree:`) and `reference/README.md` (the clone command) still say
  `sys` only: the user edits them (Edit/Write are denied under `reference/`).

Blockers:
- None. `crc32` stays `skipped: license: zlib`; amd64's TSC timecounter (`tsc.c`) is deferred.

Decisions pending (the user's): the scope section (open until M13); the PC's CPU (Intel VMX
or AMD SVM) for vmm and M15; the exact Raspberry Pi 4 model; networking in M15.
