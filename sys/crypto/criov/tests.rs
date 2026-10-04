//! Tests of the `cuio_*` functions over uios of several iovecs, including empty ones.

use core::ffi::c_void;
use std::vec::Vec;
use std::{assert_eq, vec};

use super::*;
use crate::sys::uio::UioRw;

/// A uio over `buf` cut into iovecs of the given lengths.
fn cut<'a>(buf: &mut [u8], cuts: &[usize], iov: &'a mut Vec<Iovec>) -> Uio<'a> {
    let mut off = 0;
    for &c in cuts {
        iov.push(Iovec {
            iov_base: buf.as_mut_ptr().wrapping_add(off).cast::<c_void>(),
            iov_len: c,
        });
        off += c;
    }
    assert_eq!(off, buf.len());
    Uio {
        uio_iov: iov,
        uio_offset: 0,
        uio_resid: buf.len(),
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_WRITE,
        uio_procp: None,
    }
}

fn data(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 7 + 1) as u8).collect()
}

#[test]
fn copydata_crosses_iovecs() {
    let mut buf = data(20);
    let mut iov = Vec::new();
    let uio = cut(&mut buf, &[3, 0, 9, 8], &mut iov);
    let mut out = [0u8; 12];
    cuio_copydata(&uio, 2, &mut out);
    assert_eq!(out[..], data(20)[2..14]);
    let mut all = [0u8; 20];
    cuio_copydata(&uio, 0, &mut all);
    assert_eq!(all[..], data(20)[..]);
    // Starting exactly at an iovec boundary, and copying nothing.
    let mut one = [0u8; 1];
    cuio_copydata(&uio, 12, &mut one);
    assert_eq!(one[0], data(20)[12]);
    cuio_copydata(&uio, 20, &mut []);
}

#[test]
fn copyback_writes_through_the_iovecs() {
    let mut buf = vec![0u8; 20];
    let mut iov = Vec::new();
    let uio = cut(&mut buf, &[5, 5, 5, 5], &mut iov);
    cuio_copyback(&uio, 3, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
    drop(uio);
    let mut want = vec![0u8; 20];
    want[3..14].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
    assert_eq!(buf, want);
}

#[test]
fn getptr_finds_the_iovec_and_the_offset() {
    let mut buf = data(20);
    let mut iov = Vec::new();
    let uio = cut(&mut buf, &[4, 0, 6, 10], &mut iov);
    assert_eq!(cuio_getptr(&uio, 0), Some((0, 0)));
    assert_eq!(cuio_getptr(&uio, 3), Some((0, 3)));
    // The boundary belongs to the next non-empty iovec.
    assert_eq!(cuio_getptr(&uio, 4), Some((2, 0)));
    assert_eq!(cuio_getptr(&uio, 9), Some((2, 5)));
    assert_eq!(cuio_getptr(&uio, 19), Some((3, 9)));
    // The end of the data is the end of the last iovec; past it is nothing.
    assert_eq!(cuio_getptr(&uio, 20), Some((3, 10)));
    assert_eq!(cuio_getptr(&uio, 21), None);
    assert_eq!(cuio_getptr(&uio, -1), None);
}

#[test]
fn apply_walks_the_runs_and_stops_at_the_first_error() {
    let mut buf = data(20);
    let mut iov = Vec::new();
    let uio = cut(&mut buf, &[3, 0, 9, 8], &mut iov);
    let mut runs: Vec<Vec<u8>> = Vec::new();
    cuio_apply(&uio, 2, 14, |b| {
        runs.push(b.to_vec());
        Ok(())
    })
    .expect("apply");
    let d = data(20);
    // An empty iovec is a run of no bytes, as in the C.
    assert_eq!(
        runs,
        [
            d[2..3].to_vec(),
            Vec::new(),
            d[3..12].to_vec(),
            d[12..16].to_vec()
        ]
    );
    // The first error stops the walk and is returned.
    let mut n = 0;
    let r = cuio_apply(&uio, 0, 20, |_| {
        n += 1;
        if n == 2 { Err(Errno::EIO) } else { Ok(()) }
    });
    assert_eq!((r, n), (Err(Errno::EIO), 2));
    // An empty range calls nothing.
    cuio_apply(&uio, 5, 0, |_| Err(Errno::EIO)).expect("nothing to do");
}
