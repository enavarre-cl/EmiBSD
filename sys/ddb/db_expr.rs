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
//! Expression evaluator of the debugger: `ddb/db_expr.c`.
//!
//! Upstream: sys/ddb/db_expr.c @ 3ce1f3f79392
//!
//! The grammar, loosest binding first:
//!
//! ```text
//! expression := shift
//! shift      := add  { ("<<" | ">>") add }
//! add        := mult { ("+" | "-") mult }
//! mult       := unary { ("*" | "/" | "%" | "#") term }
//! unary      := "-" unary | "*" unary | term
//! term       := identifier | number | "." | ".." | "+" | "\"" | "$" variable | "(" expression ")"
//! ```
//!
//! `.` is `db_dot`, `..` `db_prev`, `+` `db_next`, `"` `db_last_addr`; `*` reads the word at
//! an address, `#` rounds up to a multiple, `>>` shifts the low 32 bits as unsigned.
//!
//! ## Deviations
//! - Each rule returns `DbResult<Option<DbExpr>>`: `None` is the C's 0 ("no expression
//!   here"), `Err` a `db_error`.
//! - Identifiers go through `db_symbol_by_name` (`db_sym.c`, not ported): a visible stub that
//!   finds nothing, so a symbol is "Symbol not found". `*` reads memory through
//!   `db_get_value` (`db_access.c`, not ported), a visible stub that fails the command.
//! - The arithmetic wraps, as C's does on these machines; shifts by more than the width of
//!   the operand (undefined in C) give 0.

use core::sync::atomic::Ordering;

use crate::ddb::db_command::{
    DB_DOT, DB_LAST_ADDR, DB_NEXT, DB_PREV, DbResult, db_error, db_get_value,
};
use crate::ddb::db_lex::{
    db_read_token, db_tok_number, db_tok_string, db_unread_token, tDITTO, tDOLLAR, tDOT, tDOTDOT,
    tHASH, tIDENT, tLPAREN, tMINUS, tNUMBER, tPCT, tPLUS, tRPAREN, tSHIFT_L, tSHIFT_R, tSLASH,
    tSTAR,
};
use crate::ddb::db_variables::db_get_variable;
use crate::machine::db_machdep::DbExpr;
use crate::unported;

/// `db_symbol_by_name` (`db_sym.c`): the value of the symbol `name`. The symbol table is not
/// ported: finds nothing.
fn db_symbol_by_name(_name: &[u8]) -> Option<DbExpr> {
    let _ = unported!("db_symbol_by_name (db_sym.c)");
    None
}

/// `db_term`: an identifier, a number, one of the address variables, `$variable` or a
/// parenthesised expression.
fn db_term() -> DbResult<Option<DbExpr>> {
    let t = db_read_token()?;
    if t == tIDENT {
        let Some(value) = db_symbol_by_name(db_tok_string().as_bytes()) else {
            return Err(db_error(Some("Symbol not found\n")));
        };
        return Ok(Some(value));
    }
    if t == tNUMBER {
        return Ok(Some(db_tok_number()));
    }
    if t == tDOT {
        return Ok(Some(DB_DOT.load(Ordering::Relaxed) as DbExpr));
    }
    if t == tDOTDOT {
        return Ok(Some(DB_PREV.load(Ordering::Relaxed) as DbExpr));
    }
    if t == tPLUS {
        return Ok(Some(DB_NEXT.load(Ordering::Relaxed) as DbExpr));
    }
    if t == tDITTO {
        return Ok(Some(DB_LAST_ADDR.load(Ordering::Relaxed) as DbExpr));
    }
    if t == tDOLLAR {
        return db_get_variable().map(Some);
    }
    if t == tLPAREN {
        let Some(value) = db_expression()? else {
            return Err(db_error(Some("Syntax error\n")));
        };
        if db_read_token()? != tRPAREN {
            return Err(db_error(Some("Syntax error\n")));
        }
        return Ok(Some(value));
    }
    db_unread_token(t);
    Ok(None)
}

/// `db_unary`: unary minus and indirection.
fn db_unary() -> DbResult<Option<DbExpr>> {
    let t = db_read_token()?;
    if t == tMINUS {
        let Some(value) = db_unary()? else {
            return Err(db_error(Some("Syntax error\n")));
        };
        return Ok(Some(value.wrapping_neg()));
    }
    if t == tSTAR {
        // indirection
        let Some(value) = db_unary()? else {
            return Err(db_error(Some("Syntax error\n")));
        };
        return db_get_value(value as usize, size_of::<usize>(), false).map(Some);
    }
    db_unread_token(t);
    db_term()
}

/// `db_mult_expr`: `*`, `/`, `%` and `#` (round up to a multiple).
fn db_mult_expr() -> DbResult<Option<DbExpr>> {
    let Some(mut lhs) = db_unary()? else {
        return Ok(None);
    };

    let mut t = db_read_token()?;
    while t == tSTAR || t == tSLASH || t == tPCT || t == tHASH {
        let Some(rhs) = db_term()? else {
            return Err(db_error(Some("Syntax error\n")));
        };
        if t == tSTAR {
            lhs = lhs.wrapping_mul(rhs);
        } else {
            if rhs == 0 {
                return Err(db_error(Some("Divide by 0\n")));
            }
            if t == tSLASH {
                lhs = lhs.wrapping_div(rhs);
            } else if t == tPCT {
                lhs = lhs.wrapping_rem(rhs);
            } else {
                lhs = lhs
                    .wrapping_add(rhs)
                    .wrapping_sub(1)
                    .wrapping_div(rhs)
                    .wrapping_mul(rhs);
            }
        }
        t = db_read_token()?;
    }
    db_unread_token(t);
    Ok(Some(lhs))
}

/// `db_add_expr`: `+` and `-`.
fn db_add_expr() -> DbResult<Option<DbExpr>> {
    let Some(mut lhs) = db_mult_expr()? else {
        return Ok(None);
    };

    let mut t = db_read_token()?;
    while t == tPLUS || t == tMINUS {
        let Some(rhs) = db_mult_expr()? else {
            return Err(db_error(Some("Syntax error\n")));
        };
        if t == tPLUS {
            lhs = lhs.wrapping_add(rhs);
        } else {
            lhs = lhs.wrapping_sub(rhs);
        }
        t = db_read_token()?;
    }
    db_unread_token(t);
    Ok(Some(lhs))
}

/// `db_shift_expr`: `<<` and `>>`.
fn db_shift_expr() -> DbResult<Option<DbExpr>> {
    let Some(mut lhs) = db_add_expr()? else {
        return Ok(None);
    };

    let mut t = db_read_token()?;
    while t == tSHIFT_L || t == tSHIFT_R {
        let Some(rhs) = db_add_expr()? else {
            return Err(db_error(Some("Syntax error\n")));
        };
        if rhs < 0 {
            return Err(db_error(Some("Negative shift amount\n")));
        }
        let amount = u32::try_from(rhs).unwrap_or(u32::MAX);
        if t == tSHIFT_L {
            lhs = lhs.checked_shl(amount).unwrap_or(0);
        } else {
            // Shift right is unsigned (of the low 32 bits: C's `(unsigned) lhs`)
            lhs = DbExpr::from((lhs as u32).checked_shr(amount).unwrap_or(0));
        }
        t = db_read_token()?;
    }
    db_unread_token(t);
    Ok(Some(lhs))
}

/// `db_expression`: reads an expression; `None` if the next token does not start one.
pub fn db_expression() -> DbResult<Option<DbExpr>> {
    db_shift_expr()
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use core::sync::atomic::Ordering;

    use super::*;
    use crate::ddb::db_lex::{db_set_line, db_test_lock, tEOL};
    use crate::ddb::db_output::DB_RADIX;

    /// Evaluates `line` in radix 16; the token after the expression must be the end of line.
    fn eval(line: &[u8]) -> DbResult<Option<DbExpr>> {
        DB_RADIX.store(16, Ordering::Relaxed);
        db_set_line(line);
        let r = db_expression();
        if r.is_ok() {
            assert_eq!(
                db_read_token().unwrap(),
                tEOL,
                "trailing tokens in {line:?}"
            );
        }
        r
    }

    #[test]
    fn precedence_and_parentheses() {
        let _g = db_test_lock();
        assert_eq!(eval(b"2+3*4\n"), Ok(Some(14)));
        assert_eq!(eval(b"(2+3)*4\n"), Ok(Some(20)));
        assert_eq!(eval(b"10-4-3\n"), Ok(Some(0x10 - 4 - 3)));
        assert_eq!(eval(b"1+2<<4\n"), Ok(Some(3 << 4)));
        assert_eq!(eval(b"0t100/0t7%3\n"), Ok(Some((100 / 7) % 3)));
        // '#' rounds up to a multiple
        assert_eq!(eval(b"0t13#0t8\n"), Ok(Some(16)));
        assert_eq!(eval(b"0t16#0t8\n"), Ok(Some(16)));
    }

    #[test]
    fn unary_minus_and_shifts() {
        let _g = db_test_lock();
        assert_eq!(eval(b"-5\n"), Ok(Some(-5)));
        assert_eq!(eval(b"--5\n"), Ok(Some(5)));
        assert_eq!(eval(b"-3*2\n"), Ok(Some(-6)));
        // the right operand of '*' is a term, as in C: no unary minus there
        assert!(eval(b"3*-2\n").is_err());
        // >> is unsigned, of the low 32 bits
        assert_eq!(eval(b"-1>>0t28\n"), Ok(Some(0xf)));
        assert_eq!(eval(b"1<<0t40\n"), Ok(Some(1 << 40)));
        assert!(eval(b"1<<-1\n").is_err());
    }

    #[test]
    fn terms() {
        let _g = db_test_lock();
        crate::ddb::db_command::DB_DOT.store(0x1000, Ordering::Relaxed);
        crate::ddb::db_command::DB_NEXT.store(0x2000, Ordering::Relaxed);
        assert_eq!(eval(b".+10\n"), Ok(Some(0x1010)));
        assert_eq!(eval(b"+\n"), Ok(Some(0x2000)));
        assert_eq!(eval(b"$radix\n"), Ok(Some(16)));
        assert_eq!(eval(b"\n"), Ok(None));
        assert!(eval(b"nosuchsymbol\n").is_err());
        assert!(eval(b"4/0\n").is_err());
        assert!(eval(b"(1+2\n").is_err());
        assert!(eval(b"1+\n").is_err());
        // indirection needs db_get_value, which is not ported
        assert!(eval(b"*1000\n").is_err());
    }
}
/* </TESTS> */
