# Status

Milestone: **M9b and M9d done** (WireGuard tunnel; pf blocks pings, on wg0 too); M9c next. Updated: 2026-10-03.

Done:
- M7a/M7b (exit met: `init: demand-zero bss ok`, `selftest: ping 10.0.2.2`): uvm_map/fault,
  mbufs, autoconf, PCI, virtio + `vio`, routing, ARP, IPv4, ICMP; creds, signals, vfs core.
- M8 (exit met: `just smoke-shell`, both archs: root on rd0a, OpenBSD's `init(8)` in single
  user, ksh runs `uname -a`, `cat /etc/motd`, `ls /`): buffer cache, vnode pager, pipes, tty,
  ffs (`5506e47`), rd(4) + disk layer (`bd92ac0`), execve/exec_elf/pin_check and the process,
  time and select/poll syscalls (`a737765`..`e667958`), `setroot`/`diskconf`, amd64 FPU
  (`fpu.c`), OpenBSD's makefs for the image, libc/init/ksh/cat/echo/ls/uname.
- M9a (exit met: `just smoke-net`/`smoke-route`): sockets, kqueue, in_pcb/udp/raw_ip,
  rtsock; ifconfig, ping, route on the ramdisk. M8b: multi-user `/etc/rc`, getty, login.
- M9b (exit met: `just smoke-wg`, both archs): `sys/crypto`, `wg_noise`, `wg_cookie`,
  `if_wg`; `cargo xtask smoke2` boots two VMs on a private `vio1` link (`just smoke-link`).
- M9d (exit met: `just smoke-pf`, the wg0 rule in `smoke-wg`): the pf family, pflog, hfsc,
  fq_codel, radix, toeplitz, TCP headers; pfctl on the ramdisk.

Next:
- M9c IPsec (agent running; its pf_test on enc0 is wired when it lands). Then diagnostic
  tools stage 2 (libkvm, ps with `uvm_io`, fstat, vmstat, df/mount). M14b when no agents run.

Blockers:
- amd64's TSC timecounter (`tsc.c`) is deferred.

Decisions pending (the user's): the scope section (open until M13); the PC's CPU (Intel VMX
or AMD SVM) for vmm and M15; the exact Raspberry Pi 4 model; networking in M15; pfsync and
pflow left out of GENERIC's pseudo-devices (`docs/ARCHITECTURE.md`).
