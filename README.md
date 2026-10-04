# EmiBSD

<p align="center">
  <img src="docs/Ferffy.jpg" alt="Ferffy, the EmiBSD mascot: a spiky orange crab in front of a shield, above the EmiBSD name" width="600">
</p>

<p align="center"><em>Ferffy, the EmiBSD mascot: half Ferris (Rust's crab), half Puffy (OpenBSD's pufferfish).</em></p>

The OpenBSD kernel, re-implemented in Rust, one file at a time.

- Standalone `#![no_std]` kernel, booted by [Limine](https://github.com/limine-bootloader/limine),
  running in QEMU on amd64 and arm64.
- The OpenBSD C sources (`reference/openbsd-src/sys`, read-only) are the specification.
  The Rust tree mirrors them path for path.
- Stable Rust only.

Status: M5 done (boot, memory, traps, clocks, processes and a scheduler with kernel threads on amd64 and arm64 under QEMU); M6 (syscalls, a minimal init) is next. See `docs/STATUS.md` and `docs/ROADMAP.md`.

| Question | Where |
|---|---|
| How do I set up the toolchain on macOS? | `docs/SETUP.md` |
| Why is it built this way? | `docs/ARCHITECTURE.md` |
| How does a C file become a Rust file? | `docs/PORTING.md`, tracker in `ports.toml` |
| How is this C idiom written in Rust? | `docs/C_TO_RUST.md` |
| What comes next? | `docs/ROADMAP.md` |

Build and run: `just` lists every recipe (`just build`, `just run-amd64`, `just test`, `just ci`).

License: ISC for new code; ported files keep their OpenBSD license. See `LICENSE`.
