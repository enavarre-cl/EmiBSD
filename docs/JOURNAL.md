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
| M12 | `9994806` | 2026-10-05 | `fb26a93..9994806` | 26 | 708 | 2143 | 38 |
| M12+ | the commit that marks it met | 2026-10-05 | `9994806..` that commit | 9 | 709 | 2164 | 38 |

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

Boundary `9994806` ("M12 met"). Range `fb26a93..9994806`.

The range is not M12 alone. It holds 9 commits made on `main` after M11 (the parallel smokes
`2358f7c`, the EDK2 boot order `d4122ad`, the Phase 2 draft `46d127b`, the M12+ row `01f34aa`,
and the pipe-escape rules), and M13's first ports, which landed before M12 met: nvme(4)
(`86a5fff`), cd(4) and vioscsi (`d4f1d7e`), and `f4462cd`. M12's own commits are the 15 from
`d4f4726` to `9994806`.

- Went well: audio(4) with azalia(4) on both archs and auich(4)/ac97(4) on amd64 played a tone
  that QEMU's WAV capture holds (`3ae1223`, `1fb511d`); the USB core, xhci(4), uhub(4) and
  umass(4) mounted a FAT stick on both archs (`41d47f0`, `aeb8890`, `008319b`). The first bulk
  transfers through xhci(4) needed no change in it (`008319b`).
- Went well: arm64 got a PCI bus on QEMU virt (pciecam, pci_machdep, GICv2m MSI, simplebus),
  so the PCIe audio and USB controllers attach there as on amd64 (`894f69c`).
- Failed: no fix-up commit in M12's own commits, judging by the subjects. The rebase onto M13's
  nvme, vioscsi and cd moved their ioconf entries after M12's, and the docs had to say so in one
  place (`1855637`).
- Left: wskbd and `ukbdmap.c` (M13), so the USB keyboard attaches but is silent (ROADMAP note).
- Idioms: `docs/C_TO_RUST.md` gained 19 rows in the range, most for USB and HID: packed USB
  descriptors as `#[repr(C)]` structures read through bounds-checked casts, host-controller
  pipes behind an `unsafe trait`, `goto out1/out2` cascades as labelled blocks, `-1` lookups
  as `Option` (`41d47f0`, `15ec571`). Two rows only gained escaped pipes (`b1f658b`).
- Rules: `testing.md` and `xtask.md` gained the parallel smokes and run directories
  (`2358f7c`); `docs.md` gained the escaped-pipe rule (`06e70e8`), whose own example needed a
  fix right after (`bf2f682`). `86a5fff` (M13) touched the rules too.
- Numbers: 26 commits in the range (15 of them M12's); ported 645 → 708 (+63, M13's nvme and
  cd included); tests 1931 → 2143 (+212); smoke recipes 32 → 38 (`smoke-audio`, `smoke-usb`,
  and M13's `smoke-nvme` and `smoke-cd` among them).

Effort: _(user)_

Time: _(user)_

## M12+ Measurement and verification

Boundary: the commit that marks M12+ met (its own hash cannot be written inside it; the log
finds it by its subject, "docs: M12+ met"). Range `9994806..` that commit: 9 commits, one of
them M14's `e252e18` (the wider reference clone), which M12+ was rebased onto.

- Went well: `cargo xtask unsafe-report` gave the Phase 2 baseline on the first pass of its
  lexer: its block count matched a plain grep for `unsafe {` over the tree (5864 then), with
  the test code counted apart (`1adc26f`).
- Went well: the OpenBSD VM installed headless with autoinstall(8) in 272 s (amd64) and 426 s
  (arm64) once the answers were right, and a run of both systems takes one to two minutes.
- Failed: the first install stopped silently at "Directory does not contain SHA256.sig": the
  snapshot's install image carries none, and the driver had no failure pattern for
  `failed; check /tmp/ai/ai.log`. With one processor the installer also left out `bsd.mp`.
  The first boot's rc.firsttime reached the Internet (fw_update), so that boot now has none.
- Failed: most first differences came from the two root file systems, not the kernels:
  `/usr/mdec/mbr`, `/etc/fstab`, `/etc/group`, and tset(1) in OpenBSD root's `.profile`,
  which clears OXTABS. Each is now avoided or normalized in the scenario that met it.
- Found: one stale stub. `amap_copy` still reported its chunking unported although
  `uvm_map_clip_start/end` had been ported in M7a-2; mount_mfs(8) printed the gap only on
  EmiBSD. Ported, and with it `uvm_amap.c` (`15dd894`).
- Found, expected: fifofs, `exec_script.c`, file mmap and core dumps are visible stubs; each
  has a step of its own and an entry in `expected.toml`. OpenBSD's GENERIC has no tmpfs, so
  EmiBSD's could not be compared; mfs was used instead.
- Idioms: none new in the kernel. In xtask, an expected difference covers a whole step, so a
  probe that hits a stub gets a section of its own: on the first run one coredump line had
  shifted a whole section.
- Rules: `docs.md` (JOURNAL.md gains its section in the commit that marks a milestone met),
  `xtask.md` (`unsafe-report --write` writes STATUS's `Unsafe` line; `diff-openbsd` is the
  only download), `testing.md` (a fourth tier, the differential tests).
- Numbers: 9 commits; ported 708 → 709 (+1, `uvm_amap.c`); tests 2143 → 2164 (+21, xtask's);
  smoke recipes 38 (no change; `diff-openbsd` is a recipe beside them); `diff-openbsd`: 102
  steps, 97 equal, 5 expected, on both archs.

Effort: _(user)_

Time: _(user)_

## M13 Storage, firmware and console

Boundary: the commit that marks M13 met ("docs: M13 met"). Range `6836684..` that commit: 57
commits. M13's own are 28; the other 29 are M14's, which ran in parallel and landed first
(efiboot for both archs, boot(8)'s kernel entries, the comp set, ld.so and shared libraries,
bsd.rd and the install media, exec_script, diskmap), and the roadmap's renumbering (M15 code
and test layout, M16 QEMU drivers, M17 real hardware). M13's first ports (nvme, vioscsi, cd)
landed before M12 met and are counted there.

- Went well: ACPI on amd64 in four steps (`e7e9f0b`, `ded4fec`, `4cd101c`, `4d45228`): the AML
  interpreter runs q35's DSDT, the MADT replaces the hardcoded tables, MSI and MSI-X work, and
  vio(4) takes its multiqueue MSI-X path through intrmap. acpitimer and acpihpet ended the
  TSC's slow clock under load, a STATUS blocker since M11.
- Went well: the console chain end to end, one subagent per step, each with host tests over a
  fake display or keyboard first: rasops and the fonts, the vt100 emulation, wsdisplay, then
  wskbd, wsmux and the USB keymaps; `smoke-kbd` types "hi" on QEMU's USB keyboard into a reader
  of `/dev/ttyC0` (`d38aae7`, `1b269c7`, `f0214ef`, `5c91e85`).
- Went well: four network drivers ping on both archs; vmx(4) runs four MSI-X queues
  (`ec05d15`, `8ccfcc5`, `6848a8f`). QEMU's rtl8139 reports revision 0x20, so OpenBSD's driver
  for it is re(4), not rl(4); a boot showed it before anything was ported.
- Failed: the host disk filled up (ENOSPC) on 2026-10-05 with agent worktrees' `target/`
  directories; finished worktrees are deleted after each integration since then. A spend limit
  paused the work from 2026-10-05 to 2026-10-07; the handoff notes and saved patches let every
  agent resume.
- Failed: CI flakes under parallel smokes: the kthread self-test's single 50 ms reap wait
  (`ef5fe6d`), a host test that swapped the global message buffer while others printed
  (`f4ec85f`), and a monitor socket path longer than macOS's 104-byte `sun_path` in a deep
  worktree (`20f5568`).
- Found: Limine's direct map leaves OVMF's VGA window out, so the first probe of the legacy
  text memory faulted (`474a727`). re(4)'s acknowledgement of RL_ISR after starting the
  simulated moderation timer lost the timer's interrupt on about one arm64 boot in three; it
  is acknowledged first now, a documented deviation (`8ccfcc5`). vmx(4)'s `intr_barrier`
  waited forever when the network self-test configured it before the APs ran; the self-test
  moved after `cpu_boot_secondary_processors`, and `kern_sched.c` stayed as the C has it
  (`6848a8f`).
- Left: arm64's ACPI (M14, with efiboot's tables), acpicpu(4), a frame buffer console
  (`wscons_machdep.c`, `cninit`), igb traffic in QEMU, and the mouse and PS/2 drivers, now M16,
  so wsmouse(4) is tested on the host only.
- Idioms: `docs/C_TO_RUST.md` gained 28 rows in the range: tables of function pointers over an
  opaque cookie (wsdisplayvar), a guard that asserts the kernel lock around em's shared state,
  structures cast to and from a common header (wsevsrc), a compile-time sort, generated tables
  kept byte for byte under `#[rustfmt::skip]`.
- Rules: `xtask.md` gained the M13 QEMU options (`--nic`, `--fb`, `--screenshot-after`,
  `--screen-text`, `--sendkey-after`); `boot-and-link.md` and `rust-kernel.md` changed with
  M14's boot loaders.
- Numbers: 57 commits in the range (28 of them M13's); ported 709 → 967 (+258, M14's
  included); tests 2164 → 2355 (+191); smoke recipes 38 → 58 (M14's included);
  `diff-openbsd`: 102 steps, 99 equal, 3 expected, on both archs. ci-full: not yet a recipe;
  `just ci` runs every smoke on `-smp 4` under the current rule, rc=0, 49 of 49 smokes.

Effort: _(user)_

Time: _(user)_

## M14 Installable

Boundary: the commit that marks M14 met ("docs: M14 met"). Range `a84d154..` that commit:
7 commits, all M14's (the cherry-picked roadmap commit `0b9acd1` included). M14 ran beside
M12+ and M13 from 2026-10-05, and 29 of its commits landed inside M13's range
(`6836684..a84d154`), where M13's section counts them and their numbers; they are named here,
not counted again: efiboot for amd64 (`4b3857b`) and arm64 (`5f07400`) with libsa and boot(8)'s
common code, the kernel's boot(8) entries (`c5cd485`), the comp set (`ad453d6`), file-backed
mmap(2) (`a184421`), ld.so and the shared libraries (`62ed876`), bsd.rd and the install media
(`37cac2b`, `24be9f5`), exec_script.c (`98ac130`), diskmap(4) (`a1c9f4a`), and the install
runs (`26816d5`, `8eed1a7`).

- Went well: OpenBSD's `install.sub` ran unmodified, under autoinstall(8), and installed the
  signed base and comp sets on both archs; the installed system boots through its own efiboot
  and compiles with OpenBSD's clang (`26816d5`, `8eed1a7`). What it needed was kernel ports
  (exec_script, diskmap, vfs_shutdown in boot()), not changes to the installer.
- Went well: clang 22.1.6 and lld built unmodified from `gnu/llvm` by OpenBSD's own glue through
  xtask's make evaluator (`ad453d6`), and ran in EmiBSD once file-backed mmap(2) was ported.
- Went well: arm64 ACPI, brought into M14 at the user's request (2026-10-08, "no podemos dejar
  la iso mala"), took the ~1,500 lines the investigation measured, plus extent(9) ported whole
  (`62c4d82`, `a54fc51`); the arm64 install runs on `virt,acpi=on` too (`7cf3624`).
- Failed: an rd(4) without an image read a stale buffer as its label and took another disk's
  DUID, so the installed system's fsck failed now and then (`7965e92`); base's ksh had been
  built with `-DSMALL` since M8, so rc.d refused to start daemons (`1f37efb`). A ci after a
  rebase failed `smoke-softraid` reproducibly: the smoke itself used a new CRYPTO volume's
  random sector 0 as fdisk's MBR template, exposed by rnd(4)'s constant seed (`53cabda`).
- Failed: three pauses: an editor hang that stopped two agents, the host disk full (ENOSPC),
  and the monthly spend limit (2026-10-05 to 2026-10-07); handoff notes and saved patches
  carried every track over. Two independent ports of exec_script.c were written; the second's
  tests and ksh smoke line were merged (`291708d`).
- Left: the GPL parts of the sets (not in the clone), lldb, efi(4) (no UEFI boot entry: the
  fallback path boots), efipxe untested, base programs not built (printf, sort, head, find,
  ...), `__thrsleep`, rnd(4)'s constant seed, acpipci's extents not handed to the PCI bus.
- Idioms: registration tables for what the C's boot programs take at link time (`SaConf`,
  `BootMd`; `docs/C_TO_RUST.md`, `4b3857b`).
- Rules: `boot-and-link.md` (Limine until boot(8) boots the kernel; the two `kernel.ld` differ;
  how efiboot is built), `xtask.md` (efiboot, efiboot-disk, comp, rdsetroot, install, `--acpi`),
  `rust-kernel.md` (the libsa, boot and efi crates).
- Numbers (this section's range only): 7 commits; ported 967 → 975 (+8); tests 2355 → 2357 (`just test`, passed);
  smoke recipes 57 → 60 (`smoke-acpi` in `smokes`; `smoke-install-arm64-acpi` and
  `smoke-install-boot-arm64-acpi` outside); `diff-openbsd`: 102 steps, 99 equal, 3 expected, 0 unexpected, on both archs. The installer was regenerated
  from the final tree: `smoke-install` rc=0 in 519 s (amd64 99 s, arm64 154 s, arm64 on ACPI 158 s; the installers 84, 145 and 149 s), `smoke-install-boot-amd64` 26 s, `-arm64` 28 s,
  `-arm64-acpi` 28 s; `just jobs=3 ci` rc=0 (1072 s, 50 of 50 smokes), standing in for ci-full, since every smoke
  already runs on `-smp 4`.

Effort: _(user)_

Time: _(user)_


## M15 Code and test layout

Boundary: the commit that marks M15 met ("docs: M15 met"). Range `a28b01c..` that commit
(a28b01c is main's smoke-kbd fix the branch was rebased onto): 7 commits, 1471 files changed
(`git diff --shortstat a28b01c HEAD`). The user's approval of M16a..M16g came in the same
range (one docs commit).

- Went well: the migration was one script (`migrate.py`, a brace-and-string lexer to find the
  inline `mod tests` blocks, then `cargo fmt`) over 1133 files, and the first compile after it
  passed. The checks that mattered were measured, not assumed: the sorted
  `cargo test -- --list` of every crate is identical before and after, and `unsafe-report`
  gave the same totals.
- Went well: the validator found the real exceptions at once: 23 ports have no licence text
  (headers, generated files, `.S` ports whose notice stays in the `.S`), and it exposed one
  genuine gap, `libkern/lib.rs`, which had never carried `libkern.h`'s BSD-3 notice (fixed in
  its own commit).
- Failed: the first M15 `just ci` was red on `smoke-kbd`. It was not the migration: it failed
  alone on main too, because the checked-out `target/userland` ramdisk was stale (built before
  `/dev/wskbd*` existed). Fixed on main by a28b01c (xtask refuses a ramdisk whose staging
  `/dev` differs from the device table); after the rebase `just userland` and `ci` were green.
- Failed: `just test-ref` had failed for libkern and libz with a relative `OPENBSD_SRC`
  (cargo runs each crate's tests from the crate's directory); the recipe passes an absolute path.
  A `ci2` run was killed by a session restart (not a test failure) and rerun.
- Failed: my first two commits were mis-split (a `git rm` still staged when I amended commit 1
  put the deletions in it); rebuilt from a temporary branch before anything was shared.
- Idioms: none for the kernel. `include_str!`/`include_bytes!` paths are relative to the file,
  so moving libz's three tests inline turned `"../testdata/"` into `"testdata/"`.
- Rules: `rust-kernel.md` (file layout is three zones, no `tests.rs`), `testing.md`,
  `porting-workflow.md`, `ports-tracker.md` (`license = "none"`, what `ports check` validates),
  `xtask.md`, CLAUDE.md step 6. Reading rule: `sed -n '/<CODE>/,/<\/CODE>/p' file.rs`.
- Numbers: 7 commits; ported 975 → 975, `ports.toml` 1351 entries (no change); tests 2357 → 2370
  (`just test`, passed; +13, layout's); `tests.rs` files 324 → 0
  (`git ls-files | grep -c '/tests\.rs$'`); `unsafe-report` totals unchanged (the "tests files"
  column 323 → 4); smoke recipes 50 (no change). `just jobs=3 ci` rc=0 in 24m13s (50 of 50
  smokes), `just userland` rc=0, `just comp` rc=0 in 50m53s, `just jobs=3 ci-full` rc=0 in
  33m34s (50 of 50 smokes on `-smp 4` in 15m08s, then the six install recipes).

Effort: _(user)_

Time: _(user)_

## M16f arm64 platform

Boundary: the commit that marks M16f met ("docs: M16f met"). Range `50a816b..` that commit:
19 commits, 17 without the two merges (`git rev-list --count`, `--no-merges`); de4c077 is
e1995f3 cherry-picked by the agintc agent, the same change twice. 46 files changed before the
docs commit (`git diff --shortstat 50a816b 6f6a0bd`: +11966, -120). Three branches: the
coordinator's (xtask options, the GPIO cluster, the power-key probe), smmu's and agintc's
(each its own Opus subagent), merged with `--no-ff`.

- Went well: the power-key question was settled by measurement, not argument. Reading the C
  showed that plgpio(4) has no interrupt and gpiokeys(4) polls a key without one through a
  function that ignores the power key, so a faithful port could not meet "system_powerdown
  shuts the system down". A small xtask helper (`diff-openbsd powerbtn`) booted the real
  OpenBSD 8.0 arm64 snapshot on the same `virt` and showed it does nothing either; the user
  chose option A (stay faithful, change the criterion), and `smoke-powerbtn` checks EmiBSD
  behaves the same.
- Went well: smmu's negative control. Handing the PCI functions their untranslated tags made
  nvme0 fail and smmu0 log fault events for the stream, so `smoke-smmu` passing means the DMA
  really is translated.
- Failed: on QEMU the SMMUv3 port first read stale pages (the NVMe root's disklabel came back
  wrong). QEMU's SMMUv3 has no EL2 (`IDR0.Hyp` clear) and ignores the EL2 invalidations the C
  issues; the driver now uses the C's own commented-out NS-EL1 forms in that case
  (docs/ARCHITECTURE.md, Deviations).
- Failed: after the editor restarted mid-milestone, a tool rejection told the agents to stop
  and wait for the user; the agintc agent's retry, on the coordinator's relayed "carry on",
  was denied as an auto-mode bypass. Work waited until the user answered in their own words
  and authorised a new agintc agent; nothing was routed around the denial.
- Failed: on GICv3 the kernel's early self-tests (`selftest=uart`, `clock`) waited for
  interrupts that agintc leaves masked after attach, as the C does; `machine::intr_enable`
  (new contract method, all three machines) lets those tests unmask them.
- Failed once each (reruns passed): `nfs_syscalls::server_socket_list_bookkeeping`
  (`pr_find_pagehead: mbufpl: page header missing`, `just test`) and amd64's half of
  `smoke-mp` in the GICv3 run (`init: uptime monotonic ok` cut short at QEMU's exit). Neither
  touches M16f code; both are recorded as flakes in STATUS.
- Gap found: `#[cfg(test)]` code in arm64-only files (`smmureg.rs`, `pte.rs`, ...) is never
  compiled by `just test`, which builds for the host only.
- Idioms: a device-tree property's `uint32_t *cells` walked by a controller is a slice
  (`ofw_gpio`'s `gpio_controller_next_pin` returns the rest of it); registries the C keeps
  as unlocked `LIST_HEAD`s are `static`s behind a `Sync` wrapper, as `acpiiort` did.
- Rules: none changed. Merging three branches that each appended `cfdata[]` entries
  renumbered the arm64 ioconf twice (smmu 47-48, plgpio 49, gpiokeys 50, agintc 51-52,
  `cpu*` 53, `PV_FDT` [0, 1, 12, 51]).
- Numbers: ported 975 → 990, `ports.toml` 1351 → 1356 entries (`cargo xtask ports status`);
  `just test` 2370 → 2391 passed; smoke recipes 50 → 53 (`smoke-smmu`, `smoke-gicv3`,
  `smoke-powerbtn`). `EMIBSD_GIC=3 just jobs=3 smoke`: 52 of 53 in 14m58s (amd64's
  `smoke-mp` half above), then `EMIBSD_GIC=3 just smoke-mp` rc=0. `just jobs=3 ci` rc=0 in
  19m34s (53 of 53 smokes in 13m07s). `just diff-openbsd` rc=0: 102 steps, 99 equal, 3 expected, 0 unexpected, on both archs.
  `unsafe-report`: kernel 7547 → 7718 blocks, 984 → 997 fn, 701 → 724 impl, 25 → 26 trait,
  322 other; tests 790 → 798 more.

Effort: _(user)_

Time: _(user)_

## M16e Platform drivers

Boundary: the commit that marks M16e met ("docs: M16e met"). Range `12e9da2..` that commit
(12e9da2 is M16f's close on main, merged into the M16e branch at 5df827b; the M16e work itself
started on 50a816b): 25 commits (with this one; `git rev-list --count --no-merges 12e9da2..`) besides 5 merges, `git diff --shortstat 12e9da2 HEAD`:
87 files changed, 27174 insertions(+), 327 deletions(-) before this commit. Six subagents in harness worktrees, at most two at a time beside the long
acpidmar one, merged into the coordinator's branch.

- Went well: UKC first. GENERIC disables `acpidmar0` and both `ipmi0` lines, so instead of
  enabling them in our ioconf (a deviation) subr_userconf.c was ported before anything else,
  and every smoke of a disabled device boots `-c` and types `enable ...` at `UKC> ` as an
  OpenBSD user would. It made `cfdata[]` mutable (a `StaticCell`, eight free slots, locator
  names), which every later ioconf addition followed.
- Went well: real OpenBSD 8.0 as the referee, a second time after M16f's power key. The
  first ports had made ichiic enable an SMBus OVMF left disabled and ipmi_acpi take `_MIN`
  where the C takes `_MAX`, so the smokes "worked". The user chose faithfulness ("A en
  ambos"); a new `cargo xtask diff-openbsd ... probe` boots the OpenBSD snapshot with the
  same QEMU devices and UKC, and it prints exactly what the faithful port prints:
  `ichiic0 ... SMBus disabled`; `ipmi0 at acpi0 ... iobase 0xca3/2`, `sendcmd fails`,
  `no SDRs IPMI disabled`, `ipmi at mainbus0 not configured`, the watchdog period still set.
  The SMBIOS half of bios.c came with it (hw.vendor, hw.product, ipmi's mainbus probe).
- Went well: swtpm per run. xtask starts one swtpm in the run directory, waits for its socket
  and kills it on any exit (`Drop`), so parallel smokes never share a TPM; TPM2_SelfTest
  through the driver's own command path is answered by swtpm (`rc 0x0`) on TIS and CRB.
- Failed: the first two subagents were launched into hand-made `git worktree add` worktrees
  and the harness blocked their writes; the main session relaunched them with
  `isolation: "worktree"`. Rule kept in the handoff: subagents only in harness worktrees.
- Failed: parallel ioconf work. Each agent appended at the same `cfdata[]` index (55 three
  times), and the M16f merge renumbered arm64 again; every merge was a hand renumbering of
  indices, `pv[]` arrays and `NCFDATA`. Two options named `--iommu` (M16e's `intel|amd`,
  M16f's `smmuv3`) merged without a textual conflict into two statics of the same name;
  they became one table of (model, arch, device).
- Failed: the first final `just ci` hung in `just test` for 40 minutes (subr_autoconf, subr_disk
  and unveil tests "running for over 60 seconds"). Not M16e's code: under CPU load (four test
  binaries at once) 30 to 40% of full `cargo test -p bsd` runs failed on main 12e9da2 too.
  The cause was global state that outlives `setup_real_memory`, which leaks each test's kernel
  memory and hands the next a fresh block: wsmux's mux table was grown (freeing the old table)
  inside wsmouse's attach, the panic unwound out of `config_attach` with `autoconf_attdet`
  raised, and every later `config_detach` slept for ever; the nfs server cache, pf_osfp's
  fingerprint list (items of pools `pfattach` initialises again; the panic left `pf_lock` held,
  hence M16b's "enter write deadlock") and the dirhash key gave the other flakes (M16f's
  "mbufpl: page header missing"). Fixed by `*_test_reset`s called from `setup_real_memory`
  (2fd4e5f): 100 loaded runs, no failure; the rule went into `testing.md`. Open: one
  `no idleproc set on CPU0` kernel panic in about 80 loaded runs (one host CPU and run queue
  for every test thread).
- Failed: diff-openbsd at first refused to run in the worktree (no `target/openbsd`; the
  mirror had moved on, so the fetch's hashes no longer matched). Cloned from the main
  checkout with `cp -Rc` (APFS clones, no extra space).
- Idioms: `cfdata[]` in a `StaticCell` edited only by UKC before autoconfiguration (C_TO_RUST);
  cfg `machine_x86` for the x86 machine items acpidmar names (`sys/machine/x86.rs`); bios.rs's
  SMBIOS code host-tested through a `#[cfg(test)] #[path = "../amd64"]` module in
  `arch/host` (ARCHITECTURE, "Host tests of arch code"), accepted for now and a candidate fix
  for the known gap that arm64-only tests are never compiled on the host.
- Rules: `testing.md` (tier 1: a test-touched global holding kernel memory or pool items gets a
  `*_test_reset` from `setup_real_memory`); `xtask.md` (`--pci-bridges`, `--iommu`, `--machine pc`, `--ipmi`, `--tpm`,
  `diff-openbsd ... probe`); docs/SETUP.md gains swtpm (installed with the user's OK on
  2026-10-08).
- Numbers: ported 990 → 1015 (`cargo xtask ports status`, totals
  171 todo, 140 wip, 1015 ported, 37 skipped, 1363 entries; M16f closed at 188/141/990/37/1356); tests bsd 2289 → 2399 (`--list`, ignored included; `just test` passed); smoke recipes 53 → 59 (smoke-ukc, smoke-ppb,
  smoke-dmar, smoke-iic, smoke-ipmi, smoke-tpm); unsafe-report kernel 7718 → 7951 blocks. `just jobs=3 ci` rc=0 in
  21m09s (59 of 59 smokes in 15m08s); `just diff-openbsd` rc=0, 102 steps, 99 equal, 3 expected, 0 unexpected, on both archs.

Effort: _(user)_

Time: _(user)_
