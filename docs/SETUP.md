# Setup (macOS, Apple Silicon)

Verified 2026-10-02 on Darwin 25.6 with Homebrew. Linux users: `rustup`, `qemu`, `limine`, `just`
from your distribution; the rest is identical.

## Why the Homebrew `rust` formula is not enough

Homebrew's `rust` ships only the host target (`aarch64-apple-darwin`) and no `rust-lld`.
The bare-metal targets `x86_64-unknown-none` and `aarch64-unknown-none-softfloat` need both.
`rustup` manages them; in Homebrew it is keg-only because it conflicts with the `rust` formula.
(An alternative that keeps Homebrew rust, `rustup toolchain link system "$(brew --prefix rust)"`,
still lacks the bare targets. Not recommended.)

## Steps

1. Remove Homebrew rust. Nothing else depends on it (`brew uses --installed rust` is empty):
   ```sh
   brew uninstall rust
   ```
2. Install the tools:
   ```sh
   brew install rustup qemu limine just
   brew install x86_64-elf-gdb aarch64-elf-gdb   # optional, for `just gdb-*` later
   ```
3. Put rustup and cargo on `PATH`. Add to `~/.zshrc`, then open a new shell:
   ```sh
   export PATH="$HOME/.cargo/bin:$(brew --prefix rustup)/bin:$PATH"
   ```
4. Install a default toolchain once:
   ```sh
   rustup default stable
   ```
5. In the repository, the first `cargo` command installs the pinned toolchain, both bare-metal
   targets and the components listed in `rust-toolchain.toml` automatically.

## Verify

```sh
which cargo                        # ~/.cargo/bin/cargo or the rustup keg, NOT /opt/homebrew/bin
cargo --version                    # the version pinned in rust-toolchain.toml
rustup target list --installed     # x86_64-unknown-none, aarch64-unknown-none-softfloat
ls "$(brew --prefix qemu)/share/qemu/"edk2-{x86_64,aarch64}-code.fd
ls "$(brew --prefix limine)/share/limine/"BOOT{X64,AA64}.EFI
cargo check && cargo test          # host build of bsd + libkern
cargo check -p bsd --target x86_64-unknown-none
cargo check -p bsd --target aarch64-unknown-none-softfloat
just smoke                         # after milestone M0
```

## Editor

rust-analyzer works out of the box on the host target thanks to `sys/arch/host`. To analyse
arch-specific code, set `rust-analyzer.cargo.target` to one of the bare targets temporarily.
`just clippy` checks all three targets regardless of editor settings.

## What `just` expects

- `qemu-system-x86_64`, `qemu-system-aarch64` on `PATH`.
- EDK2 firmware and Limine binaries located via `brew --prefix qemu` / `brew --prefix limine`
  at runtime by `xtask` (no hardcoded paths).
