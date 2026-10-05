# Status

Milestone: **M10 done**; **M11 done** (M11a..M11e: SMP); M12 next. Updated: 2026-10-04.

Done:
- M7a..M10f: uvm, virtio, IPv4/6, TCP, ffs root, init, login, sockets, wg, IPsec, pf, bpf;
  disks, quotas, tmpfs/msdosfs/cd9660/udf/vnd, softraid, NFS, ext2fs, ntfs, fuse (`smoke-*`).
- M11a..M11d: APs through Limine, the kernel lock, per-CPU run queues, SMR, IPIs and TLB
  shootdowns; MP timekeeping; ddb on MP; eight softnet queues, one kept per CPU.
- M11e: the MP audit. pageqlock and pm_mtx real, uvm_fault and exit unlocked; MPSAFE flags,
  softclockmp and SY_NOLOCK honoured; every KERNEL_LOCK comment a real call; SMR in the
  network. Every `just smoke` recipe now boots the MP kernel with `-smp 4`; `smoke-up` is
  the one uniprocessor boot; `smoke-tcpbench` stresses TCP between two MP VMs.

Next:
- M12 (devices: audio, USB in QEMU), then M13..M15.

Blockers:
- amd64 kernel stacks are tight: about 4.9 KB stay free under softraid I/O (M10f measure).
- Under load the amd64 TSC can measure high (1.2-1.3 GHz for ~1.0), so the clock runs slow
  until acpitimer/acpihpet recalibrate it (M13; accepted by the user).
- A `diagnostic` MP kernel panics at boot (`uvm_page_physload: page size not set!`).
- Statistics counters the C bumps unlocked stay `Cell`s (docs/ARCHITECTURE.md, M11e).

Decisions pending (the user's): the scope section (open until M13); the PC's CPU (Intel VMX
or AMD SVM) for vmm, named when M15 starts; the exact Raspberry Pi 4 model; networking in M15;
whether to report QEMU's lost `sev` (arm64 TCG) upstream.
