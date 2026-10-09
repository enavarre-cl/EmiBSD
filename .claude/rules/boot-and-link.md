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
  boot(8)'s entry (`sys/stand/bootarg.rs`, called by both archs' `locore0.S`) takes its
  `BootInfo` from `machine::Cpu::getbootinfo` and shares the tail (`stand::start_kernel`).
  Limine's `_start` is not `#[no_mangle]`: the symbol `_start` is arm64's `locore0.S` entry.
- `sys/arch/{amd64,arm64}/conf/kernel.ld`: `PHDRS` text/rodata/data (M16d: and the C's
  `openbsd_randomize` `PT_OPENBSD_RANDOMIZE` header over `.openbsd.randomdata` inside rodata,
  which boot(8) fills with the seed; `etext` on both archs), `.requests*` kept,
  `.eh_frame*`/`.note*` discarded. arm64 (M14, so efiboot can load it): base
  `0xffffffff80000000`, physical addresses from 0 by `AT()` (efiboot's `LOADADDR` keeps 39
  bits and adds its 64 MB block), `.text.locore0` first, `__bss_start`/`_end`/`end` for
  `locore0.S`, `ENTRY(__start_phys)` (`_start`'s offset: `e_entry` goes through `LOADADDR`
  too). amd64 (M14, so efiboot can
  load it): OpenBSD's `ld.script` layout, `KERNTEXTOFF` `0xffffffff81000000`, physical addresses
  from `0x1000000` by `AT()`, page-aligned sections with the symbols `locore0.S` reads, `.got`
  inside `.data` (nothing after `end`), `ENTRY(start)` (the 32-bit boot(8) entry); Limine enters
  `_start` through its entry point request. The two scripts no longer have to be identical.
  `sys/build.rs` passes the script with `rustc-link-arg-bins` only when `target_os = "none"`.
- `.cargo/config.toml` never sets `[build] target`: a plain `cargo test`/`cargo check` must build
  for the host. Kernel builds always name the target through `just`.
- Rustflags are per target: `relocation-model=static` (non-PIE higher-half kernel) and
  `force-frame-pointers=yes` (panic backtraces). Adding a flag needs a line in `docs/ARCHITECTURE.md`.
- `rust-toolchain.toml` pins a stable version. Bumping it is its own commit titled
  `build: bump toolchain to <version>` and needs the user's OK. Never add nightly or `-Z` flags.
- `limine.conf` is shared by both archs; the kernel is `/bsd` on the ESP, like OpenBSD's `/bsd`.
