# Status

Milestone: **M10a done** (persistent disk: vioblk, SCSI, sd; both archs); M9+ INET6 under way. Updated: 2026-10-04.

Done:
- M7a/M7b/M8/M8b: uvm, mbufs, PCI, virtio, IPv4; ffs root on rd0a, init, ksh, /etc/rc, login.
- M9 (all in `just smoke`, both archs): sockets, kqueue, rtsock, ifconfig/ping/route; crypto
  and wg(4) between two VMs; IPsec (ESP, AH, IPIP, enc(4), PF_KEY, ipsecctl); the pf family,
  pflog, hfsc, fq_codel, pfctl; pfsync(4) and pflow(4) (`smoke-pfsync`).
- Diagnostic tools stage 2: libkvm, ps/fstat/vmstat/df over sysctl (`smoke-diag`); pledge(2)
  enforced (`kern_pledge.c`; `init: pledge ok`).
- M9+: TCP with SYN cache, SACK, ECN, TCP-MD5 (`init: tcp ok`; nc and ftp work over it);
  zlib and IPComp (`smoke-ipcomp`); HTTPS with LibreSSL, ftp and nc (`smoke-https`);
  bpf(4), divert sockets, IGMP (`smoke-esp` without forwarding); criterion recipes `smoke-tcp`,
  `smoke-divert`, `smoke-tcpdump` (libpcap, tcpdump with privsep).
- amd64 runs on the TSC (`tsc.c`, `kern.timecounter`); smokes reject `uptime went backwards`.
- M10a: vioblk(4), the SCSI midlayer, sd(4), physio(9); newfs/fsck/disklabel/fdisk; a
  persistent virtio-blk image per VM; `smoke-disk` (two boots, `fsck -fn` clean, file back).

Next:
- M9+: INET6 (ping6); then M10c, b, f, e, d and M11a..e (SMP; then every smoke runs MP).

Blockers:
- Under load the amd64 TSC can measure high (1.2-1.3 GHz for ~1.0), so the clock runs slow
  until acpitimer/acpihpet recalibrate it (M13; accepted by the user).

Decisions pending (the user's): the scope section (open until M13); the PC's CPU (Intel VMX
or AMD SVM) for vmm and M15; the exact Raspberry Pi 4 model; networking in M15.
