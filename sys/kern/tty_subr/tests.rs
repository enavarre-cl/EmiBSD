//! Host tests for the clists: the ring wrapping, the quote bits, the cursors.

use super::*;
use std::boxed::Box;
use std::vec;
use std::vec::Vec;

/// A queue of `size` characters over leaked test memory, as `clalloc` would set it up.
pub(crate) fn test_clist(size: usize, quot: bool) -> &'static Clist {
    let clp: &'static Clist = Box::leak(Box::new(Clist::new()));
    let cs: &'static mut [u8] = vec![0u8; size].leak();
    clp.c_cs.set(cs.as_mut_ptr());
    if quot {
        let cq: &'static mut [u8] = vec![0u8; qmem(size)].leak();
        clp.c_cq.set(cq.as_mut_ptr());
    }
    clp.c_cn.set(size as i32);
    clp
}

fn drain(clp: &Clist) -> Vec<i32> {
    let mut out = Vec::new();
    loop {
        let c = getc(clp);
        if c == -1 {
            return out;
        }
        out.push(c);
    }
}

#[test]
fn putc_getc_wrap_around_and_keep_quotes() {
    let q = test_clist(4, true);
    assert_eq!(getc(q), -1);
    assert_eq!(putc(i32::from(b'a'), q), 0);
    assert_eq!(putc(i32::from(b'b') | TTY_QUOTE, q), 0);
    assert_eq!(getc(q), i32::from(b'a'));
    // Wrap: three more fit (one slot was freed).
    for c in [b'c', b'd', b'e'] {
        assert_eq!(putc(i32::from(c), q), 0);
    }
    assert_eq!(putc(i32::from(b'f'), q), -1, "full");
    assert_eq!(q.c_cc.get(), 4);
    assert_eq!(
        drain(q),
        [
            i32::from(b'b') | TTY_QUOTE,
            i32::from(b'c'),
            i32::from(b'd'),
            i32::from(b'e')
        ]
    );
    assert_eq!(q.c_cc.get(), 0);
}

#[test]
fn queues_without_quoting_drop_the_bit() {
    let q = test_clist(8, false);
    putc(i32::from(b'x') | TTY_QUOTE, q);
    assert_eq!(getc(q), i32::from(b'x'));
}

#[test]
fn b_to_q_and_q_to_b_move_what_fits() {
    let q = test_clist(8, true);
    assert_eq!(b_to_q(b"", q), 0);
    assert_eq!(b_to_q(b"hello", q), 0);
    let mut buf = [0u8; 3];
    assert_eq!(q_to_b(q, &mut buf), 3);
    assert_eq!(&buf, b"hel");
    // 2 left; 6 free, wrapping at the end of the ring.
    assert_eq!(b_to_q(b"0123456789", q), 4, "four did not fit");
    assert_eq!(q.c_cc.get(), 8);
    let mut all = [0u8; 16];
    assert_eq!(q_to_b(q, &mut all), 8);
    assert_eq!(&all[..8], b"lo012345");
    assert_eq!(q_to_b(q, &mut all), 0);
    // A quote bit written by putc is cleared by b_to_q over the same slot.
    putc(i32::from(b'q') | TTY_QUOTE, q);
    assert_eq!(getc(q), i32::from(b'q') | TTY_QUOTE);
}

#[test]
fn ndqb_counts_contiguous_characters() {
    let q = test_clist(8, true);
    assert_eq!(ndqb(q, 0), 0);
    b_to_q(b"abcdef", q);
    let mut buf = [0u8; 4];
    q_to_b(q, &mut buf); // c_cf = 4, "ef" left
    b_to_q(b"ghij", q); // wraps: "gh" at the end, "ij" at the start
    assert_eq!(q.c_cc.get(), 6);
    assert_eq!(ndqb(q, 0), 4, "up to the end of the ring");
    // Stop at a character with the bit set ('g' = 0x67 has 0x40).
    assert_eq!(ndqb(q, 0x40), 0, "'e' has 0x40 too");
    ndflush(q, 2);
    assert_eq!(ndqb(q, 0x80), 2, "stops at the end of the ring");
    assert_eq!(drain(q), [0x67, 0x68, 0x69, 0x6a]);
}

#[test]
fn ndqb_with_quote_tests_the_next_bit() {
    let q = test_clist(16, true);
    putc(i32::from(b'a'), q);
    putc(i32::from(b'b') | TTY_QUOTE, q);
    putc(i32::from(b'c'), q);
    // The C tests the quote bit after the character it counts: 'a' is counted only if 'b'
    // is not quoted.
    assert_eq!(ndqb(q, TTY_QUOTE), 0);
}

#[test]
fn unputc_and_ndflush() {
    let q = test_clist(4, true);
    assert_eq!(unputc(q), -1);
    b_to_q(b"abc", q);
    putc(i32::from(b'd') | TTY_QUOTE, q);
    assert_eq!(unputc(q), i32::from(b'd') | TTY_QUOTE);
    assert_eq!(unputc(q), i32::from(b'c'));
    getc(q);
    getc(q);
    // Empty again; the next putc starts over at the ring's start.
    b_to_q(b"xyz", q);
    ndflush(q, 1);
    assert_eq!(drain(q), [i32::from(b'y'), i32::from(b'z')]);
    b_to_q(b"12", q);
    ndflush(q, 2);
    assert_eq!(q.c_cc.get(), 0);
}

#[test]
fn firstc_nextc_walk_the_queue() {
    let q = test_clist(4, true);
    let (mut c, mut cc) = (0, 0);
    assert_eq!(firstc(q, &mut c, &mut cc), None);
    b_to_q(b"ab", q);
    getc(q);
    getc(q);
    b_to_q(b"cd", q);
    putc(i32::from(b'e') | TTY_QUOTE, q); // wraps to slot 0
    let mut seen = Vec::new();
    let mut cp = firstc(q, &mut c, &mut cc);
    while cp.is_some() {
        seen.push(c);
        cp = nextc(q, cp, &mut c, &mut cc);
    }
    assert_eq!(
        seen,
        [
            i32::from(b'c'),
            i32::from(b'd'),
            i32::from(b'e') | TTY_QUOTE
        ]
    );
    assert_eq!(q.c_cc.get(), 3, "walking does not consume");
}

#[test]
fn catq_swaps_or_copies() {
    let from = test_clist(8, true);
    let to = test_clist(8, true);
    b_to_q(b"abc", from);
    let from_ring = from.c_cs.get();
    catq(from, to);
    assert_eq!(to.c_cs.get(), from_ring, "same size, empty target: swapped");
    assert_eq!(from.c_cc.get(), 0);
    let small = test_clist(4, false);
    putc(i32::from(b'z'), small);
    catq(to, small);
    assert_eq!(drain(small), [i32::from(b'z'), 0x61, 0x62, 0x63]);
}

#[test]
fn clrbits_clears_exactly_the_range() {
    let map: Vec<Cell<u8>> = (0..4).map(|_| Cell::new(0xff)).collect();
    clrbits(&map, 3, 1);
    assert_eq!(map[0].get(), 0xf7);
    clrbits(&map, 4, 2);
    assert_eq!(map[0].get(), 0xc7);
    clrbits(&map, 6, 12);
    let bytes: Vec<u8> = map.iter().map(Cell::get).collect();
    assert_eq!(bytes, [0x07, 0x00, 0xfc, 0xff]);
    // Ending on a byte boundary must not touch the byte after.
    let map: Vec<Cell<u8>> = (0..2).map(|_| Cell::new(0xff)).collect();
    clrbits(&map, 4, 12);
    let bytes: Vec<u8> = map.iter().map(Cell::get).collect();
    assert_eq!(bytes, [0x0f, 0x00]);
}
