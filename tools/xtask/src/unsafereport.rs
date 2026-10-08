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

/// One token of a Rust source file, as far as counting `unsafe` needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Tok {
    /// An identifier or keyword (`r#name` keeps its `r#`).
    Ident(String),
    /// One punctuation character (`{`, `;`, `#`, ...).
    Punct(char),
    /// A string, char, byte, number literal or a lifetime: its text does not matter.
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

/// `cargo xtask unsafe-report [--write]`.
pub(crate) fn unsafe_report(root: &Path, write: bool) -> Result<()> {
    let mut files = Vec::new();
    for dir in ["sys", "init"] {
        crate::walk_rs(&root.join(dir), &mut files)?;
    }
    files.sort();
    let mut lexed: BTreeMap<PathBuf, Vec<Tok>> = BTreeMap::new();
    for f in &files {
        let src = fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?;
        lexed.insert(f.clone(), lex(&src));
    }

    // Which files are test-only: by name first, then by the test-only `mod x;` declarations,
    // until nothing changes (a test file's own `mod` lines declare test files too).
    let mut test_files: BTreeSet<PathBuf> =
        files.iter().filter(|f| is_test_path(f)).cloned().collect();
    loop {
        let mut added = false;
        for (f, toks) in &lexed {
            let scan = scan(toks, test_files.contains(f));
            for name in scan.test_mods {
                for child in child_module_paths(f, &name) {
                    if lexed.contains_key(&child) && test_files.insert(child) {
                        added = true;
                    }
                }
            }
        }
        if !added {
            break;
        }
    }

    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    for (f, toks) in &lexed {
        let rel = f.strip_prefix(root).unwrap_or(f);
        let rel = rel.to_string_lossy().replace('\\', "/");
        let is_test = test_files.contains(f);
        let row = rows.entry(subsystem(root, &rel)).or_default();
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
    let line = totals_line(&kernel, &test);
    println!("\n{line}");

    if write {
        let path = root.join(STATUS_DOC);
        let doc = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let new = replace_status_line(&doc, &line)?;
        fs::write(&path, new).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("wrote {STATUS_DOC}");
    }
    Ok(())
}

/// The stable one-line summary recorded in docs/STATUS.md.
pub(crate) fn totals_line(k: &Counts, t: &Counts) -> String {
    format!(
        "{STATUS_PREFIX} (`cargo xtask unsafe-report`): kernel {} blocks, {} fn, {} impl, {} trait, \
         {} other; tests {} more.",
        k.blocks,
        k.fns,
        k.impls,
        k.traits,
        k.other,
        t.total()
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

/// What [`scan`] finds in one file.
pub(crate) struct Scan {
    /// Every `unsafe` keyword: its kind and whether it sits in test code.
    pub(crate) found: Vec<(Kind, bool)>,
    /// The names of the `mod name;` declarations that are test-only.
    pub(crate) test_mods: Vec<String>,
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
    Scan { found, test_mods }
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
                Some(Tok::Lit) => &after[2..],
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
            i = skip_string(&s, i);
            toks.push(Tok::Lit);
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
}
/* </TESTS> */
