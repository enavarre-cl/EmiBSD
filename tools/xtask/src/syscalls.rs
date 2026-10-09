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
//! `cargo xtask gen-syscalls [--check]`: what `sys/kern/makesyscalls.sh` does for the C tree,
//! for the Rust tree. Reads `reference/openbsd-src/sys/kern/syscalls.master` and writes
//!
//! - `sys/sys/syscall.rs` (the `SYS_*` numbers, `syscall.h`),
//! - `sys/sys/syscallargs.rs` (the argument structs, `syscallargs.h`),
//! - `sys/kern/init_sysent.rs` (the switch table, `init_sysent.c`),
//! - `sys/kern/syscalls.rs` (the names, `syscalls.c`).
//!
//! The switch table points every entry at `sys_nosys` unless a `pub fn sys_<name>(` exists
//! in a file directly under `sys/kern/`, `sys/uvm/`, `sys/dev/` or `sys/nfs/` (`getentropy(2)`
//! lives in `dev/rnd.c`, `nfssvc(2)` in `nfs/nfs_syscalls.c`), so porting a syscall is: write
//! the function, rerun the generator. `--check` regenerates in memory and fails if the files on
//! disk differ (`just ci`). Of the kernel options the master file tests, `ACCOUNTING`,
//! `NFSCLIENT` and `NFSSERVER` are configured (as in GENERIC); for `PTRACE`, `KTRACE` and
//! `SYSV*` the `#else` branches are taken. `NFSCLIENT` and `NFSSERVER` are cargo features of
//! the kernel (`nfsclient`, `nfsserver`), so an entry they guard (`nfssvc`) is in [`GATED`]: its
//! table row is `cfg`-selected between the function and the `#else` branch's `sys_nosys`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::Result;

/// A parsed line of `syscalls.master`.
#[derive(Debug, Clone)]
struct Entry {
    number: u32,
    kind: Kind,
    nolock: bool,
    /// `sys_write`.
    func: String,
    /// `write` (the alias or the function name without `sys_`).
    alias: String,
    /// `ssize_t`.
    ret: String,
    /// `(int, fd)`, ...; empty for `(void)`.
    args: Vec<(String, String)>,
    /// How many arguments precede a `...`: `None` when not variadic.
    varargc: Option<usize>,
    /// The comment of an `OBSOL`/`UNIMPL` line: `unimplemented ptrace`.
    comment: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Std,
    NoDef,
    NoArgs,
    Obsol,
    Unimpl,
}

/// Kernel options the master file may test that the kernel configures: `ACCOUNTING`
/// (`kern_acct.c`, in GENERIC). `KTRACE`, `PTRACE`, `NFS*` and `SYSV*` are not.
const CONFIGURED_OPTIONS: &[&str] = &["ACCOUNTING", "NFSCLIENT", "NFSSERVER"];

/// Syscalls whose `sys_*` function exists only with a cargo feature (the C kernel option):
/// (function, `cfg` predicate). Without it the table entry is `sys_nosys`, which is what
/// the master file's `#else` branch (`UNIMPL`) gives.
const GATED: &[(&str, &str)] = &[(
    "sys_nfssvc",
    "any(feature = \"nfsclient\", feature = \"nfsserver\")",
)];

/// The C types the master file uses, mapped to Rust. Scalars keep their `sys/types.rs`
/// alias; pointers to kernel structures the tree does not have yet are opaque.
fn rust_type(ctype: &str) -> Result<(String, bool)> {
    let t = ctype.split_whitespace().collect::<Vec<_>>().join(" ");
    let scalar = |s: &str| Some(s.to_string());
    let base: Option<String> = match t.as_str() {
        "int" => scalar("i32"),
        "u_int" | "unsigned int" | "uint32_t" | "sigset_t" => scalar("u32"),
        "long" | "int64_t" => scalar("i64"),
        "u_long" => scalar("u64"),
        "size_t" => scalar("usize"),
        "ssize_t" => scalar("isize"),
        "off_t" => scalar("Off"),
        "pid_t" => scalar("Pid"),
        "uid_t" => scalar("Uid"),
        "gid_t" => scalar("Gid"),
        "mode_t" => scalar("Mode"),
        "dev_t" => scalar("Dev"),
        "id_t" => scalar("Id"),
        "key_t" => scalar("Key"),
        "clockid_t" => scalar("Clockid"),
        "socklen_t" => scalar("Socklen"),
        "caddr_t" | "char *" => scalar("*mut u8"),
        "const char *" => scalar("*const u8"),
        "char * const *" | "char *const *" => scalar("*const *const u8"),
        "void *" => scalar("*mut c_void"),
        "const void *" | "const volatile void *" => scalar("*const c_void"),
        "int *" => scalar("*mut i32"),
        "const int *" => scalar("*const i32"),
        "u_int *" | "uint32_t *" => scalar("*mut u32"),
        "int64_t *" => scalar("*mut i64"),
        "const int64_t *" => scalar("*const i64"),
        "size_t *" => scalar("*mut usize"),
        "socklen_t *" => scalar("*mut Socklen"),
        "gid_t *" => scalar("*mut Gid"),
        "const gid_t *" => scalar("*const Gid"),
        "uid_t *" => scalar("*mut Uid"),
        "pid_t *" => scalar("*mut Pid"),
        "const sigset_t *" => scalar("*const u32"),
        _ => None,
    };
    if let Some(b) = base {
        return Ok((b, false));
    }
    // Pointers to structures the kernel does not define yet (`struct stat *`, `fd_set *`,
    // `siginfo_t *`, ...): opaque, the C type stays in the field's doc comment.
    if let Some(stripped) = t.strip_suffix('*') {
        let constness = if stripped.trim().starts_with("const ") {
            "*const c_void"
        } else {
            "*mut c_void"
        };
        return Ok((constness.to_string(), true));
    }
    Err(format!("gen-syscalls: no Rust type for C `{t}`; add it to rust_type()").into())
}

/// `CamelCase` of a C identifier: `sys___tfork_args` -> `SysTforkArgs`.
fn camel(ident: &str) -> String {
    let mut out = String::new();
    for part in ident.split('_').filter(|p| !p.is_empty()) {
        let mut chars = part.chars();
        if let Some(c) = chars.next() {
            out.extend(c.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

/// A field name, raw when it is a Rust keyword (`type` -> `r#type`).
fn field(name: &str) -> String {
    const KEYWORDS: &[&str] = &[
        "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn",
        "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
        "return", "self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use",
        "where", "while", "async", "await", "dyn", "abstract", "become", "box", "do", "final",
        "macro", "override", "priv", "typeof", "unsized", "virtual", "yield", "try", "gen",
    ];
    if KEYWORDS.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_string()
    }
}

/// Parses the master file: the sed+awk of `makesyscalls.sh`.
fn parse(master: &str) -> Result<(String, Vec<Entry>)> {
    // (1) drop dollar signs, (2) join continuation lines.
    let text = master.replace('$', "").replace("\\\n", "");
    let mut lines = text.lines();
    let first = lines.next().unwrap_or_default().to_string();
    let mut entries = Vec::new();
    let mut expected: u32 = 0;
    // (active, number saved at #if)
    let mut stack: Vec<(bool, u32)> = Vec::new();
    for (lineno, raw) in lines.enumerate() {
        let lineno = lineno + 2;
        let line = raw.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with(';') {
            continue;
        }
        if let Some(directive) = line.trim_start().strip_prefix('#') {
            let directive = directive.trim_start();
            if directive.starts_with("include") {
                continue;
            }
            if directive.starts_with("if") {
                let active = CONFIGURED_OPTIONS.iter().any(|opt| directive.contains(opt));
                stack.push((active, expected));
                continue;
            }
            if directive.starts_with("else") {
                let Some((active, saved)) = stack.pop() else {
                    return Err(format!("syscalls.master:{lineno}: unbalanced #else").into());
                };
                // The #if branch, if it was taken, consumed its numbers; the #else branch
                // is then skipped and the numbering goes on from where it stopped.
                stack.push((!active, saved));
                continue;
            }
            if directive.starts_with("endif") {
                if stack.pop().is_none() {
                    return Err(format!("syscalls.master:{lineno}: unbalanced #endif").into());
                }
                continue;
            }
            return Err(format!("syscalls.master:{lineno}: unknown directive {line}").into());
        }
        if stack.iter().any(|(active, _)| !active) {
            continue;
        }
        // insert spaces around {, }, (, ), *, and commas
        let mut spaced = String::new();
        for c in line.chars() {
            if "{}()*,".contains(c) {
                spaced.push(' ');
                spaced.push(c);
                spaced.push(' ');
            } else {
                spaced.push(c);
            }
        }
        let fields: Vec<&str> = spaced.split_whitespace().collect();
        let number: u32 = fields[0]
            .parse()
            .map_err(|_| format!("syscalls.master:{lineno}: bad number {}", fields[0]))?;
        if number != expected {
            return Err(format!(
                "syscalls.master:{lineno}: syscall number out of sync at {expected}"
            )
            .into());
        }
        let kind = match fields[1] {
            "STD" => Kind::Std,
            "NODEF" => Kind::NoDef,
            "NOARGS" => Kind::NoArgs,
            "OBSOL" => Kind::Obsol,
            "UNIMPL" => Kind::Unimpl,
            other => {
                return Err(
                    format!("syscalls.master:{lineno}: unrecognized keyword {other}").into(),
                );
            }
        };
        let entry = match kind {
            Kind::Obsol | Kind::Unimpl => {
                let what = if kind == Kind::Obsol {
                    "obsolete"
                } else {
                    "unimplemented"
                };
                let comment = std::iter::once(what)
                    .chain(fields[2..].iter().copied())
                    .collect::<Vec<_>>()
                    .join(" ");
                Entry {
                    number,
                    kind,
                    nolock: false,
                    func: String::new(),
                    alias: String::new(),
                    ret: String::new(),
                    args: Vec::new(),
                    varargc: None,
                    comment,
                }
            }
            _ => parse_decl(number, kind, &fields, lineno)?,
        };
        entries.push(entry);
        expected += 1;
    }
    if !stack.is_empty() {
        return Err("syscalls.master: unterminated #if".into());
    }
    Ok((first, entries))
}

/// `parseline()`: `number type [NOLOCK] [alias] { ret sys_name(args); }`.
fn parse_decl(number: u32, kind: Kind, fields: &[&str], lineno: usize) -> Result<Entry> {
    let err = |msg: &str| -> Box<dyn std::error::Error> {
        format!("syscalls.master:{lineno}: {msg}").into()
    };
    let mut f = 2;
    let mut nolock = false;
    let mut alias = String::new();
    let mut end = fields.len();
    if fields[end - 1] != "}" {
        alias = fields[end - 1].to_string();
        end -= 1;
    }
    if fields[f] == "NOLOCK" {
        nolock = true;
        f += 1;
    }
    if fields[f]
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        alias = fields[f].to_string();
        f += 1;
    }
    if fields[f] != "{" {
        return Err(err("expected {"));
    }
    f += 1;
    if fields[end - 1] != "}" {
        return Err(err("expected }"));
    }
    end -= 1;
    if fields[end - 1] != ";" {
        return Err(err("expected ;"));
    }
    end -= 1;
    if fields[end - 1] != ")" {
        return Err(err("expected )"));
    }
    end -= 1;
    // the return type: everything up to the token before "("
    let paren = fields[f..end]
        .iter()
        .position(|&t| t == "(")
        .ok_or_else(|| err("function argument definition (maybe \"(\"?)"))?
        + f;
    let func = fields[paren - 1].to_string();
    let ret = join_type(&fields[f..paren - 1]);
    if alias.is_empty() {
        alias = func.strip_prefix("sys_").unwrap_or(&func).to_string();
    }
    f = paren + 1;
    let mut args = Vec::new();
    let mut varargc = None;
    if f == end - 1 || (f < end && fields[f] == "void" && f + 1 == end) {
        if fields[f] != "void" {
            return Err(err("argument definition"));
        }
    } else {
        // split the argument list on commas
        let mut cur: Vec<&str> = Vec::new();
        let mut tokens: Vec<Vec<&str>> = Vec::new();
        for &t in &fields[f..end] {
            if t == "," {
                tokens.push(std::mem::take(&mut cur));
            } else {
                cur.push(t);
            }
        }
        tokens.push(cur);
        for mut toks in tokens {
            if toks.first() == Some(&"...") {
                varargc = Some(args.len());
                toks.remove(0);
            }
            let Some(name) = toks.pop() else {
                return Err(err("argument definition"));
            };
            if toks.is_empty() {
                return Err(err("argument definition"));
            }
            args.push((join_type(&toks), name.to_string()));
        }
        if args.len() > 6 {
            return Err(err("too many syscall arguments (> 6)"));
        }
    }
    Ok(Entry {
        number,
        kind,
        nolock,
        func,
        alias,
        ret,
        args,
        varargc,
        comment: String::new(),
    })
}

/// Joins type tokens the way the awk does: a space between words, none after `*`.
fn join_type(tokens: &[&str]) -> String {
    let mut s = String::new();
    let mut prev = "";
    for &t in tokens {
        if !s.is_empty() && prev != "*" {
            s.push(' ');
        }
        s.push_str(t);
        prev = t;
    }
    s
}

/// The `sys_*` functions the tree defines: name -> module path.
fn ported_syscalls(root: &Path) -> Result<BTreeMap<String, String>> {
    let mut found = BTreeMap::new();
    for (dir, module) in [
        ("sys/kern", "crate::kern"),
        ("sys/uvm", "crate::uvm"),
        ("sys/dev", "crate::dev"),
        ("sys/nfs", "crate::nfs"),
    ] {
        let mut names: Vec<_> = fs::read_dir(root.join(dir))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "rs"))
            .collect();
        names.sort();
        for path in names {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            if stem == "init_sysent" || stem == "syscalls" {
                continue;
            }
            let text = fs::read_to_string(&path)?;
            let mut rest = text.as_str();
            while let Some(i) = rest.find("pub fn sys_") {
                let after = &rest[i + "pub fn ".len()..];
                let name: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                let len = name.len();
                if after[len..].starts_with('(') {
                    found.insert(name, format!("{module}::{stem}"));
                }
                rest = &after[len..];
            }
        }
    }
    Ok(found)
}

/// The pinned commit, from `reference/PINNED.md`.
fn pin(root: &Path) -> Result<String> {
    let text = fs::read_to_string(root.join("reference/PINNED.md"))?;
    text.lines()
        .find_map(|l| l.strip_prefix("Commit: "))
        .map(|c| c[..12].to_string())
        .ok_or_else(|| "reference/PINNED.md: no Commit: line".into())
}

/// The four generated files, as (path, contents).
pub fn generate(root: &Path) -> Result<Vec<(&'static str, String)>> {
    let master = fs::read_to_string(root.join("reference/openbsd-src/sys/kern/syscalls.master"))?;
    let (created_from, entries) = parse(&master)?;
    // doc comments may not hold tabs (clippy::tabs_in_doc_comments)
    let created_from = created_from.replace('\t', " ");
    let ported = ported_syscalls(root)?;
    let pin = pin(root)?;
    let maxsyscall = entries.len();
    let banner = |what: &str| {
        format!(
            "//! DO NOT EDIT: this file is automatically generated by `cargo xtask gen-syscalls`\n\
             //! (`tools/xtask/src/syscalls.rs`, what `kern/makesyscalls.sh` does for the C tree)\n\
             //! from `sys/kern/syscalls.master`.\n\
             //! created from{created_from}\n\
             //!\n\
             //! Upstream: {what} @ {pin}\n"
        )
    };

    // --- sys/sys/syscall.rs ---------------------------------------------------------------
    let mut numbers = String::new();
    numbers.push_str("//! `<sys/syscall.h>`: system call numbers.\n//!\n");
    numbers.push_str(&banner("sys/sys/syscall.h"));
    numbers.push_str("\n#![allow(non_upper_case_globals)] // the C names: SYS_write\n\n");
    for e in &entries {
        match e.kind {
            Kind::Std | Kind::NoArgs => {
                numbers.push_str(&format!(
                    "/// syscall: \"{}\" ret: \"{}\" args:{}\n",
                    e.alias,
                    e.ret,
                    args_doc(e)
                ));
                numbers.push_str(&format!(
                    "pub const SYS_{}: i32 = {};\n\n",
                    e.alias, e.number
                ));
            }
            Kind::NoDef => {
                numbers.push_str(&format!("// {} is {}\n\n", e.number, e.alias));
            }
            Kind::Obsol => {
                numbers.push_str(&format!("// {} is {}\n\n", e.number, e.comment));
            }
            Kind::Unimpl => {}
        }
    }
    numbers.push_str(&format!(
        "/// `SYS_MAXSYSCALL`: one past the last system call number.\npub const SYS_MAXSYSCALL: usize = {maxsyscall};\n"
    ));

    // --- sys/sys/syscallargs.rs ----------------------------------------------------------
    let mut args = String::new();
    args.push_str("//! `<sys/syscallargs.h>`: system call argument lists.\n//!\n");
    args.push_str(&banner("sys/sys/syscallargs.h"));
    args.push_str(
        "//!\n//! ## Deviations\n\
         //! - `syscallarg(x)` is the `Syscallarg<T>` union: one `register_t` slot per argument with\n\
         //!   the datum in the low bytes (the C's little-endian arm); `SCARG(uap, k)` is `uap.k.get()`.\n\
         //! - Pointers to structures the kernel does not define yet are `*mut c_void`/`*const c_void`;\n\
         //!   the C type is in the field's doc comment.\n\n",
    );
    let mut used_aliases: Vec<&str> = vec!["Register"];
    for e in &entries {
        if !matches!(e.kind, Kind::Std | Kind::NoDef) {
            continue;
        }
        for (ctype, _) in &e.args {
            let (rtype, _) = rust_type(ctype)?;
            for alias in [
                "Clockid", "Dev", "Gid", "Id", "Key", "Mode", "Off", "Pid", "Socklen", "Uid",
            ] {
                if rtype.contains(alias) && !used_aliases.contains(&alias) {
                    used_aliases.push(alias);
                }
            }
        }
    }
    used_aliases.sort_unstable();
    args.push_str(&format!(
        "use core::ffi::c_void;\n\nuse crate::sys::types::{{{}}};\n\n",
        used_aliases.join(", ")
    ));
    args.push_str(
        "/// `syscallarg(x)`: one system call argument slot, `register_t` wide.\n\
         #[repr(C)]\n#[derive(Clone, Copy)]\npub union Syscallarg<T: Copy> {\n    /// `pad`: the whole register.\n    pub pad: Register,\n    /// `datum`: the argument in the register's low bytes.\n    pub datum: T,\n}\n\n\
         impl<T: Copy> Syscallarg<T> {\n    /// `SCARG(uap, k)`: the argument as its C type.\n    pub fn get(self) -> T {\n        // SAFETY: the slot holds a register the caller filled; every bit pattern of a\n        // register is a valid `T` (an integer, a pointer or an alias of one), and on this\n        // little-endian machine `T` is the register's low bytes.\n        unsafe { self.datum }\n    }\n}\n\n",
    );
    let mut seen_structs = BTreeMap::new();
    for e in &entries {
        if e.args.is_empty() || !matches!(e.kind, Kind::Std | Kind::NoDef) {
            continue;
        }
        let name = format!("{}Args", camel(&e.func));
        if let Some(prev) = seen_structs.insert(name.clone(), e.func.clone()) {
            return Err(format!(
                "gen-syscalls: {name} is both {prev}_args and {}_args",
                e.func
            )
            .into());
        }
        args.push_str(&format!(
            "/// `struct {}_args`.\n#[repr(C)]\n#[derive(Clone, Copy)]\npub struct {name} {{\n",
            e.func
        ));
        for (ctype, cname) in &e.args {
            let (rtype, opaque) = rust_type(ctype)?;
            let note = if opaque {
                " (opaque until the structure is ported)"
            } else {
                ""
            };
            args.push_str(&format!(
                "    /// `{cname}`: `{ctype}`{note}.\n    pub {}: Syscallarg<{rtype}>,\n",
                field(cname)
            ));
        }
        args.push_str("}\n\n");
    }
    let mut args = args.trim_end().to_string();
    args.push('\n');

    // --- sys/kern/init_sysent.rs ---------------------------------------------------------
    let mut sysent = String::new();
    sysent.push_str("//! System call switch table.\n//!\n");
    sysent.push_str(&banner("sys/kern/init_sysent.c"));
    sysent.push_str(
        "//!\n//! ## Deviations\n\
         //! - An entry points at its `sys_*` function only when `sys/kern`, `sys/uvm`, `sys/dev` or\n\
         //!   `sys/nfs` defines it (`pub fn sys_<name>(`); the others are `sys_nosys` with a note,\n\
         //!   so the table always compiles. Rerun the generator after porting a syscall.\n\
         //! - An entry whose function exists only with a kernel option (`nfssvc`: `NFSCLIENT` or\n\
         //!   `NFSSERVER`, cargo features `nfsclient`, `nfsserver`) has two rows, one per `cfg`; the\n\
         //!   second is the master file's `#else` branch, `sys_nosys`.\n\n",
    );
    let mut imports: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut gated_imports: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let mut used_structs: Vec<String> = Vec::new();
    let mut rows = String::new();
    for e in &entries {
        match e.kind {
            Kind::Obsol | Kind::Unimpl => {
                rows.push_str(&format!(
                    "    Sysent::new(0, 0, 0, sys_nosys), // {} = {}\n",
                    e.number, e.comment
                ));
            }
            _ => {
                let argsize = if e.args.is_empty() || e.kind == Kind::NoArgs {
                    "0".to_string()
                } else {
                    let name = format!("{}Args", camel(&e.func));
                    used_structs.push(name.clone());
                    format!("size_of::<{name}>()")
                };
                let flags = if e.nolock { "SY_NOLOCK" } else { "0" };
                let gate = GATED
                    .iter()
                    .find(|(func, _)| *func == e.func)
                    .map(|(_, cfg)| *cfg);
                let (call, note) = match ported.get(&e.func) {
                    Some(module) => {
                        match gate {
                            Some(cfg) => gated_imports
                                .entry(cfg.to_string())
                                .or_default()
                                .push((module.clone(), e.func.clone())),
                            None => imports
                                .entry(module.clone())
                                .or_default()
                                .push(e.func.clone()),
                        }
                        (e.func.clone(), String::new())
                    }
                    None => ("sys_nosys".to_string(), format!(" ({} not ported)", e.func)),
                };
                match gate {
                    Some(cfg) if call == e.func => {
                        // The entry's argument structure is named in full: `used_structs` is
                        // for the ungated rows (the import would be unused without the feature).
                        let argsize =
                            argsize.replace("size_of::<", "size_of::<crate::sys::syscallargs::");
                        used_structs.pop();
                        rows.push_str(&format!(
                            "    #[cfg({cfg})]\n    Sysent::new({}, {argsize}, {flags}, {call}), // {} = {}\n",
                            e.args.len(),
                            e.number,
                            e.alias
                        ));
                        rows.push_str(&format!(
                            "    #[cfg(not({cfg}))]\n    Sysent::new(0, 0, 0, sys_nosys), // {} = {} ({} needs {cfg})\n",
                            e.number, e.alias, e.func
                        ));
                    }
                    _ => rows.push_str(&format!(
                        "    Sysent::new({}, {argsize}, {flags}, {call}), // {} = {}{note}\n",
                        e.args.len(),
                        e.number,
                        e.alias
                    )),
                }
            }
        }
    }
    sysent.push_str("use crate::kern::kern_sig::sys_nosys;\n");
    for (module, funcs) in &imports {
        let mut funcs = funcs.clone();
        funcs.sort();
        funcs.dedup();
        sysent.push_str(&format!("use {module}::{{{}}};\n", funcs.join(", ")));
    }
    for (cfg, funcs) in &gated_imports {
        for (module, func) in funcs {
            sysent.push_str(&format!("#[cfg({cfg})]\nuse {module}::{func};\n"));
        }
    }
    sysent.push_str("use crate::sys::syscall::SYS_MAXSYSCALL;\n");
    if !used_structs.is_empty() {
        used_structs.sort();
        used_structs.dedup();
        sysent.push_str("use crate::sys::syscallargs::{\n");
        for s in &used_structs {
            sysent.push_str(&format!("    {s},\n"));
        }
        sysent.push_str("};\n");
    }
    sysent.push_str("use crate::sys::systm::{SY_NOLOCK, Sysent};\n\n");
    sysent.push_str("/// `sysent[]`: the system call switch table, indexed by `SYS_*`.\n");
    sysent.push_str("pub static SYSENT: [Sysent; SYS_MAXSYSCALL] = [\n");
    sysent.push_str(&rows);
    sysent.push_str("];\n");

    // --- sys/kern/syscalls.rs ------------------------------------------------------------
    let mut names = String::new();
    names.push_str("//! System call names.\n//!\n");
    names.push_str(&banner("sys/kern/syscalls.c"));
    names.push_str("\nuse crate::sys::syscall::SYS_MAXSYSCALL;\n\n");
    names.push_str("/// `syscallnames[]`: the name of every system call number.\n");
    names.push_str("pub static SYSCALLNAMES: [&str; SYS_MAXSYSCALL] = [\n");
    for e in &entries {
        match e.kind {
            Kind::Obsol | Kind::Unimpl => names.push_str(&format!(
                "    \"#{} ({})\", // {} = {}\n",
                e.number, e.comment, e.number, e.comment
            )),
            _ => names.push_str(&format!(
                "    \"{}\", // {} = {}\n",
                e.alias, e.number, e.alias
            )),
        }
    }
    names.push_str("];\n");

    Ok(vec![
        ("sys/sys/syscall.rs", rustfmt(root, &zoned(&numbers))?),
        ("sys/sys/syscallargs.rs", rustfmt(root, &zoned(&args))?),
        ("sys/kern/init_sysent.rs", rustfmt(root, &zoned(&sysent))?),
        ("sys/kern/syscalls.rs", rustfmt(root, &zoned(&names))?),
    ])
}

/// The generated source in the zones of `layout.rs`: the author's block alone in `<LICENSES>`
/// (the C files carry no notice, `license = "none"` in lineage.toml; N0b), then all of the
/// source in `<CODE>`. The C files' RCS ident lines are not carried over (decision 20,
/// docs/PHASE2.md): the `//! Upstream:` banner names the pinned source.
fn zoned(src: &str) -> String {
    format!(
        "/* <LICENSES> */\n{}/* </LICENSES> */\n\n/* <CODE> */\n{}\n/* </CODE> */\n",
        crate::layout::AUTHOR_BLOCK,
        src.trim()
    )
}

/// Formats generated source with the workspace's `rustfmt.toml`, so `just fmt` and
/// `gen-syscalls --check` agree.
fn rustfmt(root: &Path, src: &str) -> Result<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("rustfmt")
        .args(["--edition", "2024", "--config-path"])
        .arg(root.join("rustfmt.toml"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("rustfmt: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("rustfmt: no stdin")?
        .write_all(src.as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Err("rustfmt failed on generated source".into());
    }
    Ok(String::from_utf8(out.stdout)?)
}

/// The ` "int" "const void *" "size_t"` part of the number file's comment.
fn args_doc(e: &Entry) -> String {
    let mut s = String::new();
    let n = e.varargc.unwrap_or(e.args.len());
    for (t, _) in &e.args[..n] {
        s.push_str(&format!(" \"{t}\""));
    }
    if e.varargc.is_some() {
        s.push_str(" \"...\"");
        for (t, _) in &e.args[n..] {
            s.push_str(&format!(" \"{t}\""));
        }
    }
    s
}

/// `cargo xtask gen-syscalls`: writes the files; with `check`, only compares them.
pub fn gen_syscalls(root: &Path, check: bool) -> Result<()> {
    let files = generate(root)?;
    let mut stale = Vec::new();
    for (rel, contents) in &files {
        let path = root.join(rel);
        let current = fs::read_to_string(&path).unwrap_or_default();
        if current == *contents {
            continue;
        }
        if check {
            stale.push(*rel);
        } else {
            fs::write(&path, contents)?;
            println!("wrote {rel}");
        }
    }
    if check && !stale.is_empty() {
        return Err(format!(
            "generated syscall files are stale: {}; run `just gen-syscalls`",
            stale.join(", ")
        )
        .into());
    }
    if check {
        println!("gen-syscalls: {} files up to date", files.len());
    }
    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = ";\t$OpenBSD: syscalls.master,v 1.1 2026/01/01 00:00:00 x Exp $\n\
#include <sys/param.h>\n\
0\tUNIMPL\t\tsyscall\n\
1\tSTD\t\t{ void sys_exit(int rval); }\n\
2\tSTD NOLOCK\t{ ssize_t sys_write(int fd, const void *buf, \\\n\t\t\t    size_t nbyte); }\n\
3\tSTD\t\t{ int sys_open(const char *path, int flags, ... mode_t mode); } open\n\
#ifdef PTRACE\n\
4\tSTD\t\t{ int sys_ptrace(int req); }\n\
#else\n\
4\tUNIMPL\t\tptrace\n\
#endif\n\
5\tOBSOL\t\tvtrace\n\
6\tSTD\t\t{ int sys_obreak(char *nsize); } break\n";

    #[test]
    fn parses_the_master_grammar() {
        let (first, entries) = parse(SAMPLE).unwrap();
        assert!(first.contains("OpenBSD: syscalls.master"));
        assert_eq!(entries.len(), 7);
        assert_eq!(entries[0].kind, Kind::Unimpl);
        assert_eq!(entries[0].comment, "unimplemented syscall");
        let w = &entries[2];
        assert_eq!(w.func, "sys_write");
        assert_eq!(w.alias, "write");
        assert!(w.nolock);
        assert_eq!(w.ret, "ssize_t");
        assert_eq!(
            w.args,
            vec![
                ("int".to_string(), "fd".to_string()),
                ("const void *".to_string(), "buf".to_string()),
                ("size_t".to_string(), "nbyte".to_string())
            ]
        );
        let o = &entries[3];
        assert_eq!(o.varargc, Some(2));
        assert_eq!(o.args.len(), 3);
        assert_eq!(entries[4].kind, Kind::Unimpl); // PTRACE not configured: the #else branch
        assert_eq!(entries[5].comment, "obsolete vtrace");
        assert_eq!(entries[6].alias, "break");
    }

    #[test]
    fn types_and_names_map_like_the_c() {
        assert_eq!(
            rust_type("const void *").unwrap(),
            ("*const c_void".into(), false)
        );
        assert_eq!(
            rust_type("struct stat *").unwrap(),
            ("*mut c_void".into(), true)
        );
        assert_eq!(
            rust_type("const struct timespec *").unwrap(),
            ("*const c_void".into(), true)
        );
        assert!(rust_type("struct foo").is_err());
        assert_eq!(camel("sys___tfork_args"), "SysTforkArgs");
        assert_eq!(field("type"), "r#type");
        assert_eq!(field("fd"), "fd");
    }
}
/* </TESTS> */
