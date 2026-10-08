# Status

Milestone: **M14 done** (installable); M15 (code and test layout) next, then M16 (QEMU
drivers), M17 (real hardware, vmm). Updated: 2026-10-08.

Done:
- M7a..M13: uvm, network, file systems, SMP (`-smp 4` in every smoke), devices, diff-openbsd;
  NVMe/AHCI roots, ACPI on amd64, PSCI, RTC, em/re/vmx, the frame buffer console.
- M14: our efiboot and the kernel's boot(8) entry on both archs; bsd.rd, signed base and comp
  sets, install.sub with autoinstall; the installed disk boots to `login:` and runs
  `cc hello.c && ./a.out` (amd64, arm64, arm64 on ACPI); clang/lld, ld.so and shared libc;
  arm64 ACPI (acpipci, pluart at acpi). `just diff-openbsd`: 102 steps, 99 equal, 3 expected, on both archs.

Next:
- M15 (code and test layout, when no agents run in parallel); then M16, M17.
- Left by M14: the GPL parts of the sets (not in the clone), lldb, efi(4), base programs not
  built (printf, sort, head, find, ...), `__thrsleep`, rnd(4)'s constant seed.

Unsafe (`cargo xtask unsafe-report`): kernel 7547 blocks, 984 fn, 701 impl, 25 trait, 322 other; tests 790 more.

Blockers:
- amd64 kernel stacks are tight: about 4.9 KB stay free under softraid I/O (M10f measure).
- A `diagnostic` MP kernel panics at boot (`uvm_page_physload: page size not set!`).
- Statistics counters the C bumps unlocked stay `Cell`s (docs/ARCHITECTURE.md, M11e).
- QEMU's igb passes no traffic with em(4)'s legacy descriptors (M13; not checked in QEMU).

Decisions pending (the user's): the scope section; a SeaBIOS path for vga(4)'s text mode;
Limine's retirement (boot(8) boots the kernel now); the PC's CPU for vmm (M17); the Raspberry
Pi 4 model; networking in M17; swtpm for M16's tpm(4); reporting QEMU's lost `sev` upstream.
