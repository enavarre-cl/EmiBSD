//! Tests of `infback.rs`: raw deflate streams of `sys/lib/libz/testdata` (made by
//! `testdata/gen_inflate.py`, see `inflate/tests.rs`) decoded through closures, with and
//! without `inflate_fast()`.

use super::*;
use crate::inflate::{inflate, inflateEnd};
use crate::zlib::{Z_NO_FLUSH, inflateBackInit, inflateInit2};
use std::vec;
use std::vec::Vec;

static CORPUS: &[u8] = include_bytes!("../testdata/inflate_corpus.bin");
static RAW15: &[u8] = include_bytes!("../testdata/inflate_raw15.z");
static RAW12: &[u8] = include_bytes!("../testdata/inflate_raw12.z");

/// Decode `input` with a `1 << wbits` window, `strm.next_in` holding its first `first`
/// bytes and `in_` handing out the rest `chunk` bytes at a time. Returns the return code,
/// the output, the sizes `out` was called with and the unused input.
fn back(
    input: &[u8],
    wbits: i32,
    first: usize,
    chunk: usize,
    slow: bool,
) -> (i32, Vec<u8>, Vec<usize>, usize) {
    let mut strm = ZStream::new();
    assert_eq!(
        inflateBackInit(&mut strm, wbits, vec![0u8; 1 << wbits]),
        Z_OK
    );
    strm.next_in = &input[..first];
    let mut rest = &input[first..];
    let mut out = Vec::new();
    let mut sizes = Vec::new();
    let ret = inflate_back_impl(
        &mut strm,
        || {
            let (head, tail) = rest.split_at(chunk.min(rest.len()));
            rest = tail;
            head
        },
        |buf| {
            out.extend_from_slice(buf);
            sizes.push(buf.len());
            0
        },
        slow,
    );
    let unused = strm.avail_in() + rest.len();
    assert_eq!(inflateBackEnd(&mut strm), Z_OK);
    (ret, out, sizes, unused)
}

#[test]
fn decodes_through_closures() {
    for slow in [true, false] {
        for (input, wbits) in [(RAW15, 15), (RAW12, 12), (RAW12, 15)] {
            for (first, chunk) in [(0, 1), (0, 1000), (100, 7), (input.len(), 1)] {
                let (ret, out, sizes, unused) = back(input, wbits, first, chunk, slow);
                assert_eq!(
                    ret, Z_STREAM_END,
                    "slow={slow} wbits={wbits} {first}/{chunk}"
                );
                assert!(out == CORPUS);
                assert_eq!(unused, 0);
                // the window goes out whole each time it is full, then what is left
                let wsize = 1 << wbits;
                assert!(sizes[..sizes.len() - 1].iter().all(|&n| n == wsize));
                assert_eq!(sizes.len(), CORPUS.len().div_ceil(wsize));
            }
        }
    }
}

#[test]
fn leaves_unused_input_in_next_in() {
    let mut input = RAW15.to_vec();
    input.extend_from_slice(b"trailer");
    let mut strm = ZStream::new();
    assert_eq!(inflateBackInit(&mut strm, 15, vec![0u8; 1 << 15]), Z_OK);
    strm.next_in = &input;
    let mut out = Vec::new();
    let ret = inflateBack(
        &mut strm,
        || &[],
        |buf| {
            out.extend_from_slice(buf);
            0
        },
    );
    assert_eq!(ret, Z_STREAM_END);
    assert_eq!(strm.next_in, b"trailer");
    assert_eq!(inflateBackEnd(&mut strm), Z_OK);
    assert!(out == CORPUS);
}

#[test]
fn input_and_output_failures() {
    for slow in [true, false] {
        // in() runs dry: Z_BUF_ERROR, what was decoded is still written out
        let (ret, out, _, _) = back(&RAW15[..RAW15.len() / 2], 15, 0, 100, slow);
        assert_eq!(ret, Z_BUF_ERROR);
        assert!(!out.is_empty() && CORPUS.starts_with(&out));

        // out() fails: Z_BUF_ERROR
        let mut strm = ZStream::new();
        assert_eq!(inflateBackInit(&mut strm, 9, vec![0u8; 512]), Z_OK);
        strm.next_in = RAW12;
        let mut calls = 0;
        let ret = inflate_back_impl(
            &mut strm,
            || &[],
            |_| {
                calls += 1;
                1
            },
            slow,
        );
        assert_eq!((ret, calls), (Z_BUF_ERROR, 1));
        assert_eq!(inflateBackEnd(&mut strm), Z_OK);
    }
}

#[test]
fn data_errors() {
    for slow in [true, false] {
        // invalid block type
        let bad = [0x07u8, 0, 0, 0, 0, 0, 0, 0];
        let mut strm = ZStream::new();
        assert_eq!(inflateBackInit(&mut strm, 15, vec![0u8; 1 << 15]), Z_OK);
        strm.next_in = &bad;
        let ret = inflate_back_impl(&mut strm, || &[], |_| 0, slow);
        assert_eq!((ret, strm.msg), (Z_DATA_ERROR, Some("error")));
        // a distance too far back: a match before any output (fixed block: 1, 01, length
        // code 257 = 0000001, distance code 0 = 00000), padded for inflate_fast
        let far = [0x03u8, 0x02, 0, 0, 0, 0, 0, 0, 0, 0];
        strm.next_in = &far;
        let ret = inflate_back_impl(&mut strm, || &[], |_| 0, slow);
        assert_eq!((ret, strm.msg), (Z_DATA_ERROR, Some("error")));
        assert_eq!(inflateBackEnd(&mut strm), Z_OK);
    }
}

#[test]
fn init_and_state_checks() {
    let mut strm = ZStream::new();
    // no state
    assert_eq!(inflateBack(&mut strm, || &[], |_| 0), Z_STREAM_ERROR);
    assert_eq!(inflateBackEnd(&mut strm), Z_STREAM_ERROR);
    // bad window
    assert_eq!(
        inflateBackInit(&mut strm, 7, vec![0u8; 128]),
        Z_STREAM_ERROR
    );
    assert_eq!(
        inflateBackInit(&mut strm, 16, vec![0u8; 1 << 16]),
        Z_STREAM_ERROR
    );
    assert_eq!(
        inflateBackInit(&mut strm, 15, vec![0u8; 1000]),
        Z_STREAM_ERROR
    );
    assert_eq!(
        inflateBackInit_(
            &mut strm,
            15,
            vec![0u8; 1 << 15],
            "0.9",
            size_of::<ZStream<'_>>() as i32
        ),
        Z_VERSION_ERROR
    );
    assert_eq!(
        inflateBackInit_(&mut strm, 15, vec![0u8; 1 << 15], "1.3.2", 3),
        Z_VERSION_ERROR
    );
    // a call-back state is not an inflate() state, and the other way round
    assert_eq!(inflateBackInit(&mut strm, 15, vec![0u8; 1 << 15]), Z_OK);
    assert_eq!(inflate(&mut strm, Z_NO_FLUSH), Z_STREAM_ERROR);
    assert_eq!(inflateEnd(&mut strm), Z_STREAM_ERROR);
    assert_eq!(inflateBackEnd(&mut strm), Z_OK);
    assert_eq!(inflateInit2(&mut strm, -15), Z_OK);
    assert_eq!(inflateBack(&mut strm, || &[], |_| 0), Z_STREAM_ERROR);
    // inflateBackEnd frees an inflate() state too, as the C does
    assert_eq!(inflateBackEnd(&mut strm), Z_OK);
    assert_eq!(inflateEnd(&mut strm), Z_STREAM_ERROR);
}

#[test]
fn reused_for_several_streams() {
    let mut strm = ZStream::new();
    assert_eq!(inflateBackInit(&mut strm, 15, vec![0u8; 1 << 15]), Z_OK);
    for input in [RAW15, RAW12] {
        strm.next_in = input;
        let mut out = Vec::new();
        let ret = inflateBack(
            &mut strm,
            || &[],
            |buf| {
                out.extend_from_slice(buf);
                0
            },
        );
        assert_eq!(ret, Z_STREAM_END);
        assert!(out == CORPUS);
    }
    assert_eq!(inflateBackEnd(&mut strm), Z_OK);
}
