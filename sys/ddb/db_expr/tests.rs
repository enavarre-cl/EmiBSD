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
