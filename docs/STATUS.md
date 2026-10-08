# Status

Milestone: **M13 done** (storage, firmware and console); M14 (installable) next, then M15
(code and test layout), M16 (QEMU drivers), M17 (real hardware, vmm). Updated: 2026-10-07.

Done:
- M7a..M11: uvm, network, file systems, ffs root, login, wg, IPsec, pf; SMP (every smoke boots
  the MP kernel with `-smp 4`; `smoke-up` is UP).
- M12, M12+: audio, USB, arm64's PCI bus; unsafe-report, JOURNAL, `just diff-openbsd`.
- M13: root on NVMe and AHCI (both archs), siop, cd; ACPI on amd64 (MADT, I/O APIC, MSI, S5,
  timers); PSCI, RTC, puc; em, re, vmx; efifb/simplefb, rasops, wsdisplay, wskbd, wsmux,
  wsmouse, vga. `just diff-openbsd`: 102 steps, 99 equal, 3 expected, on both archs.

Next:
- M14 (installable; its work runs in parallel and is mostly on main).
- Left by M13: arm64 ACPI (M14, with efiboot's tables), acpicpu(4), a frame buffer console.

Unsafe (`cargo xtask unsafe-report`): kernel 7467 blocks, 975 fn, 689 impl, 24 trait, 322 other; tests 786 more.

Blockers:
- amd64 kernel stacks are tight: about 4.9 KB stay free under softraid I/O (M10f measure).
- A `diagnostic` MP kernel panics at boot (`uvm_page_physload: page size not set!`).
- Statistics counters the C bumps unlocked stay `Cell`s (docs/ARCHITECTURE.md, M11e).
- arm64 configures azalia although its GENERIC does not (QEMU's HD Audio; ROADMAP M12).
- QEMU's igb passes no traffic with em(4)'s legacy descriptors (M13; not checked in QEMU).

Decisions pending (the user's): the scope section (due at M13's close); a SeaBIOS boot path
for vga(4)'s text mode; the PC's CPU for vmm (M17); the Raspberry Pi 4 model; networking in
M17; swtpm for M16's tpm(4); reporting QEMU's lost `sev` upstream.
