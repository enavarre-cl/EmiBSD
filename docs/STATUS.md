# Status

Milestone: **M9 done** (WireGuard, IPsec ESP tunnel, pf, on both archs). Updated: 2026-10-03.

Done:
- M7a/M7b (exit met: `init: demand-zero bss ok`, `selftest: ping 10.0.2.2`): uvm_map/fault,
  mbufs, autoconf, PCI, virtio + `vio`, routing, ARP, IPv4, ICMP; creds, signals, vfs core.
- M8/M8b (exit met: `just smoke-shell`, `smoke-login`): ffs root on rd0a, OpenBSD's init,
  ksh, multi-user `/etc/rc`, getty, login; execve/ELF, buffer cache, tty, amd64 FPU, makefs.
- M9 (exit met, all in `just smoke`, both archs): M9a sockets, kqueue, inet pcbs, rtsock,
  ifconfig/ping/route (`smoke-net`, `smoke-route`); M9b `sys/crypto` + wg(4) between two
  VMs (`smoke-link`, `smoke-wg`); M9c IPsec: SA database, SPD, ESP, AH, IPIP, enc(4),
  PF_KEY, ipsecctl (`smoke-ipsec`, `smoke-esp`; IPComp reported, needs deflate); M9d the pf
  family, pflog, hfsc, fq_codel, pfctl (`smoke-pf`, a pf rule on wg0 in `smoke-wg`).
- Diagnostic tools stage 2: libkvm, ps/fstat/vmstat/df over sysctl (`smoke-diag`).

Next:
- pfsync and pflow (added to M9 by the user; agent running), then amd64's `tsc.c`. M9+
  (TCP, bpf, INET6, divert, IGMP, IPComp, HTTPS with LibreSSL and ftp) beside diagnostic
  tools stage 2; then M10a..f (disk, UFS options, tmpfs/FAT/ISO/UDF, ext2/NTFS/FUSE, NFS,
  softraid) and M11a..e (SMP; afterwards every smoke runs MP, -smp 4). M14b when idle.

Blockers:
- amd64's TSC timecounter (`tsc.c`) is deferred.
- bpf(4) is not ported (NBPFILTER 0): the C moves a decapsulated IPsec packet to enc0 only
  under NBPFILTER, so a host (not a forwarding gateway) drops tunnel traffic for an address on
  another interface as `ips_wrongif`; `smoke-esp`'s VMs forward. Fixed once bpf is ported.

Decisions pending (the user's): the scope section (open until M13); the PC's CPU (Intel VMX
or AMD SVM) for vmm and M15; the exact Raspberry Pi 4 model; networking in M15.
