# Journal

One section per milestone of `docs/ROADMAP.md`, M0 to M12+: what went well, what failed, the
idioms that took more than one attempt, the rules that had to be corrected, and reproducible
numbers. It is part 2 of milestone M12+ (measurement and verification). It is the record that
Phase 2 (`docs/PHASE2.md`) measures itself against, so it states only what the history shows.

Sources: commit messages, `ports.toml` and `.claude/rules/` at each boundary, `git log -p` of
`docs/C_TO_RUST.md`, and the ROADMAP "Met" notes. No statement here comes from memory.
`Effort` and `Time` are left for the user; nothing in git records them.

## How the numbers were obtained

The ROADMAP "Met" notes carry dates, not hashes, and there are no tags. Each boundary below is the
commit that declared the milestone met: its subject says so ("M10b met", "M7b done"), or, for
M2 to M6 and M8, `docs/STATUS.md` at that commit says "Mx done". History is linear
(`git log --merges` prints nothing). Work from parallel worktree branches was rebased onto
`main`, so author dates are not monotonic; ranges follow commit order, not dates.

A milestone's range is `<previous boundary in commit order>..<its boundary>`. M0 counts from the
root commit. For each boundary `<h>`:

```sh
git rev-list --count <prev>..<h>                      # commits in the range
git show <h>:ports.toml | grep -c 'status = "ported"' # files ported at the boundary
git grep -c '#\[test\]' <h> -- sys tools init | awk -F: '{s+=$NF} END {print s}'
git show <h>:justfile | grep -cE '^smoke[a-z0-9-]*( [^:=]*)?:([^=]|$)'   # smoke recipes
```

"Ported" counts `ports.toml` rows at that commit, so a delta includes rows promoted from `wip`.
"Tests" counts `#[test]` attributes, ignored reference tests included. "Smoke recipes" counts
every justfile recipe whose name starts with `smoke` (the `smokes` list only exists since
`2358f7c`, after M11), so `smoke` itself, `smoke-link` and `smoke-internet` are in the count.

| Milestone | Boundary | Date | Range used | Commits | Ported | Tests | Smokes |
|---|---|---|---|---|---|---|---|
| M0 | `fdc1f5c` | 2026-10-02 | root..`fdc1f5c` | 11 | 16 | 33 | 1 |
| M1 | `8a34cc0` | 2026-10-02 | `fdc1f5c..8a34cc0` | 5 | 19 | 57 | 1 |
| M2 | `e35eb3f` | 2026-10-02 | `8a34cc0..e35eb3f` | 4 | 28 | 92 | 1 |
| M3 | `eed7f11` | 2026-10-02 | `e35eb3f..eed7f11` | 3 | 28 | 130 | 1 |
| M4 | `d2ec858` | 2026-10-02 | `eed7f11..d2ec858` | 3 | 39 | 151 | 1 |
| M5 | `1ecf80e` | 2026-10-02 | `d2ec858..1ecf80e` | 6 | 54 | 186 | 1 |
| M6 | `494a597` | 2026-10-03 | `1ecf80e..494a597` | 3 | 67 | 189 | 1 |
| M7a | `6e5d56d` | 2026-10-03 | `494a597..6e5d56d` | 15 | 82 | 227 | 1 |
| M7+ (M7b) | `fb3e637` | 2026-10-03 | `6e5d56d..fb3e637` | 21 | 183 | 463 | 1 |
| M8 | `56114e8` | 2026-10-03 | `fb3e637..56114e8` | 21 | 255 | 602 | 2 |
| M8b | `8ae7616` | 2026-10-03 | `56114e8..8ae7616` | 14 | 273 | 655 | 3 |
| M9a | `7b739cf` | 2026-10-03 | `8ae7616..7b739cf` | 23 | 331 | 836 | 6 |
| M9b | `385e366` | 2026-10-03 | `7b739cf..385e366` | 7 | 337 | 853 | 7 |
| M9d | `b731ee8` | 2026-10-03 | `385e366..b731ee8` | 7 | 361 | 943 | 8 |
| M9c, M9 | `4c17194` | 2026-10-03 | `b731ee8..4c17194` | 8 | 382 | 965 | 10 |
| M10a | `f44a414` | 2026-10-04 | `4c17194..f44a414` | 82 | 449 | 1230 | 19 |
| M9+ | `c275365` | 2026-10-04 | `f44a414..c275365` | 18 | 484 | 1408 | 20 |
| M10b | `06477e3` | 2026-10-04 | `c275365..06477e3` | 8 | 491 | 1429 | 21 |
| M10c | `977aab3` | 2026-10-04 | `06477e3..977aab3` | 15 | 531 | 1571 | 22 |
| M10f | `72f8139` | 2026-10-04 | `977aab3..72f8139` | 14 | 543 | 1670 | 23 |
| M10e | `3a2f81b` | 2026-10-04 | `72f8139..3a2f81b` | 16 | 569 | 1796 | 24 |
| M10d, M10 | `7d89a82` | 2026-10-04 | `3a2f81b..7d89a82` | 11 | 608 | 1889 | 27 |
| M11a | `4a86c4a` | 2026-10-04 | `7d89a82..4a86c4a` | 9 | 625 | 1905 | 28 |
| M11b | `8f0bd35` | 2026-10-04 | `4a86c4a..8f0bd35` | 5 | 627 | 1905 | 28 |
| M11c | `23d46ae` | 2026-10-04 | `8f0bd35..23d46ae` | 7 | 640 | 1923 | 29 |
| M11d | `c3bee3c` | 2026-10-04 | `23d46ae..c3bee3c` | 3 | 642 | 1926 | 30 |
| M11e, M11 | `fb26a93` | 2026-10-04 | `c3bee3c..fb26a93` | 13 | 645 | 1931 | 32 |
| M12 | not met | | | | | | |
| M12+ | not met | | | | | | |

The table is in commit order. M9+ and M10a overlap: M10a's boundary landed before M9+'s, so the
`4c17194..f44a414` range holds most of M9+'s work. The M9+ section gives the two together.

## M0 Toolchain & boot

Boundary `fdc1f5c` ("docs: close milestone M0"). Range: root..`fdc1f5c`.

- Went well: both archs boot under EDK2 and Limine 12.9.1 and `just ci` is green (`fdc1f5c`).
  libkern helpers, the `sys/sys` base headers and `MachineParam` landed in the same range.
- Failed: the `limine` crate could not be used. 0.6 needs a nightly feature, and 0.5 stops at base
  revision 3, which Limine 12.9.1 refuses on aarch64. The protocol structs were written from the
  spec in `sys/stand/limine.rs` instead (`cfe4fba`).
- Failed: the first pin (`0c904c6`) showed that `sys/lib/libkern/crc32.c` does not exist.
  OpenBSD's crc32 is zlib's, so it was recorded as skipped and the M1 row was corrected.
- Rules: `ec8ef95` created the twelve rule files. `cfe4fba` rewrote the dependency allowlist and
  `boot-and-link.md` to drop `limine`.
- Numbers: 11 commits (root included); ported 16; tests 33; smoke recipes 1 (`smoke`).

Effort: _(user)_

Time: _(user)_

## M1 libkern + sys/sys

Boundary `8a34cc0` ("milestone M1 closes"). Range `fdc1f5c..8a34cc0`.

- Went well: `queue.h` (six list families) and `tree.h` with `subr_tree.c`. The red-black
  invariants are checked after every insert and remove (`8a34cc0`).
- Failed: the planned `intrusive-collections` crate was dropped unused (`b5ad0f8`). The reason:
  depending on a crate's policy would bind the project as `limine`'s nightly requirement did.
- Idioms: the `queue.h` row of `docs/C_TO_RUST.md` was rewritten from the crate to in-house
  adapters (`35b7e6a`); the `tree.h` row was rewritten again one commit later (`8a34cc0`).
- Rules: file section order and `<name>/tests.rs` past 50 lines (`39df2cc`); one machine module
  per `<machine/*.h>` header instead of a single `api.rs` (`6485697`); the pointer rule and the
  allowlist lose `intrusive-collections` (`b5ad0f8`).
- Numbers: 5 commits; ported 16 → 19 (+3); tests 33 → 57 (+24); smoke recipes 1.

Effort: _(user)_

Time: _(user)_

## M2 console, printf, panic, ddb-lite

Boundary `e35eb3f` (subject "milestone M2"; STATUS says M2 done). Range `8a34cc0..e35eb3f`.

- Went well: `subr_prf.c`, `init_main.c`, the polled consoles and a frame-pointer backtrace on
  both archs; `boot -d` ends in a panic with status 35. `xtask symbolize` came with it (`a2b6e93`).
- Later undone: ddb-lite's "always print the trace" path was removed when the real command loop
  arrived in M11c (`cb0a8f6`).
- Rules: `scope-and-stubs.md` turned the `unported()` helper into the `unported!` macro, and the
  licence rule gained BSD-4 (`comvar.h`, amd64 `bus.h`) and the Mach notice (`ddb/`). It was the
  first of 13 commits that each added licences to that list (`e35eb3f`..`1c9b78c`) before
  `9dac599` replaced the list in M10a. `arch.md` gained the `cons`, `bus` and `db_machdep` modules.
- Numbers: 4 commits; ported 19 → 28 (+9); tests 57 → 92 (+35); smoke recipes 1.

Effort: _(user)_

Time: _(user)_

## M3 Physical memory + uvm basics

Boundary `eed7f11` (STATUS: "M3 done (parts 1 to 3 landed)"). Range `e35eb3f..eed7f11`.

- Went well: the page allocator, kernel page tables over the bootloader's, `subr_pool.c`,
  `kern_malloc.c` and a `GlobalAlloc` over malloc(9), in three parts.
- Stand-ins that later had to change: `km_alloc` served everything from the direct map; it was
  redone as the C does in M7b (`5119474`). `union pool_lock` was a flag, so a `PR_WAITOK` get
  failed instead of sleeping; real locks came in M7a (`e85ddba`) and both locks in M11a (`0033c3a`).
  `arc4random` was a SplitMix64 placeholder.
- The ported count did not move: the M3 files entered `ports.toml` as `wip` (34 → 68 rows).
- Failed: no fix-up commit in the range.
- Numbers: 3 commits; ported 28 → 28 (+0; `wip` +34); tests 92 → 130 (+38); smoke recipes 1.

Effort: _(user)_

Time: _(user)_

## M4 Traps & interrupts

Boundary `d2ec858` (STATUS: "M4 done"). Range `eed7f11..d2ec858`.

- Went well: real traps on both archs with `ddb_regs` (`145ec7c`); interrupts and spl(9) on amd64
  with the MI softintr, mutex and evcount (`37aba7f`); the arm64 device tree, GICv2 and the PL011
  receive interrupt (`d2ec858`).
- STATUS at `d2ec858` records that the user asked to stop at M4 before M5 started.
- Failed: no fix-up commit in the range.
- Rules: none changed.
- Numbers: 3 commits; ported 28 → 39 (+11); tests 130 → 151 (+21); smoke recipes 1.

Effort: _(user)_

Time: _(user)_

## M5 Timers, scheduler, proc

Boundary `1ecf80e` (STATUS: "M5 done"). Range `d2ec858..1ecf80e`.

- Went well: timecounters, clock interrupts, timeouts and hardclock (`15470ba`); the arm64 generic
  timer (`c29ff3c`); `struct proc`/`process` (`d6992e7`); sleep queues, the scheduler,
  `cpu_switchto` and kthreads (`1ecf80e`). The project was renamed EmiBSD (`4a11746`).
- Failed: no fix-up commit in the range.
- Rules: every ported file got its licence wrapped in `/* <LICENSES> */` markers, and the rules
  say to read from the closing marker (`70624bd`, a mechanical change over the tree). Beerware
  (`kern_tc.c`) joined the licence list (`15470ba`).
- Numbers: 6 commits; ported 39 → 54 (+15); tests 151 → 186 (+35); smoke recipes 1.

Effort: _(user)_

Time: _(user)_

## M6 Syscalls + minimal init

Boundary `494a597` (STATUS: "M6 done"). Range `1ecf80e..494a597`.

- Went well: syscall tables generated from `syscalls.master`, entry paths, `copyin`, exit and
  the reaper, user address spaces and `init` in user mode on both archs.
- Approach changed: demand paging was moved out of M6 into M7a (ROADMAP M7a row, decided
  2026-10-02). M6 ran `init` on wired mappings (`uvm_map_enter_wired`), replaced in M7a.
- Idioms: the system call row was rewritten. `v: *const c_void` became `v: &SysArgs`, with
  `sysargs::<SysFooArgs>(v)` as the safe `SCARG` view (`1dd14b7`).
- Failed: no fix-up commit in the range.
- Numbers: 3 commits; ported 54 → 67 (+13); tests 186 → 189 (+3); smoke recipes 1.

Effort: _(user)_

Time: _(user)_

## M7a uvm

Boundary `6e5d56d` ("M7a done"). Range `494a597..6e5d56d`.

- Went well: `kern_rwlock.c` first, so the `vm_map` lock is a real rwlock (`e85ddba`); then the
  object layer, `uvm_map.c`, `uvm_fault.c`, pv lists, exec through `uvm_map`, and `uvm_mmap.c`.
- Roadmap churn: the milestones after M7 were redrawn in five commits in this range
  (`d916411`, `549508f`, `eaabb40`, `f8ca5c4`, `9b12ab0`). "Installable" went from M9 to M14, and
  `9b12ab0` recorded the user's order M8 to M15.
- Failed: no fix-up commit in the range.
- Rules: none changed.
- Numbers: 15 commits; ported 67 → 82 (+15); tests 189 → 227 (+38); smoke recipes 1.

Effort: _(user)_

Time: _(user)_

## M7+ (M7b: network first)

Boundary `fb3e637` ("M7b done"). Range `6e5d56d..fb3e637`. ROADMAP's "M7+" row holds this
criterion (the kernel's ping to 10.0.2.2); STATUS and the commits call it M7b.

- Went well: networking before vfs, as the user chose (`9b12ab0`). Task queues, mbufs, autoconf,
  PCI, virtio, `if_vio`, routing, ARP, IPv4 and ICMP. It was the largest jump so far (+101 ported).
- Failed: making `km_alloc` follow `uvm_km.c` broke the pmap and trap selftests. They had mapped
  a page at the start of the kernel map without reserving it (`5119474`).
- Failed: `kern_sysctl.c` reported "EmiBSD 7.8" (`5471586`). It was a misreading of the pin and
  was corrected to 8.0 in M8b (`cd43627`).
- Idioms: two rows were rewritten in this range. One is headers named like Rust keywords
  (`net/if.h`, `a2257ee`); the other is structures that are only forward-declared (`d2338ae`).
- Rules: the reference clone gained the userland sources, the user's M8 decision (`8d95e93`).
- Numbers: 21 commits; ported 82 → 183 (+101); tests 227 → 463 (+236); smoke recipes 1.

Effort: _(user)_

Time: _(user)_

## M8 Minimal userland

Boundary `56114e8` (STATUS: "M8 done"). Range `fb3e637..56114e8`.

- Went well: OpenBSD's `init(8)`, `ksh` and libc, cross-compiled unmodified, on an ffs ramdisk
  made with OpenBSD's own makefs(8). `smoke-shell` runs `uname`, `cat` and `ls`.
- Failed: on amd64 the userland's first SSE instruction raised #UD, because `CR4.OSFXSR` was never
  set. Porting `option SYSCALL_DEBUG` found it, and `fpu.c` was ported for it (`56114e8`).
- Failed: `fad7400` moved the pipe entries to `todo` without their Rust paths, which
  `ports check` requires. `851747c` fixed it.
- Idioms: rows rewritten for C names that are Rust keywords (`362d1a9`) and for reading ELF
  headers out of a file (`a737765`).
- Rules: licences accepted for Dyson (`fad7400`) and zlib's crc32 (`eedef38`), and, for the
  userland, compiler-rt's Apache-2.0 WITH LLVM-exception and makefs's notices (`6ee7cd2`,
  `119297b`). `xtask.md` gained the userland toolchain exception (`9326395`).
- Numbers: 21 commits; ported 183 → 255 (+72); tests 463 → 602 (+139); smoke recipes 1 → 2
  (`smoke-shell`).

Effort: _(user)_

Time: _(user)_

## M8b Multi-user boot and login

Boundary `8ae7616` ("M8b done"). Range `56114e8..8ae7616`.

- Went well: once the socket layer was in, BSD Auth worked, and `smoke-login` reaches
  `rc: multi-user`, logs root in and runs `id`.
- Failed: the release string was corrected from 7.8 to 8.0 (`cd43627`). The pin's `newvers.sh`
  says `osr="8.0"`, so the banner, `uname`, the smoke expectations and `/etc/motd` all changed.
- Rules: three more licence additions, each by the user's decision. They were the IPsec notice
  (`77481a9`), public domain for kernel ports (`8fb9422`), and the fdlibm, Carnegie Mellon ALTQ
  and M.I.T. notices (`1c929ba`). The `libz` crate joined the allowlist (`43a6d2e`). `8fb9422`
  also split M9 into M9a..M9d.
- Numbers: 14 commits; ported 255 → 273 (+18); tests 602 → 655 (+53); smoke recipes 2 → 3
  (`smoke-login`).

Effort: _(user)_

Time: _(user)_

## M9 Network security

Boundary `4c17194` ("M9c, M9 done"). Range `8ae7616..4c17194`, the sum of M9a..M9d.

- Went well: all four parts closed on 2026-10-03, each with a two-arch smoke.
- Order: met as a, b, d, c (`7b739cf`, `385e366`, `b731ee8`, `4c17194`). pf closed before IPsec,
  and `9b051d2` wired pf into IPsec inside M9c's range.
- Numbers: 45 commits; ported 273 → 382 (+109); tests 655 → 965 (+310); smoke recipes 3 → 10.

Effort: _(user)_

Time: _(user)_

### M9a Sockets and the network userland

Boundary `7b739cf`. Range `8ae7616..7b739cf`.

- Went well: kqueue, the socket layer, `in_pcb`, UDP, raw IP and routing sockets. OpenBSD's own
  ifconfig(8), ping(8) and route(8) run from the ramdisk, and `smoke2` boots two VMs on one link.
  The crypto primitives WireGuard and ESP need also landed here (`b870ab8`..`4b6fbc7`).
- Failed: on amd64, `pmap_is_curpmap` ignored `pmap_kernel()`, so kernel unmaps from a user
  process skipped `invlpg`. Stale TLB entries let pools and pipe buffers overwrite each other, and
  a 300-round pipe stress panicked in `pool_get` (`7bcccd9`).
- Failed: `clockintr_dispatch` panicked on `attempt to subtract with overflow`. amd64's i8254
  timecounter wraps every 27.46 ms. The C's unsigned sums wrap silently, but Rust's checked `-`
  panicked. That gave a new C_TO_RUST row for arithmetic the C lets wrap (`b6a5af1`).
- Failed: merging the inet work with kqueue dropped a closing brace. The fix rode with
  `7b739cf`.
- Idioms: the kqueue rows were rewritten three times as `kern_event.c` landed (`97347c4`,
  `5bb2c88`, `287bb55`). Each rewrite took a filter from a struct standing in for the knote to a
  filter over the real `Knote`.
- Rules: `xtask.md` gained `smoke2` (`1babcc4`).
- Numbers: 23 commits; ported 273 → 331 (+58); tests 655 → 836 (+181); smoke recipes 3 → 6
  (`smoke-link`, `smoke-net`, `smoke-route`).

Effort: _(user)_

Time: _(user)_

### M9b WireGuard

Boundary `385e366`. Range `7b739cf..385e366`.

- Went well: `wg_noise.c`, `wg_cookie.c` and `if_wg.c`; two VMs ping through `wg0`.
- Failed: a full `just test` could panic with "exit write when lock not held". On the host
  `curproc` is one global, and the wg rwlock tests ran without the memory lock, taking the locks
  in the opposite order to everyone else (`e5f917c`).
- Rules: none changed.
- Numbers: 7 commits; ported 331 → 337 (+6); tests 836 → 853 (+17); smoke recipes 6 → 7
  (`smoke-wg`).

Effort: _(user)_

Time: _(user)_

### M9c IPsec

Boundary `4c17194`. Range `b731ee8..4c17194` (met after M9d).

- Went well: SA database, SPD, ESP, AH, IPIP, enc and PF_KEY (`b5c886f`). `ipsecctl` loads
  static SAs, and the two VMs ping through an ESP tunnel.
- Failed: the net test setup left interface groups and pf kifs pointing into an earlier test's
  memory. The symptom was a non-malloced free (`0499a00`).
- Workaround, later removed: the VMs forwarded IP (`net.inet.ip.forwarding=1`) until bpf(4) was
  ported in M9+ (`5164678`).
- `b9a59fa` notes that arm64's pluart overflows its input buffer on long typed lines. The same
  overflow broke `smoke-ufsopts` in M10e's range (`f933af0`).
- Numbers: 8 commits; ported 361 → 382 (+21); tests 943 → 965 (+22); smoke recipes 8 → 10
  (`smoke-ipsec`, `smoke-esp`).

Effort: _(user)_

Time: _(user)_

### M9d pf

Boundary `b731ee8`. Range `385e366..b731ee8`.

- Went well: pf, pflog, hfsc and fq_codel landed in one commit (`aa4e452`). The structures pfctl(8)
  shares keep the C layout, pinned by a layout test against clang's. OpenBSD's pfctl drives it.
- Failed: the first pf agent ran out of context with all of pf uncommitted (`46a5781`'s message).
  The rule that followed is in M9+.
- Failed: the `pf_if` test deadlocked on `pf_lock` once `if_attach` called pf too. The test now
  attaches through the hooks (`aa4e452`).
- Numbers: 7 commits; ported 337 → 361 (+24); tests 853 → 943 (+90); smoke recipes 7 → 8
  (`smoke-pf`).

Effort: _(user)_

Time: _(user)_

### M9+ Network completion

Boundary `c275365`. Combined range `4c17194..c275365` (M9+ and M10a interleave; see the table).

- Went well: TCP; pfsync and pflow; zlib and IPComp; bpf, divert and IGMP; libkvm with ps,
  fstat, vmstat and df; pledge(2) enforced; LibreSSL and HTTPS; tcpdump; the amd64 TSC; INET6.
  A guard was added: every smoke rejects "uptime went backwards" (`5ead023`).
- Failed: `tcp_input.c` needed follow-ups: clippy cleanups and 13 host tests (`fa2e16c`), then a
  review before it was marked ported (`7487180`).
- Failed: a merge race dropped the deflate tests, which were re-added (`b2adb21`). Parallel host
  tests interfered twice: the nmbclust test starved other mbuf tests (`d225665`), and two
  cryptosoft tests passed only after another test had set up the mbuf pools (`448033d`).
- Blocked, then unblocked: tcpdump waited on a licence and clone decision (`585b251`). It went
  ahead after the user accepted the notice and the clone gained `usr.sbin/hostapd` and `etc/`
  (`bd3abf0`).
- Failed: the boot image overflowed its 64 MiB and was raised to 128 MiB (`58a433a`). On a busy
  arm64 VM, `smoke-pfsync` lost typed characters; it now polls through a short shell function
  (`2f11a82`).
- Rules: `large-ports.md` says a big `.c` file gets its own subagent, with early commits and a
  handoff note (`46a5781`, after M9d's lost agent). Licences: LibreSSL, tcpdump and all of libz
  (`8a73b31`), TRW (`9301b20`), and Carnegie Mellon's BOOTP/PPP notice (`bd3abf0`). `docs.md`:
  README's `Status:` line is kept current "after it lagged at M5 while M9 closed" (`6a27031`).
- Numbers (M9+ and M10a together): 100 commits; ported 382 → 484 (+102); tests 965 → 1408 (+443);
  smoke recipes 10 → 20 (`smoke-diag`, `-disk`, `-divert`, `-https`, `-internet`, `-ipcomp`,
  `-pfsync`, `-tcp`, `-tcpdump`, `-inet6`). `smoke-internet` stayed outside `smoke` (`58a433a`).

Effort: _(user)_

Time: _(user)_

## M10 File systems

Boundary `7d89a82` ("M10d met ... and with it M10"). Range `c275365..7d89a82`, plus M10a's
share of the combined range above.

- Order: the plan was a, c, b, f, e, d (ROADMAP M10 row). They were met as a, b, c, f, e, d (the
  ROADMAP Met note).
- Numbers (`c275365..7d89a82`): 64 commits; ported 484 → 608 (+124); tests 1408 → 1889 (+481);
  smoke recipes 20 → 27.

Effort: _(user)_

Time: _(user)_

### M10a Persistent disk

Boundary `f44a414`. Range `4c17194..f44a414`, which holds most of M9+ as well. The M10a commits
are `8bc7b20`..`ddfe517` and `d82ee4d`.

- Went well: physio(9), the SCSI midlayer, `vioblk` and `sd`. `sd0` attaches on vioblk's
  scsibus, and OpenBSD's fdisk, disklabel, newfs and fsck run on a persistent disk across two
  boots.
- Failed: no fix-up commit among the M10a commits. `d82ee4d` moved two smokes' expectations to
  `/mnt` and the sd disks.
- Rules: the OSF notice and the SCIOC* files were accepted (`1c9b78c`). Then `9dac599` replaced
  the per-licence list, which 13 commits since M2 had extended, with one rule: every licence in
  the pinned OpenBSD tree is accepted, kept whole. `docs.md`: the README's status, smoke and
  progress sections are updated whenever a milestone closes (`c657a02`).
- Numbers: segment `4c17194..f44a414` is 82 commits, ported 382 → 449, tests 965 → 1230, smoke
  recipes 10 → 19, M9+ work included. No clean M10a-only figure exists.

Effort: _(user)_

Time: _(user)_

### M10b UFS options

Boundary `06477e3`. Range `c275365..06477e3`.

- Went well: QUOTA, UFS_DIRHASH and MFS as cargo features, with OpenBSD's quota tools. A full
  quota gives `Disk quota exceeded` and survives a reboot.
- Failed: no fix-up commit in the range.
- Rules: none changed.
- Numbers: 8 commits; ported 484 → 491 (+7); tests 1408 → 1429 (+21); smoke recipes 20 → 21
  (`smoke-ufsopts`).

Effort: _(user)_

Time: _(user)_

### M10c Memory and removable file systems

Boundary `977aab3`. Range `06477e3..977aab3`.

- Went well: tmpfs, msdosfs, cd9660, udf and vnd(4); FAT, ISO and UDF images made on the host
  are attached with vnconfig and read back.
- Failed: the ramdisk's disk nodes used 16 minors per unit, but the pin's `MAKEDEV` uses 64.
  Every unit but 0 named the wrong partitions (`3136769`).
- Failed: an nd6 host test asserted 15..=45 s. The C's mask gives 14..=44, so the test failed
  whenever the test order changed (`9667a03`). The kthread ping-pong selftest raced the reaper
  (seen on arm64 inside `just ci`) and now counts from before the threads exist (`0805a95`).
- Idioms: the on-disk byte-structure row (`byte_view!`) was reworded with cd9660 (`85daa88`).
- Not ported: fifofs; tmpfs and cd9660 fifos answer `EOPNOTSUPP` (ROADMAP Met note).
- Numbers: 15 commits; ported 491 → 531 (+40); tests 1429 → 1571 (+142); smoke recipes 21 → 22
  (`smoke-fs`).

Effort: _(user)_

Time: _(user)_

### M10d Other disk file systems

Boundary `7d89a82`. Range `3a2f81b..7d89a82`.

- Went well: ext2fs on both archs, checked by e2fsprogs on the Mac too; NTFS read-only on amd64;
  FUSE with our own `fusehello`.
- Failed: OpenBSD's own `fsck_ext2fs` faults on a partial last block group
  (`reference/openbsd-src/sbin/fsck_ext2fs/pass5.c:151`). The smoke uses seven whole groups.
- Failed: `mkntfs` does not build on macOS, so the NTFS test image comes from our own generator,
  checked by macOS's NTFS driver (`d366a9b`).
- Failed: host tests kept counting malloc usage in memory they had thrown away. With more file
  system tests, an ffs mount panicked "ffs_mountfs: no memory" (`0f58e8a`). The NFS programs
  pushed df's size column one digit wider, which broke `smoke-diag` (`5ab08a0`).
- Rules: `xtask.md` gained `ntfs-image` (`d366a9b`).
- Numbers: 11 commits; ported 569 → 608 (+39); tests 1796 → 1889 (+93); smoke recipes 24 → 27
  (`smoke-ext2fs`, `smoke-fuse`, `smoke-ntfs`).

Effort: _(user)_

Time: _(user)_

### M10e NFS client and server

Boundary `3a2f81b`. Range `72f8139..3a2f81b`.

- Went well: all of `nfs/`. B mounts A's export over UDP and TCP with OpenBSD's portmap, mountd
  and nfsd.
- Failed: `process_new` did not copy all of `ps_startcopy..ps_endcopy`. A child that forked
  without exec had no signal trampoline, so portmap(8) died at pc 0 (`b41e399`).
- Failed: under host load (load average 13 to 23), arm64's pluart dropped the middle of a
  150-byte typed line and `smoke-ufsopts` timed out. Smoke input is now typed in 32-byte chunks
  (`f933af0`).
- Failed: `just ci` ran `smoke-softraid` before the other smokes, which then found RAID volumes
  on the default disk and failed the thread count. Hence `--disk-set` (`47207a8`).
- Deviation: an NFSv3 status of 10001 or more becomes EIO, because `Errno` cannot hold it.
  `nfs_aiod.c` is skipped because no OpenBSD kernel compiles it.
- Numbers: 16 commits; ported 543 → 569 (+26); tests 1670 → 1796 (+126); smoke recipes 23 → 24
  (`smoke-nfs`).

Effort: _(user)_

Time: _(user)_

### M10f softraid

Boundary `72f8139`. Range `977aab3..72f8139`.

- Went well: every discipline (RAID 0, 1, 5, 6, concat, 1C, CRYPTO) over four vioblk disks, with
  degraded RAID 1 and RAID 6 still readable.
- Failed: running it found three faults outside softraid (`f10a980`). amd64's i8259 ran shared
  PCI INTx edge-triggered and lost completions, so it now level-triggers them until the I/O APIC
  (M13). sd(4) copied whole disklabels onto the stack, and RAID 5 I/O overflowed the 24 KB kernel
  stack (a triple fault). The third was a cosmetic attach line.
- Failed: the C's own chunk-id mix-up put a RAID 6 volume assembled without a chunk in the wrong
  slots. The port numbers missing chunks by position, a recorded deviation (`5818165`).
- Failed: OpenBSD's bioctl refuses `-c 6`, so RAID 6 is made by our own `sr6create`. In the same
  range, under load average 22, `smoke-inet6` timed out and now waits for the peer (`7ed2e2b`).
- `5818165` cites the i8259 fix as `4735ff5`, a hash no branch contains. It landed as `f10a980`.
- Numbers: 14 commits; ported 531 → 543 (+12); tests 1571 → 1670 (+99); smoke recipes 22 → 23
  (`smoke-softraid`).

Effort: _(user)_

Time: _(user)_

## M11 SMP

Boundary `fb26a93` ("M11e and M11 met"). Range `7d89a82..fb26a93`.

- Went well: the five parts were met in order on 2026-10-04. Since then every smoke boots the
  `multiprocessor` kernel with `-smp 4`, and `smoke-up` keeps one UP boot per arch.
- The tests grew little (+42), because M11 was checked mostly by QEMU selftests and smokes
  (`selftest=mpstress`, `smoke-mp`, `smoke-ddbmp`, `smoke-net-mp`, `smoke-tcpbench`).
- Numbers: 37 commits; ported 608 → 645 (+37); tests 1889 → 1931 (+42); smoke recipes 27 → 32.

Effort: _(user)_

Time: _(user)_

### M11a MP bring-up

Boundary `4a86c4a`. Range `7d89a82..4a86c4a`.

- Went well: APs started through the Limine MP request, the kernel lock, `kern_sched.c`, SMR,
  percpu and per-CPU pool caches; IPIs and TLB shootdowns on both archs.
- Failed: arm64's `kdata_abort`/`udata_abort` ran `uvm_fault` without the kernel lock, which is a
  race on the page queues. The lock stayed over `uvm_fault` until M11e (`0033c3a`).
- Idioms: five new rows (`55508af`): the kernel lock as functions, atomics for fields other CPUs
  read, `CiPtr`, stack objects lent as `'static`, and built-in-place `km_alloc` structures. The
  `pool_lock` row was rewritten from a `Cell<bool>` stand-in to both locks present. The cpumem
  counters row was split in two (`0033c3a`).
- Numbers: 9 commits; ported 608 → 625 (+17); tests 1889 → 1905 (+16); smoke recipes 27 → 28
  (`smoke-mp`).

Effort: _(user)_

Time: _(user)_

### M11b MP timekeeping

Boundary `8f0bd35`. Range `4a86c4a..8f0bd35`.

- Went well: `tsc.c`'s sync test on each AP, `kern_tc.c`'s `tc_lock`, and every CPU dispatching
  its own clockintr with a monotonic uptime.
- Deviation: the C prints only failed sync tests. The "sync test passed" line is a `qemu`-only
  addition so the smoke has something to match (ROADMAP Met note).
- Open after it: under load the amd64 TSC can measure high, so the clock runs slow until M13
  (STATUS blockers; accepted by the user).
- Numbers: 5 commits; ported 625 → 627 (+2); tests 1905 → 1905 (+0); smoke recipes 28 (the
  checks went into `smoke-mp`).

Effort: _(user)_

Time: _(user)_

### M11c ddb on MP

Boundary `23d46ae`. Range `8f0bd35..23d46ae`.

- Went well: the real command loop (`db_lex.c`, `db_input.c`, `db_expr.c`, `db_variables.c`,
  `db_command.c`, `db_run.c`, `ddb_sysctl`), other CPUs stopped by IPI, and `machine ddbcpu`.
- Approach changed: `-d` stops before the APs exist, as in OpenBSD, so the smoke re-enters ddb
  from the shell with `sysctl ddb.trigger=1`. ddb-lite's always-print-trace path was removed
  (`cb0a8f6`).
- Left as visible stubs: `db_examine.c`, breakpoints, watchpoints, symbols, the disassemblers,
  single-stepping.
- Numbers: 7 commits; ported 627 → 640 (+13); tests 1905 → 1923 (+18); smoke recipes 28 → 29
  (`smoke-ddbmp`).

Effort: _(user)_

Time: _(user)_

### M11d Network parallelism

Boundary `c3bee3c`. Range `23d46ae..c3bee3c`.

- Went well: 8 softnet task queues, `kern_intrmap.c`, and SMR for the interface index map,
  exercised by creating and destroying `lo` interfaces between two MP VMs.
- Failed: the exit criterion was wrong. It expected 8 softnet threads with `-smp 4`, but
  `softnet_percpu` keeps `min(8, ncpus)` (`reference/openbsd-src/sys/net/if.c:287`). The
  criterion was corrected from the C: 4 with `-smp 4`, and a `-smp 8` boot for 8.
- Numbers: 3 commits; ported 640 → 642 (+2); tests 1923 → 1926 (+3); smoke recipes 29 → 30
  (`smoke-net-mp`).

Effort: _(user)_

Time: _(user)_

### M11e MP audit and switch

Boundary `fb26a93`. Range `c3bee3c..fb26a93`.

- Went well: a real `uvm.pageqlock` and pmap locks, unlocked faults, the MPSAFE flags and
  `SY_NOLOCK` honoured, and every commented `KERNEL_LOCK` made real. tcpbench(1) runs between two
  MP VMs.
- Failed (races the audit exposed): vio's control queue lost a wakeup (`vio1: ctrl queue
  timeout`), and art, rtable, bpf and pfsync had use-after-frees without SMR (`7362a2d`).
  `soreceive` freed bytes appended during its copy-out, and the console stand-in drove the tty
  unlocked, interleaving init's lines on arm64 (`1c214cf`).
- Failed: QEMU TCG can lose a `sev`. An instrumented ping-pong lost 44 of 90,000 wakeups, so arm64
  enables the timer event stream, a deviation (`bc13011`). Under load, the arm64 AP self-check's
  10 s timeout remapped a window under a running AP (`b6ed9b4`). The boot image grew from 128 to
  192 MiB (`2dc1356`).
- Idioms: the cpumem counters row changed for the second time; `mbstat` and `evcount` moved to
  per-CPU counters (`fb26a93`).
- Rules: `testing.md`: every smoke boots the MP kernel with `-smp 4` (`fb26a93`).
- Numbers: 13 commits; ported 642 → 645 (+3); tests 1926 → 1931 (+5); smoke recipes 30 → 32
  (`smoke-tcpbench`, `smoke-up`).

Effort: _(user)_

Time: _(user)_

## M12 Devices

Under way in another branch, not merged.

On `main` since M11 met (`fb26a93..74d2491`, 9 commits; not M12 work): the smokes run in
parallel (`2358f7c`, with new `testing.md` and `xtask.md` rules); `smoke-softraid` failed under
that load on a bare "disklabels not read:" header (`9d99ac6`). `docs.md` gained the rule to escape
`|` in table code spans (`06e70e8`), and its own example needed a fix right after (`bf2f682`).

To be written when M12 is met.

Effort: _(user)_

Time: _(user)_

## M12+ Measurement and verification

To be written when M12+ is met.

Effort: _(user)_

Time: _(user)_
