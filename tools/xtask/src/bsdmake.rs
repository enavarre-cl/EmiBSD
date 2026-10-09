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
//! `bsdmake`: the subset of OpenBSD `make(1)` that the Makefiles `xtask userland` reads use.
//!
//! It is an evaluator, not a builder: it reads a Makefile and everything it `.include`s,
//! keeps the variables (lazily expanded, as make does), the `.PATH` search list and the
//! explicit rules (targets, sources, shell commands), and answers questions about them. The
//! caller (`userland.rs`) decides what to build and runs the rules' commands through
//! `/bin/sh`, as make would.
//!
//! Supported, because the libc, csu, libutil and program Makefiles use it:
//!
//! - assignments `=`, `+=`, `?=`, `:=` (`!=` is refused);
//! - `${VAR}`, `$(VAR)`, `$X`, nested names (`${SRCS_${MACHINE_CPU}}`) and the modifiers
//!   `:L` and `:U` (lower/upper case, OpenBSD's meaning), `:M`, `:N`, `:R`, `:E`, `:T`, `:H`,
//!   `:S/old/new/[g]` with `^`, `$` and `&`, and the System V `:old=new` with `%`;
//! - `.include "file"`, `.include <file>` (the `share/mk` files are not in the reference
//!   clone; the ones the Makefiles name are stood in for by short texts, see
//!   `userland.rs`), `.sinclude`/`.-include`;
//! - `.if`, `.ifdef`, `.ifndef`, `.elif`, `.else`, `.endif` with `defined()`, `exists()`,
//!   `empty()`, `make()` (always false), `target()`, `!`, `&&`, `||`, parentheses and the
//!   comparisons `==`, `!=`, `<`, `<=`, `>`, `>=`;
//! - `.for var... in list` / `.endfor` (one or more loop variables, taking the words that
//!   many at a time), by textual substitution like make;
//! - explicit rules `targets: sources [; command]` with tab-indented commands, `.PATH:`.
//!
//! Anything else (`.undef`, `.error`, unknown modifiers, special targets other than the
//! harmless ones, shell assignments) is an error naming the file and line: a construct the
//! evaluator does not understand must never be skipped silently.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::Result;

/// An explicit rule: `targets: sources` and its shell commands (unexpanded).
#[derive(Debug, Clone)]
pub struct Rule {
    pub targets: Vec<String>,
    pub sources: Vec<String>,
    pub commands: Vec<String>,
}

/// Target-local variables (`$@`, `$>`, ...), already expanded.
pub type Locals = HashMap<String, String>;

/// A Makefile evaluated with its includes.
pub struct Make {
    vars: HashMap<String, String>,
    path: Vec<PathBuf>,
    rules: Vec<Rule>,
    curdir: PathBuf,
    sys_mk: HashMap<String, String>,
}

/// Conditional nesting state for one `.if` ... `.endif`.
struct Cond {
    parent_active: bool,
    active: bool,
    taken: bool,
}

/// Parsing state shared by a file and the files it includes.
struct Parser {
    conds: Vec<Cond>,
    /// Index into `Make::rules` of the rule whose commands follow, if any.
    open_rule: Option<usize>,
}

impl Parser {
    fn active(&self) -> bool {
        self.conds.last().is_none_or(|c| c.active)
    }
}

/// Special targets that only steer make's own behaviour; their lines are accepted and
/// ignored.
const IGNORED_SPECIAL_TARGETS: &[&str] = &[
    ".PHONY",
    ".SUFFIXES",
    ".MAIN",
    ".PRECIOUS",
    ".NOTPARALLEL",
    ".SILENT",
    ".IGNORE",
    ".DEFAULT",
    ".BEGIN",
    ".END",
    ".INTERRUPT",
    ".ORDER",
    ".WAIT",
];

const MAX_DEPTH: usize = 64;

impl Make {
    /// An empty evaluator whose `.CURDIR` is `curdir`, with `predefined` variables (what
    /// `sys.mk` and `bsd.own.mk` would have set) and the stand-ins for `<file>` includes.
    pub fn new(curdir: &Path, predefined: &[(&str, String)], sys_mk: &[(&str, &str)]) -> Self {
        let mut vars: HashMap<String, String> = predefined
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect();
        vars.insert(".CURDIR".into(), curdir.display().to_string());
        Make {
            vars,
            path: Vec::new(),
            rules: Vec::new(),
            curdir: curdir.to_path_buf(),
            sys_mk: sys_mk
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        }
    }

    /// Reads `file` (and its includes).
    pub fn read(&mut self, file: &Path) -> Result<()> {
        let text = fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        let mut p = Parser {
            conds: Vec::new(),
            open_rule: None,
        };
        self.parse_text(&text, &file.display().to_string(), file.parent(), &mut p)?;
        if !p.conds.is_empty() {
            return Err(format!("{}: unterminated .if", file.display()).into());
        }
        Ok(())
    }

    /// Sets a global variable (a raw, lazily expanded value).
    pub fn set(&mut self, name: &str, value: &str) {
        self.vars.insert(name.to_string(), value.to_string());
    }

    /// Appends `dir` to the `.PATH` search list, as `.PATH: dir` would.
    pub fn add_path(&mut self, dir: &Path) {
        self.path.push(dir.to_path_buf());
    }

    /// Whether `name` is defined.
    pub fn defined(&self, name: &str) -> bool {
        self.vars.contains_key(name)
    }

    /// Removes the word `word` from the raw value of `name`; returns whether it was there.
    /// Used for the documented compiler-flag workarounds (`-fret-clean`).
    pub fn remove_word(&mut self, name: &str, word: &str) -> bool {
        let Some(v) = self.vars.get_mut(name) else {
            return false;
        };
        let before = v.split_whitespace().count();
        let kept: Vec<&str> = v.split_whitespace().filter(|w| *w != word).collect();
        let removed = kept.len() != before;
        *v = kept.join(" ");
        removed
    }

    /// The expanded value of `name`.
    pub fn var(&self, name: &str) -> Result<String> {
        self.expand(&format!("${{{name}}}"))
    }

    /// The expanded value of `name`, split into words.
    pub fn words(&self, name: &str) -> Result<Vec<String>> {
        Ok(self
            .var(name)?
            .split_whitespace()
            .map(str::to_string)
            .collect())
    }

    /// Expands `s` with the global variables.
    pub fn expand(&self, s: &str) -> Result<String> {
        self.expand_in(s, &Locals::new(), 0)
    }

    /// Expands `s` with target-local variables as well.
    pub fn expand_local(&self, s: &str, locals: &Locals) -> Result<String> {
        self.expand_in(s, locals, 0)
    }

    /// The first `name` found in `.CURDIR`, then the `.PATH` directories, in order. The
    /// file name must match exactly, as on OpenBSD's file systems: on macOS's, which ignore
    /// case, `DWARFUnit.cpp` would otherwise be found as `CodeGen/AsmPrinter/DwarfUnit.cpp`
    /// in a `.PATH` directory that comes first (`libLLVM`, M14).
    pub fn search(&self, name: &str) -> Option<PathBuf> {
        if Path::new(name).is_absolute() {
            return Path::new(name).is_file().then(|| PathBuf::from(name));
        }
        std::iter::once(&self.curdir)
            .chain(self.path.iter())
            .map(|d| d.join(name))
            .find(|p| p.is_file() && exact_case(p))
    }

    /// Every source any rule names for `target`, with or without commands (`includes:
    /// prereq`, `prereq: obj_mac.h`), in the order the rules were read.
    pub fn sources_of(&self, target: &str) -> Vec<String> {
        self.rules
            .iter()
            .filter(|r| r.targets.iter().any(|t| t == target))
            .flat_map(|r| r.sources.iter().cloned())
            .collect()
    }

    /// The rule that has commands to make `target`, if any.
    pub fn rule_for(&self, target: &str) -> Option<&Rule> {
        self.rules
            .iter()
            .find(|r| !r.commands.is_empty() && r.targets.iter().any(|t| t == target))
    }

    // --- parsing ---------------------------------------------------------------------------

    fn parse_text(
        &mut self,
        text: &str,
        file: &str,
        dir: Option<&Path>,
        p: &mut Parser,
    ) -> Result<()> {
        let lines = logical_lines(text);
        self.parse_lines(&lines, file, dir, p)
    }

    fn parse_lines(
        &mut self,
        lines: &[(usize, String)],
        file: &str,
        dir: Option<&Path>,
        p: &mut Parser,
    ) -> Result<()> {
        let mut i = 0;
        while i < lines.len() {
            let (lineno, raw) = &lines[i];
            let at = format!("{file}:{lineno}");
            i += 1;

            if raw.starts_with('\t') {
                if !p.active() {
                    continue;
                }
                let cmd = raw.trim_start_matches('\t');
                if cmd.trim().is_empty() || cmd.trim_start().starts_with('#') {
                    continue;
                }
                match p.open_rule {
                    Some(r) => self.rules[r].commands.push(cmd.to_string()),
                    None => {
                        return Err(format!("{at}: shell command outside a rule").into());
                    }
                }
                continue;
            }

            let line = strip_comment(raw);
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Some((kw, arg)) = directive(trimmed) {
                match kw {
                    "if" | "ifdef" | "ifndef" | "ifmake" | "ifnmake" => {
                        let parent_active = p.active();
                        let result = if parent_active {
                            self.eval_if(kw, arg, &at)?
                        } else {
                            false
                        };
                        p.conds.push(Cond {
                            parent_active,
                            active: parent_active && result,
                            taken: result,
                        });
                    }
                    "elif" | "elifdef" | "elifndef" => {
                        let (parent_active, taken) = match p.conds.last() {
                            Some(c) => (c.parent_active, c.taken),
                            None => return Err(format!("{at}: .{kw} without .if").into()),
                        };
                        let result = if parent_active && !taken {
                            self.eval_if(&kw.replacen("el", "", 1), arg, &at)?
                        } else {
                            false
                        };
                        if let Some(c) = p.conds.last_mut() {
                            c.active = parent_active && !taken && result;
                            c.taken = taken || result;
                        }
                    }
                    "else" => match p.conds.last_mut() {
                        Some(c) => {
                            c.active = c.parent_active && !c.taken;
                            c.taken = true;
                        }
                        None => return Err(format!("{at}: .else without .if").into()),
                    },
                    "endif" => {
                        if p.conds.pop().is_none() {
                            return Err(format!("{at}: .endif without .if").into());
                        }
                    }
                    "for" => {
                        let start = i;
                        let mut nest = 1;
                        while i < lines.len() {
                            if let Some((k, _)) = directive(strip_comment(&lines[i].1).trim()) {
                                if k == "for" {
                                    nest += 1;
                                } else if k == "endfor" {
                                    nest -= 1;
                                    if nest == 0 {
                                        break;
                                    }
                                }
                            }
                            i += 1;
                        }
                        if i >= lines.len() {
                            return Err(format!("{at}: .for without .endfor").into());
                        }
                        let body = &lines[start..i];
                        i += 1; // the .endfor
                        if p.active() {
                            self.run_for(arg, body, file, dir, p, &at)?;
                        }
                    }
                    "endfor" => return Err(format!("{at}: .endfor without .for").into()),
                    "include" | "sinclude" | "-include" => {
                        if p.active() {
                            self.include(arg, kw != "include", dir, p, &at)?;
                        }
                    }
                    other => {
                        if p.active() {
                            return Err(format!("{at}: unsupported directive .{other}").into());
                        }
                    }
                }
                continue;
            }

            if !p.active() {
                continue;
            }
            p.open_rule = None;
            self.parse_statement(trimmed, &at, p)?;
        }
        Ok(())
    }

    fn parse_statement(&mut self, line: &str, at: &str, p: &mut Parser) -> Result<()> {
        let bytes = line.as_bytes();
        let mut depth = 0usize;
        let mut k = 0;
        while k < bytes.len() {
            let c = bytes[k];
            if c == b'$' && k + 1 < bytes.len() && (bytes[k + 1] == b'{' || bytes[k + 1] == b'(') {
                depth += 1;
                k += 2;
                continue;
            }
            if depth > 0 {
                if c == b'}' || c == b')' {
                    depth -= 1;
                } else if c == b'{' || c == b'(' {
                    depth += 1;
                }
                k += 1;
                continue;
            }
            if c == b':' {
                if bytes.get(k + 1) == Some(&b'=') {
                    return self.assign(&line[..k], ":=", &line[k + 2..], at);
                }
                return self.rule(line, k, at, p);
            }
            if c == b'=' {
                let (name, op) = match k.checked_sub(1).map(|j| bytes[j]) {
                    Some(b'+') => (&line[..k - 1], "+="),
                    Some(b'?') => (&line[..k - 1], "?="),
                    Some(b'!') => (&line[..k - 1], "!="),
                    _ => (&line[..k], "="),
                };
                return self.assign(name, op, &line[k + 1..], at);
            }
            k += 1;
        }
        Err(format!("{at}: line is neither an assignment, a rule nor a directive: {line}").into())
    }

    fn assign(&mut self, name: &str, op: &str, value: &str, at: &str) -> Result<()> {
        let name = self.expand(name.trim())?;
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err(format!("{at}: bad variable name `{name}`").into());
        }
        let value = value.trim();
        match op {
            "=" => {
                self.vars.insert(name, value.to_string());
            }
            "+=" => {
                let v = self.vars.entry(name).or_default();
                if !v.is_empty() && !value.is_empty() {
                    v.push(' ');
                }
                v.push_str(value);
            }
            "?=" => {
                self.vars.entry(name).or_insert_with(|| value.to_string());
            }
            ":=" => {
                let v = self.expand(value)?;
                // A `:=` value is final: escape `$` so a later lazy expansion keeps it.
                self.vars.insert(name, v.replace('$', "$$"));
            }
            _ => {
                return Err(format!("{at}: `{op}` (shell assignment) is not supported").into());
            }
        }
        Ok(())
    }

    fn rule(&mut self, line: &str, colon: usize, at: &str, p: &mut Parser) -> Result<()> {
        let targets: Vec<String> = self
            .expand(&line[..colon])?
            .split_whitespace()
            .map(str::to_string)
            .collect();
        let mut rest = &line[colon + 1..];
        if let Some(r) = rest.strip_prefix(':') {
            rest = r;
        }
        let (srcs, inline) = match find_top_level(rest, b';') {
            Some(j) => (&rest[..j], Some(rest[j + 1..].trim())),
            None => (rest, None),
        };
        let sources: Vec<String> = self
            .expand(srcs)?
            .split_whitespace()
            .map(str::to_string)
            .collect();

        if let Some(special) = targets.iter().find(|t| t.starts_with('.') && is_upper(t)) {
            if special == ".PATH" {
                if sources.is_empty() {
                    self.path.clear();
                }
                for s in &sources {
                    let d = PathBuf::from(s);
                    let d = if d.is_absolute() {
                        d
                    } else {
                        self.curdir.join(d)
                    };
                    if !self.path.contains(&d) {
                        self.path.push(d);
                    }
                }
                return Ok(());
            }
            if IGNORED_SPECIAL_TARGETS.contains(&special.as_str()) {
                return Ok(());
            }
            return Err(format!("{at}: unsupported special target {special}").into());
        }
        if targets.is_empty() {
            // `${EMPTY}: ...` declares nothing.
            return Ok(());
        }

        let mut commands = Vec::new();
        if let Some(c) = inline.filter(|c| !c.is_empty()) {
            commands.push(c.to_string());
        }
        self.rules.push(Rule {
            targets,
            sources,
            commands,
        });
        p.open_rule = Some(self.rules.len() - 1);
        Ok(())
    }

    fn include(
        &mut self,
        arg: &str,
        optional: bool,
        dir: Option<&Path>,
        p: &mut Parser,
        at: &str,
    ) -> Result<()> {
        let arg = arg.trim();
        if let Some(name) = arg.strip_prefix('<').and_then(|a| a.strip_suffix('>')) {
            let Some(text) = self.sys_mk.get(name).cloned() else {
                return Err(format!(
                    "{at}: <{name}> is a share/mk file the evaluator does not stand in for"
                )
                .into());
            };
            let saved = p.open_rule.take();
            self.parse_text(&text, &format!("<{name}>"), Some(&self.curdir.clone()), p)?;
            p.open_rule = saved;
            return Ok(());
        }
        let Some(name) = arg.strip_prefix('"').and_then(|a| a.strip_suffix('"')) else {
            return Err(format!("{at}: malformed .include {arg}").into());
        };
        let name = self.expand(name)?;
        let mut candidates = Vec::new();
        if Path::new(&name).is_absolute() {
            candidates.push(PathBuf::from(&name));
        } else {
            if let Some(d) = dir {
                candidates.push(d.join(&name));
            }
            candidates.push(self.curdir.join(&name));
        }
        match candidates.into_iter().find(|c| c.is_file()) {
            Some(file) => {
                let text =
                    fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
                let depth = p.conds.len();
                self.parse_text(&text, &file.display().to_string(), file.parent(), p)?;
                if p.conds.len() != depth {
                    return Err(format!("{}: unbalanced .if/.endif", file.display()).into());
                }
                Ok(())
            }
            None if optional => Ok(()),
            None => Err(format!("{at}: cannot find included file {name}").into()),
        }
    }

    fn run_for(
        &mut self,
        arg: &str,
        body: &[(usize, String)],
        file: &str,
        dir: Option<&Path>,
        p: &mut Parser,
        at: &str,
    ) -> Result<()> {
        let Some((vars, list)) = arg.split_once(" in ") else {
            return Err(format!("{at}: malformed .for {arg}").into());
        };
        // `.for dir f in ${SSLASM}` takes the words two at a time (libcrypto's amd64
        // Makefile.inc); the word count must be a multiple of the variable count.
        let vars: Vec<&str> = vars.split_whitespace().collect();
        if vars.is_empty() {
            return Err(format!("{at}: .for without a loop variable").into());
        }
        let values: Vec<String> = self
            .expand(list)?
            .split_whitespace()
            .map(str::to_string)
            .collect();
        if !values.len().is_multiple_of(vars.len()) {
            return Err(format!(
                "{at}: .for over {} words with {} variables",
                values.len(),
                vars.len()
            )
            .into());
        }
        for group in values.chunks(vars.len()) {
            let mut lines = Vec::with_capacity(body.len());
            for (n, l) in body {
                let mut l = l.clone();
                for (var, value) in vars.iter().zip(group) {
                    l = self.substitute_for(&l, var, value)?;
                }
                lines.push((*n, l));
            }
            self.parse_lines(&lines, file, dir, p)?;
        }
        Ok(())
    }

    /// Replaces `${var}`, `${var:mods}` and (one-letter names) `$var` by the loop value.
    fn substitute_for(&self, line: &str, var: &str, value: &str) -> Result<String> {
        let mut out = String::with_capacity(line.len());
        let mut rest = line;
        while let Some(pos) = rest.find('$') {
            out.push_str(&rest[..pos]);
            let after = &rest[pos + 1..];
            if let Some(a) = after.strip_prefix('$') {
                out.push_str("$$");
                rest = a;
                continue;
            }
            if after.starts_with('{') || after.starts_with('(') {
                let close = matching_close(after)
                    .ok_or_else(|| format!("unbalanced variable reference in `{line}`"))?;
                let inner = &after[1..close];
                if inner == var {
                    out.push_str(value);
                } else if let Some(mods) = inner.strip_prefix(var).and_then(|m| m.strip_prefix(':'))
                {
                    let mods = self.substitute_for(mods, var, value)?;
                    out.push_str(&self.apply_modifiers(value, &mods, &Locals::new(), 0)?);
                } else {
                    // Another variable: substitute inside it (`${ASM:N${i:R}.o}`).
                    out.push('$');
                    out.push_str(&after[..1]);
                    out.push_str(&self.substitute_for(inner, var, value)?);
                    out.push_str(&after[close..=close]);
                }
                rest = &after[close + 1..];
                continue;
            }
            if var.len() == 1 && after.starts_with(var) {
                out.push_str(value);
                rest = &after[1..];
                continue;
            }
            out.push('$');
            rest = after;
        }
        out.push_str(rest);
        Ok(out)
    }

    // --- conditionals ----------------------------------------------------------------------

    fn eval_if(&self, kw: &str, arg: &str, at: &str) -> Result<bool> {
        match kw {
            "if" => {
                let mut c = CondParser {
                    s: arg.as_bytes(),
                    i: 0,
                    make: self,
                };
                let v = c.or_expr().map_err(|e| format!("{at}: .if {arg}: {e}"))?;
                c.ws();
                if c.i != c.s.len() {
                    return Err(format!("{at}: .if {arg}: trailing text").into());
                }
                Ok(v)
            }
            "ifdef" => Ok(self.defined(&self.expand(arg.trim())?)),
            "ifndef" => Ok(!self.defined(&self.expand(arg.trim())?)),
            // No target is ever requested on the command line.
            "ifmake" => Ok(false),
            "ifnmake" => Ok(true),
            _ => Err(format!("{at}: unsupported conditional .{kw}").into()),
        }
    }

    // --- expansion -------------------------------------------------------------------------

    fn expand_in(&self, s: &str, locals: &Locals, depth: usize) -> Result<String> {
        if depth > MAX_DEPTH {
            return Err(
                format!("variable expansion too deep (recursive variable?) in `{s}`").into(),
            );
        }
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(pos) = rest.find('$') {
            out.push_str(&rest[..pos]);
            let after = &rest[pos + 1..];
            let Some(c) = after.chars().next() else {
                out.push('$');
                rest = after;
                break;
            };
            match c {
                '$' => {
                    out.push('$');
                    rest = &after[1..];
                }
                '{' | '(' => {
                    let close = matching_close(after)
                        .ok_or_else(|| format!("unbalanced variable reference in `{s}`"))?;
                    out.push_str(&self.eval_ref(&after[1..close], locals, depth)?);
                    rest = &after[close + 1..];
                }
                _ => {
                    let name = c.to_string();
                    out.push_str(&self.lookup(&name, locals, depth)?);
                    rest = &after[c.len_utf8()..];
                }
            }
        }
        out.push_str(rest);
        Ok(out)
    }

    fn lookup(&self, name: &str, locals: &Locals, depth: usize) -> Result<String> {
        if let Some(v) = locals.get(name) {
            return Ok(v.clone());
        }
        match self.vars.get(name) {
            Some(raw) => self.expand_in(raw, locals, depth + 1),
            None => Ok(String::new()),
        }
    }

    /// `NAME[:mod...]`, the inside of `${...}`.
    fn eval_ref(&self, inner: &str, locals: &Locals, depth: usize) -> Result<String> {
        let split = find_top_level(inner, b':');
        let (name, mods) = match split {
            Some(j) => (&inner[..j], Some(&inner[j + 1..])),
            None => (inner, None),
        };
        let name = self.expand_in(name, locals, depth + 1)?;
        let value = self.lookup(&name, locals, depth)?;
        match mods {
            Some(m) => self.apply_modifiers(&value, m, locals, depth),
            None => Ok(value),
        }
    }

    fn apply_modifiers(
        &self,
        value: &str,
        mods: &str,
        locals: &Locals,
        depth: usize,
    ) -> Result<String> {
        let mut words: Vec<String> = value.split_whitespace().map(str::to_string).collect();
        let mut rest = mods;
        while !rest.is_empty() {
            let first = rest.as_bytes()[0];
            let simple_end = rest.len() == 1 || rest.as_bytes()[1] == b':';
            match first {
                b'L' | b'U' | b'R' | b'E' | b'T' | b'H' if simple_end => {
                    words = words
                        .into_iter()
                        .map(|w| match first {
                            b'L' => w.to_lowercase(),
                            b'U' => w.to_uppercase(),
                            b'R' => strip_suffix_word(&w).to_string(),
                            b'E' => suffix_of(&w).to_string(),
                            b'T' => w.rsplit('/').next().unwrap_or(&w).to_string(),
                            _ => match w.rfind('/') {
                                Some(k) => w[..k].to_string(),
                                None => ".".to_string(),
                            },
                        })
                        .collect();
                    rest = rest.get(2..).unwrap_or("");
                }
                b'M' | b'N' => {
                    let end = find_top_level(&rest[1..], b':').map_or(rest.len(), |j| j + 1);
                    let pat = self.expand_in(&rest[1..end], locals, depth + 1)?;
                    let keep = first == b'M';
                    words.retain(|w| glob(pat.as_bytes(), w.as_bytes()) == keep);
                    rest = rest.get(end + 1..).unwrap_or("");
                }
                b'S' => {
                    let (old, new, flags, used) = parse_subst(&rest[1..])
                        .ok_or_else(|| format!("malformed :S modifier in `{mods}`"))?;
                    let old = self.expand_in(&old, locals, depth + 1)?;
                    let new = self.expand_in(&new, locals, depth + 1)?;
                    let global = flags.contains('g');
                    if flags.chars().any(|f| f != 'g') {
                        return Err(format!("unsupported :S flags `{flags}` in `{mods}`").into());
                    }
                    words = words
                        .into_iter()
                        .map(|w| subst_word(&w, &old, &new, global))
                        .collect();
                    rest = rest.get(1 + used..).unwrap_or("");
                    rest = rest.strip_prefix(':').unwrap_or(rest);
                }
                _ => {
                    // System V substitution `old=new`, the last modifier.
                    let Some(eq) = find_top_level(rest, b'=') else {
                        return Err(format!("unsupported modifier `:{rest}`").into());
                    };
                    let old = self.expand_in(&rest[..eq], locals, depth + 1)?;
                    let new = self.expand_in(&rest[eq + 1..], locals, depth + 1)?;
                    words = words
                        .into_iter()
                        .map(|w| sysv_word(&w, &old, &new))
                        .collect();
                    rest = "";
                }
            }
        }
        Ok(words.join(" "))
    }
}

// --- conditional expression parser ----------------------------------------------------------

struct CondParser<'a> {
    s: &'a [u8],
    i: usize,
    make: &'a Make,
}

impl CondParser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn eat(&mut self, tok: &str) -> bool {
        self.ws();
        if self.s[self.i..].starts_with(tok.as_bytes()) {
            self.i += tok.len();
            true
        } else {
            false
        }
    }

    fn or_expr(&mut self) -> Result<bool> {
        let mut v = self.and_expr()?;
        while self.eat("||") {
            let r = self.and_expr()?;
            v = v || r;
        }
        Ok(v)
    }

    fn and_expr(&mut self) -> Result<bool> {
        let mut v = self.unary()?;
        while self.eat("&&") {
            let r = self.unary()?;
            v = v && r;
        }
        Ok(v)
    }

    fn unary(&mut self) -> Result<bool> {
        self.ws();
        if self.s.get(self.i) == Some(&b'!') && self.s.get(self.i + 1) != Some(&b'=') {
            self.i += 1;
            return Ok(!self.unary()?);
        }
        if self.eat("(") {
            let v = self.or_expr()?;
            if !self.eat(")") {
                return Err("missing `)`".into());
            }
            return Ok(v);
        }
        self.atom()
    }

    fn text(&self, from: usize, to: usize) -> String {
        String::from_utf8_lossy(&self.s[from..to]).into_owned()
    }

    fn atom(&mut self) -> Result<bool> {
        self.ws();
        // Function calls: `name(` or `name (`.
        let start = self.i;
        let mut j = self.i;
        while j < self.s.len() && self.s[j].is_ascii_alphabetic() {
            j += 1;
        }
        let fname = self.text(start, j);
        let mut k = j;
        while k < self.s.len() && self.s[k].is_ascii_whitespace() {
            k += 1;
        }
        if matches!(
            fname.as_str(),
            "defined" | "exists" | "make" | "empty" | "target" | "commands"
        ) && self.s.get(k) == Some(&b'(')
        {
            let open = k;
            let mut depth = 0usize;
            let mut close = None;
            for (n, &b) in self.s.iter().enumerate().skip(open) {
                match b {
                    b'(' | b'{' => depth += 1,
                    b')' | b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            close = Some(n);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let close = close.ok_or("unbalanced function call")?;
            let arg = self.text(open + 1, close);
            self.i = close + 1;
            let m = self.make;
            return Ok(match fname.as_str() {
                "defined" => m.defined(&m.expand(arg.trim())?),
                "exists" => {
                    let a = m.expand(arg.trim())?;
                    let pth = Path::new(&a);
                    if pth.is_absolute() {
                        pth.exists()
                    } else {
                        m.curdir.join(pth).exists() || m.path.iter().any(|d| d.join(pth).exists())
                    }
                }
                "make" | "commands" => false,
                "empty" => m.expand(&format!("${{{}}}", arg.trim()))?.trim().is_empty(),
                _ => {
                    let t = m.expand(arg.trim())?;
                    m.rules.iter().any(|r| r.targets.contains(&t))
                }
            });
        }

        let lhs = self.operand()?;
        self.ws();
        let ops = ["==", "!=", "<=", ">=", "<", ">"];
        let op = ops
            .iter()
            .find(|o| self.s[self.i..].starts_with(o.as_bytes()))
            .copied();
        let Some(op) = op else {
            return Ok(match lhs.trim().parse::<f64>() {
                Ok(n) => n != 0.0,
                Err(_) => !lhs.trim().is_empty(),
            });
        };
        self.i += op.len();
        let rhs = self.operand()?;
        let (l, r) = (lhs.trim(), rhs.trim());
        if let (Ok(a), Ok(b)) = (l.parse::<f64>(), r.parse::<f64>()) {
            return Ok(match op {
                "==" => a == b,
                "!=" => a != b,
                "<=" => a <= b,
                ">=" => a >= b,
                "<" => a < b,
                _ => a > b,
            });
        }
        match op {
            "==" => Ok(l == r),
            "!=" => Ok(l != r),
            _ => Err(format!("`{op}` needs numbers, got `{l}` and `{r}`").into()),
        }
    }

    fn operand(&mut self) -> Result<String> {
        self.ws();
        let start = self.i;
        match self.s.get(self.i) {
            Some(b'$') => {
                let rest = self.text(self.i + 1, self.s.len());
                if rest.starts_with('{') || rest.starts_with('(') {
                    let close = matching_close(&rest).ok_or("unbalanced variable reference")?;
                    self.i += 1 + close + 1;
                } else {
                    self.i += 2;
                }
                self.make.expand(&self.text(start, self.i))
            }
            Some(b'"') => {
                let mut j = self.i + 1;
                while j < self.s.len() && self.s[j] != b'"' {
                    if self.s[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
                if j >= self.s.len() {
                    return Err("unterminated string".into());
                }
                self.i = j + 1;
                self.make.expand(&self.text(start + 1, j))
            }
            _ => {
                let mut j = self.i;
                while j < self.s.len() && !b" \t=!<>()&|".contains(&self.s[j]) {
                    j += 1;
                }
                if j == start {
                    return Err("expected an operand".into());
                }
                self.i = j;
                self.make.expand(&self.text(start, j))
            }
        }
    }
}

// --- lexical helpers ------------------------------------------------------------------------

/// Physical lines joined at backslash-newline, with the starting line number. The
/// continuation and the next line's leading whitespace become one space, as make does.
/// Whether the existing file `p`'s last component is spelled as on disk (`realpath(3)` gives
/// the on-disk spelling on case-insensitive file systems). A symbolic link counts as its own
/// name when it points at another one (`SupportAtomic.cpp` -> `Atomic.cpp`).
fn exact_case(p: &Path) -> bool {
    let Some(name) = p.file_name() else {
        return false;
    };
    if fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink()) {
        // A link's own entry: compare against the directory's listing.
        return p
            .parent()
            .and_then(|d| fs::read_dir(d).ok())
            .is_some_and(|entries| entries.flatten().any(|e| e.file_name() == name));
    }
    fs::canonicalize(p).is_ok_and(|real| real.file_name() == Some(name))
}

fn logical_lines(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut cur: Option<(usize, String)> = None;
    for (n, line) in text.lines().enumerate() {
        let (start, mut acc) = match cur.take() {
            Some((s, a)) => {
                let mut a = a.trim_end().to_string();
                a.push(' ');
                a.push_str(line.trim_start());
                (s, a)
            }
            // A line that is a comment from its start (after blanks) ends at its newline,
            // backslash or not: OpenBSD's make skips it to the end of the physical line
            // (`skip_to_end_of_line`, usr.bin/make/lowparse.c). libclangASTMatchers's
            // Makefile comments out a `CPPFLAGS+= ... \` line that way (M14).
            None if line.trim_start_matches(' ').starts_with('#') => {
                out.push((n + 1, line.to_string()));
                continue;
            }
            None => (n + 1, line.to_string()),
        };
        let trailing = acc.bytes().rev().take_while(|&b| b == b'\\').count();
        if trailing % 2 == 1 {
            acc.pop();
            cur = Some((start, acc));
        } else {
            out.push((start, acc));
        }
    }
    if let Some(c) = cur {
        out.push(c);
    }
    out
}

/// The line without its comment; `\#` is a literal `#`.
fn strip_comment(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'#') {
            out.push('#');
            chars.next();
        } else if c == '#' {
            break;
        } else {
            out.push(c);
        }
    }
    out
}

/// `.if foo` → `("if", "foo")`; `.  include "x"` → `("include", "\"x\"")`.
fn directive(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix('.')?.trim_start();
    let end = rest
        .find(|c: char| !(c.is_ascii_lowercase() || c == '-'))
        .unwrap_or(rest.len());
    let kw = &rest[..end];
    const KEYWORDS: &[&str] = &[
        "include", "sinclude", "-include", "if", "ifdef", "ifndef", "ifmake", "ifnmake", "elif",
        "elifdef", "elifndef", "else", "endif", "for", "endfor", "undef", "error", "warning",
        "info", "export", "unexport", "poison",
    ];
    if !KEYWORDS.contains(&kw) {
        return None;
    }
    let arg = &rest[end..];
    // `.include` must be followed by whitespace or the end; `.PATH:` never reaches here.
    if !arg.is_empty() && !arg.starts_with(char::is_whitespace) && !arg.starts_with('(') {
        return None;
    }
    Some((kw, arg.trim()))
}

/// Index of the brace or parenthesis closing `s[0]`, nesting both kinds.
fn matching_close(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, b) in s.bytes().enumerate() {
        match b {
            b'{' | b'(' => depth += 1,
            b'}' | b')' => {
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

/// The first `needle` outside `${...}`/`$(...)`.
fn find_top_level(s: &str, needle: u8) -> Option<usize> {
    let b = s.as_bytes();
    let mut depth = 0usize;
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'$' && i + 1 < b.len() && (b[i + 1] == b'{' || b[i + 1] == b'(') {
            depth += 1;
            i += 2;
            continue;
        }
        if depth > 0 {
            if b[i] == b'}' || b[i] == b')' {
                depth -= 1;
            } else if b[i] == b'{' || b[i] == b'(' {
                depth += 1;
            }
        } else if b[i] == needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn is_upper(t: &str) -> bool {
    t[1..]
        .bytes()
        .all(|b| b.is_ascii_uppercase() || b == b'_' || b == b'.')
}

/// `dir/name.c` → `dir/name`.
fn strip_suffix_word(w: &str) -> &str {
    let base = w.rfind('/').map_or(0, |k| k + 1);
    match w[base..].rfind('.') {
        Some(k) => &w[..base + k],
        None => w,
    }
}

fn suffix_of(w: &str) -> &str {
    let base = w.rfind('/').map_or(0, |k| k + 1);
    match w[base..].rfind('.') {
        Some(k) => &w[base + k + 1..],
        None => "",
    }
}

/// Parses `/old/new/flags` (any delimiter); returns the pieces and the bytes consumed,
/// flags included (up to the next `:` or the end).
fn parse_subst(s: &str) -> Option<(String, String, String, usize)> {
    let mut chars = s.char_indices();
    let (_, delim) = chars.next()?;
    let mut parts = vec![String::new()];
    let mut end = None;
    let mut escaped = false;
    for (i, c) in chars {
        if escaped {
            parts.last_mut()?.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == delim {
            if parts.len() == 2 {
                end = Some(i + c.len_utf8());
                break;
            }
            parts.push(String::new());
        } else {
            parts.last_mut()?.push(c);
        }
    }
    let end = end?;
    let flag_end = s[end..].find(':').map_or(s.len(), |k| end + k);
    let flags = s[end..flag_end].to_string();
    let new = parts.pop()?;
    let old = parts.pop()?;
    Some((old, new, flags, flag_end))
}

/// `:S/old/new/[g]` on one word: `^` and `$` anchor, `&` in `new` is the match.
fn subst_word(w: &str, old: &str, new: &str, global: bool) -> String {
    let (anchor_start, old) = match old.strip_prefix('^') {
        Some(o) => (true, o),
        None => (false, old),
    };
    let (anchor_end, old) = match old.strip_suffix('$') {
        Some(o) if !o.ends_with('\\') => (true, o),
        _ => (false, old),
    };
    let repl = new.replace('&', old);
    match (anchor_start, anchor_end) {
        (true, true) => {
            if w == old {
                repl
            } else {
                w.to_string()
            }
        }
        (true, false) => match w.strip_prefix(old) {
            Some(r) => format!("{repl}{r}"),
            None => w.to_string(),
        },
        (false, true) => match w.strip_suffix(old) {
            Some(r) => format!("{r}{repl}"),
            None => w.to_string(),
        },
        (false, false) if old.is_empty() => w.to_string(),
        (false, false) if global => w.replace(old, &repl),
        (false, false) => w.replacen(old, &repl, 1),
    }
}

/// System V `old=new` on one word, with `%` as the stem.
fn sysv_word(w: &str, old: &str, new: &str) -> String {
    if let Some((pre, post)) = old.split_once('%') {
        if w.len() >= pre.len() + post.len() && w.starts_with(pre) && w.ends_with(post) {
            let stem = &w[pre.len()..w.len() - post.len()];
            return match new.split_once('%') {
                Some((a, b)) => format!("{a}{stem}{b}"),
                None => new.to_string(),
            };
        }
        return w.to_string();
    }
    match w.strip_suffix(old) {
        Some(r) => format!("{r}{new}"),
        None => w.to_string(),
    }
}

/// Shell-style glob: `*`, `?`, `[...]` (ranges, `!`/`^` negation), `\` escapes.
pub fn glob(p: &[u8], s: &[u8]) -> bool {
    match p.first() {
        None => s.is_empty(),
        Some(b'*') => (0..=s.len()).any(|k| glob(&p[1..], &s[k..])),
        Some(b'?') => !s.is_empty() && glob(&p[1..], &s[1..]),
        Some(b'[') => {
            let Some(&c) = s.first() else {
                return false;
            };
            let mut i = 1;
            let negate = matches!(p.get(1), Some(b'!') | Some(b'^'));
            if negate {
                i += 1;
            }
            let mut matched = false;
            let mut first = true;
            while i < p.len() && (first || p[i] != b']') {
                first = false;
                if i + 2 < p.len() && p[i + 1] == b'-' && p[i + 2] != b']' {
                    if p[i] <= c && c <= p[i + 2] {
                        matched = true;
                    }
                    i += 3;
                } else {
                    if p[i] == c {
                        matched = true;
                    }
                    i += 1;
                }
            }
            if i >= p.len() {
                return false;
            }
            matched != negate && glob(&p[i + 1..], &s[1..])
        }
        Some(b'\\') if p.len() > 1 => s.first() == Some(&p[1]) && glob(&p[2..], &s[1..]),
        Some(&c) => s.first() == Some(&c) && glob(&p[1..], &s[1..]),
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    fn make_from(text: &str) -> Make {
        let mut m = Make::new(
            Path::new("/nonexistent"),
            &[("MACHINE_CPU", "amd64".to_string())],
            &[("bsd.own.mk", "YP?= yes\n")],
        );
        let mut p = Parser {
            conds: Vec::new(),
            open_rule: None,
        };
        m.parse_text(text, "test.mk", None, &mut p).unwrap();
        assert!(p.conds.is_empty());
        m
    }

    #[test]
    fn assignments_are_lazy_except_colon_equals() {
        let m = make_from("A= x ${B}\nB= one\nC:= ${B}\nB= two\nD+= d1\nD+= d2\nB?= three\n");
        assert_eq!(m.var("A").unwrap(), "x two");
        assert_eq!(m.var("C").unwrap(), "one");
        assert_eq!(m.var("D").unwrap(), "d1 d2");
    }

    #[test]
    fn modifiers() {
        let m = make_from(
            "L= a.o b.o c.c\nCANCEL= read write\nCF= -Ifoo -DX -O2 -include n.h -pipe\n\
         V= Yes\nP= ___realpath _exit\n",
        );
        assert_eq!(m.expand("${L:.o=.po}").unwrap(), "a.po b.po c.c");
        assert_eq!(m.expand("${CANCEL:%=w_%.c}").unwrap(), "w_read.c w_write.c");
        assert_eq!(m.expand("${CANCEL:=.o}").unwrap(), "read.o write.o");
        assert_eq!(m.expand("${L:N*.c:R}").unwrap(), "a b");
        assert_eq!(m.expand("${L:M*.c}").unwrap(), "c.c");
        assert_eq!(m.expand("${CF:M-[ID]*}").unwrap(), "-Ifoo -DX");
        assert_eq!(m.expand("${V:L}").unwrap(), "yes");
        assert_eq!(m.expand("${P:S/^_//}").unwrap(), "__realpath exit");
        assert_eq!(m.expand("${L:R:S/$/.o/}").unwrap(), "a.o b.o c.o");
        assert_eq!(m.expand("${SRCS_${MACHINE_CPU}}x$$y").unwrap(), "x$y");
    }

    #[test]
    fn conditionals_and_for() {
        let m = make_from(
            ".include <bsd.own.mk>\n\
         .if (${YP:L} == \"yes\")\nA= yp\n.else\nA= noyp\n.endif\n\
         .if ${MACHINE_CPU} == \"i386\" || ${MACHINE_CPU} == \"arm\"\nB= 32\n\
         .elif ${MACHINE_CPU} == \"amd64\"\nB= 64\n.else\nB= other\n.endif\n\
         .ifndef UNDEF\nC= c\n.endif\n.if !defined(UNDEF) && !empty(A)\nD= d\n.endif\n\
         ASM= a.o b.o c.o\nOVR= b.S c.S\n.for i in ${OVR}\nASM:= ${ASM:N${i:R}.o}\n.endfor\n",
        );
        assert_eq!(m.var("A").unwrap(), "yp");
        assert_eq!(m.var("B").unwrap(), "64");
        assert_eq!(m.var("C").unwrap(), "c");
        assert_eq!(m.var("D").unwrap(), "d");
        assert_eq!(m.var("ASM").unwrap(), "a.o");
    }

    #[test]
    fn for_with_two_variables_and_dependency_only_rules() {
        let m = make_from(
            "SSLASM= aes aes-x86_64 bn x86_64-mont\n\
         .for dir f in ${SSLASM}\nSRCS+= ${dir}/${f}.S\n${f}.S: ${dir}/asm/${f}.pl\n\
         \tperl ./asm/${f}.pl > ${.TARGET}\n.endfor\n\
         includes: prereq\nprereq: obj_mac.h\n",
        );
        assert_eq!(m.var("SRCS").unwrap(), "aes/aes-x86_64.S bn/x86_64-mont.S");
        assert_eq!(m.sources_of("x86_64-mont.S"), ["bn/asm/x86_64-mont.pl"]);
        assert_eq!(m.sources_of("includes"), ["prereq"]);
        assert_eq!(m.sources_of("prereq"), ["obj_mac.h"]);
        assert!(m.rule_for("prereq").is_none());
        let mut p = Parser {
            conds: Vec::new(),
            open_rule: None,
        };
        let mut odd = make_from("");
        assert!(
            odd.parse_text(
                "L= a b c\n.for x y in ${L}\n.endfor\n",
                "t.mk",
                None,
                &mut p
            )
            .is_err()
        );
    }

    #[test]
    fn rules_paths_and_continuations() {
        let m = make_from(
            "SRCS+= a.c \\\n\tb.c\nGEN=\\t.file \"${@:R}.S\"\\n\\#include \"SYS.h\" # comment\n\
         .PATH: /x/y\nOBJ= a.o b.o\n${OBJ}: ; @echo ${GEN}\n\nh.c: helper.c\n\tsed -e 's/A/B/' $> > $@\n",
        );
        assert_eq!(m.var("SRCS").unwrap(), "a.c b.c");
        let mut locals = Locals::new();
        locals.insert("@".into(), "access.o".into());
        assert_eq!(
            m.expand_local("${GEN}", &locals).unwrap(),
            "\\t.file \"access.S\"\\n#include \"SYS.h\""
        );
        assert_eq!(m.path, vec![PathBuf::from("/x/y")]);
        assert_eq!(m.rule_for("b.o").unwrap().commands, vec!["@echo ${GEN}"]);
        let r = m.rule_for("h.c").unwrap();
        assert_eq!(r.sources, vec!["helper.c"]);
        assert_eq!(r.commands, vec!["sed -e 's/A/B/' $> > $@"]);
    }

    #[test]
    fn unknown_constructs_fail_loudly() {
        let mut m = Make::new(Path::new("/nonexistent"), &[], &[]);
        let mut p = Parser {
            conds: Vec::new(),
            open_rule: None,
        };
        assert!(m.parse_text(".undef X\n", "t", None, &mut p).is_err());
        assert!(m.parse_text("X!= ls\n", "t", None, &mut p).is_err());
        assert!(
            m.parse_text("Y= ${X:Q}\nZ:= ${Y}\n", "t", None, &mut p)
                .is_err()
        );
        assert!(
            m.parse_text(".include <bsd.prog.mk>\n", "t", None, &mut p)
                .is_err()
        );
    }

    #[test]
    fn glob_matches_like_make() {
        assert!(glob(b"-[ID]*", b"-I/usr/include"));
        assert!(!glob(b"-[ID]*", b"-include"));
        assert!(glob(b"*.h", b"x.h"));
        assert!(glob(b"?x[!a-c]", b"yxd"));
        assert!(!glob(b"?x[!a-c]", b"yxb"));
    }

    /// `.PATH` lookups match the file name exactly, even where the file system ignores case.
    #[test]
    fn search_is_case_exact() {
        let dir = std::env::temp_dir().join(format!("emibsd-bsdmake-case-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("a")).unwrap();
        fs::create_dir_all(dir.join("b")).unwrap();
        fs::write(dir.join("a/DwarfUnit.cpp"), "").unwrap();
        fs::write(dir.join("b/DWARFUnit.cpp"), "").unwrap();
        std::os::unix::fs::symlink(dir.join("a/DwarfUnit.cpp"), dir.join("b/Link.cpp")).unwrap();
        let mut m = Make::new(&dir, &[], &[]);
        m.add_path(&dir.join("a"));
        m.add_path(&dir.join("b"));
        assert_eq!(m.search("DWARFUnit.cpp"), Some(dir.join("b/DWARFUnit.cpp")));
        assert_eq!(m.search("DwarfUnit.cpp"), Some(dir.join("a/DwarfUnit.cpp")));
        assert_eq!(m.search("Link.cpp"), Some(dir.join("b/Link.cpp")));
        assert_eq!(m.search("dwarfunit.cpp"), None);
        let _ = fs::remove_dir_all(&dir);
    }

    /// A whole-line comment ends at its newline even after a backslash, as in OpenBSD's make
    /// (libclangASTMatchers's Makefile); a backslash after an assignment still continues it.
    #[test]
    fn comment_lines_do_not_continue() {
        let m = make_from(
            "#CPPFLAGS+=\t-Ifoo \\\nCPPFLAGS+=\t-Ibar\n  # indented \\\nA= a \\\n  b # c \\\nB= x\n",
        );
        assert_eq!(m.var("CPPFLAGS").unwrap(), "-Ibar");
        assert_eq!(m.var("A").unwrap(), "a b");
        assert!(!m.defined("B"));
    }
}
/* </TESTS> */
