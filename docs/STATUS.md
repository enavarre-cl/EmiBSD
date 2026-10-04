# Status

Milestone: **M9 done** (WireGuard, IPsec, pf, pfsync, pflow; both archs); M9+ under way. Updated: 2026-10-04.

Done:
- M7a/M7b (`init: demand-zero bss ok`, `selftest: ping 10.0.2.2`): uvm, mbufs, PCI, virtio, IPv4.
- M8/M8b (exit met: `just smoke-shell`, `smoke-login`): ffs root on rd0a, OpenBSD's init,
  ksh, multi-user `/etc/rc`, getty, login; execve/ELF, buffer cache, tty, amd64 FPU, makefs.
- M9 (exit met, all in `just smoke`, both archs): M9a sockets, kqueue, inet pcbs, rtsock,
  ifconfig/ping/route (`smoke-net`, `smoke-route`); M9b `sys/crypto` + wg(4) between two
  VMs (`smoke-link`, `smoke-wg`); M9c IPsec: SA database, SPD, ESP, AH, IPIP, enc(4),
  PF_KEY, ipsecctl (`smoke-ipsec`, `smoke-esp`); M9d the pf
  family, pflog, hfsc, fq_codel, pfctl (`smoke-pf`, a pf rule on wg0 in `smoke-wg`);
  pfsync(4) and pflow(4) between the two VMs (`smoke-pfsync`).
- Diagnostic tools stage 2: libkvm, ps/fstat/vmstat/df over sysctl (`smoke-diag`).
- M9+: TCP with SYN cache, SACK, ECN, TCP-MD5 (`init: tcp ok`; nc and ftp work over it);
  zlib and IPComp (`smoke-ipcomp`); HTTPS with LibreSSL, ftp and nc (`smoke-https`).

Next:
- amd64's `tsc.c`; M9+: bpf, divert, IGMP (agent running), INET6, the criterion recipes.
  Then M10a..f and M11a..e (SMP; afterwards every smoke runs MP, -smp 4).

Blockers:
- amd64's TSC timecounter (`tsc.c`) is deferred.

Decisions pending (the user's): the scope section (open until M13); the PC's CPU (Intel VMX
or AMD SVM) for vmm and M15; the exact Raspberry Pi 4 model; networking in M15.
