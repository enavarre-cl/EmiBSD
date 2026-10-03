//! Host tests for `kern_subr.c`: the hash table sizes, `uiomove` in both directions and both
//! segments across several iovecs, and `ureadc`. The host double's `copyin`/`copyout` copy
//! within its one address space, so "user" buffers are ordinary arrays.

use core::ffi::c_void;
use std::{assert, assert_eq, vec};

use super::*;
use crate::sys::uio::Iovec;

#[test]
fn sizes_round_up_to_powers_of_two() {
    assert_eq!(hashsize(1), 1);
    assert_eq!(hashsize(2), 2);
    assert_eq!(hashsize(3), 4);
    assert_eq!(hashsize(64), 64);
    assert_eq!(hashsize(65), 128);
}

fn iov(buf: &mut [u8]) -> Iovec {
    Iovec {
        iov_base: buf.as_mut_ptr().cast::<c_void>(),
        iov_len: buf.len(),
    }
}

fn uio<'a>(iovs: &'a mut [Iovec], seg: UioSeg, rw: UioRw) -> Uio<'a> {
    let resid = iovs.iter().map(|v| v.iov_len).sum();
    Uio {
        uio_iov: iovs,
        uio_offset: 100,
        uio_resid: resid,
        uio_segflg: seg,
        uio_rw: rw,
        uio_procp: None,
    }
}

#[test]
fn uiomove_reads_across_iovecs_and_skips_empty_ones() {
    let (mut a, mut b, mut c) = ([0u8; 3], [0u8; 0], [0u8; 5]);
    let mut iovs = [iov(&mut a), iov(&mut b), iov(&mut c)];
    let mut u = uio(&mut iovs, UioSeg::UIO_USERSPACE, UioRw::UIO_READ);

    let mut src = *b"abcdef";
    assert_eq!(uiomove(&mut src, &mut u), Ok(()));
    assert_eq!(u.uio_resid, 2);
    assert_eq!(u.uio_offset, 106);
    assert_eq!(u.uio_iovcnt(), 1);
    assert_eq!(u.uio_iov[0].iov_len, 2);

    // n is bounded by uio_resid.
    let mut more = *b"ghijkl";
    assert_eq!(uiomove(&mut more, &mut u), Ok(()));
    assert_eq!(u.uio_resid, 0);
    assert_eq!(uiomove(&mut more, &mut u), Ok(()));
    drop(u);
    assert_eq!(&a, b"abc");
    assert_eq!(&c, b"defgh");
}

#[test]
fn uiomove_writes_from_kernel_space() {
    let mut src = *b"hello, world";
    let (h, w) = src.split_at_mut(7);
    let mut iovs = [iov(h), iov(w)];
    let mut u = uio(&mut iovs, UioSeg::UIO_SYSSPACE, UioRw::UIO_WRITE);

    let mut dst = vec![0u8; 12];
    assert_eq!(uiomove(&mut dst[..10], &mut u), Ok(()));
    assert_eq!(&dst[..10], b"hello, wor");
    assert_eq!(u.uio_resid, 2);
    assert_eq!(uiomove(&mut dst[10..], &mut u), Ok(()));
    assert_eq!(&dst, b"hello, world");
    assert_eq!(u.uio_offset, 112);
}

#[test]
fn uiomove_reports_a_fault() {
    let mut iovs = [Iovec {
        iov_base: core::ptr::null_mut(),
        iov_len: 4,
    }];
    let mut u = uio(&mut iovs, UioSeg::UIO_USERSPACE, UioRw::UIO_READ);
    assert_eq!(uiomove(&mut [1, 2, 3, 4], &mut u), Err(Errno::EFAULT));
    assert_eq!(u.uio_resid, 4);
}

#[test]
fn ureadc_fills_one_character_at_a_time() {
    let (mut a, mut b) = ([0u8; 0], [0u8; 2]);
    for seg in [UioSeg::UIO_USERSPACE, UioSeg::UIO_SYSSPACE] {
        let mut iovs = [iov(&mut a), iov(&mut b)];
        let mut u = uio(&mut iovs, seg, UioRw::UIO_READ);
        assert_eq!(ureadc(i32::from(b'x'), &mut u), Ok(()));
        assert_eq!(ureadc(i32::from(b'y'), &mut u), Ok(()));
        assert_eq!(u.uio_resid, 0);
        assert_eq!(u.uio_offset, 102);
        #[cfg(not(feature = "diagnostic"))]
        assert_eq!(ureadc(i32::from(b'z'), &mut u), Err(Errno::EINVAL));
        drop(u);
        assert!(b == *b"xy");
        b = [0; 2];
    }
}
