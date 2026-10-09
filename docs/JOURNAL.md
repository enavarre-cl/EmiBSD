# Journal

One section per milestone of `docs/ROADMAP.md`, N0 onwards: what went well, what failed, the
idioms that took more than one attempt, the rules that had to be corrected, and reproducible
numbers. Only what the history shows; `Effort` and `Time` are left for the user.

## Origin

This repository is EmiBSD.LZ's history up to `lz-origin` (`f5985f1d055a`, `docs: M16b met`,
2026-10-08), the faithful port of OpenBSD: milestones M0..M16b, 542 commits, 1860 `Upstream:`
trailers, 1042 ported and 140 wip entries of its `ports.toml`. Their journal is LZ's
`docs/JOURNAL.md` at that commit (`git show lz-origin:docs/JOURNAL.md`). From here on the port
continues in LZ and this journal records the native system.

## N0 Bootstrap

Boundary: the `docs: N0 met` commit. Range `lz-origin..` that commit (f5985f1d055a onwards).

- Went well: the governance, `lineage.toml` 1:1 from `ports.toml` and `cargo xtask lz` held
  up: every check that failed did so on its own tooling, never on the kernel. The first
  `just ci` in this tree passed all 67 smokes on its first run. The author's ISC block reached
  1200 files through LZ 44edb2c with the lz-sync machinery the milestone built, and 1017 of the
  1028 inherited modules matched LZ byte for byte with the native script as well.
- Failed: three tooling bugs from the RCS-ident drop (3a7e877), found by the first runs: `lz
  check` kept the blank line an ident left (15 false "differs from LZ"), `gen-syscalls` still
  wrote the ident line, and ten modules of licence-less C files carried an empty LICENSES zone
  without `license = "none"`. Stopping a `just ci` mid-build corrupted cargo's incremental cache
  (undefined `anon.*` symbols at link); `cargo clean -p bsd` per target fixed it. Running
  `just comp`, a subagent and the smokes at once took the load to 50 on 11 cores. The author's
  block was first planned "after" the notices, then "first"; and the push the user needed at
  once waited on a `just ci`: an explicit push order is now pushed at once.
- Idioms: none settled (N1 settles the first).
- Rules: the governance rewrite (`CLAUDE.md`, `.claude/rules/`), decisions 16 to 20
  (`docs/PHASE2.md`); the authorship rule of 2026-10-09 (the author's ISC block first in every
  `.rs`), first planned as a milestone N0b, then absorbed from LZ 44edb2c and pushed at once at
  the user's request; the milestone was dropped once it was in.
- LZ sync: `f5985f1d055a..44edb2c8323e`, 2 commits (the logo, the author's block): cherry-pick
  1, cherry-pick-conflicts 1, reimplemented 0.
- Numbers (at `lz-origin` unless said):
  - `cargo xtask unsafe-report`: kernel 8198 blocks, 1041 fn, 768 impl, 27 trait, 323 other,
    total 10357; tests 816 more. After libz's redesign (N1, already in): 10355.
  - `cargo xtask lz status`: 1028 modules, all inherited, 35 extras, 0 dropped (`git ls-files`
    of `sys/` and `tools/`: 1200 `.rs`); at this commit 1011 inherited, 4 adapted, 13 redesigned.
  - `just ci`: 67 smokes on both archs, host tests 2605 passed, 0 failed, 229 ignored, 1150 s.
  - `just ci-full`: green, 1655 s: the 67 smokes on `-smp 4`, the installer end to end on amd64,
    arm64 and arm64 ACPI (install 141, 153 and 228 s, each then booted).
  - `just diff-openbsd`: amd64 and arm64, 102 steps each, 99 equal, 3 expected (fifofs, the
    `kern.ostype` branding, core dumps), 0 unexpected.
  - `just comp`: both archs, the second (resumed) run 465 s.
  - LZ sync: 2 commits absorbed (see Rules); `lz drift --strict` 0 open.
  - Commits: `git log --oneline f5985f1d055a..` this commit.

Effort: _(user)_

Time: _(user)_

## N1 Leaves

Boundary: the `docs: N1 met` commit. Range `8e090e1..` that commit (N1's libkern and libz
commits, 33785a0..d6cfca6, landed just before N0's close and belong here).

- Went well: one subagent per area (libkern on Sonnet; libz, crypto-hash and crypto-cipher on
  Opus), each in its own worktree with a brief and a handoff note, and an independent Opus
  reviewer per security-sensitive branch. Every output stayed bit-identical: zlib's byte-for-byte
  vectors, the standards' known-answer vectors (FIPS 180-4, RFC 1321, 2104/4231, 7693, FIPS 197,
  SP 800-38D, RFC 8439, 7748, FIPS 81, RFC 2144) and `diff-openbsd` equal on both archs after
  every branch. libz lost its two `unsafe` (fallible allocation through `Vec::try_reserve_exact`
  and `Box<[T; 1]>`). The reviews found an inherited gap and closed it: LZ's cryptosoft "wiped"
  key schedules and HMAC states by storing an enum tag, leaving keys in freed session memory;
  every schedule and keyed context now zeroes itself on drop, as the C's `explicit_bzero` does.
- Failed: the first reviewer's trailer was pasted with its heading ("Security-Review: (trailer
  text) Security-Review: ...") into the seven published crypto-hash commits (8abd30a..417452c);
  left as published. A subagent's rebase left intermediate commits unbuilt; a per-commit
  `cargo check` on host, amd64 and arm64 proved all twelve. The cipher reviewer approved with
  four minor findings (a zero-round AES panic, three contexts without a wiping `Drop`, a stack
  copy of softraid's mask-key schedule, `PartialEq` on a key schedule); fixed in five commits and
  re-reviewed. The worktrees started at `lz-origin`, not `main`: every agent reset to `main`.
- Idioms: three rows in docs/IDIOMS.md: a C status code -> `Result` (libz), a hash/MAC context
  with free functions -> a type with `new`/`update`/`finalize(&mut self)` that wipes in place
  (`finalize(self)` would wipe a copy), and a key schedule wiped by its own `Drop`, no longer
  `Copy`.
- Rules: none changed; the brief for crypto (security-review.md applied in full) is the pattern
  for N8.
- Numbers:
  - `cargo xtask unsafe-report`: lib/libkern 10 -> 10, lib/libz 2 -> 0, crypto 5 -> 5; kernel
    total 10357 -> 10355.
  - `cargo xtask lz status`: 37 redesigned, 26 adapted (call sites only), 965 inherited; 111
    `[[module.fn]]` rows. libkern 5 redesigned, libz 10 (adler32 and crc32 for their tests),
    crypto 22 (spr, sk and podd stay inherited: constant tables).
  - Host tests (`just ci`): 2605 -> 2695 passed, 0 failed, 229 ignored; libz alone 85 -> 123
    (the user found 90 too few: dictionaries, the zlib and raw wrappers, every truncation and
    single-bit flip of eight streams, byte-at-a-time streaming with every flush mode,
    inflateBack, 60480 init parameter combinations, deflateBound, resets, checksums; no bug).
  - `just ci`: 67 of 67 smokes after each branch (1131-1404 s); at the close smoke-uhci failed
    once on arm64 with the inherited EDK2 UhciDxe ASSERT before the kernel, then passed on both
    archs, as did the checks after it; `just ci-full`: green in two parts: 66 of 67 smokes on `-smp 4`; `smoke-softraid` hung 2 h 30 min in
    macOS `_dyld_start` (xtask never reached `main`, QEMU never started, so its own time limit never
    ran), was killed and passed alone on 4 CPUs; then the checks and the installer end to end on
    amd64, arm64 and arm64 ACPI (install 98, 155 and 162 s, each booted; 554 s).
  - `just diff-openbsd`: amd64 and arm64, 102 steps, 99 equal, 3 expected, 0 unexpected.
  - LZ sync: no LZ commit after 44edb2c; `lz drift --security` empty.
  - Commits: 30 (`git log --oneline 8e090e1..` this commit, plus 33785a0..d6cfca6).

Effort: _(user)_

Time: _(user)_
