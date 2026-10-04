# Status

Milestone: **M9+ done** (network completion, INET6 last); **M10a**, **M10b** and **M10c done**
(persistent disk, UFS options, memory and removable file systems); M10f next. Updated: 2026-10-04.

Done:
- M7a/M7b/M8/M8b: uvm, mbufs, PCI, virtio, IPv4; ffs root on rd0a, init, ksh, /etc/rc, login.
- M9 (all in `just smoke`, both archs): sockets, kqueue, rtsock, ifconfig/ping/route; crypto
  and wg(4) between two VMs; IPsec (ESP, AH, IPIP, enc(4), PF_KEY, ipsecctl); the pf family,
  pflog, hfsc, fq_codel, pfctl; pfsync(4) and pflow(4) (`smoke-pfsync`).
- Diagnostic tools stage 2: ps/fstat/vmstat/df over sysctl (`smoke-diag`); pledge(2) enforced.
- M9+: TCP (SYN cache, SACK, ECN, TCP-MD5), IPComp, HTTPS with LibreSSL, bpf(4), divert,
  IGMP, tcpdump, INET6 (`smoke-tcp`, `-ipcomp`, `-https`, `-divert`, `-tcpdump`, `-inet6`).
- amd64 runs on the TSC (`tsc.c`, `kern.timecounter`); smokes reject `uptime went backwards`.
- M10a: vioblk(4), the SCSI midlayer, sd(4), physio(9); newfs/fsck/disklabel/fdisk; a
  persistent virtio-blk image per VM; `smoke-disk` (two boots, `fsck -fn` clean, file back).
- M10b: option QUOTA (`ufs_quota.c`), UFS_DIRHASH, MFS as default-on features; quota tools,
  su, mount_mfs; `smoke-ufsopts` (EDQUOT and repquota, a hashed 5,000-entry directory, mfs).
- M10c: tmpfs, msdosfs, cd9660, udf, vnd(4), their tools; `smoke-fs` (FAT/ISO/UDF on vnd).

Next:
- M10f, e, d, then M11a..e (SMP; then every smoke runs MP).

Blockers:
- Under load the amd64 TSC can measure high (1.2-1.3 GHz for ~1.0), so the clock runs slow
  until acpitimer/acpihpet recalibrate it (M13; accepted by the user).

Decisions pending (the user's): the scope section (open until M13); the PC's CPU (Intel VMX
or AMD SVM) for vmm and M15; the exact Raspberry Pi 4 model; networking in M15.
