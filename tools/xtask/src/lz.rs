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
//! `cargo xtask lz {check,status,drift,trace}`: the lineage of this tree against EmiBSD.LZ.
//!
//! EmiBSD derives from EmiBSD.LZ, the faithful port of OpenBSD. `lineage.toml` records, per
//! native module, the LZ files it derives from (file level), its status (`inherited`: byte for
//! byte LZ's; `adapted`: only its call sites changed, because a module it uses was redesigned;
//! `redesigned`) and, only as exceptions, the items a redesign split, renamed, moved, merged or
//! dropped (`[[module.fn]]`, function level).
//! `lz/PINNED.md` names the LZ commit the tree is synced to; `reference/emibsd-lz/` is a
//! read-only full-history clone of LZ that every subcommand reads with `git`.
//!
//! ```text
//! cargo xtask lz check                     validate lineage.toml against the tree and the pin
//! cargo xtask lz status [--write]          inherited and redesigned modules per subsystem;
//!                                          --write regenerates the tables of README.md and
//!                                          docs/STATUS.md between the lz markers
//! cargo xtask lz drift [--fetch] [--security] [--functions] [--strict]
//!                                          the LZ commits after the pin without a record in
//!                                          lz-sync.toml, with the native modules they touch;
//!                                          --functions maps the items their diffs touch
//! cargo xtask lz trace <lz rs path>:<item>  where that LZ item's code is now
//! cargo xtask lz trace --rust <path>:<item> where this native item came from
//! cargo xtask lz trace --c <c path>:<item>  the C item, through the LZ file's Upstream: line
//! ```
//!
//! Rules: `.claude/rules/lineage.md`, `lz-sync.md`. The item lexer is line based, in the
//! house style of `unsafereport.rs`: no `syn`, no ctags.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::layout::{drop_empty_licenses_zone, strip_author_block, trim_licenses_edges};
use crate::{REFERENCE_DIR, Result, git, is_structural, short, walk_rs};

const LINEAGE_FILE: &str = "lineage.toml";
const SYNC_FILE: &str = "lz-sync.toml";
const LZ_PINNED: &str = "lz/PINNED.md";
const LZ_DIR: &str = "reference/emibsd-lz";
const REFERENCE_PINNED: &str = "reference/PINNED.md";
const TABLE_BEGIN: &str = "<!-- lz:begin -->";
const TABLE_END: &str = "<!-- lz:end -->";
const TABLE_DOCS: [&str; 2] = ["README.md", "docs/STATUS.md"];
const SECURITY_WORDS: [&str; 6] = [
    "security",
    "errata",
    "cve",
    "overflow",
    "use-after-free",
    "reference: bump",
];

#[derive(Deserialize)]
struct Lineage {
    meta: Meta,
    #[serde(default, rename = "module")]
    modules: Vec<Module>,
    #[serde(default, rename = "extra")]
    extras: Vec<Extra>,
    #[serde(default, rename = "dropped")]
    dropped: Vec<Dropped>,
}

#[derive(Deserialize)]
struct Meta {
    lz: String,
}

#[derive(Deserialize)]
struct Module {
    rust: String,
    #[serde(default)]
    lz: Vec<String>,
    status: Status,
    /// A licence family that is not ISC/BSD/MIT, or `"none"`: no `<LICENSES>` zone.
    #[serde(default)]
    license: String,
    /// `adapted` only: the redesigned modules whose API forced this module's call sites to change.
    #[serde(default)]
    adapted_by: Vec<String>,
    /// Free text: LZ's `wip` notes carried over at N0 (what the port left partial), or later notes.
    #[serde(default)]
    notes: String,
    #[serde(default, rename = "fn")]
    fns: Vec<FnRow>,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
enum Status {
    Inherited,
    Adapted,
    Redesigned,
}

/// One exception of the by-name default: an LZ item that no longer has a same-named item.
#[derive(Deserialize)]
struct FnRow {
    lz: String,
    #[serde(default)]
    native: Vec<String>,
    kind: Kind,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    lines: String,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Split,
    Renamed,
    Moved,
    Merged,
    Dropped,
}

/// A native file under `sys/` with no LZ source (project helpers).
#[derive(Deserialize)]
struct Extra {
    rust: String,
    reason: String,
}

/// An LZ file nothing derives from any more.
#[derive(Deserialize)]
struct Dropped {
    lz: String,
    reason: String,
}

#[derive(Deserialize)]
struct Sync {
    meta: Meta,
    #[serde(default, rename = "commit")]
    commits: Vec<Record>,
}

#[derive(Deserialize)]
struct Record {
    lz: String,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    modules: Vec<String>,
    status: SyncStatus,
    #[serde(default)]
    emibsd: String,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    security: bool,
    /// `applied` only: `cherry-pick`, `cherry-pick-conflicts` or `reimplemented`.
    #[serde(default)]
    method: String,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "kebab-case")]
enum SyncStatus {
    Applied,
    NotApplicable,
    CoveredByRedesign,
}

/// An item a Rust file defines: its qualified name (`fork1`, `Proc::fork_thread`,
/// `memmap_type::USABLE`) and its line span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Item {
    pub(crate) name: String,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// The LZ clone and the pinned commit, checked once.
struct Lz {
    repo: PathBuf,
    pin: String,
}

fn load_lineage(root: &Path) -> Result<Lineage> {
    let path = root.join(LINEAGE_FILE);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()).into())
}

fn load_sync(root: &Path) -> Result<Sync> {
    let path = root.join(SYNC_FILE);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()).into())
}

/// The `Commit:` line of a PINNED.md.
fn commit_line(root: &Path, rel: &str) -> Result<String> {
    let path = root.join(rel);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines()
        .find_map(|l| l.strip_prefix("Commit:"))
        .map(|s| s.trim().to_string())
        .ok_or_else(|| format!("{}: no `Commit:` line", path.display()).into())
}

fn open_lz(root: &Path) -> Result<Lz> {
    let repo = root.join(LZ_DIR);
    if !repo.join(".git").exists() {
        return Err(format!(
            "{LZ_DIR} not present: git clone the EmiBSD.LZ repository there (lz/PINNED.md names it)"
        )
        .into());
    }
    let pin = commit_line(root, LZ_PINNED)?;
    if pin.len() != 40 || !pin.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(
            format!("{LZ_PINNED}: `Commit:` must be a full 40-hex commit, not `{pin}`").into(),
        );
    }
    git(&repo, &["cat-file", "-e", &format!("{pin}^{{commit}}")]).map_err(|e| {
        format!(
            "{LZ_PINNED}: commit {} is not in {LZ_DIR} ({e})",
            short(&pin)
        )
    })?;
    Ok(Lz { repo, pin })
}

impl Lz {
    /// The content of `path` at the pin.
    fn show(&self, path: &str) -> Result<String> {
        git_raw(&self.repo, &["show", &format!("{}:{path}", self.pin)])
    }

    /// Every non-structural `.rs` under `sys/` at the pin.
    fn rs_files(&self) -> Result<Vec<String>> {
        let out = git(
            &self.repo,
            &["ls-tree", "-r", "--name-only", &self.pin, "--", "sys"],
        )?;
        Ok(out
            .lines()
            .filter(|l| l.ends_with(".rs") && !is_structural(l))
            .map(str::to_string)
            .collect())
    }
}

/// `git` without trimming, for file contents.
fn git_raw(repo: &Path, args: &[&str]) -> Result<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        let cmd = args.join(" ");
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("git {cmd}: {stderr}").into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `src` without the RCS ident lines (`/* $OpenBSD: ... $ */`, `$NetBSD`, `$FreeBSD`): CVS
/// keyword expansions, not licence text, dropped from this tree and kept in LZ (decision 20).
///
/// Dropping an ident line at the top of a file, or one between two blank lines, leaves a
/// leading blank line or two blank lines in a row, which rustfmt removes; the commit that
/// dropped the lines (`6b5304c`) removed them too. So the blank line that follows a dropped
/// ident goes with it when the text kept so far is empty or ends in a blank line. No other
/// blank line is touched.
pub(crate) fn strip_ident_lines(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut drop_next_blank = false;
    for line in src.split_inclusive('\n') {
        let t = line.trim();
        let ident = t.starts_with("/*")
            && t.ends_with("*/")
            && ["$OpenBSD:", "$NetBSD:", "$FreeBSD:"]
                .iter()
                .any(|k| t.contains(k));
        if ident {
            drop_next_blank |= out.is_empty() || out.ends_with("\n\n");
            continue;
        }
        if drop_next_blank && t.is_empty() {
            drop_next_blank = false;
            continue;
        }
        drop_next_blank = false;
        out.push_str(line);
    }
    out
}

/// A module's text as `lz check` compares it with its LZ source: without the RCS ident lines
/// (decision 20) and the author's block (the authorship rule), and with the blank lines that start or end the
/// LICENSES zone dropped, and an empty LICENSES zone dropped. Applied to both sides.
fn comparable(src: &str) -> String {
    drop_empty_licenses_zone(&trim_licenses_edges(&strip_author_block(
        &strip_ident_lines(src),
    )))
}

fn rel_of(root: &Path, f: &Path) -> String {
    f.strip_prefix(root)
        .unwrap_or(f)
        .to_string_lossy()
        .replace('\\', "/")
}

fn is_hex12(s: &str) -> bool {
    s.len() == 12 && s.chars().all(|c| c.is_ascii_hexdigit())
}

// --- the item lexer ---------------------------------------------------------------------

/// `line` with comments and string/char literals blanked (their delimiters kept), tracking a
/// block comment across lines.
fn blank_line(line: &str, in_block: &mut bool) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if *in_block {
            if c == '*' && next == Some('/') {
                *in_block = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        match c {
            '/' if next == Some('/') => break,
            '/' if next == Some('*') => {
                *in_block = true;
                i += 2;
            }
            '"' => {
                out.push('"');
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    i += if chars[i] == '\\' { 2 } else { 1 };
                }
                out.push('"');
                i += 1;
            }
            '\'' => {
                // A char literal (`'x'`, `'\n'`) or a lifetime (`'a`).
                let lit_len = match (chars.get(i + 1), chars.get(i + 2), chars.get(i + 3)) {
                    (Some('\\'), _, Some('\'')) => 4,
                    (Some(ch), Some('\''), _) if *ch != '\\' => 3,
                    _ => 0,
                };
                if lit_len > 0 {
                    out.push('\'');
                    i += lit_len;
                } else {
                    out.push('\'');
                    i += 1;
                }
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

fn ident_prefix(s: &str) -> &str {
    let n = s
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_'))
        .map_or(s.len(), |(i, _)| i);
    &s[..n]
}

/// The name declared by a line such as `pub unsafe fn name(` or `pub(crate) struct Name<T>`.
fn decl_name(line: &str) -> Option<String> {
    let mut words = line.split_whitespace().peekable();
    loop {
        let w = *words.peek()?;
        if w == "pub" || w.starts_with("pub(") || matches!(w, "unsafe" | "async" | "default") {
            words.next();
        } else if w == "extern" {
            words.next();
            if words.peek().is_some_and(|n| n.starts_with('"')) {
                words.next();
            }
        } else {
            break;
        }
    }
    let kw = words.next()?;
    let name = match kw {
        "fn" | "struct" | "enum" | "union" | "trait" | "type" | "macro_rules!" => words.next()?,
        "const" => match words.next()? {
            "fn" => words.next()?,
            n => n,
        },
        "static" => match words.next()? {
            "mut" => words.next()?,
            n => n,
        },
        _ => return None,
    };
    let name = ident_prefix(name);
    (!name.is_empty()).then(|| name.to_string())
}

/// The type an `impl` line implements for: `impl<T> fmt::Display for Foo<T> {` → `Foo`.
fn impl_type(line: &str) -> Option<String> {
    let line = line.trim_start();
    let rest = line.strip_prefix("unsafe ").unwrap_or(line);
    let rest = rest.strip_prefix("impl")?;
    if !(rest.starts_with('<') || rest.starts_with(char::is_whitespace)) {
        return None;
    }
    let rest = &rest[skip_generics(rest).len()..];
    let rest = rest.split(" where ").next().unwrap_or(rest);
    let rest = rest.trim().trim_end_matches(['{', '}', ' ']);
    let target = match rest.find(" for ") {
        Some(i) => &rest[i + 5..],
        None => rest,
    };
    let target = target.trim_start_matches(['&', ' ']);
    let target = target.strip_prefix("mut ").unwrap_or(target);
    let target = if target.starts_with('\'') {
        target.split_once(' ').map_or("", |(_, t)| t)
    } else {
        target
    };
    let head = target.trim_start().split('<').next().unwrap_or(target);
    let last = head.rsplit("::").next().unwrap_or(head);
    let name = ident_prefix(last.trim());
    (!name.is_empty()).then(|| name.to_string())
}

/// The `<...>` right after `impl`, balanced, so `impl<T: Into<U>> Foo` finds `Foo`.
fn skip_generics(s: &str) -> &str {
    if !s.starts_with('<') {
        return "";
    }
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return &s[..=i];
                }
            }
            _ => {}
        }
    }
    s
}

fn mod_name(line: &str) -> Option<String> {
    let rest = line.trim_start_matches("pub ").trim_start();
    let rest = if rest.starts_with("pub(") {
        rest.split_once(' ')?.1
    } else {
        rest
    };
    let rest = rest.strip_prefix("mod ")?;
    if rest.trim_end().ends_with(';') {
        return None;
    }
    let name = ident_prefix(rest.trim_start());
    (!name.is_empty()).then(|| name.to_string())
}

/// The items `src` defines, with their line spans: free items at the top level, the methods
/// of `impl` blocks as `Type::method`, the items of an inline `mod m` as `m::item`; the
/// contents of `mod tests` are skipped.
pub(crate) fn items(src: &str) -> Vec<Item> {
    struct Scope {
        depth: usize,
        prefix: String,
        skip: bool,
    }
    let mut out: Vec<Item> = Vec::new();
    let mut open: Vec<(usize, usize)> = Vec::new();
    let mut scopes: Vec<Scope> = Vec::new();
    let mut pending: Option<Scope> = None;
    let mut depth = 0usize;
    let mut in_block = false;
    for (i, raw) in src.lines().enumerate() {
        let lineno = i + 1;
        let code = blank_line(raw, &mut in_block);
        let trimmed = code.trim();
        let skipping = scopes.last().is_some_and(|s| s.skip);
        let at_scope = scopes.last().map_or(depth == 0, |s| depth == s.depth + 1);
        if !skipping && at_scope && !trimmed.is_empty() && !trimmed.starts_with('#') {
            if let Some(ty) = impl_type(trimmed) {
                pending = Some(Scope {
                    depth,
                    prefix: ty,
                    skip: false,
                });
            } else if let Some(m) = mod_name(trimmed) {
                let skip = m == "tests";
                pending = Some(Scope {
                    depth,
                    prefix: if skip { String::new() } else { m },
                    skip,
                });
            } else if let Some(name) = decl_name(trimmed) {
                let prefix: Vec<&str> = scopes
                    .iter()
                    .map(|s| s.prefix.as_str())
                    .filter(|p| !p.is_empty())
                    .collect();
                let name = if prefix.is_empty() {
                    name
                } else {
                    format!("{}::{name}", prefix.join("::"))
                };
                out.push(Item {
                    name,
                    start: lineno,
                    end: lineno,
                });
                open.push((out.len() - 1, depth));
            }
        }
        for ch in code.chars() {
            match ch {
                '{' => {
                    if let Some(s) = pending.take() {
                        scopes.push(s);
                    }
                    depth += 1;
                }
                '}' => {
                    depth = depth.saturating_sub(1);
                    if scopes.last().is_some_and(|s| s.depth == depth) {
                        scopes.pop();
                    }
                    while let Some(&(idx, d)) = open.last() {
                        if d == depth {
                            out[idx].end = lineno;
                            open.pop();
                        } else {
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(&(idx, d)) = open.last()
            && d == depth
            && trimmed.ends_with(';')
        {
            out[idx].end = lineno;
            open.pop();
        }
    }
    out
}

// --- lz check ---------------------------------------------------------------------------

/// `cargo xtask lz check`.
pub(crate) fn check(root: &Path) -> Result<()> {
    use crate::layout::Licenses;

    let lineage = load_lineage(root)?;
    let lz = open_lz(root)?;
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    if lineage.meta.lz != lz.pin {
        errors.push(format!(
            "[meta].lz ({}) != {LZ_PINNED} Commit: ({})",
            short(&lineage.meta.lz),
            short(&lz.pin)
        ));
    }
    match load_sync(root) {
        Ok(sync) if sync.meta.lz != lz.pin => errors.push(format!(
            "{SYNC_FILE} [meta].lz ({}) != {LZ_PINNED} Commit: ({})",
            short(&sync.meta.lz),
            short(&lz.pin)
        )),
        Ok(_) => {}
        Err(e) => errors.push(e.to_string()),
    }

    // The OpenBSD pins agree: LZ's at the pinned commit, this tree's, the clone's HEAD.
    let lz_openbsd = lz
        .show(REFERENCE_PINNED)?
        .lines()
        .find_map(|l| l.strip_prefix("Commit:"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let openbsd = commit_line(root, REFERENCE_PINNED)?;
    if lz_openbsd != openbsd {
        errors.push(format!(
            "{REFERENCE_PINNED} ({}) differs from LZ's at the pin ({})",
            short(&openbsd),
            short(&lz_openbsd)
        ));
    }
    let reference = root.join(REFERENCE_DIR);
    if reference.join("sys").is_dir() {
        if let Ok(head) = git(&reference, &["rev-parse", "HEAD"])
            && head != openbsd
        {
            errors.push(format!(
                "{REFERENCE_DIR} is at {} but {REFERENCE_PINNED} says {}",
                short(&head),
                short(&openbsd)
            ));
        }
    } else {
        warnings.push(format!(
            "{REFERENCE_DIR} not present; the OpenBSD clone was not checked"
        ));
    }

    // Native coverage: every non-structural .rs under sys/ is one module or one extra.
    let mut owners: HashMap<&str, usize> = HashMap::new();
    for m in &lineage.modules {
        *owners.entry(m.rust.as_str()).or_insert(0) += 1;
    }
    for x in &lineage.extras {
        *owners.entry(x.rust.as_str()).or_insert(0) += 1;
    }
    for (rust, n) in &owners {
        if *n > 1 {
            errors.push(format!("{rust}: named by {n} entries of {LINEAGE_FILE}"));
        }
    }
    let mut rs_files = Vec::new();
    walk_rs(&root.join("sys"), &mut rs_files)?;
    for f in &rs_files {
        let rel = rel_of(root, f);
        if is_structural(&rel) || owners.contains_key(rel.as_str()) {
            continue;
        }
        errors.push(format!(
            "{rel}: not in {LINEAGE_FILE} (add a [[module]] with its lz list, or an [[extra]] with a reason)"
        ));
    }
    for x in &lineage.extras {
        let rust = &x.rust;
        if !root.join(rust).is_file() {
            errors.push(format!("[[extra]] rust = \"{rust}\": file does not exist"));
        }
        if x.reason.trim().is_empty() {
            errors.push(format!("[[extra]] rust = \"{rust}\": `reason` is required"));
        }
    }

    // LZ coverage: every LZ file at the pin is derived from or dropped.
    let lz_files: HashSet<String> = lz.rs_files()?.into_iter().collect();
    let mut derived: HashSet<&str> = HashSet::new();
    for m in &lineage.modules {
        for l in &m.lz {
            derived.insert(l.as_str());
        }
    }
    // A project helper of the same path in LZ is derived by the native [[extra]] (N0 carried
    // them over); a dropped LZ file needs its reason.
    for x in &lineage.extras {
        derived.insert(x.rust.as_str());
    }
    let dropped: HashSet<&str> = lineage.dropped.iter().map(|d| d.lz.as_str()).collect();
    for d in &lineage.dropped {
        if d.reason.trim().is_empty() {
            errors.push(format!(
                "[[dropped]] lz = \"{}\": `reason` is required",
                d.lz
            ));
        }
        if derived.contains(d.lz.as_str()) {
            errors.push(format!(
                "[[dropped]] lz = \"{}\": still in a module's lz list",
                d.lz
            ));
        }
        if !lz_files.contains(d.lz.as_str()) {
            errors.push(format!(
                "[[dropped]] lz = \"{}\": not an LZ file at the pin",
                d.lz
            ));
        }
    }
    let mut missing: Vec<&String> = lz_files
        .iter()
        .filter(|f| !derived.contains(f.as_str()) && !dropped.contains(f.as_str()))
        .collect();
    missing.sort();
    for f in missing {
        errors.push(format!(
            "{f}: an LZ file at the pin that no module derives from (list it in a module's lz, or add a [[dropped]] with a reason)"
        ));
    }

    // Per module: files, blobs, headers, rows. LZ item lists are shared across modules.
    let mut lz_items: HashMap<String, Vec<Item>> = HashMap::new();
    let mut lz_sources: HashMap<String, String> = HashMap::new();
    let mut native_items: HashMap<String, Vec<Item>> = HashMap::new();
    let mut by_lz: HashMap<&str, Vec<&Module>> = HashMap::new();
    for m in &lineage.modules {
        for l in &m.lz {
            by_lz.entry(l.as_str()).or_default().push(m);
        }
    }
    let status_of: HashMap<&str, Status> = lineage
        .modules
        .iter()
        .map(|m| (m.rust.as_str(), m.status))
        .collect();
    let mut n_inherited = 0usize;
    let mut n_adapted = 0usize;
    let mut n_rows = 0usize;
    for m in &lineage.modules {
        let rust = &m.rust;
        let tag = format!("[[module]] rust = \"{rust}\"");
        if !rust.starts_with("sys/") || !rust.ends_with(".rs") {
            errors.push(format!("{tag}: `rust` must be a .rs path under sys/"));
            continue;
        }
        let Ok(src) = fs::read_to_string(root.join(rust)) else {
            errors.push(format!("{tag}: file does not exist"));
            continue;
        };
        if m.lz.is_empty() {
            errors.push(format!(
                "{tag}: `lz` must name at least one LZ file (or make it an [[extra]])"
            ));
            continue;
        }
        for l in &m.lz {
            if !lz_files.contains(l.as_str()) && !is_structural(l) {
                errors.push(format!(
                    "{tag}: lz = \"{l}\" is not a file of LZ at the pin"
                ));
            }
        }
        match m.status {
            Status::Inherited => {
                n_inherited += 1;
                if m.lz.len() != 1 || m.lz[0] != *rust {
                    errors.push(format!(
                        "{tag}: inherited modules derive from the LZ file of the same path only"
                    ));
                    continue;
                }
                if !m.fns.is_empty() {
                    errors.push(format!(
                        "{tag}: inherited modules have no [[module.fn]] rows"
                    ));
                }
                let theirs = lz.show(rust).unwrap_or_default();
                if comparable(&src) != comparable(&theirs) {
                    errors.push(format!(
                        "{tag}: differs from LZ at the pin; set status = \"redesigned\" in the commit that changes it"
                    ));
                }
            }
            Status::Adapted => {
                n_adapted += 1;
                if m.adapted_by.is_empty() {
                    errors.push(format!(
                        "{tag}: adapted modules name the redesigned modules in `adapted_by`"
                    ));
                }
                for a in &m.adapted_by {
                    if status_of.get(a.as_str()) != Some(&Status::Redesigned) {
                        errors.push(format!(
                            "{tag}: adapted_by = \"{a}\" is not a redesigned module"
                        ));
                    }
                }
                if !m.fns.is_empty() {
                    errors.push(format!("{tag}: adapted modules have no [[module.fn]] rows (nothing of their own changed)"));
                }
                if m.lz.len() != 1 || m.lz[0] != *rust {
                    errors.push(format!(
                        "{tag}: adapted modules derive from the LZ file of the same path only"
                    ));
                } else {
                    let theirs = lz.show(rust).unwrap_or_default();
                    if comparable(&src) == comparable(&theirs) {
                        errors.push(format!(
                            "{tag}: identical to LZ at the pin; an unchanged module is `inherited`"
                        ));
                    }
                }
            }
            Status::Redesigned => {
                for l in &m.lz {
                    let want = format!("//! LZ: {l}@");
                    let ok = src
                        .lines()
                        .any(|line| line.strip_prefix(&want).is_some_and(|h| is_hex12(h.trim())));
                    if !ok {
                        errors.push(format!("{tag}: lacks a `//! LZ: {l}@<12-hex>` line"));
                    }
                    if let Ok(theirs) = lz.show(l) {
                        for up in theirs.lines().filter(|x| x.starts_with("//! Upstream:")) {
                            if !src.lines().any(|x| x == up) {
                                errors.push(format!("{tag}: lacks the line `{up}` of {l}"));
                            }
                        }
                    }
                }
                if !src.lines().any(|x| x.starts_with("//! ## Redesign")) {
                    errors.push(format!("{tag}: lacks a `//! ## Redesign` section"));
                }
            }
        }
        for row in &m.fns {
            n_rows += 1;
            let rtag = format!("{tag} [[module.fn]] lz = \"{}\"", row.lz);
            let Some((file, item)) = row.lz.rsplit_once("::") else {
                errors.push(format!("{rtag}: `lz` must be `<LZ rs path>::<item>`"));
                continue;
            };
            let file = file.to_string();
            if !m.lz.contains(&file) {
                errors.push(format!("{rtag}: {file} is not in this module's lz list"));
                continue;
            }
            let theirs = match lz_items.get(&file) {
                Some(v) => v,
                None => {
                    let text = lz.show(&file).unwrap_or_default();
                    lz_items.insert(file.clone(), items(&text));
                    lz_sources.insert(file.clone(), text);
                    &lz_items[&file]
                }
            };
            if !theirs.iter().any(|it| it.name == item) {
                errors.push(format!(
                    "{rtag}: {file} defines no item `{item}` at the pin"
                ));
            }
            if row.kind == Kind::Dropped {
                if !row.native.is_empty() {
                    errors.push(format!("{rtag}: a dropped row has no `native` targets"));
                }
                if row.reason.trim().is_empty() {
                    errors.push(format!("{rtag}: a dropped row needs a `reason`"));
                }
            } else if row.native.is_empty() {
                errors.push(format!("{rtag}: `native` must name at least one target"));
            }
            if !row.lines.is_empty() && row.kind != Kind::Split {
                errors.push(format!("{rtag}: `lines` is allowed on split rows only"));
            }
            for target in &row.native {
                let (tfile, tname) = match target.strip_prefix("sys/") {
                    Some(_) => match target.rsplit_once("::") {
                        Some((f, n)) if f.ends_with(".rs") => (f.to_string(), n.to_string()),
                        _ => {
                            errors.push(format!("{rtag}: target `{target}` must be `name`, `Type::method` or `sys/<path>.rs::name`"));
                            continue;
                        }
                    },
                    None => (rust.clone(), target.clone()),
                };
                if !native_items.contains_key(&tfile) {
                    let text = fs::read_to_string(root.join(&tfile)).unwrap_or_default();
                    native_items.insert(tfile.clone(), items(&text));
                }
                if !native_items[&tfile].iter().any(|it| it.name == tname) {
                    errors.push(format!("{rtag}: target `{target}` not found in {tfile}"));
                }
            }
        }
    }

    // Function-level coverage: every item of every LZ file a redesigned module derives from is
    // found by name in a module that lists the file, or has a row.
    let mut checked_lz_files: HashSet<&str> = HashSet::new();
    for m in lineage
        .modules
        .iter()
        .filter(|m| m.status == Status::Redesigned)
    {
        for l in &m.lz {
            if !checked_lz_files.insert(l.as_str()) {
                continue;
            }
            let theirs = match lz_items.get(l.as_str()) {
                Some(v) => v.clone(),
                None => {
                    let text = lz.show(l).unwrap_or_default();
                    let v = items(&text);
                    lz_items.insert(l.clone(), v.clone());
                    v
                }
            };
            let listing = &by_lz[l.as_str()];
            let mut names: HashSet<String> = HashSet::new();
            for lm in listing {
                if !native_items.contains_key(&lm.rust) {
                    let text = fs::read_to_string(root.join(&lm.rust)).unwrap_or_default();
                    native_items.insert(lm.rust.clone(), items(&text));
                }
                names.extend(native_items[&lm.rust].iter().map(|it| it.name.clone()));
            }
            let rows: HashSet<String> = listing
                .iter()
                .flat_map(|lm| lm.fns.iter().map(|r| r.lz.clone()))
                .collect();
            for it in &theirs {
                let key = format!("{l}::{}", it.name);
                if names.contains(&it.name) || rows.contains(&key) {
                    continue;
                }
                errors.push(format!(
                    "{l}::{}: no same-named item in {} and no [[module.fn]] row (split, renamed, moved, merged or dropped?)",
                    it.name,
                    listing.iter().map(|lm| lm.rust.as_str()).collect::<Vec<_>>().join(", ")
                ));
            }
        }
    }

    // Zone markers, with the licence policy of lineage.toml.
    let mut policy: HashMap<&str, Licenses> = HashMap::new();
    for m in &lineage.modules {
        let lic = if m.license == "none" {
            Licenses::AuthorOnly
        } else {
            Licenses::Port
        };
        policy.insert(m.rust.as_str(), lic);
    }
    let mut n_author = 0usize;
    let mut files = Vec::new();
    for tree in ["sys", "tools"] {
        walk_rs(&root.join(tree), &mut files)?;
    }
    files.sort();
    for f in &files {
        let rel = rel_of(root, f);
        if rel.ends_with("/tests.rs") {
            errors.push(format!(
                "{rel}: tests live inline in the file of the module they test (`mod tests` in <TESTS>)"
            ));
            continue;
        }
        let lic = policy
            .get(rel.as_str())
            .copied()
            .unwrap_or(Licenses::AuthorOnly);
        let src = fs::read_to_string(f).map_err(|e| format!("{rel}: {e}"))?;
        errors.extend(crate::layout::check(&rel, &src, lic));
        let author = crate::layout::check_author(&rel, &src, lic, true);
        if author.is_empty() && src.contains(crate::layout::AUTHOR_BLOCK) {
            n_author += 1;
        }
        errors.extend(author);
    }

    for w in &warnings {
        println!("warning: {w}");
    }
    for e in &errors {
        println!("error: {e}");
    }
    let nfiles = files.len();
    let (nmod, nx, nd, nerr, nwarn) = (
        lineage.modules.len(),
        lineage.extras.len(),
        lineage.dropped.len(),
        errors.len(),
        warnings.len(),
    );
    let nred = nmod - n_inherited - n_adapted;
    println!(
        "lz check: {nmod} modules ({n_inherited} inherited, {n_adapted} adapted, {nred} redesigned), {n_rows} fn rows, {nx} extras, {nd} dropped; author's block in {n_author} of {nfiles} .rs; pin {}: {nerr} error(s), {nwarn} warning(s)",
        short(&lz.pin)
    );
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("{nerr} error(s) in {LINEAGE_FILE}").into())
    }
}

// --- lz status ---------------------------------------------------------------------------

/// `cargo xtask lz status [--write]`.
pub(crate) fn status(root: &Path, write: bool) -> Result<()> {
    let lineage = load_lineage(root)?;
    let pin = commit_line(root, LZ_PINNED)?;
    let mut table: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    for m in &lineage.modules {
        let row = table
            .entry(crate::unsafereport::subsystem(root, &m.rust))
            .or_default();
        match m.status {
            Status::Inherited => row.0 += 1,
            Status::Adapted => row.1 += 1,
            Status::Redesigned => row.2 += 1,
        }
    }
    let mut md = String::new();
    md.push_str(
        "| Subsystem | inherited | adapted | redesigned | total |
|---|---:|---:|---:|---:|
",
    );
    let (mut ti, mut ta, mut tr) = (0usize, 0usize, 0usize);
    for (sub, (i, a, r)) in &table {
        md.push_str(&format!(
            "| {sub} | {i} | {a} | {r} | {} |
",
            i + a + r
        ));
        ti += i;
        ta += a;
        tr += r;
    }
    md.push_str(&format!(
        "| **total** | {ti} | {ta} | {tr} | {} |
",
        ti + ta + tr
    ));
    let rows: usize = lineage.modules.iter().map(|m| m.fns.len()).sum();
    let head = format!(
        "_Generated by `cargo xtask lz status --write` against EmiBSD.LZ {}: {} modules, {rows} function rows, {} extras, {} dropped._\n\n",
        short(&pin),
        lineage.modules.len(),
        lineage.extras.len(),
        lineage.dropped.len()
    );
    println!("EmiBSD.LZ @ {}", short(&pin));
    print!("{md}");
    if write {
        for doc_rel in TABLE_DOCS {
            let path = root.join(doc_rel);
            let doc = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let (Some(b), Some(e)) = (doc.find(TABLE_BEGIN), doc.find(TABLE_END)) else {
                return Err(
                    format!("{doc_rel}: markers {TABLE_BEGIN} / {TABLE_END} not found").into(),
                );
            };
            if e < b {
                return Err(format!("{doc_rel}: {TABLE_END} appears before {TABLE_BEGIN}").into());
            }
            // The README gets the table; docs/STATUS.md (under 30 lines) only the summary line.
            let body = if doc_rel == "README.md" {
                format!("{head}{md}")
            } else {
                format!("{}\n", head.trim_end())
            };
            let new = format!("{}{TABLE_BEGIN}\n{body}{}", &doc[..b], &doc[e..]);
            fs::write(&path, new).map_err(|e| format!("{}: {e}", path.display()))?;
            println!("wrote {doc_rel}");
        }
    }
    Ok(())
}

// --- lz drift ----------------------------------------------------------------------------

/// What `lz drift` reports about one LZ commit after the pin.
struct Pending {
    hash: String,
    subject: String,
    files: Vec<String>,
    security: bool,
}

/// `cargo xtask lz drift [--fetch] [--security] [--functions] [--strict]`.
pub(crate) fn drift(
    root: &Path,
    fetch: bool,
    security: bool,
    functions: bool,
    strict: bool,
) -> Result<()> {
    let lineage = load_lineage(root)?;
    let sync = load_sync(root)?;
    let lz = open_lz(root)?;
    if fetch {
        git(&lz.repo, &["fetch", "-q", "origin"])?;
    }
    let tip = git(&lz.repo, &["rev-parse", "--verify", "-q", "origin/main"])
        .map_err(|_| format!("{LZ_DIR}: no origin/main; run with --fetch"))?;
    let log = git(
        &lz.repo,
        &[
            "log",
            "--format=%H%x09%s",
            "--reverse",
            &format!("{}..{tip}", lz.pin),
        ],
    )?;
    let mut by_lz: HashMap<&str, Vec<&str>> = HashMap::new();
    for m in &lineage.modules {
        for l in &m.lz {
            by_lz.entry(l.as_str()).or_default().push(m.rust.as_str());
        }
    }
    let dropped: HashSet<&str> = lineage.dropped.iter().map(|d| d.lz.as_str()).collect();
    let known: HashSet<&str> = lineage.modules.iter().map(|m| m.rust.as_str()).collect();
    let mut errors = 0usize;
    for r in &sync.commits {
        if !is_hex12(&r.lz) {
            println!(
                "error: {SYNC_FILE}: lz = \"{}\" ({}) is not a 12-hex commit",
                r.lz, r.subject
            );
            errors += 1;
        }
        for m in &r.modules {
            if !known.contains(m.as_str()) {
                println!(
                    "error: {SYNC_FILE}: {} ({}): module {m} is not in {LINEAGE_FILE}",
                    r.lz, r.subject
                );
                errors += 1;
            }
        }
        match r.status {
            SyncStatus::Applied if r.emibsd.is_empty() => {
                println!("error: {SYNC_FILE}: {}: applied without `emibsd`", r.lz);
                errors += 1;
            }
            SyncStatus::Applied => {
                if git(
                    root,
                    &["cat-file", "-e", &format!("{}^{{commit}}", r.emibsd)],
                )
                .is_err()
                {
                    println!(
                        "error: {SYNC_FILE}: {}: emibsd commit {} not found",
                        r.lz, r.emibsd
                    );
                    errors += 1;
                }
                if !matches!(
                    r.method.as_str(),
                    "cherry-pick" | "cherry-pick-conflicts" | "reimplemented"
                ) {
                    println!(
                        "error: {SYNC_FILE}: {}: applied records need `method` = cherry-pick | cherry-pick-conflicts | reimplemented",
                        r.lz
                    );
                    errors += 1;
                }
            }
            SyncStatus::NotApplicable | SyncStatus::CoveredByRedesign
                if r.reason.trim().is_empty() =>
            {
                println!(
                    "error: {SYNC_FILE}: {}: `reason` is required for {:?}",
                    r.lz, r.status
                );
                errors += 1;
            }
            _ => {}
        }
        if r.security && r.status == SyncStatus::NotApplicable && r.reason.trim().is_empty() {
            println!(
                "error: {SYNC_FILE}: {}: a security fix is never not-applicable without a reason",
                r.lz
            );
            errors += 1;
        }
    }
    let mut pending: Vec<Pending> = Vec::new();
    let mut triaged = 0usize;
    for line in log.lines().filter(|l| !l.is_empty()) {
        let (hash, subject) = line.split_once('\t').unwrap_or((line, ""));
        if sync.commits.iter().any(|r| hash.starts_with(&r.lz)) {
            triaged += 1;
            continue;
        }
        let files: Vec<String> = git(&lz.repo, &["show", "--format=", "--name-only", hash])?
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect();
        let body = git(&lz.repo, &["show", "-s", "--format=%s%n%b", hash])?.to_lowercase();
        let sec = SECURITY_WORDS.iter().any(|w| body.contains(w));
        pending.push(Pending {
            hash: hash.to_string(),
            subject: subject.to_string(),
            files,
            security: sec,
        });
    }
    if security {
        pending.sort_by_key(|p| !p.security);
    }
    for p in &pending {
        let flag = if p.security { " (security)" } else { "" };
        println!("{} {}{flag}", short(&p.hash), p.subject);
        for f in &p.files {
            let where_ = match by_lz.get(f.as_str()) {
                Some(mods) => mods.join(", "),
                None if dropped.contains(f.as_str()) => "(dropped here)".to_string(),
                None if f.starts_with("sys/") && f.ends_with(".rs") => {
                    "(new in LZ: add an inherited module)".to_string()
                }
                None => "(not a module)".to_string(),
            };
            println!("    {f} -> {where_}");
            if functions && by_lz.contains_key(f.as_str()) {
                for line in touched_items(&lz, &p.hash, f, &lineage, &by_lz, root)? {
                    println!("        {line}");
                }
            }
        }
    }
    let methods: Vec<String> = ["cherry-pick", "cherry-pick-conflicts", "reimplemented"]
        .iter()
        .map(|m| {
            format!(
                "{m} {}",
                sync.commits.iter().filter(|r| r.method == *m).count()
            )
        })
        .collect();
    println!(
        "lz drift: {} commit(s) after pin {} on {}: {triaged} triaged ({}), {} open, {errors} record error(s)",
        triaged + pending.len(),
        short(&lz.pin),
        short(&tip),
        methods.join(", "),
        pending.len()
    );
    if strict && (!pending.is_empty() || errors > 0) {
        Err(format!(
            "{} LZ commit(s) without a record in {SYNC_FILE}",
            pending.len()
        )
        .into())
    } else if errors > 0 {
        Err(format!("{errors} record error(s) in {SYNC_FILE}").into())
    } else {
        Ok(())
    }
}

/// The LZ items a commit's diff of `file` touches, mapped to the native items that own them.
fn touched_items(
    lz: &Lz,
    hash: &str,
    file: &str,
    lineage: &Lineage,
    by_lz: &HashMap<&str, Vec<&str>>,
    root: &Path,
) -> Result<Vec<String>> {
    let diff = git_raw(&lz.repo, &["show", "--format=", "-U0", hash, "--", file])?;
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for l in diff.lines().filter(|l| l.starts_with("@@")) {
        let Some(plus) = l.split_whitespace().find(|w| w.starts_with('+')) else {
            continue;
        };
        let (start, len) = plus[1..]
            .split_once(',')
            .map_or((plus[1..].to_string(), "1".to_string()), |(a, b)| {
                (a.to_string(), b.to_string())
            });
        let (start, len): (usize, usize) = (start.parse().unwrap_or(0), len.parse().unwrap_or(1));
        ranges.push((start, start + len.max(1) - 1));
    }
    let after = match git_raw(&lz.repo, &["show", &format!("{hash}:{file}")]) {
        Ok(t) => t,
        Err(_) => return Ok(vec!["(deleted in LZ)".to_string()]),
    };
    let mut out = Vec::new();
    for it in items(&after) {
        if !ranges.iter().any(|(a, b)| *a <= it.end && *b >= it.start) {
            continue;
        }
        let targets = resolve_lz_item(file, &it.name, lineage, by_lz, root);
        out.push(format!("{}  ->  {}", it.name, targets.join(", ")));
    }
    if out.is_empty() {
        out.push("(no item touched: comments, docs or structure)".to_string());
    }
    Ok(out)
}

/// Where an LZ item's code lives now: the rows first, then the by-name default.
fn resolve_lz_item(
    file: &str,
    item: &str,
    lineage: &Lineage,
    by_lz: &HashMap<&str, Vec<&str>>,
    root: &Path,
) -> Vec<String> {
    let key = format!("{file}::{item}");
    let mut out = Vec::new();
    for m in &lineage.modules {
        for row in m.fns.iter().filter(|r| r.lz == key) {
            if row.kind == Kind::Dropped {
                out.push(format!("- (dropped: {})", row.reason));
            }
            for t in &row.native {
                out.push(if t.starts_with("sys/") {
                    format!("{t} ({:?})", row.kind).to_lowercase()
                } else {
                    format!("{}::{t} ({:?})", m.rust, row.kind).to_lowercase()
                });
            }
        }
    }
    if !out.is_empty() {
        return out;
    }
    for rust in by_lz.get(file).map(|v| v.as_slice()).unwrap_or(&[]) {
        let text = fs::read_to_string(root.join(rust)).unwrap_or_default();
        if items(&text).iter().any(|it| it.name == item) {
            out.push(format!("{rust}::{item} (by name)"));
        }
    }
    if out.is_empty() {
        out.push("(not found in native: new in LZ, or missing a row)".to_string());
    }
    out
}

// --- lz trace ----------------------------------------------------------------------------

/// `cargo xtask lz trace <lz path>:<item>` | `--rust <path>:<item>` | `--c <c path>:<item>`.
pub(crate) fn trace(root: &Path, args: &[&str]) -> Result<()> {
    let lineage = load_lineage(root)?;
    let lz = open_lz(root)?;
    let mut by_lz: HashMap<&str, Vec<&str>> = HashMap::new();
    for m in &lineage.modules {
        for l in &m.lz {
            by_lz.entry(l.as_str()).or_default().push(m.rust.as_str());
        }
    }
    let split = |s: &str| -> Result<(String, String)> {
        s.rsplit_once(':')
            .map(|(f, i)| (f.to_string(), i.to_string()))
            .ok_or_else(|| format!("expected <path>:<item>, got `{s}`").into())
    };
    match args {
        ["--rust", spec] => {
            let (file, item) = split(spec)?;
            let Some(m) = lineage.modules.iter().find(|m| m.rust == file) else {
                return Err(format!("{file}: not a module of {LINEAGE_FILE}").into());
            };
            let mut found = false;
            for row in m.fns.iter().filter(|r| {
                r.native.iter().any(|t| *t == item || t.ends_with(&format!("::{item}")))
            }) {
                println!("{file}::{item}  <-  {} ({:?})", row.lz, row.kind);
                found = true;
            }
            if !found {
                for l in &m.lz {
                    let text = lz.show(l).unwrap_or_default();
                    if items(&text).iter().any(|it| it.name == item) {
                        println!("{file}::{item}  <-  {l}::{item} (by name)");
                        found = true;
                    }
                }
            }
            if !found {
                println!("{file}::{item}: no LZ origin found (native code, or not an item)");
            }
            if !m.notes.is_empty() {
                println!("    notes: {}", m.notes);
            }
            for l in &m.lz {
                for up in lz.show(l).unwrap_or_default().lines().filter(|x| x.starts_with("//! Upstream:")) {
                    println!("    {l}: {up}");
                }
            }
            Ok(())
        }
        ["--c", spec] => {
            let (cpath, item) = split(spec)?;
            let hits = git(
                &lz.repo,
                &["grep", "-l", &format!("Upstream: {cpath} @"), &lz.pin, "--", "sys"],
            )
            .unwrap_or_default();
            let files: Vec<String> = hits
                .lines()
                .filter_map(|l| l.split_once(':').map(|(_, p)| p.to_string()))
                .collect();
            if files.is_empty() {
                return Err(format!("{cpath}: no LZ file names it in an Upstream: line at the pin").into());
            }
            for f in files {
                for t in resolve_lz_item(&f, &item, &lineage, &by_lz, root) {
                    println!("{cpath}:{item}  ->  {f}::{item}  ->  {t}");
                }
            }
            Ok(())
        }
        [spec] => {
            let (file, item) = split(spec)?;
            if !by_lz.contains_key(file.as_str()) {
                return Err(format!("{file}: no module derives from it ({LINEAGE_FILE})").into());
            }
            for t in resolve_lz_item(&file, &item, &lineage, &by_lz, root) {
                println!("{file}::{item}  ->  {t}");
            }
            Ok(())
        }
        _ => Err("usage: cargo xtask lz trace <lz path>:<item> | --rust <path>:<item> | --c <c path>:<item>".into()),
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_only_the_ident_lines() {
        let src = "/*\t$OpenBSD: a.c,v 1.1 2020/01/01 00:00:00 x Exp $\t*/\n/* <LICENSES> */\n/*\n * Copyright\n */\n";
        assert_eq!(
            strip_ident_lines(src),
            "/* <LICENSES> */\n/*\n * Copyright\n */\n"
        );
    }

    #[test]
    fn strips_the_blank_line_a_dropped_ident_leaves_behind() {
        // At the top of a file without a licence block: the blank line under the ident goes.
        let top = "/*\t$OpenBSD: a.h,v 1.1 x $\t*/\n\npub const A: u32 = 1;\n";
        assert_eq!(strip_ident_lines(top), "pub const A: u32 = 1;\n");
        // Between two blank lines (a second header merged in): one blank line stays.
        let mid = "a\n\n/*\t$OpenBSD: _types.h,v 1.10 x $\t*/\n\nb\n";
        assert_eq!(strip_ident_lines(mid), "a\n\nb\n");
        // Right after code: the blank line under it is real separation and stays.
        let after_code = "a\n/*\t$NetBSD: b.h,v 1.2 x $\t*/\n\nb\n";
        assert_eq!(strip_ident_lines(after_code), "a\n\nb\n");
        // The native side, already without the ident, is unchanged.
        assert_eq!(strip_ident_lines("a\n\nb\n"), "a\n\nb\n");
    }

    #[test]
    fn items_find_free_functions_methods_and_types() {
        let src = "pub struct Proc {\n    x: u32,\n}\n\nimpl Proc {\n    pub fn fork_thread(&self) {}\n    fn helper() -> u32 { 1 }\n}\n\nimpl fmt::Display for Proc {\n    fn fmt(&self) {}\n}\n\npub(crate) unsafe fn fork1() {\n    let s = \"{ not a brace }\";\n}\n\npub const MAXCOMLEN: usize = 16;\nmacro_rules! kassert { () => {} }\n\n#[cfg(test)]\nmod tests {\n    fn not_an_item() {}\n}\n";
        let names: Vec<String> = items(src).into_iter().map(|i| i.name).collect();
        assert_eq!(
            names,
            vec![
                "Proc",
                "Proc::fork_thread",
                "Proc::helper",
                "Proc::fmt",
                "fork1",
                "MAXCOMLEN",
                "kassert",
            ]
        );
    }

    #[test]
    fn items_have_line_spans() {
        let src = "fn a() {\n    1\n}\n\nfn b() {\n    2\n}\n";
        let found = items(src);
        assert_eq!((found[0].start, found[0].end), (1, 3));
        assert_eq!((found[1].start, found[1].end), (5, 7));
    }

    #[test]
    fn impl_type_handles_generics_and_paths() {
        assert_eq!(
            impl_type("impl<T: Into<U>> fmt::Display for Foo<T> {").as_deref(),
            Some("Foo")
        );
        assert_eq!(impl_type("impl Bar {").as_deref(), Some("Bar"));
        assert_eq!(
            impl_type("unsafe impl Send for Baz {}").as_deref(),
            Some("Baz")
        );
        assert_eq!(
            impl_type("impl<'a> Iterator for Iter<'a> {").as_deref(),
            Some("Iter")
        );
    }

    #[test]
    fn decl_name_skips_qualifiers() {
        assert_eq!(
            decl_name("pub(crate) const fn page_shift() -> u32 {").as_deref(),
            Some("page_shift")
        );
        assert_eq!(
            decl_name("pub const PAGE_SIZE: usize = 4096;").as_deref(),
            Some("PAGE_SIZE")
        );
        assert_eq!(decl_name("static mut X: u32 = 0;").as_deref(), Some("X"));
        assert_eq!(decl_name("use foo::bar;"), None);
        assert_eq!(
            decl_name("extern \"C\" fn trap() {").as_deref(),
            Some("trap")
        );
    }
}
/* </TESTS> */
