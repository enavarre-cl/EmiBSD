# Testing

Three tiers. Every change lands with the tier it belongs to.

1. **Host unit tests**: `just test` (`cargo test -p libkern -p bsd`). Pure logic (libkern, errno,
   page-allocator math, queue adapters, formatting) runs on macOS through `sys/arch/host`.
   Every new `pub fn` with testable logic gets a test in the same file (`#[cfg(test)] mod tests`),
   or in `<name>/tests.rs` once the tests are longer than 50 lines (`rust-kernel.md`, file layout).
   Table-driven tests for C-compatible behaviour (`strlcpy` return values, `crc32` vectors).
2. **Reference-backed tests**: `just test-ref`. Marked `#[ignore]`; they read `$OPENBSD_SRC`
   (set by the recipe to `reference/openbsd-src`) and cross-check constants against the C headers
   (`errno.h`, `param.h`, syscall numbers). They parse simple `#define` lines, nothing more.
3. **QEMU smoke tests**: `just smoke`. Boot both archs headless, assert serial lines and the exit
   code (amd64 `isa-debug-exit`, arm64 semihosting). Every change to boot, console, traps or
   scheduling adds or updates an expectation in `tools/xtask`.
   Since M11 (the user's decision of 2026-10-03) every smoke and smoke2 run boots the
   `multiprocessor` kernel with `-smp 4` (the justfile's `smp` variable, recipes built with
   `--features qemu,multiprocessor`); `smoke-up` is the one uniprocessor boot per arch, kept
   to catch a dependency on MP. New smokes follow suit: MP, `{{smp}}`, both archs.

Always:

- `just clippy` runs clippy for amd64, arm64 AND host with `-D warnings`. All three must pass.
- `just fmt` is `cargo fmt --all -- --check`.
- `just ci` runs everything and is the definition of green.
- Test code may use `unwrap`/`expect`/`panic!` (allowed via `clippy.toml`); kernel code may not.
- A test that needs `alloc` enables the `alloc` feature explicitly until M3 makes it default.
- Never weaken or delete a failing test to get green. Fix the code or tell the user.
