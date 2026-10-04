use super::*;
use crate::sys::uio::{Iovec, UioRw, UioSeg};
use core::ffi::c_void;

fn uio_over<'a>(iov: &'a mut [Iovec], offset: Off) -> Uio<'a> {
    let resid = iov.iter().map(|v| v.iov_len).sum();
    Uio {
        uio_iov: iov,
        uio_offset: offset,
        uio_resid: resid,
        uio_segflg: UioSeg::UIO_USERSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: None,
    }
}

fn no_strategy(_bp: &'static Buf) {
    panic!("physio called the strategy routine");
}

#[test]
fn minphys_clamps_to_maxphys() {
    let bp = Buf::new();
    bp.b_bcount.set(3 * MAXPHYS as i64 + 1);
    minphys(&bp);
    assert_eq!(bp.b_bcount.get(), MAXPHYS as i64);

    bp.b_bcount.set(MAXPHYS as i64);
    minphys(&bp);
    assert_eq!(bp.b_bcount.get(), MAXPHYS as i64);

    bp.b_bcount.set(512);
    minphys(&bp);
    assert_eq!(bp.b_bcount.get(), 512);
}

#[test]
fn bcount_is_limited_to_long_max() {
    assert_eq!(physio_bcount(0), 0);
    assert_eq!(physio_bcount(8192), 8192);
    assert_eq!(physio_bcount(i64::MAX as usize), i64::MAX);
    assert_eq!(physio_bcount(usize::MAX), i64::MAX);
}

#[test]
fn advance_moves_the_iovec_the_offset_and_the_residual() {
    let mut a = [0u8; 4096];
    let mut b = [0u8; 1024];
    let mut iov = [
        Iovec {
            iov_base: a.as_mut_ptr().cast::<c_void>(),
            iov_len: a.len(),
        },
        Iovec {
            iov_base: b.as_mut_ptr().cast::<c_void>(),
            iov_len: b.len(),
        },
    ];
    let mut uio = uio_over(&mut iov, 2048);

    // A full MAXPHYS-bounded piece, then a short one (the end of the disk).
    physio_advance(&mut uio, 0, 3072);
    assert_eq!(uio.uio_iov[0].iov_len, 1024);
    assert_eq!(uio.uio_iov[0].iov_base as usize, a.as_ptr() as usize + 3072);
    assert_eq!(uio.uio_offset, 2048 + 3072);
    assert_eq!(uio.uio_resid, 5120 - 3072);

    physio_advance(&mut uio, 0, 1024);
    physio_advance(&mut uio, 1, 512);
    assert_eq!(uio.uio_iov[0].iov_len, 0);
    assert_eq!(uio.uio_iov[1].iov_len, 512);
    assert_eq!(uio.uio_iov[1].iov_base as usize, b.as_ptr() as usize + 512);
    assert_eq!(uio.uio_offset, 2048 + 4608);
    assert_eq!(uio.uio_resid, 512);
}

#[test]
fn misaligned_offsets_are_einval_before_any_io() {
    let mut a = [0u8; 512];
    for offset in [1, 511, 513, -1] {
        let mut iov = [Iovec {
            iov_base: a.as_mut_ptr().cast::<c_void>(),
            iov_len: a.len(),
        }];
        let mut uio = uio_over(&mut iov, offset);
        assert_eq!(
            physio(no_strategy, 0, B_READ, minphys, &mut uio),
            Err(Errno::EINVAL),
            "offset {offset}"
        );
        assert_eq!(uio.uio_resid, 512);
        assert_eq!(uio.uio_offset, offset);
    }
}
