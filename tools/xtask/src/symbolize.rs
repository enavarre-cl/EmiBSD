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
//! `xtask symbolize`: turns the addresses of a kernel stack trace into symbol names.
//!
//! `ddb`'s trace prints return addresses as numbers until the kernel carries its own symbol
//! table (`ddb_init`). This reads the ELF kernel's `.symtab`, demangles the Rust symbols and
//! rewrites every `0x...` word of stdin that falls inside a function as
//! `0x... <name+offset>`.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::Result;

/// A function symbol: start address, size and demangled name.
struct Symbol {
    size: u64,
    name: String,
}

/// The function symbols of an ELF64 little-endian image, keyed by address.
pub struct SymbolTable {
    syms: BTreeMap<u64, Symbol>,
}

impl SymbolTable {
    /// Reads `kernel`'s `.symtab`.
    pub fn load(kernel: &Path) -> Result<Self> {
        let data = fs::read(kernel).map_err(|e| format!("{}: {e}", kernel.display()))?;
        Self::parse(&data).map_err(|e| format!("{}: {e}", kernel.display()).into())
    }

    fn parse(data: &[u8]) -> std::result::Result<Self, String> {
        let u16_at = |o: usize| -> Result16 {
            data.get(o..o + 2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .ok_or("truncated ELF header")
        };
        let u32_at = |o: usize| -> std::result::Result<u32, &'static str> {
            data.get(o..o + 4)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .ok_or("truncated ELF")
        };
        let u64_at = |o: usize| -> std::result::Result<u64, &'static str> {
            data.get(o..o + 8)
                .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
                .ok_or("truncated ELF")
        };
        if data.get(..4) != Some(b"\x7fELF") || data.get(4) != Some(&2) || data.get(5) != Some(&1) {
            return Err("not a little-endian ELF64 file".into());
        }
        let shoff = u64_at(0x28)? as usize;
        let shentsize = u16_at(0x3a)? as usize;
        let shnum = u16_at(0x3c)? as usize;
        if shentsize < 64 {
            return Err("bad section header size".into());
        }
        let section = |i: usize| -> std::result::Result<(u32, u64, u64, u32), &'static str> {
            let o = shoff + i * shentsize;
            Ok((
                u32_at(o + 4)?,
                u64_at(o + 0x18)?,
                u64_at(o + 0x20)?,
                u32_at(o + 0x28)?,
            ))
        };
        let mut syms = BTreeMap::new();
        for i in 0..shnum {
            let (sh_type, offset, size, link) = section(i)?;
            if sh_type != 2 {
                continue; // SHT_SYMTAB
            }
            let (_, str_off, str_size, _) = section(link as usize)?;
            let strtab = data
                .get(str_off as usize..(str_off + str_size) as usize)
                .ok_or("truncated string table")?;
            let count = (size / 24) as usize;
            for n in 0..count {
                let o = offset as usize + n * 24;
                let st_name = u32_at(o)? as usize;
                let st_info = *data.get(o + 4).ok_or("truncated symbol table")?;
                let st_value = u64_at(o + 8)?;
                let st_size = u64_at(o + 16)?;
                if st_info & 0xf != 2 || st_value == 0 {
                    continue; // STT_FUNC only
                }
                let raw = strtab
                    .get(st_name..)
                    .and_then(|s| s.split(|&b| b == 0).next())
                    .ok_or("bad symbol name")?;
                let name = demangle(&String::from_utf8_lossy(raw));
                syms.insert(
                    st_value,
                    Symbol {
                        size: st_size,
                        name,
                    },
                );
            }
        }
        if syms.is_empty() {
            return Err("no function symbols (is the kernel stripped?)".into());
        }
        Ok(Self { syms })
    }

    /// The function containing `addr`, as `name+offset`, if any. A return address may sit one
    /// byte past its function's end (a `call` to a function that never returns was its last
    /// instruction), so the end itself still counts, as ddb's nearest-symbol search would.
    pub fn lookup(&self, addr: u64) -> Option<String> {
        let (&start, sym) = self.syms.range(..=addr).next_back()?;
        let off = addr - start;
        if sym.size != 0 && off > sym.size {
            return None;
        }
        Some(if off == 0 {
            sym.name.clone()
        } else {
            format!("{}+{off:#x}", sym.name)
        })
    }

    /// Rewrites every hexadecimal address in `line` that falls inside a function.
    pub fn annotate(&self, line: &str) -> String {
        let mut out = String::with_capacity(line.len());
        let mut rest = line;
        while let Some(i) = rest.find("0x") {
            let (head, tail) = rest.split_at(i);
            out.push_str(head);
            let hex: String = tail[2..]
                .chars()
                .take_while(|c| c.is_ascii_hexdigit())
                .collect();
            let token_len = 2 + hex.len();
            out.push_str(&tail[..token_len]);
            if let Some(addr) = u64::from_str_radix(&hex, 16)
                .ok()
                .filter(|_| hex.len() >= 8)
                && let Some(name) = self.lookup(addr)
            {
                out.push_str(" <");
                out.push_str(&name);
                out.push('>');
            }
            rest = &tail[token_len..];
        }
        out.push_str(rest);
        out
    }
}

type Result16 = std::result::Result<u16, &'static str>;

/// Demangles a Rust symbol: the v0 scheme (`_R...`, what rustc emits now) or the legacy one
/// (`_ZN...E`); anything else is returned as is.
pub fn demangle(sym: &str) -> String {
    if let Some(inner) = sym.strip_prefix("_R") {
        // A vendor-specific suffix (`.llvm.123`) is not part of the name.
        let inner = inner.split('.').next().unwrap_or(inner);
        return V0::demangle(inner).unwrap_or_else(|| sym.to_string());
    }
    demangle_legacy(sym)
}

/// Demangles a Rust legacy symbol (`_ZN...E`); anything else is returned as is.
fn demangle_legacy(sym: &str) -> String {
    let Some(mut rest) = sym.strip_prefix("_ZN") else {
        return sym.to_string();
    };
    let mut parts: Vec<String> = Vec::new();
    loop {
        if let Some(r) = rest.strip_prefix('E') {
            if !r.is_empty() && r != ".llvm" && !r.starts_with(".llvm.") {
                return sym.to_string();
            }
            break;
        }
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        let Ok(len) = digits.parse::<usize>() else {
            return sym.to_string();
        };
        rest = &rest[digits.len()..];
        let Some(ident) = rest.get(..len) else {
            return sym.to_string();
        };
        rest = &rest[len..];
        // The trailing hash segment: `17h<16 hex digits>`.
        if len == 17 && ident.starts_with('h') && ident[1..].chars().all(|c| c.is_ascii_hexdigit())
        {
            continue;
        }
        // A segment that would start with a digit or `$` gets a leading `_` in the mangling.
        let ident = ident.strip_prefix("_$").map_or(ident, |_| &ident[1..]);
        parts.push(unescape(ident));
    }
    parts.join("::")
}

/// A recursive-descent reader of the v0 mangling (RFC 2603): enough of the grammar to name
/// every function in the kernel, printed the way `rustc-demangle` prints it.
struct V0<'a> {
    sym: &'a [u8],
    pos: usize,
    depth: u32,
}

impl<'a> V0<'a> {
    fn demangle(inner: &'a str) -> Option<String> {
        let mut p = V0 {
            sym: inner.as_bytes(),
            pos: 0,
            depth: 0,
        };
        let mut out = String::new();
        p.path(&mut out, true)?;
        // An instantiating crate may follow; it is not part of the printed name.
        Some(out)
    }

    fn peek(&self) -> Option<u8> {
        self.sym.get(self.pos).copied()
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn next(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos += 1;
        Some(c)
    }

    /// `base-62-number`: `_` is 0, otherwise the digits plus one.
    fn integer_62(&mut self) -> Option<u64> {
        if self.eat(b'_') {
            return Some(0);
        }
        let mut x: u64 = 0;
        loop {
            let c = self.next()?;
            let d = match c {
                b'0'..=b'9' => u64::from(c - b'0'),
                b'a'..=b'z' => u64::from(c - b'a') + 10,
                b'A'..=b'Z' => u64::from(c - b'A') + 36,
                b'_' => return x.checked_add(1),
                _ => return None,
            };
            x = x.checked_mul(62)?.checked_add(d)?;
        }
    }

    /// `[disambiguator]`: `s` base-62-number, 0 when absent.
    fn disambiguator(&mut self) -> Option<u64> {
        if self.eat(b's') {
            self.integer_62()
        } else {
            Some(0)
        }
    }

    /// `undisambiguated-identifier`: `["u"] decimal ["_"] bytes`.
    fn ident(&mut self) -> Option<String> {
        let punycode = self.eat(b'u');
        let mut len: usize = 0;
        let mut digits = 0;
        while let Some(c @ b'0'..=b'9') = self.peek() {
            len = len.checked_mul(10)?.checked_add(usize::from(c - b'0'))?;
            self.pos += 1;
            digits += 1;
        }
        if digits == 0 {
            return None;
        }
        self.eat(b'_');
        let bytes = self.sym.get(self.pos..self.pos + len)?;
        self.pos += len;
        let text = String::from_utf8_lossy(bytes).into_owned();
        Some(if punycode {
            format!("punycode{{{text}}}")
        } else {
            text
        })
    }

    fn backref(&mut self) -> Option<V0<'a>> {
        let start = self.pos - 1;
        let i = self.integer_62()? as usize;
        if i >= start || self.depth > 64 {
            return None;
        }
        Some(V0 {
            sym: self.sym,
            pos: i,
            depth: self.depth + 1,
        })
    }

    /// `path`; `in_value` says whether generic arguments print as `::<...>`.
    fn path(&mut self, out: &mut String, in_value: bool) -> Option<()> {
        if self.depth > 64 {
            return None;
        }
        match self.next()? {
            b'C' => {
                let _dis = self.disambiguator()?;
                out.push_str(&self.ident()?);
            }
            b'N' => {
                let ns = self.next()?;
                self.path(out, in_value)?;
                let dis = self.disambiguator()?;
                let name = self.ident()?;
                match ns {
                    b'C' => {
                        out.push_str("::{closure");
                        if !name.is_empty() {
                            out.push(':');
                            out.push_str(&name);
                        }
                        out.push_str(&format!("#{dis}}}"));
                    }
                    b'S' => out.push_str(&format!("::{{shim:{name}#{dis}}}")),
                    c if c.is_ascii_uppercase() => {
                        out.push_str(&format!("::{{{}:{name}#{dis}}}", char::from(c)));
                    }
                    _ => {
                        out.push_str("::");
                        out.push_str(&name);
                    }
                }
            }
            b'M' => {
                let _dis = self.disambiguator()?;
                let mut scratch = String::new();
                self.path(&mut scratch, false)?; // the impl's own location: not printed
                out.push('<');
                self.ty(out)?;
                out.push('>');
            }
            b'X' => {
                let _dis = self.disambiguator()?;
                let mut scratch = String::new();
                self.path(&mut scratch, false)?;
                out.push('<');
                self.ty(out)?;
                out.push_str(" as ");
                self.path(out, false)?;
                out.push('>');
            }
            b'Y' => {
                out.push('<');
                self.ty(out)?;
                out.push_str(" as ");
                self.path(out, false)?;
                out.push('>');
            }
            b'I' => {
                self.path(out, in_value)?;
                out.push_str(if in_value { "::<" } else { "<" });
                let mut first = true;
                while !self.eat(b'E') {
                    if !first {
                        out.push_str(", ");
                    }
                    first = false;
                    self.generic_arg(out)?;
                }
                out.push('>');
            }
            b'B' => {
                let mut sub = self.backref()?;
                sub.path(out, in_value)?;
            }
            _ => return None,
        }
        Some(())
    }

    fn generic_arg(&mut self, out: &mut String) -> Option<()> {
        match self.peek()? {
            b'L' => {
                self.pos += 1;
                let _ = self.integer_62()?;
                out.push_str("'_");
            }
            b'K' => {
                self.pos += 1;
                self.konst(out)?;
            }
            _ => self.ty(out)?,
        }
        Some(())
    }

    fn konst(&mut self, out: &mut String) -> Option<()> {
        match self.next()? {
            b'p' => out.push('_'),
            b'B' => {
                let mut sub = self.backref()?;
                sub.konst(out)?;
            }
            c => {
                // A typed value: the type letter, then `["n"] hex "_"`.
                let _ = c;
                let neg = self.eat(b'n');
                let mut hex = String::new();
                while let Some(c) = self.next() {
                    if c == b'_' {
                        break;
                    }
                    hex.push(char::from(c));
                }
                match u128::from_str_radix(if hex.is_empty() { "0" } else { &hex }, 16) {
                    Ok(v) => out.push_str(&format!("{}{v}", if neg { "-" } else { "" })),
                    Err(_) => out.push_str("{const}"),
                }
            }
        }
        Some(())
    }

    fn ty(&mut self, out: &mut String) -> Option<()> {
        if self.depth > 64 {
            return None;
        }
        let basic = |c: u8| -> Option<&'static str> {
            Some(match c {
                b'a' => "i8",
                b'b' => "bool",
                b'c' => "char",
                b'd' => "f64",
                b'e' => "str",
                b'f' => "f32",
                b'h' => "u8",
                b'i' => "isize",
                b'j' => "usize",
                b'l' => "i32",
                b'm' => "u32",
                b'n' => "i128",
                b'o' => "u128",
                b's' => "i16",
                b't' => "u16",
                b'u' => "()",
                b'v' => "...",
                b'x' => "i64",
                b'y' => "u64",
                b'z' => "!",
                b'p' => "_",
                _ => return None,
            })
        };
        let c = self.peek()?;
        if let Some(name) = basic(c) {
            self.pos += 1;
            out.push_str(name);
            return Some(());
        }
        match c {
            b'A' => {
                self.pos += 1;
                out.push('[');
                self.ty(out)?;
                out.push_str("; ");
                self.konst(out)?;
                out.push(']');
            }
            b'S' => {
                self.pos += 1;
                out.push('[');
                self.ty(out)?;
                out.push(']');
            }
            b'T' => {
                self.pos += 1;
                out.push('(');
                let mut n = 0;
                while !self.eat(b'E') {
                    if n > 0 {
                        out.push_str(", ");
                    }
                    self.ty(out)?;
                    n += 1;
                }
                if n == 1 {
                    out.push(',');
                }
                out.push(')');
            }
            b'R' | b'Q' => {
                self.pos += 1;
                if self.eat(b'L') {
                    let _ = self.integer_62()?;
                }
                out.push_str(if c == b'R' { "&" } else { "&mut " });
                self.ty(out)?;
            }
            b'P' | b'O' => {
                self.pos += 1;
                out.push_str(if c == b'P' { "*const " } else { "*mut " });
                self.ty(out)?;
            }
            b'F' => {
                self.pos += 1;
                if self.eat(b'G') {
                    let _ = self.integer_62()?;
                }
                let _unsafe = self.eat(b'U');
                if self.eat(b'K') && !self.eat(b'C') {
                    let _ = self.ident()?;
                }
                out.push_str("fn(");
                let mut first = true;
                while !self.eat(b'E') {
                    if !first {
                        out.push_str(", ");
                    }
                    first = false;
                    self.ty(out)?;
                }
                out.push(')');
                let mut ret = String::new();
                self.ty(&mut ret)?;
                if ret != "()" {
                    out.push_str(" -> ");
                    out.push_str(&ret);
                }
            }
            b'D' => {
                self.pos += 1;
                if self.eat(b'G') {
                    let _ = self.integer_62()?;
                }
                out.push_str("dyn ");
                let mut first = true;
                while !self.eat(b'E') {
                    if !first {
                        out.push_str(" + ");
                    }
                    first = false;
                    self.path(out, false)?;
                    while self.eat(b'p') {
                        let name = self.ident()?;
                        out.push('<');
                        out.push_str(&name);
                        out.push_str(" = ");
                        self.ty(out)?;
                        out.push('>');
                    }
                }
                if self.eat(b'L') {
                    let _ = self.integer_62()?;
                }
            }
            b'B' => {
                self.pos += 1;
                let mut sub = self.backref()?;
                sub.ty(out)?;
            }
            _ => self.path(out, false)?,
        }
        Some(())
    }
}

/// The `$..$` escapes of the legacy mangling.
fn unescape(ident: &str) -> String {
    let mut out = String::new();
    let mut rest = ident;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("..") {
            out.push_str("::");
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix('$')
            && let Some(end) = r.find('$')
        {
            let code = &r[..end];
            let replacement = match code {
                "SP" => Some("@"),
                "BP" => Some("*"),
                "RF" => Some("&"),
                "LT" => Some("<"),
                "GT" => Some(">"),
                "LP" => Some("("),
                "RP" => Some(")"),
                "C" => Some(","),
                _ => None,
            };
            if let Some(s) = replacement {
                out.push_str(s);
                rest = &r[end + 1..];
                continue;
            }
            if let Some(hex) = code.strip_prefix('u')
                && let Ok(n) = u32::from_str_radix(hex, 16)
                && let Some(c) = char::from_u32(n)
            {
                out.push(c);
                rest = &r[end + 1..];
                continue;
            }
        }
        let c = rest.chars().next().unwrap_or('?');
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// Reads stdin, annotates it with `kernel`'s symbols and writes it to stdout.
pub fn symbolize(kernel: &Path) -> Result<()> {
    let table = SymbolTable::load(kernel)?;
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        writeln!(stdout, "{}", table.annotate(&line))?;
    }
    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demangles_v0_symbols() {
        assert_eq!(
            demangle("_RNvNtNtCs95cFkrpvQ9w_3bsd4kern8subr_prf5panic"),
            "bsd::kern::subr_prf::panic"
        );
        assert_eq!(
            demangle("_RNvMNtNtCs95cFkrpvQ9w_3bsd3sys5typesNtB2_5Vaddr3new"),
            "<bsd::sys::types::Vaddr>::new"
        );
        assert_eq!(
            demangle("_RNvMs0_NtNtCs95cFkrpvQ9w_3bsd7machine8bootinfoNtB5_8BootInfo9boothowto"),
            "<bsd::machine::bootinfo::BootInfo>::boothowto"
        );
        assert_eq!(
            demangle("_RNvCs1njKG4L9aB3_7___rustc17rust_begin_unwind"),
            "__rustc::rust_begin_unwind"
        );
        assert_eq!(
            demangle("_RNvNtCs9XUhdq2V6z1_4core3fmt5write.llvm.42"),
            "core::fmt::write"
        );
        assert_eq!(
            demangle("_RNvXs_NtCs9XUhdq2V6z1_4core3fmtRNtNtB4_3str3strNtB4_7Display3fmt"),
            "<&core::fmt::str::str as core::fmt::Display>::fmt"
        );
        assert_eq!(
            demangle("_RINvNtCs9XUhdq2V6z1_4core3ptr13drop_in_placeRhEB2_"),
            "core::ptr::drop_in_place::<&u8>"
        );
        assert_eq!(demangle("_RNCNvCs0_3bsd4main0"), "bsd::main::{closure#0}");
        assert_eq!(demangle("_Rbogus"), "_Rbogus");
    }

    #[test]
    fn demangles_legacy_symbols() {
        assert_eq!(
            demangle("_ZN3bsd4kern8subr_prf5panic17h0123456789abcdefE"),
            "bsd::kern::subr_prf::panic"
        );
        assert_eq!(
            demangle("_ZN4core3fmt5write17hdeadbeefdeadbeefE.llvm.123"),
            "core::fmt::write"
        );
        assert_eq!(
            demangle(
                "_ZN49_$LT$bsd..arch..amd64..Machine$u20$as$u20$Cpu$GT$4halt17h0000000000000000E"
            ),
            "<bsd::arch::amd64::Machine as Cpu>::halt"
        );
        assert_eq!(demangle("_start"), "_start");
        assert_eq!(demangle("_ZNbogus"), "_ZNbogus");
    }

    #[test]
    fn annotates_addresses_inside_functions() {
        let mut syms = BTreeMap::new();
        syms.insert(
            0xffff_ffff_8000_1000,
            Symbol {
                size: 0x40,
                name: "f".into(),
            },
        );
        syms.insert(
            0xffff_ffff_8000_2000,
            Symbol {
                size: 0,
                name: "g".into(),
            },
        );
        let t = SymbolTable { syms };
        assert_eq!(
            t.annotate("ret 0xffffffff80001010 at 0xffffffff80001000"),
            "ret 0xffffffff80001010 <f+0x10> at 0xffffffff80001000 <f>"
        );
        assert_eq!(
            t.annotate("0xffffffff80001040 past the end"),
            "0xffffffff80001040 <f+0x40> past the end"
        );
        assert_eq!(
            t.annotate("0xffffffff80001041 outside"),
            "0xffffffff80001041 outside"
        );
        assert_eq!(
            t.annotate("0xffffffff80002100 sizeless"),
            "0xffffffff80002100 <g+0x100> sizeless"
        );
        assert_eq!(t.annotate("0x10 short"), "0x10 short");
        assert_eq!(t.annotate("no addresses"), "no addresses");
    }
}
/* </TESTS> */
