---
paths:
  - "tools/xtask/**"
---

# xtask (host tooling)

- Host-only Rust with `std`. Never depends on `bsd` or `libkern`; never compiled for a bare target.
- Invoked as `cargo xtask <cmd>` (alias in `.cargo/config.toml`). The workspace root is derived
  from `CARGO_MANIFEST_DIR`, never from the current directory.
- Subcommands: `image` (FAT ESP with Limine + `/bsd`), `qemu` (run), `smoke` (run, assert serial
  lines and exit code), `smoke2` (two VMs on a private link, `twovm.rs`), `ports {check,status,drift,next}`, `gen-syscalls`, `symbolize`, `userland`
  (M8: OpenBSD's libc and programs cross-compiled from the reference sources, `userland.rs` over
  the make-subset evaluator `bsdmake.rs`).
- External tools (`qemu-system-*`, Limine binaries, EDK2 firmware) are located at runtime via
  `brew --prefix <formula>` or `$PATH`, with an error that names the missing formula and points to
  `docs/SETUP.md`. Never hardcode `/opt/homebrew` or `/usr/local`. Exception: `userland`'s
  toolchain, approved by the user on 2026-10-03, defaults to `/usr/bin/clang` and
  `~/.swiftly/bin` (LLD 17, llvm-ar, llvm-ranlib, llvm-objcopy, llvm-objdump), overridable with
  `$EMIBSD_CC` and `$EMIBSD_LLVM_BIN`.
- Workspace lints apply here too: no `unwrap`/`expect`; return `Result` and print one clear
  `xtask: <what failed>: <why>` line. Non-zero exit on any error.
- Keep dependencies minimal: `serde` + `toml` for the tracker, `fatfs` for images when M0 lands.
  No `clap`; argument parsing is a `match` on `std::env::args`.
- `ports status --write` is the only command that writes to the tree, and only between the
  `<!-- ports:begin -->` / `<!-- ports:end -->` markers in `docs/PORTING.md`.
