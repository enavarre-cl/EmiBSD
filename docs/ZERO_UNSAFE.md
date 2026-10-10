# Zero unsafe outside the core

A proposal: make `unsafe` a compile error everywhere except in a small, listed core, and measure
the redesign by how much of the kernel already compiles that way.

## 1. Status and goal

**Status: in force since 2026-10-10** (`docs/PHASE2.md`, "Decisions"; section 10 records the
answers). The plan is iterative: each step is measured and this file follows what the steps
teach. Every number below was measured on `main` at `72ea58e`, with
`cargo xtask unsafe-report`, `grep` over `sys/` and one compile experiment.

### The goal, as a tool can check it

1. **No `unsafe` outside the core.** Every module outside a listed core compiles under
   `#[forbid(unsafe_code)]`. The compiler then refuses `unsafe` blocks, `unsafe fn`,
   `unsafe trait` and `unsafe impl` there, macro expansions included.
2. **The core is listed and budgeted.** All `unsafe` lives in the paths of `unsafe-core.toml`
   (new): the hardware (`sys/arch/amd64/`, `sys/arch/arm64/`, `sys/machine/`), the boot code
   (`sys/stand/`, `sys/lib/libsa/`, a separate binary), the host test harness
   (`sys/arch/host/`) and a runtime of primitives (section 4). Every `unsafe` there has a
   `// SAFETY:` (clippy's `undocumented_unsafe_blocks` is already denied workspace-wide,
   `Cargo.toml:42`). Each core row has a budget, and no budget goes up without the user.
3. **Legacy only shrinks.** What lies outside the core and is not yet `forbid` is legacy
   (below). Its total never rises, an LZ sync aside (section 8), and it reaches 0 at N9.

Why: "fewer and smaller `unsafe` blocks" (goal 1 of `docs/PHASE2.md`) has no end point. A
boundary the compiler checks has one, and every step towards it is a number from a tool.

What userland sees does not change. `forbid` and safe traits act at compile time only, and each
slice keeps the ABI edge and passes the smokes and `just diff-openbsd`.

### Why not literal zero

A kernel on real hardware must do what no type system can check: write device registers
(MMIO), build page tables, switch contexts, enter and leave interrupts, copy across the user
boundary (`copyin`, `copyout`) and hand out raw memory inside its allocator. Some code has to
state those invariants by hand. The goal keeps that code small, listed, argued and reviewed,
and keeps everything else out of it.

### The legacy layer

Legacy is a state, not a directory: a module outside the core that does not compile under
`forbid` yet. Today that is most of the kernel (section 2): the LZ modules not yet redesigned
that still hold `unsafe`. They keep it for now. A module leaves legacy when the sweep marks it
because it holds no `unsafe` (section 3), or when it is redesigned, which then means `forbid`
(section 5). It never goes back: the count of `forbid` modules never falls.

### Prior art

Others draw the same line:

- Asterinas calls its design a framekernel: a small framework, OSTD, holds the `unsafe`, and
  the services built on it forbid it.
- Tock compiles its capsules, the drivers and services, with `unsafe` forbidden; the kernel
  core beneath them holds the rest.
- Rust for Linux writes abstractions with `unsafe` over the C kernel's interfaces, so that
  drivers can be written in safe Rust.

## 2. Where the unsafe is today

`cargo xtask unsafe-report` at `72ea58e`, kernel column (test code is counted apart):

| Layer | `unsafe-report` rows | Total | Fate |
|---|---|---:|---|
| Core: hardware | `arch/amd64` 863, `arch/arm64` 943, `machine` 85 | 1,891 | stays, budgeted |
| Core: boot | `stand` 235, `lib/libsa` 84 | 319 | stays; a separate binary |
| Core: test harness | `arch/host` 38 | 38 | stays; host only |
| Core: runtime candidates | the files of the next table | ~250 | stays, budgeted, shrinks |
| Kernel (everything else) | `net` 1,579, `kern` 1,146, `netinet` 553, `netinet6` 355, `sys` 330, `uvm` 215, the 22 `dev` rows 3,975 (`dev/pci` 737, `dev/ic` 664 and `dev/usb` 528 the largest), the seven file-system rows 635, `scsi` 149, `ddb` 15, `init` 11, `lib/libkern` 10, `crypto` 5, `conf` 2 | ~8,700 | 0 |
| Total | | 11,228 | |

The kernel rows hold 8,980 in all. The runtime candidates sit inside `kern`, `sys` and
`lib/libkern`; their row is a grep of `unsafe` tokens in the candidates' code zones, not an
`unsafe-report` count, and the kernel layer is what is left, about 8,700.

Runtime candidates (`unsafe` tokens in each file's code zone):

| Primitive | Files |
|---|---|
| Statics | `sys/lib/libkern/staticcell.rs` 11 |
| Locks and reference counts | `sys/sys/mutex.rs` 2, `sys/kern/kern_lock.rs` 8, `sys/sys/rwlock.rs` 2, `sys/kern/kern_rwlock.rs` 2, `sys/sys/mplock.rs` 0, `sys/sys/refcnt.rs` 1 |
| Per-CPU data and SMR | `sys/sys/percpu.rs` 7, `sys/kern/subr_percpu.rs` 17, `sys/sys/smr.rs` 8, `sys/kern/kern_smr.rs` 6 |
| Allocators | `sys/sys/pool.rs` 1, `sys/kern/subr_pool.rs` 58, `sys/sys/malloc.rs` 3, `sys/kern/kern_malloc.rs` 14 |
| Lists and trees | `sys/sys/queue.rs` 49 (6 blocks, 38 `unsafe fn`, the trait and its impl), `sys/sys/tree.rs` 23, `sys/kern/subr_tree.rs` 15 |
| Copy, print, time | `sys/kern/kern_subr.rs` 6 (`uiomove`), `sys/kern/subr_prf.rs` 7, `sys/sys/systm.rs` 1, `sys/kern/kern_tc.rs` 7 |

The atomics live in `sys/machine/atomic.rs`, already inside the hardware core.

**Shapes to replace**, kernel-wide:

| Shape | Count | Target (section 4) |
|---|---:|---|
| `StaticCell<T>` uses | 626 in 117 files | init-once cell, `Mutex<T>` statics, `PerCpu<T>` |
| `unsafe impl Send` and `unsafe impl Sync` | 325 | structs made of `Send` and `Sync` parts |
| `Cell<*const T>` and `Cell<*mut T>` | 674 (0 left in queue.rs and tree.rs) | state owned by its lock, owners' collections |
| `UnsafeCell` | 74 | `Mutex<T>`, `Rwlock<T>` |

**The primitives are C-shaped.** A lock holds no data (`mtx_enter(&Mutex)`,
`rw_enter(&Rwlock, flags)`), so what it guards sits elsewhere, in `Cell`s. `pool_get` and
`malloc` return `Option<NonNull<u8>>`: raw memory that the caller types and frees by hand.

**Already safe at the signature**, with the `unsafe` inside the core: `copyin`, `copyout` and
`copyinstr` over slices (`sys/machine/copy.rs:105`), `uiomove(&mut [u8], &mut Uio)`
(`sys/kern/kern_subr.rs:93`), `bus_space_read_4` and `bus_space_write_4`
(`sys/machine/bus.rs:195`, `sys/machine/bus.rs:210`).

**Day-one baseline.** Of the 1,191 `.rs` files under `sys/` (host, stand and `mod.rs`
excluded), 423 hold no `unsafe` token anywhere, tests included (433 with the tests left out).
One mechanical sweep can mark them `forbid`: about 35% of the files on day one, less the few
that section 3 says must wait. For scale, the hardware core is about 9% of the lines:
`arch/amd64` 40,372, `arch/arm64` 44,715 and `machine` 5,551 of the 1,002,358 under `sys/`,
tests included.

## 3. The mechanism

The attribute goes on the `mod` declaration in the parent, never inside the module's own file:

```rust
// sys/kern/mod.rs, line 89; today the line reads `pub mod sys_pipe;`
#[forbid(unsafe_code)]
pub mod sys_pipe;
```

**Rust note:** a lint level says what the compiler does when a lint fires. `warn` prints a
warning, `deny` makes it an error, and `forbid` makes it an error that no `#[allow]` further in
can lift.

A compile experiment (`cargo check`, rustc 1.98.1, a small crate with one `forbid` module)
verified the two facts the design rests on.

**Fact 1: `forbid` on the declaration reaches the out-of-line file.**
`#[forbid(unsafe_code)] pub mod svc;` in the crate root made a plain `unsafe {}` in `svc/mod.rs`
an error ("usage of an `unsafe` block"). So the ratchet lives in the `mod.rs` files and the
crate roots, which `lineage.toml` does not track (`.claude/rules/lineage.md`); the layout rule
already keeps every `mod` declaration there (`.claude/rules/rust-kernel.md`). An `inherited`
module can be `forbid` and stay byte-identical to LZ's. Why there: an inner
`#![forbid(unsafe_code)]` would change the module's own file and break that identity. The
attribute also covers the module's inline `mod tests`, so the 10 files whose only `unsafe` is
in their tests wait until those tests lose it.

**Fact 2: a macro's `unsafe impl` is refused.** A macro of the same crate that expands to
`unsafe impl Trait` is an error inside a `forbid` module ("implementation of an `unsafe` trait
... in this macro invocation"). `queue_adapter!` expands to `unsafe impl Adapter`
(`sys/sys/queue.rs:160`), and `tree_adapter!` expands to `queue_adapter!`
(`sys/sys/tree.rs:137`): 267 and 37 sites in 133 and 18 files. An `unsafe impl` is the
implementer's promise that a contract the compiler cannot check holds (here: `OFFSET` is the
offset of the field that `entry` returns), and `forbid` refuses that promise wherever it is
written. Every `unsafe impl Send` and `unsafe impl Sync` (325 in `sys/`) is refused the same
way.

What follows from fact 2:

- **`Adapter` becomes a safe trait.** `Adapter` (`sys/sys/queue.rs:1587`) drops its `unsafe`.
  The macro already computes `OFFSET` with `core::mem::offset_of!` (`sys/sys/queue.rs:163`) and
  writes `entry` as `&elem.field`, so the two agree by construction. What changes is who vouches
  for that: today the `unsafe impl` does; after the change the macro alone does, because
  `cargo xtask lz check` refuses an `Adapter` impl written by hand (none exists today). The
  core's `SAFETY:` comments that rest on the contract (`sys/sys/tree.rs:616`, "upheld by
  `queue_adapter!`") then cite that check. `TreeAdapter` (`sys/sys/tree.rs:980`) is a safe trait
  already; only the `queue_adapter!` it expands made it fail. The files that invoke the macros
  do not change. Why: with the trait left `unsafe`, every module that declares a list stays out
  of `forbid` until its own slice.
- **The proof moves from the compiler to a tool.** A hand-written `impl Adapter` would compile,
  and `just ci`, which runs `lz check`, fails on it. This is the one place where the proposal
  trades a compiler guarantee for a CI check; every review of a core change should know it.
- **`unsafe impl Send` and `Sync` are not swept.** Such a struct must become `Send` or `Sync`
  from its parts (the candidate row of `docs/IDIOMS.md`), usually because its mutable state
  moves into a core type: a `Mutex<T>`, an atomic. That happens owner by owner, in the slices
  (section 6).
- **The compiler judges the sweep.** The sweep adds the attribute to the 423 candidates and
  keeps the declarations that compile. A file with no `unsafe` token is still refused when it
  invokes a macro that expands one: the adapters, or `byte_view!` (`sys/msdosfs/bpb.rs:212`).
  Such a file waits for its macro's fix.

**Rust note:** `core::mem::offset_of!(Type, field)` is a constant the compiler computes, the
byte offset of a field inside its struct. It is stable and needs no `unsafe`; only
dereferencing an element pointer computed from it does, and that stays in the core
(`docs/IDIOMS.md`, the `container_of` row).

## 4. The core

### The initial list

`unsafe-core.toml` (new, at the root) holds the paths and globs that may contain `unsafe`.
`cargo xtask unsafe-report` and `cargo xtask lz check` read it.

| Layer | Paths | Why it is core |
|---|---|---|
| Hardware | `sys/arch/amd64/**`, `sys/arch/arm64/**`, `sys/machine/**` | MMIO, page tables, traps, context switch, `copyin`/`copyout`, atomics |
| Boot | `sys/stand/**`, `sys/lib/libsa/**` | the boot loaders: a separate binary that runs before the kernel |
| Test harness | `sys/arch/host/**` | the host test double; host only |
| Runtime | the files of the runtime table (section 2), and `sys/lib/libkern/explicit_bzero.rs` | the primitives every subsystem builds on; their `unsafe` pays for the safe API above them |

`explicit_bzero.rs` is not among the measured candidates, but it is `redesigned`
(`lineage.toml:4376`) and keeps the volatile store that N1 left (`docs/ROADMAP.md`, N1), so
"redesigned = `forbid`" (section 5) puts it here. The volatile `wipe` that `docs/STATUS.md`
lists as a crypto follow-up is the same kind of store and would join it. Two rows stay out, as
legacy like the rest: `conf` (2) and `init` (11, the freestanding init stand-in that
`unsafe-report` also reads).

Rules of the list:

- A path joins it only with the user's OK, in a commit of its own, like a budget raise. Why: the
  list is the boundary, and growing it is the one way around the goal.
- A core file may carry `forbid` too, and should once its `unsafe` is gone (`sys/sys/mplock.rs`
  has none today). Why: the list says where `unsafe` may live, not where it must.
- The runtime gets a budget row of its own, carved out of `kern`, `sys` and `lib/libkern`. Why:
  those rows then measure legacy alone, and the runtime's size is a number of its own.

### What it must expose

The runtime exists to give `forbid` modules a safe API. Its target shapes, each a row of
`docs/IDIOMS.md` in the commit that first uses it (the candidates there name most of them):

- **Locks that own their data.** `Mutex<T>` and `Rwlock<T>` with guards, replacing the C-shaped
  locks and the `Cell` fields beside them. A guard can sleep (`rwsleep`) and wake (`wakeup`).
- **Typed allocation.** `Pool<T>` handing out an owned `PoolBox<T>`; a typed `malloc` as
  `KBox<T>` and `KVec<T>`, fallible, behind the `alloc` feature.
- **Statics.** An init-once cell for tables written at boot, `Mutex<T>` statics, and `PerCpu<T>`
  behind the interrupt-level guard. They replace `StaticCell`, whose API stays frozen by the N1
  timing rule, so each use moves with the module that owns it.
- **Collections for owners.** Arena lists (a slab with a generational index: O(1), several lists
  per element, and a stale index is an error instead of undefined behaviour) and lists of
  `Arc`-owned elements. The typed-link intrusive lists stay inside the core, for its own use;
  queue.rs's 38 `unsafe fn` mutators are deleted once no module outside the core calls them
  (`queue_adapter!` alone appears in 133 files today).
- **The user boundary.** `copyin`, `copyout` and `uiomove` keep their slice signatures.
- **Devices (N7).** A typed `Mmio<Regs>` over `bus_space` and an owned `DmaBuf` over `bus_dma`.

**Rust note:** an `unsafe fn` declares a contract its caller must keep, so every call is written
inside an `unsafe { }` block, where the caller takes that duty on. A module that calls one of
queue.rs's mutators therefore holds `unsafe` blocks and cannot be `forbid`.

### Its rules

- Every `unsafe` block has a `// SAFETY:` soundness argument and every `unsafe fn` a `# Safety`
  section, both enforced (`undocumented_unsafe_blocks`, `Cargo.toml:42`; `missing_safety_doc`,
  `Cargo.toml:43`). No `static mut`.
- An agent that did not write the change reviews every core change (`reviewer`). Why: a mistake
  in the core is undefined behaviour for every module above it.
- Each core row has a budget in `unsafe-budget.toml`. Budgets ratchet down and rise only with the
  user, as today (`.claude/rules/unsafe-budget.md`).
- A primitive lands with its first user, never ahead of it. Why: `docs/IDIOMS.md` adds a row in
  the commit that first uses an idiom, and an API nobody calls is an untested design.
- Miri, an interpreter that runs tests and stops at undefined behaviour, could run the core's
  host tests. It needs a nightly toolchain to run, while the code stays on stable; the project
  forbids nightly features, not necessarily a nightly tool for tests. The user decides
  (decision 4).

## 5. The metric

`cargo xtask unsafe-report` splits the kernel into three groups:

- **core**: the files of `unsafe-core.toml`, per budget row;
- **forbid**: the modules whose declaration carries `#[forbid(unsafe_code)]`, read from the
  `mod.rs` files and the crate roots; they hold 0 by construction;
- **legacy**: everything else.

`--check`, in `just ci`, then enforces three things:

1. the number of `forbid` modules never falls;
2. the legacy total never rises (an LZ sync is the one exception, section 8);
3. each core row stays within its budget.

The floor of item 1 and the ceiling of item 2 live in `unsafe-budget.toml` beside the rows, and
`--write` moves them only in the good direction, as it already does the budgets. The
per-subsystem rows stay; outside the core their sum is the legacy total. `--write` also writes
the `Unsafe` lines of `docs/STATUS.md` and of `README.md` ("Lineage and safety"): core, legacy,
and `forbid` modules out of all modules.

Beside the gate, the report prints four shape counts per subsystem and per module: `StaticCell`
uses, `unsafe impl Send` and `Sync`, `Cell<*const T>` and `Cell<*mut T>`, `UnsafeCell`. They
gate nothing; the milestone exits name them where they matter (N2, N4). Why: a count of `Cell`s
says little alone, and a gate on it would reward moving them around.

**"Redesigned" means "compiles with `forbid`".** `cargo xtask lz check` refuses a `redesigned`
module outside the core whose declaration lacks the attribute, and an `Adapter` impl written by
hand (section 3). Why: the status then states something the compiler checked, not only that the
shape changed. An `inherited` module may be `forbid` as well (the sweep) and stays `inherited`.

**Where it shows.** `cargo xtask lz status --write` gains a `forbid` column per subsystem in the
README table, and `/progress` a `forbid` column per milestone: `forbid` modules out of the row's
modules. Why: the goal's progress then sits where the redesign's progress is already read.

## 6. The order

**Runtime first, then vertical slices per owner.** A slice takes one owner, the module that owns
a data structure (`sys/kern/sys_pipe.rs` for a pipe), with the header types it owns, and
redesigns them to `forbid` in one go, once the core primitives it needs exist. Why: a type's
fields and their users change together; a horizontal pass ("every `Cell` out of `sys/sys/`")
would rewrite modules that later milestones own, and pay for it again in every LZ sync.

### The first slice: pipe(2), recommended

- Modules: `sys/kern/sys_pipe.rs` (1,659 lines, 21 `unsafe`) and `sys/sys/pipe.rs` (233 lines,
  14 `Cell`, 5 of them raw-pointer `Cell`s), declared at `sys/kern/mod.rs:89` and
  `sys/sys/mod.rs:76`.
- Sources: one C file with its header, `reference/openbsd-src/sys/kern/sys_pipe.c` and
  `reference/openbsd-src/sys/sys/pipe.h`; in LZ, `reference/emibsd-lz/sys/kern/sys_pipe.rs` and
  `reference/emibsd-lz/sys/sys/pipe.rs`.
- Callers: 4 files.
- Imports: `kern_descrip`, `kern_event`, `kern_rwlock`, `kern_sig`, `kern_subr::uiomove`,
  `kern_synch`, `kern_tc`, `subr_pool`, `machine::copy`, `uvm_km`, `file` and `filedesc`.
- Coverage: shell pipelines in the `--send` lines of 12 smokes (ahci, ehci, ext2fs, fuse, https,
  nvme, pf, siop, smmu, softraid, tpm, uaudio) and in every installer run. No `diff-openbsd`
  step names pipes, so the slice adds one.

Why pipe: it is small, comes from one C file, has four callers, every smoke with a pipeline
exercises it, and it uses much of the runtime at once (a sleeping lock, a pool, `uiomove`,
kqueue, `SIGIO`). It proves the core's API on real code.

What it needs first, six items:

1. `Rwlock<PipeState>`: the pipe's state owned by its lock;
2. `Pool<Pipe>`: pipes from a typed pool;
3. an owned buffer: `km_alloc` returning owned pages instead of an address;
4. a safe seam over the legacy `kern_event` (`klist`, `knote`);
5. a safe seam over the legacy `kern_sig` (`sigio`);
6. `rwsleep` and `wakeup` on the `Rwlock<T>` guard.

A seam is a safe function that a legacy module exports, with the `unsafe` inside it, so that a
`forbid` caller can use the module before that module's own slice. The ratchet still holds: the
slice must take out more legacy `unsafe` than its seams put in.

The slice's exit is part of N2's (section 7): both files `forbid`, no raw-pointer `Cell` left in
them, every smoke and `just diff-openbsd` equal with the new pipe step.

### The warm-up alternative: msgbuf

`sys/kern/subr_log.rs` (695 lines, 12 `unsafe`, 5 `StaticCell`) and `sys/sys/msgbuf.rs` (7
`Cell`): 9 callers, `dmesg` in `smoke-virtio` and `smoke-wscons`, and imports of the sockets
(`sosend`) and the kernel lock. It is smaller, and it exercises the statics (the init-once cell,
`Mutex<T>` statics) more than the sleeping lock. Its coverage is two smokes, and it needs a seam
into the socket layer. Pipe stays the recommendation for its broader coverage; msgbuf suits a
smaller first step (decision 3).

Other owners measured, for the order after the first slice:

| Owner | Lines | `unsafe` | Callers |
|---|---:|---:|---:|
| `kern_timeout` | 1,198 | 35 | 93 |
| `kern_event` | 3,199 | 76 | 42 |
| `kern_task` | 714 | 19 | 46 |
| `kern_sensors` | 467 | 16 | 11 |

`kern_timeout` has too many callers for a first slice; a change to its API would also fall
under the timing rule of `.claude/rules/re-engineering.md` (more than 50 modules).

## 7. Milestones

This replaces rows N2 to N8 of the table in `docs/ROADMAP.md` and adds N9. N0 and N1 stay as
they are (met), and so do the order (leaves first, the security subsystems last) and the rolling
LZ sync. Same columns; `<n>` is filled from the tools when a row starts (`docs/ROADMAP.md`,
"Adding or changing a milestone"). A row's scope is resolved as `/progress` resolves it, the most
specific path winning, so a row's numbers count only its own modules: `sys/kern/vfs_*` belongs
to N5, and the files N8 names belong to N8, not to N4, N6 or N7.

| Milestone | Scope | Exit criterion |
|---|---|---|
| **N2 Core runtime** (under way; was "Core structures") | First step, done: `sys/sys/queue.rs`, `sys/sys/tree.rs` and `sys/kern/subr_tree.rs` with typed links. Then: the metric in `just ci` (`unsafe-core.toml`, the core, forbid and legacy groups, the ratchet); `Adapter` a safe trait; the `forbid` sweep (~423 modules); the runtime's safe API, each piece with its first user: `Mutex<T>` (`sys/sys/mutex.rs`, `sys/kern/kern_lock.rs`), `Rwlock<T>` (`sys/sys/rwlock.rs`, `sys/kern/kern_rwlock.rs`), `Pool<T>` (`sys/sys/pool.rs`, `sys/kern/subr_pool.rs`) and the init-once static; the verdicts of the 46 ABI-constant headers; the first slice, pipe(2): `sys/kern/sys_pipe.rs`, `sys/sys/pipe.rs` | `cargo xtask unsafe-report` prints the core, forbid and legacy groups, and `--check` passes in `just ci`; `forbid` modules >= `<n>`; `sys/kern/sys_pipe.rs` and `sys/sys/pipe.rs` declared `forbid`, and `grep -c 'Cell<\*'` prints 0 for each; every smoke and `just diff-openbsd` equal, with the new pipe step |
| **N3 Memory** | `sys/uvm/` in owner slices (amap, aobj, map, the pagers; `uvm_fault` last, per `security-review.md`); the pmap interface in the core | the `uvm` row holds 0 `unsafe` outside the core |
| **N4 Processes and scheduling** | `sys/kern/` by owner (timeout, task, event and kqueue, signals, proc and the scheduler), each with its header types | the `kern` row holds 0 `unsafe` outside the core, apart from the modules of N5 and N8; `StaticCell` uses in `kern`: 0 |
| **N5 VFS and file systems** | `sys/kern/vfs_*`, `sys/ufs/` first, then `sys/isofs/`, `sys/msdosfs/`, `sys/nfs/`, `sys/ntfs/`, `sys/tmpfs/`, `sys/miscfs/` | the `ufs`, `isofs`, `msdosfs`, `nfs`, `ntfs`, `tmpfs` and `miscfs` rows hold 0 `unsafe` outside the core; every `sys/kern/vfs_*` module `forbid` |
| **N6 Network stack** | `sys/net/`, `sys/netinet/`, `sys/netinet6/`, except the files of N8 | the `net`, `netinet` and `netinet6` rows hold 0 `unsafe` outside the core, apart from the modules of N8 |
| **N7 Devices** | `sys/dev/` and the bus layers (`sys/scsi/`), one bus directory at a time; a typed `Mmio<Regs>` and an owned `DmaBuf` in the core; drivers `forbid`; `sys/dev/rnd.rs` last (`security-review.md`) | every `dev` row and the `scsi` row hold 0 `unsafe` outside the core, apart from `sys/dev/softraid_crypto.rs` (N8) |
| **N8 Security subsystems** | Under `security-review.md`: `sys/net/pf*` (pf and pfkeyv2), `sys/net/if_pf*`, `sys/net/if_wg.rs`, `sys/net/wg_*`, `sys/netinet/ip_ipsp.rs`, `sys/netinet/ip_esp.rs`, `sys/netinet/ip_ah.rs`, `sys/netinet/ip_ipcomp.rs`, `sys/netinet/ipsec_*`, `sys/dev/softraid_crypto.rs`, `sys/kern/kern_pledge.rs`, `sys/kern/kern_unveil.rs`, `sys/kern/exec_*`, and the crypto framework (N1 did the primitives) | every file of the scope `forbid`; `Security-Review:` on every commit; `cargo xtask lz drift --security` empty |
| **N9 Close** | What no row claims (`sys/ddb/`, `sys/conf/`, the init stand-in) and the legacy left; the boundary made final: the core in a crate of its own, with `#![forbid(unsafe_code)]` at the root of the crate that uses it, or kept as the module list with every declaration outside it `forbid` (decision 5); Miri on the core's host tests if adopted (decision 4) | `cargo xtask unsafe-report`: legacy 0; with a core crate, the logic crate's root carries `#![forbid(unsafe_code)]` and `just build` passes; the core's size recorded in `docs/STATUS.md`; Miri green if adopted |

Every exit also requires:

- each module of the scope is `redesigned` (and so `forbid`), listed in the core, or `reviewed`:
  a per-file verdict in `lineage.toml`, with its reason, for a module that needs no redesign and
  is `forbid` as it stands (the 46 ABI-constant headers of `sys/sys/` get `forbid` and
  `just test-ref` checks of their constants);
- `just ci`, `just ci-full` and `just diff-openbsd` equal, with the same expected list;
- `cargo xtask lz drift --security` empty, and the numbers in the closing commit.

## 8. LZ

- **A `forbid` module takes an LZ change as `reimplemented` when it must.** An `inherited`
  module that is `forbid` is still byte-identical to LZ's, so LZ's changes to it still arrive by
  cherry-pick. A change that adds `unsafe` does not compile there, and it is re-implemented in
  safe code. The compiler decides: a cherry-pick that compiles is still a cherry-pick. Why: the
  `forbid` count never falls, so a sync cannot buy LZ's shape back with an `unsafe` line.
- **New LZ files arrive `inherited`, as legacy.** Their `unsafe` raises the legacy total, the
  one way it may rise: the sync re-baselines the ceiling in a commit of its own, with the user's
  OK, as the M16 sync's budget rows were. Why: LZ's new code is not a regression of this tree,
  but its size must still be seen and approved.
- **Drivers become `forbid` in N7**, with the typed bus API. Until then LZ's driver commits keep
  landing by cherry-pick.
- **`reimplemented` growing is expected now**, no longer the signal to reorder the roadmap that
  `.claude/rules/lz-sync.md` makes it today. The new signal is a legacy total that stops falling
  from one sync to the next.
- **Security fixes keep their rule** (`.claude/rules/lz-sync.md`): applied, re-implemented in a
  `forbid` module if need be, or `covered-by-redesign` with the test or type named; never
  skipped silently.
- **The cost moves to the `mod.rs` files.** An LZ commit that adds a module next to a `forbid`
  declaration conflicts there, and the resolution is one line (`cherry-pick-conflicts`). The
  adapter change touches only `sys/sys/queue.rs` and `sys/sys/tree.rs`, both redesigned already,
  so it costs nothing in the files that invoke the macros.

## 9. Commits after the OK

One topic each, in this order. Every commit builds, every group ends with `just ci` green, and
the trailers follow `.claude/rules/git-commits.md`.

1. **`docs: adopt zero unsafe outside a listed core`.** This file in force (its status line);
   in `docs/PHASE2.md` the decision row below and goal 1 reworded; in `docs/ROADMAP.md` the
   table of section 7; the status lines of `docs/STATUS.md` and `README.md` (N2 "Core runtime").
2. **`rules: forbid unsafe code outside the core`.** `unsafe-budget.md` (the core list, the
   ratchet, `forbid`), `re-engineering.md` (redesigned = `forbid`), `lineage.md` (`forbid` in
   `mod.rs`; the `lz check` rules), `lz-sync.md` (a `forbid` module may need `reimplemented`;
   the legacy re-baseline), `rust-kernel.md` (no `unsafe` outside the core; the adapter rule),
   `subagents.md` (what a launch prompt says about the core), and the one line this adds to the
   non-negotiables of `CLAUDE.md`. Files under `.claude/` are edited with Edit/Write (the user's
   rule).
3. **`xtask: split unsafe-report into core, forbid and legacy`.** `unsafe-core.toml`; the split,
   the `forbid` count, the floor and the ceiling, the shape counts; `lz check` refuses a
   hand-written `Adapter` impl; the `forbid` columns of `lz status --write` and of
   `.claude/skills/progress/scripts/progress.py`.
4. **`sys:` the adapters, then the sweep.** First `Adapter` a safe trait (`sys/sys/queue.rs`,
   `sys/sys/tree.rs`; the files that invoke the macros unchanged). Then the sweep over the
   `mod.rs` files and crate roots, with `unsafe-report --write` recording the floor. Last, an
   `xtask:` commit turns on the `lz check` rule "redesigned = `forbid`", which passes only once
   the sweep has marked the redesigned modules of N1.
5. **The pipe slice.** A `redesigner` (opus) with the six items of section 6, then a `reviewer`,
   then the `integrator` and `just ci`: one commit per primitive with its `docs/IDIOMS.md` row,
   the seams, the pipe redesign (`kern:`) and the `diff-openbsd` pipe step (`xtask:`), each with
   its `LZ:` and `Unsafe:` trailers.

The decision row proposed for `docs/PHASE2.md`:

| Date | Decision |
|---|---|
| `<date of the OK>` | Zero `unsafe` outside a listed core (`docs/ZERO_UNSAFE.md`): every module outside `unsafe-core.toml` compiles under `#[forbid(unsafe_code)]`, set on its `mod` declaration; the core (hardware, boot, the host harness, a runtime of primitives) is budgeted and reviewed; the legacy total only falls, an LZ sync aside; "redesigned" means "compiles with `forbid`"; N2 becomes "Core runtime" and N9 "Close" is added. |

Goal 1 reworded: "**Safety.** Zero `unsafe` outside a listed core: every module outside
`unsafe-core.toml` compiles under `#[forbid(unsafe_code)]`, and every `unsafe` inside the core is
budgeted, argued and reviewed (`docs/ZERO_UNSAFE.md`, `.claude/rules/unsafe-budget.md`)."

## 10. Decisions

Taken by the user on 2026-10-10 ("ta bien, demosle, igual esto es iterativo. lz check porfa"):

1. **The goal: adopted** as stated in section 1.
2. **The initial core list: as listed** in section 4, `explicit_bzero.rs` included, `conf` and
   `init` left as legacy. A later addition is a commit of its own, with the user's OK.
3. **The first slice: pipe(2)** (section 6).
4. **Miri: deferred.** Decided at N9; the plan does not depend on it.
5. **The core at N9, crate or list: deferred** to N9. Nothing before it depends on it.
6. **LZ: `reimplemented` is the normal method for `forbid` modules** (section 8).
7. **The adapter: a safe trait checked by `lz check`.** `Adapter` (`sys/sys/queue.rs`) becomes a
   safe trait whose impls only `queue_adapter!` and `tree_adapter!` write; `cargo xtask lz check`
   refuses a hand-written impl. The alternative (back links that point at the element or the
   head, so no offset and no `container_of`) was set aside for now; it stays open if the
   tool-enforced boundary proves weak.
