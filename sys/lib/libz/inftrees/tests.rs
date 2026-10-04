//! Tests of `inflate_table`: the fixed tables of `inffixed.rs` rebuilt as `makefixed()` builds
//! them, and the corner cases of the code-length checks.

use super::*;
use crate::inffixed::{distfix, lenfix};
use std::vec;
use std::vec::Vec;

/// The canonical Huffman codes of `lens` (RFC 1951, section 3.2.2), bit-reversed as deflate
/// sends them (first bit in the low bit). `None` for unused symbols.
fn canonical_codes(lens: &[u16]) -> Vec<Option<u32>> {
    let mut bl_count = [0u32; MAXBITS + 1];
    for &l in lens {
        bl_count[l as usize] += 1;
    }
    bl_count[0] = 0;
    let mut next_code = [0u32; MAXBITS + 1];
    let mut code = 0;
    for bits in 1..=MAXBITS {
        code = (code + bl_count[bits - 1]) << 1;
        next_code[bits] = code;
    }
    lens.iter()
        .map(|&l| {
            if l == 0 {
                return None;
            }
            let c = next_code[l as usize];
            next_code[l as usize] += 1;
            Some(c.reverse_bits() >> (32 - l))
        })
        .collect()
}

/// Decode the code `code` (bit-reversed, `len` bits) through a table of `root` index bits, as
/// inflate does: a root entry, and a sub-table entry when the root entry is a link. Returns
/// the final entry and the total number of bits it consumed.
fn decode(table: &[Code], root: u32, code: u32) -> (Code, u32) {
    let here = table[(code & ((1 << root) - 1)) as usize];
    if here.op != 0 && here.op & 0xf0 == 0 {
        let sub = (code >> root) & ((1 << here.op) - 1);
        let there = table[here.val as usize + sub as usize];
        (there, root + u32::from(there.bits))
    } else {
        (here, u32::from(here.bits))
    }
}

#[test]
fn rebuilds_the_fixed_tables() {
    // buildtables(): the fixed literal/length code, then the fixed distance code
    let mut lens = [0u16; 320];
    lens[..144].fill(8);
    lens[144..256].fill(9);
    lens[256..280].fill(7);
    lens[280..288].fill(8);
    let mut work = [0u16; 288];
    let mut fixed = vec![Code::default(); 544];
    let mut next = 0;
    let mut bits = 9;
    assert_eq!(
        inflate_table(
            CodeType::LENS,
            &lens[..288],
            &mut fixed,
            &mut next,
            &mut bits,
            &mut work
        ),
        0
    );
    assert_eq!((next, bits), (512, 9));
    let distoff = next;
    lens[..32].fill(5);
    bits = 5;
    assert_eq!(
        inflate_table(
            CodeType::DISTS,
            &lens[..32],
            &mut fixed,
            &mut next,
            &mut bits,
            &mut work
        ),
        0
    );
    assert_eq!((next, bits), (544, 5));

    // makefixed() prints entries with (low & 127) == 99 (codes 286 and 287) as plain invalid
    for (low, (built, want)) in fixed[..512].iter().zip(lenfix.iter()).enumerate() {
        if low & 127 == 99 {
            assert_ne!(built.op & 64, 0, "entry {low} must be invalid");
            assert_eq!(
                (want.op, want.bits, want.val),
                (64, built.bits, built.val),
                "entry {low}"
            );
        } else {
            assert_eq!(built, want, "lenfix[{low}]");
        }
    }
    assert_eq!(&fixed[distoff..], &distfix[..]);
}

#[test]
fn fixed_tables_decode_every_symbol() {
    let mut lens = [0u16; 288];
    lens[..144].fill(8);
    lens[144..256].fill(9);
    lens[256..280].fill(7);
    lens[280..288].fill(8);
    for (sym, code) in canonical_codes(&lens).into_iter().enumerate() {
        let (here, used) = decode(&lenfix, 9, code.unwrap());
        assert_eq!(used, u32::from(lens[sym]), "symbol {sym}");
        match sym {
            0..=255 => assert_eq!((here.op, here.val), (0, sym as u16)),
            256 => assert_eq!(here.op, 96),
            257..=285 => {
                assert_eq!(here.op, lext[sym - 257] as u8);
                assert_eq!(here.val, lbase[sym - 257]);
            }
            _ => assert_eq!(here.op, 64),
        }
    }
    for sym in 0..32u32 {
        let code = sym.reverse_bits() >> 27;
        let here = distfix[code as usize];
        assert_eq!(here.op, dext[sym as usize] as u8);
        assert_eq!(here.val, dbase[sym as usize]);
    }
}

#[test]
fn builds_sub_tables_for_long_codes() {
    // a complete distance code with lengths 1..=15 (and a second 15): root 6, sub-tables
    let mut lens: Vec<u16> = (1..=15).collect();
    lens.push(15);
    let mut table = vec![Code::default(); ENOUGH_DISTS];
    let mut work = [0u16; 16];
    let mut next = 0;
    let mut bits = 6;
    assert_eq!(
        inflate_table(
            CodeType::DISTS,
            &lens,
            &mut table,
            &mut next,
            &mut bits,
            &mut work
        ),
        0
    );
    assert_eq!(bits, 6);
    assert!(next > 64 && next <= ENOUGH_DISTS);
    for (sym, code) in canonical_codes(&lens).into_iter().enumerate() {
        let (here, used) = decode(&table, bits, code.unwrap());
        assert_eq!(used, u32::from(lens[sym]), "symbol {sym}");
        assert_eq!(
            (here.op, here.val),
            (dext[sym] as u8, dbase[sym]),
            "symbol {sym}"
        );
    }
}

#[test]
fn no_symbols_make_an_erroring_table() {
    let lens = [0u16; 30];
    let mut table = [Code::default(); 4];
    let mut work = [0u16; 30];
    let mut next = 1;
    let mut bits = 6;
    assert_eq!(
        inflate_table(
            CodeType::DISTS,
            &lens,
            &mut table,
            &mut next,
            &mut bits,
            &mut work
        ),
        0
    );
    let bad = Code {
        op: 64,
        bits: 1,
        val: 0,
    };
    assert_eq!(table, [Code::default(), bad, bad, Code::default()]);
    assert_eq!((next, bits), (3, 1));
}

#[test]
fn rejects_over_subscribed_and_incomplete_codes() {
    let mut table = vec![Code::default(); ENOUGH];
    let mut work = [0u16; 19];
    let mut next = 0;
    let mut bits = 7;
    // three codes of one bit: over-subscribed
    assert_eq!(
        inflate_table(
            CodeType::CODES,
            &[1, 1, 1],
            &mut table,
            &mut next,
            &mut bits,
            &mut work
        ),
        -1
    );
    // codes of lengths 1 and 2: incomplete, refused for CODES and for codes longer than 1 bit
    assert_eq!(
        inflate_table(
            CodeType::CODES,
            &[1, 2],
            &mut table,
            &mut next,
            &mut bits,
            &mut work
        ),
        -1
    );
    assert_eq!(
        inflate_table(
            CodeType::LENS,
            &[1, 2],
            &mut table,
            &mut next,
            &mut bits,
            &mut work
        ),
        -1
    );
    // a single one-bit code is incomplete but allowed for LENS and DISTS: the other entry is
    // an invalid code marker
    assert_eq!(
        inflate_table(
            CodeType::CODES,
            &[1],
            &mut table,
            &mut next,
            &mut bits,
            &mut work
        ),
        -1
    );
    assert_eq!(next, 0);
    bits = 6;
    assert_eq!(
        inflate_table(
            CodeType::DISTS,
            &[0, 1],
            &mut table,
            &mut next,
            &mut bits,
            &mut work
        ),
        0
    );
    assert_eq!((next, bits), (2, 1));
    assert_eq!(
        table[0],
        Code {
            op: 16,
            bits: 1,
            val: 2
        }
    );
    assert_eq!(
        table[1],
        Code {
            op: 64,
            bits: 1,
            val: 0
        }
    );
}

#[test]
fn reports_when_enough_is_not_enough() {
    // a complete code with lengths 1..=10 (and a second 10) and a 10-bit root table needs
    // 1024 entries: more than ENOUGH_LENS and ENOUGH_DISTS
    let mut lens: Vec<u16> = (1..=10).collect();
    lens.push(10);
    let mut table = vec![Code::default(); 1024];
    let mut work = [0u16; 11];
    for type_ in [CodeType::LENS, CodeType::DISTS] {
        let mut next = 0;
        let mut bits = 10;
        assert_eq!(
            inflate_table(type_, &lens, &mut table, &mut next, &mut bits, &mut work),
            1
        );
    }
    // CODES has no such limit
    let mut next = 0;
    let mut bits = 10;
    assert_eq!(
        inflate_table(
            CodeType::CODES,
            &lens,
            &mut table,
            &mut next,
            &mut bits,
            &mut work
        ),
        0
    );
    assert_eq!((next, bits), (1024, 10));
}
