---
paths:
  - "reference/**"
---

# The OpenBSD reference tree is read-only

`reference/openbsd-src/` is a sparse, shallow clone of https://github.com/openbsd/src (only `sys/`),
pinned at the commit in `reference/PINNED.md`. It is the specification, not part of the product.

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
