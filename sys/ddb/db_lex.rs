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

/*
 * Mach Operating System
 * Copyright (c) 1993,1992,1991,1990 Carnegie Mellon University
 * All Rights Reserved.
 *
 * Permission to use, copy, modify and distribute this software and its
 * documentation is hereby granted, provided that both the copyright
 * notice and this permission notice appear in all copies of the
 * software, derivative works or modified versions, and any portions
 * thereof, and that both notices appear in supporting documentation.
 *
 * CARNEGIE MELLON ALLOWS FREE USE OF THIS SOFTWARE IN ITS "AS IS"
 * CONDITION.  CARNEGIE MELLON DISCLAIMS ANY LIABILITY OF ANY KIND FOR
 * ANY DAMAGES WHATSOEVER RESULTING FROM THE USE OF THIS SOFTWARE.
 *
 * Carnegie Mellon requests users of this software to return to
 *
 *  Software Distribution Coordinator  or  Software.Distribution@CS.CMU.EDU
 *  School of Computer Science
 *  Carnegie Mellon University
 *  Pittsburgh PA 15213-3890
 *
 * any improvements or extensions that they make and grant Carnegie Mellon
 * the rights to redistribute these changes.
 *
 *	Author: David B. Golub, Carnegie Mellon University
 *	Date:	7/90
 */
/* </LICENSES> */

/* <CODE> */
//! Lexical analyzer of the debugger's command language: `ddb/db_lex.c` and `<ddb/db_lex.h>`.
//!
//! Upstream: sys/ddb/db_lex.c @ 3ce1f3f79392
//! Upstream: sys/ddb/db_lex.h @ 3ce1f3f79392
//!
//! `db_read_line` reads one line from the console (`db_readline`, `db_input.rs`) into
//! `db_line`; `db_read_token` cuts it into tokens, with one token and one character of
//! push-back (`db_unread_token`, `db_unread_char`). A number token leaves its value in
//! `db_tok_number`, an identifier its text in `db_tok_string`. Numbers are read in `db_radix`
//! unless they start with `0o`, `0t` or `0x` (octal, decimal, hexadecimal).
//!
//! ## Deviations
//! - The line, the cursors, the push-back and the token live in one [`StaticCell`]
//!   (`DB_LEX`) instead of six globals; ddb runs on one CPU at a time. `db_lp`/`db_endlp` are
//!   indices into the line.
//! - `db_tok_string` and `db_tok_number` are read through [`db_tok_string`] (a copy of the
//!   token, so no reference into the lexer outlives a call) and [`db_tok_number`].
//! - A function that reaches `db_error` returns [`DbResult`] (`docs/C_TO_RUST.md`, the
//!   `db_error`/`db_recover` row).
//! - Line bytes are unsigned, as `char` is on arm64; on amd64 C reads bytes above 0x7f as
//!   negative, which both treat as white space before a token and as its end inside one.

use libkern::StaticCell;

use crate::ddb::db_command::{DbResult, db_error};
use crate::ddb::db_input::db_readline;
use crate::ddb::db_output::DB_RADIX;
use crate::kern::subr_prf::db_printf;
use crate::machine::db_machdep::DbExpr;

/// The token values of `<ddb/db_lex.h>`, with the C's names.
#[allow(non_upper_case_globals)] // the C names
pub mod tokens {
    /// `tEOF`: end of input.
    pub const tEOF: i32 = -1;
    /// `tEOL`: end of line.
    pub const tEOL: i32 = 1;
    /// `tNUMBER`: a number, in `db_tok_number`.
    pub const tNUMBER: i32 = 2;
    /// `tIDENT`: an identifier, in `db_tok_string`.
    pub const tIDENT: i32 = 3;
    /// `tPLUS`: `+`.
    pub const tPLUS: i32 = 4;
    /// `tMINUS`: `-`.
    pub const tMINUS: i32 = 5;
    /// `tDOT`: `.`.
    pub const tDOT: i32 = 6;
    /// `tSTAR`: `*`.
    pub const tSTAR: i32 = 7;
    /// `tSLASH`: `/`.
    pub const tSLASH: i32 = 8;
    /// `tEQ`: `=`.
    pub const tEQ: i32 = 9;
    /// `tLPAREN`: `(`.
    pub const tLPAREN: i32 = 10;
    /// `tRPAREN`: `)`.
    pub const tRPAREN: i32 = 11;
    /// `tPCT`: `%`.
    pub const tPCT: i32 = 12;
    /// `tHASH`: `#`.
    pub const tHASH: i32 = 13;
    /// `tCOMMA`: `,`.
    pub const tCOMMA: i32 = 14;
    /// `tDITTO`: `"`.
    pub const tDITTO: i32 = 15;
    /// `tDOLLAR`: `$`.
    pub const tDOLLAR: i32 = 16;
    /// `tEXCL`: `!`.
    pub const tEXCL: i32 = 17;
    /// `tSHIFT_L`: `<<`.
    pub const tSHIFT_L: i32 = 18;
    /// `tSHIFT_R`: `>>`.
    pub const tSHIFT_R: i32 = 19;
    /// `tDOTDOT`: `..`.
    pub const tDOTDOT: i32 = 20;
}
pub use tokens::*;

/// `TOK_STRING_SIZE`: the size of `db_tok_string`.
pub const TOK_STRING_SIZE: usize = 120;

/// The size of `db_line`.
const DB_LINE_SIZE: usize = 120;

/// The lexer's state: `db_line`, `db_lp`, `db_endlp`, `db_look_char`, `db_look_token`,
/// `db_tok_number` and `db_tok_string`.
struct DbLex {
    /// `db_line`: the line being lexed.
    line: [u8; DB_LINE_SIZE],
    /// `db_lp`: the next character of `line`.
    lp: usize,
    /// `db_endlp`: the end of `line`.
    endlp: usize,
    /// `db_look_char`: a pushed-back character, 0 if none.
    look_char: i32,
    /// `db_look_token`: a pushed-back token, 0 if none.
    look_token: i32,
    /// `db_tok_number`: the value of the last `tNUMBER`.
    tok_number: DbExpr,
    /// `db_tok_string`: the text of the last `tIDENT`, NUL-terminated.
    tok_string: [u8; TOK_STRING_SIZE],
}

/// A copy of `db_tok_string`: the text of the last `tIDENT`.
#[derive(Clone, Copy)]
pub struct DbTokString {
    /// The bytes, NUL-terminated.
    buf: [u8; TOK_STRING_SIZE],
}

impl DbTokString {
    /// The identifier, without its NUL.
    pub fn as_bytes(&self) -> &[u8] {
        let len = self
            .buf
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.buf.len());
        &self.buf[..len]
    }
}

/// The lexer. Only the CPU in the debugger touches it (`db_active`; `ddb_mp_mutex` with
/// `MULTIPROCESSOR`), and every access is one short call of [`with_lex`].
static DB_LEX: StaticCell<DbLex> = StaticCell::new(DbLex {
    line: [0; DB_LINE_SIZE],
    lp: 0,
    endlp: 0,
    look_char: 0,
    look_token: 0,
    tok_number: 0,
    tok_string: [0; TOK_STRING_SIZE],
});

/// Runs `f` on the lexer's state. `f` must not call back into this module.
fn with_lex<R>(f: impl FnOnce(&mut DbLex) -> R) -> R {
    // SAFETY: ddb runs on one CPU at a time (see `DB_LEX`), and `f` never re-enters
    // `with_lex`, so this is the only reference to the state while it lives.
    f(unsafe { DB_LEX.get_mut() })
}

/// `db_read_line`: reads a line from the console into `db_line`. Returns its length with the
/// newline, 0 at end of input.
pub fn db_read_line() -> usize {
    // The console is read into a local line, so no reference to the lexer is live while
    // db_readline echoes through db_putchar.
    let mut line = [0u8; DB_LINE_SIZE];
    let i = db_readline(&mut line);
    if i == 0 {
        return 0; // EOI
    }
    with_lex(|l| {
        l.line = line;
        l.lp = 0;
        l.endlp = i;
    });
    i
}

/// `db_flush_line`: empties `db_line`.
fn db_flush_line() {
    with_lex(|l| {
        l.lp = 0;
        l.endlp = 0;
    });
}

/// `db_read_char`: the next character of the line, the pushed-back one first; -1 at its end.
fn db_read_char() -> i32 {
    with_lex(|l| {
        if l.look_char != 0 {
            let c = l.look_char;
            l.look_char = 0;
            c
        } else if l.lp >= l.endlp {
            -1
        } else {
            let c = i32::from(l.line[l.lp]);
            l.lp += 1;
            c
        }
    })
}

/// `db_unread_char`: pushes `c` back.
fn db_unread_char(c: i32) {
    with_lex(|l| l.look_char = c);
}

/// `db_unread_token`: pushes the token `t` back.
pub fn db_unread_token(t: i32) {
    with_lex(|l| l.look_token = t);
}

/// `db_read_token`: the next token, the pushed-back one first.
pub fn db_read_token() -> DbResult<i32> {
    let look = with_lex(|l| core::mem::replace(&mut l.look_token, 0));
    if look != 0 { Ok(look) } else { db_lex() }
}

/// `db_flush_lex`: drops the rest of the line and the push-back.
pub fn db_flush_lex() {
    db_flush_line();
    with_lex(|l| {
        l.look_char = 0;
        l.look_token = 0;
    });
}

/// `db_tok_number`: the value of the last `tNUMBER`.
pub fn db_tok_number() -> DbExpr {
    with_lex(|l| l.tok_number)
}

/// `db_tok_string`: the text of the last `tIDENT`.
pub fn db_tok_string() -> DbTokString {
    DbTokString {
        buf: with_lex(|l| l.tok_string),
    }
}

/// Whether `c` is `[A-Za-z]`.
fn is_alpha(c: i32) -> bool {
    (i32::from(b'A')..=i32::from(b'Z')).contains(&c)
        || (i32::from(b'a')..=i32::from(b'z')).contains(&c)
}

/// Whether `c` is `[0-9]`.
fn is_digit(c: i32) -> bool {
    (i32::from(b'0')..=i32::from(b'9')).contains(&c)
}

/// `db_lex`: reads the next token of the line.
pub fn db_lex() -> DbResult<i32> {
    let mut c = db_read_char();
    while c <= i32::from(b' ') || c > i32::from(b'~') {
        if c == i32::from(b'\n') || c == -1 {
            return Ok(tEOL);
        }
        c = db_read_char();
    }

    if is_digit(c) {
        // number
        let r: DbExpr;
        if c > i32::from(b'0') {
            r = DbExpr::from(DB_RADIX.load(core::sync::atomic::Ordering::Relaxed));
        } else {
            c = db_read_char();
            r = match u8::try_from(c) {
                Ok(b'O' | b'o') => 8,
                Ok(b'T' | b't') => 10,
                Ok(b'X' | b'x') => 16,
                _ => {
                    db_unread_char(c);
                    DbExpr::from(DB_RADIX.load(core::sync::atomic::Ordering::Relaxed))
                }
            };
            c = db_read_char();
        }
        let mut number: DbExpr = 0;
        loop {
            let digit = if c >= i32::from(b'0')
                && c <= if r == 8 {
                    i32::from(b'7')
                } else {
                    i32::from(b'9')
                } {
                c - i32::from(b'0')
            } else if r == 16 && (i32::from(b'a')..=i32::from(b'f')).contains(&c) {
                c - i32::from(b'a') + 10
            } else if r == 16 && (i32::from(b'A')..=i32::from(b'F')).contains(&c) {
                c - i32::from(b'A') + 10
            } else {
                break;
            };
            number = number.wrapping_mul(r).wrapping_add(DbExpr::from(digit));
            c = db_read_char();
        }
        with_lex(|l| l.tok_number = number);
        if is_digit(c) || is_alpha(c) || c == i32::from(b'_') {
            return Err(db_error(Some("Bad character in number\n")));
        }
        db_unread_char(c);
        return Ok(tNUMBER);
    }
    if is_alpha(c) || c == i32::from(b'_') || c == i32::from(b'\\') {
        // string
        let mut tok = [0u8; TOK_STRING_SIZE];
        let mut cp = 0;
        if c == i32::from(b'\\') {
            c = db_read_char();
            if c == i32::from(b'\n') || c == -1 {
                return Err(db_error(Some("Bad escape\n")));
            }
        }
        tok[cp] = c as u8;
        cp += 1;
        loop {
            c = db_read_char();
            if is_alpha(c)
                || is_digit(c)
                || c == i32::from(b'_')
                || c == i32::from(b'\\')
                || c == i32::from(b':')
            {
                if c == i32::from(b'\\') {
                    c = db_read_char();
                    if c == i32::from(b'\n') || c == -1 {
                        return Err(db_error(Some("Bad escape\n")));
                    }
                }
                tok[cp] = c as u8;
                cp += 1;
                if cp == TOK_STRING_SIZE {
                    return Err(db_error(Some("String too long\n")));
                }
            } else {
                tok[cp] = 0;
                break;
            }
        }
        with_lex(|l| l.tok_string = tok);
        db_unread_char(c);
        return Ok(tIDENT);
    }

    match c {
        -1 => return Ok(tEOF),
        _ => match c as u8 {
            b'+' => return Ok(tPLUS),
            b'-' => return Ok(tMINUS),
            b'.' => {
                c = db_read_char();
                if c == i32::from(b'.') {
                    return Ok(tDOTDOT);
                }
                db_unread_char(c);
                return Ok(tDOT);
            }
            b'*' => return Ok(tSTAR),
            b'/' => return Ok(tSLASH),
            b'=' => return Ok(tEQ),
            b'%' => return Ok(tPCT),
            b'#' => return Ok(tHASH),
            b'(' => return Ok(tLPAREN),
            b')' => return Ok(tRPAREN),
            b',' => return Ok(tCOMMA),
            b'"' => return Ok(tDITTO),
            b'$' => return Ok(tDOLLAR),
            b'!' => return Ok(tEXCL),
            b'<' => {
                c = db_read_char();
                if c == i32::from(b'<') {
                    return Ok(tSHIFT_L);
                }
                db_unread_char(c);
            }
            b'>' => {
                c = db_read_char();
                if c == i32::from(b'>') {
                    return Ok(tSHIFT_R);
                }
                db_unread_char(c);
            }
            _ => {}
        },
    }
    db_printf(format_args!("Bad character\n"));
    db_flush_lex();
    Ok(tEOF)
}

/// Test-only: the lock that serialises the tests sharing the lexer (and the debugger's other
/// globals). Hold it across [`db_set_line`] and the calls that read the line.
#[cfg(test)]
pub fn db_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Test-only: makes `line` the line the lexer reads, as if `db_read_line` had read it.
#[cfg(test)]
pub fn db_set_line(line: &[u8]) {
    with_lex(|l| {
        l.line = [0; DB_LINE_SIZE];
        l.line[..line.len()].copy_from_slice(line);
        l.lp = 0;
        l.endlp = line.len();
        l.look_char = 0;
        l.look_token = 0;
    });
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use core::sync::atomic::Ordering;
    use std::vec;
    use std::vec::Vec;

    use super::*;

    /// Every token of `line` up to `tEOL`, with the number or identifier each carried.
    fn lex_all(line: &[u8]) -> Vec<(i32, DbExpr, Vec<u8>)> {
        db_set_line(line);
        let mut out = Vec::new();
        loop {
            let t = db_read_token().unwrap();
            if t == tEOL || t == tEOF {
                out.push((t, 0, Vec::new()));
                return out;
            }
            let n = if t == tNUMBER { db_tok_number() } else { 0 };
            let s = if t == tIDENT {
                db_tok_string().as_bytes().to_vec()
            } else {
                Vec::new()
            };
            out.push((t, n, s));
        }
    }

    #[test]
    fn numbers_in_every_radix() {
        let _g = db_test_lock();
        DB_RADIX.store(16, Ordering::Relaxed);
        let toks = lex_all(b"10 0x1f 0t99 0o17 017 ff\n");
        assert_eq!(toks[0], (tNUMBER, 0x10, vec![]));
        assert_eq!(toks[1], (tNUMBER, 0x1f, vec![]));
        assert_eq!(toks[2], (tNUMBER, 99, vec![]));
        assert_eq!(toks[3], (tNUMBER, 0o17, vec![]));
        // a leading 0 without a radix letter keeps db_radix
        assert_eq!(toks[4], (tNUMBER, 0x17, vec![]));
        // in radix 16, "ff" starts with a letter: an identifier
        assert_eq!(toks[5], (tIDENT, 0, b"ff".to_vec()));
        assert_eq!(toks[6].0, tEOL);

        DB_RADIX.store(8, Ordering::Relaxed);
        assert_eq!(lex_all(b"17\n")[0], (tNUMBER, 0o17, vec![]));
        DB_RADIX.store(10, Ordering::Relaxed);
        assert_eq!(lex_all(b"17 0x10\n")[0], (tNUMBER, 17, vec![]));
        assert_eq!(lex_all(b"17 0x10\n")[1], (tNUMBER, 16, vec![]));
        DB_RADIX.store(16, Ordering::Relaxed);
    }

    #[test]
    fn bad_number_is_a_db_error() {
        let _g = db_test_lock();
        DB_RADIX.store(10, Ordering::Relaxed);
        db_set_line(b"12ab\n");
        assert!(db_read_token().is_err());
        // db_error flushed the line
        assert_eq!(db_read_token().unwrap(), tEOL);
        db_set_line(b"0o8\n");
        assert!(db_read_token().is_err());
        DB_RADIX.store(16, Ordering::Relaxed);
    }

    #[test]
    fn identifiers_and_escapes() {
        let _g = db_test_lock();
        let toks = lex_all(b"show all_procs foo:bar \\1x\n");
        assert_eq!(toks[0], (tIDENT, 0, b"show".to_vec()));
        assert_eq!(toks[1], (tIDENT, 0, b"all_procs".to_vec()));
        assert_eq!(toks[2], (tIDENT, 0, b"foo:bar".to_vec()));
        assert_eq!(toks[3], (tIDENT, 0, b"1x".to_vec()));
        db_set_line(b"a\\");
        assert!(db_read_token().is_err());
    }

    #[test]
    fn operators() {
        let _g = db_test_lock();
        let toks: Vec<i32> = lex_all(b"+ - . .. * / = % # ( ) , \" $ ! << >>\n")
            .into_iter()
            .map(|t| t.0)
            .collect();
        assert_eq!(
            toks,
            [
                tPLUS, tMINUS, tDOT, tDOTDOT, tSTAR, tSLASH, tEQ, tPCT, tHASH, tLPAREN, tRPAREN,
                tCOMMA, tDITTO, tDOLLAR, tEXCL, tSHIFT_L, tSHIFT_R, tEOL
            ]
        );
        // a lone '<' is a bad character: tEOF and the line is gone
        db_set_line(b"< 1\n");
        assert_eq!(db_read_token().unwrap(), tEOF);
        assert_eq!(db_read_token().unwrap(), tEOL);
    }

    #[test]
    fn push_back_and_flush() {
        let _g = db_test_lock();
        db_set_line(b"x 1\n");
        let t = db_read_token().unwrap();
        assert_eq!(t, tIDENT);
        db_unread_token(t);
        assert_eq!(db_read_token().unwrap(), tIDENT);
        db_flush_lex();
        assert_eq!(db_read_token().unwrap(), tEOL);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/ddb/db_lex.h");
        let ours: &[(&str, i64)] = &[
            ("TOK_STRING_SIZE", TOK_STRING_SIZE as i64),
            ("tEOF", tEOF.into()),
            ("tEOL", tEOL.into()),
            ("tNUMBER", tNUMBER.into()),
            ("tIDENT", tIDENT.into()),
            ("tPLUS", tPLUS.into()),
            ("tMINUS", tMINUS.into()),
            ("tDOT", tDOT.into()),
            ("tSTAR", tSTAR.into()),
            ("tSLASH", tSLASH.into()),
            ("tEQ", tEQ.into()),
            ("tLPAREN", tLPAREN.into()),
            ("tRPAREN", tRPAREN.into()),
            ("tPCT", tPCT.into()),
            ("tHASH", tHASH.into()),
            ("tCOMMA", tCOMMA.into()),
            ("tDITTO", tDITTO.into()),
            ("tDOLLAR", tDOLLAR.into()),
            ("tEXCL", tEXCL.into()),
            ("tSHIFT_L", tSHIFT_L.into()),
            ("tSHIFT_R", tSHIFT_R.into()),
            ("tDOTDOT", tDOTDOT.into()),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
