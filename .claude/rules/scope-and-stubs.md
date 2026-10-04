# Scope and stubs

The kernel is ported incrementally; most of OpenBSD is not here yet. These rules keep that honest.

- Never `todo!()`, `unimplemented!()` or an empty body standing in for real work. Both lints are
  denied workspace-wide; do not `#[allow]` them.
- When a ported file calls into an unported subsystem, make the gap explicit and visible, either:
  - `return Err(unported!("uvm_map"))` (`sys/kern/unported.rs`), a macro that prints the gap once
    per site and yields `Errno::ENOSYS`; or
  - gate the whole path behind a cargo feature mirroring the OpenBSD `option(4)`
    (`diagnostic`, `multiprocessor`, ...), documented in `sys/Cargo.toml`.
- Every stub is recorded twice: in the file's `//! ## Deviations` list and in the entry's `notes`
  in `ports.toml`.
- Never delete a code path because "we don't need it yet". Port it or stub it visibly.
- Drivers for hardware QEMU does not expose: `status = "skipped"`, `notes = "deferred-driver: ..."`.
- Licences (the user's rule of 2026-10-04, replacing the list of per-licence decisions): any
  licence or notice on a file in the pinned OpenBSD tree (`reference/openbsd-src`) is accepted
  without asking, for kernel ports and for the compiled userland alike; OpenBSD already accepted
  it into its tree. That covers ISC, BSD (2, 3, 4 clauses), MIT, Mach, beerware, Dyson, zlib,
  public domain, the HPND-style notices (M.I.T., Carnegie Mellon, OSF, TRW, the IPsec authors),
  LibreSSL's OpenSSL/SSLeay, Apache-2.0 WITH LLVM-exception, files with no licence text, and any
  other. The conditions:
  - The original notice is kept whole between `/* <LICENSES> */` and `/* </LICENSES> */`; a file
    without licence text keeps its copyright lines as they are. Never shorten, reword or
    relicense it. A zlib port says it is an altered version (zlib clause 2).
  - `ports.toml` `notes` names the licence when it is not ISC, BSD or MIT
    (e.g. `license: OSF notice, kept whole; ...`).
  - `LICENSE` lists the licence families present; a new family is added there in the same commit.
- Code that does not come from the pinned OpenBSD tree (ZFS, XFS, external libraries, ...) is
  outside that rule: its licence is decided by the user when the milestone that brings it is
  proposed. Until then: stop, tell the user, do not port it (`status = "skipped"`,
  `notes = "license: <which>"`). A translation is still a derivative work, so rewriting does not
  escape a licence.
- Code replaced by a project-level decision (bootloader, build system, `config(8)`):
  `status = "skipped"`, `notes = "replaced-by-<what>: ..."`, and the decision is in `docs/ARCHITECTURE.md`.
- Scope changes (dropping an arch, skipping a subsystem, changing the boot protocol) are the
  user's decision. Propose; do not decide.
