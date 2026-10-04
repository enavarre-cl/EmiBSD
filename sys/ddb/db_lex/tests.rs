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
