# Porting workflow

Applies to every port of a C file from `reference/openbsd-src/sys/` into `sys/`.

- Before writing a line: read the `.c` file completely, its header(s), and the `(9)` man page
  comments it references. Note locking/SPL assumptions, error paths and `#ifdef` options.
  Big files are read in ranges and ported by a subagent of their own (`large-ports.md`).
- List every function, struct, macro and global the file uses. For each, check `ports.toml`:
  `ported`, `wip`, `todo`, or missing (add it as `todo`). Port leaf dependencies first;
  `cargo xtask ports next` shows what is unblocked.
- One `.c` → one `.rs` with the same name in the same directory. Several trivially small `.c`
  files may share one `.rs`; then each of them gets its own `ports.toml` entry pointing at it.
- Keep OpenBSD function names verbatim (`uvm_fault`, `sys_open`, `pmap_enter`). Types become
  CamelCase (`struct vm_map_entry` → `VmMapEntry`). Constants keep their names.
- Types defined in a header go to `sys/sys/<header>.rs` (or the arch `include/`); functions go to
  the module of the `.c` file, as `impl` blocks or free functions.
- Re-express semantics in idiomatic Rust; do not transliterate. `docs/C_TO_RUST.md` is the idiom
  table. A new idiom decision is a new row there, in the same commit.
- Header of every ported file: original `$OpenBSD$` line, full original copyright/license block
  between `/* <LICENSES> */` and `/* </LICENSES> */`, then, inside `/* <CODE> */` ...
  `/* </CODE> */`, the `//!` docs with `Upstream: <c path> @ <12-hex>` and a `## Deviations`
  list. When reading a ported file, read the code zone (`sed -n '/<CODE>/,/<\/CODE>/p' file`);
  the licence text never changes. A C file with no licence text gets no LICENSES zone and
  `license = "none"` on its `ports.toml` entry.
- Logic that can run on the host gets an inline `#[cfg(test)] mod tests { .. }` inside the
  `/* <TESTS> */` ... `/* </TESTS> */` zone at the end of the same file, however long it is
  (no `tests.rs`); constants that mirror C headers get a reference-backed `#[ignore]` test
  (see `testing.md`).
- Mark the entry `wip` when starting; `ported` only when `just ci` is green. Fill
  `upstream_commit` (= `[meta].pinned`) and `upstream_blob`
  (`git -C reference/openbsd-src rev-parse HEAD:<c path>`).
- One commit per file or coherent cluster (`git-commits.md`). Never mix a port with a refactor.
- Before ending a session, update `docs/STATUS.md` (milestone, done, next, blockers).
