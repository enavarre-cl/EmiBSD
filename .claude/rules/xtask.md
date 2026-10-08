---
paths:
  - "tools/xtask/**"
---

# xtask (host tooling)

- Host-only Rust with `std`. Never depends on `bsd` or `libkern`; never compiled for a bare target.
- Invoked as `cargo xtask <cmd>` (alias in `.cargo/config.toml`). The workspace root is derived
  from `CARGO_MANIFEST_DIR`, never from the current directory.
- Subcommands: `image` (FAT ESP with Limine + `/bsd`), `qemu` (run), `smoke` (run, assert serial
  lines and exit code), `smoke2` (two VMs on a private link, `twovm.rs`), `smoke-all`
  (`just smoke`'s recipes N at a time, each in `target/smoke/<recipe>`, `smokeall.rs`), `ports {check,status,drift,next}` (`check` also validates the zone markers of every `.rs` under `sys/` and `tools/`: `layout.rs`, M15), `unsafe-report` (M12+: `unsafe` per subsystem, `unsafereport.rs`), `diff-openbsd` (M12+: EmiBSD against a real OpenBSD VM, `diffopenbsd.rs`; its `powerbtn` (M16f) and `probe` (M16e) boot that OpenBSD alone: `probe --arch A [--ipmi] [--usb] [--usb-hc xhci|ehci] [--ukc CMD]... [--sh CMD]` with the smokes' device options (`--ipmi` so far, and `devices.rs`'s `--usb`/`--usb-hc`, M16b), `boot -c` and the UKC commands given, and records its dmesg and CMD's output in `<run dir>/<arch>/openbsd-probe.log`, which is how a driver is checked against what OpenBSD 8.0 does on the same machine), `gen-syscalls`, `symbolize`, `ntfs-image` (M10d: our NTFS test volume, `ntfsgen.rs`, checked by macOS's NTFS driver), `efiboot` and `efiboot-disk` (M14: BOOTX64.EFI from the efiboot ELF with the toolchain's llvm-objcopy, and `smoke-efiboot`'s MBR + disklabel + makefs FFS + ESP disk, `efiboot.rs`), `nvme-root` (M13a: the labelled root disk of `smoke-nvme` and `smoke-ahci`, `--root-dev sdNa` naming its fstab root when it is not `sd0a`; `hwopts.rs`, which also holds M13's QEMU device options: `--nvme`, `--ahci`, `--scsi-cd`, `--lsi FILE` with `--lsi-cd ISO`, `--reboot` for a guest reset that restarts the VM, `--vio-mq` for a multiqueue virtio-net vio0 (`mq=on`, MSI-X, intrmap), and M14's `--acpi` (arm64 `virt,acpi=on` with PCI virtio disks and NIC, booted through efiboot's ACPI tables: `smoke-acpi`), `--nic MODEL` replacing vio0 on the user network with QEMU's `e1000`, `e1000e` or `igb` (M13, em(4)), `rtl8139` (re(4)) or `vmxnet3` (vmx(4)), `--fb` for a GOP display (`ramfb` on arm64), `--screenshot-after LINE` for a QEMU `screendump` checked against the kernel's `selftest=fb` line (or, with `--screen-text ROW:COL:TEXT`, for TEXT on wsdisplay's grid that `selftest=wscons` describes), `--sendkey-after LINE --sendkeys KEYS` (repeatable) for QEMU monitor `sendkey`s on the guest keyboard once LINE is on the serial line (`smoke-kbd`), `--usb-mouse`, `--usb-tablet`, `--usb-wacom-tablet` and `--usb-ccid` (M16b: QEMU's USB pointers and smart card reader on the `qemu-xhci` bus, with or without `--usb`; `devices.rs`; pointer events then come from `--monitor-after LINE --monitor 'mouse_set N'`/`'mouse_move DX DY'`/`'mouse_button STATE'`), and `{host-ms}` in a `--send` text, the host clock as it is sent), `--audio usb` (M16b: QEMU's `usb-audio` on the `--usb-hc` controller's bus (full speed: xhci) into the `wav` backend, beside M12's `--audio hda|ac97`, so `--expect-tone` checks uaudio(4); `devices.rs`), the USB and audio devices of `devices.rs` (M12: `--usb` for a `qemu-xhci` with the FAT32 `usb-storage` stick and a `usb-kbd`, `--audio hda|ac97`, `--speakers`, `--expect-tone`; M16b: `--usb-hc xhci|ehci`, implying `--usb`, the host controller the stick sits on, `usb-ehci` for ehci(4) with the stick alone since QEMU refuses a full speed `usb-kbd` on a companionless EHCI; a new controller is one arm of `UsbHc`), `userland`
  (M8: OpenBSD's libc and programs cross-compiled from the reference sources, `userland.rs` over
  the make-subset evaluator `bsdmake.rs`), `comp` (M14: OpenBSD's clang, lld and libc++ from
  `gnu/llvm` by its build glue, `userland/comp.rs`; outside `ci`, `just comp`).
- External tools (`qemu-system-*`, Limine binaries, EDK2 firmware) are located at runtime via
  `brew --prefix <formula>` or `$PATH`, with an error that names the missing formula and points to
  `docs/SETUP.md`. Never hardcode `/opt/homebrew` or `/usr/local`. Exception: `userland`'s
  toolchain, approved by the user on 2026-10-03, defaults to `/usr/bin/clang` and
  `~/.swiftly/bin` (LLD 17, llvm-ar, llvm-ranlib, llvm-objcopy, llvm-objdump), overridable with
  `$EMIBSD_CC` and `$EMIBSD_LLVM_BIN`.
- Network: nothing downloads but `diff-openbsd fetch` (approved by the user on 2026-10-05):
  only the snapshot that `openbsd-snapshot.toml` records, only from the mirror it names, with
  macOS's `curl` and `shasum`, into `target/openbsd/`, refused unless both recorded hashes
  match. A new snapshot is the user's decision. Its binaries are test fixtures: never
  committed or redistributed. The OpenBSD VM's first boot has no network (`restrict=on`).
- Workspace lints apply here too: no `unwrap`/`expect`; return `Result` and print one clear
  `xtask: <what failed>: <why>` line. Non-zero exit on any error.
- Keep dependencies minimal: `serde` + `toml` for the tracker, `fatfs` for images when M0 lands.
  No `clap`; argument parsing is a `match` on `std::env::args`.
- Two commands write to the tree: `ports status --write`, only between the
  `<!-- ports:begin -->` / `<!-- ports:end -->` markers in `docs/PORTING.md`, and
  `unsafe-report --write`, only the line of `docs/STATUS.md` that starts with `Unsafe`.
