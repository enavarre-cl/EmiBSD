# Scope and stubs

The kernel is ported incrementally; most of OpenBSD is not here yet. These rules keep that honest.

- Never `todo!()`, `unimplemented!()` or an empty body standing in for real work. Both lints are
  denied workspace-wide; do not `#[allow]` them.
- When a ported file calls into an unported subsystem, make the gap explicit and visible, either:
  - `return Err(kern::unported::unported("uvm_map"))`, a helper that logs the gap once and yields
    an `Errno`; or
  - gate the whole path behind a cargo feature mirroring the OpenBSD `option(4)`
    (`diagnostic`, `multiprocessor`, ...), documented in `sys/Cargo.toml`.
- Every stub is recorded twice: in the file's `//! ## Deviations` list and in the entry's `notes`
  in `ports.toml`.
- Never delete a code path because "we don't need it yet". Port it or stub it visibly.
- Drivers for hardware QEMU does not expose: `status = "skipped"`, `notes = "deferred-driver: ..."`.
- Code whose license is not ISC, BSD-2/3-Clause or MIT: stop, tell the user, record
  `status = "skipped"`, `notes = "license: <which>"`. Do not port it.
- Code replaced by a project-level decision (bootloader, build system, `config(8)`):
  `status = "skipped"`, `notes = "replaced-by-<what>: ..."`, and the decision is in `docs/ARCHITECTURE.md`.
- Scope changes (dropping an arch, skipping a subsystem, changing the boot protocol) are the
  user's decision. Propose; do not decide.
