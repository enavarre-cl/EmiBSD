/* <LICENSES> */
/*
 * Copyright (c) 1989, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * Rick Macklem at The University of Guelph.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)nfsm_subs.h	8.2 (Berkeley) 3/30/95
 */
/* </LICENSES> */

/* <CODE> */
//! `<nfs/nfsm_subs.h>`: the request/reply state of an NFS RPC (`struct nfsm_info`) and the
//! inline functions that dissect a reply or request mbuf chain: `nfsm_dissect`,
//! `nfsm_adv`, `nfsm_strsiz`, `nfsm_mtouio`, `nfsm_postop_attr`, `nfsm_strtom`, and the
//! server's `nfsd_*` versions over `struct nfsrv_descript`.
//!
//! Upstream: sys/nfs/nfsm_subs.h @ 3ce1f3f79392
//!
//! A dissection cursor is an mbuf (`nmi_md`, `nd_md`) and a position in its data
//! (`nmi_dpos`, `nd_dpos`, the C's `caddr_t`). The position stays a raw pointer, but nothing
//! dereferences it outside the primitives of this file and `nfs_subs.rs`, which check it lies
//! inside the mbuf's data ([`nfsm_avail`]) before every use. What the C hands back as a
//! `caddr_t`/`u_int32_t *` into the mbufs is a bounds-checked view instead: [`XdrIn`] for
//! bytes read (`nfsm_dissect`), [`XdrOut`] for bytes written (`nfsm_build`). A view borrows
//! the cursor it came from, so the chain cannot be freed or dissected further while it is
//! alive.
//!
//! ## Deviations
//! - `int *nmi_errorp` and the `int *errorp` of the `nfsd_*` functions are gone: every
//!   function returns `Result`, and on failure frees `nmi_mrep` (`nmi_mreq` for
//!   `nfsm_strtom`, `nd_mrep` for the `nfsd_*` ones) and clears it, as the C does before
//!   storing the error. The caller's `goto nfsmout` is `?` out of the body the C writes
//!   before `nfsmout:` (see `docs/C_TO_RUST.md`).
//! - `nfsm_dissect`/`nfsd_dissect` return an [`XdrIn`] of the `s` bytes, not a pointer;
//!   `nfsm_postop_attr` keeps the C's `int *attrflagp` (untouched when there is no reply).
//! - `struct nfsm_info` has a lifetime for `nmi_procp` (the vnode operations' `a_p`);
//!   `nmi_cred` is the operations' raw `a_cred`; `nmi_v3` is a `bool`.
//! - Lengths: `nfsm_dissect`/`nfsm_adv` sizes, `nfsm_strsiz`'s result and `nfsm_strtom`'s
//!   are `usize`; `nfsm_mtouio`'s `len` stays an `i32`, since a length `<= 0` is the C's
//!   no-op.
//! - A cursor with no mbuf (`nmi_md` NULL, which the C would dereference) fails with
//!   `EBADRPC`.
//! - `nfsm_postop_attr` is `NFSCLIENT` only (`feature = "nfsclient"`): it calls
//!   `nfs_loadattrcache`, which the C compiles only for the client.

use core::marker::PhantomData;
use core::ptr;

use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::m_freem;
use crate::machine::copy::AbiPod;
use crate::nfs::nfs::NfsrvDescript;
#[cfg(feature = "nfsclient")]
use crate::nfs::nfs_subs::nfs_loadattrcache;
use crate::nfs::nfs_subs::{nfs_adv, nfsm_disct, nfsm_mbuftouio, nfsm_strtombuf};
use crate::nfs::nfsproto::NFSX_UNSIGNED;
use crate::nfs::xdr_subs::{fxdr_hyper, fxdr_unsigned, txdr_hyper, xdr_bytes, xdr_get};
use crate::sys::errno::Errno;
use crate::sys::mbuf::{Mbuf, mtod};
use crate::sys::proc::Proc;
use crate::sys::ucred::Ucred;
use crate::sys::uio::Uio;
#[cfg(feature = "nfsclient")]
use crate::sys::vnode::Vnode;

/// `struct nfsm_info`: the state of one NFS RPC on the client: the request being built, the
/// reply and the dissection cursor into it.
pub struct NfsmInfo<'a> {
    /// `nmi_mreq`: the request.
    pub nmi_mreq: Option<&'static Mbuf>,
    /// `nmi_mrep`: the reply.
    pub nmi_mrep: Option<&'static Mbuf>,
    /// `nmi_procp`: XXX XXX XXX
    pub nmi_procp: Option<&'a Proc>,
    /// `nmi_cred`: XXX XXX XXX (a real credential: `nfs_request` puts it in the RPC header).
    pub nmi_cred: *const Ucred,
    /// `nmi_md`: setting up / tearing down: the dissection mbuf.
    pub nmi_md: Option<&'static Mbuf>,
    /// `nmi_dpos`: the dissection position, inside `nmi_md`'s data.
    pub nmi_dpos: *mut u8,
    /// `nmi_v3`: the RPC is NFS version 3.
    pub nmi_v3: bool,
}

impl NfsmInfo<'_> {
    /// An empty state (the C's stack `struct nfsm_info` before it is filled in).
    pub const fn new() -> Self {
        Self {
            nmi_mreq: None,
            nmi_mrep: None,
            nmi_procp: None,
            nmi_cred: ptr::null(),
            nmi_md: None,
            nmi_dpos: ptr::null_mut(),
            nmi_v3: false,
        }
    }

    /// `info.nmi_md = m; info.nmi_dpos = mtod(m, caddr_t)`: dissection starts at the
    /// beginning of `m` (a reply chain's first mbuf).
    pub fn dissect_from(&mut self, m: Option<&'static Mbuf>) {
        self.nmi_md = m;
        self.nmi_dpos = m.map_or(ptr::null_mut(), mtod::<u8>);
    }
}

impl Default for NfsmInfo<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// A bounds-checked read view of `len` contiguous bytes of an mbuf chain, what the C's
/// `nfsm_dissect` returns as a pointer. It borrows the cursor it came from.
#[derive(Clone, Copy)]
pub struct XdrIn<'a> {
    p: *const u8,
    len: usize,
    _cursor: PhantomData<&'a [u8]>,
}

impl<'a> XdrIn<'a> {
    /// A view of the `len` bytes at `p`.
    ///
    /// # Safety
    ///
    /// `p` is valid for reads of `len` initialised bytes (mbuf data) that nothing writes
    /// while the view, borrowing the cursor for `'a`, is alive.
    pub(crate) unsafe fn new(p: *const u8, len: usize) -> Self {
        Self {
            p,
            len,
            _cursor: PhantomData,
        }
    }

    /// The number of bytes in view.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the view is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The bytes in view.
    pub fn bytes(&self) -> &'a [u8] {
        if self.len == 0 {
            return &[];
        }
        // SAFETY: `new`'s contract: `len` readable, initialised bytes that nothing writes
        // during `'a`.
        unsafe { core::slice::from_raw_parts(self.p, self.len) }
    }

    /// `tl[i]`: the raw XDR word `i` (read unaligned); panics past the view.
    pub fn get(&self, i: usize) -> u32 {
        let b = &self.bytes()[4 * i..4 * i + 4];
        u32::from_ne_bytes([b[0], b[1], b[2], b[3]])
    }

    /// `fxdr_hyper(&tl[i])`: the 64-bit value of raw words `i` and `i + 1`.
    pub fn fxdr_hyper(&self, i: usize) -> u64 {
        fxdr_hyper([self.get(i), self.get(i + 1)])
    }

    /// `(T *)((caddr_t)tl + off)`: a wire structure copied out from byte `off`; bytes past
    /// the view (a version 2 reply in a structure sized for version 3) read as zero.
    pub fn read<T: AbiPod>(&self, off: usize) -> T {
        xdr_get(&self.bytes()[off..])
    }
}

/// A bounds-checked write view of `len` contiguous bytes at the end of an mbuf chain, what
/// the C's `nfsm_build` returns as a pointer. It borrows the build cursor; [`XdrOut::put`]
/// writes word after word like the C's `*tl++ = ...`.
pub struct XdrOut<'a> {
    p: *mut u8,
    len: usize,
    pos: usize,
    _cursor: PhantomData<&'a mut [u8]>,
}

impl XdrOut<'_> {
    /// A view of the `len` bytes at `p`.
    ///
    /// # Safety
    ///
    /// `p` is valid for writes of `len` bytes (mbuf storage) that nothing else touches while
    /// the view, borrowing the build cursor, is alive.
    pub(crate) unsafe fn new(p: *mut u8, len: usize) -> Self {
        Self {
            p,
            len,
            pos: 0,
            _cursor: PhantomData,
        }
    }

    /// The number of bytes in view.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the view is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Copies `b` to byte `off` of the view; panics past its end.
    pub fn set_bytes(&mut self, off: usize, b: &[u8]) {
        if off > self.len || b.len() > self.len - off {
            panic(format_args!(
                "nfsm_build: {} bytes at {} past a {}-byte view",
                b.len(),
                off,
                self.len
            ));
        }
        // SAFETY: `off + b.len() <= len` (checked above), so the destination is inside the
        // `len` writable bytes of `new`'s contract; `b` is another object.
        unsafe { ptr::copy_nonoverlapping(b.as_ptr(), self.p.add(off), b.len()) };
    }

    /// `tl[i] = w`: stores the raw XDR word `w` as word `i`.
    pub fn set(&mut self, i: usize, w: u32) {
        self.set_bytes(4 * i, &w.to_ne_bytes());
    }

    /// `*tl++ = w`: stores the raw XDR word `w` after the last one `put` stored.
    pub fn put(&mut self, w: u32) {
        self.set(self.pos, w);
        self.pos += 1;
    }

    /// `txdr_hyper(v, &tl[i])`: stores `v` as raw words `i` and `i + 1`.
    pub fn txdr_hyper(&mut self, i: usize, v: u64) {
        let w = txdr_hyper(v);
        self.set(i, w[0]);
        self.set(i + 1, w[1]);
    }

    /// `txdr_hyper(v, tl); tl += 2`.
    pub fn put_hyper(&mut self, v: u64) {
        self.txdr_hyper(self.pos, v);
        self.pos += 2;
    }

    /// Copies the wire structure `v` to byte `off`, as much of it as the view holds (a
    /// version 2 `nfs_fattr` is shorter than the structure).
    pub fn write<T: AbiPod>(&mut self, off: usize, v: &T) {
        let b = xdr_bytes(v);
        let n = b.len().min(self.len.saturating_sub(off));
        self.set_bytes(off, &b[..n]);
    }
}

/// The C's `avail`: the bytes of `md`'s data at and after `dpos`
/// (`mtod(md, caddr_t) + md->m_len - dpos`). Panics when `dpos` is not inside `md`'s data,
/// which would make every later access wild.
pub fn nfsm_avail(md: &Mbuf, dpos: *mut u8) -> usize {
    let start = mtod::<u8>(md).addr();
    let end = start + md.m_len().get() as usize;
    let d = dpos.addr();
    if d < start || d > end {
        panic(format_args!(
            "nfsm: position {:p} outside mbuf {:p}",
            dpos, md
        ));
    }
    end - d
}

/// The body shared by `nfsm_dissect` and `nfsd_dissect`, before the error path.
fn dissect<'b>(
    mdp: &'b mut Option<&'static Mbuf>,
    dposp: &'b mut *mut u8,
    s: usize,
) -> Result<XdrIn<'b>, Errno> {
    let Some(md) = *mdp else {
        return Err(Errno::EBADRPC);
    };
    if nfsm_avail(md, *dposp) >= s {
        let ret = *dposp;
        *dposp = ret.wrapping_add(s);
        // SAFETY: the `s` bytes at `ret` are `md`'s data (`nfsm_avail`), which the reply's
        // owner does not write while the view borrows its cursor.
        return Ok(unsafe { XdrIn::new(ret, s) });
    }
    nfsm_disct(mdp, dposp, s)
}

/// The body shared by `nfsm_adv` and `nfsd_adv`, before the error path.
fn adv(mdp: &mut Option<&'static Mbuf>, dposp: &mut *mut u8, s: usize) -> Result<(), Errno> {
    let Some(md) = *mdp else {
        return Err(Errno::EBADRPC);
    };
    if nfsm_avail(md, *dposp) >= s {
        *dposp = dposp.wrapping_add(s);
        return Ok(());
    }
    nfs_adv(mdp, dposp, s)
}

/// `nfsd_dissect(nfsd, s, errorp)`: the next `s` bytes of a request, made contiguous;
/// frees the request on failure.
pub fn nfsd_dissect(nfsd: &mut NfsrvDescript, s: usize) -> Result<XdrIn<'_>, Errno> {
    match dissect(&mut nfsd.nd_md, &mut nfsd.nd_dpos, s) {
        Ok(v) => Ok(v),
        Err(e) => {
            m_freem(nfsd.nd_mrep.take());
            Err(e)
        }
    }
}

/// `nfsm_dissect(infop, s)`: the next `s` bytes of a reply, made contiguous; frees the reply
/// on failure.
pub fn nfsm_dissect<'b>(infop: &'b mut NfsmInfo<'_>, s: usize) -> Result<XdrIn<'b>, Errno> {
    match dissect(&mut infop.nmi_md, &mut infop.nmi_dpos, s) {
        Ok(v) => Ok(v),
        Err(e) => {
            m_freem(infop.nmi_mrep.take());
            Err(e)
        }
    }
}

/// `nfsm_rndup(a)`: `a` rounded up to a whole XDR word.
pub const fn nfsm_rndup(a: usize) -> usize {
    (a + 3) & !0x3
}

/// `nfsd_adv(nfsd, s, errorp)`: skips `s` bytes of a request; frees it on failure.
pub fn nfsd_adv(nfsd: &mut NfsrvDescript, s: usize) -> Result<(), Errno> {
    adv(&mut nfsd.nd_md, &mut nfsd.nd_dpos, s).inspect_err(|_| {
        m_freem(nfsd.nd_mrep.take());
    })
}

/// `nfsm_adv(infop, s)`: skips `s` bytes of a reply; frees it on failure.
pub fn nfsm_adv(infop: &mut NfsmInfo<'_>, s: usize) -> Result<(), Errno> {
    adv(&mut infop.nmi_md, &mut infop.nmi_dpos, s).inspect_err(|_| {
        m_freem(infop.nmi_mrep.take());
    })
}

/// `nfsm_postop_attr(infop, vpp, attrflagp)`: a version 3 `post_op_attr`: the flag word
/// and, when it is set, the attributes loaded into the attribute cache of `*vpp` (which
/// `nfs_loadattrcache` may replace by an alias). Does nothing, not even set `*attrflagp`,
/// when the reply is already gone.
#[cfg(feature = "nfsclient")]
pub fn nfsm_postop_attr(
    infop: &mut NfsmInfo<'_>,
    vpp: &mut &'static Vnode,
    attrflagp: &mut i32,
) -> Result<(), Errno> {
    if infop.nmi_mrep.is_none() {
        return Ok(());
    }

    let mut ttvp = *vpp;
    let attrflag = fxdr_unsigned(nfsm_dissect(infop, NFSX_UNSIGNED)?.get(0)) as i32;
    if attrflag != 0 {
        if let Err(e) = nfs_loadattrcache(&mut ttvp, &mut infop.nmi_md, &mut infop.nmi_dpos, None) {
            m_freem(infop.nmi_mrep.take());
            return Err(e);
        }
        *vpp = ttvp;
    }
    *attrflagp = attrflag;
    Ok(())
}

/// `nfsd_strsiz(nfsd, lenp, maxlen, errorp)`: a request's string length word, checked to be
/// at most `maxlen` (`EBADRPC` otherwise, the request freed).
pub fn nfsd_strsiz(nfsd: &mut NfsrvDescript, maxlen: usize) -> Result<usize, Errno> {
    let len = fxdr_unsigned(nfsd_dissect(nfsd, NFSX_UNSIGNED)?.get(0)) as i32;
    if len < 0 || len as usize > maxlen {
        m_freem(nfsd.nd_mrep.take());
        return Err(Errno::EBADRPC);
    }
    Ok(len as usize)
}

/// `nfsm_strsiz(infop, lenp, maxlen)`: a reply's string length word, checked to be at most
/// `maxlen` (`EBADRPC` otherwise, the reply freed).
pub fn nfsm_strsiz(infop: &mut NfsmInfo<'_>, maxlen: usize) -> Result<usize, Errno> {
    let len = fxdr_unsigned(nfsm_dissect(infop, NFSX_UNSIGNED)?.get(0)) as i32;
    if len < 0 || len as usize > maxlen {
        m_freem(infop.nmi_mrep.take());
        return Err(Errno::EBADRPC);
    }
    Ok(len as usize)
}

/// `nfsd_mtouio(nfsd, uiop, len, errorp)`: copies `len` bytes of a request (and its XDR
/// padding) into `uiop`; nothing when `len <= 0`. Frees the request on failure.
pub fn nfsd_mtouio(nfsd: &mut NfsrvDescript, uiop: &mut Uio<'_>, len: i32) -> Result<(), Errno> {
    if len <= 0 {
        return Ok(());
    }
    nfsm_mbuftouio(&mut nfsd.nd_md, uiop, len as usize, &mut nfsd.nd_dpos).inspect_err(|_| {
        m_freem(nfsd.nd_mrep.take());
    })
}

/// `nfsm_mtouio(infop, uiop, len)`: copies `len` bytes of a reply (and its XDR padding) into
/// `uiop`; nothing when `len <= 0`. Frees the reply on failure.
pub fn nfsm_mtouio(infop: &mut NfsmInfo<'_>, uiop: &mut Uio<'_>, len: i32) -> Result<(), Errno> {
    if len <= 0 {
        return Ok(());
    }
    nfsm_mbuftouio(&mut infop.nmi_md, uiop, len as usize, &mut infop.nmi_dpos).inspect_err(|_| {
        m_freem(infop.nmi_mrep.take());
    })
}

/// `nfsm_strtom(infop, mb, str, len, maxlen)`: appends the XDR string `str` to the request
/// at `*mb`; `ENAMETOOLONG` (the request freed) when it is longer than `maxlen`.
pub fn nfsm_strtom(
    infop: &mut NfsmInfo<'_>,
    mb: &mut &'static Mbuf,
    str: &[u8],
    maxlen: usize,
) -> Result<(), Errno> {
    if str.len() > maxlen {
        m_freem(infop.nmi_mreq.take());
        return Err(Errno::ENAMETOOLONG);
    }
    nfsm_strtombuf(mb, str);
    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `<nfs/nfsm_subs.h>`: the reply and request dissectors, their error paths
    // (the chain freed and cleared, the error returned) and the views.

    use std::vec;

    use super::*;
    use crate::kern::uipc_mbuf::tests::setup;
    use crate::nfs::nfs_subs::nfsm_reqhead;
    use crate::nfs::nfs_subs::tests::{bytes, chain};
    use crate::nfs::nfsproto::{NFSX_V3FATTR, NfsFattr, Nfsv3Time};
    use crate::nfs::xdr_subs::txdr_unsigned;
    use crate::sys::uio::{Iovec, UioRw, UioSeg};

    /// The raw XDR words of `w`, as bytes.
    fn words(w: &[u32]) -> std::vec::Vec<u8> {
        w.iter()
            .flat_map(|x| txdr_unsigned(*x).to_ne_bytes())
            .collect()
    }

    #[test]
    fn dissect_reads_words_and_structures() {
        let _g = setup();
        let w = words(&[1, 2, 0x10, 0x20]);
        let head = chain(&[&w[..6], &w[6..]]);
        let mut info = NfsmInfo::new();
        info.nmi_mrep = Some(head);
        info.dissect_from(Some(head));
        let tl = nfsm_dissect(&mut info, 8).expect("two words");
        assert_eq!(fxdr_unsigned(tl.get(0)), 1);
        assert_eq!(fxdr_unsigned(tl.get(1)), 2);
        let t: Nfsv3Time = nfsm_dissect(&mut info, 8).expect("a time").read(0);
        assert_eq!(
            (fxdr_unsigned(t.nfsv3_sec), fxdr_unsigned(t.nfsv3_nsec)),
            (0x10, 0x20)
        );
        // A short view reads the rest of a structure as zero.
        let tl = nfsm_dissect(&mut info, 0).expect("nothing");
        let fa: NfsFattr = tl.read(0);
        assert_eq!(fa, NfsFattr::default());
        assert!(size_of::<NfsFattr>() == NFSX_V3FATTR);

        // Past the end: the reply is freed and cleared, the error returned.
        assert_eq!(nfsm_dissect(&mut info, 4).err(), Some(Errno::EBADRPC));
        assert!(info.nmi_mrep.is_none());
    }

    #[test]
    fn strsiz_and_mtouio() {
        let _g = setup();
        let mut w = words(&[5]);
        w.extend_from_slice(b"abcde\0\0\0");
        w.extend(words(&[300]));
        let head = chain(&[&w]);
        let mut info = NfsmInfo::new();
        info.nmi_mrep = Some(head);
        info.dissect_from(Some(head));
        let len = nfsm_strsiz(&mut info, 255).expect("a length");
        assert_eq!(len, 5);
        let mut buf = vec![0u8; 5];
        let mut iov = [Iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: 5,
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: 0,
            uio_resid: 5,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        };
        nfsm_mtouio(&mut info, &mut uio, len as i32).expect("the string");
        nfsm_mtouio(&mut info, &mut uio, -1).expect("a no-op");
        assert_eq!(&buf, b"abcde");
        assert_eq!(nfsm_strsiz(&mut info, 255).err(), Some(Errno::EBADRPC));
        assert!(info.nmi_mrep.is_none(), "too long: the reply is freed");
    }

    #[test]
    fn server_side_dissect_and_adv() {
        let _g = setup();
        let w = words(&[7, 8, 9]);
        let head = chain(&[&w[..2], &w[2..]]);
        let mut nd = NfsrvDescript::new();
        nd.nd_mrep = Some(head);
        nd.nd_md = Some(head);
        nd.nd_dpos = mtod::<u8>(head);
        nfsd_adv(&mut nd, 4).expect("skip a word");
        assert_eq!(
            fxdr_unsigned(nfsd_dissect(&mut nd, 4).expect("a word").get(0)),
            8
        );
        let tl = nfsd_dissect(&mut nd, 4).expect("a word");
        assert_eq!(fxdr_unsigned(tl.get(0)), 9);
        assert_eq!(nfsd_adv(&mut nd, 4).err(), Some(Errno::EBADRPC));
        assert!(nd.nd_mrep.is_none());
    }

    #[test]
    fn strtom_refuses_long_names() {
        let _g = setup();
        let mut info = NfsmInfo::new();
        let req = nfsm_reqhead(0);
        info.nmi_mreq = Some(req);
        let mut mb = req;
        nfsm_strtom(&mut info, &mut mb, b"ok", 2).expect("fits");
        assert_eq!(bytes(req), [0, 0, 0, 2, b'o', b'k', 0, 0]);
        assert_eq!(
            nfsm_strtom(&mut info, &mut mb, b"long", 3).err(),
            Some(Errno::ENAMETOOLONG)
        );
        assert!(info.nmi_mreq.is_none());
    }

    #[test]
    fn build_views_write_words_and_structures() {
        let _g = setup();
        let req = nfsm_reqhead(0);
        let mut mb = req;
        let mut tl = crate::nfs::nfs_subs::nfsm_build(&mut mb, 12);
        tl.put(txdr_unsigned(1));
        tl.write(
            4,
            &Nfsv3Time::from_words([txdr_unsigned(2), txdr_unsigned(3)]),
        );
        assert_eq!(bytes(req), [0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3]);
        crate::kern::uipc_mbuf::m_freem(req);
    }
}
/* </TESTS> */
