# Status

Milestone: **M11 done** (SMP); **M12 done** (devices in QEMU); M13 under way. Updated: 2026-10-05.

Done:
- M7a..M10f: uvm, virtio, IPv4/6, TCP, ffs root, init, login, sockets, wg, IPsec, pf, bpf;
  disks, quotas, tmpfs/msdosfs/cd9660/udf/vnd, softraid, NFS, ext2fs, ntfs, fuse (`smoke-*`).
- M11: APs, the kernel lock, per-CPU run queues, SMR, IPIs; MP timekeeping and ddb; softnet
  per CPU; the MP audit. Every smoke boots the MP kernel with `-smp 4`; `smoke-up` is UP.
- M12: audio(4), azalia (amd64 and arm64) and auich/ac97 (amd64); the USB core, xhci,
  uhub, umass, uhidev/ukbd, hid; a PCI bus on arm64 (pciecam, GICv2m MSI). `smoke-audio`
  plays a tone with aucat(1) into QEMU's WAV capture; `smoke-usb` mounts a FAT stick.

Next:
- M13 (storage, firmware, console; nvme, vioscsi and cd are in), then M14, M15.
- M13 picks up M12's leftovers: wskbd and ukbdmap.c (the keyboard is silent until then).

Unsafe (`cargo xtask unsafe-report`): kernel 5319 blocks, 644 fn, 497 impl, 17 trait, 103 other; tests 569 more.

Blockers:
- amd64 kernel stacks are tight: about 4.9 KB stay free under softraid I/O (M10f measure).
- Under load the amd64 TSC can measure high (1.2-1.3 GHz for ~1.0), so the clock runs slow
  until acpitimer/acpihpet recalibrate it (M13; accepted by the user).
- A `diagnostic` MP kernel panics at boot (`uvm_page_physload: page size not set!`).
- Statistics counters the C bumps unlocked stay `Cell`s (docs/ARCHITECTURE.md, M11e).
- arm64 configures azalia although its GENERIC does not (QEMU's HD Audio; ROADMAP M12).

Decisions pending (the user's): the scope section (open until M13); the PC's CPU (Intel VMX
or AMD SVM) for vmm, named when M15 starts; the exact Raspberry Pi 4 model; networking in M15;
whether to report QEMU's lost `sev` (arm64 TCG) upstream.
