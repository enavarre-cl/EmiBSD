//! Tests of `inflate.rs` (and `inffast.rs`, which every decoding test runs too: each stream is
//! decoded with `SLOW`, the kernel's configuration, and without it).
//!
//! The streams under `sys/lib/libz/testdata/inflate_*` were made by Python's zlib (1.2.12)
//! with `sys/lib/libz/testdata/gen_inflate.py`; to regenerate them run
//! `/usr/bin/python3 sys/lib/libz/testdata/gen_inflate.py` from the repository root. The
//! RFC 1951 streams of the error and corner-case tests are built here, bit by bit.

use super::*;
use crate::adler32::adler32;
use crate::zconf::MAX_WBITS;
use crate::zlib::{Z_NO_FLUSH, Z_PARTIAL_FLUSH, Z_SYNC_FLUSH, inflateInit, inflateInit2};
use std::vec;
use std::vec::Vec;

macro_rules! testdata {
    ($name:literal) => {
        include_bytes!(concat!("../testdata/inflate_", $name))
    };
}

static CORPUS: &[u8] = testdata!("corpus.bin");
static SMALL_CORPUS: &[u8] = testdata!("small.bin");

/// Both decoders: `SLOW` (no `inflate_fast`) and fast.
const BOTH: [bool; 2] = [true, false];

/// The result of decoding a stream.
struct Decoded {
    ret: i32,
    out: Vec<u8>,
    total_in: u64,
    adler: u32,
    msg: Option<&'static str>,
}

/// Run `inflate_impl` on `strm` over `input`, giving it at most `in_chunk` new input bytes
/// and `out_chunk` bytes of output space at a time, until it stops making progress or
/// returns something other than `Z_OK` (or `Z_BUF_ERROR` while there is more to give).
/// The buffers live in a temporary stream that lends `strm`'s state.
fn run(
    strm: &mut ZStream<'_>,
    input: &[u8],
    slow: bool,
    in_chunk: usize,
    out_chunk: usize,
    flush: i32,
) -> Decoded {
    let mut buf = vec![0u8; 1 << 17];
    let mut s = ZStream::new();
    s.state = core::mem::take(&mut strm.state);
    s.total_in = strm.total_in;
    s.total_out = strm.total_out;
    s.adler = strm.adler;
    let start_out = s.total_out;
    let start_in = s.total_in;
    let mut input_rest = input;
    let mut out_rest: &mut [u8] = &mut buf;
    let mut ret;
    loop {
        if s.avail_in() == 0 && !input_rest.is_empty() {
            let n = in_chunk.min(input_rest.len());
            s.next_in = &input_rest[..n];
            input_rest = &input_rest[n..];
        }
        if s.avail_out() == 0 {
            let n = out_chunk.min(out_rest.len());
            let (head, tail) = core::mem::take(&mut out_rest).split_at_mut(n);
            s.next_out = head;
            out_rest = tail;
        }
        ret = inflate_impl(&mut s, flush, slow);
        let more = (s.avail_in() == 0 && !input_rest.is_empty())
            || (s.avail_out() == 0 && !out_rest.is_empty());
        if !(ret == Z_OK || (ret == Z_BUF_ERROR && more)) {
            break;
        }
    }
    let written = (s.total_out - start_out) as usize;
    let unused_in = s.avail_in() + input_rest.len();
    strm.state = core::mem::take(&mut s.state);
    strm.total_in = s.total_in;
    strm.total_out = s.total_out;
    strm.adler = s.adler;
    strm.msg = s.msg;
    strm.data_type = s.data_type;
    let (total_in, adler, msg) = (s.total_in, s.adler, s.msg);
    let d = Decoded {
        ret,
        out: buf[..written].to_vec(),
        total_in,
        adler,
        msg,
    };
    assert_eq!((d.total_in - start_in) as usize, input.len() - unused_in);
    d
}

/// Decode `input` with a new stream of `windowBits` `wbits`.
fn decode(
    input: &[u8],
    wbits: i32,
    slow: bool,
    in_chunk: usize,
    out_chunk: usize,
    flush: i32,
) -> Decoded {
    let mut strm = ZStream::new();
    assert_eq!(inflateInit2(&mut strm, wbits), Z_OK);
    let d = run(&mut strm, input, slow, in_chunk, out_chunk, flush);
    assert_eq!(inflateEnd(&mut strm), Z_OK);
    d
}

/// Decode `input` in one call per buffer (all input, 128K of output).
fn decode_all(input: &[u8], wbits: i32, slow: bool) -> Decoded {
    decode(input, wbits, slow, usize::MAX, usize::MAX, Z_NO_FLUSH)
}

/// Check that `input` decodes to `want`, with an Adler-32 check if it is a zlib stream.
fn assert_decodes(input: &[u8], wbits: i32, want: &[u8]) {
    for slow in BOTH {
        let d = decode_all(input, wbits, slow);
        assert_eq!(d.ret, Z_STREAM_END, "slow={slow} msg={:?}", d.msg);
        assert!(d.out == want, "slow={slow}: output differs");
        assert_eq!(d.total_in as usize, input.len());
        if wbits >= 0 {
            assert_eq!(d.adler, adler32(1, Some(want)));
        }
    }
}

/// Bits written first-bit-in-the-low-bit, as deflate packs them.
struct Bits {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl Bits {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            acc: 0,
            n: 0,
        }
    }

    fn bit(&mut self, b: u32) {
        self.acc |= (b & 1) << self.n;
        self.n += 1;
        if self.n == 8 {
            self.out.push(self.acc as u8);
            self.acc = 0;
            self.n = 0;
        }
    }

    /// A value of `n` bits, low bit first (header fields, extra bits).
    fn put(&mut self, v: u32, n: u32) {
        for i in 0..n {
            self.bit(v >> i);
        }
    }

    /// A Huffman code of `len` bits, high bit first.
    fn huff(&mut self, code: u32, len: u32) {
        for i in (0..len).rev() {
            self.bit(code >> i);
        }
    }

    /// A fixed-code literal/length symbol.
    fn fixed(&mut self, sym: u32) {
        match sym {
            0..=143 => self.huff(0x30 + sym, 8),
            144..=255 => self.huff(0x190 + sym - 144, 9),
            256..=279 => self.huff(sym - 256, 7),
            _ => self.huff(0xc0 + sym - 280, 8),
        }
    }

    /// Pad to a byte boundary.
    fn align(&mut self) {
        while self.n != 0 {
            self.bit(0);
        }
    }

    fn bytes(&mut self, b: &[u8]) {
        self.align();
        self.out.extend_from_slice(b);
    }

    fn finish(mut self) -> Vec<u8> {
        self.align();
        self.out
    }
}

/// A raw stored block (final) with `data`.
fn stored(data: &[u8]) -> Vec<u8> {
    let mut b = Bits::new();
    b.put(1, 1);
    b.put(0, 2);
    let len = data.len() as u16;
    b.bytes(&len.to_le_bytes());
    b.bytes(&(!len).to_le_bytes());
    b.bytes(data);
    b.finish()
}

/// "aaaa" as a raw fixed block: the literal 'a' and a match of length 3, distance 1.
fn fixed_aaaa() -> Vec<u8> {
    let mut b = Bits::new();
    b.put(1, 1); // BFINAL
    b.put(1, 2); // BTYPE fixed
    b.fixed(u32::from(b'a'));
    b.fixed(257); // length 3
    b.huff(0, 5); // distance code 0: distance 1
    b.fixed(256);
    b.finish()
}

/// A two-byte zlib header with compression method/info `cmf`, made to pass the check.
fn zlib_header(cmf: u8) -> [u8; 2] {
    let flg = (31 - (u32::from(cmf) * 256) % 31) % 31;
    [cmf, flg as u8]
}

/// `raw` wrapped in a zlib header and an Adler-32 trailer of `data`.
fn zlib_wrap(raw: &[u8], data: &[u8]) -> Vec<u8> {
    let mut v = zlib_header(0x78).to_vec();
    v.extend_from_slice(raw);
    v.extend_from_slice(&adler32(1, Some(data)).to_be_bytes());
    v
}

#[test]
fn python_levels() {
    let streams: [&[u8]; 10] = [
        testdata!("level0.z"),
        testdata!("level1.z"),
        testdata!("level2.z"),
        testdata!("level3.z"),
        testdata!("level4.z"),
        testdata!("level5.z"),
        testdata!("level6.z"),
        testdata!("level7.z"),
        testdata!("level8.z"),
        testdata!("level9.z"),
    ];
    for s in streams {
        assert_decodes(s, 15, SMALL_CORPUS);
        assert_decodes(s, 0, SMALL_CORPUS);
    }
}

#[test]
fn python_strategies_and_big_streams() {
    let streams: [&[u8]; 6] = [
        testdata!("big1.z"),
        testdata!("big6.z"),
        testdata!("filtered.z"),
        testdata!("huffman.z"),
        testdata!("rle.z"),
        testdata!("fixed.z"),
    ];
    for s in streams {
        assert_decodes(s, MAX_WBITS, CORPUS);
    }
}

#[test]
fn python_window_sizes() {
    let streams: [(i32, &[u8]); 7] = [
        (9, testdata!("wbits9.z")),
        (10, testdata!("wbits10.z")),
        (11, testdata!("wbits11.z")),
        (12, testdata!("wbits12.z")),
        (13, testdata!("wbits13.z")),
        (14, testdata!("wbits14.z")),
        (15, testdata!("wbits15.z")),
    ];
    for (wbits, s) in streams {
        assert_decodes(s, wbits, CORPUS);
        assert_decodes(s, 15, CORPUS);
        assert_decodes(s, 0, CORPUS);
        // a window smaller than the header asks for is refused
        if wbits > 9 {
            for slow in BOTH {
                let d = decode_all(s, wbits - 1, slow);
                assert_eq!((d.ret, d.msg), (Z_DATA_ERROR, Some("error")));
                assert!(d.out.is_empty());
            }
        }
    }
}

#[test]
fn python_raw_streams() {
    assert_decodes(testdata!("raw15.z"), -15, CORPUS);
    assert_decodes(testdata!("raw12.z"), -12, CORPUS);
    assert_decodes(testdata!("raw12.z"), -15, CORPUS);
}

#[test]
fn the_window_limits_the_distance() {
    // 5000 stored bytes, then a match 4500 back (distance code 24: 4097 + 11 extra bits)
    let data: Vec<u8> = (0..5000u32).map(|i| (i * 7 + i / 251) as u8).collect();
    let mut s = stored(&data);
    s[0] = 0; // not last
    let mut b = Bits::new();
    b.put(1, 1);
    b.put(1, 2);
    b.fixed(257);
    b.huff(24, 5);
    b.put(4500 - 4097, 11);
    b.fixed(256);
    s.extend_from_slice(&b.finish());
    s.extend_from_slice(&[0; 8]); // so that inflate_fast may run
    let mut want = data.clone();
    want.extend_from_slice(&data[500..503]);
    for slow in BOTH {
        // one call: the match is in the output, whatever the window size
        assert_eq!(decode_all(&s, -12, slow).out, want);
        // two calls: the match must come from the window, and 4K is not enough
        let d = decode(&s, -15, slow, usize::MAX, 5000, Z_NO_FLUSH);
        assert_eq!(d.ret, Z_STREAM_END);
        assert!(d.out == want);
        let d = decode(&s, -12, slow, usize::MAX, 5000, Z_NO_FLUSH);
        assert_eq!((d.ret, d.msg), (Z_DATA_ERROR, Some("error")));
        assert!(d.out == data);
    }
}

#[test]
fn one_byte_at_a_time() {
    for (s, wbits, want) in [
        (&testdata!("big6.z")[..], 15, CORPUS),
        (&testdata!("level0.z")[..], 15, SMALL_CORPUS),
        (&testdata!("fixed.z")[..], 15, CORPUS),
        (&testdata!("raw12.z")[..], -12, CORPUS),
    ] {
        for slow in BOTH {
            let d = decode(s, wbits, slow, 1, 1, Z_NO_FLUSH);
            assert_eq!(d.ret, Z_STREAM_END);
            assert!(d.out == want);
        }
    }
}

#[test]
fn assorted_buffer_sizes() {
    let s = testdata!("huffman.z");
    for (in_chunk, out_chunk) in [
        (7, 13),
        (100, 300),
        (6, 258),
        (4096, 1000),
        (1, 40000),
        (40000, 1),
    ] {
        for slow in BOTH {
            let d = decode(s, 15, slow, in_chunk, out_chunk, Z_SYNC_FLUSH);
            assert_eq!(d.ret, Z_STREAM_END, "{in_chunk}/{out_chunk} slow={slow}");
            assert!(d.out == CORPUS, "{in_chunk}/{out_chunk} slow={slow}");
        }
    }
}

#[test]
fn z_finish_without_a_window() {
    for slow in BOTH {
        let mut strm = ZStream::new();
        assert_eq!(inflateInit(&mut strm), Z_OK);
        let d = run(
            &mut strm,
            testdata!("big6.z"),
            slow,
            usize::MAX,
            usize::MAX,
            Z_FINISH,
        );
        assert_eq!(d.ret, Z_STREAM_END);
        assert!(d.out == CORPUS);
        // one call that reached the end with Z_FINISH never needed a window
        let InternalState::Inflate(state) = &strm.state else {
            panic!("no inflate state")
        };
        assert!(state.window.is_none());
        let mut len = 1;
        assert_eq!(inflateGetDictionary(&mut strm, None, Some(&mut len)), Z_OK);
        assert_eq!(len, 0);
        assert_eq!(inflateEnd(&mut strm), Z_OK);
    }
}

/// The IPComp path (`xform_ipcomp.c`): raw deflate, `Z_PARTIAL_FLUSH`, fresh output buffers
/// whenever one is full (`Z_OK` with `avail_out == 0`), until `Z_STREAM_END`.
#[test]
fn ipcomp_raw_partial_flush_with_fresh_buffers() {
    let input = testdata!("raw15.z");
    let mut bufs: Vec<Vec<u8>> = (0..64).map(|_| vec![0u8; 1000]).collect();
    let mut used = Vec::new();
    let mut strm = ZStream::new();
    assert_eq!(inflateInit2(&mut strm, -MAX_WBITS), Z_OK);
    strm.next_in = input;
    let mut bufs_iter = bufs.iter_mut();
    let mut full = 0;
    let ret = loop {
        let Some(buf) = bufs_iter.next() else {
            panic!("out of buffers")
        };
        strm.next_out = buf;
        let ret = inflate(&mut strm, Z_PARTIAL_FLUSH);
        used.push(1000 - strm.avail_out());
        if ret == Z_STREAM_END {
            break ret;
        }
        assert_eq!(ret, Z_OK);
        // "give me more space"
        assert_eq!(strm.avail_out(), 0);
        full += 1;
    };
    assert_eq!(ret, Z_STREAM_END);
    assert_eq!(strm.avail_in(), 0);
    assert_eq!(strm.total_in as usize, input.len());
    assert_eq!(strm.total_out as usize, CORPUS.len());
    assert_eq!(inflateEnd(&mut strm), Z_OK);
    assert_eq!(full, CORPUS.len() / 1000);
    let out: Vec<u8> = bufs
        .iter()
        .zip(&used)
        .flat_map(|(b, &n)| b[..n].iter().copied())
        .collect();
    assert!(out == CORPUS);
}

#[test]
fn preset_dictionary() {
    let dict = testdata!("dict.bin");
    let input = testdata!("dict.z");
    for slow in BOTH {
        let mut buf = vec![0u8; 8192];
        let mut strm = ZStream::new();
        assert_eq!(inflateInit(&mut strm), Z_OK);
        // not before inflate() asks for it
        assert_eq!(inflateSetDictionary(&mut strm, dict), Z_STREAM_ERROR);
        strm.next_in = input;
        strm.next_out = &mut buf;
        assert_eq!(inflate_impl(&mut strm, Z_NO_FLUSH, slow), Z_NEED_DICT);
        assert_eq!(strm.adler, adler32(1, Some(dict)));
        assert_eq!(strm.total_out, 0);
        assert_eq!(inflateSetDictionary(&mut strm, &dict[1..]), Z_DATA_ERROR);
        assert_eq!(inflateSetDictionary(&mut strm, dict), Z_OK);
        assert_eq!(inflate_impl(&mut strm, Z_NO_FLUSH, slow), Z_STREAM_END);
        let n = strm.total_out as usize;
        assert_eq!(strm.adler, adler32(1, Some(SMALL_CORPUS)));
        assert_eq!(inflateEnd(&mut strm), Z_OK);
        assert!(buf[..n] == *SMALL_CORPUS);

        // a raw stream takes the dictionary up front
        let mut buf = vec![0u8; 8192];
        let mut strm = ZStream::new();
        assert_eq!(inflateInit2(&mut strm, -15), Z_OK);
        assert_eq!(inflateSetDictionary(&mut strm, dict), Z_OK);
        strm.next_in = testdata!("dict_raw.z");
        strm.next_out = &mut buf;
        assert_eq!(inflate_impl(&mut strm, Z_NO_FLUSH, slow), Z_STREAM_END);
        let n = strm.total_out as usize;
        assert_eq!(inflateEnd(&mut strm), Z_OK);
        assert!(buf[..n] == *SMALL_CORPUS);
    }
}

/// The offsets `gen_inflate.py` wrote next to a stream.
fn offsets(txt: &[u8]) -> Vec<usize> {
    core::str::from_utf8(txt)
        .unwrap()
        .split_whitespace()
        .map(|w| w.parse().unwrap())
        .collect()
}

#[test]
fn sync_recovers_at_a_full_flush_point() {
    let stream = testdata!("fullflush.z");
    let off = offsets(testdata!("fullflush.txt"));
    let (flush1, piece2) = (off[0], off[2]);
    assert_eq!(&stream[flush1 - 4..flush1], &[0, 0, 0xff, 0xff]);
    let mut corrupt = stream.to_vec();
    corrupt[40..60].fill(0x55);
    for slow in BOTH {
        let mut buf = vec![0u8; 1 << 16];
        let buf_len = buf.len();
        let mut strm = ZStream::new();
        assert_eq!(inflateInit(&mut strm), Z_OK);
        // no input: nothing to search
        assert_eq!(inflateSync(&mut strm), Z_BUF_ERROR);
        // decode the damaged start; whatever comes out is discarded
        strm.next_in = &corrupt[..flush1 - 100];
        strm.next_out = &mut buf;
        let _ = inflate_impl(&mut strm, Z_NO_FLUSH, slow);
        // the marker split over two inputs: the search state carries over
        strm.next_in = &corrupt[flush1 - 100..flush1 - 2];
        let before = strm.total_in;
        assert_eq!(inflateSync(&mut strm), Z_DATA_ERROR);
        assert_eq!(strm.total_in - before, 98);
        strm.next_in = &corrupt[flush1 - 2..];
        assert_eq!(inflateSync(&mut strm), Z_OK);
        assert_eq!(strm.avail_in(), corrupt.len() - flush1);
        let out_before = strm.total_out as usize;
        let start = buf_len - strm.avail_out();
        assert_eq!(inflate_impl(&mut strm, Z_FINISH, slow), Z_STREAM_END);
        let end = start + (strm.total_out as usize - out_before);
        drop(strm);
        assert!(buf[start..end] == CORPUS[piece2..]);
    }
}

#[test]
fn sync_flush_point() {
    let stream = testdata!("syncflush.z");
    let off = offsets(testdata!("syncflush.txt"));
    let (marker, piece2) = (off[0], off[1]);
    for slow in BOTH {
        let mut buf = vec![0u8; 8192];
        let mut strm = ZStream::new();
        assert_eq!(inflateInit(&mut strm), Z_OK);
        // everything before the empty stored block's length bytes: all of the first piece
        // comes out, and inflate waits for the lengths (what PPP checks)
        strm.next_in = &stream[..marker];
        strm.next_out = &mut buf;
        assert_eq!(inflate_impl(&mut strm, Z_SYNC_FLUSH, slow), Z_OK);
        assert_eq!(strm.total_out as usize, piece2);
        assert_eq!(inflateSyncPoint(&mut strm), 1);
        strm.next_in = &stream[marker..];
        assert_eq!(inflateSyncPoint(&mut strm), 1);
        assert_eq!(inflate_impl(&mut strm, Z_SYNC_FLUSH, slow), Z_STREAM_END);
        assert_eq!(inflateSyncPoint(&mut strm), 0);
        let n = strm.total_out as usize;
        assert_eq!(inflateEnd(&mut strm), Z_OK);
        assert!(buf[..n] == *SMALL_CORPUS);
    }
}

#[test]
fn rfc1951_hand_built_blocks() {
    // a stored block
    let s = stored(b"hello");
    assert_eq!(s, [1, 5, 0, 0xfa, 0xff, b'h', b'e', b'l', b'l', b'o']);
    assert_decodes(&s, -15, b"hello");
    assert_decodes(&zlib_wrap(&s, b"hello"), 15, b"hello");
    // a fixed-Huffman block with a literal and an overlapping match
    assert_decodes(&fixed_aaaa(), -15, b"aaaa");
    assert_decodes(&zlib_wrap(&fixed_aaaa(), b"aaaa"), 15, b"aaaa");
    // an empty final block, fixed and stored
    let mut b = Bits::new();
    b.put(1, 1);
    b.put(1, 2);
    b.fixed(256);
    let empty = b.finish();
    assert_eq!(empty, [3, 0]);
    assert_decodes(&empty, -15, b"");
    assert_decodes(&stored(b""), -15, b"");
    assert_decodes(&zlib_wrap(&empty, b""), 15, b"");
    // a non-final stored block, then a fixed one
    let mut s = stored(b"abc");
    s[0] = 0; // not last
    s.extend_from_slice(&fixed_aaaa());
    assert_decodes(&s, -15, b"abcaaaa");
    // a long fixed block: 300 'x' as a literal and matches of 258 and 41 (codes 285 and 273
    // with three extra bits, 41 = 35 + 6)
    let mut b = Bits::new();
    b.put(1, 1);
    b.put(1, 2);
    b.fixed(u32::from(b'x'));
    b.fixed(285);
    b.huff(0, 5);
    b.fixed(273);
    b.put(6, 3);
    b.huff(0, 5);
    b.fixed(256);
    let mut v = b.finish();
    v.extend_from_slice(&[0; 8]); // trailing bytes, left unused
    for slow in BOTH {
        let d = decode_all(&v, -15, slow);
        assert_eq!(d.ret, Z_STREAM_END);
        assert!(d.out == [b'x'; 300]);
        assert_eq!(d.total_in as usize, v.len() - 8);
    }
}

/// Decode `input` as a whole and expect a data error with the `SMALL` message.
fn assert_data_error(input: &[u8], wbits: i32) {
    for slow in BOTH {
        let d = decode_all(input, wbits, slow);
        assert_eq!(d.ret, Z_DATA_ERROR, "slow={slow}");
        assert_eq!(d.msg, Some("error"));
    }
}

#[test]
fn header_errors() {
    // incorrect header check
    assert_data_error(&[0x78, 0x9d, 3, 0], 15);
    // unknown compression method (9)
    let mut v = zlib_header(0x79).to_vec();
    v.extend_from_slice(&[3, 0]);
    assert_data_error(&v, 15);
    // invalid window size (CINFO 8: 64K)
    let mut v = zlib_header(0x88).to_vec();
    v.extend_from_slice(&[3, 0]);
    assert_data_error(&v, 15);
    // a wrong check value, and the same stream when the check is not validated
    let mut v = testdata!("big6.z").to_vec();
    *v.last_mut().unwrap() ^= 1;
    assert_data_error(&v, 15);
    for slow in BOTH {
        let mut strm = ZStream::new();
        assert_eq!(inflateInit(&mut strm), Z_OK);
        assert_eq!(inflateValidate(&mut strm, 0), Z_OK);
        let d = run(&mut strm, &v, slow, usize::MAX, usize::MAX, Z_NO_FLUSH);
        assert_eq!(d.ret, Z_STREAM_END);
        assert!(d.out == CORPUS);
        assert_eq!(inflateValidate(&mut strm, 1), Z_OK);
        assert_eq!(inflateEnd(&mut strm), Z_OK);
    }
}

#[test]
fn block_errors() {
    // invalid block type (3)
    assert_data_error(&[0x07, 0, 0, 0, 0, 0, 0, 0], -15);
    // invalid stored block lengths
    assert_data_error(&[1, 5, 0, 0xfa, 0xfe, 1, 2, 3, 4, 5], -15);
    // a distance too far back: a match before any output (padded for inflate_fast)
    let mut b = Bits::new();
    b.put(1, 1);
    b.put(1, 2);
    b.fixed(257);
    b.huff(0, 5);
    b.fixed(256);
    let mut v = b.finish();
    v.extend_from_slice(&[0; 8]);
    assert_data_error(&v, -15);
    // too many length symbols (HLIT 30: 287)
    let mut b = Bits::new();
    b.put(1, 1);
    b.put(2, 2);
    b.put(30, 5);
    b.put(0, 5);
    b.put(0, 4);
    assert_data_error(&b.finish(), -15);
    // an over-subscribed code length code: four code lengths of one bit
    let mut b = Bits::new();
    b.put(1, 1);
    b.put(2, 2);
    b.put(0, 5);
    b.put(0, 5);
    b.put(0, 4);
    for _ in 0..4 {
        b.put(1, 3);
    }
    assert_data_error(&b.finish(), -15);
    // no end-of-block code: code length codes 0 and 17 (one bit each), every length 0
    let mut b = Bits::new();
    b.put(1, 1);
    b.put(2, 2);
    b.put(0, 5);
    b.put(0, 5);
    b.put(0, 4);
    b.put(0, 3); // 16
    b.put(1, 3); // 17
    b.put(0, 3); // 18
    b.put(1, 3); // 0
    for _ in 0..258 {
        b.huff(0, 1);
    }
    assert_data_error(&b.finish(), -15);
}

#[test]
fn truncated_stream() {
    let s = testdata!("level6.z");
    let cut = &s[..s.len() - 5];
    for slow in BOTH {
        let d = decode(cut, 15, slow, usize::MAX, usize::MAX, Z_FINISH);
        assert_eq!(d.ret, Z_BUF_ERROR);
        assert!(SMALL_CORPUS.starts_with(&d.out));
        // without Z_FINISH: Z_OK while it consumes input, Z_BUF_ERROR once stuck
        let mut strm = ZStream::new();
        assert_eq!(inflateInit(&mut strm), Z_OK);
        let mut buf = vec![0u8; 8192];
        strm.next_in = cut;
        strm.next_out = &mut buf;
        assert_eq!(inflate_impl(&mut strm, Z_NO_FLUSH, slow), Z_OK);
        assert_eq!(inflate_impl(&mut strm, Z_NO_FLUSH, slow), Z_BUF_ERROR);
        assert_eq!(inflateEnd(&mut strm), Z_OK);
    }
}

#[test]
fn prime_inserts_bits() {
    let s = testdata!("raw15.z");
    for slow in BOTH {
        // the first byte primed, the rest as input
        let mut strm = ZStream::new();
        assert_eq!(inflateInit2(&mut strm, -15), Z_OK);
        assert_eq!(inflatePrime(&mut strm, 8, i32::from(s[0])), Z_OK);
        let d = run(&mut strm, &s[1..], slow, 512, 512, Z_NO_FLUSH);
        assert_eq!(d.ret, Z_STREAM_END);
        assert!(d.out == CORPUS);
        // two bytes primed in one call, after clearing a junk prime
        assert_eq!(inflateReset(&mut strm), Z_OK);
        assert_eq!(inflatePrime(&mut strm, 5, 0x1f), Z_OK);
        assert_eq!(inflatePrime(&mut strm, -1, 0), Z_OK);
        assert_eq!(
            inflatePrime(
                &mut strm,
                16,
                i32::from(s[0]) | i32::from(s[1]) << 8 | 0x7fff_0000
            ),
            Z_OK
        );
        let d = run(&mut strm, &s[2..], slow, usize::MAX, usize::MAX, Z_NO_FLUSH);
        assert_eq!(d.ret, Z_STREAM_END);
        assert!(d.out == CORPUS);
        // limits: 16 bits a call, 32 in all
        assert_eq!(inflateReset(&mut strm), Z_OK);
        assert_eq!(inflatePrime(&mut strm, 17, 0), Z_STREAM_ERROR);
        assert_eq!(inflatePrime(&mut strm, 0, 0), Z_OK);
        assert_eq!(inflatePrime(&mut strm, 16, 0), Z_OK);
        assert_eq!(inflatePrime(&mut strm, 16, 0), Z_OK);
        assert_eq!(inflatePrime(&mut strm, 1, 0), Z_STREAM_ERROR);
        assert_eq!(inflateEnd(&mut strm), Z_OK);
    }
}

#[test]
fn mark_reports_the_position() {
    let mut strm = ZStream::new();
    assert_eq!(inflateMark(&mut strm), -(1 << 16));
    // in a stored block: the bytes left to copy
    let s = stored(b"hello");
    let mut out = [0u8; 8];
    assert_eq!(inflateInit2(&mut strm, -15), Z_OK);
    assert_eq!(inflateMark(&mut strm), -(1 << 16));
    strm.next_in = &s;
    strm.next_out = &mut out[..2];
    assert_eq!(inflate(&mut strm, Z_NO_FLUSH), Z_OK);
    assert_eq!(inflateMark(&mut strm), -(1 << 16) + 3);
    assert_eq!(inflateEnd(&mut strm), Z_OK);

    // in a match: the bits of the length/distance codes back, and the bytes copied so far
    let s = fixed_aaaa();
    let mut out = [0u8; 4];
    let mut strm = ZStream::new();
    assert_eq!(inflateInit2(&mut strm, -15), Z_OK);
    strm.next_in = &s;
    let (o1, rest) = out.split_at_mut(1);
    let (o2, rest) = rest.split_at_mut(1);
    let (o3, o4) = rest.split_at_mut(1);
    strm.next_out = o1;
    assert_eq!(inflate(&mut strm, Z_NO_FLUSH), Z_OK);
    assert_eq!(inflateMark(&mut strm), 12 << 16);
    strm.next_out = o2;
    assert_eq!(inflate(&mut strm, Z_NO_FLUSH), Z_OK);
    assert_eq!(inflateMark(&mut strm), (12 << 16) + 1);
    strm.next_out = o3;
    assert_eq!(inflate(&mut strm, Z_NO_FLUSH), Z_OK);
    assert_eq!(inflateMark(&mut strm), (12 << 16) + 2);
    strm.next_out = o4;
    assert_eq!(inflate(&mut strm, Z_NO_FLUSH), Z_STREAM_END);
    assert_eq!(inflateMark(&mut strm), -(1 << 16));
    assert_eq!(inflateEnd(&mut strm), Z_OK);
    assert_eq!(&out, b"aaaa");
}

#[test]
fn get_dictionary_returns_the_window() {
    // a zlib stream decoded in one call never fills the window: inflate() resets its output
    // count after the check value, so the end-of-call update sees no output (as in the C)
    let mut strm = ZStream::new();
    assert_eq!(inflateInit(&mut strm), Z_OK);
    let d = run(
        &mut strm,
        testdata!("big6.z"),
        true,
        usize::MAX,
        usize::MAX,
        Z_NO_FLUSH,
    );
    assert_eq!(d.ret, Z_STREAM_END);
    let mut len = 1;
    assert_eq!(inflateGetDictionary(&mut strm, None, Some(&mut len)), Z_OK);
    assert_eq!(len, 0);
    assert_eq!(inflateEnd(&mut strm), Z_OK);

    for (s, wbits, out_chunk, slow) in [
        (&testdata!("raw15.z")[..], -15, usize::MAX, true),
        (&testdata!("raw15.z")[..], -15, usize::MAX, false),
        (&testdata!("raw15.z")[..], -15, 1000, true),
        (&testdata!("raw15.z")[..], -15, 777, false),
    ] {
        let mut strm = ZStream::new();
        assert_eq!(inflateInit2(&mut strm, wbits), Z_OK);
        let d = run(&mut strm, s, slow, usize::MAX, out_chunk, Z_NO_FLUSH);
        assert_eq!(d.ret, Z_STREAM_END);
        let mut len = 0;
        assert_eq!(inflateGetDictionary(&mut strm, None, Some(&mut len)), Z_OK);
        assert_eq!(len, 32768);
        let mut dict = vec![0u8; 32768];
        assert_eq!(inflateGetDictionary(&mut strm, Some(&mut dict), None), Z_OK);
        assert!(dict == CORPUS[CORPUS.len() - 32768..]);
        let mut short = vec![0u8; 100];
        assert_eq!(
            inflateGetDictionary(&mut strm, Some(&mut short), None),
            Z_BUF_ERROR
        );
        assert_eq!(inflateEnd(&mut strm), Z_OK);
    }
}

#[test]
fn copy_continues_independently() {
    let s = testdata!("raw15.z");
    let half = s.len() / 2;
    for slow in BOTH {
        let mut strm = ZStream::new();
        assert_eq!(inflateInit2(&mut strm, -15), Z_OK);
        let first = run(
            &mut strm,
            &s[..half],
            slow,
            usize::MAX,
            usize::MAX,
            Z_NO_FLUSH,
        );
        assert_eq!(first.ret, Z_BUF_ERROR); // all input used, then no progress
        let mut dest = ZStream::new();
        assert_eq!(inflateCopy(&mut dest, &strm), Z_OK);
        assert_eq!(dest.total_in, strm.total_in);
        assert_eq!(dest.total_out, strm.total_out);
        assert_eq!(dest.avail_out(), 0);
        let a = run(
            &mut strm,
            &s[half..],
            slow,
            usize::MAX,
            usize::MAX,
            Z_NO_FLUSH,
        );
        let b = run(&mut dest, &s[half..], slow, 100, 100, Z_NO_FLUSH);
        assert_eq!((a.ret, b.ret), (Z_STREAM_END, Z_STREAM_END));
        assert!(a.out == b.out);
        assert!([first.out, a.out].concat() == CORPUS);
        assert_eq!(inflateEnd(&mut strm), Z_OK);
        assert_eq!(inflateEnd(&mut dest), Z_OK);
    }
    let empty = ZStream::new();
    let mut dest = ZStream::new();
    assert_eq!(inflateCopy(&mut dest, &empty), Z_STREAM_ERROR);
}

#[test]
fn reset2_changes_the_window() {
    let mut strm = ZStream::new();
    assert_eq!(inflateInit2(&mut strm, 15), Z_OK);
    let d = run(
        &mut strm,
        testdata!("big6.z"),
        true,
        usize::MAX,
        4096,
        Z_NO_FLUSH,
    );
    assert_eq!(d.ret, Z_STREAM_END);
    let window_len = |strm: &ZStream<'_>| match &strm.state {
        InternalState::Inflate(state) => state.window.as_ref().map(Vec::len),
        _ => panic!("no inflate state"),
    };
    assert_eq!(window_len(&strm), Some(32768));
    // same size: the window is kept (inflateReset2 -> inflateReset)
    assert_eq!(inflateReset2(&mut strm, 15), Z_OK);
    assert_eq!(window_len(&strm), Some(32768));
    // a smaller window: freed, then allocated at the new size when needed
    assert_eq!(inflateReset2(&mut strm, 9), Z_OK);
    assert_eq!(window_len(&strm), None);
    let d = run(
        &mut strm,
        testdata!("wbits9.z"),
        true,
        usize::MAX,
        4096,
        Z_NO_FLUSH,
    );
    assert_eq!(d.ret, Z_STREAM_END);
    assert!(d.out == CORPUS);
    assert_eq!(window_len(&strm), Some(512));
    // raw
    assert_eq!(inflateReset2(&mut strm, -15), Z_OK);
    let d = run(
        &mut strm,
        testdata!("raw15.z"),
        false,
        usize::MAX,
        4096,
        Z_NO_FLUSH,
    );
    assert_eq!(d.ret, Z_STREAM_END);
    assert!(d.out == CORPUS);
    // windowBits 0: the header says
    assert_eq!(inflateReset2(&mut strm, 0), Z_OK);
    let d = run(
        &mut strm,
        testdata!("wbits12.z"),
        true,
        3000,
        3000,
        Z_NO_FLUSH,
    );
    assert_eq!(d.ret, Z_STREAM_END);
    assert!(d.out == CORPUS);
    assert_eq!(window_len(&strm), Some(4096));
    // refused: too small, too large, gzip (31) and auto-detect (47) without GUNZIP
    for bad in [7, 16, 31, 47, -7, -16, 1] {
        assert_eq!(inflateReset2(&mut strm, bad), Z_STREAM_ERROR, "{bad}");
    }
    assert_eq!(inflateEnd(&mut strm), Z_OK);
}

#[test]
fn reset_keep_keeps_the_window() {
    let mut strm = ZStream::new();
    assert_eq!(inflateInit2(&mut strm, -15), Z_OK);
    let d = run(
        &mut strm,
        testdata!("raw15.z"),
        true,
        usize::MAX,
        4096,
        Z_NO_FLUSH,
    );
    assert_eq!(d.ret, Z_STREAM_END);
    assert_eq!(inflateResetKeep(&mut strm), Z_OK);
    assert_eq!((strm.total_in, strm.total_out, strm.msg), (0, 0, None));
    let mut len = 0;
    assert_eq!(inflateGetDictionary(&mut strm, None, Some(&mut len)), Z_OK);
    assert_eq!(len, 32768);
    assert_eq!(inflateReset(&mut strm), Z_OK);
    assert_eq!(inflateGetDictionary(&mut strm, None, Some(&mut len)), Z_OK);
    assert_eq!(len, 0);
    assert_eq!(inflateEnd(&mut strm), Z_OK);
}

#[test]
fn block_flushes_and_data_type() {
    // Z_BLOCK stops at each block boundary; data_type says where
    let mut s = stored(b"abc");
    s[0] = 0;
    s.extend_from_slice(&fixed_aaaa());
    let mut buf = [0u8; 16];
    let mut strm = ZStream::new();
    assert_eq!(inflateInit2(&mut strm, -15), Z_OK);
    strm.next_in = &s;
    strm.next_out = &mut buf;
    assert_eq!(inflate(&mut strm, Z_BLOCK), Z_OK);
    assert_eq!(strm.total_out, 3);
    assert_eq!(strm.data_type & 128, 128); // at the end of a block
    assert_eq!(strm.data_type & 64, 0); // not the last
    assert_eq!(inflate(&mut strm, Z_TREES), Z_OK);
    assert_eq!(strm.total_out, 3);
    assert_eq!(strm.data_type & 256, 256); // after the fixed block's header
    assert_eq!(strm.data_type & 64, 64); // the last block
    // the last block's end is a block boundary too
    assert_eq!(inflate(&mut strm, Z_BLOCK), Z_OK);
    assert_eq!(strm.total_out, 7);
    assert_eq!(strm.data_type & (64 | 128), 64 | 128);
    assert_eq!(inflate(&mut strm, Z_BLOCK), Z_STREAM_END);
    assert_eq!(inflateEnd(&mut strm), Z_OK);
    assert_eq!(&buf[..7], b"abcaaaa");
}

#[test]
fn codes_used_counts_the_dynamic_tables() {
    let mut strm = ZStream::new();
    assert_eq!(inflateCodesUsed(&mut strm), u64::MAX);
    assert_eq!(inflateInit(&mut strm), Z_OK);
    assert_eq!(inflateCodesUsed(&mut strm), 0);
    let d = run(
        &mut strm,
        testdata!("big6.z"),
        true,
        2000,
        usize::MAX,
        Z_NO_FLUSH,
    );
    assert_eq!(d.ret, Z_STREAM_END);
    let used = inflateCodesUsed(&mut strm);
    assert!(used > 512 && used <= ENOUGH as u64, "{used}");
    assert_eq!(inflateEnd(&mut strm), Z_OK);
}

#[test]
fn api_misuse() {
    let mut strm = ZStream::new();
    // no state
    assert_eq!(inflate(&mut strm, Z_NO_FLUSH), Z_STREAM_ERROR);
    assert_eq!(inflateEnd(&mut strm), Z_STREAM_ERROR);
    assert_eq!(inflateReset(&mut strm), Z_STREAM_ERROR);
    assert_eq!(inflateResetKeep(&mut strm), Z_STREAM_ERROR);
    assert_eq!(inflateReset2(&mut strm, 15), Z_STREAM_ERROR);
    assert_eq!(inflatePrime(&mut strm, 1, 1), Z_STREAM_ERROR);
    assert_eq!(inflateSetDictionary(&mut strm, b"x"), Z_STREAM_ERROR);
    assert_eq!(inflateGetDictionary(&mut strm, None, None), Z_STREAM_ERROR);
    assert_eq!(inflateSync(&mut strm), Z_STREAM_ERROR);
    assert_eq!(inflateSyncPoint(&mut strm), Z_STREAM_ERROR);
    assert_eq!(inflateUndermine(&mut strm, 1), Z_STREAM_ERROR);
    assert_eq!(inflateValidate(&mut strm, 1), Z_STREAM_ERROR);
    assert_eq!(
        inflateGetHeader(&mut strm, &mut GzHeader::default()),
        Z_STREAM_ERROR
    );
    // version and size checks
    assert_eq!(
        inflateInit_(&mut strm, "2.0", size_of::<ZStream<'_>>() as i32),
        Z_VERSION_ERROR
    );
    assert_eq!(
        inflateInit_(&mut strm, "", size_of::<ZStream<'_>>() as i32),
        Z_VERSION_ERROR
    );
    assert_eq!(inflateInit_(&mut strm, "1.2.12", 8), Z_VERSION_ERROR);
    assert_eq!(
        inflateInit_(&mut strm, "1.2.12", size_of::<ZStream<'_>>() as i32),
        Z_OK
    );
    assert_eq!(inflateEnd(&mut strm), Z_OK);
    assert_eq!(inflateEnd(&mut strm), Z_STREAM_ERROR);
    // gzip is not configured
    assert_eq!(inflateInit2(&mut strm, 31), Z_STREAM_ERROR);
    assert!(matches!(strm.state, InternalState::None));
    assert_eq!(inflateInit2(&mut strm, 15), Z_OK);
    assert_eq!(
        inflateGetHeader(&mut strm, &mut GzHeader::default()),
        Z_STREAM_ERROR
    );
    // no ARRR build: undermining is refused
    assert_eq!(inflateUndermine(&mut strm, 1), Z_DATA_ERROR);
    // a dictionary is refused in the middle of a zlib stream
    assert_eq!(inflateSetDictionary(&mut strm, b"x"), Z_STREAM_ERROR);
    // after an error, inflate stays in BAD until reset
    let bad = [0x78u8, 0x9d, 3, 0];
    strm.next_in = &bad;
    let mut out = [0u8; 4];
    strm.next_out = &mut out;
    assert_eq!(inflate(&mut strm, Z_NO_FLUSH), Z_DATA_ERROR);
    assert_eq!(inflate(&mut strm, Z_NO_FLUSH), Z_DATA_ERROR);
    assert_eq!(inflateReset(&mut strm), Z_OK);
    assert_eq!(strm.msg, None);
    assert_eq!(inflateEnd(&mut strm), Z_OK);
}

#[test]
fn messages_follow_small() {
    const { assert!(SMALL) };
    assert_eq!(small_msg("incorrect header check"), "error");
}

#[test]
fn full_flush_stream_decodes_whole() {
    assert_decodes(testdata!("fullflush.z"), 15, CORPUS);
    assert_decodes(testdata!("syncflush.z"), 15, SMALL_CORPUS);
}

/// Compress `data` with this crate's deflate (`level`, `windowBits`, `strategy`), flushing
/// with `flush` after every `chunk` bytes and finishing with `Z_FINISH`.
fn deflate_with(
    data: &[u8],
    level: i32,
    wbits: i32,
    strategy: i32,
    chunk: usize,
    flush: i32,
) -> Vec<u8> {
    use crate::deflate::{deflate, deflateEnd};
    use crate::zlib::{Z_DEFLATED, deflateInit2};
    let mut out = vec![0u8; data.len() + data.len() / 2 + 1024];
    let mut strm = ZStream::new();
    assert_eq!(
        deflateInit2(&mut strm, level, Z_DEFLATED, wbits, 8, strategy),
        Z_OK
    );
    strm.next_out = &mut out;
    let pieces: Vec<&[u8]> = data.chunks(chunk).collect();
    for (i, piece) in pieces.iter().enumerate() {
        strm.next_in = piece;
        let last = i + 1 == pieces.len();
        let ret = deflate(&mut strm, if last { Z_FINISH } else { flush });
        assert_eq!(ret, if last { Z_STREAM_END } else { Z_OK });
        assert_eq!(strm.avail_in(), 0);
    }
    let n = strm.total_out as usize;
    assert_eq!(deflateEnd(&mut strm), Z_OK);
    out.truncate(n);
    out
}

#[test]
fn round_trips_with_this_crates_deflate() {
    use crate::zlib::{
        Z_DEFAULT_STRATEGY, Z_FILTERED, Z_FIXED, Z_FULL_FLUSH, Z_HUFFMAN_ONLY, Z_RLE,
    };
    for level in 0..=9 {
        let z = deflate_with(
            CORPUS,
            level,
            15,
            Z_DEFAULT_STRATEGY,
            usize::MAX,
            Z_NO_FLUSH,
        );
        assert_decodes(&z, 15, CORPUS);
    }
    for strategy in [Z_FILTERED, Z_HUFFMAN_ONLY, Z_RLE, Z_FIXED] {
        let z = deflate_with(CORPUS, 6, -15, strategy, usize::MAX, Z_NO_FLUSH);
        assert_decodes(&z, -15, CORPUS);
    }
    for wbits in [9, 12, 15] {
        let z = deflate_with(CORPUS, 9, wbits, Z_DEFAULT_STRATEGY, 5000, Z_FULL_FLUSH);
        assert_decodes(&z, wbits, CORPUS);
        let z = deflate_with(CORPUS, 9, -wbits, Z_DEFAULT_STRATEGY, 3000, Z_SYNC_FLUSH);
        assert_decodes(&z, -wbits, CORPUS);
    }
}

#[test]
fn round_trips_the_ipcomp_way() {
    // xform_ipcomp.c: raw deflate of a packet with Z_FINISH, raw inflate with
    // Z_PARTIAL_FLUSH into fresh buffers
    use crate::zlib::Z_DEFAULT_STRATEGY;
    for packet in [&CORPUS[..1400], &CORPUS[5000..5100], CORPUS] {
        let z = deflate_with(
            packet,
            -1,
            -MAX_WBITS,
            Z_DEFAULT_STRATEGY,
            usize::MAX,
            Z_NO_FLUSH,
        );
        for slow in BOTH {
            let d = decode(&z, -MAX_WBITS, slow, usize::MAX, 256, Z_PARTIAL_FLUSH);
            assert_eq!(d.ret, Z_STREAM_END);
            assert!(d.out == packet);
        }
    }
}

/// Compress `input` with this crate's deflate in one `Z_FINISH` call, with `memLevel`
/// `mem_level` (an empty input too, which `deflate_with` has no piece for).
fn deflate_all(input: &[u8], level: i32, wbits: i32, mem_level: i32) -> Vec<u8> {
    use crate::deflate::{deflate, deflateEnd};
    use crate::zlib::{Z_DEFAULT_STRATEGY, Z_DEFLATED, Z_FINISH, deflateInit2};
    let mut out = vec![0u8; input.len() + input.len() / 8 + 1024];
    let mut strm = ZStream::new();
    assert_eq!(
        deflateInit2(
            &mut strm,
            level,
            Z_DEFLATED,
            wbits,
            mem_level,
            Z_DEFAULT_STRATEGY
        ),
        Z_OK
    );
    strm.next_in = input;
    strm.next_out = &mut out;
    assert_eq!(deflate(&mut strm, Z_FINISH), Z_STREAM_END);
    let n = strm.total_out as usize;
    assert_eq!(deflateEnd(&mut strm), Z_OK);
    out.truncate(n);
    out
}

#[test]
fn round_trips_every_window_and_memory_size() {
    for wbits in 9..=15 {
        for mem_level in [1, 5, 9] {
            let z = deflate_all(CORPUS, 6, wbits, mem_level);
            assert_decodes(&z, wbits, CORPUS);
            assert_decodes(&z, 0, CORPUS);
            let raw = deflate_all(SMALL_CORPUS, 9, -wbits, mem_level);
            assert_decodes(&raw, -wbits, SMALL_CORPUS);
        }
    }
}

#[test]
fn round_trips_odd_inputs() {
    let zeros = vec![0u8; 70_000];
    let noise: Vec<u8> = (0u32..50_000)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    for input in [&[][..], &[42][..], &zeros[..], &noise[..]] {
        for level in [0, 1, 6, 9] {
            let z = deflate_all(input, level, 15, 8);
            assert_decodes(&z, 15, input);
        }
    }
}

/// One stream written in pieces with every flush mode between them (`Z_PARTIAL_FLUSH` and
/// `Z_BLOCK` too) decodes whole, and with one byte of input and output at a time.
#[test]
fn round_trips_every_flush_mode() {
    use crate::deflate::{deflate, deflateEnd};
    use crate::zlib::{Z_BLOCK, Z_FINISH, Z_FULL_FLUSH, deflateInit};
    let mut out = vec![0u8; CORPUS.len() * 2];
    let mut strm = ZStream::new();
    assert_eq!(deflateInit(&mut strm, 6), Z_OK);
    strm.next_out = &mut out;
    let flushes = [
        Z_NO_FLUSH,
        Z_PARTIAL_FLUSH,
        Z_SYNC_FLUSH,
        Z_FULL_FLUSH,
        Z_BLOCK,
    ];
    for (i, piece) in CORPUS.chunks(3000).enumerate() {
        strm.next_in = piece;
        assert_eq!(deflate(&mut strm, flushes[i % flushes.len()]), Z_OK);
        assert_eq!(strm.avail_in(), 0);
    }
    assert_eq!(deflate(&mut strm, Z_FINISH), Z_STREAM_END);
    let n = strm.total_out as usize;
    assert_eq!(deflateEnd(&mut strm), Z_OK);
    let z = &out[..n];
    assert_decodes(z, 15, CORPUS);
    for slow in BOTH {
        let d = decode(z, 15, slow, 1, 1, Z_NO_FLUSH);
        assert_eq!(d.ret, Z_STREAM_END);
        assert!(d.out == CORPUS);
    }
}

/// `xform_ipcomp.c`'s compression side: raw deflate with `Z_FINISH` into fresh 512-byte
/// buffers whenever one is full, then raw inflate with `Z_PARTIAL_FLUSH` into 333-byte ones.
#[test]
fn round_trips_ipcomp_output_buffers() {
    use crate::deflate::{deflate, deflateEnd};
    use crate::zlib::{Z_DEFAULT_STRATEGY, Z_DEFLATED, Z_FINISH, deflateInit2};
    for size in [1, 100, 1000, 1400, 9000] {
        let packet = &CORPUS[..size];
        let mut bufs: Vec<Vec<u8>> = (0..40).map(|_| vec![0u8; 512]).collect();
        let mut used = Vec::new();
        let mut c = ZStream::new();
        assert_eq!(
            deflateInit2(&mut c, 6, Z_DEFLATED, -11, 8, Z_DEFAULT_STRATEGY),
            Z_OK
        );
        c.next_in = packet;
        for buf in bufs.iter_mut() {
            c.next_out = buf;
            let ret = deflate(&mut c, Z_FINISH);
            used.push(512 - c.avail_out());
            if ret == Z_STREAM_END {
                break;
            }
            assert_eq!(ret, Z_OK);
        }
        assert_eq!(deflateEnd(&mut c), Z_OK);
        let z: Vec<u8> = bufs
            .iter()
            .zip(&used)
            .flat_map(|(b, &n)| b[..n].iter().copied())
            .collect();
        for slow in BOTH {
            let d = decode(&z, -11, slow, usize::MAX, 333, Z_PARTIAL_FLUSH);
            assert_eq!(d.ret, Z_STREAM_END);
            assert!(d.out == packet, "size={size} slow={slow}");
        }
    }
}
