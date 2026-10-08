/*	$OpenBSD: uipc_mbuf2.c,v 1.50 2025/06/25 20:26:32 miod Exp $	*/
/*	$KAME: uipc_mbuf2.c,v 1.29 2001/02/14 13:42:10 itojun Exp $	*/
/*	$NetBSD: uipc_mbuf.c,v 1.40 1999/04/01 00:23:25 thorpej Exp $	*/
/* <LICENSES> */
/*
 * Copyright (C) 1999 WIDE Project.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the project nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE PROJECT AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE PROJECT OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */

/*
 * Copyright (c) 1982, 1986, 1988, 1991, 1993
 *	The Regents of the University of California.  All rights reserved.
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
 *	@(#)uipc_mbuf.c	8.4 (Berkeley) 2/14/95
 */
/* </LICENSES> */

/* <CODE> */
//! `m_pulldown` and the packet tags: `kern/uipc_mbuf2.c`.
//!
//! Upstream: sys/kern/uipc_mbuf2.c @ 3ce1f3f79392
//!
//! `m_pulldown` makes a range of a packet contiguous without touching what lies before it (the
//! KAME alternative to `m_pullup`). Packet tags are small records from `mtagpool` hung off the
//! packet header (`ph_tags`), with `ph_tagsset` as the summary of their types.
//!
//! Status: `ported` (M7b).
//!
//! ## Deviations
//! - Tags are `&MTag` borrowed from their mbuf; `m_tag_get` hands out a `&'static MTag` the
//!   caller attaches with `m_tag_prepend`. `m_tag_delete` is `unsafe`: the tag must be on the
//!   mbuf's list, which the C's `SLIST_REMOVE` assumes without checking.
//! - `m_pulldown`'s `offp` is `Option<&mut i32>`: `None` is the C's NULL, which also changes
//!   where the data may start.

use core::ptr::{self, NonNull};
use core::slice;

use crate::kassert;
use crate::kern::subr_pool::{pool_get, pool_put};
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{
    MTAGPOOL, m_adj, m_copydata, m_dup_pkthdr, m_free, m_freem, m_get, m_gethdr, m_getptr,
    m_leadingspace, m_trailingspace,
};
use crate::sys::errno::Errno;
use crate::sys::malloc::M_WAITOK;
use crate::sys::mbuf::{
    M_DONTWAIT, M_EXT, M_PKTHDR, MAXMCLBYTES, MHLEN, MLEN, MTag, MTagList, MbstatCounters, Mbuf,
    PACKET_TAG_MAXSIZE, m_readonly, mbstat_inc, mclgetl, mtod,
};
use crate::sys::pool::{PR_NOWAIT, PR_WAITOK};
use crate::sys::queue::SlistHead;

/// `m_pulldown`: ensure that `[off, off + len]` is contiguous on the mbuf chain `m`. Packet
/// chain before `off` is kept untouched. If `offp` is `None`, the target will start at
/// `<retval, 0>` on resulting chain. If `offp` is `Some`, the target will start at
/// `<retval, *offp>` on resulting chain.
///
/// On error return (`None` return value), original `m` will be freed.
///
/// XXX `m_trailingspace`/`m_leadingspace` on shared cluster (sharedcluster)
pub fn m_pulldown(
    m: &'static Mbuf,
    off: i32,
    len: i32,
    offp: Option<&mut i32>,
) -> Option<&'static Mbuf> {
    let has_offp = offp.is_some();

    let Some((n, off)) = m_getptr(m, off) else {
        m_freem(m);
        return None; // mbuf chain too short
    };

    let sharedcluster = m_readonly(n);

    let (n, off) = 'ok: {
        // the target data is on <n, off>. if we got enough data on the mbuf "n", we're done.
        if (off == 0 || has_offp) && len <= n.m_len().get() as i32 - off && !sharedcluster {
            break 'ok (n, off);
        }

        // when len <= n->m_len - off and off != 0, it is a special case. len bytes from
        // <n, off> sits in single mbuf, but the caller does not like the starting position
        // (off). chop the current mbuf into two pieces, set off to 0.
        if len <= n.m_len().get() as i32 - off {
            mbstat_inc(MbstatCounters::MbsPulldownAlloc as usize);
            let Some(o) = m_dup1(n, off, n.m_len().get() as i32 - off, M_DONTWAIT) else {
                m_freem(m);
                return None; // ENOBUFS
            };
            let mut mlast = o;
            while let Some(next) = mlast.m_next().get() {
                mlast = next;
            }
            n.m_len().set(off as u32);
            mlast.m_next().set(n.m_next().get());
            n.m_next().set(Some(o));
            break 'ok (o, 0);
        }

        // we need to take hlen from <n, off> and tlen from <n->m_next, 0>, and construct
        // contiguous mbuf with m_len == len. note that hlen + tlen == len, and tlen > 0.
        let hlen = n.m_len().get() as i32 - off;
        let tlen = len - hlen;

        // ensure that we have enough trailing data on mbuf chain. if not, we can do nothing
        // about the chain.
        let mut olen = 0i32;
        let mut o = n.m_next().get();
        while let Some(oo) = o {
            olen += oo.m_len().get() as i32;
            o = oo.m_next().get();
        }
        if hlen + olen < len {
            m_freem(m);
            return None; // mbuf chain too short
        }
        // tlen > 0 bytes follow n, so it has a successor.
        let Some(next) = n.m_next().get() else {
            m_freem(m);
            return None;
        };

        // easy cases first. we need to use m_copydata() to get data from <n->m_next, 0>.
        if (off == 0 || has_offp) && m_trailingspace(n) >= tlen && !sharedcluster {
            mbstat_inc(MbstatCounters::MbsPulldownCopy as usize);
            // SAFETY: `n` has `tlen` bytes of trailing space (checked above).
            m_copydata(next, 0, unsafe {
                slice::from_raw_parts_mut(
                    mtod::<u8>(n).add(n.m_len().get() as usize),
                    tlen as usize,
                )
            });
            n.m_len().set(n.m_len().get() + tlen as u32);
            m_adj(next, tlen);
            break 'ok (n, off);
        }
        if (off == 0 || has_offp)
            && m_leadingspace(next) >= hlen
            && !sharedcluster
            && next.m_len().get() as i32 >= tlen
        {
            next.m_data()
                .set(next.m_data().get().wrapping_sub(hlen as usize));
            next.m_len().set(next.m_len().get() + hlen as u32);
            mbstat_inc(MbstatCounters::MbsPulldownCopy as usize);
            // SAFETY: `next` had `hlen` bytes of leading space, now its first bytes; the
            // source is `n`'s last `hlen` bytes, in another mbuf.
            unsafe {
                ptr::copy_nonoverlapping(
                    mtod::<u8>(n).add(off as usize),
                    mtod::<u8>(next),
                    hlen as usize,
                );
            }
            n.m_len().set(n.m_len().get() - hlen as u32);
            break 'ok (next, 0);
        }

        // now, we need to do the hard way. don't m_copym as there's no room on both ends.
        if len > MAXMCLBYTES as i32 {
            m_freem(m);
            return None;
        }
        mbstat_inc(MbstatCounters::MbsPulldownAlloc as usize);
        let mut o = m_get(M_DONTWAIT, i32::from(m.m_type().get()));
        if let Some(oo) = o
            && len > MLEN as i32
        {
            let _ = mclgetl(oo, M_DONTWAIT, len as u32);
            if oo.m_flags().get() & M_EXT == 0 {
                m_free(oo);
                o = None;
            }
        }
        let Some(o) = o else {
            m_freem(m);
            return None; // ENOBUFS
        };
        // get hlen from <n, off> into <o, 0>
        o.m_len().set(hlen as u32);
        // SAFETY: `o` holds `len` bytes (MLEN or the cluster); `hlen` bytes from `off` are
        // `n`'s data.
        unsafe {
            ptr::copy_nonoverlapping(
                mtod::<u8>(n).add(off as usize),
                mtod::<u8>(o),
                hlen as usize,
            );
        }
        n.m_len().set(n.m_len().get() - hlen as u32);
        // get tlen from <n->m_next, 0> into <o, hlen>
        // SAFETY: `o` holds `len = hlen + tlen` bytes.
        m_copydata(next, 0, unsafe {
            slice::from_raw_parts_mut(mtod::<u8>(o).add(hlen as usize), tlen as usize)
        });
        o.m_len().set(o.m_len().get() + tlen as u32);
        m_adj(next, tlen);
        o.m_next().set(Some(next));
        n.m_next().set(Some(o));
        (o, 0)
    };

    // ok:
    kassert!(n.m_len().get() as i32 >= off + len);
    if let Some(offp) = offp {
        *offp = off;
    }
    Some(n)
}

/// `m_dup1`: a copy of `len` bytes of `m` from `off` in one mbuf (or cluster); can't call it
/// `m_dup()`, as freebsd\[34\] uses `m_dup()` with different arg.
fn m_dup1(m: &Mbuf, off: i32, len: i32, wait: i32) -> Option<&'static Mbuf> {
    if len > MAXMCLBYTES as i32 {
        return None;
    }
    let (mut n, l) = if off == 0 && m.m_flags().get() & M_PKTHDR != 0 {
        let n = m_gethdr(wait, i32::from(m.m_type().get()))?;
        if m_dup_pkthdr(n, m, wait).is_err() {
            m_free(n);
            return None;
        }
        (Some(n), MHLEN as i32)
    } else {
        (m_get(wait, i32::from(m.m_type().get())), MLEN as i32)
    };
    if let Some(nn) = n
        && len > l
    {
        let _ = mclgetl(nn, wait, len as u32);
        if nn.m_flags().get() & M_EXT == 0 {
            m_free(nn);
            n = None;
        }
    }
    let n = n?;

    // SAFETY: `n` holds `len` bytes at `m_data` (its own area or the cluster).
    m_copydata(m, off, unsafe {
        slice::from_raw_parts_mut(mtod::<u8>(n), len as usize)
    });
    n.m_len().set(len as u32);

    Some(n)
}

/// `m_tag_get`: get a packet tag structure along with specified data following.
pub fn m_tag_get(type_: u16, len: i32, wait: i32) -> Option<&'static MTag> {
    if len < 0 {
        return None;
    }
    if len > PACKET_TAG_MAXSIZE as i32 {
        panic(format_args!(
            "requested tag size for pool {:#x} is too big",
            type_
        ));
    }
    let t = pool_get(
        &MTAGPOOL,
        if wait == M_WAITOK {
            PR_WAITOK
        } else {
            PR_NOWAIT
        },
    )?;
    let t = t.cast::<MTag>().as_ptr();
    // SAFETY: an `mtagpool` item is `sizeof(struct m_tag) + PACKET_TAG_MAXSIZE` bytes with the
    // pool's alignment; the structure is written whole before the reference is made, and the
    // item is the tag's until `m_tag_delete` returns it.
    unsafe {
        t.write(MTag::new(type_, len as u16));
        Some(&*t)
    }
}

/// `m_tag_prepend`: prepend a packet tag.
pub fn m_tag_prepend(m: &Mbuf, t: &'static MTag) {
    // SAFETY: a tag from `m_tag_get` is on no list, and stays valid until `m_tag_delete` or
    // `m_tag_delete_chain` unlinks and frees it.
    unsafe { m.m_pkthdr().ph_tags.insert_head(t) };
    m.m_pkthdr()
        .ph_tagsset
        .set(m.m_pkthdr().ph_tagsset.get() | t.m_tag_id.get());
}

/// `m_tag_delete`: unlink and free a packet tag.
///
/// # Safety
///
/// `t` is on `m`'s tag list; it is gone when this returns.
pub unsafe fn m_tag_delete(m: &Mbuf, t: &MTag) {
    let mut ph_tagsset = 0u16;

    // SAFETY: the caller's guarantee.
    unsafe { m.m_pkthdr().ph_tags.remove(t) };
    pool_put(&MTAGPOOL, NonNull::from(t).cast::<u8>());

    for p in m.m_pkthdr().ph_tags.iter() {
        ph_tagsset |= p.m_tag_id.get();
    }
    m.m_pkthdr().ph_tagsset.set(ph_tagsset);
}

/// `m_tag_delete_chain`: unlink and free a packet tag chain.
pub fn m_tag_delete_chain(m: &Mbuf) {
    while let Some(p) = m.m_pkthdr().ph_tags.first() {
        let p = NonNull::from(p);
        // SAFETY: the list is not empty.
        unsafe { m.m_pkthdr().ph_tags.remove_head() };
        pool_put(&MTAGPOOL, p.cast::<u8>());
    }
    m.m_pkthdr().ph_tagsset.set(0);
}

/// `m_tag_find`: find a tag, starting from a given position.
pub fn m_tag_find<'a>(m: &'a Mbuf, type_: u16, t: Option<&'a MTag>) -> Option<&'a MTag> {
    if m.m_pkthdr().ph_tagsset.get() & type_ == 0 {
        return None;
    }

    let mut p = match t {
        None => m.m_pkthdr().ph_tags.first(),
        Some(t) => SlistHead::<MTagList>::next(t),
    };
    while let Some(pp) = p {
        if pp.m_tag_id.get() == type_ {
            return Some(pp);
        }
        p = SlistHead::<MTagList>::next(pp);
    }
    None
}

/// `m_tag_copy`: copy a single tag.
pub fn m_tag_copy(t: &MTag, wait: i32) -> Option<&'static MTag> {
    let p = m_tag_get(t.m_tag_id.get(), i32::from(t.m_tag_len.get()), wait)?;
    // SAFETY: both items have `PACKET_TAG_MAXSIZE` bytes after the structure, and
    // `m_tag_len` is at most that (`m_tag_get` checks it). Copy the data.
    unsafe {
        ptr::copy_nonoverlapping(
            t.data().cast_const(),
            p.data(),
            usize::from(t.m_tag_len.get()),
        );
    }
    Some(p)
}

/// `m_tag_copy_chain`: copy two tag chains. The destination mbuf (`to`) loses any attached
/// tags even if the operation fails. This should not be a problem, as `m_tag_copy_chain()` is
/// typically called with a newly-allocated destination mbuf.
pub fn m_tag_copy_chain(to: &Mbuf, from: &Mbuf, wait: i32) -> Result<(), Errno> {
    let mut tprev: Option<&MTag> = None;

    m_tag_delete_chain(to);
    for p in from.m_pkthdr().ph_tags.iter() {
        let Some(t) = m_tag_copy(p, wait) else {
            m_tag_delete_chain(to);
            return Err(Errno::ENOBUFS);
        };
        match tprev {
            // SAFETY: `t` is a fresh tag, on no list; it lives until deleted from `to`.
            None => unsafe { to.m_pkthdr().ph_tags.insert_head(t) },
            // SAFETY: as above; `tprev` is on `to`'s list.
            Some(tprev) => unsafe { SlistHead::<MTagList>::insert_after(tprev, t) },
        }
        tprev = Some(t);
        to.m_pkthdr()
            .ph_tagsset
            .set(to.m_pkthdr().ph_tagsset.get() | t.m_tag_id.get());
    }
    Ok(())
}

/// `m_tag_first`: get first tag in chain.
pub fn m_tag_first(m: &Mbuf) -> Option<&MTag> {
    m.m_pkthdr().ph_tags.first()
}

/// `m_tag_next`: get next tag in chain.
pub fn m_tag_next<'a>(_m: &'a Mbuf, t: &'a MTag) -> Option<&'a MTag> {
    SlistHead::<MTagList>::next(t)
}
/* </CODE> */
