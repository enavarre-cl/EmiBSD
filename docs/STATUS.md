# Status

Milestone: **M10 done**, **M11a done** (MP bring-up: four CPUs per VM with the
`multiprocessor` kernel); M11b next. Updated: 2026-10-04.

Done:
- M7a..M9+: uvm, mbufs, virtio, IPv4/6, TCP; ffs root, init, ksh, login; sockets, kqueue,
  wg(4), IPsec, the pf family, bpf, tcpdump, HTTPS; ps/fstat/vmstat/df; pledge(2).
- M10a..M10f: vioblk, sd(4), the disk tools; QUOTA, DIRHASH, MFS; tmpfs, msdosfs, cd9660, udf,
  vnd; softraid; NFS; ext2fs, ntfs (amd64), fuse. Each has its `smoke-*` recipe.
- M11a: APs started through the Limine MP request (replaces mptramp.S/PSCI), the kernel lock
  and spinning mutex, kern_sched.c whole, SMR, percpu, per-CPU pool caches, real pool/malloc/
  fpageq locks; amd64 IPIs and TLB shootdowns, arm64 cpu.c and ampintc SGIs; `smoke-mp`
  (`-smp 4`, both archs: 4 cpus running, kthread across CPUs, mpstress). Default kernel: UP.

Next:
- M11b (TSC sync, per-CPU clocks), M11c (ddb on MP), M11d (softnet x8), M11e (the audit;
  then every smoke runs MP), then M12.

Blockers:
- Until M11e, unaudited paths run under the kernel lock (MPSAFE flags and SY_NOLOCK ignored,
  uvm_fault and process teardown locked: `uvm.pageqlock` is still a no-op).
- amd64 kernel stacks are tight: about 4.9 KB stay free under softraid I/O (M10f measure).
- Under load the amd64 TSC can measure high (1.2-1.3 GHz for ~1.0), so the clock runs slow
  until acpitimer/acpihpet recalibrate it (M13; accepted by the user).

Decisions pending (the user's): the scope section (open until M13); the PC's CPU (Intel VMX
or AMD SVM) for vmm, named when M15 starts; the exact Raspberry Pi 4 model; networking in M15.
