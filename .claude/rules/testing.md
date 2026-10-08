# Testing

Four tiers. Every change lands with the tier it belongs to.

1. **Host unit tests**: `just test` (`cargo test -p libkern -p bsd`). Pure logic (libkern, errno,
   page-allocator math, queue adapters, formatting) runs on macOS through `sys/arch/host`.
   Every new `pub fn` with testable logic gets a test in the same file: an inline
   `#[cfg(test)] mod tests` in the `<TESTS>` zone at the end, whatever its length (there is no
   `tests.rs`; `rust-kernel.md`, file layout). `cargo xtask ports check` rejects a `tests.rs`, a
   `mod tests` outside `<TESTS>` and a `<TESTS>` zone without one.
   Table-driven tests for C-compatible behaviour (`strlcpy` return values, `crc32` vectors).
2. **Reference-backed tests**: `just test-ref`. Marked `#[ignore]`; they read `$OPENBSD_SRC`
   (set by the recipe to `reference/openbsd-src`) and cross-check constants against the C headers
   (`errno.h`, `param.h`, syscall numbers). They parse simple `#define` lines, nothing more.
3. **QEMU smoke tests**: `just smoke`. Boot both archs headless, assert serial lines and the exit
   code (amd64 `isa-debug-exit`, arm64 semihosting). Every change to boot, console, traps or
   scheduling adds or updates an expectation in `tools/xtask`.
   Every smoke and smoke2 run boots the `multiprocessor` kernel (recipes built with
   `--features qemu,multiprocessor`). Since the user's decision of 2026-10-07 (replacing the
   `-smp 4` everywhere of 2026-10-03) the CPU count is the justfile's `smp` variable,
   `-smp 2` by default (`ncpu`, from `EMIBSD_NCPU`, 2 or 4): every MP bug found so far needs
   only two CPUs. An expectation that names the count (`bsd: N processors`, `hw.ncpu=`,
   `cpuN at mainbus0`, `N of N application processors`, ...) follows `ncpu` through the
   justfile's derived variables (`aps`), so the recipe passes on 2 and on 4. A fixed group
   stays on `-smp 4` (`smp4`) to stress MP whatever `ncpu` says, with literal 4-CPU
   expectations: `smoke-mp` (mpstress, the kthread ping-pong), the multi-queue network
   smokes (`smoke-vmx`, `smoke-net-mp`, whose `-smp 8` boot stays as is) and
   `smoke-softraid` (the disk smoke under load). `smoke-up` is the one uniprocessor boot per
   arch, kept to catch a dependency on MP. New smokes follow suit: MP, `{{smp}}`, both archs;
   `{{smp4}}` only by the user's decision.
   `just ci-full` is `ci` with every `{{smp}}` recipe on four CPUs (`EMIBSD_NCPU=4`), then
   the installer end to end on both archs, also on four CPUs (the user's decision of
   2026-10-07): `smoke-install-amd64`, `smoke-install-arm64` (with `smoke-install-arm64-acpi`)
   and `smoke-install-boot-amd64`, `-arm64`, `-arm64-acpi`, which make the install media
   afresh from the tree, install to a fresh disk and boot the installed system; so it needs
   `just userland` and `just comp`. Two CPUs make the races rarer and nothing else checks the
   installer, so `ci-full` is mandatory before a milestone is marked met, and its result (rc
   and wall time) goes in the milestone's closing commit. A milestone split into lettered
   sub-milestones (M16a..M16g, the user's decision of 2026-10-08) runs it once, at the
   whole milestone's close; each sub-milestone closes with `just ci`.
   `just smoke` runs the recipes of the justfile's `smokes` list in parallel, `JOBS` at a time
   (default 4; `cargo xtask smoke-all`, docs/ARCHITECTURE.md "Parallel smokes"). So a smoke
   recipe: is added to `smokes`; builds nothing in its body (what it boots is built by its
   dependencies and by `smoke-build`, which runs alone first); writes per-run files only
   through xtask (`EMIBSD_RUN_DIR` puts them in `target/smoke/<recipe>/`), never a shared
   path under `target/`; and depends on no other recipe's disks or order. A recipe that
   needs a fixed host port has it to itself; a time limit is not tightened to fit a quiet
   machine (`EMIBSD_TIMEOUT_SCALE` scales them under parallel load).
4. **Differential tests** (M12+): `just diff-openbsd`, beside `ci`, not in it. The same
   scenarios on EmiBSD and on the OpenBSD snapshot of `tools/xtask/openbsd-snapshot.toml`
   (docs/ARCHITECTURE.md "diff-openbsd"). Run it before closing a milestone and after any
   change to system calls, VFS or a file system. A new difference is a bug to fix in its own
   commit, or, only when EmiBSD must differ (an unported part with its visible stub, the
   branding, the snapshot's gap to the pin), an entry in `tools/xtask/diff-openbsd/
   expected.toml` with its reason, kept alone in its own step. An unported part that gets
   ported removes its entry. Scenario steps print only what is deterministic, or normalize it.

Always:

- `just clippy` runs clippy for amd64, arm64 AND host with `-D warnings`. All three must pass.
- `just fmt` is `cargo fmt --all -- --check`.
- `just ci` runs everything and is the definition of green.
- Test code may use `unwrap`/`expect`/`panic!` (allowed via `clippy.toml`); kernel code may not.
- A test that needs `alloc` enables the `alloc` feature explicitly until M3 makes it default.
- Never weaken or delete a failing test to get green. Fix the code or tell the user.
