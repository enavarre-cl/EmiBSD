# Status

Milestone: **M8 done** (OpenBSD's init(8) and ksh on an ffs ramdisk root). Updated: 2026-10-03.

Done:
- M7a (exit met: `init: demand-zero bss ok`, user pages served by `uvm_fault`): `uvm_map.c`,
  `uvm_fault.c`, pv lists, `pmap_protect`, exec through `uvm_map`, `uvm_mmap.c`, `uvm_unix.c`.
- M7b (exit met: `selftest: ping 10.0.2.2: echo reply received` on both archs): task queues,
  mbufs, autoconf, bus_dma + PCI (amd64), `if.c`/`ifq.c`/ether, virtio (pci, mmio) + `vio`,
  routing (art, rtable, route), ARP, IPv4, ICMP. For M8: credentials, limits, signals, the
  file table, `sysctl` (EmiBSD 8.0), the vfs core.
- M8 (exit met: `just smoke-shell`, both archs: root on rd0a, OpenBSD's `init(8)` in single
  user, ksh runs `uname -a`, `cat /etc/motd`, `ls /`): buffer cache, vnode pager, pipes, tty,
  ffs (`5506e47`), rd(4) + disk layer (`bd92ac0`), execve/exec_elf/pin_check and the process,
  time and select/poll syscalls (`a737765`..`e667958`), `setroot`/`diskconf`, amd64 FPU
  (`fpu.c`), OpenBSD's makefs for the image, libc/init/ksh/cat/echo/ls/uname.

Next:
- After M8 (estimates given 2026-10-03): crc32 (zlib, accepted) then GPT in `subr_disk.c`;
  diagnostic tools stage 1 (sysctl, hostname, date, pwd, id), stage 2 (libkvm, ps with
  `uvm_io`, fstat, vmstat, df/mount, passwd/pwd.db); `/etc/rc` and multi-user later.

Blockers:
- amd64's TSC timecounter (`tsc.c`) is deferred. (`crc32`'s zlib licence was accepted on
  2026-10-03; its port and GPT follow M8.)

Decisions pending (the user's): the scope section (open until M13); the PC's CPU (Intel VMX
or AMD SVM) for vmm and M15; the exact Raspberry Pi 4 model; networking in M15.
