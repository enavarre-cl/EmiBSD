# Status

Milestone: **M7a in progress** (parts 0 to 3a done; 3b, the pmap side and exec through
`uvm_map`, next). Updated: 2026-10-03.

Done:
- M7a-3a (`e527558`): `uvm_fault.c` whole; `uvmexp_counters`, `uvm_swapisfull`, `pmap_unwire`.
  Host tests serve zero fill, copy-on-write after fork, fault-ahead, wiring. The traps do not
  call `uvm_fault` yet; swap paths are reported until M7.
- M7a-2 (`a4f74ba`): `uvm_map.c` and `uvm_addr.c`; a real `kernel_map`, `uvmspace_fork`
  copying entries; amd64's kernel window fixed above Limine's direct map.
- M7a-1 (`576f912`): the object layer: `uvm_anon.c`, `uvm_amap.c`, `uvm_aobj.c`, pager ops.

Next:
- M7a-3b: amd64 pv lists and `pmap_protect`, arm64 `pmap_fault_fixup`, the traps calling
  `uvm_fault`, exec through `uvm_map` (the wired stand-ins retire), `uvm_mmap.c`. Exit: a user
  page fault served by `uvm_fault` on both archs.
- M7b: the network first (mbufs, `if.c`, ARP, ip/icmp, virtio-net); exit: the kernel's ICMP
  echo to `10.0.2.2` is answered. Then the rest of M7+, then M8 (`docs/ROADMAP.md`).
- Widen the reference clone to `lib/ bin/ sbin/ usr.bin/ libexec/` (same pin, sparse) in its
  own `reference:` + `rules:` commit, before M8.

Blockers:
- None. `crc32` stays `skipped: license: zlib`; amd64's TSC timecounter (`tsc.c`) is deferred.

Decisions pending (the user's): the scope section (open until M13); the PC's CPU (Intel VMX
or AMD SVM) for vmm and M15; the exact Raspberry Pi 4 model; networking in M15.
