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
  (`just smoke`'s recipes N at a time, each in `target/smoke/<recipe>`, `smokeall.rs`), `ports {check,status,drift,next}`, `unsafe-report` (M12+: `unsafe` per subsystem, `unsafereport.rs`), `diff-openbsd` (M12+: EmiBSD against a real OpenBSD VM, `diffopenbsd.rs`), `gen-syscalls`, `symbolize`, `ntfs-image` (M10d: our NTFS test volume, `ntfsgen.rs`, checked by macOS's NTFS driver), `nvme-root` (M13a: the labelled NVMe root disk of `smoke-nvme`; `hwopts.rs`, which also holds M13's QEMU device options such as `--nvme`), `userland`
  (M8: OpenBSD's libc and programs cross-compiled from the reference sources, `userland.rs` over
  the make-subset evaluator `bsdmake.rs`).
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
