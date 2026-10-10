# The unsafe budget

Safety is the first goal of the native system (`docs/PHASE2.md`): zero `unsafe` outside a
listed core (`docs/ZERO_UNSAFE.md`, the user's decision of 2026-10-10). The budget makes that
measurable and keeps it from regressing.

- The core is the list of paths in `unsafe-core.toml` (the hardware, the boot code, the host
  harness, the runtime of primitives). Only there may `unsafe` live. A path joins the list only
  with the user's OK, in a commit of its own, like a budget raise.
- Every module outside the core is either `forbid` (its `mod` declaration in the parent
  `mod.rs` or crate root, or an ancestor's, carries `#[forbid(unsafe_code)]`, so the compiler
  refuses any `unsafe` in it) or legacy (inherited, not yet redesigned). The `forbid` count
  never falls; the legacy total never rises, except in the re-baseline commit of an LZ sync
  (below).
- `cargo xtask unsafe-report` prints, beside the per-subsystem rows, the three groups over the
  kernel counts (test code apart): core (the `unsafe` in the files of `unsafe-core.toml`, per
  row), `forbid` (the modules so declared, read from the module tree down from every `lib.rs`
  and `main.rs`, out of all the modules under `sys/` that `lineage.toml` tracks) and legacy
  (the `unsafe` outside the core). It also prints the shape counts (`StaticCell` uses,
  `unsafe impl Send`/`Sync`, `Cell<*const|*mut T>`, `UnsafeCell`) per subsystem, per module
  with `--shapes`; they gate nothing.
- `unsafe-budget.toml` holds the `[ratchet]` table (`forbid_floor`, `legacy_ceiling`) and, per
  `cargo xtask unsafe-report` row (`kern`, `uvm`, `net`, `dev/pci`, `arch/amd64`,
  `lib/libkern`, ...), the kernel total (blocks + fn + impl + trait + other) the subsystem may
  not exceed; the core's rows among them. Test code is not budgeted.
- `cargo xtask unsafe-report --check` runs in `just ci` and fails when any subsystem exceeds its
  budget, when the `forbid` modules fall below `forbid_floor`, when legacy rises above
  `legacy_ceiling`, or when a pattern of `unsafe-core.toml` names nothing.
  `cargo xtask unsafe-report --write` records the totals and the groups in `docs/STATUS.md`,
  lowers the budgets to the current counts, raises the floor and lowers the ceiling; it never
  moves any of them the other way.
- Raising a budget is a commit of its own (`build: raise the unsafe budget of <subsystem> to
  <n>`) with the reason in the body, after the user's OK. A redesign that needs more `unsafe`
  than LZ had is suspect by default.
- An LZ sync re-baselines the budget (the user's decision of 2026-10-10, after the M16 sync
  brought 122 inherited modules and put 14 rows over or without a budget): once its `lz-sync:`
  commits and the pin bump are in, one `build: re-baseline the unsafe budget after the LZ sync
  to <12-hex>` commit raises each row to the count the synced tree has, and `legacy_ceiling`
  with them (`docs/ZERO_UNSAFE.md`, section 8), with the before/after table and the LZ range in
  the body; `forbid_floor` never falls. It covers only what the inherited files and the
  cherry-picks into inherited files brought: a row that grew because of an adapted or
  redesigned module is raised only by the user, as above.
- Every commit whose change moves a subsystem's count carries the trailer
  `Unsafe: <subsystem> <before> -> <after>`, numbers from the tool, one line per subsystem.
- In a redesigned module, every `unsafe` that stays has a `// SAFETY:` that is a soundness
  argument: the invariant, who establishes it, who upholds it, and why the operation cannot
  violate it. "The C does this" is not an argument. Every `unsafe fn` has a `# Safety` section;
  `static mut` stays forbidden; the workspace lints stay denied and are never `#[allow]`ed.
- A safe abstraction that replaces `unsafe` across modules (a typed MMIO accessor, an owned
  DMA buffer, a lock that owns its data) is a row of `docs/IDIOMS.md` and, if it wants a crate,
  goes through the dependency allowlist (`rust-kernel.md`).
- `docs/STATUS.md` keeps the baseline table (the totals at `lz-origin`) next to the current
  ones; a milestone's exit criterion names the subsystems it lowered and by how much.
