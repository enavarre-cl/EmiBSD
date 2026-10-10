# integrator: lessons across runs

One line per lesson, appended at the end; task state goes in HANDOFF.md
(`.claude/rules/subagents.md`, "Memory"). Committed with the tree.
- LZ sync, parallel LZ branches (M16c drivers): each apply conflicts where LZ's merge joined them (pcidevs.rs, mii/miidevs.rs, the ic/pci/mii mod.rs, amd64 ioconf numbering, the smokes list); resolve every one to `git show <merge>:<path>` (ioconf: cfdata renumbered, PV_MII, NCFDATA, MP cpu* as LZ's merge), and record that merge `applied` with `modules` = the shared [[module]]s, or `lz check` compares them with a parent and fails.
- LZ sync: the auto-merge of two LZ branches' pcidevs.rs additions declared PCI_VENDOR_COMPEX twice (ne and dc); take LZ's merge version of shared id tables instead of trusting a clean auto-merge.
- LZ sync: a record's `modules` takes only lineage [[module]] paths (`lz drift` rejects mod.rs, sys/machine/*, and [[extra]]s such as sys/arch/*/conf/ioconf.rs), even when drift.txt lists them.
- LZ sync: tools/xtask/src/main.rs, diffopenbsd.rs and .claude/rules/xtask.md conflict on most xtask commits (native has `lz` where LZ has `ports`); keep ours and insert LZ's new option text by hand, checking against `git show lz/main:<path>` when two LZ branches touched it.
- Never `git commit --amend -F msg` after a message builder that can fail: a stale message file once retitled the wrong commit; have the builder delete the old file first and chain build && commit.
- LZ sync: before cherry-picking, scan every file a chunk brings for LZ items native renamed/split/dropped ([[module.fn]] rows); M16d's rnd.rs (3feb0258) calls the C-shaped chacha_private/sha2 API N1 redesigned, so it cannot equal LZ (security area: stop and hand back).
- LZ sync: when LZ's merge M joins commit K of one branch but also carries later commits of K's branch, K's shared files (ioconf, smokes list) are M less those later commits' own hunks (60214109 = 486e0e43 less cf384384), then M's version at the later commit.
- LZ sync: run the amend of an apply alone, never beside the next cherry-pick; a subject over 72 chars fails the amend and leaves the pick on an unreworded commit (cherry-pick --abort, amend, redo).
- Worktree guard: no shell variables or loops around git/python commands and no heredoc text naming git; write scripts with Write and call them by literal path. /usr/bin/python3 is 3.9 (no tomllib): use Homebrew's python3.13 for the TOML helpers.
