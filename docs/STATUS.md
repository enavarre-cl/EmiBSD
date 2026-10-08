# Status

Milestone: **M16e and M16f done** (platform drivers, arm64 platform); the rest of M16
(M16a..M16d, M16g: storage, USB, network, console/virtio/legacy, install images) under way,
then M17 (real hardware, vmm). Updated: 2026-10-08.

Done:
- M15: LICENSES, CODE and TESTS zones in every `.rs`, validated by `ports check`; tests inline.
- M16f: agintc (GICv3, ITS; every arm64 smoke passes with `EMIBSD_GIC=3`), smmu (SMMUv2/v3;
  `smoke-smmu`), gpio(4), plgpio, gpiokeys (power key ignored, as on OpenBSD 8.0).
- M16e: UKC (`boot -c`), ppb, acpidmar (VT-d, AMD-Vi), iic with ichiic and piixpm, ipmi with the
  watchdog and SMBIOS, tpm on swtpm, acpicpu; ichiic and ipmi behave as on OpenBSD 8.0 in QEMU.

Next:
- M16a..M16d, M16g (`ci-full` once at M16's close); then M17.
- Left by M14: GPL parts of the sets, lldb, efi(4), base programs (sort, find, ...), `__thrsleep`.

Unsafe (`cargo xtask unsafe-report`): kernel 7951 blocks, 1015 fn, 739 impl, 27 trait, 323 other; tests 806 more.

Blockers:
- amd64 kernel stacks are tight: about 4.9 KB stay free under softraid I/O (M10f measure).
- A `diagnostic` MP kernel panics at boot (`uvm_page_physload: page size not set!`).
- Statistics counters the C bumps unlocked stay `Cell`s (docs/ARCHITECTURE.md, M11e).
- Open flakes: amd64 serial cut at QEMU exit (M16f); host test `no idleproc set on CPU0`, ~1 in 80 loaded runs.
- QEMU's igb passes no traffic with em(4)'s legacy descriptors (M13; not checked in QEMU).

Decisions pending (the user's): the scope section; a SeaBIOS path for vga(4)'s text mode;
Limine's retirement; the PC's CPU for vmm (M17); the Raspberry Pi 4 model; networking in M17;
reporting QEMU's lost `sev` upstream.
