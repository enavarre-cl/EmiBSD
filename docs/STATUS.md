# Status

Milestone: **M16b, M16e and M16f done** (USB, platform drivers, arm64 platform); the rest of M16
(M16a, M16c, M16d, M16g: storage, network, console/virtio/legacy, install images) under way,
then M17 (real hardware, vmm). Updated: 2026-10-08.

Done:
- M16b: ehci, uhci, ohci; ums/uwacom over hidms, uhid, ugen and usbdevs(8), cdce, ucom with
  uftdi, uaudio. ehci, and a write through ohci, behave as on OpenBSD 8.0 in QEMU.
- M16f: agintc (GICv3, ITS), smmu (SMMUv2/v3), gpio(4), plgpio, gpiokeys (as on OpenBSD 8.0).
- M16e: UKC (`boot -c`), ppb, acpidmar, iic with ichiic and piixpm, ipmi, tpm on swtpm, acpicpu.

Next:
- M16a, M16c, M16d, M16g (`ci-full` once at M16's close); then M17.
- Left by M14: GPL parts of the sets, lldb, efi(4), base programs (sort, find, ...), `__thrsleep`.

Unsafe (`cargo xtask unsafe-report`): kernel 8198 blocks, 1041 fn, 768 impl, 27 trait, 323 other; tests 816 more.

Blockers:
- amd64 kernel stacks are tight: about 4.9 KB stay free under softraid I/O (M10f measure).
- A `diagnostic` MP kernel panics at boot (`uvm_page_physload: page size not set!`).
- Statistics counters the C bumps unlocked stay `Cell`s (docs/ARCHITECTURE.md, M11e).
- Open flakes: amd64 serial cut at QEMU exit (M16f); host test `no idleproc set on CPU0`, ~1 in
  80 loaded runs; EDK2's UhciDxe ASSERT before the kernel on arm64 (~1 in 5 uhci boots; M16b).

Decisions pending (the user's): the scope section; a SeaBIOS path for vga(4)'s text mode;
Limine's retirement; the PC's CPU for vmm (M17); the Raspberry Pi 4 model; networking in M17;
reporting QEMU's lost `sev` upstream; the restated M16b ehci/ohci criterion (relayed).
