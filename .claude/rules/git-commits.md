# Git and commits

- Commit only when the user asks. Never force-push, never amend a published commit.
- Subject: `<scope>: <imperative summary>`, at most 72 characters, lowercase scope, no trailing period.
  Scopes: `kern`, `uvm`, `sys`, `libkern`, `arch/amd64`, `arch/arm64`, `machine`, `dev/<dir>`,
  `ddb`, `stand`, `xtask`, `docs`, `rules`, `build`, `reference`.
  Examples: `kern: port subr_prf.c (printf, panic)`, `arch/arm64: add PL011 polled console`,
  `reference: bump OpenBSD pin to 3ce1f3f79392`.
- Body: what the C did, what the Rust does differently and why. Mention stubs and skipped paths.
- Trailers, in this order:
  - `Upstream: sys/kern/subr_prf.c@3ce1f3f79392`, one line per ported C file, 12-hex commit.
  - `Co-Authored-By: ...` lines as required by the session.
- One port (file or coherent cluster) per commit. `ports.toml` and docs changes ride with the
  commit that made them necessary.
- Toolchain bumps (`rust-toolchain.toml`) and reference pin bumps are their own commits.
- Never commit `reference/openbsd-src/`, `target/`, `*.img` or editor files. `.gitignore` covers
  them; check `git status` before committing anyway.
- Default branch is `main`. If the user wants branches: `port/<scope>-<name>` or `m<N>/<topic>`.
