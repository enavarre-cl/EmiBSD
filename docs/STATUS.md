# Status

Milestone: **M9+ done**; **M10a**, **M10b**, **M10c**, **M10f** and **M10e done** (persistent
disk, UFS options, memory and removable file systems, softraid, NFS); M10d next.
Updated: 2026-10-04.

Done:
- M7a..M8b: uvm, mbufs, PCI, virtio, IPv4; ffs root on rd0a, init, ksh, /etc/rc, login. M9:
  sockets, kqueue, rtsock, wg(4), IPsec, pf, pfsync, pflow; ps/fstat/vmstat/df; pledge(2).
  M9+: TCP, IPComp, HTTPS, bpf, divert, tcpdump, INET6.
- amd64 runs on the TSC (`tsc.c`, `kern.timecounter`); smokes reject `uptime went backwards`.
- M10a: vioblk(4), SCSI midlayer, sd(4), physio(9), the disk tools, a persistent image per VM
  (`smoke-disk`). M10b: QUOTA, UFS_DIRHASH, MFS; quota tools, su, mount_mfs (`smoke-ufsopts`).
- M10c: tmpfs, msdosfs, cd9660, udf, vnd(4), their tools; `smoke-fs` (FAT/ISO/UDF on vnd).
- M10f: softraid and all seven disciplines, bio(4), sensors, bioctl, four disks per VM
  (`--disks`); `smoke-softraid` (RAID 6 via our `sr6create`: bioctl has no `-c 6`).
- M10e: all of `nfs/` (nfs_aiod.c skipped: not compiled by OpenBSD), portmap, mountd, nfsd,
  mount_nfs, showmount; `smoke-nfs` (two VMs, mounts over UDP and TCP, files both ways).

Next:
- M10d (ext2fs, ntfs, fuse), then M11a..e (SMP; then every smoke runs MP).

Blockers:
- amd64 kernel stacks are tight: about 4.9 KB stay free under softraid I/O (M10f measure).
- Under load the amd64 TSC can measure high (1.2-1.3 GHz for ~1.0), so the clock runs slow
  until acpitimer/acpihpet recalibrate it (M13; accepted by the user).

Decisions pending (the user's): the scope section (open until M13); the PC's CPU (Intel VMX
or AMD SVM) for vmm, named when M15 starts; the exact Raspberry Pi 4 model; networking in M15.
