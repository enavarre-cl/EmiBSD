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
- M8b (multi-user boot and login, `docs/ROADMAP.md`): /etc files, pwd_mkdb for the host,
  getty/login/login_passwd, AF_UNIX sockets for BSD Auth, a writable root (unveil and the
  amd64 RTC are in: `af65fa0`, `c7b370d`). Then diagnostic tools stage 2 (libkvm, ps with
  `uvm_io`, fstat, vmstat, df/mount). M9 is split: M9a sockets and network userland, M9b
  WireGuard, M9c IPsec, M9d pf (last: it filters the tunnels too). M14b when no agents run.

Blockers:
- amd64's TSC timecounter (`tsc.c`) is deferred.

Decisions pending (the user's): the scope section (open until M13); the PC's CPU (Intel VMX
or AMD SVM) for vmm and M15; the exact Raspberry Pi 4 model; networking in M15.
