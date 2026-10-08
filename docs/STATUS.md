# Status

Milestone: **M15 done** (code and test layout); M16a..M16g (QEMU drivers: storage, USB, network,
console/virtio/legacy, platform, arm64 platform, install images) next, then M17 (real hardware,
vmm). Updated: 2026-10-08.

Done:
- M7a..M13: uvm, network, file systems, SMP (smokes on `-smp 2`, `-smp 4` in `ci-full`),
  devices, diff-openbsd; NVMe/AHCI roots, ACPI on amd64, PSCI, RTC, em/re/vmx, the frame buffer console.
- M14: our efiboot and boot(8) entry on both archs; bsd.rd, signed base and comp sets,
  install.sub; the installed disk boots and runs `cc`; clang/lld, ld.so; arm64 ACPI.
- M15: every `.rs` under `sys/` and `tools/` is split into LICENSES, CODE and TESTS zones
  (`ports check` validates them); the 324 `tests.rs` are inline; same tests, same unsafe totals.

Next:
- M16a..M16g (`ci-full` once at each split milestone's close); then M17.
- Left by M14: GPL parts of the sets, lldb, efi(4), base programs (sort, find, ...), `__thrsleep`.

Unsafe (`cargo xtask unsafe-report`): kernel 7547 blocks, 984 fn, 701 impl, 25 trait, 322 other; tests 790 more.

Blockers:
- amd64 kernel stacks are tight: about 4.9 KB stay free under softraid I/O (M10f measure).
- A `diagnostic` MP kernel panics at boot (`uvm_page_physload: page size not set!`).
- Statistics counters the C bumps unlocked stay `Cell`s (docs/ARCHITECTURE.md, M11e).
- QEMU's igb passes no traffic with em(4)'s legacy descriptors (M13; not checked in QEMU).

Decisions pending (the user's): the scope section; a SeaBIOS path for vga(4)'s text mode;
Limine's retirement; the PC's CPU for vmm (M17); the Raspberry Pi 4 model; networking in M17;
swtpm for M16's tpm(4); reporting QEMU's lost `sev` upstream.
