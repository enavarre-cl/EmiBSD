# Status

Milestone: **M12 done** (devices in QEMU); **M12+ done** (measurement and verification); M13
under way. Updated: 2026-10-05.

Done:
- M7a..M11: uvm, network, file systems, ffs root, login, wg, IPsec, pf; SMP (every smoke boots
  the MP kernel with `-smp 4`; `smoke-up` is UP).
- M12: audio(4) (azalia, auich), the USB core, xhci, umass, ukbd; arm64's PCI bus.
- M12+: `cargo xtask unsafe-report` (below); `docs/JOURNAL.md`, a section per milestone;
  `just diff-openbsd`: 102 steps against the OpenBSD 8.0 snapshot of 2026-10-03/04, 99 equal,
  3 expected (fifofs, core dumps, branding), on both archs.

Next:
- M13 (storage, firmware, console; nvme, vioscsi and cd are in), then M14, M15 (code and test
  layout), M16 (QEMU drivers), M17 (real hardware, vmm).
- M13 picks up M12's leftovers: wskbd and ukbdmap.c (the keyboard is silent until then).

Unsafe (`cargo xtask unsafe-report`): kernel 5972 blocks, 723 fn, 578 impl, 22 trait, 129 other; tests 629 more.

Blockers:
- amd64 kernel stacks are tight: about 4.9 KB stay free under softraid I/O (M10f measure).
- A `diagnostic` MP kernel panics at boot (`uvm_page_physload: page size not set!`).
- Statistics counters the C bumps unlocked stay `Cell`s (docs/ARCHITECTURE.md, M11e).
- arm64 configures azalia although its GENERIC does not (QEMU's HD Audio; ROADMAP M12).

Decisions pending (the user's): the scope section (open until M13); the PC's CPU for vmm (M17);
the Raspberry Pi 4 model; networking in M17; swtpm for M16's tpm(4); reporting QEMU's lost
`sev` upstream.
