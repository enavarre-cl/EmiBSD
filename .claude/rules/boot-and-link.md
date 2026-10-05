---
paths:
  - "sys/stand/**"
  - "sys/arch/*/conf/**"
  - "sys/build.rs"
  - "sys/main.rs"
  - ".cargo/**"
  - "rust-toolchain.toml"
---

# Boot and link

- Boot protocol is Limine (base revision 6) on UEFI for both archs until OpenBSD's own boot loader
  boots the same kernel (M14, the user's decision): `boot(8)`/efiboot are being ported
  (`sys/lib/libsa`, `sys/stand/boot`, `sys/stand/efi`, `sys/arch/<arch>/stand/efiboot`;
  docs/ARCHITECTURE.md, "Boot loaders"), with the kernel's own `bootarg` entry beside Limine's.
  No Multiboot, no Linux image header, no direct `-kernel` loading.
- efiboot is built for the `none` targets (no UEFI target), position-independent:
  `RUSTFLAGS="-C relocation-model=pie"` in `just efiboot-<arch>`, its own `--target-dir
  target/efiboot`, its `ldscript.<arch>` with the hand-written PE header first, and
  `cargo xtask efiboot` (`llvm-objcopy -O binary`). Code that runs before `self_reloc` may not
  use statics, the GOT, formatting or panics.
- Limine requests are `#[used] static`s in `.requests`, bracketed by the start/end marker sections,
  all referenced from `_start`. The linker script `KEEP`s them.
- `sys/stand/limine.rs` holds the protocol structs (base revision tag, request/response
  `#[repr(C)]` layouts, magic IDs), written from the Limine protocol specification
  (https://github.com/limine-bootloader/limine-protocol, `PROTOCOL.md` and `include/limine.h`),
  not from a crate. `sys/stand/` converts responses into `stand::BootInfo` (arch-neutral: memory
  map, HHDM offset, DTB/RSDP pointers, framebuffer, modules) and hands that to
  `machine::Machine::early_init`, then `kern::init_main::main`. Nothing else sees Limine types.
- `sys/arch/{amd64,arm64}/conf/kernel.ld` are byte-identical except `OUTPUT_FORMAT`. Kernel base
  `0xffffffff80000000`, `PHDRS` text/rodata/data, `.requests*` kept, `.eh_frame*`/`.note*` discarded.
  `sys/build.rs` passes the script with `rustc-link-arg-bins` only when `target_os = "none"`.
- `.cargo/config.toml` never sets `[build] target`: a plain `cargo test`/`cargo check` must build
  for the host. Kernel builds always name the target through `just`.
- Rustflags are per target: `relocation-model=static` (non-PIE higher-half kernel) and
  `force-frame-pointers=yes` (panic backtraces). Adding a flag needs a line in `docs/ARCHITECTURE.md`.
- `rust-toolchain.toml` pins a stable version. Bumping it is its own commit titled
  `build: bump toolchain to <version>` and needs the user's OK. Never add nightly or `-Z` flags.
- `limine.conf` is shared by both archs; the kernel is `/bsd` on the ESP, like OpenBSD's `/bsd`.
