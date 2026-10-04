//! Tests of the deflate compressor.
//!
//! The vectors in `testdata/deflate_*` come from zlib 1.2.12 (the macOS system libz, which
//! Python's zlib module uses), driven through the same calls as `run` below. To regenerate
//! them: `/usr/bin/python3 sys/lib/libz/testdata/gen_deflate.py`. `deflate_vectors.txt`
//! holds the length and CRC-32 of every output (and `total_in`/`adler` at the end); three
//! complete outputs are in `.bin` files. `make_inputs` builds the inputs exactly as the
//! script does; the `input` lines check that.

use super::*;
use crate::crc32::crc32;
use crate::zlib::{deflateInit, deflateInit2};
use std::format;
use std::string::String;
use std::vec;
use std::vec::Vec;

const VECTORS: &str = include_str!("../testdata/deflate_vectors.txt");
const TEXT: &[u8] = include_bytes!("../testdata/deflate_text.txt");
const IPCOMP_TEXT: &[u8] = include_bytes!("../testdata/deflate_ipcomp_text.bin");
const ZLIB_TEXT_L9: &[u8] = include_bytes!("../testdata/deflate_zlib_text_l9.bin");
const RAW_TEXT_L1: &[u8] = include_bytes!("../testdata/deflate_raw_text_l1.bin");

/// The "large" output buffer of the vectors.
const BIG: usize = 1 << 18;

/// The generator's pseudo-random sequence: x = x * 1103515245 + 12345 mod 2^31.
struct Lcg(u32);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = (self.0.wrapping_mul(1_103_515_245).wrapping_add(12345)) & 0x7fff_ffff;
        self.0
    }

    fn byte(&mut self) -> u8 {
        (self.next() >> 16) as u8
    }
}

/// The inputs of `gen_deflate.py`'s `make_inputs`.
fn make_inputs() -> Vec<(&'static str, Vec<u8>)> {
    let mut rep = Vec::new();
    for _ in 0..1500 {
        rep.extend_from_slice(b"0123456789abcdef");
    }
    rep.extend(std::iter::repeat_n(b'z', 7000));
    for _ in 0..900 {
        rep.extend_from_slice(b"hello, world. ");
    }
    let mut g = Lcg(1);
    let rand: Vec<u8> = (0..30000).map(|_| g.byte()).collect();
    let mut mixed = Vec::new();
    let mut g = Lcg(7);
    let mut i = 0usize;
    while mixed.len() < 102_400 {
        match i % 4 {
            0 => {
                let start = (i * 37) % TEXT.len();
                let end = (start + 3000).min(TEXT.len());
                mixed.extend_from_slice(&TEXT[start..end]);
            }
            1 => mixed.extend((0..2000).map(|_| g.byte())),
            2 => mixed.extend(std::iter::repeat_n((i * 7) as u8, 500 + (i * 13) % 3000)),
            _ => mixed.extend((0..4000).map(|_| b'a' + ((g.next() >> 16) % 4) as u8)),
        }
        i += 1;
    }
    mixed.truncate(102_400);
    vec![
        ("empty", Vec::new()),
        ("one", b"a".to_vec()),
        ("text", TEXT.to_vec()),
        ("rep", rep),
        ("rand", rand),
        ("mixed", mixed),
    ]
}

fn input<'i>(inputs: &'i [(&str, Vec<u8>)], name: &str) -> &'i [u8] {
    &inputs.iter().find(|(n, _)| *n == name).unwrap().1
}

/// What the driver does between `deflateInit2` and `deflateEnd`.
#[derive(Debug, Clone, Copy)]
enum Schedule {
    /// All input with `Z_FINISH`.
    Finish,
    /// Input up to `at` with `flush`, the rest with `Z_FINISH`.
    Flush(i32, usize),
    /// Input up to `at` with `Z_NO_FLUSH`, `deflateParams(level, strategy)`, the rest with
    /// `Z_FINISH`.
    Params(usize, i32, i32),
}

/// The output buffers handed to the stream: one buffer cut into `chunk`-sized pieces, one
/// piece per call, and which bytes of each piece the call wrote.
struct Slots<'a> {
    rest: &'a mut [u8],
    offset: usize,
    chunk: usize,
    used: Vec<(usize, usize)>,
}

impl<'a> Slots<'a> {
    fn give(&mut self, strm: &mut ZStream<'a>) {
        let (piece, rest) = core::mem::take(&mut self.rest).split_at_mut(self.chunk);
        self.rest = rest;
        strm.next_out = piece;
    }

    fn taken(&mut self, strm: &ZStream<'a>) {
        self.used.push((self.offset, self.chunk - strm.avail_out()));
        self.offset += self.chunk;
    }
}

/// `deflate(flush)` over `input` with fresh output buffers until a call leaves room in its
/// buffer (and, for `Z_FINISH`, returns `Z_STREAM_END`).
fn feed<'a>(strm: &mut ZStream<'a>, slots: &mut Slots<'a>, input: &'a [u8], flush: i32) {
    strm.next_in = input;
    loop {
        slots.give(strm);
        let ret = deflate(strm, flush);
        assert!(
            matches!(ret, Z_OK | Z_STREAM_END | Z_BUF_ERROR),
            "deflate: {ret}"
        );
        slots.taken(strm);
        if strm.avail_out() != 0 {
            if flush == Z_FINISH {
                assert_eq!(ret, Z_STREAM_END);
            }
            break;
        }
    }
    assert_eq!(strm.avail_in(), 0);
}

/// The result of one compression.
struct Output {
    bytes: Vec<u8>,
    total_in: u64,
    adler: u32,
    bound: usize,
}

/// Compress `data` the way `gen_deflate.py`'s `run` does.
fn run(
    data: &[u8],
    (level, wbits, memlevel, strategy): (i32, i32, i32, i32),
    schedule: Schedule,
    chunk: usize,
    dictionary: Option<&[u8]>,
) -> Output {
    let pieces = (data.len() + data.len() / 8 + 1024) / chunk + 8;
    let mut space = vec![0u8; pieces * chunk];
    let (used, total_in, adler, bound);
    {
        let mut slots = Slots {
            rest: &mut space,
            offset: 0,
            chunk,
            used: Vec::new(),
        };
        let mut strm = ZStream::new();
        let ret = deflateInit2(&mut strm, level, Z_DEFLATED, wbits, memlevel, strategy);
        assert_eq!(ret, Z_OK);
        if let Some(d) = dictionary {
            assert_eq!(deflateSetDictionary(&mut strm, d), Z_OK);
        }
        bound = deflateBound_z(&strm, data.len());
        match schedule {
            Schedule::Finish => feed(&mut strm, &mut slots, data, Z_FINISH),
            Schedule::Flush(flush, at) => {
                feed(&mut strm, &mut slots, &data[..at], flush);
                feed(&mut strm, &mut slots, &data[at..], Z_FINISH);
            }
            Schedule::Params(at, l, s) => {
                feed(&mut strm, &mut slots, &data[..at], Z_NO_FLUSH);
                slots.give(&mut strm);
                assert_eq!(deflateParams(&mut strm, l, s), Z_OK);
                slots.taken(&strm);
                feed(&mut strm, &mut slots, &data[at..], Z_FINISH);
            }
        }
        total_in = strm.total_in;
        adler = strm.adler;
        assert_eq!(
            strm.total_out,
            slots.used.iter().map(|&(_, n)| n as u64).sum::<u64>()
        );
        assert_eq!(deflateEnd(&mut strm), Z_OK);
        used = slots.used;
    }
    let mut bytes = Vec::new();
    for (at, n) in used {
        bytes.extend_from_slice(&space[at..at + n]);
    }
    Output {
        bytes,
        total_in,
        adler,
        bound,
    }
}

/// One line of `deflate_vectors.txt`.
struct Case<'v> {
    line: &'v str,
    input: &'v str,
    params: (i32, i32, i32, i32),
    schedule: Schedule,
    chunk: usize,
    dict: Option<usize>,
    out_len: usize,
    out_crc: u32,
    total_in: u64,
    adler: u32,
}

fn cases() -> Vec<Case<'static>> {
    let mut cases = Vec::new();
    for line in VECTORS.lines() {
        let f: Vec<&str> = line.split('|').collect();
        if line.starts_with('#') || f[0] == "input" {
            continue;
        }
        let num = |i: usize| -> i32 { f[i].parse().unwrap() };
        let s: Vec<&str> = f[6].split(':').collect();
        let schedule = match s[0] {
            "finish" => Schedule::Finish,
            "params" => Schedule::Params(
                s[1].parse().unwrap(),
                s[2].parse().unwrap(),
                s[3].parse().unwrap(),
            ),
            name => {
                let flush = match name {
                    "partial" => Z_PARTIAL_FLUSH,
                    "sync" => crate::zlib::Z_SYNC_FLUSH,
                    "full" => Z_FULL_FLUSH,
                    "block" => Z_BLOCK,
                    _ => panic!("schedule {name}"),
                };
                Schedule::Flush(flush, s[1].parse().unwrap())
            }
        };
        cases.push(Case {
            line,
            input: f[1],
            params: (num(2), num(3), num(4), num(5)),
            schedule,
            chunk: f[7].parse().unwrap(),
            dict: (f[8] != "-").then(|| f[8].split(':').nth(1).unwrap().parse().unwrap()),
            out_len: f[9].parse().unwrap(),
            out_crc: u32::from_str_radix(f[10], 16).unwrap(),
            total_in: f[11].parse().unwrap(),
            adler: u32::from_str_radix(f[12], 16).unwrap(),
        });
    }
    cases
}

fn run_case(inputs: &[(&str, Vec<u8>)], c: &Case<'_>) -> Output {
    let dict = c.dict.map(|n| &TEXT[..n]);
    run(input(inputs, c.input), c.params, c.schedule, c.chunk, dict)
}

#[test]
fn inputs_match_the_generator() {
    let inputs = make_inputs();
    let mut seen = 0;
    for line in VECTORS.lines().filter(|l| l.starts_with("input|")) {
        let f: Vec<&str> = line.split('|').collect();
        let data = input(&inputs, f[1]);
        assert_eq!(data.len(), f[2].parse::<usize>().unwrap(), "{line}");
        assert_eq!(
            crc32(0, data),
            u32::from_str_radix(f[3], 16).unwrap(),
            "{line}"
        );
        seen += 1;
    }
    assert_eq!(seen, inputs.len());
}

#[test]
fn outputs_match_zlib_1_2_12() {
    let inputs = make_inputs();
    let cases = cases();
    assert!(cases.len() > 400);
    let mut failures = Vec::<String>::new();
    for c in &cases {
        let out = run_case(&inputs, c);
        let got = (
            out.bytes.len(),
            crc32(0, &out.bytes),
            out.total_in,
            out.adler,
        );
        if got != (c.out_len, c.out_crc, c.total_in, c.adler) {
            failures.push(format!(
                "{} -> got len {} crc {:08x} total_in {} adler {:08x}",
                c.line, got.0, got.1, got.2, got.3
            ));
        }
        // zlib.h: the bound holds when all input is compressed with Z_FINISH (other flushes
        // may exceed it).
        if matches!(c.schedule, Schedule::Finish) {
            assert!(
                out.bound >= out.bytes.len(),
                "deflateBound too small: {}",
                c.line
            );
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

#[test]
fn complete_outputs_match_byte_for_byte() {
    let inputs = make_inputs();
    let text = input(&inputs, "text");
    let ipcomp = run(
        text,
        (Z_DEFAULT_COMPRESSION, -12, 8, Z_DEFAULT_STRATEGY),
        Schedule::Finish,
        BIG,
        None,
    );
    assert_eq!(ipcomp.bytes, IPCOMP_TEXT);
    let zlib9 = run(
        text,
        (9, 15, 8, Z_DEFAULT_STRATEGY),
        Schedule::Finish,
        BIG,
        None,
    );
    assert_eq!(zlib9.bytes, ZLIB_TEXT_L9);
    let raw1 = run(
        text,
        (1, -15, 8, Z_DEFAULT_STRATEGY),
        Schedule::Finish,
        BIG,
        None,
    );
    assert_eq!(raw1.bytes, RAW_TEXT_L1);
}

/// The IPComp path of `xform_ipcomp.c`: a raw stream with a 4 KiB window, `Z_FINISH`
/// repeatedly with fresh 16-byte output buffers: `Z_OK` while a buffer fills up, then
/// `Z_STREAM_END`; `deflateEnd` is `Z_OK`.
#[test]
fn ipcomp_finish_with_small_buffers() {
    let mut space = [0u8; 2048];
    let mut written = 0;
    {
        let mut strm = ZStream::new();
        let ret = deflateInit2(
            &mut strm,
            Z_DEFAULT_COMPRESSION,
            Z_DEFLATED,
            -12,
            8,
            Z_DEFAULT_STRATEGY,
        );
        assert_eq!(ret, Z_OK);
        strm.next_in = TEXT;
        let mut rest: &mut [u8] = &mut space;
        let mut calls = 0;
        loop {
            let (piece, r) = core::mem::take(&mut rest).split_at_mut(16);
            rest = r;
            strm.next_out = piece;
            let ret = deflate(&mut strm, Z_FINISH);
            calls += 1;
            written += 16 - strm.avail_out();
            if ret == Z_STREAM_END {
                break;
            }
            assert_eq!(ret, Z_OK);
            assert_eq!(strm.avail_out(), 0, "Z_OK with room left");
        }
        assert!(calls > 50);
        assert_eq!(strm.total_in, TEXT.len() as u64);
        assert_eq!(strm.total_out, written as u64);
        // more Z_FINISH calls keep returning Z_STREAM_END
        let mut extra = [0u8; 4];
        strm.next_out = &mut extra;
        assert_eq!(deflate(&mut strm, Z_FINISH), Z_STREAM_END);
        assert_eq!(deflateEnd(&mut strm), Z_OK);
    }
    assert_eq!(&space[..written], IPCOMP_TEXT);
}

#[test]
fn bad_parameters_are_refused() {
    let mut strm = ZStream::new();
    let bad = [
        (10, Z_DEFLATED, 15, 8, 0),
        (-2, Z_DEFLATED, 15, 8, 0),
        (6, 7, 15, 8, 0),
        (6, Z_DEFLATED, 16, 8, 0), // gzip wrapper: not configured (NO_GZIP)
        (6, Z_DEFLATED, 31, 8, 0),
        (6, Z_DEFLATED, 7, 8, 0),
        (6, Z_DEFLATED, -8, 8, 0), // 8 only with the zlib wrapper
        (6, Z_DEFLATED, -16, 8, 0),
        (6, Z_DEFLATED, 15, 0, 0),
        (6, Z_DEFLATED, 15, 10, 0),
        (6, Z_DEFLATED, 15, 8, 5),
        (6, Z_DEFLATED, 15, 8, -1),
    ];
    for (level, method, wbits, mem, strategy) in bad {
        let ret = deflateInit2(&mut strm, level, method, wbits, mem, strategy);
        assert_eq!(
            ret, Z_STREAM_ERROR,
            "{level} {method} {wbits} {mem} {strategy}"
        );
    }
    let ret = deflateInit2_(
        &mut strm,
        6,
        Z_DEFLATED,
        15,
        8,
        0,
        "2.0",
        size_of::<ZStream<'_>>() as i32,
    );
    assert_eq!(ret, Z_VERSION_ERROR);
    let ret = deflateInit_(&mut strm, 6, ZLIB_VERSION, 4);
    assert_eq!(ret, Z_VERSION_ERROR);
    // a stream that was never initialised
    assert_eq!(deflate(&mut strm, Z_FINISH), Z_STREAM_ERROR);
    assert_eq!(deflateEnd(&mut strm), Z_STREAM_ERROR);
    assert_eq!(deflateReset(&mut strm), Z_STREAM_ERROR);
    assert_eq!(deflateParams(&mut strm, 1, 0), Z_STREAM_ERROR);
    assert_eq!(deflateBound(&strm, 1000), 1000 + 125 + 3 + 1 + 4 + 18);
}

#[test]
fn stream_errors() {
    let mut out = [0u8; 256];
    let mut strm = ZStream::new();
    assert_eq!(deflateInit(&mut strm, Z_DEFAULT_COMPRESSION), Z_OK);
    assert_eq!(
        deflateSetHeader(&mut strm, &GzHeader::default()),
        Z_STREAM_ERROR
    );
    assert_eq!(deflate(&mut strm, Z_BLOCK + 1), Z_STREAM_ERROR);
    assert_eq!(deflate(&mut strm, -1), Z_STREAM_ERROR);
    // no output space
    strm.next_in = b"hello";
    assert_eq!(deflate(&mut strm, Z_NO_FLUSH), Z_BUF_ERROR);
    assert_eq!(strm.msg, Some("buffer error"));
    strm.next_out = &mut out;
    assert_eq!(deflate(&mut strm, Z_NO_FLUSH), Z_OK);
    // nothing to do: a second Z_NO_FLUSH without input is a buffer error
    assert_eq!(deflate(&mut strm, Z_NO_FLUSH), Z_BUF_ERROR);
    assert_eq!(deflate(&mut strm, Z_FINISH), Z_STREAM_END);
    assert_eq!(deflate(&mut strm, Z_FINISH), Z_STREAM_END);
    // no more input after Z_FINISH, and no other flush
    strm.next_in = b"more";
    assert_eq!(deflate(&mut strm, Z_FINISH), Z_BUF_ERROR);
    assert_eq!(deflate(&mut strm, Z_NO_FLUSH), Z_STREAM_ERROR);
    assert_eq!(deflateEnd(&mut strm), Z_OK);
    assert_eq!(deflateEnd(&mut strm), Z_STREAM_ERROR);
}

#[test]
fn end_while_busy_is_a_data_error() {
    let mut out = [0u8; 64];
    let mut strm = ZStream::new();
    assert_eq!(deflateInit(&mut strm, 6), Z_OK);
    strm.next_in = TEXT;
    strm.next_out = &mut out;
    assert_eq!(deflate(&mut strm, Z_NO_FLUSH), Z_OK);
    assert_eq!(deflateEnd(&mut strm), Z_DATA_ERROR);
    assert!(matches!(strm.state, InternalState::None));
}

#[test]
fn reset_gives_the_same_output() {
    let mut a = [0u8; 2048];
    let mut b = [0u8; 2048];
    let mut strm = ZStream::new();
    assert_eq!(deflateInit2(&mut strm, 6, Z_DEFLATED, 15, 8, 0), Z_OK);
    strm.next_in = TEXT;
    strm.next_out = &mut a;
    assert_eq!(deflate(&mut strm, Z_FINISH), Z_STREAM_END);
    let n = strm.total_out as usize;
    assert_eq!(deflateReset(&mut strm), Z_OK);
    assert_eq!((strm.total_in, strm.total_out, strm.adler), (0, 0, 1));
    strm.next_in = TEXT;
    strm.next_out = &mut b;
    assert_eq!(deflate(&mut strm, Z_FINISH), Z_STREAM_END);
    assert_eq!(strm.total_out as usize, n);
    assert_eq!(deflateEnd(&mut strm), Z_OK);
    assert_eq!(a[..n], b[..n]);
}

#[test]
fn copy_continues_like_the_original() {
    let mut out1 = [0u8; 4096];
    let mut out2 = [0u8; 4096];
    let (half1, half2) = TEXT.split_at(TEXT.len() / 2);
    let mut head = [0u8; 4096];
    let mut strm = ZStream::new();
    assert_eq!(deflateInit2(&mut strm, 9, Z_DEFLATED, -15, 8, 0), Z_OK);
    strm.next_in = half1;
    strm.next_out = &mut head;
    assert_eq!(deflate(&mut strm, Z_NO_FLUSH), Z_OK);
    let mut copy = ZStream::new();
    assert_eq!(deflateCopy(&mut copy, &strm), Z_OK);
    assert_eq!(copy.avail_out(), 0);
    assert_eq!(copy.total_in, strm.total_in);
    for (s, out) in [(&mut strm, &mut out1), (&mut copy, &mut out2)] {
        s.next_in = half2;
        s.next_out = out;
        assert_eq!(deflate(s, Z_FINISH), Z_STREAM_END);
    }
    assert_eq!(strm.total_out, copy.total_out);
    assert_eq!(deflateEnd(&mut strm), Z_OK);
    assert_eq!(deflateEnd(&mut copy), Z_OK);
    assert_eq!(out1, out2);
}

#[test]
fn dictionary_round_trips_through_get_dictionary() {
    let mut out = [0u8; 4096];
    let mut strm = ZStream::new();
    assert_eq!(deflateInit2(&mut strm, 6, Z_DEFLATED, 9, 8, 0), Z_OK);
    // longer than the 512-byte window: only the tail is kept
    assert_eq!(deflateSetDictionary(&mut strm, TEXT), Z_OK);
    assert_eq!(strm.adler, crate::adler32::adler32(1, Some(TEXT)));
    let mut dict = [0u8; 512];
    let mut len = 0;
    assert_eq!(
        deflateGetDictionary(&strm, Some(&mut dict), Some(&mut len)),
        Z_OK
    );
    assert_eq!(len, 512);
    assert_eq!(dict[..], TEXT[TEXT.len() - 512..]);
    let mut short = [0u8; 100];
    assert_eq!(
        deflateGetDictionary(&strm, Some(&mut short), None),
        Z_BUF_ERROR
    );
    strm.next_in = b"abc";
    strm.next_out = &mut out;
    assert_eq!(deflate(&mut strm, Z_FINISH), Z_STREAM_END);
    // a zlib stream takes a dictionary only before the first deflate
    assert_eq!(deflateReset(&mut strm), Z_OK);
    strm.next_in = b"abc";
    assert_eq!(deflate(&mut strm, Z_NO_FLUSH), Z_OK);
    assert_eq!(deflateSetDictionary(&mut strm, b"abc"), Z_STREAM_ERROR);
    assert_eq!(deflateEnd(&mut strm), Z_DATA_ERROR);
}

#[test]
fn pending_prime_used_and_tune() {
    let mut out = [0u8; 4096];
    let mut strm = ZStream::new();
    assert_eq!(deflateInit2(&mut strm, 6, Z_DEFLATED, -15, 8, 0), Z_OK);
    // a raw stream may start with bits of the caller's own
    assert_eq!(deflatePrime(&mut strm, 17, 0), Z_BUF_ERROR);
    assert_eq!(deflatePrime(&mut strm, 3, 0b101), Z_OK);
    assert_eq!(deflatePrime(&mut strm, 10, 0x3ff), Z_OK);
    let (mut pending, mut bits) = (0, 0);
    assert_eq!(
        deflatePending(&strm, Some(&mut pending), Some(&mut bits)),
        Z_OK
    );
    assert_eq!((pending, bits), (1, 5));
    assert_eq!(deflateTune(&mut strm, 4, 4, 8, 4), Z_OK);
    strm.next_in = TEXT;
    strm.next_out = &mut out;
    assert_eq!(deflate(&mut strm, Z_FINISH), Z_STREAM_END);
    let mut used = 0;
    assert_eq!(deflateUsed(&strm, Some(&mut used)), Z_OK);
    assert!((1..=8).contains(&used));
    assert_eq!(deflateEnd(&mut strm), Z_OK);
    assert_eq!(out[0], 0b1111_1101);
}

#[test]
fn bound_formulas() {
    let mut strm = ZStream::new();
    // the default parameters get the tight bound
    assert_eq!(deflateInit(&mut strm, 6), Z_OK);
    assert_eq!(
        deflateBound_z(&strm, 100_000),
        100_000 + 24 + 6 + 13 - 6 + 6
    );
    assert_eq!(deflateBound(&strm, u64::MAX), u64::MAX);
    assert_eq!(deflateEnd(&mut strm), Z_OK);
    // IPComp's: window 2^12 <= hash bits 15, level != 0: the fixed-block bound, raw
    assert_eq!(deflateInit2(&mut strm, -1, Z_DEFLATED, -12, 8, 0), Z_OK);
    assert_eq!(deflateBound_z(&strm, 1000), 1000 + 125 + 3 + 1 + 4);
    assert_eq!(deflateEnd(&mut strm), Z_OK);
}
