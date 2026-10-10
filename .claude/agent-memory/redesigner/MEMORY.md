# redesigner: lessons across runs

One line per lesson, appended at the end; task state goes in HANDOFF.md
(`.claude/rules/subagents.md`, "Memory"). Committed with the tree.
- The smoke ESP image (`IMAGE_SECTORS`, tools/xtask/src/boot.rs) holds the debug kernel plus the ramdisk; a redesign that lets LLVM inline more grows `.debug_info` by MBs even when `.text` shrinks, and every `--expect-ramdisk` smoke then fails with a bare "xtask: No space left on device". Compare `llvm-objdump -h` of the old and new `bsd` (the sysroot's `lib/rustlib/<host>/bin/`) before blaming the code; N2 raised the image to 256 MiB.
- Worktree sandbox: git commands must be plain (`git show`, `git add <path>`), never `git -C`, never chained with `&&` after other git calls in one line, never with variables; `reference/emibsd-lz` cannot be queried with git from a worktree, so confirm an LZ blob with `diff` against the clone's file and `cargo xtask lz check`.
- clippy `declare_interior_mutable_const` fires on a named `const` of a type with `Cell` links (queue heads) in tests; use an inline `const { .. }` block instead.
- `kassert!` without `diagnostic` still builds a closure per expansion (to keep names used); in generic hot helpers gate it with `#[cfg(feature = "diagnostic")]` and call `crate::kassert!` by path so no import goes unused.
- The host build with `--features diagnostic` was broken at the M16 sync (dev/usb/ucom.rs, `usbd_errstr` not imported); to run a module's diagnostic tests, patch it locally and `git checkout` it before committing.
