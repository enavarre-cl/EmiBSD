# The unsafe budget

Safety is the first goal of the native system (`docs/PHASE2.md`): fewer and smaller `unsafe`
blocks, safe abstractions over the ones that remain, and a soundness argument wherever `unsafe`
stays. The budget makes that measurable and keeps it from regressing.

- `unsafe-budget.toml` holds, per `cargo xtask unsafe-report` row (`kern`, `uvm`, `net`,
  `dev/pci`, `arch/amd64`, `lib/libkern`, ...), the kernel total (blocks + fn + impl + trait +
  other) the subsystem may not exceed. Test code is not budgeted.
- `cargo xtask unsafe-report --check` runs in `just ci` and fails when any subsystem exceeds its
  budget. `cargo xtask unsafe-report --write` records the totals in `docs/STATUS.md` and lowers
  the budgets to the current counts; it never raises one.
- Raising a budget is a commit of its own (`build: raise the unsafe budget of <subsystem> to
  <n>`) with the reason in the body, after the user's OK. A redesign that needs more `unsafe`
  than LZ had is suspect by default.
- An LZ sync re-baselines the budget (the user's decision of 2026-10-10, after the M16 sync
  brought 122 inherited modules and put 14 rows over or without a budget): once its `lz-sync:`
  commits and the pin bump are in, one `build: re-baseline the unsafe budget after the LZ sync
  to <12-hex>` commit raises each row to the count the synced tree has, with the before/after
  table and the LZ range in the body. It covers only what the inherited files and the
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
