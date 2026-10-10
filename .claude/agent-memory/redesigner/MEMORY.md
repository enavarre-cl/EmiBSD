# redesigner: lessons across runs

One line per lesson, appended at the end; task state goes in HANDOFF.md
(`.claude/rules/subagents.md`, "Memory"). Committed with the tree.
- Worktree sandbox: run git, cargo and file moves as plain single commands from the worktree root; compound lines (cd+git, loops over cargo, `python3 /dev/stdin`) are refused; write edit scripts to the scratchpad and run them with paths as args.
- Clippy: `cargo clippy -p bsd --all-targets` drags in hundreds of unrelated test-code lints; check as `just clippy` does (lib only, host and `--target ... --features qemu,multiprocessor`).
- Intrusive links: keep element-wide provenance by deriving the entry pointer with `NonNull::from(elem).cast::<u8>().map_addr(|a| a.saturating_add(OFFSET))` (safe, stable), never from `&elem.field`, so a container_of step back is sound; became a docs/IDIOMS.md row (typed handle, tree.rs/subr_tree.rs).
- clippy::type_complexity fires on `PhantomData<(&'a X, fn() -> T)>`; split it into two PhantomData fields.
- Safe readers over intrusive links are only sound if the unlink path clears the unlinked element's links and a poison value is flagged (never followed); tree(3)/RBT_INIT(9) define element ops only for linked elements, so clearing is not a deviation.
