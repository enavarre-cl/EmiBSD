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
| **N2 Core structures** | `sys/sys/queue.rs`, `sys/sys/tree.rs` and the `Cell`-everywhere header types of `sys/sys/`: ownership under the lock that guards them, typed handles where the C has raw links, `UnsafeCell` fields documented or removed; `docs/IDIOMS.md` rows for each shape | unsafe totals of `sys` and `kern` down by `<n>` and `<n>`; no `Cell<*const T>` outside the list entries of the remaining intrusive structures; every smoke and `diff-openbsd` equal |
| **N3 Memory** | `sys/uvm/`: amap, aobj, map, fault, the pagers, the pmaps' interface through `machine`: ownership of entries and pages, fault paths without raw pointers at the API | unsafe total of `uvm` at `<n>`; `selftest=mpstress` and the uvm smokes pass; `bench` boot time and vioblk throughput not worse than the baseline |
| **N4 Processes and scheduling** | `sys/kern/`: proc, process, the scheduler and its run queues, synch (sleep queues), signals, kthreads, the file descriptor table | unsafe total of `kern` at `<n>`; `smoke-mp`, `smoke-ddbmp`, `smoke-login` pass; `diff-openbsd` syscall set equal |
| **N5 VFS and file systems** | `sys/kern/vfs_*`, `sys/ufs/` first, then the other file systems: vnode ownership, buffer cache, the VOP table as a trait | unsafe totals of the file-system subsystems at `<n>`; every `smoke-disk`, `smoke-fs`, `smoke-nfs`, `smoke-ufsopts`, `smoke-ext2fs`, `smoke-ntfs`, `smoke-fuse` pass; `diff-openbsd` fs set equal; `bench` vioblk throughput not worse |
| **N6 Network stack** | `sys/net/`, `sys/netinet/`, `sys/netinet6/` except the security areas: mbufs as owned buffers, interfaces, routing, sockets, TCP | unsafe totals at `<n>`; the two-VM smokes pass; `bench` tcpbench not worse than the baseline |
| **N7 Devices** | `sys/dev/` and the bus layers: typed `bus_space` accessors, owned DMA buffers, the autoconf driver model as traits, one bus directory at a time | unsafe totals of `dev/*` at `<n>`; every device smoke passes on both archs |
| **N8 Security subsystems** | pf, pfsync, pflog, WireGuard, IPsec and pfkeyv2, softraid CRYPTO, the crypto framework, pledge, unveil, exec, under `security-review.md` | unsafe totals at `<n>`; `Security-Review:` on every commit; `smoke-pf`, `smoke-pfsync`, `smoke-wg`, `smoke-esp`, `smoke-ipcomp`, `smoke-softraid`, `smoke-login` pass; `cargo xtask lz drift --security` empty |

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
