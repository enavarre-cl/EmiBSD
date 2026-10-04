use std::vec::Vec;

use super::*;
use crate::ddb::db_lex::db_test_lock;

/// Runs `db_readline_from` on the keys `keys` with a buffer of `size` bytes; returns the line.
fn read(keys: &[u8], size: usize) -> Vec<u8> {
    let mut buf = [0u8; 120];
    let mut it = keys.iter();
    let n = db_readline_from(&mut buf[..size], || {
        it.next().map_or(i32::from(b'\n'), |&c| c.into())
    });
    buf[..n].to_vec()
}

#[test]
fn editing_keys() {
    let _g = db_test_lock();
    assert_eq!(read(b"trace\n", 120), b"trace\n");
    // ^H and DEL erase backwards
    assert_eq!(read(b"trax\x08ce\x7f\x7fce\r", 120), b"trace\r");
    // ^A, then insert at the start; ^E back at the end
    assert_eq!(read(b"ace\x01tr\x05!\n", 120), b"trace!\n");
    // ^B twice, ^D erases forwards, ^K to the end
    assert_eq!(read(b"abcd\x02\x02\x04\n", 120), b"abd\n");
    assert_eq!(read(b"abcd\x01\x06\x0b\n", 120), b"a\n");
    // ^W erases a word and the blanks after it, ^U the line, ^T twiddles
    assert_eq!(read(b"show all  \x17regs\n", 120), b"show regs\n");
    assert_eq!(read(b"junk\x15ok\n", 120), b"ok\n");
    assert_eq!(read(b"tarce\x02\x02\x14\n", 120), b"trace\n");
    // a full buffer rings the bell and keeps the room for the newline
    assert_eq!(read(b"abcdef\n", 4), b"abc\n");
}

#[test]
fn history() {
    let _g = db_test_lock();
    assert_eq!(read(b"first\n", 120), b"first\n");
    assert_eq!(read(b"second\n", 120), b"second\n");
    // ^P walks back (a repeated line is not saved again), ^N forward again
    assert_eq!(read(b"\x10\n", 120), b"second\n");
    assert_eq!(read(b"\x10\x10\n", 120), b"first\n");
    // the history is now first, second, first
    assert_eq!(read(b"\x10\x10\n", 120), b"second\n");
    // now first, second, first, second: back three, forward one
    assert_eq!(read(b"\x10\x10\x10\x0e\n", 120), b"first\n");
}
