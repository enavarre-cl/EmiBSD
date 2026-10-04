//! Tests of the Huffman half of deflate. The table values are those of zlib's `trees.h`; the
//! `#[ignore]` test (`just test-ref`) compares every table with the header in the reference
//! tree. Expected bit streams were checked against Python's zlib 1.2.12
//! (`zlib.compressobj(1, zlib.DEFLATED, -15)`).

use super::*;
use std::vec::Vec;

/// A fresh state as `deflateInit2_` allocates it (32K window, memLevel 8), trees initialised.
fn state(level: i32) -> std::boxed::Box<DeflateState> {
    let mut s = std::boxed::Box::new(DeflateState::alloc(15, 15, 1 << 14).unwrap());
    s.level = level;
    _tr_init(&mut s);
    s
}

#[test]
fn static_ltree_matches_trees_h() {
    // first entries, the 9-bit range, the 7-bit range and the last entries of trees.h
    assert_eq!(static_ltree[0], CtData::new(12, 8));
    assert_eq!(static_ltree[1], CtData::new(140, 8));
    assert_eq!(static_ltree[143], CtData::new(253, 8));
    assert_eq!(static_ltree[144], CtData::new(19, 9));
    assert_eq!(static_ltree[255], CtData::new(511, 9));
    assert_eq!(static_ltree[256], CtData::new(0, 7));
    assert_eq!(static_ltree[279], CtData::new(116, 7));
    assert_eq!(static_ltree[280], CtData::new(3, 8));
    assert_eq!(static_ltree[287], CtData::new(227, 8));
}

#[test]
fn static_dtree_matches_trees_h() {
    let codes = [
        0, 16, 8, 24, 4, 20, 12, 28, 2, 18, 10, 26, 6, 22, 14, 30, 1, 17, 9, 25, 5, 21, 13, 29, 3,
        19, 11, 27, 7, 23,
    ];
    for (n, &c) in codes.iter().enumerate() {
        assert_eq!(static_dtree[n], CtData::new(c, 5), "static_dtree[{n}]");
    }
}

#[test]
fn code_tables_match_trees_h() {
    assert_eq!(
        &_dist_code[..20],
        &[0, 1, 2, 3, 4, 4, 5, 5, 6, 6, 6, 6, 7, 7, 7, 7, 8, 8, 8, 8]
    );
    assert_eq!(&_dist_code[255..260], &[15, 0, 0, 16, 17]);
    assert_eq!(_dist_code[511], 29);
    assert_eq!(&_length_code[..12], &[0, 1, 2, 3, 4, 5, 6, 7, 8, 8, 9, 9]);
    assert_eq!(&_length_code[252..], &[27, 27, 27, 28]);
    assert_eq!(
        base_length,
        [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 32, 40, 48, 56, 64, 80, 96, 112,
            128, 160, 192, 224, 0
        ]
    );
    assert_eq!(
        base_dist,
        [
            0, 1, 2, 3, 4, 6, 8, 12, 16, 24, 32, 48, 64, 96, 128, 192, 256, 384, 512, 768, 1024,
            1536, 2048, 3072, 4096, 6144, 8192, 12288, 16384, 24576
        ]
    );
}

/// The numbers of one table of `trees.h`: everything between `<name>[...] = {` and `};`.
fn trees_h_table(text: &str, name: &str) -> Vec<u32> {
    let start = text.find(name).unwrap();
    let body = &text[start..];
    let open = body.find("= {").unwrap() + 3;
    let close = body.find("};").unwrap();
    body[open..close]
        .split(|c: char| !c.is_ascii_digit())
        .filter(|t| !t.is_empty())
        .map(|t| t.parse().unwrap())
        .collect()
}

#[test]
#[ignore = "reads trees.h from $OPENBSD_SRC (just test-ref)"]
fn every_table_matches_trees_h() {
    let root = match std::env::var_os("OPENBSD_SRC") {
        Some(dir) => std::path::PathBuf::from(dir),
        None => panic!("OPENBSD_SRC is not set; run `just test-ref`"),
    };
    let root = if root.is_absolute() {
        root
    } else {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join(root)
    };
    let text = std::fs::read_to_string(root.join("sys/lib/libz/trees.h")).unwrap();

    let flat = |t: &[CtData]| -> Vec<u32> {
        t.iter()
            .flat_map(|ct| [u32::from(ct.fc), u32::from(ct.dl)])
            .collect()
    };
    assert_eq!(trees_h_table(&text, "static_ltree"), flat(&static_ltree));
    assert_eq!(trees_h_table(&text, "static_dtree"), flat(&static_dtree));
    let bytes = |t: &[u8]| -> Vec<u32> { t.iter().map(|&b| u32::from(b)).collect() };
    assert_eq!(trees_h_table(&text, "_dist_code"), bytes(&_dist_code));
    assert_eq!(trees_h_table(&text, "_length_code"), bytes(&_length_code));
    assert_eq!(trees_h_table(&text, "base_length"), base_length.to_vec());
    assert_eq!(trees_h_table(&text, "base_dist"), base_dist.to_vec());
}

#[test]
fn bi_reverse_reverses_the_low_bits() {
    assert_eq!(bi_reverse(0b1, 1), 0b1);
    assert_eq!(bi_reverse(0b110, 3), 0b011);
    assert_eq!(bi_reverse(0b10011, 5), 0b11001);
    assert_eq!(bi_reverse(0x7fff, 15), 0x7fff);
    assert_eq!(bi_reverse(1, 15), 0x4000);
}

#[test]
fn send_bits_packs_lsb_first() {
    let mut s = state(6);
    send_bits(&mut s, 0b101, 3);
    send_bits(&mut s, 0x1fff, 13); // exactly fills bi_buf
    assert_eq!(s.bi_valid, 16);
    assert_eq!(s.pending, 0);
    send_bits(&mut s, 0b11, 2); // spills: put_short, keep 2 bits
    assert_eq!(&s.pending_buf[..2], &[0xfd, 0xff]);
    assert_eq!((s.bi_buf, s.bi_valid), (0b11, 2));
    bi_windup(&mut s);
    assert_eq!(s.pending_buf[2], 0b11);
    assert_eq!((s.bi_valid, s.bi_used), (0, 2));
}

#[test]
fn align_sends_an_empty_static_block() {
    let mut s = state(6);
    _tr_align(&mut s);
    // 3 bits of block type (010) and the 7-bit END_BLOCK code (0000000): one byte out, two
    // bits kept.
    assert_eq!(s.pending, 1);
    assert_eq!(s.pending_buf[0], 0x02);
    assert_eq!(s.bi_valid, 2);
}

#[test]
fn one_literal_makes_a_fixed_block() {
    // Python: zlib.compressobj(1, zlib.DEFLATED, -15).compress(b"a") + flush() == 4b 04 00
    let mut s = state(1);
    s.window[0] = b'a';
    assert!(!_tr_tally(&mut s, 0, usize::from(b'a')));
    let mut data_type = Z_UNKNOWN;
    _tr_flush_block(&mut s, &mut data_type, Some(0), 1, true);
    assert_eq!(&s.pending_buf[..s.pending], &[0x4b, 0x04, 0x00]);
    assert_eq!(data_type, Z_TEXT);
    assert_eq!(s.sym_next, 0); // init_block ran
}

#[test]
fn incompressible_block_is_stored() {
    // 16 distinct 9-bit literals cost as much as storing them.
    let mut s = state(6);
    for n in 0..16u8 {
        s.window[usize::from(n)] = 200 + n;
        _tr_tally(&mut s, 0, usize::from(200 + n));
    }
    let mut data_type = Z_UNKNOWN;
    _tr_flush_block(&mut s, &mut data_type, Some(0), 16, true);
    assert_eq!(data_type, Z_TEXT); // 32..255 are allow-listed
    // Python: zlib.compressobj(6, zlib.DEFLATED, -15) gives the same stored block:
    // header byte 01, LEN 16, NLEN, then the bytes: header byte 01, LEN 16, NLEN, then the bytes
    assert_eq!(&s.pending_buf[..5], &[0x01, 0x10, 0x00, 0xef, 0xff]);
    assert_eq!(&s.pending_buf[5..s.pending], &s.window[..16]);
    // without the window bytes (buf NULL), it is compressed instead
    let mut s = state(6);
    for n in 0..16u8 {
        _tr_tally(&mut s, 0, usize::from(200 + n));
    }
    let mut data_type = Z_UNKNOWN;
    s.dyn_ltree[1].fc = 1; // a block-listed byte makes it binary
    _tr_flush_block(&mut s, &mut data_type, None, 16, true);
    assert_eq!(data_type, Z_BINARY);
    assert_ne!(s.pending_buf[0] & 0x06, 0);
}

#[test]
fn tally_counts_matches_and_reports_a_full_buffer() {
    let mut s = state(6);
    s.sym_end = 6;
    assert!(!_tr_tally(&mut s, 0, 7));
    assert!(_tr_tally(&mut s, 1, 0)); // a match of MIN_MATCH at distance 1
    assert_eq!(s.matches, 1);
    assert_eq!(s.dyn_ltree[7].fc, 1);
    assert_eq!(s.dyn_ltree[LITERALS + 1].fc, 1);
    assert_eq!(s.dyn_dtree[0].fc, 1);
    let sb = s.sym_buf;
    assert_eq!(&s.pending_buf[sb..sb + 6], &[0, 0, 7, 1, 0, 0]);
}

#[test]
fn overflowing_bit_lengths_are_limited() {
    // Fibonacci frequencies would need 18-bit codes; the bit length tree allows 7.
    let mut s = state(6);
    let (mut a, mut b) = (1u16, 1u16);
    for n in 0..BL_CODES {
        s.bl_tree[n].fc = a;
        (a, b) = (b, a + b);
    }
    build_tree(&mut s, Tree::Bl);
    let lens: Vec<u16> = s.bl_tree[..BL_CODES].iter().map(|ct| ct.dl).collect();
    assert!(lens.iter().all(|&l| (1..=7).contains(&l)), "{lens:?}");
    let kraft: u32 = lens.iter().map(|&l| 1u32 << (7 - l)).sum();
    assert_eq!(kraft, 1 << 7, "the code is complete");
    assert_eq!(s.bl_desc.max_code, BL_CODES as i32 - 1);
    // the codes are a prefix code: no code is a prefix of another (codes are bit-reversed)
    for i in 0..BL_CODES {
        for j in 0..BL_CODES {
            if i != j && lens[i] <= lens[j] {
                let mask = (1u16 << lens[i]) - 1;
                assert_ne!(s.bl_tree[i].fc, s.bl_tree[j].fc & mask, "{i} prefixes {j}");
            }
        }
    }
}

#[test]
fn a_single_symbol_still_gets_two_codes() {
    let mut s = state(6);
    s.dyn_dtree[5].fc = 3;
    build_tree(&mut s, Tree::D);
    assert_eq!(s.d_desc.max_code, 5);
    assert_eq!(s.dyn_dtree[5].dl, 1);
    assert_eq!(s.dyn_dtree[0].dl, 1); // forced second code
}
