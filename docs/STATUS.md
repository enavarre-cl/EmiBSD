# Status

Milestone: **M10 done**; **M11a**..**M11d done** (MP bring-up, timekeeping, ddb on MP,
network parallelism); M11e next. Updated: 2026-10-04.

Done:
- M7a..M10f: uvm, virtio, IPv4/6, TCP, ffs root, init, login, sockets, wg, IPsec, pf, bpf;
  disks, quotas, tmpfs/msdosfs/cd9660/udf/vnd, softraid, NFS, ext2fs, ntfs, fuse (`smoke-*`).
- M11a: APs started through the Limine MP request, the kernel lock, kern_sched.c, SMR, percpu,
  per-CPU pool caches; IPIs and TLB shootdowns on both archs; `smoke-mp`. Default kernel: UP.
- M11b: tsc.c's sync test runs against each AP (TCG passes it); every CPU dispatches its own
  clockintr with a monotonic uptime; kern_tc.c (tc_lock) and arm64 agtimer.c ported.
- M11c: the ddb command loop (db_command.c and friends); on MP the other CPUs stop by IPI,
  `machine cpuinfo`/`ddbcpu`/`startcpu`/`stopcpu`; `smoke-ddbmp` (`-smp 4`, both archs).
- M11d: 8 softnet queues, one kept per CPU, kern_intrmap.c, SMR for the interface index map;
  `smoke-net-mp` (two MP VMs, `-smp 4`: softnets-4, ping, wg, TCP; `-smp 8`: softnets-8).

Next:
- M11e (the audit; then every smoke runs MP), then M12.

Blockers:
- Until M11e, unaudited paths run under the kernel lock (MPSAFE flags and SY_NOLOCK ignored,
  uvm_fault and process teardown locked: `uvm.pageqlock` is still a no-op).
- amd64 kernel stacks are tight: about 4.9 KB stay free under softraid I/O (M10f measure).
- Under load the amd64 TSC can measure high (1.2-1.3 GHz for ~1.0), so the clock runs slow
  until acpitimer/acpihpet recalibrate it (M13; accepted by the user).

Decisions pending (the user's): the scope section (open until M13); the PC's CPU (Intel VMX
or AMD SVM) for vmm, named when M15 starts; the exact Raspberry Pi 4 model; networking in M15.
