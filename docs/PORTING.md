# Porting process

How a C file becomes a Rust file. The rules Claude follows are in
`.claude/rules/porting-workflow.md`; this is the longer human version plus the status table.

## 1. Pick a file

`cargo xtask ports next` lists `todo` entries whose dependencies are `ported` or `skipped`.
If the file you want is not listed, add it to `ports.toml` as `todo` with its `deps` first.
`docs/ROADMAP.md` says which files belong to the current milestone.

## 2. Read

Read the `.c` completely. Then its header(s). Then the `(9)` man pages it mentions.
Before coding, write down:

- the call graph: what it calls, what calls it;
- locking and SPL assumptions (`splhigh`, `mtx_enter`, `KERNEL_LOCK`, "called at IPL_x");
- every error path and what it returns;
- every `#ifdef` and the `option(4)` behind it;
- data structures it owns versus borrows.

## 3. Header template

Every ported file starts like this (`sys/lib/libkern/strlcpy.rs`). The `$OpenBSD$` line and the
licence block are copied verbatim from the C file; the licence block sits between the
`/* <LICENSES> */` and `/* </LICENSES> */` marker lines (nothing checks them yet; M14b makes
`cargo xtask ports check` validate them). A file whose C has several notices keeps all of them inside one pair of markers. To read a
ported file, start at the closing marker (`sed -n '/<\/LICENSES>/,$p' <file>`).

```rust
/*	$OpenBSD: strlcpy.c,v 1.9 2019/01/25 00:19:26 millert Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1998, 2015 Todd C. Miller <millert@openbsd.org>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * ...
 */
/* </LICENSES> */

//! `strlcpy(3)`: size-bounded string copy.
//!
//! Upstream: sys/lib/libkern/strlcpy.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - Operates on byte slices: the destination size is `dst.len()`, not a separate argument, and
//!   `src` ends at its first NUL or at `src.len()`, whichever comes first.
```

## 4. Write

Idiomatic Rust that preserves semantics. `docs/C_TO_RUST.md` is the idiom table; follow it or add
a row. Keep OpenBSD names. Put types in the header's module, functions in the file's module.
When something the file needs is not ported yet, stub it visibly
(`.claude/rules/scope-and-stubs.md`), never silently.

## 5. Test

- Pure logic: `#[cfg(test)] mod tests` in the same file (or `<name>/tests.rs` once longer than
  50 lines), runs with `just test`.
- Constants mirrored from C headers: `#[ignore]` reference-backed test, runs with `just test-ref`.
- Boot, console, trap behaviour: smoke expectation in `tools/xtask`, runs with `just smoke`.

## 6. Record

In `ports.toml`: `status = "ported"`, `upstream_commit` = `[meta].pinned`,
`upstream_blob` = `git -C reference/openbsd-src rev-parse HEAD:<c path>`, `notes` for stubs.
Then `cargo xtask ports check` and `cargo xtask ports status --write`.

## 7. Commit

`<scope>: port <file>.c (<what>)` with one `Upstream:` trailer per C file.
See `.claude/rules/git-commits.md`.

## Skipping

`status = "skipped"` with `notes` starting with one of `replaced-by-limine`, `provided-by-core`,
`deferred-driver`, `license: <which>`, `not-applicable`, followed by a short reason.
Skipped is a recorded decision, not "later".

## Bumping the reference pin

Deliberate, never automatic:

```sh
git -C reference/openbsd-src fetch --depth 1 origin master
git -C reference/openbsd-src checkout --detach FETCH_HEAD
cargo xtask ports drift --diff      # what changed upstream among ported files
# triage each DRIFT line: re-port, or add a note explaining why the change does not apply
# update reference/PINNED.md (Commit:, Date:) and ports.toml [meta].pinned
git commit -m "reference: bump OpenBSD pin to <12-hex>"
```

## Measuring unsafe

`cargo xtask unsafe-report` (M12+) counts the `unsafe` keywords of the kernel crates (`sys/`,
with `sys/lib/libkern` and `sys/lib/libz`, and `init/`; not `tools/xtask`). It is the Phase 2
baseline ([PHASE2.md](PHASE2.md)); `--write` puts the totals on the `Unsafe` line of
`docs/STATUS.md`.

- What counts: `unsafe { }` blocks, `unsafe fn` declarations (also `unsafe extern "C" fn`),
  `unsafe impl`, `unsafe trait`. "Other" is every remaining `unsafe`: `unsafe extern` blocks,
  `#[unsafe(no_mangle)]` attributes and `unsafe fn(..)` pointer types.
- What does not: an `unsafe` inside a comment, a string or a raw string (a small lexer skips
  them), and `r#unsafe`.
- Test code is a column of its own: `tests.rs` files, files declared by a test-only `mod`,
  and the item after a test-only `#[cfg]` (`test`, `all(.., test)`; `any(test, feature = "x")`
  also builds into a kernel and counts as kernel code).
- Subsystems: the first directory under `sys/` (`kern`, `uvm`, `net`, `netinet`, `ufs` with
  ffs/mfs/ext2fs, `isofs`, ...); `arch/<a>` and `lib/<l>` by two; `dev/<d>` for the bus and
  chip directories that hold a `mod.rs` (`pci`, `pv`, `ic`, `isa`, `fdt`, `ofw`, `efi`), and
  `dev` for the rest of `sys/dev` (softraid, vnd, rd, bio, cons, rnd); `sys/*.rs` is
  `(crate root)`. `arch/host` is the `cargo test` double, counted as code.

## Status

<!-- ports:begin -->
_Generated by `cargo xtask ports status --write` against pin 3ce1f3f79392._

| Subsystem | todo | wip | ported | skipped | total |
|---|---:|---:|---:|---:|---:|
| arch/amd64 | 1 | 38 | 70 | 1 | 110 |
| arch/arm64 | 6 | 25 | 56 | 2 | 89 |
| conf | 0 | 1 | 1 | 2 | 4 |
| crypto | 0 | 0 | 49 | 0 | 49 |
| ddb | 2 | 2 | 13 | 0 | 17 |
| dev | 180 | 12 | 220 | 7 | 419 |
| isofs | 0 | 0 | 18 | 0 | 18 |
| kern | 0 | 21 | 61 | 2 | 84 |
| lib/libkern | 0 | 0 | 11 | 3 | 14 |
| lib/libsa | 10 | 0 | 50 | 8 | 68 |
| lib/libz | 0 | 0 | 20 | 0 | 20 |
| miscfs | 0 | 0 | 10 | 0 | 10 |
| msdosfs | 0 | 0 | 12 | 0 | 12 |
| net | 0 | 2 | 60 | 0 | 62 |
| netinet | 0 | 1 | 57 | 0 | 58 |
| netinet6 | 0 | 0 | 30 | 0 | 30 |
| nfs | 0 | 0 | 25 | 1 | 26 |
| ntfs | 0 | 0 | 13 | 0 | 13 |
| scsi | 0 | 0 | 12 | 0 | 12 |
| stand | 0 | 0 | 21 | 8 | 29 |
| sys | 1 | 17 | 89 | 2 | 109 |
| tmpfs | 0 | 0 | 8 | 0 | 8 |
| ufs | 0 | 0 | 44 | 1 | 45 |
| uvm | 0 | 20 | 17 | 0 | 37 |
| **total** | 200 | 139 | 967 | 37 | 1343 |
<!-- ports:end -->
