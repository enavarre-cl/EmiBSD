# Status

Milestone: **M9 done** (WireGuard, IPsec, pf, pfsync, pflow; both archs); M9+ under way. Updated: 2026-10-04.

Done:
- M7a/M7b/M8/M8b: uvm, mbufs, PCI, virtio, IPv4; ffs root on rd0a, init, ksh, /etc/rc, login.
- M9 (exit met, all in `just smoke`, both archs): M9a sockets, kqueue, inet pcbs, rtsock,
  ifconfig/ping/route (`smoke-net`, `smoke-route`); M9b `sys/crypto` + wg(4) between two
  VMs (`smoke-link`, `smoke-wg`); M9c IPsec: SA database, SPD, ESP, AH, IPIP, enc(4),
  PF_KEY, ipsecctl (`smoke-ipsec`, `smoke-esp`); M9d the pf
  family, pflog, hfsc, fq_codel, pfctl (`smoke-pf`, a pf rule on wg0 in `smoke-wg`);
  pfsync(4) and pflow(4) between the two VMs (`smoke-pfsync`).
- Diagnostic tools stage 2: libkvm, ps/fstat/vmstat/df over sysctl (`smoke-diag`); pledge(2)
  enforced (`kern_pledge.c`; `init: pledge ok`).
- M9+: TCP with SYN cache, SACK, ECN, TCP-MD5 (`init: tcp ok`; nc and ftp work over it);
  zlib and IPComp (`smoke-ipcomp`); HTTPS with LibreSSL, ftp and nc (`smoke-https`);
  bpf(4), divert sockets, IGMP (`smoke-esp` without IP forwarding).
- amd64 runs on the TSC (`tsc.c`, `kern.timecounter`); smokes reject `uptime went backwards`.

Next:
- M9+: INET6, the criterion recipes (tcpdump, nc between VMs, ping6, divert-to). Then
  M10a..f and M11a..e (SMP; afterwards every smoke runs MP, -smp 4).

Blockers:
- Under load the amd64 TSC can measure high (1.2-1.3 GHz for ~1.0), so the clock runs slow
  until acpitimer/acpihpet recalibrate it (M13; accepted by the user).

Decisions pending (the user's): the scope section (open until M13); the PC's CPU (Intel VMX
or AMD SVM) for vmm and M15; the exact Raspberry Pi 4 model; networking in M15.
