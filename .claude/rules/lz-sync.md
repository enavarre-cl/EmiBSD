# Syncing with EmiBSD.LZ

LZ keeps porting OpenBSD; native EmiBSD absorbs that work deliberately, commit by commit, and
records every decision. LZ is never written to: `reference/emibsd-lz/` is a read-only clone
(`reference-readonly.md`), and nothing in LZ refers to this repository.

- When: after every LZ milestone close and every LZ pin bump, and in any case before an `N`
  milestone is marked met. `just ci` runs `cargo xtask lz drift --strict`, so an untriaged LZ
  commit is a red build.
- How: `git -C reference/emibsd-lz fetch` (the only network use besides `diff-openbsd fetch`),
  then `cargo xtask lz drift --security`. Every listed commit gets one record in
  `lz-sync.toml`, in a commit with the `lz-sync:` scope that does nothing else:
  - `applied`: an inherited module takes the change as a cherry-pick (`git -C
    reference/emibsd-lz format-patch -1 <hash>` and `git am`, or `git cherry-pick` with the
    clone as a remote); an adapted module as a cherry-pick with conflicts expected at its call
    sites, resolved by hand; a redesigned module takes it re-implemented in its new shape. The
    record names the EmiBSD commit and the `method`: `cherry-pick`, `cherry-pick-conflicts` or
    `reimplemented`. The three totals go in every `lz-sync:` commit body and in the JOURNAL:
    if `reimplemented` grows month over month, the roadmap's order is reconsidered. A file new in LZ is applied as a new `inherited` module in
    the same commit (`lineage.toml` in the same commit, as always).
  - `not-applicable`: the change touches nothing native derives from, or a path that does not
    exist here; the reason says which.
  - `covered-by-redesign`: the redesigned code cannot have the bug or already has the feature;
    the reason names the test or the type that proves it.
- Security fixes are never skipped silently. A commit `lz drift --security` lists (or any the
  triage recognises; set `security = true` by hand) is either `applied` or has a reason that
  names the fix and says why the native code is not affected. No `N` milestone closes with an
  open security record.
- The ABI equals LZ's at the pin. New syscalls, sysctl MIBs, ioctls, device majors and shared
  structs arrive only as `applied` records of LZ commits, never invented here.
- The LZ pin (`lz/PINNED.md`, `lineage.toml [meta].lz`, `lz-sync.toml [meta].lz`) moves in a
  commit titled `lz: bump pin to <12-hex>`, only when every LZ commit up to the new pin has a
  record, and only with the user's OK, as the OpenBSD pin moves in LZ.
- Trailer: `LZ: <12-hex>` (commit-only form) on every `lz-sync:` commit; the record lists the
  files.
- The runbook, step by step, is `docs/SYNC.md`; `just lz-sync` fetches and prints the list to
  triage with the items each commit touches (`lz drift --security --functions`).
