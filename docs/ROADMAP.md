# Roadmap

Native milestones, numbered `N0..` so they never collide with the port's `M` numbers (LZ's
`docs/ROADMAP.md`, M0..M17; M0..M14 are also this repository's history). Every milestone has a
mechanical exit criterion: `just ci` green (which includes `cargo xtask lz check`, `lz drift
--strict` and `unsafe-report --check`), `just ci-full` green, `just diff-openbsd` equal with the
same expected list, `cargo xtask lz drift --security` empty, and the subsystem's unsafe totals
against the baseline of N0. The numbers in the criteria are filled at N0 from the tools (real
data only, `docs.md`); until then they read `<n>`.

Order (recommended, the user's call at the go): leaves first, where the tests are strongest and
the risk lowest; the security subsystems last, with their own review rule. LZ sync is a rolling
gate for every milestone, never a milestone of its own.

| Milestone | Scope | Exit criterion |
|---|---|---|
| **N0 Bootstrap** | In `EmiBSD/`, in place, from `lz-origin`: the governance rewrite applied (`CLAUDE.md`, `.claude/rules/`, `docs/PHASE2.md`, this file, `docs/IDIOMS.md`, `docs/SYNC.md`, the README); `lz/PINNED.md`; `reference/emibsd-lz/` cloned; `lineage.toml` generated 1:1 from `ports.toml` (every LZ `.rs` maps to itself, `inherited`, licences and `wip` notes carried over; the `adapted` status and the `[[module.fn]]` rows in the schema from the first commit); `cargo xtask lz {check,status,drift,trace}`, `unsafe-report --check`, `unsafe-budget.toml` at the current counts, `just lz-sync`; `lz-sync.toml` with `[meta]` only; `ports.toml`, the `ports` subcommands and `docs/PORTING.md` retired; the RCS ident lines removed (decision 20); the baseline in `docs/STATUS.md`, `unsafe-budget.toml` and the N0 section of `docs/JOURNAL.md` (unsafe totals, smoke count, `diff-openbsd` result, the inherited blockers; the benchmarks when `just bench` exists); the memory directory curated | `cargo xtask lz check` passes; `cargo xtask lz drift` prints nothing; `cargo xtask unsafe-report --check` passes; `just ci` and `just ci-full` green; `just diff-openbsd` equal to LZ's last run; the baseline numbers recorded in `docs/JOURNAL.md` |
| **N1 Leaves** | `sys/lib/libkern`, `sys/lib/libz`, the primitives of `sys/crypto/` (ciphers, hashes, MACs; not the framework): slice and `&mut [u8]` APIs, no raw pointers, `unsafe` to zero or a justified list, the C's test vectors as table-driven tests, property tests for the parsers | unsafe total of `lib/libkern`, `lib/libz` and `crypto` at 15 (from 10, 2 and 5 at `lz-origin`: 10, 0 and 5; the 10 of libkern are `StaticCell`, API frozen by the timing rule, and the `explicit_bzero` volatile store; the 5 of crypto are in the framework, N8), each remaining `unsafe` with a soundness argument; host test count (`just ci`) 2605 -> 2695; every crypto smoke passes (`smoke-wg`, `smoke-esp`, `smoke-ipcomp`, `smoke-softraid`, `smoke-https`) |
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

Since 2026-10-10 the goal is zero `unsafe` outside a listed core (`docs/ZERO_UNSAFE.md`); a row's
scope resolves as `/progress` resolves it, the most specific path winning.

## Rolling: LZ sync

After every LZ milestone close and every LZ pin bump, and before any `N` is marked met:
`cargo xtask lz drift --security`, one `lz-sync.toml` record per LZ commit, `lz-sync:` commits,
then `lz: bump pin to <12-hex>` with the user's OK (`.claude/rules/lz-sync.md`). The ABI grows
only this way (M16's and M17's devices, syscalls and sysctls arrive as `applied` records).

## Adding or changing a milestone

State the subsystem it claims, the LZ modules it derives from (they turn `redesigned` in
`lineage.toml`), and one mechanical exit criterion with numbers from the tools. Update
`docs/STATUS.md` when a milestone starts and when it closes; `docs/JOURNAL.md` gains its section
at the close.
