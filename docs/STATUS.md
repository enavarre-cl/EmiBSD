# Status

Milestone: **pre-M0 (bootstrap)**. Updated: 2026-10-02.

Done:
- Workspace skeleton (`bsd`, `libkern`, `xtask`) compiles and tests on the host.
- `CLAUDE.md`, `.claude/rules/`, `docs/` written; `ports.toml` seeded with M1/M2 files.

Next:
- `docs/SETUP.md`: install rustup, QEMU, Limine, just (needs the user's go-ahead).
- Clone and pin the reference (`reference/README.md`); fill `PINNED.md` and `[meta].pinned`.
- M0: Limine boot on both archs printing `bsd: booted on <arch>`; `xtask image/qemu/smoke`.

Blockers:
- Bare-metal targets cannot be built until rustup replaces Homebrew's rust.
