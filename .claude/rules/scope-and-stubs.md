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
- Code whose license is not ISC, BSD (2, 3 or 4 clauses; the 4-clause advertising clause was
  accepted by the user at M2 for `comvar.h` and amd64 `bus.h`), MIT or Mach (Carnegie Mellon, the
  `ddb/` and `db_*` files, also accepted at M2) or beerware (Poul-Henning Kamp's `kern_tc.c`,
  accepted by the user at M5) or John S. Dyson's licence (`sys_pipe.c`, `pipe.h`, accepted by
  the user on 2026-10-03): stop, tell the user, record `status = "skipped"`,
  `notes = "license: <which>"`. Do not port it. A port in another language is still a derivative
  work, so a skipped file's license is not escaped by rewriting it; route around it (use what a
  permissive file defines, write interfaces from the manual page) or ask.
- Code replaced by a project-level decision (bootloader, build system, `config(8)`):
  `status = "skipped"`, `notes = "replaced-by-<what>: ..."`, and the decision is in `docs/ARCHITECTURE.md`.
- Compiled-not-ported userland (M8) also accepts, by the user's decision of 2026-10-03:
  Apache-2.0 WITH LLVM-exception (`gnu/llvm/compiler-rt`), public domain (pdksh), files with no
  licence text, the Lucent (gdtoa), Birgmeier (rand48), SunPro (fdlibm), Cheusov (`wcsdup.c`)
  and Boulet/RTMX (`sys/msg.h`) notices, and the Unicode data-files licence (makefs's
  `msdosfs_unicode.c`). Kernel ports still follow the list above.
- Scope changes (dropping an arch, skipping a subsystem, changing the boot protocol) are the
  user's decision. Propose; do not decide.
