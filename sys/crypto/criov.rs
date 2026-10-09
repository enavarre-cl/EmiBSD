/* <LICENSES> */
/*
 * Copyright (c) 1999 Theo de Raadt
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 *
 * 1. Redistributions of source code must retain the above copyright
 *   notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *   notice, this list of conditions and the following disclaimer in the
 *   documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `cuio_*`: the crypto framework's view of a `struct uio` as a flat buffer of bytes (the
//! same four operations `m_copydata`, `m_copyback`, `m_getptr` and `m_apply` are for mbuf
//! chains).
//!
//! Upstream: sys/crypto/criov.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The data buffers are slices (`caddr_t cp` and `len`). The iovecs of the uio are kernel
//!   buffers whoever built the uio vouches for (`sys/uio.rs`): the functions read and write
//!   through `iov_base` with that as their contract, and refuse a `UIO_USERSPACE` uio.
//!   `cuio_copyback` takes the uio by shared reference: it writes the bytes the iovecs
//!   point to, not the uio.
//! - `cuio_getptr` returns `Option<(usize, i32)>` (the index of the iovec and the offset in
//!   it) for the C's index or -1 and `*off`. `cuio_apply`'s callback is a closure over the
//!   byte run (see `m_apply`) and the function returns `Result<(), Errno>`.
//! - The `panic`s of the C are the same panics.

use core::slice;

use crate::kassert;
use crate::kern::subr_prf::panic;
use crate::sys::errno::Errno;
use crate::sys::uio::{Iovec, Uio, UioSeg};

/// The bytes of the iovec `iov` from `off`, for `count` bytes.
fn iov_run(iov: &Iovec, off: usize, count: usize) -> &[u8] {
    // SAFETY: the uio holds kernel buffers (`uio_segflg` is checked by the callers) that
    // whoever built it vouches for; `off + count <= iov_len` by the callers' arithmetic.
    unsafe { slice::from_raw_parts(iov.iov_base.cast::<u8>().cast_const().add(off), count) }
}

/// The bytes of the iovec `iov` from `off`, for `count` bytes, writable.
#[allow(clippy::mut_from_ref)] // the bytes are behind `iov_base`, not in the `Iovec`
pub(super) fn iov_run_mut(iov: &Iovec, off: usize, count: usize) -> &mut [u8] {
    // SAFETY: as for `iov_run`; the framework is the only user of these bytes while the
    // request runs (`crypto_invoke` owns the request), so no other reference to them is live.
    unsafe { slice::from_raw_parts_mut(iov.iov_base.cast::<u8>().add(off), count) }
}

/// `cuio_copydata`: copies `cp.len()` bytes from `off` bytes into the uio into `cp`.
pub fn cuio_copydata(uio: &Uio<'_>, off: i32, cp: &mut [u8]) {
    kassert!(uio.uio_segflg == UioSeg::UIO_SYSSPACE);
    let mut off = off;
    let mut len = cp.len();
    let mut at = 0usize;
    let mut iov: &[Iovec] = &uio.uio_iov[..];

    if off < 0 {
        panic(format_args!("cuio_copydata: off {} < 0", off));
    }
    while off > 0 {
        let Some((first, rest)) = iov.split_first() else {
            panic(format_args!("iov_copydata: empty in skip"));
        };
        if (off as usize) < first.iov_len {
            break;
        }
        off -= first.iov_len as i32;
        iov = rest;
    }
    while len > 0 {
        let Some((first, rest)) = iov.split_first() else {
            panic(format_args!("cuio_copydata: empty"));
        };
        let count = (first.iov_len - off as usize).min(len);
        cp[at..at + count].copy_from_slice(iov_run(first, off as usize, count));
        len -= count;
        at += count;
        off = 0;
        iov = rest;
    }
}

/// `cuio_copyback`: copies `cp` into the uio, `off` bytes in.
pub fn cuio_copyback(uio: &Uio<'_>, off: i32, cp: &[u8]) {
    kassert!(uio.uio_segflg == UioSeg::UIO_SYSSPACE);
    let mut off = off;
    let mut len = cp.len();
    let mut at = 0usize;
    let mut iov: &[Iovec] = &uio.uio_iov[..];

    if off < 0 {
        panic(format_args!("cuio_copyback: off {} < 0", off));
    }
    while off > 0 {
        let Some((first, rest)) = iov.split_first() else {
            panic(format_args!("cuio_copyback: empty in skip"));
        };
        if (off as usize) < first.iov_len {
            break;
        }
        off -= first.iov_len as i32;
        iov = rest;
    }
    while len > 0 {
        let Some((first, rest)) = iov.split_first() else {
            panic(format_args!("uio_copyback: empty"));
        };
        let count = (first.iov_len - off as usize).min(len);
        iov_run_mut(first, off as usize, count).copy_from_slice(&cp[at..at + count]);
        len -= count;
        at += count;
        off = 0;
        iov = rest;
    }
}

/// `cuio_getptr`: the iovec (its index) and the offset in it of location `loc`; the end of
/// the last iovec when `loc` is the total length; `None` past it.
pub fn cuio_getptr(uio: &Uio<'_>, loc: i32) -> Option<(usize, i32)> {
    let mut loc = loc;
    let mut ind = 0usize;

    while loc >= 0 && ind < uio.uio_iovcnt() {
        let len = uio.uio_iov[ind].iov_len as i32;
        if len > loc {
            return Some((ind, loc));
        }
        loc -= len;
        ind += 1;
    }

    if ind > 0 && loc == 0 {
        ind -= 1;
        return Some((ind, uio.uio_iov[ind].iov_len as i32));
    }

    None
}

/// `cuio_apply`: applies `f` to the runs of the uio's bytes in `[off, off + len)`; the first
/// error stops the walk.
pub fn cuio_apply(
    uio: &Uio<'_>,
    off: i32,
    len: i32,
    mut f: impl FnMut(&[u8]) -> Result<(), Errno>,
) -> Result<(), Errno> {
    kassert!(uio.uio_segflg == UioSeg::UIO_SYSSPACE);
    let mut off = off;
    let mut len = len;

    if len < 0 {
        panic(format_args!("cuio_apply: len {} < 0", len));
    }
    if off < 0 {
        panic(format_args!("cuio_apply: off {} < 0", off));
    }

    let mut ind = 0usize;
    while off > 0 {
        if ind >= uio.uio_iovcnt() {
            panic(format_args!(
                "cuio_apply: ind {} >= uio_iovcnt {} for off",
                ind,
                uio.uio_iovcnt()
            ));
        }
        let uiolen = uio.uio_iov[ind].iov_len as i32;
        if off < uiolen {
            break;
        }
        off -= uiolen;
        ind += 1;
    }
    while len > 0 {
        if ind >= uio.uio_iovcnt() {
            panic(format_args!(
                "cuio_apply: ind {} >= uio_iovcnt {} for len",
                ind,
                uio.uio_iovcnt()
            ));
        }
        let iov = &uio.uio_iov[ind];
        let count = (iov.iov_len as i32 - off).min(len);

        f(iov_run(iov, off as usize, count as usize))?;

        len -= count;
        off = 0;
        ind += 1;
    }

    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Tests of the `cuio_*` functions over uios of several iovecs, including empty ones.

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
}
/* </TESTS> */
