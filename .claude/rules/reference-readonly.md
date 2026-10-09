---
paths:
  - "reference/**"
---

# The reference trees are read-only

`reference/openbsd-src/` is a sparse, shallow clone of https://github.com/openbsd/src, pinned at the
commit in `reference/PINNED.md`. It holds `sys/` (the kernel: the specification the port follows)
and, since 2026-10-03 (the user's M8 decision), the userland sources M8 cross-compiles unmodified:
`lib/`, `bin/`, `sbin/`, `usr.bin/`, `libexec/` and `include/` (the `/usr/include` headers libc
needs), plus `gnu/lib/libcompiler_rt` and `gnu/llvm/compiler-rt` (the compiler runtime, Apache-2.0
WITH LLVM-exception, compiled unmodified; the user's decision of 2026-10-03), and `usr.sbin/makefs` (makefs(8), built as a host tool to
make the ffs ramdisk image; same decision date) and `usr.sbin/pwd_mkdb` (host tool for the ramdisk's
`pwd.db`/`spwd.db`, M8b) `usr.sbin/tcpdump` (compiled unmodified for the ramdisk, M9+), and `usr.sbin/portmap`,
`quotaon`, `edquota`, `repquota` (NFS and quotas, M10; all the user's decisions of 2026-10-03), and `usr.sbin/hostapd` (tcpdump's `iapp.h`) and `etc/` (OpenBSD's uids and gids for the ramdisk; both the user's decisions of 2026-10-04), and for M14 all of
`usr.sbin/`, `distrib/`, `share/`, `gnu/llvm` and OpenBSD's clang build glue (`gnu/usr.bin/clang`,
`gnu/lib/{libcxx,libcxxabi,libclang_rt}`; pre-approved by the user on 2026-10-04, added 2026-10-05). No other `gnu/`, no `xenocara/`. It is the specification, not part of the product.
Widening or narrowing the sparse set at the same pin is `git -C reference/openbsd-src
sparse-checkout add|set ...`, a user decision recorded in LZ's `docs/ROADMAP.md`; this tree
keeps the same set, at the pin LZ records at the pinned LZ commit.

`reference/emibsd-lz/` is a full-history clone of https://github.com/enavarre-cl/EmiBSD.LZ,
the faithful port this system derives from, pinned at the commit in `lz/PINNED.md`. It is the
shape every native module started from and the history `cargo xtask lz drift` reads. It is
gitignored, never edited, and never pushed to: LZ does not know this repository exists.

- Never edit, format, rename, delete or create files under `reference/`. `.claude/settings.json`
  denies Edit/Write there as a backstop; do not work around it.
- Never copy C verbatim into Rust and never transliterate line by line. Read, understand,
  re-express. Identifiers, constants, data layouts and the *meaning* of comments are preserved;
  the text is yours.
- When discussing C code with the user, cite it as `reference/openbsd-src/sys/<path>:<line>`;
  LZ code as `reference/emibsd-lz/sys/<path>:<line>`; native code as `sys/<path>:<line>`.
- Every file keeps the complete original copyright and licence block(s) of every LZ file it
  derives from, verbatim, as `/* ... */` comments inside `<LICENSES>` (a redesigned module
  carries the blocks of all its sources, `lineage.md`). The RCS ident lines are not kept
  (decision 20: CVS keywords, not licence text; provenance is `lineage.toml` and the
  `//! Upstream:` line). Then the `//!` docs.
  Never shorten, reword, relicense or add restrictions to license text. BSD-3 non-endorsement and
  ISC/BSD notice obligations apply to this project's distribution; `LICENSE` explains how.
- Every licence or notice in the pinned tree is accepted, kept whole (`scope-and-stubs.md`, the
  user's rule of 2026-10-04). Only code from outside this tree needs the user's decision.
- The OpenBSD pin is LZ's; this tree follows it through `lz/PINNED.md` (`lz-sync.md`).
  `cargo xtask lz check` asserts `reference/openbsd-src` is at the commit LZ records at the
  pinned LZ commit. Neither pin moves without the user.
- `git log` inside `reference/openbsd-src` is useless (depth 1). `reference/emibsd-lz` has the
  full history: drift is detected there by `git log`, not by blob hashes.
