/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

/* <CODE> */
//! `cargo xtask unsafe-report [--write]`: how much `unsafe` the kernel holds, per subsystem.
//!
//! The Phase 2 baseline (docs/PHASE2.md, "Metrics"). Every `.rs` file of the kernel crates is
//! read: `sys/` (the `bsd` crate, `sys/lib/libkern`, `sys/lib/libz`) and `init/` (the
//! freestanding init stand-in). `tools/xtask` is host tooling and is not counted.
//!
//! A small lexer ([`lex`]) turns each file into identifiers and punctuation, dropping comments
//! (nested block comments too), string, byte-string, C-string and raw-string literals, char
//! and byte literals, lifetimes and numbers, so an `unsafe` in a comment or a string never
//! counts and `r#unsafe` is an identifier, not the keyword. Each `unsafe` keyword is then
//! classified by the tokens that follow it ([`Kind`]).
//!
//! Test code is counted apart: the tests live inline since M15 (`#[cfg(test)] mod tests { .. }` in
//! the TESTS zone of their file), so the rule that counts them is the `cfg` one below; files
//! named `tests.rs` (and anything under a `tests/` directory) still count as test files, as do
//! files whose `mod` declaration is test-only, a file that starts with
//! `#![cfg(test)]`, and the item that follows a test-only `#[cfg(...)]` attribute (a
//! `mod tests { .. }`, a test helper `fn`, an `impl`). A `cfg` predicate is test-only when it
//! can only hold under `cargo test`: `test`, `all(.., test, ..)`, or `any(..)` whose every
//! arm is test-only. `cfg(any(test, feature = "x"))` code also builds into a kernel, so it
//! counts as kernel code.
//!
//! The grouping is in [`subsystem`] and docs/PORTING.md ("Measuring unsafe").
//! `--write` replaces the one line of docs/STATUS.md that starts with `Unsafe` with the
//! totals line ([`totals_line`]).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::Result;

const STATUS_DOC: &str = "docs/STATUS.md";
const STATUS_PREFIX: &str = "Unsafe";
const BUDGET_FILE: &str = "unsafe-budget.toml";
const CORE_FILE: &str = "unsafe-core.toml";

/// One token of a Rust source file, as far as counting `unsafe` needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Tok {
    /// An identifier or keyword (`r#name` keeps its `r#`).
    Ident(String),
    /// One punctuation character (`{`, `;`, `#`, ...).
    Punct(char),
    /// A plain `".."` string literal, its text as written between the quotes (escapes kept):
    /// what a `#[path = ".."]` attribute names.
    Str(String),
    /// Any other string, char, byte, number literal or a lifetime: its text does not matter.
    Lit,
}

/// What one `unsafe` keyword introduces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// `unsafe { .. }`.
    Block,
    /// `unsafe fn name`, `unsafe extern "C" fn name`.
    Fn,
    /// `unsafe impl`.
    Impl,
    /// `unsafe trait`, `unsafe auto trait`.
    Trait,
    /// Anything else: `unsafe extern "C" { .. }` blocks, `#[unsafe(no_mangle)]` attributes,
    /// `unsafe fn(..)` pointer types.
    Other,
}

/// Counts of each [`Kind`], plus how many files they came from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    pub(crate) files: usize,
    pub(crate) blocks: usize,
    pub(crate) fns: usize,
    pub(crate) impls: usize,
    pub(crate) traits: usize,
    pub(crate) other: usize,
}

impl Counts {
    fn add(&mut self, k: Kind) {
        match k {
            Kind::Block => self.blocks += 1,
            Kind::Fn => self.fns += 1,
            Kind::Impl => self.impls += 1,
            Kind::Trait => self.traits += 1,
            Kind::Other => self.other += 1,
        }
    }

    fn merge(&mut self, o: &Counts) {
        self.files += o.files;
        self.blocks += o.blocks;
        self.fns += o.fns;
        self.impls += o.impls;
        self.traits += o.traits;
        self.other += o.other;
    }

    fn total(&self) -> usize {
        self.blocks + self.fns + self.impls + self.traits + self.other
    }
}

/// One subsystem's kernel and test counts.
#[derive(Debug, Default)]
struct Row {
    kernel: Counts,
    test: Counts,
}

/// `unsafe-budget.toml` (`.claude/rules/unsafe-budget.md`): the kernel total each subsystem may
/// not exceed, and the zero-unsafe ratchet (docs/ZERO_UNSAFE.md, section 5).
#[derive(serde::Deserialize)]
struct Budget {
    #[serde(default, rename = "subsystem")]
    subsystems: Vec<BudgetRow>,
    #[serde(default)]
    ratchet: Option<Ratchet>,
}

#[derive(serde::Deserialize)]
struct BudgetRow {
    name: String,
    total: usize,
}

/// The two numbers that only move one way: the `forbid` modules may not fall below
/// `forbid_floor`, the legacy `unsafe` (outside the core) may not rise above `legacy_ceiling`.
#[derive(serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ratchet {
    pub(crate) forbid_floor: usize,
    pub(crate) legacy_ceiling: usize,
}

/// The rows and the ratchet of `unsafe-budget.toml`; empty and `None` when the file is absent.
fn load_budget(root: &Path) -> Result<(BTreeMap<String, usize>, Option<Ratchet>)> {
    let path = root.join(BUDGET_FILE);
    if !path.is_file() {
        return Ok((BTreeMap::new(), None));
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let b: Budget = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let rows = b
        .subsystems
        .into_iter()
        .map(|r| (r.name, r.total))
        .collect();
    Ok((rows, b.ratchet))
}

/// What `--check` says of the ratchet: one line per number on the wrong side of its bound.
pub(crate) fn ratchet_errors(r: &Ratchet, forbid: usize, legacy: usize) -> Vec<String> {
    let mut out = Vec::new();
    if forbid < r.forbid_floor {
        out.push(format!(
            "forbid modules: {forbid}, below the floor {} ({BUDGET_FILE} [ratchet]); a module lost its #[forbid(unsafe_code)]",
            r.forbid_floor
        ));
    }
    if legacy > r.legacy_ceiling {
        out.push(format!(
            "legacy unsafe: {legacy}, above the ceiling {} ({BUDGET_FILE} [ratchet]); unsafe outside unsafe-core.toml may only fall",
            r.legacy_ceiling
        ));
    }
    out
}

/// The ratchet `--write` records: the floor only rises and the ceiling only falls; with no
/// ratchet yet, today's numbers.
pub(crate) fn ratchet_next(old: Option<Ratchet>, forbid: usize, legacy: usize) -> Ratchet {
    match old {
        Some(o) => Ratchet {
            forbid_floor: o.forbid_floor.max(forbid),
            legacy_ceiling: o.legacy_ceiling.min(legacy),
        },
        None => Ratchet {
            forbid_floor: forbid,
            legacy_ceiling: legacy,
        },
    }
}

/// The three groups of docs/ZERO_UNSAFE.md, section 5, over the kernel counts (test code
/// apart): the core of `unsafe-core.toml`, the `forbid` modules, and legacy (the rest).
#[derive(Debug, Default)]
pub(crate) struct Groups {
    /// `unsafe` in the core's files.
    pub(crate) core: usize,
    /// Core files that hold any.
    pub(crate) core_files: usize,
    /// The core's `unsafe` per `unsafe-report` row.
    pub(crate) core_rows: BTreeMap<String, usize>,
    /// `unsafe` outside the core.
    pub(crate) legacy: usize,
    /// Files outside the core that hold any.
    pub(crate) legacy_files: usize,
    /// Modules (the `.rs` files under `sys/` that `lineage.toml` tracks) whose `mod`
    /// declaration is `#[forbid(unsafe_code)]`, directly or through an ancestor.
    pub(crate) forbid: usize,
    /// All those modules.
    pub(crate) modules: usize,
}

/// Every kernel `.rs` file (`sys/` and `init/`), lexed, by absolute path.
pub(crate) fn lex_tree(root: &Path) -> Result<BTreeMap<PathBuf, Vec<Tok>>> {
    let mut files = Vec::new();
    for dir in ["sys", "init"] {
        crate::walk_rs(&root.join(dir), &mut files)?;
    }
    let mut lexed = BTreeMap::new();
    for f in files {
        let src = fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        let toks = lex(&src);
        lexed.insert(f, toks);
    }
    Ok(lexed)
}

/// Which files are test-only: by name first, then by the test-only `mod x;` declarations,
/// until nothing changes (a test file's own `mod` lines declare test files too).
fn test_files(lexed: &BTreeMap<PathBuf, Vec<Tok>>) -> BTreeSet<PathBuf> {
    let mut test: BTreeSet<PathBuf> = lexed.keys().filter(|f| is_test_path(f)).cloned().collect();
    loop {
        let mut added = false;
        for (f, toks) in lexed {
            let scan = scan(toks, test.contains(f));
            for name in scan.test_mods {
                for child in child_module_paths(f, &name) {
                    if lexed.contains_key(&child) && test.insert(child) {
                        added = true;
                    }
                }
            }
        }
        if !added {
            return test;
        }
    }
}

/// The `forbid` modules as workspace-relative paths (for `lz status` and `lz check`).
pub(crate) fn forbid_modules(root: &Path) -> Result<BTreeSet<String>> {
    let lexed = lex_tree(root)?;
    Ok(forbid_files(&lexed)
        .iter()
        .map(|f| rel_path(root, f))
        .collect())
}

/// `f` relative to the workspace root, with `/` separators.
fn rel_path(root: &Path, f: &Path) -> String {
    let rel = f.strip_prefix(root).unwrap_or(f);
    rel.to_string_lossy().replace('\\', "/")
}

/// `cargo xtask unsafe-report [--write] [--check] [--shapes]`.
pub(crate) fn unsafe_report(root: &Path, write: bool, check: bool, per_module: bool) -> Result<()> {
    let lexed = lex_tree(root)?;
    let test_files = test_files(&lexed);
    let core = CoreList::load(root)?;
    let forbid = forbid_files(&lexed);

    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    let mut groups = Groups::default();
    let mut shape_rows: BTreeMap<String, Shapes> = BTreeMap::new();
    let mut shape_files: Vec<(String, Shapes)> = Vec::new();
    for (f, toks) in &lexed {
        let rel = rel_path(root, f);
        let is_test = test_files.contains(f);
        let sub = subsystem(root, &rel);
        let row = rows.entry(sub.clone()).or_default();
        let scan = scan(toks, is_test);
        let mut k = Counts::default();
        let mut t = Counts::default();
        for (kind, in_test) in scan.found {
            if in_test {
                t.add(kind);
            } else {
                k.add(kind);
            }
        }
        if is_test {
            t.files = 1;
        } else {
            k.files = 1;
        }
        row.kernel.merge(&k);
        row.test.merge(&t);

        let n = k.total();
        if core.contains(&rel) {
            groups.core += n;
            *groups.core_rows.entry(sub.clone()).or_default() += n;
            groups.core_files += usize::from(n > 0);
        } else {
            groups.legacy += n;
            groups.legacy_files += usize::from(n > 0);
        }
        if rel.starts_with("sys/") && !crate::is_structural(&rel) {
            groups.modules += 1;
            groups.forbid += usize::from(forbid.contains(f));
        }
        shape_rows.entry(sub).or_default().merge(&scan.shapes);
        if !scan.shapes.is_empty() {
            shape_files.push((rel, scan.shapes));
        }
    }

    let mut kernel = Counts::default();
    let mut test = Counts::default();
    println!(
        "{:<16} {:>5} {:>6} {:>4} {:>4} {:>5} {:>5} {:>6} | {:>5} {:>6} {:>4} {:>4} {:>5}",
        "subsystem",
        "files",
        "blocks",
        "fn",
        "impl",
        "trait",
        "other",
        "total",
        "tests",
        "blocks",
        "fn",
        "impl",
        "other"
    );
    let print_row = |name: &str, k: &Counts, t: &Counts| {
        println!(
            "{:<16} {:>5} {:>6} {:>4} {:>4} {:>5} {:>5} {:>6} | {:>5} {:>6} {:>4} {:>4} {:>5}",
            name,
            k.files,
            k.blocks,
            k.fns,
            k.impls,
            k.traits,
            k.other,
            k.total(),
            t.files,
            t.blocks,
            t.fns,
            t.impls,
            t.traits + t.other
        );
    };
    for (name, row) in &rows {
        print_row(name, &row.kernel, &row.test);
        kernel.merge(&row.kernel);
        test.merge(&row.test);
    }
    print_row("total", &kernel, &test);

    // The shapes to replace (docs/ZERO_UNSAFE.md, section 2): counted, never gated.
    println!(
        "\n{:<16} {:>10} {:>10} {:>7} {:>10}   (shapes, kernel code)",
        "subsystem", "StaticCell", "Send/Sync", "Cell<*", "UnsafeCell"
    );
    let print_shapes = |name: &str, s: &Shapes| {
        println!(
            "{:<16} {:>10} {:>10} {:>7} {:>10}",
            name, s.static_cell, s.send_sync, s.raw_cell, s.unsafe_cell
        );
    };
    let mut all_shapes = Shapes::default();
    for (name, s) in &shape_rows {
        if !s.is_empty() {
            print_shapes(name, s);
        }
        all_shapes.merge(s);
    }
    print_shapes("total", &all_shapes);
    if per_module {
        println!();
        for (rel, s) in &shape_files {
            print_shapes(rel, s);
        }
    }

    let (budget, ratchet) = load_budget(root)?;
    let shown = |r: Option<Ratchet>, f: fn(&Ratchet) -> usize| {
        r.as_ref().map_or("none".to_string(), |r| f(r).to_string())
    };
    println!("\nzero-unsafe groups (docs/ZERO_UNSAFE.md; {CORE_FILE}):");
    let core_rows: Vec<String> = groups
        .core_rows
        .iter()
        .filter(|(_, n)| **n > 0)
        .map(|(name, n)| format!("{name} {n}"))
        .collect();
    println!(
        "  core   {:>6} unsafe in {} files ({})",
        groups.core,
        groups.core_files,
        core_rows.join(", ")
    );
    println!(
        "  forbid {:>6} of {} modules (floor {})",
        groups.forbid,
        groups.modules,
        shown(ratchet, |r| r.forbid_floor)
    );
    println!(
        "  legacy {:>6} unsafe in {} files (ceiling {})",
        groups.legacy,
        groups.legacy_files,
        shown(ratchet, |r| r.legacy_ceiling)
    );
    let line = totals_line(&kernel, &test, &groups);
    println!("\n{line}");

    if write {
        let path = root.join(STATUS_DOC);
        let doc = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let new = replace_status_line(&doc, &line)?;
        fs::write(&path, new).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("wrote {STATUS_DOC}");
        // The budget only ever goes down here, the floor only up; raising a budget is a
        // commit of its own.
        let next = ratchet_next(ratchet, groups.forbid, groups.legacy);
        let mut text = String::from(
            "# Per-subsystem unsafe budget: the kernel total (blocks + fn + impl + trait + other)\n\
             # each subsystem may not exceed. `cargo xtask unsafe-report --check` (in `just ci`) fails\n\
             # when one does; `--write` lowers a budget to the current count and never raises it.\n\
             # Raising a budget is a commit of its own, with the reason in the body\n\
             # (.claude/rules/unsafe-budget.md). Names are unsafe-report's row names.\n\
             #\n\
             # [ratchet] (docs/ZERO_UNSAFE.md, section 5): the `forbid` modules may not fall below\n\
             # forbid_floor, and the `unsafe` outside unsafe-core.toml may not rise above\n\
             # legacy_ceiling; `--write` only raises the floor and lowers the ceiling.\n",
        );
        text.push_str(&format!(
            "\n[ratchet]\nforbid_floor = {}\nlegacy_ceiling = {}\n",
            next.forbid_floor, next.legacy_ceiling
        ));
        for (name, row) in &rows {
            let now = row.kernel.total();
            let total = budget.get(name).map_or(now, |b| (*b).min(now));
            text.push_str(&format!(
                "\n[[subsystem]]\nname = \"{name}\"\ntotal = {total}\n"
            ));
        }
        let path = root.join(BUDGET_FILE);
        fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("wrote {BUDGET_FILE}");
    }
    if check {
        if budget.is_empty() {
            return Err(format!(
                "{BUDGET_FILE} missing or empty; run `cargo xtask unsafe-report --write`"
            )
            .into());
        }
        let mut bad = 0usize;
        for (name, row) in &rows {
            let now = row.kernel.total();
            match budget.get(name) {
                Some(b) if now > *b => {
                    println!("error: {name}: {now} unsafe, budget {b} ({BUDGET_FILE})");
                    bad += 1;
                }
                Some(_) => {}
                None => {
                    println!(
                        "error: {name}: no budget in {BUDGET_FILE} (run `cargo xtask unsafe-report --write`)"
                    );
                    bad += 1;
                }
            }
        }
        for p in core.missing(root) {
            println!("error: {CORE_FILE}: `{p}` names no file");
            bad += 1;
        }
        match ratchet {
            Some(r) => {
                for e in ratchet_errors(&r, groups.forbid, groups.legacy) {
                    println!("error: {e}");
                    bad += 1;
                }
            }
            None => {
                println!("error: {BUDGET_FILE}: no [ratchet] table (forbid_floor, legacy_ceiling)");
                bad += 1;
            }
        }
        if bad > 0 {
            return Err(format!(
                "{bad} unsafe budget error(s): a subsystem over or without its budget, the core list or the ratchet"
            )
            .into());
        }
        println!(
            "unsafe-report --check: every subsystem within its budget; forbid {} >= {}, legacy {} <= {}",
            groups.forbid,
            shown(ratchet, |r| r.forbid_floor),
            groups.legacy,
            shown(ratchet, |r| r.legacy_ceiling)
        );
    }
    Ok(())
}

/// The stable one-line summary recorded in docs/STATUS.md.
pub(crate) fn totals_line(k: &Counts, t: &Counts, g: &Groups) -> String {
    format!(
        "{STATUS_PREFIX} (`cargo xtask unsafe-report`): kernel {} blocks, {} fn, {} impl, {} trait, \
         {} other; tests {} more; core {}, legacy {}, forbid {} of {} modules.",
        k.blocks,
        k.fns,
        k.impls,
        k.traits,
        k.other,
        t.total(),
        g.core,
        g.legacy,
        g.forbid,
        g.modules
    )
}

/// `doc` with its single line starting with [`STATUS_PREFIX`] replaced by `line`.
fn replace_status_line(doc: &str, line: &str) -> Result<String> {
    let hits = doc.lines().filter(|l| l.starts_with(STATUS_PREFIX)).count();
    if hits != 1 {
        return Err(format!(
            "{STATUS_DOC}: expected exactly one line starting with `{STATUS_PREFIX}`, found {hits}"
        )
        .into());
    }
    let mut out = String::with_capacity(doc.len());
    for l in doc.lines() {
        out.push_str(if l.starts_with(STATUS_PREFIX) {
            line
        } else {
            l
        });
        out.push('\n');
    }
    Ok(out)
}

/// `tests.rs`, or a file under a `tests/` directory.
fn is_test_path(p: &Path) -> bool {
    p.file_name().and_then(|n| n.to_str()) == Some("tests.rs")
        || p.components().any(|c| c.as_os_str() == "tests")
}

/// The files `mod name;` in `parent` may load: `name.rs` or `name/mod.rs` next to a
/// `mod.rs`/`lib.rs`/`main.rs`, inside the directory named after the file otherwise.
fn child_module_paths(parent: &Path, name: &str) -> [PathBuf; 2] {
    let dir = parent.parent().unwrap_or(Path::new(""));
    let file = parent.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let base = if matches!(file, "mod.rs" | "lib.rs" | "main.rs") {
        dir.to_path_buf()
    } else {
        dir.join(file.trim_end_matches(".rs"))
    };
    [
        base.join(format!("{name}.rs")),
        base.join(name).join("mod.rs"),
    ]
}

/// The subsystem a kernel file (path relative to the workspace root) is counted under:
///
/// - `sys/arch/<a>/..` → `arch/<a>`, `sys/lib/<l>/..` → `lib/<l>`;
/// - `sys/dev/<d>/..` → `dev/<d>` when `sys/dev/<d>` is a bus or chip directory (it has a
///   `mod.rs`: `pci`, `pv`, `ic`, `isa`, `fdt`, `ofw`, `efi`, later `usb`, ...); every other
///   file of `sys/dev` (softraid, vnd, rd, bio, cons, rnd)
///   → `dev`;
/// - any other `sys/<x>/..` → `x` (`kern`, `uvm`, `net`, `netinet`, `ufs`, `isofs`, ...);
/// - `sys/<file>.rs` → `(crate root)`, `init/..` → `init`.
pub(crate) fn subsystem(root: &Path, rel: &str) -> String {
    if rel.starts_with("init/") {
        return "init".to_string();
    }
    let parts: Vec<&str> = rel.strip_prefix("sys/").unwrap_or(rel).split('/').collect();
    match parts.as_slice() {
        [_] => "(crate root)".to_string(),
        ["arch" | "lib", second, _, ..] => format!("{}/{second}", parts[0]),
        ["dev", d, _, ..] if root.join("sys/dev").join(d).join("mod.rs").is_file() => {
            format!("dev/{d}")
        }
        [first, ..] => (*first).to_string(),
        [] => "(crate root)".to_string(),
    }
}

/// The paths of `unsafe-core.toml`: the only places `unsafe` may live (docs/ZERO_UNSAFE.md,
/// section 4). A pattern is an exact file path or a directory followed by `/**`.
#[derive(Debug)]
pub(crate) struct CoreList {
    patterns: Vec<String>,
}

#[derive(serde::Deserialize)]
struct CoreFile {
    core: CoreSection,
}

#[derive(serde::Deserialize)]
struct CoreSection {
    paths: Vec<String>,
}

impl CoreList {
    /// Read `unsafe-core.toml` at the workspace root.
    pub(crate) fn load(root: &Path) -> Result<Self> {
        let path = root.join(CORE_FILE);
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{CORE_FILE}: {e}").into())
    }

    /// Parse the file's text; every pattern must be `sys/<path>.rs` or `sys/<dir>/**`.
    pub(crate) fn parse(text: &str) -> std::result::Result<Self, String> {
        let f: CoreFile = toml::from_str(text).map_err(|e| e.to_string())?;
        for p in &f.core.paths {
            let body = p.strip_suffix("/**").unwrap_or(p);
            let exact_ok = p.ends_with(".rs") || p.ends_with("/**");
            if !p.starts_with("sys/") || !exact_ok || body.contains(['*', '?', '[']) {
                return Err(format!(
                    "`{p}`: a pattern is `sys/<path>.rs` or `sys/<dir>/**`, nothing else"
                ));
            }
        }
        Ok(CoreList {
            patterns: f.core.paths,
        })
    }

    /// Whether the workspace-relative `rel` lies in the core.
    pub(crate) fn contains(&self, rel: &str) -> bool {
        self.patterns.iter().any(|p| match p.strip_suffix("/**") {
            Some(dir) => rel
                .strip_prefix(dir)
                .is_some_and(|rest| rest.starts_with('/')),
            None => rel == p,
        })
    }

    /// The patterns that name nothing in the tree (a file renamed or gone).
    pub(crate) fn missing(&self, root: &Path) -> Vec<String> {
        self.patterns
            .iter()
            .filter(|p| match p.strip_suffix("/**") {
                Some(dir) => !root.join(dir).is_dir(),
                None => !root.join(p).is_file(),
            })
            .cloned()
            .collect()
    }
}

/// One out-of-line `mod name;` a file declares.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ChildMod {
    /// The files it may load, in the order rustc tries them (`name.rs`, then `name/mod.rs`;
    /// one file with `#[path]`).
    pub(crate) files: Vec<PathBuf>,
    /// Its own attribute, or that of an inline `mod x { .. }` around it, is
    /// `#[forbid(unsafe_code)]`.
    pub(crate) forbid: bool,
    /// Loaded through `#[path]`: the file then counts as a `mod.rs` for its own children.
    pub(crate) via_path: bool,
}

/// What [`mod_scan`] finds in one file.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ModScan {
    /// The file opens with `#![forbid(unsafe_code)]`.
    pub(crate) inner_forbid: bool,
    /// Its out-of-line module declarations, inline modules looked through.
    pub(crate) children: Vec<ChildMod>,
}

/// The module declarations of `file` (lexed as `toks`). `mod_rs` says whether its children
/// live next to it (`mod.rs`, `lib.rs`, `main.rs`, a file loaded by `#[path]`) or in the
/// directory named after it.
pub(crate) fn mod_scan(file: &Path, toks: &[Tok], mod_rs: bool) -> ModScan {
    let file_dir = file.parent().unwrap_or(Path::new("")).to_path_buf();
    let base = if mod_rs {
        file_dir.clone()
    } else {
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        file_dir.join(stem)
    };
    let (inner_forbid, start) = inner_attrs(toks, 0, toks.len());
    let mut children = Vec::new();
    let ctx = ModCtx {
        dir: base,
        file_dir: &file_dir,
        top: true,
        forbid: false,
    };
    walk_mods(toks, start, toks.len(), &ctx, &mut children);
    ModScan {
        inner_forbid,
        children,
    }
}

/// Where [`walk_mods`] is: the directory children resolve in, the file's own directory (a
/// top-level `#[path]` is relative to it), and the inline modules' `forbid`.
struct ModCtx<'a> {
    dir: PathBuf,
    file_dir: &'a Path,
    top: bool,
    forbid: bool,
}

/// Collect the `mod` declarations between `from` and `to` (one item level: other items'
/// bodies are skipped), descending into inline `mod x { .. }` blocks.
fn walk_mods(toks: &[Tok], from: usize, to: usize, ctx: &ModCtx, out: &mut Vec<ChildMod>) {
    let mut i = from;
    while i < to {
        match &toks[i] {
            Tok::Punct('{') => i = matching(toks, i).map_or(to, |e| e + 1),
            Tok::Ident(kw) if kw == "mod" => {
                let Some(Tok::Ident(name)) = toks.get(i + 1) else {
                    i += 1;
                    continue;
                };
                let attrs = outer_attrs(toks, from, i);
                let forbid = ctx.forbid || attrs.iter().any(|a| is_forbid_attr(a));
                let path = attrs.iter().find_map(|a| path_attr(a));
                match toks.get(i + 2) {
                    Some(Tok::Punct(';')) => {
                        let files = match path {
                            Some(p) if ctx.top => vec![normalize(&ctx.file_dir.join(p))],
                            Some(p) => vec![normalize(&ctx.dir.join(p))],
                            None => vec![
                                ctx.dir.join(format!("{name}.rs")),
                                ctx.dir.join(name).join("mod.rs"),
                            ],
                        };
                        out.push(ChildMod {
                            files,
                            forbid,
                            via_path: path.is_some(),
                        });
                        i += 3;
                    }
                    Some(Tok::Punct('{')) => {
                        let end = matching(toks, i + 2).unwrap_or(to).min(to);
                        let (inner, start) = inner_attrs(toks, i + 3, end);
                        let sub = ModCtx {
                            dir: normalize(&ctx.dir.join(path.unwrap_or(name))),
                            file_dir: ctx.file_dir,
                            top: false,
                            forbid: forbid || inner,
                        };
                        walk_mods(toks, start, end, &sub, out);
                        i = end + 1;
                    }
                    _ => i += 1,
                }
            }
            _ => i += 1,
        }
    }
}

/// The inner attributes (`#![..]`) at `from`: whether one is `forbid(unsafe_code)`, and the
/// index past them.
fn inner_attrs(toks: &[Tok], from: usize, to: usize) -> (bool, usize) {
    let mut i = from;
    let mut forbid = false;
    while i + 2 < to
        && toks[i] == Tok::Punct('#')
        && toks[i + 1] == Tok::Punct('!')
        && toks[i + 2] == Tok::Punct('[')
    {
        let Some(end) = matching(toks, i + 2) else {
            break;
        };
        forbid |= is_forbid_attr(&toks[i + 3..end]);
        i = end + 1;
    }
    (forbid, i)
}

/// The outer attributes (`#[..]`, the inside of each) of the item whose keyword is at `kw`,
/// looking back past a visibility (`pub`, `pub(crate)`, ...) but not before `from`.
fn outer_attrs(toks: &[Tok], from: usize, kw: usize) -> Vec<&[Tok]> {
    let mut j = kw;
    if j > from && toks[j - 1] == Tok::Punct(')') {
        if let Some(k) = matching_back(toks, j - 1)
            && k > from
            && toks[k - 1] == Tok::Ident("pub".into())
        {
            j = k - 1;
        }
    } else if j > from && toks[j - 1] == Tok::Ident("pub".into()) {
        j -= 1;
    }
    let mut attrs = Vec::new();
    while j > from && toks[j - 1] == Tok::Punct(']') {
        let Some(k) = matching_back(toks, j - 1) else {
            break;
        };
        if k <= from || toks[k - 1] != Tok::Punct('#') || toks[k] != Tok::Punct('[') {
            break;
        }
        attrs.push(&toks[k + 1..j - 1]);
        j = k - 1;
    }
    attrs
}

/// The index of the bracket that opens the one that closes at `close`.
fn matching_back(toks: &[Tok], close: usize) -> Option<usize> {
    let mut depth = 0usize;
    for i in (0..=close).rev() {
        match toks[i] {
            Tok::Punct(')' | ']' | '}') => depth += 1,
            Tok::Punct('(' | '[' | '{') => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// The inside of an attribute is `forbid(.., unsafe_code, ..)`.
fn is_forbid_attr(attr: &[Tok]) -> bool {
    matches!(attr, [Tok::Ident(f), Tok::Punct('('), .., Tok::Punct(')')] if f == "forbid")
        && attr.contains(&Tok::Ident("unsafe_code".into()))
}

/// The inside of an attribute is `path = ".."`: the path.
fn path_attr(attr: &[Tok]) -> Option<&str> {
    match attr {
        [Tok::Ident(p), Tok::Punct('='), Tok::Str(s)] if p == "path" => Some(s),
        _ => None,
    }
}

/// `p` with its `.` and `..` components folded, as the module paths rustc builds.
fn normalize(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The files of the module tree that are `#[forbid(unsafe_code)]`: from every crate root
/// (`lib.rs`, `main.rs`) down the out-of-line `mod` declarations, a module being `forbid`
/// when its declaration, an inline module around it, its own `#![forbid]` or any ancestor's
/// says so. A file no declaration reaches is not counted.
pub(crate) fn forbid_files(lexed: &BTreeMap<PathBuf, Vec<Tok>>) -> BTreeSet<PathBuf> {
    let mut out = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut stack: Vec<(PathBuf, bool, bool)> = lexed
        .keys()
        .filter(|f| {
            matches!(
                f.file_name().and_then(|n| n.to_str()),
                Some("lib.rs" | "main.rs")
            )
        })
        .map(|f| (f.clone(), false, true))
        .collect();
    while let Some((file, inherited, mod_rs)) = stack.pop() {
        if !seen.insert(file.clone()) {
            continue;
        }
        let Some(toks) = lexed.get(&file) else {
            continue;
        };
        let scan = mod_scan(&file, toks, mod_rs);
        let forbid = inherited || scan.inner_forbid;
        if forbid {
            out.insert(file.clone());
        }
        for child in scan.children {
            let Some(found) = child.files.iter().find(|c| lexed.contains_key(*c)) else {
                continue;
            };
            let child_mod_rs =
                child.via_path || found.file_name().and_then(|n| n.to_str()) == Some("mod.rs");
            stack.push((found.clone(), forbid || child.forbid, child_mod_rs));
        }
    }
    out
}

/// Counts of the shapes the zero-unsafe plan replaces (docs/ZERO_UNSAFE.md, section 2), in
/// kernel code: measured, never gated.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Shapes {
    /// Uses of `StaticCell`.
    pub(crate) static_cell: usize,
    /// `unsafe impl Send` and `unsafe impl Sync`.
    pub(crate) send_sync: usize,
    /// `Cell<*const T>` and `Cell<*mut T>`.
    pub(crate) raw_cell: usize,
    /// Uses of `UnsafeCell`.
    pub(crate) unsafe_cell: usize,
}

impl Shapes {
    fn merge(&mut self, o: &Shapes) {
        self.static_cell += o.static_cell;
        self.send_sync += o.send_sync;
        self.raw_cell += o.raw_cell;
        self.unsafe_cell += o.unsafe_cell;
    }

    fn is_empty(&self) -> bool {
        *self == Shapes::default()
    }
}

/// The [`Shapes`] of the tokens not marked as test code.
fn shapes(toks: &[Tok], in_test: &[bool]) -> Shapes {
    let ident = |i: usize, s: &str| matches!(toks.get(i), Some(Tok::Ident(x)) if x == s);
    let mut out = Shapes::default();
    for i in 0..toks.len() {
        if in_test[i] {
            continue;
        }
        if ident(i, "StaticCell") {
            out.static_cell += 1;
        } else if ident(i, "UnsafeCell") {
            out.unsafe_cell += 1;
        } else if ident(i, "Cell")
            && toks.get(i + 1) == Some(&Tok::Punct('<'))
            && toks.get(i + 2) == Some(&Tok::Punct('*'))
            && (ident(i + 3, "const") || ident(i + 3, "mut"))
        {
            out.raw_cell += 1;
        } else if ident(i, "unsafe") && ident(i + 1, "impl") && impl_of_send_sync(&toks[i + 2..]) {
            out.send_sync += 1;
        }
    }
    out
}

/// After `impl`: `<..>`? `path::Send for` or `path::Sync for`.
fn impl_of_send_sync(after: &[Tok]) -> bool {
    let mut i = 0;
    if after.first() == Some(&Tok::Punct('<')) {
        let mut depth = 0usize;
        while i < after.len() {
            match after[i] {
                Tok::Punct('<') => depth += 1,
                Tok::Punct('>') => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
    let mut last: Option<&str> = None;
    for t in &after[i..] {
        match t {
            Tok::Ident(x) if x == "for" => {
                return matches!(last, Some("Send" | "Sync"));
            }
            Tok::Ident(x) => last = Some(x),
            Tok::Punct(':') => {}
            _ => return false,
        }
    }
    false
}

/// What [`scan`] finds in one file.
pub(crate) struct Scan {
    /// Every `unsafe` keyword: its kind and whether it sits in test code.
    pub(crate) found: Vec<(Kind, bool)>,
    /// The names of the `mod name;` declarations that are test-only.
    pub(crate) test_mods: Vec<String>,
    /// The shapes to replace, in its kernel code.
    pub(crate) shapes: Shapes,
}

/// Classify every `unsafe` in `toks` and find the test-only regions; `file_is_test` makes the
/// whole file test code.
pub(crate) fn scan(toks: &[Tok], file_is_test: bool) -> Scan {
    let mut in_test = vec![file_is_test; toks.len()];
    let mut test_mods = Vec::new();
    // `#![cfg(test)]` at the top of a file.
    if let [Tok::Punct('#'), Tok::Punct('!'), Tok::Punct('['), ..] = toks
        && let Some(end) = matching(toks, 2)
        && attr_is_test_cfg(&toks[3..end])
    {
        in_test.iter_mut().for_each(|t| *t = true);
    }
    let mut i = 0;
    while i < toks.len() {
        if toks[i] == Tok::Punct('#')
            && toks.get(i + 1) == Some(&Tok::Punct('['))
            && let Some(end) = matching(toks, i + 1)
        {
            if attr_is_test_cfg(&toks[i + 2..end]) {
                mark_item(toks, end + 1, &mut in_test);
            }
            i = end + 1;
            continue;
        }
        // A `mod name;` inside test code (or in a test file) declares a test file.
        if toks[i] == Tok::Ident("mod".into())
            && in_test[i]
            && let (Some(Tok::Ident(name)), Some(Tok::Punct(';'))) =
                (toks.get(i + 1), toks.get(i + 2))
        {
            test_mods.push(name.clone());
        }
        i += 1;
    }
    let found = toks
        .iter()
        .enumerate()
        .filter(|(_, t)| **t == Tok::Ident("unsafe".into()))
        .map(|(i, _)| (classify(&toks[i + 1..]), in_test[i]))
        .collect();
    let shapes = shapes(toks, &in_test);
    Scan {
        found,
        test_mods,
        shapes,
    }
}

/// What the tokens after an `unsafe` keyword make of it.
fn classify(after: &[Tok]) -> Kind {
    let ident = |t: Option<&Tok>, s: &str| matches!(t, Some(Tok::Ident(x)) if x == s);
    match after.first() {
        Some(Tok::Punct('{')) => Kind::Block,
        Some(Tok::Ident(x)) if x == "impl" => Kind::Impl,
        Some(Tok::Ident(x)) if x == "trait" => Kind::Trait,
        Some(Tok::Ident(x)) if x == "auto" && ident(after.get(1), "trait") => Kind::Trait,
        Some(Tok::Ident(x)) if x == "fn" => fn_kind(&after[1..]),
        Some(Tok::Ident(x)) if x == "extern" => {
            // `unsafe extern "C" fn name` / `unsafe extern "C" { .. }`.
            let rest = match after.get(1) {
                Some(Tok::Lit | Tok::Str(_)) => &after[2..],
                _ => &after[1..],
            };
            if ident(rest.first(), "fn") {
                fn_kind(&rest[1..])
            } else {
                Kind::Other
            }
        }
        _ => Kind::Other,
    }
}

/// `fn name` declares a function; `fn(` is a function pointer type.
fn fn_kind(after_fn: &[Tok]) -> Kind {
    match after_fn.first() {
        Some(Tok::Ident(_)) => Kind::Fn,
        _ => Kind::Other,
    }
}

/// The index of the bracket that closes the one at `open` (`(`, `[` or `{`).
fn matching(toks: &[Tok], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, t) in toks.iter().enumerate().skip(open) {
        match t {
            Tok::Punct('(' | '[' | '{') => depth += 1,
            Tok::Punct(')' | ']' | '}') => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Mark the item that starts at `start` (after a test-only attribute) as test code: up to
/// its first `;` or through its `{ .. }` body, skipping brackets in its signature. (A
/// `mod name;` item so marked is then found by [`scan`]'s own pass.)
fn mark_item(toks: &[Tok], start: usize, in_test: &mut [bool]) {
    let mut i = start;
    while i < toks.len() {
        match &toks[i] {
            Tok::Punct('(' | '[') => match matching(toks, i) {
                Some(e) => i = e + 1,
                None => return,
            },
            Tok::Punct(';') => {
                in_test[start..=i].iter_mut().for_each(|t| *t = true);
                return;
            }
            Tok::Punct('{') => {
                let end = matching(toks, i).unwrap_or(toks.len() - 1);
                in_test[start..=end].iter_mut().for_each(|t| *t = true);
                return;
            }
            _ => i += 1,
        }
    }
}

/// The inside of `#[ .. ]` is `cfg(P)` with a test-only predicate `P`.
fn attr_is_test_cfg(attr: &[Tok]) -> bool {
    match attr {
        [Tok::Ident(c), Tok::Punct('('), pred @ .., Tok::Punct(')')] if c == "cfg" => {
            pred_is_test(pred)
        }
        _ => false,
    }
}

/// A `cfg` predicate that can only be true in a `cargo test` build.
fn pred_is_test(pred: &[Tok]) -> bool {
    match pred {
        [Tok::Ident(t)] => t == "test",
        [Tok::Ident(op), Tok::Punct('('), args @ .., Tok::Punct(')')] => {
            let args = split_args(args);
            match op.as_str() {
                "all" => args.iter().any(|a| pred_is_test(a)),
                "any" => !args.is_empty() && args.iter().all(|a| pred_is_test(a)),
                _ => false,
            }
        }
        _ => false,
    }
}

/// Split a predicate list at its top-level commas.
fn split_args(toks: &[Tok]) -> Vec<&[Tok]> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut from = 0;
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tok::Punct('(') => depth += 1,
            Tok::Punct(')') => depth = depth.saturating_sub(1),
            Tok::Punct(',') if depth == 0 => {
                out.push(&toks[from..i]);
                from = i + 1;
            }
            _ => {}
        }
    }
    if from < toks.len() {
        out.push(&toks[from..]);
    }
    out
}

/// Tokenize Rust source, dropping comments, literals, lifetimes and whitespace.
pub(crate) fn lex(src: &str) -> Vec<Tok> {
    let s: Vec<char> = src.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    let at = |i: usize| s.get(i).copied().unwrap_or('\0');
    while i < s.len() {
        let c = s[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '/' && at(i + 1) == '/' {
            while i < s.len() && s[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && at(i + 1) == '*' {
            let mut depth = 0usize;
            while i < s.len() {
                if s[i] == '/' && at(i + 1) == '*' {
                    depth += 1;
                    i += 2;
                } else if s[i] == '*' && at(i + 1) == '/' {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else if c == '"' {
            let from = i + 1;
            i = skip_string(&s, i);
            let to = i.saturating_sub(1).max(from).min(s.len());
            toks.push(Tok::Str(s[from..to].iter().collect()));
        } else if c == '\'' {
            i = skip_quote(&s, i);
            toks.push(Tok::Lit);
        } else if c.is_ascii_digit() {
            while i < s.len() && (s[i].is_alphanumeric() || s[i] == '_') {
                i += 1;
                // A fraction (`1.5`), but not a range (`1..2`) or a method (`1.max(2)`).
                if at(i) == '.' && at(i + 1).is_ascii_digit() {
                    i += 1;
                }
            }
            toks.push(Tok::Lit);
        } else if c.is_alphabetic() || c == '_' {
            let from = i;
            while i < s.len() && (s[i].is_alphanumeric() || s[i] == '_') {
                i += 1;
            }
            let word: String = s[from..i].iter().collect();
            let next = at(i);
            match (word.as_str(), next) {
                // Raw strings: r"..", r#".."#, br"..", cr"..".
                ("r" | "br" | "cr", '"' | '#') if raw_string_start(&s, i) => {
                    i = skip_raw_string(&s, i);
                    toks.push(Tok::Lit);
                }
                // Raw identifier r#name.
                ("r", '#') if at(i + 1).is_alphabetic() || at(i + 1) == '_' => {
                    let from = i + 1;
                    i += 1;
                    while i < s.len() && (s[i].is_alphanumeric() || s[i] == '_') {
                        i += 1;
                    }
                    let name: String = s[from..i].iter().collect();
                    toks.push(Tok::Ident(format!("r#{name}")));
                }
                ("b" | "c", '"') => {
                    i = skip_string(&s, i);
                    toks.push(Tok::Lit);
                }
                ("b", '\'') => {
                    i = skip_quote(&s, i);
                    toks.push(Tok::Lit);
                }
                _ => toks.push(Tok::Ident(word)),
            }
        } else {
            toks.push(Tok::Punct(c));
            i += 1;
        }
    }
    toks
}

/// Past the `".."` string that starts at `i`.
fn skip_string(s: &[char], mut i: usize) -> usize {
    i += 1;
    while i < s.len() {
        match s[i] {
            '\\' => i += 2,
            '"' => return i + 1,
            _ => i += 1,
        }
    }
    i
}

/// Past the char literal or lifetime whose `'` is at `i`.
fn skip_quote(s: &[char], i: usize) -> usize {
    let at = |j: usize| s.get(j).copied().unwrap_or('\0');
    if at(i + 1) == '\\' {
        // An escape: '\n', '\'', '\u{1F600}'.
        let mut j = i + 3;
        while j < s.len() && s[j] != '\'' {
            j += 1;
        }
        return j + 1;
    }
    if at(i + 2) == '\'' {
        return i + 3; // 'a'
    }
    // A lifetime or a label: 'a, 'static.
    let mut j = i + 1;
    while j < s.len() && (s[j].is_alphanumeric() || s[j] == '_') {
        j += 1;
    }
    j
}

/// At `i` (just past `r`, `br` or `cr`) starts `#*"`.
fn raw_string_start(s: &[char], mut i: usize) -> bool {
    while s.get(i) == Some(&'#') {
        i += 1;
    }
    s.get(i) == Some(&'"')
}

/// Past the raw string whose hashes (or quote) start at `i`.
fn skip_raw_string(s: &[char], mut i: usize) -> usize {
    let mut hashes = 0;
    while s.get(i) == Some(&'#') {
        hashes += 1;
        i += 1;
    }
    i += 1; // the opening quote
    while i < s.len() {
        if s[i] == '"' && (1..=hashes).all(|k| s.get(i + k) == Some(&'#')) {
            return i + 1 + hashes;
        }
        i += 1;
    }
    i
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(Kind, bool)> {
        scan(&lex(src), false).found
    }

    fn idents(src: &str) -> Vec<String> {
        lex(src)
            .into_iter()
            .filter_map(|t| match t {
                Tok::Ident(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn comments_and_strings_hide_unsafe() {
        let src = r####"
        // unsafe { line comment }
        /* unsafe /* nested unsafe */ still unsafe */
        /// doc: unsafe fn
        let a = "unsafe { in a string \" unsafe }";
        let b = r#"raw "unsafe" { }"#;
        let c = br##"raw bytes "# unsafe"##;
        let d = b"unsafe";
        let e = c"unsafe";
        let f = '"';
        let g = b'"';
        let h = '\'';
        let i = '\u{1F600}';
    "####;
        assert!(kinds(src).is_empty());
        assert!(!idents(src).contains(&"unsafe".to_string()));
    }

    #[test]
    fn lifetimes_are_not_char_literals() {
        // Were 'a read as the start of a char literal, `unsafe` would be swallowed.
        let src = "fn f<'a>(x: &'a u8) -> &'a u8 { unsafe { g(x) } } 'outer: loop {}";
        assert_eq!(kinds(src), vec![(Kind::Block, false)]);
    }

    #[test]
    fn raw_identifier_is_not_the_keyword() {
        assert!(kinds("let r#unsafe = 1;").is_empty());
        assert_eq!(idents("r#unsafe"), vec!["r#unsafe".to_string()]);
    }

    #[test]
    fn numbers_and_ranges() {
        let src = "let x = 1.5e3 + 0x1F_u32 as f64; for i in 0..2 { unsafe { f(i) } }";
        assert_eq!(kinds(src), vec![(Kind::Block, false)]);
        assert!(idents(src).contains(&"i".to_string()));
    }

    #[test]
    fn each_kind_is_classified() {
        let src = r#"
        unsafe fn a() {}
        pub(crate) const unsafe fn b() {}
        unsafe extern "C" fn c() {}
        unsafe extern "C" { fn d(); }
        unsafe impl Send for X {}
        unsafe trait T {}
        unsafe auto trait U {}
        #[unsafe(no_mangle)]
        fn e() { unsafe { f() } }
        type P = unsafe fn(u8);
        type Q = unsafe extern "C" fn();
    "#;
        let k: Vec<Kind> = kinds(src).into_iter().map(|(k, _)| k).collect();
        assert_eq!(
            k,
            vec![
                Kind::Fn,
                Kind::Fn,
                Kind::Fn,
                Kind::Other,
                Kind::Impl,
                Kind::Trait,
                Kind::Trait,
                Kind::Other,
                Kind::Block,
                Kind::Other,
                Kind::Other,
            ]
        );
    }

    #[test]
    fn cfg_test_items_count_as_tests() {
        let src = r#"
        unsafe fn kernel() {}
        #[cfg(test)]
        mod tests {
            #[test]
            fn t() { unsafe { x() } }
        }
        #[cfg(test)]
        fn helper(a: [u8; 4]) { unsafe { y() } }
        #[cfg(any(test, feature = "debug"))]
        fn both() { unsafe { z() } }
        #[cfg(all(test, feature = "alloc"))]
        #[allow(dead_code)]
        unsafe impl Sync for W {}
        #[cfg(not(test))]
        fn not_test() { unsafe { w() } }
        #[cfg(test)]
        use std::vec::Vec;
        unsafe impl Send for V {}
    "#;
        assert_eq!(
            kinds(src),
            vec![
                (Kind::Fn, false),
                (Kind::Block, true),
                (Kind::Block, true),
                (Kind::Block, false),
                (Kind::Impl, true),
                (Kind::Block, false),
                (Kind::Impl, false),
            ]
        );
    }

    #[test]
    fn test_mod_declarations_are_found() {
        let src = "#[cfg(test)]\nmod tests;\n#[cfg(test)]\npub(crate) mod reftest;\nmod real;\n";
        let s = scan(&lex(src), false);
        assert_eq!(
            s.test_mods,
            vec!["tests".to_string(), "reftest".to_string()]
        );
        // Inside a test file every `mod x;` is a test file.
        let s = scan(&lex("mod helpers;\nunsafe fn f() {}"), true);
        assert_eq!(s.test_mods, vec!["helpers".to_string()]);
        assert_eq!(s.found, vec![(Kind::Fn, true)]);
    }

    #[test]
    fn inner_cfg_test_marks_the_file() {
        let s = scan(&lex("#![cfg(test)]\nunsafe fn f() {}"), false);
        assert_eq!(s.found, vec![(Kind::Fn, true)]);
    }

    #[test]
    fn child_paths() {
        let [a, b] = child_module_paths(Path::new("sys/kern/mod.rs"), "tty");
        assert_eq!(a, Path::new("sys/kern/tty.rs"));
        assert_eq!(b, Path::new("sys/kern/tty/mod.rs"));
        let [a, _] = child_module_paths(Path::new("sys/kern/tty.rs"), "tests");
        assert_eq!(a, Path::new("sys/kern/tty/tests.rs"));
    }

    #[test]
    fn subsystems() {
        let root = Path::new("/nonexistent");
        assert_eq!(subsystem(root, "sys/kern/tty.rs"), "kern");
        assert_eq!(
            subsystem(root, "sys/arch/amd64/amd64/pmap.rs"),
            "arch/amd64"
        );
        assert_eq!(subsystem(root, "sys/lib/libkern/strlcpy.rs"), "lib/libkern");
        assert_eq!(subsystem(root, "sys/ufs/ffs/ffs_alloc.rs"), "ufs");
        assert_eq!(subsystem(root, "sys/dev/softraid.rs"), "dev");
        assert_eq!(subsystem(root, "sys/lib.rs"), "(crate root)");
        assert_eq!(subsystem(root, "init/src/main.rs"), "init");
    }

    #[test]
    fn status_line_is_replaced() {
        let doc = "# Status\n\nUnsafe old totals.\nNext:\n";
        let new = replace_status_line(doc, "Unsafe new.").unwrap();
        assert_eq!(new, "# Status\n\nUnsafe new.\nNext:\n");
        assert!(replace_status_line("# Status\n", "Unsafe x").is_err());
    }
    #[test]
    fn plain_strings_keep_their_text() {
        assert_eq!(
            lex(r#"#[path = "../amd64"] mod a;"#)[4],
            Tok::Str("../amd64".to_string())
        );
        // `unsafe extern "C" fn` is still a function.
        assert_eq!(
            kinds(r#"unsafe extern "C" fn f() {}"#),
            vec![(Kind::Fn, false)]
        );
    }

    /// The children of a file, as (first candidate, forbid, via_path).
    fn children(file: &str, src: &str, mod_rs: bool) -> (bool, Vec<(String, bool, bool)>) {
        let s = mod_scan(Path::new(file), &lex(src), mod_rs);
        let c = s
            .children
            .into_iter()
            .map(|c| {
                (
                    c.files[0].to_string_lossy().into_owned(),
                    c.forbid,
                    c.via_path,
                )
            })
            .collect();
        (s.inner_forbid, c)
    }

    #[test]
    fn forbid_attributes_on_mod_declarations() {
        let src = r#"
        //! docs
        pub mod plain;
        #[forbid(unsafe_code)]
        pub mod direct;
        #[cfg(feature = "msdosfs")]
        #[forbid(unsafe_code)]
        pub(crate) mod after_cfg;
        #[forbid(unsafe_code)]
        #[cfg(any(feature = "nfsclient", feature = "nfsserver"))]
        pub mod before_cfg;
        #[deny(unsafe_code)]
        mod denied;
        #[forbid(missing_docs)]
        mod other_lint;
        #[forbid(missing_docs, unsafe_code)]
        mod two_lints;
        fn body() { let x = { 1 }; }
        impl X { fn y() {} }
        mod last;
    "#;
        let (inner, c) = children("sys/kern/mod.rs", src, true);
        assert!(!inner);
        let want = [
            ("sys/kern/plain.rs", false),
            ("sys/kern/direct.rs", true),
            ("sys/kern/after_cfg.rs", true),
            ("sys/kern/before_cfg.rs", true),
            ("sys/kern/denied.rs", false),
            ("sys/kern/other_lint.rs", false),
            ("sys/kern/two_lints.rs", true),
            ("sys/kern/last.rs", false),
        ];
        let got: Vec<(&str, bool)> = c.iter().map(|(f, b, _)| (f.as_str(), *b)).collect();
        assert_eq!(got, want);
        // `name/mod.rs` is the second candidate.
        let s = mod_scan(Path::new("sys/kern/mod.rs"), &lex("mod sub;"), true);
        assert_eq!(s.children[0].files[1], Path::new("sys/kern/sub/mod.rs"));
    }

    #[test]
    fn inline_modules_pass_forbid_and_their_directory_down() {
        let src = r#"
        #[forbid(unsafe_code)]
        pub mod outer {
            pub mod deep;
            mod inner { mod deeper; }
        }
        mod open {
            #![forbid(unsafe_code)]
            mod a;
        }
        mod plain { mod b; }
        #[cfg(test)]
        mod tests {
            use super::*;
            #[test]
            fn t() { mod not_a_child; }
        }
    "#;
        let (_, c) = children("sys/dev/usb/uhub.rs", src, false);
        let got: Vec<(&str, bool)> = c.iter().map(|(f, b, _)| (f.as_str(), *b)).collect();
        assert_eq!(
            got,
            [
                ("sys/dev/usb/uhub/outer/deep.rs", true),
                ("sys/dev/usb/uhub/outer/inner/deeper.rs", true),
                ("sys/dev/usb/uhub/open/a.rs", true),
                ("sys/dev/usb/uhub/plain/b.rs", false),
            ]
        );
    }

    #[test]
    fn inner_forbid_and_path_attributes() {
        let (inner, _) = children(
            "sys/x.rs",
            "#![forbid(unsafe_code)]\n#![no_std]\nfn f() {}",
            false,
        );
        assert!(inner);
        let (inner, _) = children("sys/x.rs", "#![no_std]\n#![forbid(unsafe_code)]", false);
        assert!(inner);
        let src = r#"
        #[path = "alloc.rs"]
        mod sa_alloc;
        #[cfg(test)]
        #[path = "../amd64"]
        mod amd64_bios {
            pub mod include { pub mod biosvar; }
        }
    "#;
        let (_, c) = children("sys/arch/host/mod.rs", src, true);
        assert_eq!(
            c,
            [
                ("sys/arch/host/alloc.rs".to_string(), false, true),
                (
                    "sys/arch/amd64/include/biosvar.rs".to_string(),
                    false,
                    false
                ),
            ]
        );
    }

    #[test]
    fn forbid_is_inherited_down_the_tree() {
        let files = [
            (
                "/w/sys/lib.rs",
                "pub mod kern;\n#[forbid(unsafe_code)]\npub mod crypto;",
            ),
            (
                "/w/sys/kern/mod.rs",
                "pub mod a;\n#[forbid(unsafe_code)] pub mod b;",
            ),
            ("/w/sys/kern/a.rs", "mod nested;"),
            ("/w/sys/kern/a/nested.rs", ""),
            ("/w/sys/kern/b.rs", "mod child;"),
            ("/w/sys/kern/b/child.rs", ""),
            ("/w/sys/crypto/mod.rs", "pub mod sha2;"),
            ("/w/sys/crypto/sha2.rs", ""),
            ("/w/sys/orphan.rs", ""),
            (
                "/w/sys/lib/libz/lib.rs",
                "#![forbid(unsafe_code)]\nmod zutil;",
            ),
            ("/w/sys/lib/libz/zutil.rs", ""),
        ];
        let lexed: BTreeMap<PathBuf, Vec<Tok>> = files
            .iter()
            .map(|(p, s)| (PathBuf::from(p), lex(s)))
            .collect();
        let got: Vec<String> = forbid_files(&lexed)
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            got,
            [
                "/w/sys/crypto/mod.rs",
                "/w/sys/crypto/sha2.rs",
                // PathBuf orders by components: `b/child.rs` before `b.rs`.
                "/w/sys/kern/b/child.rs",
                "/w/sys/kern/b.rs",
                "/w/sys/lib/libz/lib.rs",
                "/w/sys/lib/libz/zutil.rs",
            ]
        );
    }

    #[test]
    fn core_patterns() {
        let core =
            CoreList::parse("[core]\npaths = [\"sys/arch/amd64/**\", \"sys/sys/queue.rs\"]\n")
                .unwrap();
        assert!(core.contains("sys/arch/amd64/amd64/pmap.rs"));
        assert!(core.contains("sys/sys/queue.rs"));
        assert!(!core.contains("sys/arch/amd64x/a.rs"));
        assert!(!core.contains("sys/arch/amd64"));
        assert!(!core.contains("sys/sys/queue.rs.bak"));
        assert!(!core.contains("sys/sys/tree.rs"));
        for bad in [
            "sys/kern/*.rs",
            "sys/kern",
            "kern/a.rs",
            "sys/**/x.rs",
            "sys/a?.rs",
        ] {
            let text = format!("[core]\npaths = [\"{bad}\"]\n");
            assert!(CoreList::parse(&text).is_err(), "{bad}");
        }
        assert!(CoreList::parse("[core]\n").is_err());
    }

    #[test]
    fn ratchet_moves_one_way() {
        let r = Ratchet {
            forbid_floor: 400,
            legacy_ceiling: 8000,
        };
        assert!(ratchet_errors(&r, 400, 8000).is_empty());
        assert!(ratchet_errors(&r, 401, 7999).is_empty());
        assert_eq!(ratchet_errors(&r, 399, 8000).len(), 1);
        assert_eq!(ratchet_errors(&r, 400, 8001).len(), 1);
        assert_eq!(ratchet_errors(&r, 0, 9000).len(), 2);
        // --write raises the floor and lowers the ceiling, never the other way.
        assert_eq!(
            ratchet_next(Some(r), 420, 7900),
            Ratchet {
                forbid_floor: 420,
                legacy_ceiling: 7900
            }
        );
        assert_eq!(ratchet_next(Some(r), 10, 9999), r);
        assert_eq!(
            ratchet_next(None, 7, 70),
            Ratchet {
                forbid_floor: 7,
                legacy_ceiling: 70
            }
        );
    }

    #[test]
    fn ratchet_is_read_from_the_budget() {
        let b: Budget = toml::from_str(
            "[ratchet]\nforbid_floor = 3\nlegacy_ceiling = 9\n\n[[subsystem]]\nname = \"kern\"\ntotal = 5\n",
        )
        .unwrap();
        assert_eq!(
            b.ratchet,
            Some(Ratchet {
                forbid_floor: 3,
                legacy_ceiling: 9
            })
        );
        assert_eq!(b.subsystems.len(), 1);
        let b: Budget = toml::from_str("[[subsystem]]\nname = \"kern\"\ntotal = 5\n").unwrap();
        assert_eq!(b.ratchet, None);
    }

    #[test]
    fn shapes_are_counted_outside_tests() {
        let src = r#"
        static A: StaticCell<u32> = StaticCell::new(0);
        struct S { a: Cell<*const u8>, b: Cell<*mut u8>, c: Cell<Option<NonNull<u8>>>, d: UnsafeCell<u8> }
        unsafe impl Send for S {}
        unsafe impl<T: Sync> core::marker::Sync for W<T> {}
        unsafe impl Adapter for X {}
        #[cfg(test)]
        mod tests { static B: StaticCell<u8> = StaticCell::new(0); unsafe impl Send for T {} }
    "#;
        let s = scan(&lex(src), false).shapes;
        assert_eq!(
            s,
            Shapes {
                static_cell: 2,
                send_sync: 2,
                raw_cell: 2,
                unsafe_cell: 1
            }
        );
    }

    #[test]
    fn totals_line_names_the_groups() {
        let k = Counts {
            files: 1,
            blocks: 5,
            fns: 4,
            impls: 3,
            traits: 2,
            other: 1,
        };
        let g = Groups {
            core: 10,
            legacy: 5,
            forbid: 400,
            modules: 1165,
            ..Groups::default()
        };
        assert_eq!(
            totals_line(&k, &Counts::default(), &g),
            "Unsafe (`cargo xtask unsafe-report`): kernel 5 blocks, 4 fn, 3 impl, 2 trait, 1 other; tests 0 more; core 10, legacy 5, forbid 400 of 1165 modules."
        );
    }
}
/* </TESTS> */
