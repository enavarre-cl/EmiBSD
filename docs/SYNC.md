# Syncing with EmiBSD.LZ, step by step

The rule is `.claude/rules/lz-sync.md`; this is what is executed, usually once a month and
always before an `N` milestone is marked met. LZ is read, never written.

1. `just lz-sync`: fetches `reference/emibsd-lz`, then prints the LZ commits after the pin that
   have no record in `lz-sync.toml`, security candidates first, with the native modules and the
   items (`--functions`) each one touches.
2. Triage every commit into `lz-sync.toml`, one `[[commit]]` record each:
   - `applied`, with `emibsd = "<12-hex>"` and `method`: `cherry-pick` (an inherited module:
     `git -C reference/emibsd-lz format-patch -1 <hash> | git am`), `cherry-pick-conflicts` (an
     adapted module: conflicts expected at its call sites, resolved by hand) or `reimplemented`
     (a redesigned module: the change re-expressed in its new shape);
   - `not-applicable`, with the reason (nothing native derives from it, or the path does not
     exist here);
   - `covered-by-redesign`, with the test or the type that proves it;
   - `security = true` on every security fix; such a record is never `not-applicable` without a
     reason that names the fix.
   A file new in LZ becomes a new `inherited` module of `lineage.toml` in the same commit.
3. One `lz-sync:` commit per LZ commit or coherent cluster, nothing else in it, trailer
   `LZ: <12-hex>`; the body carries the three method totals so far.
4. `just ci` (which runs `cargo xtask lz drift --strict`), then `just diff-openbsd`.
5. When every commit up to a point has a record, with the user's OK: `lz: bump pin to <12-hex>`
   (`lz/PINNED.md`, `lineage.toml [meta].lz`, `lz-sync.toml [meta].lz` together).
6. One line in `docs/JOURNAL.md`'s current section: the LZ range absorbed, the three method
   totals and the wall time.
