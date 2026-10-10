# integrator: lessons across runs

One line per lesson, appended at the end; task state goes in HANDOFF.md
(`.claude/rules/subagents.md`, "Memory"). Committed with the tree.
- LZ sync, parallel LZ branches (M16c drivers): each apply conflicts where LZ's merge joined them (pcidevs.rs, mii/miidevs.rs, the ic/pci/mii mod.rs, amd64 ioconf numbering, the smokes list); resolve every one to `git show <merge>:<path>` (ioconf: cfdata renumbered, PV_MII, NCFDATA, MP cpu* as LZ's merge), and record that merge `applied` with `modules` = the shared [[module]]s, or `lz check` compares them with a parent and fails.
- LZ sync: the auto-merge of two LZ branches' pcidevs.rs additions declared PCI_VENDOR_COMPEX twice (ne and dc); take LZ's merge version of shared id tables instead of trusting a clean auto-merge.
- LZ sync: a record's `modules` takes only lineage [[module]] paths (`lz drift` rejects mod.rs, sys/machine/*, and [[extra]]s such as sys/arch/*/conf/ioconf.rs), even when drift.txt lists them.
- LZ sync: tools/xtask/src/main.rs, diffopenbsd.rs and .claude/rules/xtask.md conflict on most xtask commits (native has `lz` where LZ has `ports`); keep ours and insert LZ's new option text by hand, checking against `git show lz/main:<path>` when two LZ branches touched it.
- Never `git commit --amend -F msg` after a message builder that can fail: a stale message file once retitled the wrong commit; have the builder delete the old file first and chain build && commit.
- LZ sync: a scripted "ours + theirs" resolution (by LZ merge context) is wrong when both sides end with the same lines outside the conflict (two reference-backed tests in pcidevs.rs): it dropped the first test's closing; build after every scripted resolution.
- LZ sync: when LZ's resolving merge also carries another chunk's branch (475b49bb = M16a + M16d), check the resolution with `git merge-file` (worktree, LZ@other's base, LZ@other) against LZ@merge; a shared inherited table then equals no LZ commit and `lz check` reports it until the integration records that merge.
- Host python3 here is 3.9 (no tomllib); run TOML helpers with python3.13. The worktree sandbox refuses git in a command with variables, $(...) or heredocs: keep git calls literal, or inside a helper script.
