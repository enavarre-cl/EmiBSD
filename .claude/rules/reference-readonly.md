---
paths:
  - "reference/**"
---

# The OpenBSD reference tree is read-only

`reference/openbsd-src/` is a sparse, shallow clone of https://github.com/openbsd/src, pinned at the
commit in `reference/PINNED.md`. It holds `sys/` (the kernel: the specification the port follows)
and, since 2026-10-03 (the user's M8 decision), the userland sources M8 cross-compiles unmodified:
`lib/`, `bin/`, `sbin/`, `usr.bin/`, `libexec/` and `include/` (the `/usr/include` headers libc
needs), plus `gnu/lib/libcompiler_rt` and `gnu/llvm/compiler-rt` (the compiler runtime, Apache-2.0
WITH LLVM-exception, compiled unmodified; the user's decision of 2026-10-03), and `usr.sbin/makefs` (makefs(8), built as a host tool to
make the ffs ramdisk image; same decision date) and `usr.sbin/pwd_mkdb` (host tool for the ramdisk's
`pwd.db`/`spwd.db`, M8b) and `usr.sbin/tcpdump` (compiled unmodified for the ramdisk, M9+; the
user's decision of 2026-10-03). No other `gnu/`, no `xenocara/`. It is the specification, not part of the product.
Widening or narrowing the sparse set at the same pin is `git -C reference/openbsd-src
sparse-checkout add|set ...`, a user decision recorded in `docs/ROADMAP.md`.

- Never edit, format, rename, delete or create files under `reference/`. `.claude/settings.json`
  denies Edit/Write there as a backstop; do not work around it.
- Never copy C verbatim into Rust and never transliterate line by line. Read, understand,
  re-express. Identifiers, constants, data layouts and the *meaning* of comments are preserved;
  the text is yours.
- When discussing C code with the user, cite it as `reference/openbsd-src/sys/<path>:<line>`.
- Every ported `.rs` file starts with the original `/* $OpenBSD: ... $ */` line and the complete
  original copyright and license block, verbatim, as `/* ... */` comments. Then the `//!` docs.
  Never shorten, reword, relicense or add restrictions to license text. BSD-3 non-endorsement and
  ISC/BSD notice obligations apply to this project's distribution; `LICENSE` explains how.
- Licenses found in OpenBSD `sys/` are ISC, BSD-2/3-Clause and MIT. Anything else, or a file with
  no license block: stop and ask the user (`scope-and-stubs.md`).
- Updating the pin is a deliberate act (`reference/README.md`, `docs/PORTING.md`): fetch, check out,
  run `cargo xtask ports drift`, update `PINNED.md` and `ports.toml [meta].pinned` in one commit.
- `git log` inside the clone is useless (depth 1). Drift is detected by blob hash, not history.
